//! 类型族解析（阶段 D；`M2-design.md` §5.2 / §5.4）。
//!
//! 对齐官方 `parser.go` 的 `type_`（`:1322`）/ `typeOrNil`（`:1352`）/
//! `typeInstance`（`:1441`）/ `funcType`（`:1462`）/ `arrayType`（`:1493`）/
//! `sliceType`（`:1519`）/ `chanElem`（`:1526`）/ `structType`（`:1542`）/
//! `interfaceType`（`:1561`）/ `funcResult`（`:1587`）/ `addField`（`:1607`）/
//! `fieldDecl`（`:1629`）/ `arrayOrTArgs`（`:1711`）/ `methodDecl`（`:1760`）/
//! `embeddedElem`（`:1854`）/ `embeddedTerm`（`:1879`）/ `paramDeclOrNil`（`:1904`）/
//! `paramList`（`:2007`）。
//!
//! # 类型与表达式同族（`G18`）
//!
//! `type_()` 返回 [`Expr`] 而不是单独的 `Type` —— parser 阶段分不清
//! `T`（类型名还是变量名）。官方如此，我们照做（`rgoc_ast::expr` 的模块文档）。
//!
//! # `typeOrNil` 的 `None` 是**真语义**，不是"还没写"
//!
//! 它表示「**这里没有类型**」—— 驱动 `constDecl` 走 `gotAssign()` 那条分支
//! （`parser.go:598-601`）。把"还没实现"也归到 `None`，会让 `const x []int = …`
//! **静默**走上"无类型"分支，树形看起来完全合理（`AGENTS.md` 记的「漏了不会报错」形状）。
//! 本模块已无骨架臂：每个分支都是官方对应的产生式。
//!
//! # 两处「官方的临时 `nil`」怎么在 Rust 里表达
//!
//! 官方 `Field.Type` / `IndexExpr.X` 都是**接口**，可以在构造中途为 `nil`：
//!
//! | 官方 | 何时为空 | 这里的处理 |
//! |---|---|---|
//! | `paramList` 手里的 `Field.Type` | `func(a)` 的 `a` 先只是"名字" | [`PartialField`]（**局部**类型，出口折成 `Field`） |
//! | `arrayOrTArgs` 返回的 `IndexExpr.X` | 由调用方填那个名字 | [`ArrayOrTArgs::Index`]（把 `X` 留成参数） |
//!
//! 都不拿 `Expr::Bad` 当哨兵 —— 哨兵会被 `Type != nil` 这类判据算成"有类型"。

use rgoc_ast::{
    ArrayType, BadExpr, ChanDir, ChanType, DotsType, Expr, Field, FuncType, IndexExpr,
    InterfaceType, ListExpr, MapType, Name, Op, Operation, SliceType, StructType,
};
use rgoc_lex::diag::DiagCode;
use rgoc_lex::pos::Pos;
use rgoc_lex::token::{Delim, Keyword, OpForm, Operator, TokenKind};

use crate::expr::unary_node;
use crate::parser::Parser;
use crate::recovery::{
    ARROW, CHAN, COMMA, ELLIPSIS, EMBEDDED_TERM_FOLLOW, INTERFACE, LBRACE, LBRACK, LPAREN, RBRACE,
    RBRACK, RPAREN, SEMI, STRUCT, TYPE_RECOVER_FOLLOW,
};

/// 官方给「类型位置的函数类型」这个上下文取的名字（`funcType("function type")`）。
///
/// 它唯一的作用是报错文案：`context != ""` 时类型参数列表被拒
/// （官方 `parser.go:1474`）。**空串有明确含义**（顶层 `func` 声明与 `typeDecl` 允许
/// 类型参数），所以不能用 `Option` 顺手"简化"。
const FUNC_TYPE_CONTEXT: &str = "function type";

impl Parser {
    /// 官方 `type_`（`parser.go:1322`）：`typeOrNil` 的报错版。
    ///
    /// **返回 `Expr` 而不是 `Type`** —— 类型与表达式同族（`G18`）。
    pub(crate) fn type_(&mut self) -> Expr {
        match self.type_or_nil() {
            Some(t) => t,
            None => {
                // 官方：**先**造 `badExpr`（用当前位置）**再**报错、再同步。
                let x = self.bad_expr();
                let pos = self.tok.pos;
                self.syntax_error_at(pos, DiagCode::SynUnexpectedToken, "expected type");
                self.advance(TYPE_RECOVER_FOLLOW);
                x
            }
        }
    }

