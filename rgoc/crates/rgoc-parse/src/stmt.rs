//! 语句解析（`M2-design.md` §5.2）：`stmtOrNil` 分派 + `stmtList` + `blockStmt`。
//!
//! 对齐官方 `parser.go` 的 `stmtOrNil`（`:2610`）/ `stmtList`（`:2713`）/
//! `blockStmt`（`:2299`）。
//!
//! # 为什么这三个函数住在一起
//!
//! 它们共用同一套「语句边界」约定：`stmtOrNil` 决定**一条**语句从哪开始、
//! `stmtList` 决定到哪结束（含恢复点 ②）、`blockStmt` 决定它们被哪对花括号包住。
//! 拆到三个文件会让「语句到哪结束」这件事散落 —— 而散落的症状是
//! **某一条路径上的恢复行为与 oracle 分叉**，最难定位。
//!
//! # `stmtOrNil` 目前是**骨架**
//!
//! 官方 `stmtOrNil` 有 10 组分派臂，其中大部分要等依赖到位：表达式族
//! （`T-AP10`..`T-AP12`）、简单语句（`T-AP13`）、其余语句（`T-AP14`）、局部声明（`T-AP08`）。
//! 本任务把**完整的分派骨架**摆出来（臂的顺序与官方 `switch p.tok` 逐条一致 ——
//! 改顺序会改行为），能独立实现的（`;` → `EmptyStmt`、`{` → `BlockStmt`）**实现**，
//! 其余臂体是 [`Parser::pending`]。
//!
//! 这样后续任务填的是一个**已经在正确位置的空臂**，而不是「该往哪儿插这条分派」——
//! 后者每插一次都要重新核对官方顺序，是分叉的温床。

use rgoc_ast::{BlockStmt, EmptyStmt, Stmt};
use rgoc_lex::diag::DiagCode;
use rgoc_lex::token::{Delim, Keyword, OpForm, Operator, TokenKind};

