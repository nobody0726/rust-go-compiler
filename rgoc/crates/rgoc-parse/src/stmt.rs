//! 语句解析（`M2-design.md` §5.2）：`stmtOrNil` 分派 + `stmtList` + `blockStmt` + `funcBody`
//! + 简单语句与控制流（`simpleStmt` / `header` / `if` / `for` / `switch` / `select` / case 子句）。
//!
//! 对齐官方 `parser.go` 的 `stmtOrNil`（`:2610`）/ `stmtList`（`:2713`）/
//! `blockStmt`（`:2299`）/ `funcBody`（`:845`）/ `simpleStmt`（`:2151`）/
//! `header`（`:2352`）/ `ifStmt`（`:2466`）/ `forStmt`（`:2338`）/
//! `switchStmt`（`:2492`）/ `selectStmt`（`:2515`）/ `caseClause`（`:2537`）/
//! `commClause`（`:2565`）/ `labeledStmtOrNil`（`:2266`）。
//!
//! # 为什么这些函数住在一起
//!
//! 它们共用同一套「语句边界」约定：`stmtOrNil` 决定**一条**语句从哪开始、
//! `stmtList` 决定到哪结束（含恢复点 ②）、`blockStmt` 决定它们被哪对花括号包住，
//! `funcBody` 决定**什么时候 stopset 生效**（`fnest > 0`），而 `header` 决定
//! **`xnest` 什么时候置 -1**（`if`/`for`/`switch` 头部里禁复合字面量）。
//! 拆到几个文件会让「语句到哪结束」这件事散落 —— 而散落的症状是
//! **某一条路径上的恢复行为与 oracle 分叉**，最难定位。
//!
//! # `header` 为什么也必须在这里
//!
//! 官方把 `header` 排在 `ifStmt`/`forStmt`/`switchStmt` **之间**（`:2352`），
//! 因为它是这三条控制流的**共同前半段**，且它内部会调 `simpleStmt`。
//! 换句话说 `header` 是 `simpleStmt` 与三种控制流之间**唯一的**接缝 ——
//! 把 `simpleStmt` 挪走就等于把这个接缝切开。
//!
//! # `stmtOrNil` 已全部分派完（`T-AP14` 之后**无 `todo!`**）
//!
//! `T-AP13` 接通了 `_Name`（`exprList` → labeled / `simpleStmt`）、一元与 operand 臂、
//! 以及 `for`/`switch`/`select`/`if` 四条控制流臂；`T-AP14` 补上了剩下的
//! `_Var`/`_Const`/`_Type`（`decl_stmt`）、`fallthrough`/`break`/`continue`/`goto`
//! （三个 `*_stmt`）、`go`/`defer`（`call_stmt`）、`return`（`return_stmt`）。
//! **`stmt.rs` 里不再有 `todo!`。**
//!
//! ⚠️ `_Name` 臂**整体**属于 `T-AP13`（含 `Label:` 那一支），尽管 `M2-plan.md` 把
//! 「`_Name` 后判 `:`」写进了 `T-AP14`。理由：该臂是**原子的** —— 把它劈开意味着
//! T-AP13 的交付物（`AssignStmt`/`SendStmt`）**只**能从 `_Name` 臂到达，
//! 而劈开就要在臂中间留 `todo!`，于是每条普通赋值语句都会炸。见 `progress.md` 的 Ruling。
//!
//! # 为什么 `stmt_or_nil` / `stmt_or_nil_rest` 拆成两个方法
//!
//! 官方的 `stmtOrNil` 里有**两个** `switch`，中间夹了一次 `p.clearPragma()`（`:2637`）——
//! 而 `_Var`/`_Const`/`_Type` 那三条臂在**这次 `clearPragma` 之前**。
//! 拆成两个方法让「谁经过 `clearPragma`」这道界线**在类型上可见**；
//! 合成一个大 `match` 就只能靠注释记着（这类注释正是被抄错的重灾区）。

use rgoc_ast::{
    AssignOp, AssignStmt, BadExpr, BlockStmt, BranchStmt, BranchTok, CallStmt, CallTok, CaseClause,
    CommClause, DeclStmt, EmptyStmt, Expr, ExprStmt, ForStmt, IfStmt, LabeledStmt, Name,
    RangeClause, ReturnStmt, SelectStmt, SendStmt, SimpleStmt, Stmt, SwitchStmt,
};
use rgoc_lex::diag::DiagCode;
use rgoc_lex::pos::Pos;
use rgoc_lex::token::{Delim, Keyword, OpForm, Operator, SemiOrigin, TokenKind};

use crate::decl::DeclKind;
use crate::expr::unparen;
use crate::parser::Parser;
use crate::recovery::{
    ASSIGN, BLOCK_STMT_FOLLOW, CASE_DEFAULT_RBRACE_FOLLOW, COLON, COLON_CASE_DEFAULT_RBRACE_FOLLOW,
    DEFINE, ELSE, FOR, Follow, IF, LBRACE, LBRACE_OR_RBRACE_FOLLOW, LPAREN, RBRACE, SELECT, SEMI,
    SEMI_OR_RBRACE_FOLLOW, STMT_LIST_FOLLOW, SWITCH, VAR,
};
use crate::text::{emphasize_list, simple_stmt_pos, simple_stmt_text};

/// `simpleStmt` 的 `keyword` 参数 —— 官方传 `0` / `_For` / `_If` / `_Switch`。
///
/// # 为什么要有类型，而不是传一个 `Option<Keyword>`
///
/// 官方传的是 `token`，且**三处**用到它：① `_For && _Range`（range 子句）；
/// ② `_Switch && :=`（类型 switch 守卫）；③ `header` 的消息文本
/// （`var declaration not allowed in %s initializer` 里的 `%s`）。
/// 官方用同一个值同时表达"哪个控制流"和"能不能起 range"—— 用 `Option<Keyword>`
/// 会照抄这个重载；用独立 enum 则把「传 `0`」这一档显式命名成 [`StmtCtx::Plain`]。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum StmtCtx {
    /// 官方 `0` —— **不允许** range 子句（`range` 只可能出现在 `for` 头部）。
    Plain,
    For,
    If,
    Switch,
}

impl StmtCtx {
    /// 官方 `keyword.String()`（用在 `var declaration not allowed in %s initializer`）。
    fn keyword_str(self) -> &'static str {
        match self {
            // 官方那一支只在 `keyword == _For`/`_If`/`_Switch` 时被调到（`header` 的三个调用方）；
            // `Plain` 进不去（`header` 的形参只由那三个调用方给出）。
            StmtCtx::Plain => "0",
            StmtCtx::For => "for",
            StmtCtx::If => "if",
            StmtCtx::Switch => "switch",
        }
    }
}

/// `stmtList` 的**终止词元**（官方 `:2718` 的四个：`_EOF` / `_Rbrace` / `_Case` / `_Default`）。
///
/// # 为什么提成常量，而不是内联 `matches!`
///
/// 因为**内联的话它不可观测**。四个终止词元里，`stmtOrNil` 对前三个
/// （`_EOF` / `_Case` / `_Default`）本来就返回 `None`（官方 `:2709` 的 `return nil`），
/// 而 `stmtList` 拿到 `None` 也会 `break` —— 所以「少写一个终止词元」**不会让任何
/// 行为测试变红**，只是白白多调一次 `stmtOrNil`。这正是 `AGENTS.md` 记的
/// 「漏了不会报错」形状，只不过这里的"漏"是**防御性代码被悄悄删掉**。
///
/// 提取成常量后，`stmt_list_终止词元集合与官方一致` 能直接断言集合本身。
const STMT_LIST_END: [TokenKind; 4] = [
    TokenKind::Eof,
    TokenKind::Delim(Delim::RBrace),
    TokenKind::Keyword(Keyword::Case),
    TokenKind::Keyword(Keyword::Default),
];

impl Parser {
    /// 官方 `stmtOrNil`（`parser.go:2610`）。
    ///
    /// **不报错**：认不出的语句起始词元直接返回 [`None`]（官方 `:2709` 的 `return nil`）。
    /// 错误由调用方报出（`stmtList` 的收尾检查、或 `blockStmt`/`want` 的 `expected }`）。
    /// 这就是「`Stmt` 不设 `Bad` 变体」的实现面（`M2-design.md` §3.1）。
    ///
    /// 本函数同时是**语句递归边的深度计数点**（`T-A-96`）：块 → `stmtList` →
    /// 语句 → 块……每一层都回到这里。超限返回 `None`（同「认不出」路径，
    /// 外层的恢复会把后续 `}` 逐层吃掉）。
    pub(crate) fn stmt_or_nil(&mut self) -> Option<Stmt> {
        if !self.depth_enter() {
            return None;
        }
        let s = self.stmt_or_nil_inner();
        self.depth_leave();
        s
    }