    /// 官方 `typeOrNil`（`parser.go:1352`）。
    ///
    /// ```text
    /// Type     = TypeName | TypeLit | "(" Type ")" .
    /// TypeName = identifier | QualifiedIdent .
    /// TypeLit  = ArrayType | StructType | PointerType | FunctionType | InterfaceType |
    ///            SliceType | MapType | Channel_Type .
    /// ```
    ///
    /// # 为什么先 `clone` 判别键
    ///
    /// `match &self.tok.kind` 会把 `self` 借到 `match` 结束，而每个臂体都要
    /// `&mut self`。与 `stmt.rs` 的同一处理（那条 Ruling 的理由：臂体与官方 `switch`
    /// **逐条同形**比省一次克隆重要）。
    /// （深度计数点，`T-A-96`）类型的**全部**递归边都经过这里：`type_` 委托
    /// 给它，直接调用方（`arrayOrTArgs` 的元素、`paramList` 等）也在这里。
    /// 守 `type_` 会漏掉后者，所以守本函数。
    pub(crate) fn type_or_nil(&mut self) -> Option<Expr> {
        if !self.depth_enter() {
            return None;
        }
        let t = self.type_or_nil_inner();
        self.depth_leave();
        t
    }

    pub(crate) fn type_or_nil_inner(&mut self) -> Option<Expr> {
        let pos = self.tok.pos;
        match self.tok.kind.clone() {
            // 官方 `case _Star:` —— `*T`。官方造的是 `Operation{Op: Mul, X: ...}`，
            // 也就是**一元 `*`**（`newIndirect`）。
            TokenKind::Op {
                form: OpForm::Plain,
                op: Operator::Mul,
                ..
            } => {
                self.next();
                Some(unary_node(Op::Mul, self.type_(), pos))
            }

            // 官方 `case _Arrow:` —— `<-chan E`（只收通道）。
            // ⚠️ `<-x`（接收运算）**不走这里** —— 那条在 `unaryExpr` 的 `_Arrow` 分支里。
            TokenKind::Op {
                form: OpForm::Plain,
                op: Operator::Arrow,
                ..
            } => {
                self.next();
                self.want(&CHAN);
                let elem = self.chan_elem();
                Some(Expr::ChanType(ChanType {
                    dir: Some(ChanDir::RecvOnly),
                    elem: Box::new(elem),
                    pos,
                }))
            }

            // 官方 `case _Func:` —— 函数类型（类型参数被丢弃，官方 `_, t :=`）。
            TokenKind::Keyword(Keyword::Func) => {
                self.next();
                let (_, t) = self.func_type(FUNC_TYPE_CONTEXT);
                Some(Expr::FuncType(t))
            }

            // 官方 `case _Lbrack:` —— `[]T` 或 `[n]T` / `[...]T`（`[` 已消费）。
            TokenKind::Delim(Delim::LBrack) => {
                self.next();
                if self.got(&RBRACK) {
                    Some(self.slice_type(pos))
                } else {
                    Some(self.array_type(pos, None))
                }
            }

            // 官方 `case _Chan:` —— `chan T` / `chan<- T`。
            TokenKind::Keyword(Keyword::Chan) => {
                self.next();
                let dir = if self.got(&ARROW) {
                    Some(ChanDir::SendOnly)
                } else {
                    None
                };
                let elem = self.chan_elem();
                Some(Expr::ChanType(ChanType {
                    dir,
                    elem: Box::new(elem),
                    pos,
                }))
            }

            // 官方 `case _Map:` —— `map[K]V`。
            TokenKind::Keyword(Keyword::Map) => {
                self.next();
                self.want(&LBRACK);
                let key = self.type_();
                self.want(&RBRACK);
                let value = self.type_();
                Some(Expr::MapType(MapType {
                    key: Box::new(key),
                    value: Box::new(value),
                    pos,
                }))
            }

            TokenKind::Keyword(Keyword::Struct) => Some(Expr::StructType(self.struct_type())),
            TokenKind::Keyword(Keyword::Interface) => {
                Some(Expr::InterfaceType(self.interface_type()))
            }

            // 官方 `case _Name:` —— 类型名 / 限定名 / **类型实例化**（`T[int]`）。
            TokenKind::Ident(_) => Some(self.qualified_name(None)),

            // 官方 `case _Lparen:` —— `( Type )`，且 `keep_parens` 是**写死的 `false`**
            // ⇒ 括号**不进树**（官方注释：parser 不留无用的括号）。
            TokenKind::Delim(Delim::LParen) => {
                self.next();
                let t = self.type_();
                self.want(&RPAREN);
                Some(t)
            }

            // 其余词元**确实**不是类型的开头（`=` `:=` `;` `)` `,` 字面量 …）。
            _ => None,
        }
    }

    /// 官方 `typeInstance`（`parser.go:1441`）：`T[A, B]`。
    ///
    /// 由 [`Parser::qualified_name`] 尾部调用 —— 名字后面跟 `[` 就是类型实例化。
    pub(crate) fn type_instance(&mut self, typ: Expr) -> Expr {
        let pos = self.tok.pos;
        self.want(&LBRACK);
        let index = if self.at(&TokenKind::Delim(Delim::RBrack)) {
            // 官方：`T[]` 非法 —— 报一条、造 `badExpr`，**照常产出节点**。
            let p = self.tok.pos;
            self.syntax_error_at(
                p,
                DiagCode::SynUnexpectedToken,
                "expected type argument list",
            );
            self.bad_expr()
        } else {
            self.type_list(true).0
        };
        self.want(&RBRACK);
        Expr::Index(IndexExpr {
            x: Box::new(typ),
            index: Box::new(index),
            pos,
        })
    }

