//! 表达式族解析（阶段 D；`M2-design.md` §5.5）。
//!
//! 对齐官方 `parser.go` 的 `expr`（`:864`）/ `binaryExpr`（`:873`）/ `unaryExpr`（`:893`）/
//! `operand`（`:1010`）/ `pexpr`（`:1104`）/ `exprList`（`:2828`）/ `typeList`（`:2855`）/
//! `qualifiedName`（`:2794`）。
//!
//! # 三个「官方内联、我们拆出来」的地方（`T-AP11` 起**全部落地**）
//!
//! 官方把下面三块**写在 `pexpr`/`operand` 的 `switch` 臂里**，本文件各给它们函数：
//!
//! | 官方位置 | 这里 |
//! |---|---|
//! | `pexpr` 的 `case _Lparen`（`:1210`） | [`Parser::call_expr`] + [`Parser::arg_list`] |
//! | `pexpr` 的 `case _Lbrace`（`:1218`）+ `complitexpr`（`:1288`） | [`Parser::complit`] + [`Parser::complitexpr`] + [`Parser::bare_complitexpr`] |
//! | `operand` 的 `case _Func`（`:1053`） | [`Parser::func_type_or_lit`] |
//!
//! 拆出来**不是**为了让代码好看，而是让「还没实现」有一个**有名字的落点** ——
//! 于是 `pexpr`/`operand` 的**主干**（也就是与官方逐行同形的部分）
//! 从 `T-AP10` 到 `T-AP11` **一行都没改动**。主干一旦被改动，与官方的分叉
//! 就必须重新逐行比对（成本高，且漏比对不报错）。
//!
//! # ⚠️ 未实现的臂一律 `todo!()`，**不返回 `None`/空节点**
//!
//! 官方 `None` 有确切语义（「这里不是一个类型」/「这个语句起始词元认不出来」）。
//! 拿它当"还没写"，骨架就会**静默看起来是对的** —— 这是 `AGENTS.md`
//! 「漏了不会报错」的镜像（**没写的东西看起来像写对了**）。
//! ⚠️ **本文件至今已无 `todo!`**（`T-AP11` 填完了最后三处）；顺带一个易踩点：
//! `func` 字面量的**函数体**不在本文件，它是 `stmt.rs` 的 [`Parser::func_body`]
//! （官方 `funcBody` 也住在 `parser.go` 的表达式区之前，但语义上是「块 + `fnest` 记账」）。

use rgoc_ast::{
    AssertExpr, BadExpr, CallExpr, ChanDir, ChanType, CompositeLit, Expr, FuncLit, IndexExpr,
    KeyValueExpr, ListExpr, Name, Op, Operation, ParenExpr, SelectorExpr, SliceExpr,
    TypeSwitchGuard,
};
use rgoc_lex::diag::DiagCode;
use rgoc_lex::pos::Pos;
use rgoc_lex::token::{Delim, Keyword, OpForm, Operator, TokenKind};

use crate::parser::Parser;
use crate::recovery::{
    ASSIGN, CLOSE_RECOVER_FOLLOW, COLON, COMMA, DEFINE, ELLIPSIS, LBRACE, QUALIFIED_NAME_FOLLOW,
    RBRACE, RBRACK, RPAREN, SEMI_OR_RPAREN_FOLLOW, SLICE_RECOVER_FOLLOW, TYPE,
};

impl Parser {
    /// 官方 `expr`（`parser.go:864`）：`Expression = UnaryExpr | Expression binary_op Expression`。
    ///
    /// 直接是 `binaryExpr(nil, 0)` —— 官方**没有**独立的"表达式树"结构，
    /// 优先级爬升就是它的全部。
    ///
    /// 本函数同时是**表达式递归边的深度计数点**（`T-A-96`）：表达式的每一层
    /// 嵌套（括号 / 一元 / 调用实参 / 下标…）都会回到这里。
    pub(crate) fn expr(&mut self) -> Expr {
        if !self.depth_enter() {
            return self.bad_expr();
        }
        let x = self.binary_expr(None, 0);
        self.depth_leave();
        x
    }

    /// 官方 `binaryExpr`（`parser.go:873`）—— **优先级爬升**。
    ///
    /// # 判据为什么是「`Op` **且** `form == Plain` **且** `tprec > prec`」
    ///
    /// 官方的条件是 `(p.tok == _Operator || p.tok == _Star) && p.prec > prec`。
    /// 它要单列 `_Star`，是因为官方的词法把 `*` 与其它运算符分成了**不同的 tok**；
    /// 而 Rust 侧 `*` 也在 `TokenKind::Op` 里 ⇒ `form == Plain` 就同时覆盖了两者。
    ///
    /// ⚠️ **`form == Plain` 这一条不能省，也不能用 `prec` 代替**
    /// （`T-AP13` 实测踩到）：`_AssignOp`(`+=`) 与 `_IncOp`(`++`) 在官方是**不同的 tok**，
    /// 而 Rust 侧它们与 `+` 的 `op`/`prec` **完全相同**
    /// （`(Assign, Add, 4)` / `(IncDec, Add, 4)`，见 `rgoc_lex::token::op_expr`）。
    /// 只比 `prec` 会把 `x += 1` 读成 `Binary(Add, x, 1)` ——
    /// 于是 `simpleStmt` 路径一的 `_AssignOp` / `_IncOp` 两条臂**永远不可达**，
    /// 而症状是「`for i := 0; i < n; i++` 报 `expected expression`」，指不到真正的原因。
    ///
    /// 另外三个 `form == Plain` 但 `prec == 0` 的（`=` / `:=` / `<-`）仍由
    /// `tprec > prec` 挡住（`prec >= 0` 时 `0 > prec` 恒假）。
    pub(crate) fn binary_expr(&mut self, x: Option<Expr>, prec: u8) -> Expr {
        let mut x = match x {
            Some(x) => x,
            None => self.unary_expr(),
        };
        // 判据集中在 `binary_op_here` 一处：循环体里**只**比大小、不再看词元。
        while let Some((op, tprec, tpos)) = self.binary_op_here() {
            if tprec <= prec {
                break;
            }
            self.next();
            // 官方 `t.Y = p.binaryExpr(nil, tprec)`：**右操作数从 0 起重新爬**，
            // 同优先级的循环由外层 `while` 继续 ⇒ 自然左结合。
            let y = self.binary_expr(None, tprec);
            x = Expr::Binary(Operation {
                op: op_of(op),
                x: Box::new(x),
                y: Some(Box::new(y)),
                pos: tpos,
            });
        }
        x
    }