    fn stmt_or_nil_inner(&mut self) -> Option<Stmt> {
        // 官方 `:2615-2624`。两条路径共用「先读一个 `exprList`」这一步：
        // `Label:` 是**带标签的语句**，其余是**简单语句**。官方为此把 `exprList`
        // 提到 `stmtOrNil` 里读（而不是各读一次），所以 `simpleStmt` 要能接收
        // 「已经读好的 lhs」—— 这就是它的 `lhs` 形参存在的唯一理由。
        if matches!(self.tok.kind, TokenKind::Ident(_)) {
            // 官方 `p.clearPragma()` —— 注释可能给下一条语句设 pragma，标识符一出现就作废。
            self.clear_pragma();
            let lhs = self.expr_list();
            // 官方 `:2620`：`if label, ok := lhs.(*Name); ok && p.tok == _Colon`
            // —— 两个条件是**合取**（let-chain 直译官方那个 `&&`；顺序照抄：
            // 先"是不是名字"再"是不是冒号"，虽然这里判不出差别，但不留第二种读法）。
            if self.at_follow(&COLON)
                && let Expr::Name(label) = &lhs
            {
                let label = label.clone();
                return self.labeled_stmt_or_nil(label);
            }
            return Some(stmt_of_simple(self.simple_stmt(Some(lhs), StmtCtx::Plain)));
        }

        // ── 局部声明（官方 `:2626-2635`）───────────────────────────────────
        // ⚠️ 这三条臂**在** `clearPragma()` **之前**（官方 `:2637` 那次调用在
        // 这个 `switch` 之后）⇒ 声明语句的 pragma 由 `varDecl`/`constDecl`/
        // `typeDecl` 自己 `takePragma()`。顺序反了会静默错位（注释归属对不上）。
        match self.tok.kind.clone() {
            TokenKind::Keyword(Keyword::Var) => Some(Stmt::Decl(self.decl_stmt(DeclKind::Var))),
            TokenKind::Keyword(Keyword::Const) => Some(Stmt::Decl(self.decl_stmt(DeclKind::Const))),
            TokenKind::Keyword(Keyword::Type) => Some(Stmt::Decl(self.decl_stmt(DeclKind::Type))),
            _ => self.stmt_or_nil_rest(),
        }
    }

    /// `stmtOrNil` 的**后半段**（官方 `:2637-2710`）—— [`Parser::stmt_or_nil`] 的延续。
    ///
    /// # 为什么拆成一个自由函数式的独立方法（而不是一个长 `match`）
    ///
    /// 因为官方那个 `switch` 之前夹了一次 `p.clearPragma()`（`:2637`）——
    /// 两段之间有**副作用**，而前一段的 `_Var`/`_Const`/`_Type` **不**经过它。
    /// 保留成两个方法，这道「谁经过 `clearPragma`」的界线就是**类型上可见的**；
    /// 合成一个大 `match` 则只能靠注释记着（而这类注释正是被抄错的重灾区）。
    fn stmt_or_nil_rest(&mut self) -> Option<Stmt> {
        // 官方 `:2637`：非标识符起始的语句同样要清 pragma。
        self.clear_pragma();

        // 克隆一次当前词元，**结束对 `self.tok` 的借用**：`match &self.tok.kind` 会把整个
        // match 借住，而臂体要 `&mut self`（`block_stmt` 等）。每条语句复制一次 `TokenKind`
        // （含 `Vec<u8>` 载荷）在本阶段没有意义 —— M2 的判据是**结构对齐**，不是性能
        // （`M2-design.md` §2.4 取舍 2）。这比再造一层「分派键枚举」更不容易与官方分叉。
        match self.tok.kind.clone() {
            // 官方 `:2640-2642`
            TokenKind::Delim(Delim::LBrace) => Some(Stmt::Block(self.block_stmt(""))),

            // 官方 `:2643-2647` `_Operator, _Star` 里只把自己当**一元前缀**的那几个算
            // 语句起始（`+x` `-x` `*x` `&x` `^x` `!x`）。其余 `form == Plain` 的运算符
            // （如 `<`）**不匹配本臂**，会下落到后面的臂 —— 这正是官方 `switch p.op` 的行为。
            //
            // ⚠️ `_Star` 就是 `*`：`form == Plain` 且 `op == Mul`。官方把它单列是因为
            // lexer 给 `*` 单独一个 token 值；我们这里 `Mul` 已在列表里，**不要**再写一遍。
            TokenKind::Op {
                form: OpForm::Plain,
                op:
                    Operator::Add
                    | Operator::Sub
                    | Operator::Mul
                    | Operator::And
                    | Operator::Xor
                    | Operator::Not,
                ..
            }
            // 官方 `:2649-2652`：operand 与复合类型起始词元。两者**共用同一条臂**
            // （同一次 `return p.simpleStmt(nil, 0)`），所以这里也只调一次。
            | TokenKind::Literal { .. }
            | TokenKind::Op {
                op: Operator::Arrow,
                ..
            }
            | TokenKind::Keyword(Keyword::Func)
            | TokenKind::Delim(Delim::LParen | Delim::LBrack)
            | TokenKind::Keyword(
                Keyword::Struct | Keyword::Map | Keyword::Chan | Keyword::Interface,
            ) => Some(stmt_of_simple(self.simple_stmt(None, StmtCtx::Plain))),

            // 官方 `:2654-2664`：四条控制流臂。
            TokenKind::Keyword(Keyword::For) => Some(self.for_stmt()),
            TokenKind::Keyword(Keyword::Switch) => Some(self.switch_stmt()),
            TokenKind::Keyword(Keyword::Select) => Some(self.select_stmt()),
            TokenKind::Keyword(Keyword::If) => Some(self.if_stmt()),

            // ── 官方 `:2666-2711` 的六条臂（`T-AP14` 落地）────────────────────
            //
            // ⚠️ 六条臂里**只有五条**读标签：`_Fallthrough`（`:2666-2671`）
            // 只做 `p.next()`，而 `_Break`/`_Continue`（`:2678`）是
            // 「**有**名字才读」、`_Goto`（`:2691`）是「**无条件**调 `p.name()`」。
            // 这三种差别只有配上「后面跟了一个名字 / 没跟名字」的输入才现形。
            TokenKind::Keyword(Keyword::Fallthrough) => Some(self.fallthrough_stmt()),
            TokenKind::Keyword(k @ (Keyword::Break | Keyword::Continue)) => Some(self.branch_stmt(
                if k == Keyword::Break {
                    BranchTok::Break
                } else {
                    BranchTok::Continue
                },
            )),
            TokenKind::Keyword(k @ (Keyword::Go | Keyword::Defer)) => Some(self.call_stmt(
                if k == Keyword::Go {
                    CallTok::Go
                } else {
                    CallTok::Defer
                },
            )),
            TokenKind::Keyword(Keyword::Goto) => Some(self.goto_stmt()),
            TokenKind::Keyword(Keyword::Return) => Some(self.return_stmt()),

            // 官方 `:2703-2706`：**不消费**这个 `;` —— 留给 `stmtList`
            // 的 `p.got(_Semi)` 收尾。在这里消费会让每条空语句少一格。
            TokenKind::Semi(_) => Some(Stmt::Empty(EmptyStmt { pos: self.tok.pos })),

            // 官方 `:2709` 的 `return nil` —— **不报错**，错误由调用方报。
            _ => None,
        }
    }

    /// 官方 `stmtList`（`parser.go:2713`）。
    pub(crate) fn stmt_list(&mut self) -> Vec<Stmt> {
        let mut l: Vec<Stmt> = Vec::new();

        // 终止条件 == 官方 `for p.tok != _EOF && p.tok != _Rbrace && p.tok != _Case &&
        // p.tok != _Default`（`:2718`）。集合见 [`STMT_LIST_END`]。
        while !STMT_LIST_END.iter().any(|k| k == &self.tok.kind) {
            // 官方 `:2721-2723`：认不出就收尾（**不报错**，交给调用方的 `want`）。
            let Some(s) = self.stmt_or_nil() else {
                break;
            };
            l.push(s);

            // `";" is optional before "}"`（官方 `:2725-2730`）。
            // ⚠️ 用 [`SEMI`]（**类别**）而不是按值 —— 换行自动补的分号必须也算（官方同）。
            if !self.got(&SEMI) && !self.at(&TokenKind::Delim(Delim::RBrace)) {
                let pos = self.tok.pos;
                self.syntax_error_at(pos, DiagCode::SynExpectedSemicolon, "at end of statement");
                // 恢复点 ②（`parser.go:2728`）
                self.advance(STMT_LIST_FOLLOW);
                self.got(&SEMI); // avoid spurious empty statement
            }
        }
        l
    }

    /// 官方 `funcBody`（`parser.go:845`）：函数体 = 块语句 + **`fnest` 记账**。
    ///
    /// # 为什么它和 `block_stmt` 住在一起（而官方把它排在表达式区之前）
    ///
    /// 官方的 `funcBody` 物理位置在 `expr()` **上面**，但语义上它只做一件事：
    /// 「`fnest++` → `blockStmt` → `fnest--`」。`fnest > 0` 正是
    /// [`Parser::advance`] 判定「要不要把 `STOPSET`（`recovery.rs`）并入 followset」的依据 ——
    /// 换句话说**函数体是 stopset 生效的唯一场所**。和 [`Parser::block_stmt`]
    /// 放一起，改 `fnest` 语义时不会漏掉另一个。
    ///
    /// # 两个调用方（`T-AP11` 先落地第一个）
    ///
    /// - `func` **字面量**（`operand` 的 `case _Func`，`T-AP11` ✅）
    /// - `funcDeclOrNil` 的声明式函数（`T-AP09`）
    ///
    /// # ⚠️ `CheckBranches` 的**门控**条件（`T-AP16` 落地）
    ///
    /// 官方 `funcBody`（`parser.go:845-859`）：
    ///
    /// ```text
    /// errcnt := p.errcnt
    /// body := p.blockStmt("")
    /// p.fnest--
    /// if p.mode&CheckBranches != 0 && errcnt == p.errcnt {
    ///     checkBranches(body, p.errh)
    /// }
    /// ```
    ///
    /// #1 `p.mode&CheckBranches` —— 我们**恒真**（M2 不提供「关掉」的入口）。
    /// 官方的 `CheckBranches` 是 `Mode` 的一个位，调用方（`cmd/compile`）传进来；
    /// 我们只提供 `parse_file`（相当于「带 CheckBranches 解析」）。
    /// ⚠️ **刻意不提供关闭入口**：官方那个位在 M2 没有对应的调用场景，
    /// 而「关掉」入口一旦存在，「忘了开」就是一个**不报错的静默错误**。
    ///
    /// #2 `errcnt == p.errcnt` —— **本函数体内没有语法错**。
    /// 官方注释：`as it may lead to spurious errors (e.g., see test/switch2.go)
    /// or possibly crashes due to incomplete syntax trees`。
    ///
    /// ⚠️ 判据是「**进 `funcBody` 之前**的 `errcnt`」，所以文件里**别处**有错
    /// **不影响**这个函数的检查。写成 `self.first.is_none()` 就成了「整个文件干净
    /// 才检查」⇒ 一个坏 import 会让全文件所有分支检查消失（症状：**报错变少**）。
    pub(crate) fn func_body(&mut self) -> BlockStmt {
        let errcnt = self.errcnt;
        self.fnest += 1;
        let body = self.block_stmt("");
        self.fnest -= 1;

        if errcnt == self.errcnt {
            crate::branches::check_branches(&body, self);
        }
        body
    }