    /// 官方 `funcType`（`parser.go:1462`）：`func [T params] (params) results`。
    ///
    /// 返回 `(类型参数表, FuncType)` —— 官方是
    /// `funcType(context string) ([]*Field, *FuncType)`，类型参数**返回给调用方**，
    /// `FuncType` 自己没有这个字段（`rgoc_ast::FuncType` 的文档）。
    ///
    /// `context != ""` 时**拒绝**类型参数（报一条，但照常解析）。
    pub(crate) fn func_type(&mut self, context: &str) -> (Vec<Field>, FuncType) {
        let pos = self.tok.pos;
        let mut tparams: Vec<Field> = Vec::new();

        if self.got(&LBRACK) {
            if !context.is_empty() {
                // 官方 `syntaxErrorAt(typ.pos, context+" must have no type parameters")`。
                self.syntax_error_at(
                    pos,
                    DiagCode::SynUnexpectedToken,
                    &format!("{context} must have no type parameters"),
                );
            }
            if self.at(&TokenKind::Delim(Delim::RBrack)) {
                let p = self.tok.pos;
                self.syntax_error_at(p, DiagCode::SynUnexpectedToken, "empty type parameter list");
                self.next();
            } else {
                // `requireNames = true`（类型参数**必须**带名字/约束）。
                tparams = self.param_list(None, None, RBRACK.clone(), true, false);
            }
        }

        self.want(&LPAREN);
        let params = self.param_list(None, None, RPAREN.clone(), false, true);
        let results = self.func_result();

        (
            tparams,
            FuncType {
                params,
                results,
                pos,
            },
        )
    }

    /// 官方 `arrayType`（`parser.go:1493`）：`[` 已消费，`pos` 是 `[` 的位置。
    ///
    /// `len` 非 `None` 时是**调用方已经解析好的长度**（`fieldDecl` / `typeDecl` 那条路）。
    pub(crate) fn array_type(&mut self, pos: Pos, len: Option<Expr>) -> Expr {
        let len = match len {
            Some(l) => Some(l),
            None => {
                if self.got(&ELLIPSIS) {
                    // `[...]T` —— 官方让 `len` **保持 nil**。
                    None
                } else {
                    // ⚠️ 官方在这里显式 `p.xnest++`：数组长度是**表达式**，
                    // 所以 `[f(x)]int` 里的 `(` 是调用而不是参数表。
                    self.xnest += 1;
                    let e = self.expr();
                    self.xnest -= 1;
                    Some(e)
                }
            }
        };

        if self.at(&TokenKind::Delim(Delim::Comma)) {
            // 官方：类型参数表允许尾逗号，数组长度**不允许** ——
            // 「接受但抱怨」，兼顾更好的错误恢复。
            let p = self.tok.pos;
            self.syntax_error_at(
                p,
                DiagCode::SynUnexpectedToken,
                "unexpected comma; expected ]",
            );
            self.next();
        }

        self.want(&RBRACK);
        let elem = self.type_();
        Expr::ArrayType(ArrayType {
            len: len.map(Box::new),
            elem: Box::new(elem),
            pos,
        })
    }

    /// 官方 `sliceType`（`parser.go:1519`）：`[` 与 `]` 都已消费，`pos` 是 `[` 的位置。
    pub(crate) fn slice_type(&mut self, pos: Pos) -> Expr {
        let elem = self.type_();
        Expr::SliceType(SliceType {
            elem: Box::new(elem),
            pos,
        })
    }

    /// 官方 `chanElem`（`parser.go:1526`）：通道元素类型。
    ///
    /// ⚠️ 缺元素类型时官方**不** `advance`（注释：`assume element type is simply absent`）——
    /// 因为 `chan` 后面那个词元可能正是调用方要接着解析的东西。
    pub(crate) fn chan_elem(&mut self) -> Expr {
        match self.type_or_nil() {
            Some(t) => t,
            None => {
                let x = self.bad_expr();
                let p = self.tok.pos;
                self.syntax_error_at(
                    p,
                    DiagCode::SynUnexpectedToken,
                    "missing channel element type",
                );
                x
            }
        }
    }

    /// 官方 `structType`（`parser.go:1542`）：`StructType = "struct" "{" { FieldDecl ";" } "}"`。
    pub(crate) fn struct_type(&mut self) -> StructType {
        let pos = self.tok.pos;
        self.want(&STRUCT);
        self.want(&LBRACE);

        let mut fields: Vec<Field> = Vec::new();
        self.list("struct type", SEMI, RBRACE, |p| {
            p.field_decl(&mut fields);
            // 官方回调恒返回 `false` ⇒ 一直收到 `}` 为止。
            false
        });

        StructType { fields, pos }
    }

