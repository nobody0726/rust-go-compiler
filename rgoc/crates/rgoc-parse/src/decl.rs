//! 声明族解析（阶段 C；`M2-design.md` §5.2）。
//!
//! 对齐官方 `parser.go` 的 `appendGroup`（`:532`）/ `importDecl`（`:553`）/
//! `constDecl`（`:586`）/ `typeDecl`（`:608`）/ `extractName`（`:690`）/
//! `varDecl`（`:771`）/ `funcDeclOrNil`（`:799`）。
//!
//! # `appendGroup` 为什么住在这里
//!
//! 它不是「import 的辅助函数」，而是**四种声明共用的收口**：`import` / `const` /
//! `type` / `var` 的顶层形态都是 `appendGroup(f)`（`func` 是唯一例外 —— 官方
//! `FuncDecl` 不走分组）。把它和四个声明放在一起，才能一眼看出「谁走分组、谁不走」。
//!
//! # 五个声明的完成度
//!
//! - `import`：**完整**（`T-AP08`）。
//! - `const` / `var`：**完整**（`T-AP10` 接上 `Type`/`Values` 与 `file()` 的两条臂）。
//! - `type`：**完整**（`T-AP09`）—— tparams / alias / `[` 二义（见 [`Parser::type_decl`]）。
//! - `func`：**完整**（`T-AP09`）—— 可选 recv / 类型参数 / **可选体**（前向声明）。
//!
//! ⚠️ 未到位的分支一律 `todo!()`（不是 `None`）—— 官方 `None` 有确切语义
//! （"这里本来就没有"），拿它当"还没写"会让骨架**静默看起来是对的**
//! （见 `stmt.rs` 的同一处理）。本文件至今**已无 `todo!`**。

use rgoc_ast::{
    ConstDecl, Decl, Expr, Field, FuncDecl, FuncType, Group, ImportDecl, LitKind, Name, TypeDecl,
    VarDecl,
};
use rgoc_lex::diag::DiagCode;
use rgoc_lex::token::{Delim, TokenKind};

use crate::expr::extract_name;
use crate::parser::Parser;
use crate::recovery::{LBRACE, LPAREN, RBRACK, RPAREN, SEMI, SEMI_OR_RPAREN_FOLLOW};

/// 走 `appendGroup` 的四种声明。
///
/// # 为什么是枚举，而不是官方那样的 `func(*Group) Decl` 回调
///
/// 官方的 `f` 是个**函数值**，调用点写 `p.appendGroup(list, p.importDecl)`。
/// Rust 里同样的写法要传闭包，而闭包一旦捕获 `self` 就与本方法的 `&mut self`
/// **借用冲突**（`self.append_group(list, |g| self.import_decl(g))` 编译不过）。
/// 枚举把「调哪个方法」变成数据：分派在 `DeclKind::parse` 里**穷尽**展开，
/// 加第五种声明时编译期就会提醒。
///
/// `func` **不在这里** —— 官方的 `FuncDecl` 不走 `appendGroup`（`parser.go:456-460`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum DeclKind {
    Import,
    /// `const`（`T-AP08` 起解析代码就位，`T-AP10` 接上 `file()` 的臂与 `Type`/`Values`）。
    Const,
    /// `type`（`T-AP09` 起完整：tparams / alias / `[` 二义）。
    Type,
    /// `var`（同 `Const`，`T-AP10` 接上）。
    Var,
}

impl DeclKind {
    /// 分派到对应的声明产生式。
    ///
    /// 返回 `Option` 是**忠实于官方的 `f` 契约**（`if x := f(g); x != nil`）：
    /// 四个函数其实永不返回 `nil`，但留着这一层，将来加"能返回 nil 的声明种类"
    /// 时不会漏掉判空。
    fn parse(self, p: &mut Parser, group: Option<Group>) -> Option<Decl> {
        match self {
            DeclKind::Import => Some(p.import_decl(group)),
            DeclKind::Const => Some(p.const_decl(group)),
            DeclKind::Type => Some(p.type_decl(group)),
            DeclKind::Var => Some(p.var_decl(group)),
        }
    }
}