    /// 当前词元是**可参与二元优先级**的运算符时，取出 `(语义, 优先级, 位置)`。
    ///
    /// # 为什么单独成一个函数
    ///
    /// ① **借用**：`match &self.tok.kind` 写在循环条件里会把 `self` 借到循环体结束，
    /// 而循环体要 `&mut self`（`self.next()` / 递归）—— 返回**拥有所有权的三元组**
    /// 就把借用收在这里了。
    /// ② **判据唯一**：「什么叫二元运算符」只有这一个地方说了算。
    ///
    /// ⚠️ 官方在这一步额外判 `p.tok == _Operator || p.tok == _Star` ——
    /// 在 Rust 侧那一对恰好就是 `form == Plain`（`*` 是 `(Plain, Mul)`），
    /// 所以这里的 `form: OpForm::Plain` **就是**官方的那个 tok 判据，
    /// 不是"多余的保险"。省掉它会让 `+=` / `++` 被当成二元运算符 ——
    /// 理由与现场症状见 [`Parser::binary_expr`]。
    fn binary_op_here(&self) -> Option<(Operator, u8, Pos)> {
        match &self.tok.kind {
            TokenKind::Op {
                form: OpForm::Plain,
                op,
                prec,
            } => Some((*op, *prec, self.tok.pos)),
            _ => None,
        }
    }

    /// 官方 `unaryExpr`（`parser.go:893`）：`UnaryExpr = PrimaryExpr | unary_op UnaryExpr`。
    ///
    /// # `_Operator` 与 `_Star` 的折叠
    ///
    /// 官方分两个 `case`（`_Operator, _Star` 与 `_Star` 里的 `Mul`）。Rust 侧 `*`
    /// 就是 `Op{form: Plain, op: Mul}` ⇒ 一并放进同一个 `match`。
    pub(crate) fn unary_expr(&mut self) -> Expr {
        let pos = self.tok.pos;
        // 先取判别结果（结束对 `self.tok` 的借用）。
        let head = match &self.tok.kind {
            TokenKind::Op {
                form: OpForm::Plain,
                op,
                ..
            } => Some(*op),
            _ => None,
        };

        if let Some(op) = head {
            match op {
                // 官方 `case Mul, Add, Sub, Not, Xor, Tilde` —— 六个前缀运算符。
                Operator::Mul
                | Operator::Add
                | Operator::Sub
                | Operator::Not
                | Operator::Xor
                | Operator::Tilde => {
                    self.next();
                    let x = self.unary_expr();
                    return unary_node(op_of(op), x, pos);
                }
                // 官方单独一支：`&` 要先把括号剥掉（`&(T{})` 里的括号让 `{}` 被误认成块）。
                Operator::And => {
                    self.next();
                    let x = unparen(self.unary_expr());
                    return unary_node(Op::And, x, pos);
                }
                // 官方 `case _Arrow:`：`<-x`（接收）与 `<-chan E`（只收通道）**同形**，
                // 要等 `x` 解析完才知道是哪个 —— 是通道就走重新关联。
                Operator::Arrow => {
                    self.next();
                    let x = self.unary_expr();
                    if matches!(x, Expr::ChanType(_)) {
                        return self.recv_chan_reassoc(x);
                    }
                    return unary_node(Op::Arrow, x, pos);
                }
                _ => {}
            }
        }

        // 官方 `return p.pexpr(nil, true)` —— `keep_parens` 为真（那条 `TODO(mdempsky)`
        // 注释的理由：要能报出 `(x) := true`）。
        self.pexpr(None, true)
    }

    /// 官方 `operand`（`parser.go:1010`）。
    ///
    /// ```text
    /// Operand     = Literal | OperandName | MethodExpr | "(" Expression ")" .
    /// Literal     = BasicLit | CompositeLit | FunctionLit .
    /// OperandName = identifier | QualifiedIdent.
    /// ```
    ///
    /// # `keep_parens` 的两个来源
    ///
    /// 1. **调用方强制**（`unaryExpr` 传 `true`）；
    /// 2. **自己推断**：括号后紧跟 `{` ⇒ `x` 可能是复合字面量类型，括号必须留下
    ///    （否则 `(T){}` 的 `{}` 会被当语句块）。
    pub(crate) fn operand(&mut self, keep_parens: bool) -> Expr {
        // 官方 `case _Name`
        if matches!(self.tok.kind, TokenKind::Ident(_)) {
            return Expr::Name(self.name());
        }
        // 官方 `case _Literal`
        if let Some(l) = self.oliteral() {
            return Expr::BasicLit(l);
        }
        // 官方 `case _Lparen`
        if self.at(&TokenKind::Delim(Delim::LParen)) {
            let pos = self.tok.pos;
            self.next();
            // 括号内**允许**复合字面量：`(T{})` 合法。
            self.xnest += 1;
            let x = self.expr();
            self.xnest -= 1;
            self.want(&RPAREN);

            let keep = keep_parens || self.at(&TokenKind::Delim(Delim::LBrace));
            return if keep {
                Expr::Paren(ParenExpr {
                    x: Box::new(x),
                    pos,
                })
            } else {
                x
            };
        }

        // 官方 `case _Func` → `func_type_or_lit`
        if self.at(&TokenKind::Keyword(Keyword::Func)) {
            return self.func_type_or_lit();
        }
        // 官方 `case _Lbrack, _Chan, _Map, _Struct, _Interface` → `p.type_()`
        // （注释里叫 `othertype`）
        if matches!(
            self.tok.kind,
            TokenKind::Delim(Delim::LBrack)
                | TokenKind::Keyword(
                    Keyword::Chan | Keyword::Map | Keyword::Struct | Keyword::Interface
                )
        ) {
            return self.type_();
        }

        // 官方 `default:` —— **先造节点再报错**，所以畸形输入仍产出一棵完整的树
        // （对撞时两边的**节点数**才对得上）。
        let x = self.bad_expr();
        let pos = self.tok.pos;
        self.syntax_error_at(pos, DiagCode::SynUnexpectedToken, "expected expression");
        self.advance(CLOSE_RECOVER_FOLLOW);
        x
    }