    /// 官方 `interfaceType`（`parser.go:1561`）：
    /// `InterfaceType = "interface" "{" { ( MethodDecl | EmbeddedElem ) ";" } "}"`。
    ///
    /// 每一项走哪条路由官方那三行决定：**先试方法**（`_Name` 才试），
    /// **结果没有名字就当嵌入元素**。所以 `int | string` 里的 `int` 会先被
    /// `methodDecl` 读成"名字"，再因为 `Name == nil` 被转去 `embeddedElem`
    /// —— 不是两条互斥的分支，是「试一条、不行就转」。
    pub(crate) fn interface_type(&mut self) -> InterfaceType {
        let pos = self.tok.pos;
        self.want(&INTERFACE);
        self.want(&LBRACE);

        let mut methods: Vec<Field> = Vec::new();
        self.list("interface type", SEMI, RBRACE, |p| {
            let mut f = if matches!(p.tok.kind, TokenKind::Ident(_)) {
                Some(p.method_decl())
            } else {
                None
            };
            let need_embedded = match &f {
                None => true,
                Some(x) => x.name.is_none(),
            };
            if need_embedded {
                f = Some(p.embedded_elem(f));
            }
            methods.push(into_field(f.expect("embeddedElem 恒返回 Some")));
            false
        });

        InterfaceType { methods, pos }
    }

    /// 官方 `funcResult`（`parser.go:1587`）：`Result = Parameters | Type`。
    pub(crate) fn func_result(&mut self) -> Vec<Field> {
        if self.got(&LPAREN) {
            return self.param_list(None, None, RPAREN.clone(), false, false);
        }
        let pos = self.tok.pos;
        match self.type_or_nil() {
            Some(typ) => vec![Field {
                name: None,
                type_: typ,
                tag: None,
                pos,
            }],
            // 官方 `return nil` —— **不报错**（"没有结果"是合法的）。
            None => Vec::new(),
        }
    }

    /// 官方 `fieldDecl`（`parser.go:1629`）：
    ///
    /// ```text
    /// FieldDecl      = (IdentifierList Type | AnonymousField) [ Tag ] .
    /// AnonymousField = [ "*" ] TypeName .
    /// Tag            = string_lit .
    /// ```
    ///
    /// # 「小心舞步」（官方注释的 `Careful dance`）
    ///
    /// 名字后面一个 `[` 有两种含义：**嵌入的实例化类型** `T[P1, P2]`，或
    /// **`T of [P]E`**（即 `a [3]int`）。官方先按「类型实参表」解析，**解析完才知道**
    /// —— 拿到的若是 `IndexExpr` 就是实例化，否则是数组/切片。简写成「见 `[` 就当实例化」
    /// 会把 `struct{ a [3]int }` 读成嵌入类型（而两者 dump 出来**长得很像**）。
    pub(crate) fn field_decl(&mut self, fields: &mut Vec<Field>) {
        let pos = self.tok.pos;
        match self.tok.kind.clone() {
            TokenKind::Ident(_) => {
                let name = self.name();
                // 嵌入类型：名字后面是 `.`（限定名）/ 字面量（标签）/ `;` / `}`。
                if self.at(&TokenKind::Delim(Delim::Dot))
                    || matches!(self.tok.kind, TokenKind::Literal { .. })
                    || matches!(self.tok.kind, TokenKind::Semi(_))
                    || self.at(&TokenKind::Delim(Delim::RBrace))
                {
                    let typ = self.qualified_name(Some(name));
                    let tag = self.oliteral();
                    fields.push(Field {
                        name: None,
                        type_: typ,
                        tag,
                        pos,
                    });
                    return;
                }

                // `name1, name2, ... Type [ tag ]`
                let names = self.name_list(name);
                let typ = if names.len() == 1 && self.at(&TokenKind::Delim(Delim::LBrack)) {
                    match self.array_or_t_args() {
                        // 嵌入的实例化类型：`X` 就是这个**名字**。
                        ArrayOrTArgs::Index { index, pos: ipos } => {
                            let ix = IndexExpr {
                                x: Box::new(Expr::Name(names[0].clone())),
                                index: Box::new(index),
                                pos: ipos,
                            };
                            let tag = self.oliteral();
                            fields.push(Field {
                                name: None,
                                type_: Expr::Index(ix),
                                tag,
                                pos,
                            });
                            return;
                        }
                        ArrayOrTArgs::Type(t) => t,
                    }
                } else {
                    self.type_()
                };

                let tag = self.oliteral();
                for n in names {
                    let npos = n.pos;
                    fields.push(Field {
                        name: Some(n),
                        type_: typ.clone(),
                        tag: tag.clone(),
                        pos: npos,
                    });
                }
            }

            // 官方 `case _Star:` —— `*T` 嵌入。
            TokenKind::Op {
                form: OpForm::Plain,
                op: Operator::Mul,
                ..
            } => {
                self.next();
                let typ = if self.at(&TokenKind::Delim(Delim::LParen)) {
                    // `*(T)` —— 接受但抱怨。
                    let p = self.tok.pos;
                    self.syntax_error_at(
                        p,
                        DiagCode::SynUnexpectedToken,
                        "cannot parenthesize embedded type",
                    );
                    self.next();
                    let t = self.qualified_name(None);
                    self.got(&RPAREN); // 缺了也不再抱怨
                    t
                } else {
                    self.qualified_name(None)
                };
                let tag = self.oliteral();
                fields.push(Field {
                    name: None,
                    type_: unary_node(Op::Mul, typ, pos),
                    tag,
                    pos,
                });
            }

            // 官方 `case _Lparen:` —— `(T)` / `(*T)` 嵌入，都是「接受但抱怨」。
            TokenKind::Delim(Delim::LParen) => {
                self.syntax_error_at(
                    pos,
                    DiagCode::SynUnexpectedToken,
                    "cannot parenthesize embedded type",
                );
                self.next();
                let typ = if self.op_is(Operator::Mul) {
                    let ipos = self.tok.pos;
                    self.next();
                    unary_node(Op::Mul, self.qualified_name(None), ipos)
                } else {
                    self.qualified_name(None)
                };
                self.got(&RPAREN);
                let tag = self.oliteral();
                fields.push(Field {
                    name: None,
                    type_: typ,
                    tag,
                    pos,
                });
            }

            _ => {
                self.syntax_error_at(
                    pos,
                    DiagCode::SynUnexpectedToken,
                    "expected field name or embedded type",
                );
                self.advance(&[SEMI, RBRACE]);
            }
        }
    }