impl Parser {
    /// 官方 `appendGroup`（`parser.go:532`）：
    ///
    /// ```text
    /// appendGroup(f) = f | "(" { f ";" } ")" .   // ";" is optional before ")"
    /// ```
    ///
    /// 每解析出一条就**追加**到 `list`（官方是 `list = append(list, x)` 并返回它；
    /// Rust 里收 `&mut Vec` 更直接，也避免把整个列表搬来搬去）。
    pub(crate) fn append_group(&mut self, list: &mut Vec<Decl>, kind: DeclKind) {
        // ⚠️ 用 `matches` 而不是 `got` —— 官方在这里是「**看**一眼是不是 `(`」，
        // 消费必须发生在 `clear_pragma()` **之后**（下一行的官方注释）。
        if LPAREN.matches(&self.tok.kind) {
            // `(` 的位置同时就是这一组的**身份**（见 `rgoc_ast::aux::Group`）。
            let group = Group { open: self.tok.pos };

            // ⚠️ 官方在这里留了一句注释：**must consume "(" after calling
            // clearPragma!** —— 顺序反了，`(` **之后**那条注释就会去给
            // **组外**的声明设 pragma（注释归属错位是静默的）。
            self.clear_pragma();
            self.next();

            self.list("grouped declaration", SEMI, RPAREN, |p| {
                if let Some(d) = kind.parse(p, Some(group)) {
                    list.push(d);
                }
                // 官方回调恒返回 `false` ⇒ 一直收到 `)` 为止。
                false
            });
        } else if let Some(d) = kind.parse(self, None) {
            // 单条声明的 `group` 是 `None`（官方传 `nil`）：`nil means not part of a group`。
            list.push(d);
        }
    }

    /// 官方 `importDecl`（`parser.go:553`）：
    ///
    /// ```text
    /// ImportSpec = [ "." | PackageName ] ImportPath .
    /// ImportPath = string_lit .
    /// ```
    ///
    /// ⚠️ 只验**合法字符串语法**，**不加载包**（`G5M` 的边界）。
    pub(crate) fn import_decl(&mut self, group: Option<Group>) -> Decl {
        let pos = self.tok.pos;
        let pragma = self.take_pragma();

        // 官方 `switch p.tok` 的顺序：`_Name` / `_Dot` / 其它。
        // 先取出判别结果再动 `self` —— `match &self.tok.kind` 会把 match 借住，
        // 而 `self.name()` 要 `&mut self`。
        let name = if matches!(self.tok.kind, TokenKind::Ident(_)) {
            Some(self.name())
        } else if self.at(&TokenKind::Delim(Delim::Dot)) {
            let dot = self.tok.pos;
            self.next();
            // 官方把 `.` 也**记成一个名字**（`NewName(p.pos(), ".")`）而不是单独的标记。
            Some(Name {
                value: ".".to_string(),
                pos: dot,
            })
        } else {
            None
        };

        let path = match self.oliteral() {
            None => {
                // 官方 `:572`：报一条、按 `advance(_Semi, _Rparen)` 同步，
                // 然后**照常返回这个节点**（`Path == nil`）。
                let p = self.tok.pos;
                self.syntax_error_at(p, DiagCode::SynUnexpectedToken, "missing import path");
                self.advance(SEMI_OR_RPAREN_FOLLOW);
                None
            }
            Some(mut lit) => {
                if !lit.bad && lit.kind != LitKind::String {
                    // 官方 `:577`：报在**字面量自己**的位置（`syntaxErrorAt(d.Path.Pos(), ...)`），
                    // 并把 `Bad` 置 true —— 那是下游判断"这个字面量可信吗"的依据。
                    let p = lit.pos;
                    self.syntax_error_at(
                        p,
                        DiagCode::SynUnexpectedToken,
                        "import path must be a string",
                    );
                    lit.bad = true;
                }
                Some(lit)
            }
        };

        Decl::Import(ImportDecl {
            group,
            pragma,
            name,
            path,
            pos,
        })
    }

    /// 官方 `constDecl`（`parser.go:586`）：
    ///
    /// ```text
    /// ConstSpec = IdentifierList [ [ Type ] "=" ExpressionList ] .
    /// ```
    pub(crate) fn const_decl(&mut self, group: Option<Group>) -> Decl {
        let pos = self.tok.pos;
        let pragma = self.take_pragma();
        // 先取出第一个名字 —— `self.name_list(self.name())` 是嵌套可变借用。
        let first = self.name();
        let names = self.name_list(first);

        // 官方 `:597`：`if p.tok != _EOF && p.tok != _Semi && p.tok != _Rparen` 才解析
        // Type/Values。**这个判断本身必须实现** —— 它决定 `const ( a = iota; b )`
        // 里那个 `b` 的 Type/Values 是**空**（继承了上一行的值），而不是解析失败。
        let (type_, values) = if self.at_decl_spec_end() {
            (None, Vec::new())
        } else {
            self.const_spec()
        };

        Decl::Const(ConstDecl {
            group,
            pragma,
            names,
            type_,
            values,
            pos,
        })
    }