    /// 官方 `pexpr`（`parser.go:1104`）—— 后缀链。
    ///
    /// ```text
    /// PrimaryExpr = Operand | Conversion
    ///             | PrimaryExpr Selector | PrimaryExpr Index | PrimaryExpr Slice
    ///             | PrimaryExpr TypeAssertion | PrimaryExpr Arguments .
    /// ```
    ///
    /// `x` 为 `None` 表示"还没有主表达式，自己去 `operand` 拿一个"（官方传 `nil`）。
    pub(crate) fn pexpr(&mut self, x: Option<Expr>, keep_parens: bool) -> Expr {
        let mut x = match x {
            Some(x) => x,
            None => self.operand(keep_parens),
        };

        loop {
            // 官方在进 `switch` **之前**取 `pos` —— 所以后面三种后缀节点的 `pos`
            // 都是**触发后缀的那个词元**（`.` / `[` / `(`），不是 `x` 的起点。
            let pos = self.tok.pos;
            // 克隆一次判别键，结束对 `self.tok` 的借用（臂体要 `&mut self`）。
            let kind = self.tok.kind.clone();

            match kind {
                // ── Selector / TypeAssertion / TypeSwitchGuard ──
                TokenKind::Delim(Delim::Dot) => {
                    self.next();
                    let inner = self.tok.kind.clone();
                    match inner {
                        TokenKind::Ident(_) => {
                            let sel = self.name();
                            x = Expr::Selector(SelectorExpr {
                                x: Box::new(x),
                                sel,
                                pos,
                            });
                        }
                        TokenKind::Delim(Delim::LParen) => {
                            self.next();
                            if self.got(&TYPE) {
                                // 官方：`t.Lhs is filled in by parser.simpleStmt`（T-AP13）
                                x = Expr::TypeSwitchGuard(TypeSwitchGuard {
                                    lhs: None,
                                    x: Box::new(x),
                                    pos,
                                });
                            } else {
                                let t = self.type_();
                                x = Expr::TypeAssert(AssertExpr {
                                    x: Box::new(x),
                                    type_: Some(Box::new(t)),
                                    pos,
                                });
                            }
                            self.want(&RPAREN);
                        }
                        _ => {
                            self.syntax_error_at(
                                pos,
                                DiagCode::SynUnexpectedToken,
                                "expected name or (",
                            );
                            self.advance(SEMI_OR_RPAREN_FOLLOW);
                        }
                    }
                }

                // ── Index / Slice ──
                TokenKind::Delim(Delim::LBrack) => {
                    self.next();

                    let mut low: Option<Expr> = None;
                    if !self.at(&TokenKind::Delim(Delim::Colon)) {
                        let (i, comma) = if self.at(&TokenKind::Delim(Delim::RBrack)) {
                            // 官方：`x[]` 非法（空索引/空实例化）—— 报一条，造 badExpr。
                            let p = self.tok.pos;
                            self.syntax_error_at(
                                p,
                                DiagCode::SynUnexpectedToken,
                                "expected operand",
                            );
                            (self.bad_expr(), false)
                        } else {
                            self.type_list(false)
                        };
                        if comma || self.at(&TokenKind::Delim(Delim::RBrack)) {
                            self.want(&RBRACK);
                            x = Expr::Index(IndexExpr {
                                x: Box::new(x),
                                index: Box::new(i),
                                pos,
                            });
                            // 官方这里是 `break`（跳出 `switch`，继续外层 `for`）。
                            continue;
                        }
                        low = Some(i);
                    }

                    // `x[i:` …。官方特意**不**用 `p.want(_Colon)`（见 go.dev/issue/47704）。
                    if !self.got(&COLON) {
                        let p = self.tok.pos;
                        self.syntax_error_at(
                            p,
                            DiagCode::SynUnexpectedToken,
                            "expected comma, : or ]",
                        );
                        self.advance(SLICE_RECOVER_FOLLOW);
                    }
                    self.xnest += 1;
                    let mut high: Option<Expr> = None;
                    if !self.at(&TokenKind::Delim(Delim::Colon))
                        && !self.at(&TokenKind::Delim(Delim::RBrack))
                    {
                        high = Some(self.expr());
                    }
                    let (mut full, mut max) = (false, None);
                    if self.at(&TokenKind::Delim(Delim::Colon)) {
                        full = true;
                        if high.is_none() {
                            // ⚠️ 官方用的是 `p.error`（**不是** `syntaxError`）⇒
                            // 无 `syntax error: ` 前缀、且**不受 EOF 抑制**。
                            let p = self.tok.pos;
                            self.error_at(
                                p,
                                DiagCode::SynUnexpectedToken,
                                "middle index required in 3-index slice".to_string(),
                            );
                            high = Some(self.bad_expr());
                        }
                        self.next();
                        if !self.at(&TokenKind::Delim(Delim::RBrack)) {
                            max = Some(self.expr());
                        } else {
                            let p = self.tok.pos;
                            self.error_at(
                                p,
                                DiagCode::SynUnexpectedToken,
                                "final index required in 3-index slice".to_string(),
                            );
                            max = Some(self.bad_expr());
                        }
                    }
                    self.xnest -= 1;
                    self.want(&RBRACK);
                    x = Expr::Slice(SliceExpr {
                        x: Box::new(x),
                        low: low.map(Box::new),
                        high: high.map(Box::new),
                        max: max.map(Box::new),
                        full,
                        pos,
                    });
                }

                // ── Arguments（CallExpr）──
                //
                // ⚠️ 官方是 `x = t`，**不是** `return t` —— 后缀链必须能**接着长**：
                // `f(a)(b)` / `f(a).b` / `f(a)[0]`。写成 `return` 会让 `f(a)(b)` 的
                // 第二个 `(` 落回语句层，报一条 `after top level declaration`。
                TokenKind::Delim(Delim::LParen) => {
                    x = self.call_expr(x, pos);
                }

                // ── `{`：复合字面量 vs 语句块 ──
                TokenKind::Delim(Delim::LBrace) => {
                    if !self.complit_starts_here(&x) {
                        // 官方 `break loop` —— `{` 不属于本主表达式（多半是块的开头）。
                        break;
                    }
                    // ⚠️ 同样：官方 `x = n` 后**继续循环** ⇒ `T{}.f` / `T{}[0]` 成立。
                    x = self.complit(x);
                }

                _ => break,
            }
        }

        x
    }