    /// 官方 `arrayOrTArgs`（`parser.go:1711`）：名字后面那个 `[` 的两种含义。
    ///
    /// 返回 [`ArrayOrTArgs`] —— `Index` 那一支**还没有 `X`**，由调用方填成那个名字
    /// （官方 `t.X will be filled in by caller`）。
    pub(crate) fn array_or_t_args(&mut self) -> ArrayOrTArgs {
        let pos = self.tok.pos;
        self.want(&LBRACK);
        if self.got(&RBRACK) {
            return ArrayOrTArgs::Type(self.slice_type(pos));
        }

        let (n, comma) = self.type_list(false);
        self.want(&RBRACK);
        if !comma && let Some(elem) = self.type_or_nil() {
            // `x [n]E`
            return ArrayOrTArgs::Type(Expr::ArrayType(ArrayType {
                len: Some(Box::new(n)),
                elem: Box::new(elem),
                pos,
            }));
        }

        // `x[n,]` / `x[n1, n2]` ⇒ 实例化（`X` 由调用方填）
        ArrayOrTArgs::Index { index: n, pos }
    }

    /// 官方 `methodDecl`（`parser.go:1760`）：`MethodSpec = MethodName Signature | InterfaceTypeName`。
    ///
    /// ⚠️ **只有走 `(`（有签名）那条路才给 `f.name` 赋值**。`default`（嵌入类型）
    /// 与「嵌入的实例化类型」两支都**故意留 `name == None`** —— `interfaceType` 正是靠
    /// 这个 `None` 把它转去 `embeddedElem` 的。顺手补上名字会让 `int | string`
    /// 在 `|` 处报错（`| string` 没人收）。
    fn method_decl(&mut self) -> PartialField {
        let pos = self.tok.pos;
        let name = self.name();
        const CONTEXT: &str = "interface method";

        match self.tok.kind.clone() {
            TokenKind::Delim(Delim::LParen) => {
                let (_, t) = self.func_type(CONTEXT);
                PartialField {
                    name: Some(name),
                    type_: Some(Expr::FuncType(t)),
                    pos,
                }
            }

            // 「小心舞步」的第二个现场：`m[T C](x T)`（泛型方法）还是
            // `T[P1, P2]`（嵌入的实例化类型）—— 先按参数表解析，再回头判。
            TokenKind::Delim(Delim::LBrack) => {
                let lpos = self.tok.pos;
                self.next();

                if self.at(&TokenKind::Delim(Delim::RBrack)) {
                    // `[]` 类型参数/实参表都不允许 ⇒ 当它没写。
                    let rpos = self.tok.pos;
                    self.next();
                    if self.at(&TokenKind::Delim(Delim::LParen)) {
                        // `name[](`
                        self.error_at(
                            rpos,
                            DiagCode::SynUnexpectedToken,
                            "empty type parameter list".to_string(),
                        );
                        let (_, t) = self.func_type(CONTEXT);
                        PartialField {
                            name: Some(name),
                            type_: Some(Expr::FuncType(t)),
                            pos,
                        }
                    } else {
                        // `name[]` 后面不是 `(` ⇒ 空的类型实参表
                        self.error_at(
                            rpos,
                            DiagCode::SynUnexpectedToken,
                            "empty type argument list".to_string(),
                        );
                        PartialField {
                            name: None,
                            type_: Some(Expr::Name(name)),
                            pos,
                        }
                    }
                } else {
                    // 类型实参表长得像「只有类型的参数表」，先按参数表解析。
                    let list = self.param_list(None, None, RBRACK.clone(), false, false);
                    if list.is_empty() {
                        // 有其他错误导致什么都没收到 ⇒ 当 `[]` 没写。
                        if self.at(&TokenKind::Delim(Delim::LParen)) {
                            let (_, t) = self.func_type(CONTEXT);
                            PartialField {
                                name: Some(name),
                                type_: Some(Expr::FuncType(t)),
                                pos,
                            }
                        } else {
                            PartialField {
                                name: None,
                                type_: Some(Expr::Name(name)),
                                pos,
                            }
                        }
                    } else if list[0].name.is_some() {
                        // 泛型方法：接受但抱怨。
                        let (_, t) = self.func_type(CONTEXT);
                        self.error_at(
                            lpos,
                            DiagCode::SynUnexpectedToken,
                            "interface method must have no type parameters".to_string(),
                        );
                        PartialField {
                            name: Some(name),
                            type_: Some(Expr::FuncType(t)),
                            pos,
                        }
                    } else {
                        // 嵌入的实例化类型。
                        let index = if list.len() == 1 {
                            list[0].type_.clone()
                        } else {
                            // ⚠️ `l.pos = list[0].Pos()`（官方 `:1836`）——
                            // 取的是 `Field::Pos()`（有名字用名字），不是 `Type.Pos()`。
                            let ipos = list[0].pos();
                            Expr::List(ListExpr {
                                elems: list.iter().map(|f| f.type_.clone()).collect(),
                                pos: ipos,
                            })
                        };
                        PartialField {
                            name: None,
                            type_: Some(Expr::Index(IndexExpr {
                                x: Box::new(Expr::Name(name)),
                                index: Box::new(index),
                                pos: lpos,
                            })),
                            pos,
                        }
                    }
                }
            }

            // 官方 `default:` —— 嵌入类型。⚠️ **不给 `name` 赋值**（见方法文档）。
            _ => PartialField {
                name: None,
                type_: Some(self.qualified_name(Some(name))),
                pos,
            },
        }
    }