    /// 官方 `blockStmt`（`parser.go:2299`）。
    pub(crate) fn block_stmt(&mut self, context: &str) -> BlockStmt {
        let mut s = BlockStmt {
            stmts: Vec::new(),
            rbrace: self.tok.pos, // 占位；下面按官方两处 `s.Rbrace = p.pos()` 覆盖
            pos: self.tok.pos,
        };

        // 官方注释：从 C 过来的人常忘了 Go 的花括号是必需的。
        if !self.got(&LBRACE) {
            let pos = self.tok.pos;
            self.syntax_error_at(
                pos,
                DiagCode::SynUnexpectedToken,
                &format!("expected {{ after {context}"),
            );
            self.advance(BLOCK_STMT_FOLLOW);
            s.rbrace = self.tok.pos; // 官方 `s.Rbrace = p.pos()`（可能正好落在 `}`）
            if self.got(&RBRACE) {
                return s; // 官方 `:2312-2314` 的早退
            }
        }

        s.stmts = self.stmt_list();
        // 官方在 `want` **之前**记 `s.Rbrace` —— 这样即使 `}` 缺失，位置也在树里。
        s.rbrace = self.tok.pos;
        self.want(&RBRACE);
        s
    }

    // ── 简单语句（官方 `parser.go:2151`）────────────────────────────────────

    /// 官方 `simpleStmt`（`parser.go:2151`）。
    ///
    /// ```text
    /// SimpleStmt = EmptyStmt | ExpressionStmt | SendStmt | IncDecStmt | Assignment | ShortVarDecl .
    /// ```
    ///
    /// # 两个入口（`lhs` 形参的全部意义）
    ///
    /// - `lhs == Some(x)`：`stmtOrNil` 已经把 `exprList` 读好了（它要先用同一个
    ///   表达式判是不是 `Label:`），所以这里**不能重读** —— 重读会多吃一个表达式。
    /// - `lhs == None`：`header` 的三个槽、`commClause` 的 comm 位在这里起头。
    ///
    /// # 两条互斥的路径（与官方同构）
    ///
    /// 判别式是官方那句
    /// `if _, ok := lhs.(*ListExpr); !ok && p.tok != _Assign && p.tok != _Define`：
    ///
    /// | 条件 | 走的路径 | 结果是 |
    /// |---|---|---|
    /// | `lhs` **不是**列表 **且** 后面不是 `=`/`:=` | 第一条 | `op=` / `++` / `--` / `<-` / 裸表达式 |
    /// | 其余 | 第二条 | `=`/`:=` 赋值（含 range 子句、类型 switch 守卫） |
    ///
    /// ⚠️ 第二条路径里还有个 `default` 分支（`expected := or = or comma`）——
    /// 它**到得了**，因为 `lhs` 是 `ListExpr` 时第一条路径被跳过、而后面既不是 `=`
    /// 也不是 `:=`（比如 `x, y z`）。
    pub(crate) fn simple_stmt(&mut self, lhs: Option<Expr>, keyword: StmtCtx) -> SimpleStmt {
        // 官方 `:2156-2162`：`for range x {}` —— 后面**必须**是 `range`，
        // 且官方 `debug` 档断言这条路径上调用方没给 `lhs`。
        if keyword == StmtCtx::For && self.at(&TokenKind::Keyword(Keyword::Range)) {
            debug_assert!(lhs.is_none(), "invalid call of simpleStmt");
            return SimpleStmt::Range(self.new_range_clause(None, false));
        }

        let lhs = match lhs {
            Some(x) => x,
            None => self.expr_list(),
        };

        if !matches!(lhs, Expr::List(_)) && !self.at_follow(&ASSIGN) && !self.at_follow(&DEFINE) {
            // ── 路径一：`expr` / `lhs op= rhs` / `lhs++` / `lhs <- rhs` ────────
            let pos = self.tok.pos;
            return match self.tok.kind.clone() {
                // 官方 `_AssignOp`（11 个复合赋值）
                TokenKind::Op {
                    form: OpForm::Assign,
                    op,
                    ..
                } => {
                    let aop = compound_assign_op(op);
                    self.next();
                    let rhs = self.expr();
                    SimpleStmt::Assign(AssignStmt {
                        op: aop,
                        lhs: vec![lhs],
                        rhs: vec![rhs],
                        pos,
                    })
                }
                // 官方 `_IncOp`（`++`/`--`）：官方 `newAssignStmt(pos, op, lhs, nil)`
                // ⇒ 我们的 `rhs` 是**空表**（`AST-39` 的约定）。
                TokenKind::Op {
                    form: OpForm::IncDec,
                    op,
                    ..
                } => {
                    let aop = incdec_op(op);
                    self.next();
                    SimpleStmt::Assign(AssignStmt {
                        op: aop,
                        lhs: vec![lhs],
                        rhs: Vec::new(),
                        pos,
                    })
                }
                // 官方 `_Arrow`：`lhs <- rhs`
                TokenKind::Op {
                    op: Operator::Arrow,
                    ..
                } => {
                    self.next();
                    let value = self.expr();
                    SimpleStmt::Send(SendStmt {
                        chan: Box::new(lhs),
                        value: Box::new(value),
                        pos,
                    })
                }
                // 官方 `default`：裸表达式。
                // ⚠️ 位置取 `lhs.Pos()`（**不是** `p.pos()`）—— 两者在
                // `f()` 这类输入上恰好相同，但在 `(x)` 这类上会差一列。
                _ => {
                    let pos = lhs.pos();
                    SimpleStmt::Expr(ExprStmt { x: lhs, pos })
                }
            };
        }

        // ── 路径二：`expr_list op= ...` ──────────────────────────────────────
        if self.at_follow(&ASSIGN) || self.at_follow(&DEFINE) {
            let pos = self.tok.pos;
            let op = if self.at_follow(&DEFINE) {
                AssignOp::Define
            } else {
                AssignOp::Assign
            };
            self.next();

            // 官方 `:2212-2215`：`for x := range y` / `for x = range y`。
            if keyword == StmtCtx::For && self.at(&TokenKind::Keyword(Keyword::Range)) {
                return SimpleStmt::Range(self.new_range_clause(Some(lhs), op == AssignOp::Define));
            }

            let rhs = self.expr_list();

            // 官方 `:2220-2229`：`switch x := v.(type)` —— 把 lhs 填进守卫，
            // 结果是一条**表达式语句**（不是赋值）。
            //
            // 官方两个条件（`rhs` 是 `*TypeSwitchGuard`、`lhs` 是 `*Name`）
            // **都满足**才改写成 ExprStmt；只满足一个就照常建 AssignStmt。
            // 用一个 match 把"两个都满足"与"其余"分开，免得先判后拆地走两遍。
            return match (
                keyword == StmtCtx::Switch && op == AssignOp::Define,
                rhs,
                lhs,
            ) {
                (true, Expr::TypeSwitchGuard(mut g), Expr::Name(n)) => {
                    g.lhs = Some(n);
                    let pos = g.pos;
                    SimpleStmt::Expr(ExprStmt {
                        x: Expr::TypeSwitchGuard(g),
                        pos,
                    })
                }
                (_, rhs, lhs) => SimpleStmt::Assign(AssignStmt {
                    op,
                    lhs: exprs_of(lhs),
                    rhs: exprs_of(rhs),
                    pos,
                }),
            };
        }

        // ── 路径二的 `default`（官方 `:2233-2243`）───────────────────────────
        //
        // 走到这里 `lhs` **必然是** `Expr::List`：路径一的条件是
        // 「`lhs` 不是列表」，而这里 `tok` 既不是 `=` 也不是 `:=`
        // ⇒ 只有「`lhs` 是列表」这一种可能把控制流送到这里。
        let p = self.tok.pos;
        self.syntax_error_at(p, DiagCode::SynUnexpectedToken, "expected := or = or comma");
        self.advance(SEMI_OR_RBRACE_FOLLOW);
        // 官方「make the best of what we have」：取列表**第一个**元素当表达式语句。
        let lhs = match lhs {
            Expr::List(l) => l
                .elems
                .into_iter()
                .next()
                .expect("`exprList` 至少产出一个元素"),
            other => other,
        };
        let pos = lhs.pos();
        SimpleStmt::Expr(ExprStmt { x: lhs, pos })
    }

    /// 官方 `newRangeClause`（`parser.go:2247`）。
    ///
    /// `pos` 是 **`range` 关键字**的位置（官方先 `r.pos = p.pos()` 再 `p.next()`）。
    /// `lhs == None` 对应官方 `r.Lhs == nil`（`for range x {}`）⇒ 我们的空表。
    fn new_range_clause(&mut self, lhs: Option<Expr>, def: bool) -> RangeClause {
        let pos = self.tok.pos;
        self.next(); // consume `range`
        let x = self.expr();
        RangeClause {
            lhs: match lhs {
                Some(l) => exprs_of(l),
                None => Vec::new(),
            },
            is_define: def,
            x,
            pos,
        }
    }