    /// 官方 `varDecl`（`parser.go:771`）：
    ///
    /// ```text
    /// VarSpec = IdentifierList ( Type [ "=" ExpressionList ] | "=" ExpressionList ) .
    /// ```
    pub(crate) fn var_decl(&mut self, group: Option<Group>) -> Decl {
        let pos = self.tok.pos;
        let pragma = self.take_pragma();
        let first = self.name();
        let names = self.name_list(first);

        // 官方 `:782-789` 的三条分支（`gotAssign` → Values；否则 `type_()`，
        // 再看一次 `gotAssign`）—— 注意 `var` **没有** `const` 那个
        // 「到此为止」的前置判断，`=` 分支排在 `type_()` **前面**。
        let (type_, values) = self.var_spec();

        Decl::Var(VarDecl {
            group,
            pragma,
            names,
            type_,
            values,
            pos,
        })
    }

    /// 官方 `typeDecl`（`parser.go:608`）：
    ///
    /// ```text
    /// TypeSpec = identifier [ TypeParams ] [ "=" ] Type .
    /// ```
    ///
    /// # `[` 的二义（`T-A-23` / §5.6）—— 本任务最容易写反的地方
    ///
    /// `type A[N]int`（数组）与 `type A[T any] int`（泛型）**词法上无法预先区分**。
    /// 官方靠**先解析一个完整表达式**（`p.binaryExpr(p.pexpr(name, false), 0)`），
    /// 再用 [`extract_name`] 判它能否拆成「类型参数名 + 约束」，**两条倾斜规则**：
    ///
    /// - **单个名字 + `]`** ⇒ 数组（`ptype == nil && tok == _Rbrack` ⇒ 不满足条件）；
    /// - **约束可拆且后面不是 `]`** ⇒ 类型参数表。
    ///
    /// ⚠️ 简写成「看 `[` 后面是不是名字」会把 `type A [N]int` 误判成泛型 ——
    /// 不报错，只是 `Type` 从 `ArrayType` 变成别的东西。
    ///
    /// ⚠️ `type A [N + 1]int` 里 `N` 后面**不是 `]`** ⇒ 必须真的展开 `pexpr`+`binaryExpr`，
    /// 否则长度只剩 `N`（`+` 之后的丢掉）—— 而 `[N]int` 这种简单情形照样正确。
    pub(crate) fn type_decl(&mut self, group: Option<Group>) -> Decl {
        let pos = self.tok.pos;
        let pragma = self.take_pragma();
        let name = self.name();

        let (alias, tparams, type_) = if self.at(&TokenKind::Delim(Delim::LBrack)) {
            let bpos = self.tok.pos; // `[` 的位置 = 数组/切片类型的节点位置
            self.next();

            if matches!(self.tok.kind, TokenKind::Ident(_)) {
                // `d.Name "[" ...` —— 数组类型**或**类型参数表。
                let mut x: Expr = Expr::Name(self.name());
                if !self.at(&TokenKind::Delim(Delim::LBrack)) {
                    // `name` 后面不是 `[` ⇒ 展开成完整表达式（与 `expr()` 同形：
                    // 这里展开 `p.expr()` 的调用序列，把 `name` 直接喂给 `pexpr`）。
                    self.xnest += 1;
                    let pe = self.pexpr(Some(x), false);
                    x = self.binary_expr(Some(pe), 0);
                    self.xnest -= 1;
                }
                let force = self.at(&TokenKind::Delim(Delim::Comma));
                let (pname, ptype) = extract_name(x.clone(), force);
                let is_tparams = pname.is_some()
                    && (ptype.is_some() || !self.at(&TokenKind::Delim(Delim::RBrack)));
                if is_tparams {
                    // `d.Name "[" pname [ptype] ...` ⇒ 类型参数表（`]` 收尾）。
                    let tparams = self.param_list(pname, ptype, RBRACK.clone(), true, false);
                    let alias = self.got_assign();
                    let t = self.type_or_nil();
                    (alias, tparams, t)
                } else {
                    // `d.Name "[" x ...`（含 `"[" pname "]"`）⇒ 数组类型，长度就是 `x`。
                    (false, Vec::new(), Some(self.array_type(bpos, Some(x))))
                }
            } else if self.at(&TokenKind::Delim(Delim::RBrack)) {
                // `d.Name "[" "]" ...` ⇒ 切片类型。
                self.next();
                (false, Vec::new(), Some(self.slice_type(bpos)))
            } else {
                // `d.Name "[" ...` ⇒ 数组类型（长度由 `array_type` 自己解析，`[...]` 或表达式）。
                (false, Vec::new(), Some(self.array_type(bpos, None)))
            }
        } else {
            // 官方 `:686-689`：`d.Alias = p.gotAssign(); d.Type = p.typeOrNil()`。
            let alias = self.got_assign();
            let t = self.type_or_nil();
            (alias, Vec::new(), t)
        };

        let type_ = match type_ {
            Some(t) => t,
            None => {
                // 官方 `:691-695`：缺类型 ⇒ 造 `badExpr`、报 `in type declaration`、
                // `advance(_Semi, _Rparen)`，然后**照常返回节点**。
                let bad = self.bad_expr();
                let p = self.tok.pos;
                self.syntax_error_at(p, DiagCode::SynUnexpectedToken, "in type declaration");
                self.advance(SEMI_OR_RPAREN_FOLLOW);
                bad
            }
        };

        Decl::Type(TypeDecl {
            group,
            pragma,
            name,
            alias,
            tparams,
            type_,
            pos,
        })
    }