    /// 官方 `pexpr` 的 `case _Lbrace`（`parser.go:1218-1248`）的**判定部分**。
    ///
    /// 决定「这个 `{` 是复合字面量的开始，还是语句块的开头」。
    ///
    /// ⚠️ 判定**必须**先 `Unparen`：`(T){}` 的 `x` 是 `ParenExpr`，
    /// 直接看 `x` 的类型会得 `false`（于是整段被当块）。
    ///
    /// 判定的**负例**只能从 crate 内驱动（`xnest = -1` 只出现在官方的 `header()`，
    /// 归 `T-AP13`）⇒ 单测见本文件底部的 `rf1_xnest_为负时花括号不是复合字面量`。
    fn complit_starts_here(&self, x: &Expr) -> bool {
        let t = unparen(x.clone());
        match &t {
            // 官方：`case *Name, *SelectorExpr:` 且 `p.xnest >= 0`
            Expr::Name(_) | Expr::Selector(_) => self.xnest >= 0,
            // 官方：`case *IndexExpr:` 且 `p.xnest >= 0 && !isValue(t)`
            Expr::Index(_) => self.xnest >= 0 && !is_value(&t),
            // 官方：`case *ArrayType, *SliceType, *StructType, *MapType:` —— 无条件
            Expr::ArrayType(_) | Expr::SliceType(_) | Expr::StructType(_) | Expr::MapType(_) => {
                true
            }
            _ => false,
        }
    }

    // ── 调用（`AST-22`）─────────────────────────────────────────────────

    /// 官方 `pexpr` 的 `case _Lparen`（`parser.go:1210-1216`）：`Arguments`，即 `CallExpr`。
    ///
    /// `pos` 是调用方在**进 `switch` 之前**取的位置 ⇒ **`(`** 的位置（不是 `x` 的）。
    ///
    /// ⚠️ 官方这里**没有** `xnest++` —— 加层的是 [`Parser::arg_list`]（`p.argList` 自己
    /// `p.xnest++`）。两处都加会让 `xnest` 多一层，症状是嵌套过深时某个 `T{` 被
    /// 误判成块 —— **不报错**，只是树形悄悄变浅。
    fn call_expr(&mut self, fun: Expr, pos: Pos) -> Expr {
        self.next(); // 吃掉 `(`
        let (args, has_dots) = self.arg_list();
        Expr::Call(CallExpr {
            fun: Box::new(fun),
            args,
            has_dots,
            pos,
        })
    }

    /// 官方 `argList`（`parser.go:2740`）：
    ///
    /// ```text
    /// argList = [ arg { "," arg } [ "..." ] [ "," ] ] ")" .
    /// ```
    ///
    /// 返回 `(实参表, 末参是否带 ...)`。
    ///
    /// # 为什么是「逐元素」而不是 `exprList`
    ///
    /// `arg` 是**单个** `Expression`。用 `expr_list` 会把 `f(a, b)` 收成**一个**
    /// `ListExpr` 当唯一实参 —— 而 dump 侧 `ListExpr` 是**扁平化**的（§5.3），
    /// 于是「实参表」看起来完全正确，只有节点计数悄悄不同。
    ///
    /// # `has_dots` 是**覆盖**赋值，不是「见过就为真」
    ///
    /// 官方每轮都写 `hasDots = p.got(_DotDotDot)`，且 `...` 一出现就 `return true`
    /// 结束循环 —— 所以它等价于「**最后一个**实参后面跟了 `...`」。
    /// 若写成 `has_dots |= …`，`f(a..., b)`（本不合法）会留下 `true`。
    fn arg_list(&mut self) -> (Vec<Expr>, bool) {
        let mut args: Vec<Expr> = Vec::new();
        let mut has_dots = false;
        self.xnest += 1;
        self.list("argument list", COMMA, RPAREN, |p| {
            args.push(p.expr());
            has_dots = p.got(&ELLIPSIS);
            has_dots
        });
        self.xnest -= 1;
        (args, has_dots)
    }

    // ── 复合字面量（`AST-12/13`）─────────────────────────────────────────

    /// 官方 `pexpr` 的 `case _Lbrace` 在**判定通过之后**那一段（`parser.go:1240-1247`）。
    ///
    /// `x` 是 `{` **前面**那棵树（可能是 `Name` / `Selector` / `Index` / 类型字面量）。
    ///
    /// # `n.Type = x` 用的是**原样**的 `x`，不是 `Unparen` 后的
    ///
    /// 对 `(T){}`：官方先报一条 `cannot parenthesize type in composite literal`，
    /// **然后照常**把 `ParenExpr` 挂进 `Type`（`t` 只用于判定与判错）。
    /// 这里把括号丢掉的话，dump 会少一层、oracle 逐行 diff 直接歪。
    fn complit(&mut self, x: Expr) -> Expr {
        if unparen(x.clone()) != x {
            let pos = self.tok.pos;
            self.syntax_error_at(
                pos,
                DiagCode::SynUnexpectedToken,
                "cannot parenthesize type in composite literal",
            );
        }
        let mut n = self.complitexpr();
        n.type_ = Some(Box::new(x));
        Expr::CompositeLit(n)
    }