    // ── 语句头部（官方 `parser.go:2352`）────────────────────────────────────

    /// 官方 `header`（`parser.go:2352`）：`if`/`for`/`switch` 的**共同前半段**。
    ///
    /// 返回 `(init, cond, post)`。三条 `None` 都有确切语义：
    ///
    /// | 位置 | `None` 的含义 |
    /// |---|---|
    /// | `init` | 没有初始化语句（`if x {}` / `for {}`） |
    /// | `cond` | **没有条件** —— `for {}`（无限循环）或 `if` 的某条错误路径 |
    /// | `post` | 没有 post 语句（`if`/`switch` 恒为 `None`；`for` 只在三段式里有） |
    ///
    /// # `xnest = -1` —— 本函数存在的**第二个**理由
    ///
    /// 官方在整个头部把 `p.xnest` 置 `-1`（`:2364-2365`），出来时恢复 `outer`。
    /// 这是全仓**唯一**把 `xnest` 设为负的地方，语义是「头部里禁复合字面量」：
    /// `if x == T{} {}` 的那个 `T{` 必须被当成**块**（`if` 的条件到此为止），
    /// 而不是 `T{}` 复合字面量 —— 否则 `if` 的条件会一路吃到 `}`。这就是 RF1。
    ///
    /// ⚠️ 三条出口都必须恢复 `outer`：`{` 紧跟（早退）、range 子句（早退）、正常收尾。
    /// 漏一条 ⇒ 之后的**整个文件**都不再允许复合字面量，而症状是树形悄悄变浅（不报错）。
    pub(crate) fn header(
        &mut self,
        keyword: StmtCtx,
    ) -> (Option<SimpleStmt>, Option<Expr>, Option<SimpleStmt>) {
        // 官方 `p.want(keyword)`。三个调用方分别是 `ifStmt`/`forStmt`/`switchStmt`。
        let kw_follow = match keyword {
            StmtCtx::For => FOR,
            StmtCtx::If => IF,
            StmtCtx::Switch => SWITCH,
            StmtCtx::Plain => unreachable!("`header` 只由 if/for/switch 调用（官方三个调用方）"),
        };
        // 官方 `p.want(keyword)` ⇒ `expected keyword for` / `expected keyword if` …
        //
        // ⚠️ 注意是**带 `keyword ` 前缀**的（`tokstring` 的第三个特例）。
        // 早先这里传 `keyword.keyword_str()`（裸 `for` / `if` / `switch`）⇒
        // 消息与 oracle 差一个前缀，而语法上完全正常。
        self.want(&kw_follow);

        let mut init: Option<SimpleStmt> = None;
        let mut cond: Option<Expr> = None;
        let mut post: Option<SimpleStmt> = None;

        // 官方 `:2355-2361`：紧跟 `{` ⇒ 无头（`for {}` / `switch {}`）。
        // ⚠️ `if` 例外：`if {}` 是硬错，且**造一个 `BadExpr`** 当条件（位置 = 那个 `{`）。
        if self.at_follow(&LBRACE) {
            if keyword == StmtCtx::If {
                let p = self.tok.pos;
                self.syntax_error_at(
                    p,
                    DiagCode::SynUnexpectedToken,
                    "missing condition in if statement",
                );
                cond = Some(Expr::Bad(BadExpr { pos: p }));
            }
            return (init, cond, post);
        }

        let outer = self.xnest;
        self.xnest = -1;

        if !self.at_follow(&SEMI) {
            // 官方 `:2369-2371`：语句头里不许 `var` 声明 —— 但 `got(_Var)` **照常消费**它，
            // 于是后面的解析还能继续。位置取**消费之后**的 `p.pos()`（官方同）。
            if self.got(&VAR) {
                let p = self.tok.pos;
                self.syntax_error_at(
                    p,
                    DiagCode::SynUnexpectedToken,
                    &format!(
                        "var declaration not allowed in {} initializer",
                        keyword.keyword_str()
                    ),
                );
            }
            let first = self.simple_stmt(None, keyword);
            // 官方 `:2373-2377`：range 子句是**终点**（`for x := range y {}`），
            // 没有 cond/post。
            if matches!(first, SimpleStmt::Range(_)) {
                self.xnest = outer;
                return (Some(first), None, None);
            }
            init = Some(first);
        }

        // 官方用一个匿名 struct 记住"那个分号"——它同时要**位置**与**lit**
        // （`lit` 用来区分 `;` / 换行 / EOF，见下面 `done` 处的两条消息）。
        // 我们把"未知"表达成 `None`，而不是造一个 `line == 0` 的假 `Pos`。
        let mut semi_pos: Option<Pos> = None;
        let mut semi_lit: Option<SemiOrigin> = None;
        let mut cond_stmt: Option<SimpleStmt> = None;

        if !self.at_follow(&LBRACE) {
            if self.at_follow(&SEMI) {
                semi_pos = Some(self.tok.pos);
                semi_lit = semi_origin_of(&self.tok.kind);
                self.next();
            } else {
                // 官方注释：这里**问 `{` 而不是 `;`** 能得到更好的消息
                // （`if x y` 会报 `expected {`，比"expected ;"更贴近用户的意图）。
                self.want(&LBRACE);
                if !self.at_follow(&LBRACE) {
                    // 官方 `:2394`（go.dev/issue/22581）：冲一次同步点再继续。
                    self.advance(LBRACE_OR_RBRACE_FOLLOW);
                }
            }

            // 官方在这里有一条 `goto done` —— 用标志位直译（Rust 没有 goto）。
            let mut to_done = false;
            if keyword == StmtCtx::For {
                if !self.at_follow(&SEMI) {
                    if self.at_follow(&LBRACE) {
                        let p = self.tok.pos;
                        self.syntax_error_at(
                            p,
                            DiagCode::SynUnexpectedToken,
                            "expected for loop condition",
                        );
                        // 官方 `goto done`：**不再**尝试 cond/post。
                        to_done = true;
                    } else {
                        // ⚠️ 传 `Plain`（官方传 `0`）：**range 不许出现在 cond 位**。
                        cond_stmt = Some(self.simple_stmt(None, StmtCtx::Plain));
                    }
                }
                if !to_done {
                    self.want(&SEMI);
                    if !self.at_follow(&LBRACE) {
                        let ps = self.simple_stmt(None, StmtCtx::Plain);
                        // 官方 `:2408-2410`：post 位不许 `:=`。
                        if let SimpleStmt::Assign(a) = &ps
                            && a.op == AssignOp::Define
                        {
                            self.syntax_error_at(
                                a.pos,
                                DiagCode::SynUnexpectedToken,
                                "cannot declare in post statement of for loop",
                            );
                        }
                        post = Some(ps);
                    }
                }
            } else if !self.at_follow(&LBRACE) {
                // `if` / `switch` 的 cond/tag 位 —— 这里**允许** range？官方传的是
                // `keyword` 本身，所以 `if range x {}` 会走 `simpleStmt` 的 range 分支
                // （然后被下面的三档判定报一条）。照抄，不"顺手"改成 `Plain`。
                cond_stmt = Some(self.simple_stmt(None, keyword));
            }
        } else {
            // 官方 `:2415-2418`：`if x {` / `switch x {` ⇒ 刚才那段其实是**条件**，
            // 不是在初始化 ⇒ 把它从 `init` 挪到 `cond`。
            cond_stmt = init.take();
        }

        // ── 官方 `done:` 处的三档判定（`:2420-2449`）──────────────────────────
        match cond_stmt {
            None => {
                // 官方 `:2424-2433`：**只有 `if` 且真的见过分号**时才报这一条。
                // （`for {}` / `switch {}` 走到这里是完全正常的。）
                if keyword == StmtCtx::If
                    && let Some(p) = semi_pos
                {
                    let lit = match semi_lit {
                        Some(SemiOrigin::Newline) => "newline",
                        Some(SemiOrigin::Eof) => "EOF",
                        // 官方 `if semi.lit != "semicolon"` 的 else 支 —— 走到 else
                        // 意味着 lit **就是** `"semicolon"`，所以这里给的值用不上；
                        // 仍给 `"semicolon"` 以免把三档写成一个 `_ =>` 的兜底。
                        _ => "semicolon",
                    };
                    if semi_lit != Some(SemiOrigin::Explicit) {
                        self.syntax_error_at(
                            p,
                            DiagCode::SynUnexpectedToken,
                            &format!("unexpected {lit}, expected {{ after if clause"),
                        );
                    } else {
                        self.syntax_error_at(
                            p,
                            DiagCode::SynUnexpectedToken,
                            "missing condition in if statement",
                        );
                    }
                    // ⚠️ `badExpr` 的位置是**那个分号**（官方 `b.pos = semi.pos`），
                    // 不是当前词元。用 `self.bad_expr()` 会差若干列 —— 而列差不报错。
                    cond = Some(Expr::Bad(BadExpr { pos: p }));
                }
            }
            // 官方 `case *ExprStmt:` —— 正常条件。
            Some(SimpleStmt::Expr(e)) => cond = Some(e.x),
            // 官方 `default:` —— 条件位上是个赋值/发送/自增。
            Some(other) => {
                let msg = match &other {
                    // 官方 `:2442-2447`：把 `==` 写成 `=` 是最常见的笔误 ⇒ 换成
                    // `assignment X = Y` 的措辞。两处 `emphasize` 给**顶层二元**套括号。
                    SimpleStmt::Assign(a) if a.op == AssignOp::Assign => {
                        format!(
                            "assignment {} = {}",
                            emphasize_list(&a.lhs),
                            emphasize_list(&a.rhs)
                        )
                    }
                    _ => simple_stmt_text(&other),
                };
                let p = simple_stmt_pos(&other);
                self.syntax_error_at(
                    p,
                    DiagCode::SynUnexpectedToken,
                    &format!("cannot use {msg} as value"),
                );
            }
        }

        self.xnest = outer;
        (init, cond, post)
    }