    /// 官方 `funcDeclOrNil`（`parser.go:799`）：
    ///
    /// ```text
    /// FunctionDecl = "func" FunctionName [ TypeParams ] ( Function | Signature ) .
    /// Function     = Signature FunctionBody .
    /// MethodDecl   = "func" Receiver MethodName ( Function | Signature ) .
    /// Receiver     = Parameters .
    /// ```
    ///
    /// # 名字里的 `OrNil`
    ///
    /// 这个版本的官方实现**从不返回 `nil`**（"`OrNil`" 是旧契约的遗留名）：认不出名字时
    /// 它造一个 `"_"` 名字 + 空 `FuncType` 继续走。返回 `Option<Decl>` 是为**忠实于调用方**
    /// `if d := p.funcDeclOrNil(); d != nil` 的判空形状（与 [`DeclKind::parse`] 同理）。
    ///
    /// # 入口状态
    ///
    /// ⚠️ `func` 关键字**已被 `file()` 消费** ⇒ 入口的 `self.tok` 是**名字**或 receiver 的 `(`。
    /// 这与 `typeDecl`/`constDecl` 一致（`f.pos = p.pos()` 取的是**关键字之后**那枚词元）。
    ///
    /// # 三条易错点
    ///
    /// 1. receiver 的 `case 0/1/default` 与 `default → fallthrough → case 1` 的**双动作**
    ///    （既报错又**取第一个**）—— 漏掉 fallthrough 会静默丢 `recv`；
    /// 2. 认不出名字时造 `"_"` 而**不是**放弃（`expected name` 的两种文案由 `hasRecv` 决定）；
    /// 3. **体是可选的**：只有当下一个词元是 `{` 才解析 —— 否则 `body == None`（前向声明）
    ///    且**不报错**。
    pub(crate) fn func_decl_or_nil(&mut self) -> Option<Decl> {
        let pos = self.tok.pos;
        let pragma = self.take_pragma();

        let mut recv: Option<Field> = None;
        let has_recv = self.got(&LPAREN);
        if has_recv {
            let rcvr = self.param_list(None, None, RPAREN.clone(), false, false);
            // 官方 `switch len(rcvr)`：0 ⇒ 报"没有 receiver"；1 ⇒ 取之；
            // default ⇒ 报"多个 receiver" **并** fallthrough 到取 `rcvr[0]`。
            match rcvr.len() {
                0 => {
                    let p = self.tok.pos;
                    self.error_at(
                        p,
                        DiagCode::SynUnexpectedToken,
                        "method has no receiver".to_string(),
                    );
                }
                1 => recv = rcvr.into_iter().next(),
                _ => {
                    let p = self.tok.pos;
                    self.error_at(
                        p,
                        DiagCode::SynUnexpectedToken,
                        "method has multiple receivers".to_string(),
                    );
                    recv = rcvr.into_iter().next();
                }
            }
        }

        let (name, tparams, type_) = if matches!(self.tok.kind, TokenKind::Ident(_)) {
            let name = self.name();
            // 官方传的 context 是**空串**（顶层 func 允许类型参数）。
            let (tparams, ft) = self.func_type("");
            (name, tparams, Expr::FuncType(ft))
        } else {
            // 认不出名字 ⇒ 造 `"_"` + 空 `FuncType`，报一条，同步到 `{`/`;`。
            let p = self.tok.pos;
            let name = Name {
                value: "_".to_string(),
                pos: p,
            };
            let msg = if has_recv {
                "expected name"
            } else {
                "expected name or ("
            };
            self.syntax_error_at(p, DiagCode::SynUnexpectedToken, msg);
            self.advance(&[LBRACE, SEMI]);
            (
                name,
                Vec::new(),
                Expr::FuncType(FuncType {
                    params: Vec::new(),
                    results: Vec::new(),
                    pos: p,
                }),
            )
        };

        // 体**可选**：只有见到 `{` 才解析（前向声明 `func f()` ⇒ `body == None`）。
        let body = if self.at(&TokenKind::Delim(Delim::LBrace)) {
            Some(self.func_body())
        } else {
            None
        };

        Some(Decl::Func(FuncDecl {
            recv,
            name,
            tparams,
            type_,
            body,
            pragma,
            pos,
        }))
    }