    /// 官方 `complitexpr`（`parser.go:1288`）：
    ///
    /// ```text
    /// LiteralValue = "{" [ ElementList [ "," ] ] "}" .
    /// ```
    ///
    /// 出口的 `type_` 恒为 `None` —— 有类型的形态由调用方（[`Parser::complit`] 或
    /// [`Parser::bare_complitexpr`] 的**外层**）填。这是有意的：官方 `complitexpr`
    /// 自己也**不**填 `Type`（`:1289-1318` 全程没碰过 `x.Type`），
    /// 「谁认得类型谁填」把职责放在唯一知道答案的那一层。
    fn complitexpr(&mut self) -> CompositeLit {
        let pos = self.tok.pos; // `{`
        self.xnest += 1;
        self.want(&LBRACE);
        let mut elems: Vec<Expr> = Vec::new();
        let rbrace = self.list("composite literal", COMMA, RBRACE, |p| {
            let mut e = p.bare_complitexpr();
            if p.at(&TokenKind::Delim(Delim::Colon)) {
                // ⚠️ 官方的 `l.pos = p.pos()` 在 `p.next()` **之前** ⇒ 位置取的是
                // **`:`**，既不是 key 的也不是 value 的。
                let kpos = p.tok.pos;
                p.next();
                let value = p.bare_complitexpr();
                e = Expr::KeyValue(KeyValueExpr {
                    key: Box::new(e),
                    value: Box::new(value),
                    pos: kpos,
                });
            }
            elems.push(e);
            false // 官方 `return false` —— 单个元素不终止列表
        });
        self.xnest -= 1;
        CompositeLit {
            type_: None,
            elems,
            rbrace,
            pos,
        }
    }

    /// 官方 `bare_complitexpr`（`parser.go:1272`）：
    ///
    /// ```text
    /// Element = Expression | LiteralValue .
    /// ```
    ///
    /// 见到 `{` 就是**省略类型**的嵌套字面量（`[][]int{{1}}` 的内层）。
    /// 其余走完整 [`Parser::expr`]（于是 `T{-1, a + b}` 的两个元素各自是整棵树）。
    fn bare_complitexpr(&mut self) -> Expr {
        if self.at(&TokenKind::Delim(Delim::LBrace)) {
            return Expr::CompositeLit(self.complitexpr());
        }
        self.expr()
    }

    /// 官方 `exprList`（`parser.go:2828`）：
    ///
    /// ```text
    /// ExpressionList = Expression { "," Expression } .
    /// ```
    ///
    /// 多值时结果是一个 [`Expr::List`]（官方 `ListExpr`）—— 见 `decl.rs` 里
    /// 「`Expr` 还是 `Vec<Expr>`」那条 C6 归一的说明。
    pub(crate) fn expr_list(&mut self) -> Expr {
        let x = self.expr();
        if self.got(&COMMA) {
            let mut elems = vec![x, self.expr()];
            while self.got(&COMMA) {
                elems.push(self.expr());
            }
            // 官方 `t.pos = x.Pos()` —— 首元素**自己**的位置。
            let pos = elems[0].pos();
            Expr::List(ListExpr { elems, pos })
        } else {
            x
        }
    }

    /// 官方 `typeList`（`parser.go:2855`）。
    ///
    /// ```text
    /// typeList = arg { "," arg } [ "," ] .
    /// ```
    ///
    /// 返回 `(表达式, 是否见到了逗号)`。第二个值是 `pexpr` 用来区分
    /// 「`x[i]` 索引」与「`x[i, j]` 空实例化/多下标」的判据。
    ///
    /// `strict` 为真时首元素必须是**类型**（泛型实例化），为假时可以是任意表达式。
    pub(crate) fn type_list(&mut self, strict: bool) -> (Expr, bool) {
        self.xnest += 1;
        let mut x = if strict { self.type_() } else { self.expr() };
        let mut comma = false;
        if self.got(&COMMA) {
            comma = true;
            if let Some(t) = self.type_or_nil() {
                let mut elems = vec![x, t];
                while self.got(&COMMA) {
                    match self.type_or_nil() {
                        Some(t) => elems.push(t),
                        None => break,
                    }
                }
                let pos = elems[0].pos();
                x = Expr::List(ListExpr { elems, pos });
            }
        }
        self.xnest -= 1;
        (x, comma)
    }

    /// 官方 `qualifiedName`（`parser.go:2794`）：`OperandName | QualifiedIdent`。
    ///
    /// 与 [`Parser::pexpr`] 的区别：这里**只**认「名字」或「名字 `.` 名字」，
    /// **不带**任何后缀（`(` / `[` / `{`）。类型名用这条，因为它不该吃调用/索引后缀
    /// —— 那些是 `typeInstance` 之类另有的路径。
    ///
    /// `name` 是 `Some` 时用调用方已经读好的第一个名字（官方 `name != nil` 分支）。
    pub(crate) fn qualified_name(&mut self, name: Option<Name>) -> Expr {
        let mut x = match name {
            Some(n) => Expr::Name(n),
            None => {
                if matches!(self.tok.kind, TokenKind::Ident(_)) {
                    Expr::Name(self.name())
                } else {
                    // 官方 `x = NewName(p.pos(), "_")` —— **先**造节点，**再**报错、再同步。
                    let pos = self.tok.pos;
                    self.syntax_error_at(pos, DiagCode::SynUnexpectedToken, "expected name");
                    self.advance(QUALIFIED_NAME_FOLLOW);
                    Expr::Name(Name {
                        value: "_".to_string(),
                        pos,
                    })
                }
            }
        };

        if self.at(&TokenKind::Delim(Delim::Dot)) {
            let pos = self.tok.pos;
            self.next();
            let sel = self.name();
            x = Expr::Selector(SelectorExpr {
                x: Box::new(x),
                sel,
                pos,
            });
        }

        // ⚠️ 官方 `:2820` 还有**这一段**（`T[int]` 的类型实例化）——
        // `T-AP12` 回源逐行读 `qualifiedName` 时才发现漏了。少了它，
        // `var x T[int]` 会在 `[` 处报「expected type」。
        if self.at(&TokenKind::Delim(Delim::LBrack)) {
            x = self.type_instance(x);
        }
        x
    }

    /// 官方 `badExpr`（`parser.go:2141`）：一个只记位置的占位节点。
    pub(crate) fn bad_expr(&self) -> Expr {
        Expr::Bad(BadExpr { pos: self.tok.pos })
    }