    /// 官方 `embeddedElem`（`parser.go:1854`）：
    /// `EmbeddedElem = MethodSpec | EmbeddedTerm { "|" EmbeddedTerm }`。
    ///
    /// 传入 `Some(f)` 时只在它**已经有 `Type`** 的前提下接 `|` 链；`None` 时
    /// 自己 `embeddedTerm` 拿第一项。
    fn embedded_elem(&mut self, f: Option<PartialField>) -> PartialField {
        let mut f = match f {
            Some(f) => f,
            None => {
                let pos = self.tok.pos;
                let t = self.embedded_term();
                PartialField {
                    name: None,
                    type_: Some(t),
                    pos,
                }
            }
        };

        while self.op_is(Operator::Or) {
            let pos = self.tok.pos;
            self.next();
            let y = self.embedded_term();
            let x = f.type_.take().expect("embeddedElem 的 `X` 必须有类型");
            f.type_ = Some(Expr::Binary(Operation {
                op: Op::Or,
                x: Box::new(x),
                y: Some(Box::new(y)),
                pos,
            }));
        }

        f
    }

    /// 官方 `embeddedTerm`（`parser.go:1879`）：`EmbeddedTerm = [ "~" ] Type`。
    pub(crate) fn embedded_term(&mut self) -> Expr {
        if self.op_is(Operator::Tilde) {
            let pos = self.tok.pos;
            self.next();
            let x = self.type_();
            return unary_node(Op::Tilde, x, pos);
        }

        match self.type_or_nil() {
            Some(t) => t,
            None => {
                let x = self.bad_expr();
                let p = self.tok.pos;
                self.syntax_error_at(p, DiagCode::SynUnexpectedToken, "expected ~ term or type");
                self.advance(EMBEDDED_TERM_FOLLOW);
                x
            }
        }
    }