    /// `constDecl` 的 `Type` / `Values` 段（官方 `parser.go:598-601`）。
    ///
    /// ⚠️ 官方这一段的顺序是 **`typeOrNil` 在前、`gotAssign` 在后**：
    /// `typeOrNil` 返回 `nil`（"这里没有类型"）时**不报错**，继续看有没有 `=`。
    /// 反过来的话 `const x = 1` 会在 `=` 处报一条"expected type"。
    fn const_spec(&mut self) -> (Option<Expr>, Vec<Expr>) {
        let type_ = self.type_or_nil();
        let values = if self.got_assign() {
            flatten_expr_list(self.expr_list())
        } else {
            Vec::new()
        };
        (type_, values)
    }

    /// `varDecl` 的 `Type` / `Values` 段（官方 `parser.go:782-789`）。
    ///
    /// 与 `const` 的**结构差异**：`var` 先试 `gotAssign`（`var x = 1`），
    /// 否则**必须**有类型（`type_()` 会报错），再看一次 `gotAssign`。
    /// 抄错顺序的症状是 `var x = 1` 在 `=` 处多一条"expected type"。
    fn var_spec(&mut self) -> (Option<Expr>, Vec<Expr>) {
        if self.got_assign() {
            // `var x = 1`
            (None, flatten_expr_list(self.expr_list()))
        } else {
            // `var x T` / `var x T = 1`
            let type_ = self.type_();
            let values = if self.got_assign() {
                flatten_expr_list(self.expr_list())
            } else {
                Vec::new()
            };
            (Some(type_), values)
        }
    }

    /// 官方 `constDecl` / `varDecl` 里判断「说明到此为止」的三个词元（`parser.go:597`）。
    fn at_decl_spec_end(&self) -> bool {
        matches!(
            self.tok.kind,
            TokenKind::Eof | TokenKind::Semi(_) | TokenKind::Delim(Delim::RParen)
        )
    }
}

/// 官方 `Values Expr`（多值时是 `ListExpr`）→ 我们的 `Vec<Expr>`。
///
/// # 这是 C6 登记过的那条表示差异，落地在**唯一一处**
///
/// 官方 `ConstDecl.Values` / `VarDecl.Values` 的类型是 **`Expr`**：单值时就是那个
/// 表达式，多值时是一个 `ListExpr`。我们的模型是 `Vec<Expr>`
/// （`PA-1` 的对齐记录里登记为「C6 归一：ListExpr 扁平化」）。
///
/// # 为什么不会把"单个 ListExpr 值"误拆
///
/// 只有 `exprList` / `typeList` 会造 `ListExpr`，而它们**只在见到逗号时**才造。
/// `expr()` 自身**不可能**在顶层返回裸 `Expr::List` —— `typeList` 的结果总是被
/// `IndexExpr.index` 包住（`a[b, c]` 见 `test_expr.rs` 的用例）。
/// 所以「见到 `Expr::List` 就拆」与官方的语义**一一对应**。
fn flatten_expr_list(e: Expr) -> Vec<Expr> {
    match e {
        Expr::List(l) => l.elems,
        other => vec![other],
    }
}