    /// 官方 `gotAssign`（`parser.go:218`）：`=` 与 `:=` 都收，`:=` 报一条。
    ///
    /// # 为什么 `:=` 要 `fallthrough` 到 `_Assign` 之后
    ///
    /// 官方是「报错 **+ 照常消费**」：`:=` 在声明里几乎总是笔误（写成了短变量声明），
    /// 消费掉它才有可能继续解析出后面的值表达式。只报错不消费会让整条声明报废。
    pub(crate) fn got_assign(&mut self) -> bool {
        if DEFINE.matches(&self.tok.kind) {
            let pos = self.tok.pos;
            self.syntax_error_at(pos, DiagCode::SynUnexpectedToken, "expected =");
            self.next();
            return true;
        }
        self.got(&ASSIGN)
    }

    /// 官方 `unaryExpr` 的 `_Arrow` 后段（`parser.go:942-968`）：`<-chan E` 的**重新关联**。
    ///
    /// `<-x` 是接收运算，`<-chan E` 是只收通道 —— 两者**同形**，要等 `x` 解析完才分得清。
    /// 判据就是「解析出来的 `x` 是不是 `ChanType`」。
    ///
    /// # 为什么不能像官方那样"就地改"
    ///
    /// 官方在循环里 `c.Dir = RecvOnly` 再 `t = c.Elem` —— 那是**指针树上的原地修改**。
    /// Rust 是值语义 ⇒ 改成「先把 `SendOnly` 链**拆开**（同时按官方顺序报错），
    /// 再自底向上重建」。链长 = 源码里连续 `chan` 的个数，实际几乎总是 1。
    ///
    /// # 两条错误
    ///
    /// - 链上撞到**已经是** `RecvOnly` 的层（`<-<-chan E`）⇒ `unexpected <-, expected chan`；
    /// - 走到头 `dir` 仍停在 `SendOnly`（最内层元素不是通道，如 `<-chan<- int`）
    ///   ⇒ `unexpected …, expected chan`。
    ///
    /// 两条都**不阻止产出节点** —— 恢复纪律是「报错 + 继续给出一棵完整的树」。
    fn recv_chan_reassoc(&mut self, x: Expr) -> Expr {
        let mut levels: Vec<Pos> = Vec::new(); // 每层 `chan` 的位置（自外向内）
        let mut dir = Some(ChanDir::SendOnly); // 官方 `dir := SendOnly`
        let mut t = x;

        while dir == Some(ChanDir::SendOnly) {
            match t {
                Expr::ChanType(c) => {
                    dir = c.dir;
                    if dir == Some(ChanDir::RecvOnly) {
                        // `<-<-chan E` 不成立 —— 官方把错误报在**当前词元**上（已前进过）。
                        let p = self.tok.pos;
                        self.syntax_error_at(
                            p,
                            DiagCode::SynUnexpectedToken,
                            "unexpected <-, expected chan",
                        );
                    }
                    levels.push(c.pos);
                    t = *c.elem;
                }
                // 官方 `if !ok { break }` —— 最内层元素不是通道。
                _ => break,
            }
        }

        if dir == Some(ChanDir::SendOnly) {
            let p = self.tok.pos;
            // 官方的消息里带 `String(t)`（把那个元素表达式渲染出来）——
            // 渲染器归 `T-AP16`，这里只保留消息形态。
            self.syntax_error_at(
                p,
                DiagCode::SynUnexpectedToken,
                "unexpected expression, expected chan",
            );
        }

        // 自底向上重建：链上每层都变 `RecvOnly`，**叶子原样**（它不是通道）。
        let mut acc = t;
        for cpos in levels.into_iter().rev() {
            acc = Expr::ChanType(ChanType {
                dir: Some(ChanDir::RecvOnly),
                elem: Box::new(acc),
                pos: cpos,
            });
        }
        acc
    }

    /// 官方 `operand` 的 `case _Func`（`parser.go:1053-1068`）：`FuncLit` 或**函数类型**。
    ///
    /// 两者都要 `funcType`；区别只在后面跟不跟 `{`。
    ///
    /// ⚠️ 官方这一步传的 context 是 **`"function type"`**（与 `typeOrNil` 同一句），
    /// 所以**表达式位置的 `func[T any]()` 也报「不许有类型参数」**。
    ///
    /// # 位置取 `func` 关键字，不是 `(`
    ///
    /// 官方先 `pos := p.pos()` 再 `p.next()`；`FuncLit.pos` 因此是 `func` 的位置。
    /// （与 [`Parser::call_expr`] 的 `(` 一样，都是「最左标示其种类的词元」。）
    fn func_type_or_lit(&mut self) -> Expr {
        let pos = self.tok.pos; // `func`
        self.next();
        let (_, ftyp) = self.func_type("function type");
        if self.at(&TokenKind::Delim(Delim::LBrace)) {
            // 官方 `p.xnest++` … `p.xnest--`：**函数体里允许**复合字面量
            // （`{ x := T{} }` 的 `T{}` 不该被当成块）。少了这对增减，
            // 函数体里第一个 `T{` 会被读成嵌套块 —— 不报错，只让树变浅。
            self.xnest += 1;
            let body = self.func_body();
            self.xnest -= 1;
            return Expr::FuncLit(FuncLit {
                type_: Box::new(Expr::FuncType(ftyp)),
                body: Box::new(body),
                pos,
            });
        }
        Expr::FuncType(ftyp)
    }
}

/// 造一个一元 `Operation`。
///
/// 官方只有一个 `Operation`（靠 `Y == nil` 区分一元/二元）；我们拆成
/// [`Expr::Unary`] / [`Expr::Binary`] 两个变体 ⇒ **这里必须写死 `y: None`**，
/// 否则拆分就退化成不可区分的两个名字。
///
/// `pub(crate)` 是因为 `type_.rs` 也要它：官方的 `newIndirect`（`*T`）与
/// `embeddedTerm`（`~T`）造的也是一元 `Operation`，两处**必须同形**。
pub(crate) fn unary_node(op: Op, x: Expr, pos: Pos) -> Expr {
    Expr::Unary(Operation {
        op,
        x: Box::new(x),
        y: None,
        pos,
    })
}

/// 官方 `Unparen`（`parser.go:2887`）：反复剥掉最外层的 `ParenExpr`。
///
/// `pub(crate)` 是因为 `callStmt`（`stmt.rs`）要用它做 `go (f())` 的检查
/// —— 官方那份是包级函数，两处共用同一份逻辑。
pub(crate) fn unparen(x: Expr) -> Expr {
    match x {
        Expr::Paren(p) => unparen(*p.x),
        other => other,
    }
}