use crate::parser::Parser;
use crate::recovery::{BLOCK_STMT_FOLLOW, LBRACE, RBRACE, SEMI, STMT_LIST_FOLLOW};

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
    #[allow(dead_code)] // 生产入口要等 `T-AP09` 的 `funcBody`（现在只有 `stmt_list` 与测试调它）
    pub(crate) fn stmt_or_nil(&mut self) -> Option<Stmt> {
        // 官方先探 `_Name`：大部分语句（赋值）以标识符开头，先探它可避免走更贵的分派。
        // ⚠️ 本臂要 `exprList`（`T-AP10`）+ `labeledStmtOrNil`/`simpleStmt`（`T-AP13`）。
        if matches!(self.tok.kind, TokenKind::Ident(_)) {
            return Some(self.pending(
                "stmtOrNil 的 `_Name` 臂：exprList → labeledStmtOrNil | simpleStmt（T-AP13）",
            ));
        }

        // 克隆一次当前词元，**结束对 `self.tok` 的借用**：`match &self.tok.kind` 会把整个
        // match 借住，而臂体要 `&mut self`（`block_stmt` 等）。每条语句复制一次 `TokenKind`
        // （含 `Vec<u8>` 载荷）在本阶段没有意义 —— M2 的判据是**结构对齐**，不是性能
        // （`M2-design.md` §2.4 取舍 2）。这比再造一层「分派键枚举」更不容易与官方分叉。
        match self.tok.kind.clone() {
            // ── 本任务**实现**的两条臂 ────────────────────────────────────────
            TokenKind::Delim(Delim::LBrace) => Some(Stmt::Block(self.block_stmt(""))),

            // ⚠️ 官方**不消费**这个 `;`（`:2703-2706` 没有 `p.next()`）—— 留给 `stmtList`
            // 的 `p.got(_Semi)` 收尾。在这里消费会让每条空语句少一格。
            TokenKind::Semi(_) => Some(Stmt::Empty(EmptyStmt { pos: self.tok.pos })),

            // ── 待后续任务填实的臂（顺序 == 官方 `switch p.tok`）──────────────
            TokenKind::Keyword(Keyword::Var | Keyword::Const | Keyword::Type) => {
                Some(self.pending("stmtOrNil 的 declStmt 臂（T-AP08 的 appendGroup）"))
            }

            // 官方 `case _Operator, _Star:` 里只把自己当**一元前缀**的那几个算语句起始
            // （`+x` `-x` `*x` `&x` `^x` `!x`）。其余 `form == Plain` 的运算符
            // （如 `<`）**不匹配本臂**，会下落到后面的臂 —— 这正是官方 `switch p.op` 的行为。
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
            } => Some(self.pending("stmtOrNil 的一元运算臂 → simpleStmt（T-AP13）")),

            TokenKind::Literal { .. }
            | TokenKind::Op {
                op: Operator::Arrow,
                ..
            }
            | TokenKind::Keyword(Keyword::Func)
            | TokenKind::Delim(Delim::LParen | Delim::LBrack)
            | TokenKind::Keyword(
                Keyword::Struct | Keyword::Map | Keyword::Chan | Keyword::Interface,
            ) => Some(self.pending("stmtOrNil 的 operand 臂 → simpleStmt（T-AP13）")),

            TokenKind::Keyword(Keyword::For | Keyword::Switch | Keyword::Select | Keyword::If) => {
                Some(self.pending("stmtOrNil 的控制流臂 for/switch/select/if（T-AP13）"))
            }

            TokenKind::Keyword(Keyword::Fallthrough) => {
                Some(self.pending("fallthrough → BranchStmt（T-AP14）"))
            }
            TokenKind::Keyword(Keyword::Break | Keyword::Continue) => {
                Some(self.pending("break/continue → BranchStmt（T-AP14，含可选 label）"))
            }
            TokenKind::Keyword(Keyword::Go | Keyword::Defer) => {
                Some(self.pending("go/defer → CallStmt（T-AP14）"))
            }
            TokenKind::Keyword(Keyword::Goto) => {
                Some(self.pending("goto → BranchStmt（T-AP14，label 必填）"))
            }
            TokenKind::Keyword(Keyword::Return) => {
                Some(self.pending("return → ReturnStmt（T-AP14）"))
            }

            // 官方 `:2709` 的 `return nil` —— **不报错**，错误由调用方报。
            _ => None,
        }
    }

    /// 官方 `stmtList`（`parser.go:2713`）。
    #[allow(dead_code)] // 生产入口要等 `T-AP09` 的 `funcBody`
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
            if !self.got(SEMI) && !self.at(&TokenKind::Delim(Delim::RBrace)) {
                let pos = self.tok.pos;
                self.syntax_error_at(pos, DiagCode::SynExpectedSemicolon, "at end of statement");
                // 恢复点 ②（`parser.go:2728`）
                self.advance(STMT_LIST_FOLLOW);
                self.got(SEMI); // avoid spurious empty statement
            }
        }
        l
    }

    /// 官方 `blockStmt`（`parser.go:2299`）。
    #[allow(dead_code)] // 生产入口要等 `T-AP09` 的 `funcBody`
    pub(crate) fn block_stmt(&mut self, context: &str) -> BlockStmt {
        let mut s = BlockStmt {
            stmts: Vec::new(),
            rbrace: self.tok.pos, // 占位；下面按官方两处 `s.Rbrace = p.pos()` 覆盖
            pos: self.tok.pos,
        };

        // 官方注释：从 C 过来的人常忘了 Go 的花括号是必需的。
        if !self.got(LBRACE) {
            let pos = self.tok.pos;
            self.syntax_error_at(
                pos,
                DiagCode::SynUnexpectedToken,
                &format!("expected {{ after {context}"),
            );
            self.advance(BLOCK_STMT_FOLLOW);
            s.rbrace = self.tok.pos; // 官方 `s.Rbrace = p.pos()`（可能正好落在 `}`）
            if self.got(RBRACE) {
                return s; // 官方 `:2312-2314` 的早退
            }
        }

        s.stmts = self.stmt_list();
        // 官方在 `want` **之前**记 `s.Rbrace` —— 这样即使 `}` 缺失，位置也在树里。
        s.rbrace = self.tok.pos;
        self.want(RBRACE, "}");
        s
    }

    /// 尚未实现的产生式 —— 留一个**指名归属任务**的空臂。
    ///
    /// # 为什么不是 `None`
    ///
    /// `None` 在官方语义里是「这个语句起始词元**认不出来**」（`parser.go:2709`）。
    /// 拿 `None` 当"还没写"，会让骨架**静默**变正确 —— 正是 `AGENTS.md` 记的
    /// 「漏了不会报错」形状（这里是它的镜像：**没写的东西看起来像已经写对了**）。
    /// `todo!` 一碰就炸，且消息里带着归属任务，改起来不用再查计划。
    fn pending(&self, what: &str) -> Stmt {
        todo!("{what}")
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
}