    /// 官方 `paramList`（`parser.go:2007`）。
    ///
    /// ```text
    /// Parameters    = "(" [ ParameterList [ "," ] ] ")" .
    /// ParameterList = ParameterDecl { "," ParameterDecl } .
    /// ```
    ///
    /// `(` 或 `[` **已被调用方消费**。`close` 是收尾词元（`)` 或 `]`）；
    /// 它同时决定 `typeSetsOk`（只有 `]` 那条路允许类型集写法 `~T` / `T|U`）。
    ///
    /// # 出口不变式
    ///
    /// 返回的每条 `Field.type_` **一定已定**：三段分布逻辑（原样 / 名字搬去当类型 /
    /// 从右往左补全）覆盖了所有入口。`into_field` 用 `unreachable!` 钉住这个不变式 ——
    /// 若真有漏网，宁可炸也不要静默造出一棵错树。
    pub(crate) fn param_list(
        &mut self,
        name: Option<Name>,
        typ: Option<Expr>,
        close: FollowArg,
        require_names: bool,
        dddok: bool,
    ) -> Vec<Field> {
        // 官方：`p.list` 在「已到 close」时**不调用**闭包 ⇒ 手上那对
        // `(name, typ)` 要单独收下（否则它会凭空消失）。
        // 用引用解构（`&name` / `&typ`）而非 `.expect()`：后者要先把值 move 走，
        // 会让下面 `let mut name = name` 拿不到东西。
        if close.matches(&self.tok.kind)
            && let (Some(n), Some(t)) = (&name, &typ)
        {
            self.next();
            return vec![Field {
                name: Some(n.clone()),
                type_: t.clone(),
                tag: None,
                pos: n.pos,
            }];
        }

        let mut name = name;
        let mut typ = typ;
        let mut named = 0usize; // 既有名字又有类型的参数个数
        let mut typed = 0usize; // 有类型的参数个数
        let mut list: Vec<PartialField> = Vec::new();

        let end = self.list("parameter list", COMMA, close.clone(), |p| {
            let par = match typ.take() {
                Some(t) => {
                    // 官方 `debug` 断言：给了 type 必然也给了 name。
                    let n = name.take().expect("官方的 debug 断言：有 type 必有 name");
                    let pos = n.pos;
                    Some(PartialField {
                        name: Some(n),
                        type_: Some(t),
                        pos,
                    })
                }
                None => p.param_decl_or_nil(name.take(), &close),
            };
            name = None; // 首个名字/类型已消费
            typ = None;
            if let Some(par) = &par {
                if par.name.is_some() && par.type_.is_some() {
                    named += 1;
                }
                if par.type_.is_some() {
                    typed += 1;
                }
            }
            if let Some(par) = par {
                list.push(par);
            }
            false
        });

        if list.is_empty() {
            return Vec::new();
        }

        if named == 0 && !require_names {
            // 全是未命名 ⇒ 那些"名字"其实都是**类型名**（`func(a, b)`）。
            for par in &mut list {
                if par.name.is_some() {
                    par.type_ = par.name.take().map(Expr::Name);
                }
            }
        } else if named != list.len() {
            // 有名字（或这是类型参数表）⇒ 所有参数都必须有名字 ⇒ 从右往左补类型。
            let mut err_pos: Option<Pos> = None;
            let mut cur: Option<Expr> = None;
            for par in list.iter_mut().rev() {
                if let Some(t) = &par.type_ {
                    cur = Some(t.clone());
                    if par.name.is_none() {
                        // 官方 `errPos = StartPos(typ)`（parser.go:2077）——
                        // ⚠️ 是 **StartPos** 不是 `Pos()`：`t[t]` 的起点是 `t`
                        // 不是 `[`（positions.go:45 对 IndexExpr 修正到 X）。
                        // 用 `pos()` 会右偏一列，tparams/typeset 对撞实测。
                        let p = t.start_pos();
                        err_pos = Some(p);
                        par.name = Some(Name {
                            value: "_".to_string(),
                            pos: p,
                        });
                    }
                } else if let Some(t) = &cur {
                    par.type_ = Some(t.clone());
                } else {
                    // 只有名字、右边也没有类型 ⇒ 补一个坏类型占位。
                    let p = par.name.as_ref().expect("无类型必有名").pos;
                    err_pos = Some(p);
                    par.type_ = Some(Expr::Bad(BadExpr { pos: p }));
                }
            }
            if let Some(mut ep) = err_pos {
                let msg = if named == typed {
                    ep = end; // 位置改报到收尾词元上
                    if require_names {
                        "missing type constraint"
                    } else {
                        "missing parameter type"
                    }
                } else if require_names {
                    if list.len() == 1 {
                        "missing type parameter name or invalid array length"
                    } else {
                        "missing type parameter name"
                    }
                } else {
                    "missing parameter name"
                };
                self.syntax_error_at(ep, DiagCode::SynUnexpectedToken, msg);
            }
        }

        // `...` 只允许出现在**最后**一个参数上；非法的把它退回元素类型 `T`。
        // 官方只用 `first` 报**第一条**（同一处错误重复报没有信息量）。
        let mut first = true;
        let n = list.len();
        for (i, f) in list.iter_mut().enumerate() {
            if let Some(elem) = dots_elem(&f.type_)
                && (!dddok || i + 1 < n)
            {
                if first {
                    first = false;
                    if let Some(dots) = &f.type_ {
                        let p = dots.pos();
                        let msg = if dddok {
                            "can only use ... with final parameter"
                        } else {
                            "invalid use of ..."
                        };
                        self.error_at(p, DiagCode::SynUnexpectedToken, msg.to_string());
                    }
                }
                f.type_ = Some(elem);
            }
        }

        list.into_iter().map(into_field).collect()
    }