/// 官方 `isValue`（`parser.go:1259`）：`x` 在**语法上必定是值**（而非类型）。
///
/// 判据只有一条用途：`x[i]` 里的 `x` **是值**时，后面的 `{` 不可能是复合字面量
/// （「值的索引」不是类型）。所以这张表只需要**保守**：判错的代价是多/少一次 complit。
///
/// ⚠️ 与官方逐条对齐，**不"顺手"加变体**：加了会让某些 `{` 被误判成块，
/// 而症状是「树形悄悄变浅」，不会报错。
fn is_value(x: &Expr) -> bool {
    match x {
        Expr::BasicLit(_)
        | Expr::CompositeLit(_)
        | Expr::FuncLit(_)
        | Expr::Slice(_)
        | Expr::TypeAssert(_)
        | Expr::TypeSwitchGuard(_)
        | Expr::Call(_) => true,
        // `*T` 可能是个类型 ⇒ 只有二元的 `*` 才算值（`x.Op != Mul || x.Y != nil`）
        Expr::Unary(o) | Expr::Binary(o) => o.op != Op::Mul || o.y.is_some(),
        Expr::Paren(p) => is_value(&p.x),
        Expr::Index(i) => is_value(&i.x) || is_value(&i.index),
        _ => false,
    }
}

/// 官方 `isTypeElem`（`parser.go:747`）：`x` 是否是（可能带括号的）**类型元素**表达式。
///
/// # 为什么会返回 `false`
///
/// 结果为 `false` 有两种：**确定不是**类型元素，以及「既可能是类型元素、又可能是普通
/// 值表达式」（如 `x` 是个裸名字）。官方的注释点名了后者 —— 那种情况**不能据它下结论**。
///
/// 唯一消费者是 [`extract_name`]（`typeDecl` 的 `[` 二义消解）。
fn is_type_elem(x: &Expr) -> bool {
    match x {
        Expr::ArrayType(_)
        | Expr::StructType(_)
        | Expr::FuncType(_)
        | Expr::InterfaceType(_)
        | Expr::SliceType(_)
        | Expr::MapType(_)
        | Expr::ChanType(_) => true,
        Expr::Unary(o) | Expr::Binary(o) => {
            is_type_elem(&o.x) || o.y.as_deref().is_some_and(is_type_elem) || o.op == Op::Tilde
        }
        Expr::Paren(p) => is_type_elem(&p.x),
        _ => false,
    }
}

/// 官方 `extractName`（`parser.go:690`）：把表达式拆成 `(名字, 表达式)`。
///
/// | 官方输入 | 结果 |
/// |---|---|
/// | `*Name` | `(name, None)` —— 只有名字、没有约束 |
/// | `P*E` / `P*E\|F\|~G`（`*` 或 `\|` 顶层） | `(P, 约束)` |
/// | `P(E)`（单实参、无 `...`） | `(P, E)`（括号被剥掉） |
/// | 其余 | `(None, Some(x))` —— **拆不动**，原样交回 |
///
/// # 拆分条件
///
/// `*` / `|` / `P(E)` 三条都要 `force || is_type_elem(右/实参)` —— 即「右边那坨
/// **确定**是类型元素」才敢把左边那个名字当成**类型参数名**。`force` 在 `typeDecl`
/// 里等于「`[` 后面的东西以 `,` 结尾」（见 [`Parser::type_decl`]）。
///
/// # 为什么返回 `Option<Expr>`
///
/// 官方第二个返回值可以是 `nil`（`*Name` 那条：「没有约束」）。用 `Option` 保留这个
/// 区分 —— 它直接决定 `type A[N]int` 是数组还是泛型。
///
/// 住在 `expr.rs`（而不是官方的"声明区"）是因为它**整体是表达式变换**：要用到私有的
/// [`unparen`]、要造一元 [`Operation`]。放这里免得把 `unparen` 扩成 `pub(crate)`。
pub(crate) fn extract_name(x: Expr, force: bool) -> (Option<Name>, Option<Expr>) {
    match x {
        // `x = name`（没有约束）。
        Expr::Name(n) => (Some(n), None),

        Expr::Unary(op) | Expr::Binary(op) => {
            // 一元运算（`*P`）不是 `name expr`。
            if op.y.is_none() {
                return (None, Some(Expr::Unary(op)));
            }
            match op.op {
                Op::Mul => {
                    // `P *E` ⇒ 把 `*` 改成一元约束：`name *E`。
                    let ok = force || op.y.as_deref().is_some_and(is_type_elem);
                    let name = match &*op.x {
                        Expr::Name(n) if ok => Some(n.clone()),
                        _ => None,
                    };
                    match name {
                        Some(name) => {
                            let y = *op.y.expect("二元必有 Y");
                            (Some(name), Some(unary_node(Op::Mul, y, op.pos)))
                        }
                        None => (None, Some(Expr::Binary(op))),
                    }
                }
                Op::Or => {
                    // `P lhs|x.Y` ⇒ 先把 `x.X` 拆出 `(name, lhs)`。
                    let force2 = force || op.y.as_deref().is_some_and(is_type_elem);
                    let (name, lhs) = extract_name((*op.x).clone(), force2);
                    match (name, lhs) {
                        // ⚠️ **两个都非空**才算拆成（官方 `name != nil && lhs != nil`）：
                        // 只拆出名字（`lhs == nil`）说明右边根本不是约束 ⇒ 拆不动。
                        (Some(name), Some(lhs)) => (
                            Some(name),
                            Some(Expr::Binary(Operation {
                                op: Op::Or,
                                x: Box::new(lhs),
                                y: op.y,
                                pos: op.pos,
                            })),
                        ),
                        _ => (None, Some(Expr::Binary(op))),
                    }
                }
                _ => (None, Some(Expr::Binary(op))),
            }
        }

        Expr::Call(call) => {
            // `P(E)` —— 名字 + **恰好一个**实参 + 无 `...` + 实参是类型元素。
            // 官方 `keep_parens = false` ⇒ 直接 `Unparen(实参)`（不保留括号）。
            let ok = force || call.args.first().is_some_and(is_type_elem);
            if ok
                && call.args.len() == 1
                && !call.has_dots
                && let Expr::Name(n) = &*call.fun
            {
                let arg0 = call.args.into_iter().next().expect("len == 1");
                return (Some(n.clone()), Some(unparen(arg0)));
            }
            (None, Some(Expr::Call(call)))
        }

        other => (None, Some(other)),
    }
}