    // ── 控制流（官方 `parser.go:2338` / `:2466` / `:2492` / `:2515`）────────

    /// 官方 `forStmt`（`parser.go:2338`）。
    pub(crate) fn for_stmt(&mut self) -> Stmt {
        let pos = self.tok.pos;
        let (init, cond, post) = self.header(StmtCtx::For);
        let body = self.block_stmt("for clause");
        Stmt::For(ForStmt {
            init,
            cond,
            post,
            body,
            pos,
        })
    }

    /// 官方 `ifStmt`（`parser.go:2466`）。
    pub(crate) fn if_stmt(&mut self) -> Stmt {
        let pos = self.tok.pos;
        let (init, cond, _) = self.header(StmtCtx::If);
        let then = self.block_stmt("if clause");

        let mut els: Option<Box<Stmt>> = None;
        if self.got(&ELSE) {
            if self.at_follow(&IF) {
                // `else if` ⇒ **嵌套的** `IfStmt`（官方递归调 `ifStmt`）。
                els = Some(Box::new(self.if_stmt()));
            } else if self.at_follow(&LBRACE) {
                els = Some(Box::new(Stmt::Block(self.block_stmt(""))));
            } else {
                let p = self.tok.pos;
                self.syntax_error_at(
                    p,
                    DiagCode::SynUnexpectedToken,
                    "else must be followed by if or statement block",
                );
                // 官方 `:2485` `p.advance(_Name, _Rbrace)` —— 与 `blockStmt` 缺 `{`
                // 是**同一份**字面量，所以复用 [`BLOCK_STMT_FOLLOW`]（不另写一份）。
                self.advance(BLOCK_STMT_FOLLOW);
            }
        }

        Stmt::If(IfStmt {
            init,
            cond,
            then,
            els,
            pos,
        })
    }

    /// 官方 `switchStmt`（`parser.go:2492`）。类型 switch 的 `tag` 是
    /// [`Expr::TypeSwitchGuard`]（由 [`Parser::simple_stmt`] 填好）。
    pub(crate) fn switch_stmt(&mut self) -> Stmt {
        let pos = self.tok.pos;
        let (init, tag, _) = self.header(StmtCtx::Switch);

        if !self.got(&LBRACE) {
            let p = self.tok.pos;
            self.syntax_error_at(
                p,
                DiagCode::SynUnexpectedToken,
                "missing { after switch clause",
            );
            self.advance(CASE_DEFAULT_RBRACE_FOLLOW);
        }

        let mut cases: Vec<CaseClause> = Vec::new();
        // ⚠️ 终止条件是 `_EOF` **与** `_Rbrace` 两个 —— 少了 `_EOF` 会在畸形输入上
        // 死循环（`caseClause` 自己不会消费 EOF）。
        while !self.at(&TokenKind::Eof) && !self.at_follow(&RBRACE) {
            cases.push(self.case_clause());
        }
        let rbrace = self.tok.pos;
        self.want(&RBRACE);

        Stmt::Switch(SwitchStmt {
            init,
            tag,
            cases,
            rbrace,
            pos,
        })
    }

    /// 官方 `selectStmt`（`parser.go:2515`）。
    pub(crate) fn select_stmt(&mut self) -> Stmt {
        let pos = self.tok.pos;
        // ⚠️ 与 `switchStmt` 不同：`select` 没有 `header`，所以这个 `want(_Select)`
        // 在这里**显式**写（官方亦然）。
        self.want(&SELECT);

        if !self.got(&LBRACE) {
            let p = self.tok.pos;
            self.syntax_error_at(
                p,
                DiagCode::SynUnexpectedToken,
                "missing { after select clause",
            );
            self.advance(CASE_DEFAULT_RBRACE_FOLLOW);
        }

        let mut clauses: Vec<CommClause> = Vec::new();
        while !self.at(&TokenKind::Eof) && !self.at_follow(&RBRACE) {
            clauses.push(self.comm_clause());
        }
        let rbrace = self.tok.pos;
        self.want(&RBRACE);

        Stmt::Select(SelectStmt {
            clauses,
            rbrace,
            pos,
        })
    }

    /// 官方 `caseClause`（`parser.go:2537`）。
    ///
    /// `cases == None` ⟺ `default`（官方 `c.Cases == nil`）。
    pub(crate) fn case_clause(&mut self) -> CaseClause {
        let pos = self.tok.pos;
        let mut cases: Option<Expr> = None;

        if self.at(&TokenKind::Keyword(Keyword::Case)) {
            self.next();
            // 多值 `case a, b:` ⇒ 一个 `ListExpr`（官方 `p.exprList()`）——
            // **不**拆成 Vec：这里官方要的就是"一个表达式"。
            cases = Some(self.expr_list());
        } else if self.at(&TokenKind::Keyword(Keyword::Default)) {
            self.next();
        } else {
            let p = self.tok.pos;
            self.syntax_error_at(
                p,
                DiagCode::SynUnexpectedToken,
                "expected case or default or }",
            );
            self.advance(COLON_CASE_DEFAULT_RBRACE_FOLLOW);
        }

        // 官方 `c.Colon = p.pos()` 在 `want(_Colon)` **之前** —— 于是即使冒号缺失，
        // `Colon` 也记在"本该是冒号的那个词元"上（而不是 `want` 消费之后的位置）。
        let colon = self.tok.pos;
        self.want(&COLON);
        let body = self.stmt_list();

        CaseClause {
            cases,
            body,
            colon,
            pos,
        }
    }

    /// 官方 `commClause`（`parser.go:2565`）。`comm == None` ⟺ `default`。
    ///
    /// 官方注释点明：这里**不**校验通讯语句的形状（`lhs <- x` / `<-x` / `lhs = <-x` /
    /// `lhs := <-x`）—— `simpleStmt` 认得出更宽的一类，不合法的那种留给类型检查报。
    /// 照抄（在这里加校验会让诊断流多出官方没有的条目 ⇒ 对撞假红）。
    pub(crate) fn comm_clause(&mut self) -> CommClause {
        let pos = self.tok.pos;
        let mut comm: Option<SimpleStmt> = None;

        if self.at(&TokenKind::Keyword(Keyword::Case)) {
            self.next();
            comm = Some(self.simple_stmt(None, StmtCtx::Plain));
        } else if self.at(&TokenKind::Keyword(Keyword::Default)) {
            self.next();
        } else {
            let p = self.tok.pos;
            self.syntax_error_at(
                p,
                DiagCode::SynUnexpectedToken,
                "expected case or default or }",
            );
            self.advance(COLON_CASE_DEFAULT_RBRACE_FOLLOW);
        }

        let colon = self.tok.pos;
        self.want(&COLON);
        let body = self.stmt_list();

        CommClause {
            comm,
            body,
            colon,
            pos,
        }
    }

    /// 官方 `labeledStmtOrNil`（`parser.go:2266`）。
    ///
    /// # `pos` 取的是**冒号**，不是标签
    ///
    /// 官方 `s.pos = p.pos()` 在 `p.want(_Colon)` **之前** ⇒ `LabeledStmt.pos` 是
    /// 那个 `:` 的位置（标签自己的位置在 `s.Label.pos` 里）。这正是官方的
    /// `nodes.go` 约定，别"顺手"改成标签位置。
    ///
    /// # 返回 `Option`（官方会返回 `nil`）
    ///
    /// 官方末尾有一条 `return nil`（`missing statement after label` 之后），
    /// 目的是**避免连锁错误**（`fixedbugs/bug274.go`）。我们的 `Stmt` 无 `Bad/*`
    /// 变体 ⇒ 用 `None` 表达，与官方 `return nil` 同义。
    pub(crate) fn labeled_stmt_or_nil(&mut self, label: Name) -> Option<Stmt> {
        let pos = self.tok.pos;
        self.want(&COLON);

        // 官方 `:2277-2285`：`L: }` —— 因为 `}` 前的分号可省，所以这是**空语句**。
        if self.at_follow(&RBRACE) {
            let e = EmptyStmt { pos: self.tok.pos };
            return Some(Stmt::Labeled(LabeledStmt {
                label,
                stmt: Box::new(Stmt::Empty(e)),
                pos,
            }));
        }

        match self.stmt_or_nil() {
            Some(s) => Some(Stmt::Labeled(LabeledStmt {
                label,
                stmt: Box::new(s),
                pos,
            })),
            None => {
                // 官方在 `:` 那一行报（`p.syntaxErrorAt(s.pos, ...)`），
                // 且**不 advance**（已经在被标注语句的末尾了）。
                self.syntax_error_at(
                    pos,
                    DiagCode::SynUnexpectedToken,
                    "missing statement after label",
                );
                None
            }
        }
    }

    // ── `T-AP14`：局部声明 / 分支 / 调用 / 返回 ────────────────────────────