    /// 官方 `paramDeclOrNil`（`parser.go:1904`）：
    /// `ParameterDecl = [ IdentifierList ] [ "..." ] Type`。
    ///
    /// 返回 `None` 表示「这一段完全没解析出东西」（官方 `return nil`）——
    /// **已经报过一条并 `advance` 过**，调用方不该再报。
    ///
    /// `typeSetsOk` 由 `close == _Rbrack` 决定：**只有类型参数表**允许
    /// `~T` / `T | U` 这种类型集写法。
    fn param_decl_or_nil(&mut self, name: Option<Name>, close: &FollowArg) -> Option<PartialField> {
        let type_sets_ok = *close == RBRACK;

        let mut pos = self.tok.pos;
        if let Some(n) = &name {
            pos = n.pos;
        } else if type_sets_ok && self.op_is(Operator::Tilde) {
            // `~` ...
            return Some(self.embedded_elem(None));
        }

        let mut f = PartialField {
            name: None,
            type_: None,
            pos,
        };

        if matches!(self.tok.kind, TokenKind::Ident(_)) || name.is_some() {
            let n = match name {
                Some(n) => n,
                None => self.name(),
            };

            if self.at(&TokenKind::Delim(Delim::LBrack)) {
                // `name "[" ...`
                match self.array_or_t_args() {
                    ArrayOrTArgs::Index { index, pos: ipos } => {
                        // `name "[" ... "]"` ⇒ 实例化类型，名字当 `X`
                        f.type_ = Some(Expr::Index(IndexExpr {
                            x: Box::new(Expr::Name(n)),
                            index: Box::new(index),
                            pos: ipos,
                        }));
                    }
                    ArrayOrTArgs::Type(t) => {
                        // `name "[" n "]" E` ⇒ 是参数名 + 类型
                        f.name = Some(n);
                        f.type_ = Some(t);
                    }
                }
                if type_sets_ok && self.op_is(Operator::Or) {
                    return Some(self.embedded_elem(Some(f)));
                }
                return Some(f);
            }

            if self.at(&TokenKind::Delim(Delim::Dot)) {
                // `name "." ...` ⇒ 限定名
                f.type_ = Some(self.qualified_name(Some(n)));
                if type_sets_ok && self.op_is(Operator::Or) {
                    return Some(self.embedded_elem(Some(f)));
                }
                return Some(f);
            }

            if type_sets_ok && self.op_is(Operator::Or) {
                // `name "|" ...` ⇒ 名字本身就是第一个约束项
                f.type_ = Some(Expr::Name(n));
                return Some(self.embedded_elem(Some(f)));
            }

            f.name = Some(n);
        }

        if self.at(&TokenKind::Delim(Delim::Ellipsis)) {
            // `[name] "..." ...`
            let pos = self.tok.pos;
            self.next();
            match self.type_or_nil() {
                Some(e) => {
                    f.type_ = Some(Expr::DotsType(DotsType {
                        elem: Box::new(e),
                        pos,
                    }));
                }
                None => {
                    f.type_ = Some(self.bad_expr());
                    let p = self.tok.pos;
                    self.syntax_error_at(p, DiagCode::SynUnexpectedToken, "... is missing type");
                }
            }
            return Some(f);
        }

        if type_sets_ok && self.op_is(Operator::Tilde) {
            // `[name] "~" ...`
            f.type_ = Some(
                self.embedded_elem(None)
                    .type_
                    .expect("embeddedElem(nil) 必有类型"),
            );
            return Some(f);
        }

        let t = self.type_or_nil();
        let t_is_some = t.is_some();
        f.type_ = t;
        if type_sets_ok && self.op_is(Operator::Or) && t_is_some {
            // `[name] type "|" ...`
            return Some(self.embedded_elem(Some(f)));
        }
        if f.name.is_some() || f.type_.is_some() {
            return Some(f);
        }

        // 官方 `syntaxError("expected " + tokstring(follow))` + `advance(_Comma, follow)`。
        let p = self.tok.pos;
        self.syntax_error_at(
            p,
            DiagCode::SynUnexpectedToken,
            &format!("expected {}", crate::recovery::tokstring(close)),
        );
        let adv: Vec<FollowArg> = vec![COMMA, close.clone()];
        self.advance(&adv);
        None
    }
}

/// `paramDeclOrNil` / `paramList` 的**中间形态**（见模块文档的表）。
///
/// 存在的理由：官方 `Field.Type` 可以在构造中途为 `nil`（`func(a)` 的名字 `a`
/// 先没有类型），而我们的 `Field.type_` 是非 `Option`。
struct PartialField {
    name: Option<Name>,
    type_: Option<Expr>,
    pos: Pos,
}

/// 把中间形态折成 [`Field`]。
///
/// `partial.type_` 在这里必须已经是 `Some` —— `param_list` 的三段分布逻辑覆盖了
/// 所有入口（见 [`Parser::param_list`] 的「出口不变式」）。
fn into_field(partial: PartialField) -> Field {
    Field {
        name: partial.name,
        type_: partial
            .type_
            .unwrap_or_else(|| unreachable!("paramList/interfaceType 出口处 Type 必已定")),
        tag: None,
        pos: partial.pos,
    }
}

/// `arrayOrTArgs` 的结果 —— `Index` 那一支**只差 `X`**（官方留给调用方填）。
pub(crate) enum ArrayOrTArgs {
    /// 已经是一个完整的类型（`[]T` / `[n]T` / `[...]T`）。
    Type(Expr),
    /// 「`名[...]`」，`X` 由调用方填成那个名字（`t.X will be filled in by caller`）。
    Index { index: Expr, pos: Pos },
}

/// `f.type_` 若是 `...T`，取它的元素类型（官方 `f.Type.(*DotsType)`）。
fn dots_elem(t: &Option<Expr>) -> Option<Expr> {
    match t {
        Some(Expr::DotsType(d)) => Some((*d.elem).clone()),
        _ => None,
    }
}

/// `typeOrNil` 里 `FollowArg` = `Follow` 的别名（避免与 `rgoc_lex::token` 的
/// 名字撞车时读起来混淆）。
type FollowArg = crate::recovery::Follow;