/// 词法层的运算符语义 → AST 层的运算符。
///
/// 用**穷尽 `match`** 写：任一侧加变体都会在编译期炸 —— 不会像"两边各维护一份
/// 映射表"那样静默漏掉。两个枚举都只有 22/24 个变体，机械但对齐要求高。
fn op_of(op: Operator) -> Op {
    use Operator as L;
    match op {
        L::Add => Op::Add,
        L::Sub => Op::Sub,
        L::Mul => Op::Mul,
        L::Div => Op::Quo,
        L::Rem => Op::Rem,
        L::And => Op::And,
        L::Or => Op::Or,
        L::Xor => Op::Xor,
        L::Shl => Op::Shl,
        L::Shr => Op::Shr,
        L::AndNot => Op::AndNot,
        L::AndAnd => Op::LAnd,
        L::OrOr => Op::LOr,
        L::Not => Op::Not,
        L::Tilde => Op::Tilde,
        L::Arrow => Op::Arrow,
        L::Eql => Op::Eql,
        L::Neq => Op::Neq,
        L::Lss => Op::Lss,
        L::Leq => Op::Leq,
        L::Gtr => Op::Gtr,
        L::Geq => Op::Geq,
        // `=` / `:=` 的优先级是 0 ⇒ `binary_expr` 的 `tprec > prec` 永远挡住它们，
        // `unary_expr` 的 `match` 也不列它们。**不可达**，不是"暂时没写"。
        L::Assign | L::Define => {
            unreachable!("`=`/`:=` 的优先级为 0，不会走到运算符节点（调用点已排除）")
        }
    }
}

#[cfg(test)]
mod tests {
    use rgoc_lex::token::SemiOrigin;

    use super::*;
    use crate::parser::ErrMode;

    fn parser(src: &[u8]) -> Parser {
        Parser::new(src, "t.go", ErrMode::Continue)
    }

    fn name_of(e: &Expr) -> &str {
        match e {
            Expr::Name(n) => &n.value,
            other => panic!("期望 Name，got {other:?}"),
        }
    }

    /// **RF1 钉点**：`xnest` 决定 `T{}` 里的 `{` 是**复合字面量**还是**块的开头**。
    ///
    /// 官方在 `if`/`for`/`switch` 的 header 里把 `p.xnest` 置 `-1`（`parser.go:2365`），
    /// 因为那里 `{` 多半是**块**：`if x == T{} {}` 的第一个 `{}` 属于 `T`、
    /// 第二个才是块 —— 而语法上两者**同形**，只能靠上下文层数区分。
    ///
    /// # 为什么这条只能在 crate 内测
    ///
    /// `p.xnest = -1` **只**出现在官方的 `header()`（`:2356`）里，而 `header` 是
    /// `ifStmt`/`forStmt`/`switchStmt` 的公共前缀 —— 归 `T-AP13`。从文件入口
    /// （`const x = …`）无法把 `xnest` 弄成负数，所以这里直接驱动 `pub(crate)` 的
    /// [`Parser::expr`]。与 `recovery.rs` 的 `advance` 边界单测同一个理由
    /// （那条 Ruling：**外面驱动不到的内部语义，在 crate 内测**）。
    ///
    /// # 为什么是**双向**的
    ///
    /// 同一个源串 `x == T{}` 跑两遍，**只改 `xnest`**。只测负例的话
    /// 「`complit_starts_here` 恒返回 `false`」也能过，而那会让**所有**复合字面量失效。
    /// 反过来只测正例，「`xnest` 从未被读」也能过 —— 于是 `if x == T{} {}` 会歪。
    ///
    /// `T-AP13` 落地 `header()` 后应再补一条**端到端**的 `if x == T{} {}`
    /// （已登记在残留表）。
    #[test]
    fn rf1_xnest_为负时花括号不是复合字面量() {
        // 负例：header 里 `xnest = -1` ⇒ `T` 后面的 `{` **不是**复合字面量。
        let mut p = parser(b"x == T{}");
        p.xnest = -1;
        let e = p.expr();
        match &e {
            Expr::Binary(o) => {
                assert_eq!(o.op, Op::Eql, "该解析成 `==`");
                assert_eq!(o.y.as_deref().map(name_of), Some("T"), "右操作数是 `T`");
            }
            other => panic!("期望 Binary(`==`)，got {other:?}"),
        }
        assert_eq!(p.errcnt, 0, "这条路径不该报错");
        assert!(
            p.at(&TokenKind::Delim(Delim::LBrace)),
            "`{{` 必须**留在流里**（留给 `header` 之后的 `blockStmt`），got {:?}",
            p.tok.kind
        );

        // 正例：`xnest >= 0`（顶层 / 括号内 / 实参表内）⇒ 同一个输入**必须**
        // 吃成复合字面量。
        let mut q = parser(b"x == T{}");
        let e2 = q.expr();
        match &e2 {
            Expr::Binary(o) => match o.y.as_deref() {
                Some(Expr::CompositeLit(lit)) => {
                    assert_eq!(lit.pos.col, 7, "`{{` 在第 7 列");
                    assert_eq!(lit.rbrace.col, 8, "`}}` 在第 8 列 ⇒ 整个 `{{}}` 被吃掉了");
                }
                other => panic!("`xnest >= 0` 时 `T{{}}` 该是 CompositeLit，got {other:?}"),
            },
            other => panic!("期望 Binary(`==`)，got {other:?}"),
        }
        assert_eq!(q.errcnt, 0);
        // 吃到了结尾：此后只剩词法层在文件尾**自动补**的分号（`LEX-07`）。
        assert!(
            q.tok.kind == TokenKind::Semi(SemiOrigin::Eof),
            "该停在文件尾补出的分号上，got {:?}",
            q.tok.kind
        );
    }
}