    /// 官方 `declStmt`（`parser.go:2324`）。
    ///
    /// ```text
    /// Declaration = ConstDecl | TypeDecl | VarDecl .
    /// ```
    ///
    /// # `pos` 在 `next()` **之前**取
    ///
    /// 官方 `:2330-2332` 是 `s.pos = p.pos(); p.next()` ⇒ `DeclStmt.pos` 落在
    /// **`var`/`const`/`type` 关键字**上（`nodes_test.go` 的 `@const x = 0` 是权威判据）。
    /// 顺序反了会落在声明的第一个名字上，而**列差不报错**。
    ///
    /// # `DeclList` 是**列表**（官方 `nodes.go:389`）
    ///
    /// `appendGroup` 每读完一条声明就 `append` 一次 ⇒ `var (a int; b int)` 产出
    /// **2** 条 `VarDecl`。写成单条 `Decl` 的实现在这里静默丢掉后面那些。
    pub(crate) fn decl_stmt(&mut self, kind: DeclKind) -> DeclStmt {
        let pos = self.tok.pos;
        self.next(); // _Const, _Type, or _Var
        // 官方 `:2333`：`p.appendGroup(nil, f)` —— `nil` 列表由 `appendGroup` 自己建。
        let mut decls = Vec::new();
        self.append_group(&mut decls, kind);
        DeclStmt { decls, pos }
    }

    /// 官方 `:2666-2671` 的 `_Fallthrough` 臂。
    ///
    /// # 它**不读标签**
    ///
    /// 官方这一臂只有 `s.pos = p.pos(); p.next(); s.Tok = _Fallthrough` ——
    /// 不像 `_Break`/`_Continue`/`_Goto` 那样调 `p.name()`。
    /// 这个差别**只有**在「`fallthrough` 后面跟一个名字」时才现形：
    /// 单独写 `fallthrough` 时多读的那个词元是分号，看不出差别；
    /// 写 `fallthrough L` 时 `L` 会留给 `stmtList` 去报 `at end of statement`。
    fn fallthrough_stmt(&mut self) -> Stmt {
        let pos = self.tok.pos;
        self.next();
        Stmt::Branch(BranchStmt {
            tok: BranchTok::Fallthrough,
            label: None,
            pos,
        })
    }

    /// 官方 `:2673-2681` 的 `_Break`/`_Continue` 臂。
    ///
    /// # `pos` 与 `Tok` 都取在 `next()` **之前**
    ///
    /// 官方 `:2675-2676`：`s.pos = p.pos(); s.Tok = p.tok` 然后才 `p.next()`
    /// ⇒ `pos` 落在 `break`/`continue` **关键字**上（`nodes_test.go` 的 `@break`）。
    ///
    /// # 标签是**可选**的：`if p.tok == _Name { s.Label = p.name() }`
    ///
    /// 用 [`Follow::Name`]（**类别**）而不是 `TokenKind::Ident(_)` —— 两者等价，
    /// 但官方写的是 `p.tok == _Name`，跟着写同一个判据更不容易在将来分叉。
    fn branch_stmt(&mut self, tok: BranchTok) -> Stmt {
        let pos = self.tok.pos;
        self.next();
        let label = if self.at_follow(&Follow::Name) {
            Some(self.name())
        } else {
            None
        };
        Stmt::Branch(BranchStmt { tok, label, pos })
    }

    /// 官方 `:2686-2692` 的 `_Goto` 臂。
    ///
    /// # 标签是**必填**的：`s.Label = p.name()`（无条件）
    ///
    /// 与 [`Parser::branch_stmt`] 的「有名字才读」相反 —— 这里**总是**调 `name()`。
    ///
    /// 缺标签时 [`Parser::name`] 走它自己的恢复：报 `expected name` + `advance()`，
    /// 然后**造一个 `"_"` 假名**（官方 `NewName(p.pos(), "_")`）⇒
    /// 树里出现 `Label: _` 而不是 `None`。
    ///
    /// ⚠️ 这正是「假标签会让树形静默变样」的形状：实现若图省事改成
    /// 「缺名字就 `None`」，诊断**照报**、树形也**照能用**，
    /// 只有"树上多一个 `_`"这一层差别——而它恰好是 oracle 对撞会逐字段 diff 的。
    fn goto_stmt(&mut self) -> Stmt {
        let pos = self.tok.pos;
        self.next();
        Stmt::Branch(BranchStmt {
            tok: BranchTok::Goto,
            label: Some(self.name()),
            pos,
        })
    }

    /// 官方 `callStmt`（`parser.go:985`）。
    ///
    /// ```text
    /// GoStmt     = "go" Expression .
    /// DeferStmt  = "defer" Expression .
    /// ```
    ///
    /// # `keep_parens` 的唯一用途是下面那条报错
    ///
    /// 官方 `:995` 传 `p.tok == _Lparen`（注释写明 `keep_parens so we can report
    /// error below`）—— 于是 `go (f())` 能保住括号，报错才知道报在哪。
    /// 传 `false` 的实现**不会报错**，`go (f())` 会被静默剥掉括号。
    ///
    /// ⚠️ 判据是「**`go`/`defer` 之后**的第一个词元是不是 `(`」——
    /// 官方 `:993` 的 `p.next()` 在 `:995` 的判断**之前**，所以 `pexpr` 的起点
    /// 已经被推进过了。
    ///
    /// # 报错走 `errorAt`（**不是** `syntaxErrorAt`）
    ///
    /// 官方 `:997` 是 `p.errorAt(...)` ⇒ 消息**没有** `syntax error: ` 前缀，
    /// 也**不受** EOF 抑制（`F5`）。写成 `syntax_error_at` 会多一个前缀，
    /// 而测试断言若照着前缀写就变成恒真。
    ///
    /// # `%s` 是 `s.Tok`（`go` / `defer`）—— **不能写死**
    ///
    /// `defer (f())` 的文案是 `expression in defer must not be parenthesized`。
    fn call_stmt(&mut self, tok: CallTok) -> Stmt {
        let pos = self.tok.pos;
        self.next(); // _Defer or _Go
        let x = self.pexpr(None, self.at_follow(&LPAREN));
        // 官方 `:996-1000`：`if t := Unparen(x); t != x { ...; x = t }`
        // —— 报错后**已经前进过了**，所以官方特意注明 `no need to advance`。
        let t = unparen(x.clone());
        let call = if t != x {
            self.error_at(
                x.pos(),
                DiagCode::SynUnexpectedToken,
                format!(
                    "expression in {} must not be parenthesized",
                    call_tok_str(tok)
                ),
            );
            t
        } else {
            x
        };
        Stmt::Call(CallStmt { tok, call, pos })
    }

    /// 官方 `:2694-2701` 的 `_Return` 臂。
    ///
    /// # 条件是**两个**：`if p.tok != _Semi && p.tok != _Rbrace { s.Results = p.exprList() }`
    ///
    /// `_Rbrace` 那半个条件容易被漏掉：`return` 后面**紧跟** `}`（同一行、没有自动分号）
    /// 时若仍去读 `exprList()`，就会把 `}` 当表达式解析、报 `expected expression`。
    /// `_Semi` 用 [`Follow::Semi`]（**类别**）：显式写的、换行补的、文件尾补的
    /// 在官方是同一个 token，按值判会漏掉后两者。
    ///
    /// # `Results` 是**一个** `Expr`（多值时是 `ListExpr`），Rust 侧是 `Vec`
    ///
    /// 与 `AssignStmt.Lhs/Rhs` 同一套 C6 归一，见 [`exprs_of`] 的说明。
    fn return_stmt(&mut self) -> Stmt {
        let pos = self.tok.pos;
        self.next();
        let results = if self.at_follow(&SEMI) || self.at_follow(&RBRACE) {
            Vec::new()
        } else {
            exprs_of(self.expr_list())
        };
        Stmt::Return(ReturnStmt { results, pos })
    }
}

/// `CallTok` 的**关键字**拼写（官方 `s.Tok` 打印出来的样子）。
///
/// 官方 `fmt.Sprintf("expression in %s must not be parenthesized", s.Tok)`
/// 里的 `%s` 走的是 `token` 的 `String()` —— 也就是 `go` / `defer` 两个关键字本身。
/// 写死 `"go"` 的实现会在 `defer (f())` 上给出**错误文案**而不报错
/// （消息断言仍然过 —— 因为两边都在"报了错"，只是错得不一样）。
fn call_tok_str(t: CallTok) -> &'static str {
    match t {
        CallTok::Go => "go",
        CallTok::Defer => "defer",
    }
}

/// `SimpleStmt` → `Stmt` 的**提升**（官方那边不需要：`SimpleStmt` 是 `Stmt` 的子接口）。
///
/// # 为什么这个函数存在（我们与官方的**类型系统差异**）
///
/// 官方 `type SimpleStmt interface { stmt() }`、`Stmt interface { ... }`，
/// 而所有 `SimpleStmt` 实现也都实现 `Stmt` ⇒ 一个 `SimpleStmt` 值**直接**就是 `Stmt`
/// （`stmtOrNil` 里写 `return p.simpleStmt(lhs, 0)` 就够了）。
/// Rust 侧 `SimpleStmt` 是独立 enum（`M2-design.md` §3.2 的刻意选择：`if`/`for`/`switch`
/// 的 init 位只该收那四种），所以需要一个显式提升。
///
/// ⚠️ **两个 `unreachable!` 都是不变式，不是兜底**：
///
/// - `Range`：`range` 子句**不是**语句（官方只在 `header` 里把它当 `ForStmt.Init`）。
/// - `Empty`：`simpleStmt` 从不产出它（官方 `:2703` 的 `_Semi` 臂才产出）。
///
/// 给兜底（比如"当作 ExprStmt"）会让"谁把它塞进来的"永远查不出来 —— 正是
/// `AGENTS.md` 记的「漏了不会报错」形状。
///
/// 是**自由函数**而不是 `&self` 方法：调用点写成
/// `stmt_of_simple(self.simple_stmt(...))`，若收 `&self` 会与 `&mut self` 撞借用。
fn stmt_of_simple(s: SimpleStmt) -> Stmt {
    match s {
        SimpleStmt::Expr(x) => Stmt::Expr(x),
        SimpleStmt::Send(x) => Stmt::Send(x),
        SimpleStmt::Assign(x) => Stmt::Assign(x),
        SimpleStmt::Range(_) => {
            unreachable!("`range` 子句不是语句（官方只在 `header` 里用它当 `ForStmt.Init`）")
        }
        SimpleStmt::Empty => {
            unreachable!("`simpleStmt` 不产出 EmptyStmt（官方 `:2703` 的 `_Semi` 臂才产出它）")
        }
    }
}

/// 官方 `AssignStmt.Lhs`/`Rhs` 是**一个** `Expr`（多值时是 `ListExpr`）；
/// 我们的 AST 用 `Vec<Expr>`（见 `decl.rs` 里「`Expr` 还是 `Vec<Expr>`」那条 C6 归一说明）。
///
/// ⚠️ 反过来（`Vec` → `ListExpr`）**不需要**：官方造 `ListExpr` 的唯一入口是
/// `exprList`，而它要求至少一个逗号 ⇒ **单元素列表不存在**。所以这里拆包不会丢信息。
fn exprs_of(x: Expr) -> Vec<Expr> {
    match x {
        Expr::List(l) => l.elems,
        other => vec![other],
    }
}

/// `+=` 这类复合赋值 → [`AssignOp`]。
///
/// `unreachable!` 的是**词法层不变式**：`OpForm::Assign` 只有 11 个成员
/// （`+=` `-=` `*=` `/=` `%=` `&=` `|=` `^=` `<<=` `>>=` `&^=`，见 `token.rs`）。
/// 写成 `_ => AssignOp::Assign` 会把"lexer 变了但这里没跟"变成**静默的错误分类**。
fn compound_assign_op(op: Operator) -> AssignOp {
    match op {
        Operator::Add => AssignOp::Add,
        Operator::Sub => AssignOp::Sub,
        Operator::Mul => AssignOp::Mul,
        Operator::Div => AssignOp::Quo,
        Operator::Rem => AssignOp::Rem,
        Operator::And => AssignOp::And,
        Operator::Or => AssignOp::Or,
        Operator::Xor => AssignOp::Xor,
        Operator::Shl => AssignOp::Shl,
        Operator::Shr => AssignOp::Shr,
        Operator::AndNot => AssignOp::AndNot,
        other => unreachable!("`OpForm::Assign` 只有 11 个成员，got {other:?}"),
    }
}

/// `++` / `--` → [`AssignOp::Inc`] / [`AssignOp::Dec`]。
///
/// 词法层把 `++` 记成 `(IncDec, Add)`、`--` 记成 `(IncDec, Sub)`（`token.rs` 的
/// 「`++` 没有自己的 `Operator`」决定）⇒ 这里靠 `Add`/`Sub` 分辨。官方则直接是
/// `Inc`/`Dec` 两个 `Operator` 变体（`tokens.go`），这也是"同一件事两边形状不同"的一例。
fn incdec_op(op: Operator) -> AssignOp {
    match op {
        Operator::Add => AssignOp::Inc,
        Operator::Sub => AssignOp::Dec,
        other => unreachable!("`++`/`--` 的 op 只可能是 Add/Sub，got {other:?}"),
    }
}

/// 分号词元的**来源** → 官方的 `p.lit` 三档（`"semicolon"` / `"newline"` / `"EOF"`）。
///
/// 非分号词元返回 `None`（调用方只在 `at(&SEMI)` 成立时问它）。
fn semi_origin_of(k: &TokenKind) -> Option<SemiOrigin> {
    match k {
        TokenKind::Semi(o) => Some(*o),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use rgoc_lex::token::{Delim, Keyword};

    use super::*;
    use crate::parser::ErrMode;

    fn parser(src: &[u8]) -> Parser {
        Parser::new(src, "t.go", ErrMode::Continue)
    }

    fn n_empty(l: &[Stmt]) -> usize {
        l.iter().filter(|s| matches!(s, Stmt::Empty(_))).count()
    }

    // ── `stmtList` 的终止条件（官方 `:2718`：`_EOF` / `_Rbrace` / `_Case` / `_Default`）──

    /// `}` 处立刻收尾，且**一个词元都不消费**（停在 `}` 上，交回调用方 `want(_Rbrace)`）。
    #[test]
    fn stmt_list_空块返回空表() {
        let mut p = parser(b"}");
        let before = p.tok.pos;
        assert!(p.stmt_list().is_empty());
        assert_eq!(p.tok.pos, before, "`stmtList` 不该吃掉 `}}`");
    }

    /// `stmtList` 的**终止词元集合**逐条对官方 `:2718`。
    ///
    /// ⚠️ **为什么用集合断言，而不是行为断言**：`_Case` / `_Default` 作为终止词元
    /// 在**行为上不可观测** —— `stmtOrNil` 对它们本来就返回 `None`，`stmtList` 同样会
    /// `break`。所以「少写一个终止词元」不会让任何行为测试变红。
    /// 只有断言**集合本身**才钉得住（否则就是 `AGENTS.md` 记的「漏了不会报错」）。
    /// **反向**部分同样重要：混进不相干的终止词元会**提前收尾**、静默吃掉语句。
    #[test]
    fn stmt_list_终止词元集合与官方一致() {
        assert_eq!(STMT_LIST_END.len(), 4);
        assert!(STMT_LIST_END.contains(&TokenKind::Eof));
        assert!(STMT_LIST_END.contains(&TokenKind::Delim(Delim::RBrace)));
        assert!(STMT_LIST_END.contains(&TokenKind::Keyword(Keyword::Case)));
        assert!(STMT_LIST_END.contains(&TokenKind::Keyword(Keyword::Default)));
        for kw in [Keyword::Else, Keyword::Func, Keyword::Var, Keyword::Return] {
            assert!(
                !STMT_LIST_END.contains(&TokenKind::Keyword(kw)),
                "{kw:?} 不该是终止词元（会静默吃掉语句）"
            );
        }
    }

    /// 终止词元在**行为上**的效果：一个都不消费，也一个语句都不产出。
    /// （集合本身由上面那条钉；这条只钉"看到它们就收尾"这一半。）
    #[test]
    fn stmt_list_在终止词元处收尾且不消费() {
        for src in ["}", "case 1:", "default:"] {
            let mut p = parser(src.as_bytes());
            let before = p.tok.pos;
            assert!(p.stmt_list().is_empty(), "`{src}` 处应收尾");
            assert_eq!(p.tok.pos, before, "`{src}`：一个词元都不该被消费");
        }
    }

    /// EOF 处收尾（空文件）。
    #[test]
    fn stmt_list_在_eof_处停() {
        let mut p = parser(b"");
        assert!(p.stmt_list().is_empty());
    }

    // ── `stmtOrNil` 的两条本任务就实现的分派臂 ────────────────────────────────

    /// `;` → `EmptyStmt`，且**不消费**这个分号（官方 `:2703-2706` **不调** `p.next()`）——
    /// 留给 `stmtList` 的 `p.got(_Semi)` 收尾。消费了就少一条语句。
    #[test]
    fn stmt_or_nil_空语句不消费分号() {
        let mut p = parser(b"; x");
        let semi_pos = p.tok.pos;
        let s = p.stmt_or_nil().expect("`;` 是合法语句起始");
        match s {
            Stmt::Empty(e) => assert_eq!(e.pos, semi_pos, "空语句的 pos 是那个 `;`"),
            other => panic!("期望 EmptyStmt，got {other:?}"),
        }
        assert!(
            matches!(p.tok.kind, TokenKind::Semi(_)),
            "分号必须还在（由 stmtList 收尾），got {:?}",
            p.tok.kind
        );
    }

    /// 认不出的语句起始词元 → `None`，且**不报错**（官方 `:2709` 的 `return nil`）。
    ///
    /// 「不报错」是这条断言的重点：错误由调用方报（`stmtList` 的收尾检查 /
    /// `blockStmt` 的 `want(_Rbrace)`）。在这里顺手报一条，会让诊断流多出
    /// oracle 没有的条目 ⇒ 对撞假红。
    #[test]
    fn stmt_or_nil_认不出时返回_none_且不报错() {
        let mut p = parser(b"}");
        assert!(p.stmt_or_nil().is_none());
        assert_eq!(p.errcnt, 0, "官方 `return nil` 不报错");
    }

    // ── `stmtList` 与恢复点 ②（官方 `:2726-2730`）────────────────────────────

    /// 连续分号：`; ; }` ⇒ **两条**空语句（`;` 不消费 + `stmtList` 收尾各吃一个）。
    #[test]
    fn stmt_list_连续分号得到两条空语句() {
        let mut p = parser(b"; ; }");
        let l = p.stmt_list();
        assert_eq!(l.len(), 2, "两条空语句，got {l:?}");
        assert_eq!(n_empty(&l), 2);
        assert_eq!(p.errcnt, 0, "分号齐全时不该报错");
    }

    /// 语句间缺分号 ⇒ 报一条 + **恢复点 ②** 把词元啃到同步点（`}`）。
    #[test]
    fn stmt_list_缺分号报错并用恢复点二同步() {
        // `{ }` 是一个合法的块语句，紧跟 `x` 而没有分号
        let mut p = parser(b"{ } x }");
        let l = p.stmt_list();
        assert_eq!(l.len(), 1, "只有那个块语句，got {l:?}");
        assert!(matches!(l[0], Stmt::Block(_)));
        assert_eq!(p.errcnt, 1, "缺分号应恰好报一条");
        assert_eq!(
            p.tok.kind,
            TokenKind::Delim(Delim::RBrace),
            "恢复点 ② 应停在 `}}`"
        );
    }

    /// 恢复点 ② 在 `;` 上同步 —— 这里用的是**显式**分号。
    #[test]
    fn stmt_list_恢复点二在分号上停() {
        let mut p = parser(b"{ } x ; }");
        let l = p.stmt_list();
        assert_eq!(l.len(), 1, "got {l:?}");
        assert_eq!(p.errcnt, 1);
    }

    /// **换行自动补出的分号也算分号**。
    ///
    /// 这是 `recovery::Follow::Semi` 这个**类别**存在的理由：若 `got(_Semi)` 按值比较
    /// （只认 `SemiOrigin::Explicit`），这里会被误判成「缺分号」而报一条错。
    /// 而 Go 里**绝大多数分号都是换行自动补的** —— 这种漏法在真实语料上必然发生。
    #[test]
    fn stmt_list_换行自动补的分号被认作分号() {
        let mut p = parser(b"{ }\n}");
        let l = p.stmt_list();
        assert_eq!(l.len(), 1, "got {l:?}");
        assert_eq!(p.errcnt, 0, "`}}`+换行会自动补一个分号，不该报「缺分号」");
    }

    // ── `blockStmt`（官方 `:2299`）───────────────────────────────────────────

    /// 空块 `{ }`：空 `stmts`，`pos` 在 `{`，`rbrace` 在 `}`（**两者必须不同**）。
    #[test]
    fn block_stmt_空块() {
        let mut p = parser(b"{ }");
        let b = p.block_stmt("");
        assert!(b.stmts.is_empty(), "got {:?}", b.stmts);
        assert_eq!(b.pos.line, 1);
        assert_eq!(b.rbrace.line, 1);
        assert!(b.rbrace.col > b.pos.col, "`}}` 在 `{{` 右边");
    }

    /// `Rbrace` 真的是**右花括号**的位置（跨行验证，避免只看列而猜错）。
    #[test]
    fn block_stmt_右花括号位置跨行() {
        let mut p = parser(b"{\n}");
        let b = p.block_stmt("");
        assert_eq!(b.pos.line, 1, "`pos` 在 `{{` 那一行");
        assert_eq!(b.rbrace.line, 2, "`rbrace` 在 `}}` 那一行");
    }

    /// 嵌套块 `{ { } }` ⇒ 1 条语句，且它是空块。这条同时证明
    /// `stmtOrNil` 的 `_Lbrace` 臂**接进了 `stmtList`**（而不是各自单独可用）。
    #[test]
    fn block_stmt_嵌套块() {
        let mut p = parser(b"{ { } }");
        let outer = p.block_stmt("");
        assert_eq!(
            outer.stmts.len(),
            1,
            "外层只有内层那一个块，got {:?}",
            outer.stmts
        );
        match &outer.stmts[0] {
            Stmt::Block(inner) => assert!(inner.stmts.is_empty(), "内层是空块"),
            other => panic!("期望嵌套块，got {other:?}"),
        }
    }

    /// 缺 `{` ⇒ 报一条 `expected { after <context>`，并按
    /// `advance(_Name, _Rbrace)` 同步（官方 `:2308-2315`）。
    /// 这里喂一个立刻落在同步点上的 `}`，走官方的 `return s` 早退分支。
    #[test]
    fn block_stmt_缺左括号时报错并按_name_rbrace_同步() {
        let mut p = parser(b" }");
        let b = p.block_stmt("for clause");
        assert!(b.stmts.is_empty());
        assert_eq!(p.errcnt, 1, "缺 `{{` 应恰好报一条");
        let first = p.first.as_ref().expect("首错必须留下");
        assert!(
            first.message.contains("expected { after for clause"),
            "消息要带上 context，got {:?}",
            first.message
        );
    }

    // ── `funcBody` 的 `fnest` 记账（官方 `:845`）─────────────────────────────
    //
    // 为什么这条放在 crate 内：`fnest` 的**唯一可观测后果**是
    // `advance` 的 stopset 分支（`recovery.rs` 的 `self.fnest > 0 && at_stopset()`），
    // 而要从**文件入口**进到「函数体里出了错、且错误后面紧跟 stopset 关键字」
    // 这个状态，得先走完 `funcDecl` → `funcBody` → 出错 → 再恢复 —— 那条链上
    // 任何一环变了都会连带影响这条用例的**诊断条数**。crate 内直接构造
    // `parser(b"{ {} return }")` 才让「只有 `fnest` 不同」这个对照干净。

    /// **A/B 对照**：同一个源串，**只有 `fnest` 不同** —— `fnest` 是 `advance`
    /// 的 stopset 分支的**唯一**开关（官方 `:354`：`if p.fnest > 0 { followset |= stopset }`）。
    ///
    /// ```text
    /// 源:  { {} return }
    ///        ^^ 一个块语句；紧跟 `return` 而**没有分号** ⇒ 报一条 + 恢复点 ②
    /// ```
    ///
    /// - `fnest == 0`（`block_stmt` 直接调）：恢复点 ② 的 followset
    ///   `[SEMI, RBRACE, Case, Default]` 里**没有** `return` ⇒ 啃掉它、停在 `}` 上
    ///   ⇒ 语句列表**只有 1 条**（那个块）。
    /// - `fnest > 0`（`func_body`）：`return` 是 stopset 关键字 ⇒ 恢复**原地停住**
    ///   ⇒ 语句循环**又去解**它、这次 `_Return` 臂已实现 ⇒ 解析成 `ReturnStmt`
    ///   ⇒ 语句列表**有 2 条**。
    ///
    /// # 为什么判据是「条数」而不是「炸没炸」（RF4）
    ///
    /// `T-AP14` 之前这条用例断言的是「撞上未实现的 `return` 臂会 panic」——
    /// **那是一条靠 panic 通过的测试**：`return` 臂一旦实现，它必然失败，
    /// 而失败原因与它真正要验的东西（`fnest` 有没有生效）**无关**。
    /// 现在改成 RF4 的形态：断言**报错之后的剩余 AST 仍然正确**
    /// （B 档第二条语句必须是 `ReturnStmt`），这才是 `fnest` 生效的正面证据。
    ///
    /// ⚠️ A/B **两个方向**都要断言。只断言 B 档「解析出 `ReturnStmt`」的话，
    /// 一个「stopset 在 `fnest == 0` 时也生效」的实现同样能过（两条都在）——
    /// 差异被抹平了。这正是 `AGENTS.md` 记的「漏了不会报错」的**镜像**：
    /// **多了也不会报错**。
    #[test]
    fn func_body_体内_fnest_生效_恢复停在_stopset_上() {
        // A 档：函数体外 ⇒ 恢复跳过 `return`，块正常收尾（**不炸**就是证据：
        // `return` 若留在流里，循环会再去解它、并把它算成第二条语句）。
        let mut q = parser(b"{ {} return }");
        let b = q.block_stmt("");
        assert_eq!(b.stmts.len(), 1, "只有内层块那一条，got {:?}", b.stmts);
        assert!(matches!(b.stmts[0], Stmt::Block(_)));
        assert_eq!(q.errcnt, 1, "缺分隔符报一条");
        let first = q.first.as_ref().expect("首错要留下");
        assert!(
            first.message.contains("at end of statement"),
            "走的是恢复点 ② 那条路径，got {:?}",
            first.message
        );
        assert!(
            matches!(q.tok.kind, TokenKind::Semi(_)),
            "`return` 与收尾的 `}}` 都被吃掉 ⇒ 只剩文件尾补的分号，got {:?}",
            q.tok.kind
        );

        // B 档：函数体内 ⇒ 恢复**停在** `return`（stopset）上，语句循环于是又去解它，
        // 而 `_Return` 臂已实现（`T-AP14`）⇒ 它成了第二条语句。
        //
        // ⚠️ 这里**不许**再回到「有没有 panic」的判据上：`fnest` 是 `usize`，
        // 少了 `fnest += 1` 的话出口的 `fnest -= 1` 会在 0 上**下溢**
        // （debug 档 panic）—— 只看「炸了」的断言会被这个**完全无关**的 panic
        // 满足（`M10` 变异下实测过这条假绿）。现在断言的是**条数**与**变体**，
        // 下溢会让函数直接 panic、根本到不了断言 —— 两种失败都暴露得出来。
        let mut p = parser(b"{ {} return }");
        let b = p.func_body();
        assert_eq!(
            b.stmts.len(),
            2,
            "`fnest > 0` 时恢复点 ② 必须停在 stopset（`return`）上 ⇒ 它是第二条语句，got {:?}",
            b.stmts
        );
        assert!(matches!(b.stmts[0], Stmt::Block(_)), "got {:?}", b.stmts[0]);
        // RF4：报错**之后**的 AST 仍然正确 —— `return` 被解成了 `ReturnStmt`
        // （而不是 `None`、也不是被吞掉）。
        match &b.stmts[1] {
            Stmt::Return(r) => {
                assert!(
                    r.results.is_empty(),
                    "`return` 后面紧跟 `}}` ⇒ 结果表为空，got {:?}",
                    r.results
                );
            }
            other => panic!("第二条必须是 ReturnStmt（RF4），got {other:?}"),
        }
        assert_eq!(p.errcnt, 1, "仍然只报一条（`at end of statement`）");
    }

    /// 出口恢复的是**入口值** —— 不是硬编码的 `0`。
    ///
    /// 入口取 `3`（而不是默认的 `0`）：写成 `self.fnest = 0` 的实现会在 `fnest == 0`
    /// 的用例上**看起来完全正确**，只有非零入口才分得出来。
    #[test]
    fn func_body_出口恢复入口的_fnest() {
        let mut p = parser(b"{}");
        p.fnest = 3;
        let b = p.func_body();
        assert!(b.stmts.is_empty(), "空函数体");
        assert_eq!(p.fnest, 3, "出口必须回到入口值（`fnest--` 不能丢）");
    }
}
