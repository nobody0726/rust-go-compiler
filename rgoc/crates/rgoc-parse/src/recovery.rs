//! 错误恢复 —— `advance` / `stopset` / `errh` 双档（对齐 `F2`–`F5`）。
//!
//! 设计依据：`M2-design.md` §5.3。核心只有一个函数 —— [`Parser::advance`]。
//!
//! # 为什么这四件事单独放一个文件
//!
//! 官方 `parser.go` 里 `advance` / `stopset` / `errorAt` / `syntaxErrorAt` 是四个
//! 互不相关的小函数，散落在 parser 主体中间。但它们合起来是**一个机制**：
//! 出错时「报什么、要不要停、跳到哪」。拆开散进 `parser.rs`，改其中一块很容易漏掉另一块 ——
//! 而漏掉的症状是「恢复位置与 oracle 分叉」，极难定位。

use std::sync::OnceLock;

use rgoc_lex::diag::{Diag, DiagCode};
use rgoc_lex::pos::Pos;
use rgoc_lex::token::{Delim, Keyword, OpForm, Operator, TokenKind};

use crate::parser::{ErrMode, Parser};

/// `stopset`：能**起头一个语句**的关键字（`F3`，官方 `parser.go:326-339`）。
///
/// 它们是错误恢复时的**同步点** —— 撞上就停下，通常不该跳过。
/// **只有在函数体内（`fnest > 0`）才并入 followset**：顶层的 `func` 也在这个集合里，
/// 若无条件并入，顶层恢复会一撞到 `func` 就停（那正是官方用 `fnest` 挡掉的）。
///
/// # 为什么类型写成 `[Keyword; 14]` 而不是 `&[Keyword]`
///
/// 「14」这个数曾经在文档里写错过（`M2-design.md` §14 勘误：初稿写 13，而列了 14 个）。
/// 写成固定长度数组，**「13」在类型上就不可表达** —— 数字再也错不了。
/// 成员的**集合**则由 `t_a_61_stopset_恰好十四个且与官方逐条一致` 钉住。
pub const STOPSET: [Keyword; 14] = [
    Keyword::Break,
    Keyword::Const,
    Keyword::Continue,
    Keyword::Defer,
    Keyword::Fallthrough,
    Keyword::For,
    Keyword::Go,
    Keyword::Goto,
    Keyword::If,
    Keyword::Return,
    Keyword::Select,
    Keyword::Switch,
    Keyword::Type,
    Keyword::Var,
];

/// followset 的一个成员。
///
/// # 为什么不能直接用 `TokenKind`
///
/// 官方 `advance(followlist ...token)` 里的 token 是**整数**，而它的词法把
/// 「所有标识符」折叠成一个 `_Name`、「所有分号」折叠成一个 `_Semi`（`tokens.go`）。
/// Rust 侧 [`TokenKind`] 把**载荷留着**（`Ident(Vec<u8>)` / `Semi(SemiOrigin)` /
/// `Op{form,op,prec}`），所以**没有任何一个 `TokenKind` 值能表示"任意标识符"** ——
/// 按值比较只能表达"具体词元"，表达不了"词元类别"。
///
/// # 为什么只有三个类别变体
///
/// 回源枚举 `parser.go` 里**全部 26 处** `p.advance(...)` 调用，出现在 followlist 里的
/// 类别词元**恰好三个**：`_Name`（`:2310` / `:2485`）、`_Semi`（`:212`…`:2808` 共十处）、
/// `_Operator`（`:1897`）。其余全是关键字与分隔符（无载荷，用 [`Follow::Kind`] 即可）。
/// **官方 `_Literal` 从未出现在任何 followlist 里**，所以不加 —— 按证据加，不按对称感加。
///
/// # 为什么住在 `rgoc-parse` 而不是 `rgoc-lex`
///
/// 「哪些 token 是同一个类别」是 `rgoc-lex` 的知识，但**这个枚举只为一个用途存在**：
/// 表达 parser 的 followset。放进 `rgoc-lex` 会把一个纯 parser 概念塞进 M1 的冻结 crate。
///
/// # 为什么不实现 `Copy`
///
/// [`Follow::Kind`] 装的是 [`TokenKind`]，而它带着 `Vec<u8>` 载荷 ⇒ 类型上就不是 `Copy`。
/// 需要传值时用 `Clone`；**const 成员不受影响**（`SEMI` 这类常量每次使用都是一个新值）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum Follow {
    /// 一个**具体**词元（关键字 / 分隔符）—— 无载荷，按值比较。
    Kind(TokenKind),
    /// 官方 `_Name`：**任意**标识符。
    Name,
    /// 官方 `_Semi`：**任意来源**的分号（显式 / 换行自动补 / 文件尾补）。
    Semi,
    /// 官方 `_Operator`。
    ///
    /// ⚠️ **比 [`TokenKind::Op`] 更窄**：官方把 `_Star`(`*`) / `_Assign`(`=`) /
    /// `_Define`(`:=`) / `_Arrow`(`<-`) / `_AssignOp`(`+=`) / `_IncOp`(`++`)
    /// 都算**独立** token，而 Rust 侧它们全都共用 `TokenKind::Op`。
    /// 按官方的折叠边界，`_Operator` 只含 `form == Plain` 且
    /// `op ∉ {Assign, Define, Arrow, Mul}` 的那些。
    #[allow(dead_code)] // 唯一消费者是官方的 `embeddedTerm()`（`parser.go:1897`），归 `T-AP12`
    Operator,
}

impl Follow {
    /// 词元 `k` 是否落在本成员所表示的集合里。
    ///
    /// `pub(crate)` 是给 [`crate::parser::Parser::got`] / `want` 用的 ——
    /// 它们是「按 followset 成员匹配」的通用入口，不该各自重写一遍匹配规则
    /// （两份匹配规则 ⇒ 迟早分叉，且分叉不报错）。
    pub(crate) fn matches(&self, k: &TokenKind) -> bool {
        match self {
            Follow::Kind(want) => k == want,
            Follow::Name => matches!(k, TokenKind::Ident(_)),
            Follow::Semi => matches!(k, TokenKind::Semi(_)),
            Follow::Operator => matches!(
                k,
                TokenKind::Op { form: OpForm::Plain, op, .. }
                    if !matches!(
                        op,
                        Operator::Assign | Operator::Define | Operator::Arrow | Operator::Mul
                    )
            ),
        }
    }
}

/// 便捷常量：分号（写 followlist 时少一层 `Follow::Semi` 的噪声）。
pub(crate) const SEMI: Follow = Follow::Semi;
/// 便捷常量：`{`。
pub(crate) const LBRACE: Follow = Follow::Kind(TokenKind::Delim(Delim::LBrace));
/// 便捷常量：`}`。
pub(crate) const RBRACE: Follow = Follow::Kind(TokenKind::Delim(Delim::RBrace));

/// 恢复点 ①（**顶层声明之间**）—— 官方
/// `advance(_Import, _Const, _Type, _Var, _Func)`（`parser.go:469` / `479`）。
pub(crate) const TOP_DECL_FOLLOW: &[Follow] = &[
    Follow::Kind(TokenKind::Keyword(Keyword::Import)),
    Follow::Kind(TokenKind::Keyword(Keyword::Const)),
    Follow::Kind(TokenKind::Keyword(Keyword::Type)),
    Follow::Kind(TokenKind::Keyword(Keyword::Var)),
    Follow::Kind(TokenKind::Keyword(Keyword::Func)),
];

/// 恢复点 ②（**语句列表**）—— 官方
/// `advance(_Semi, _Rbrace, _Case, _Default)`（`parser.go:2728`）。
pub(crate) const STMT_LIST_FOLLOW: &[Follow] = &[
    SEMI,
    RBRACE,
    Follow::Kind(TokenKind::Keyword(Keyword::Case)),
    Follow::Kind(TokenKind::Keyword(Keyword::Default)),
];

/// `blockStmt` 缺 `{` 时的同步点 —— 官方 `advance(_Name, _Rbrace)`（`parser.go:2310`）。
pub(crate) const BLOCK_STMT_FOLLOW: &[Follow] = &[Follow::Name, RBRACE];

/// `StopOnFirst` 档用来**跳转**的负载。
///
/// 它**不是崩溃**，是控制流 —— 官方 `errorAt` 在 `errh == nil` 时 `panic(p.first)`（`F4`），
/// 由 `Parse` 外层的 `recover` 接住转成 first error（`F19`）。Rust 侧最贴近的直译就是
/// `panic_any` + `catch_unwind`。
///
/// # 为什么不用「在每个递归函数里检查一个中止标志」
///
/// 那种写法**漏检查不会报错** —— 它只会让解析在出错后继续多跑一段，
/// 而症状要到与 oracle 对撞时才暴露。`AGENTS.md` 记的「漏了不会报错」正是这个形状。
/// 栈展开则**不可能被漏掉**。
#[derive(Debug)]
pub(crate) struct Abort;

static QUIET_ABORT: OnceLock<()> = OnceLock::new();

/// 装一次「**只对我们的 abort 负载静音**」的 panic hook。
///
/// # 为什么需要
///
/// 默认 hook 会在**每次** `StopOnFirst` 解析出错时往 stderr 打一行
/// `thread '…' panicked at …`。那不是崩溃报告，却会被读成崩溃。
///
/// # 为什么是「只对我们的负载」
///
/// 全量静音会把**真实的断言失败**一起吞掉 —— 那正是 `AGENTS.md` 记的「静默失败」形状。
/// 所以这里只过滤 payload 是 [`Abort`] 的，其余**原样交给上一个 hook**。
pub(crate) fn install_quiet_abort_hook() {
    QUIET_ABORT.get_or_init(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if info.payload().downcast_ref::<Abort>().is_some() {
                return;
            }
            prev(info);
        }));
    });
}

impl Parser {
    /// 错误恢复：吃掉词元，直到落在 **followset** 上或撞到 EOF
    /// （`F2`，官方 `parser.go:345-375`）。
    ///
    /// 三条语义都不能少：
    ///
    /// 1. **EOF 永远在 followset 里** —— 不跳过 EOF。
    /// 2. `fnest > 0`（在函数体内）时 [`STOPSET`] 并入 —— 语句起始关键字是天然同步点。
    /// 3. **`followlist` 为空 ⇒ 恰好消费一个（非 EOF）词元**：保证一定前进。
    ///    少了这条，调用方在「当前词元不认识」时会原地死循环。
    ///
    /// ⚠️ 这条「空表时消费一个」的语义在官方注释里写得很直白
    /// （`if it is empty, exactly one (non-EOF) token is consumed to ensure progress`），
    /// 但它**不是**「什么都不做」—— 很容易读反。
    pub(crate) fn advance(&mut self, follow: &[Follow]) {
        let consume_one = follow.is_empty();
        loop {
            if self.at(&TokenKind::Eof) {
                break; // 1
            }
            if !consume_one {
                if self.fnest > 0 && self.at_stopset() {
                    break; // 2
                }
                if follow.iter().any(|f| f.matches(&self.tok.kind)) {
                    break;
                }
            }
            self.next();
            if consume_one {
                break; // 3
            }
        }
    }

    /// 当前词元是不是 [`STOPSET`] 里的关键字。
    pub(crate) fn at_stopset(&self) -> bool {
        match &self.tok.kind {
            TokenKind::Keyword(k) => STOPSET.contains(k),
            _ => false,
        }
    }

    /// 报告一个错误（`F4`，官方 `errorAt`）。
    ///
    /// 两档由 [`ErrMode`] 决定：
    ///
    /// - `StopOnFirst` —— 记下 `first` 后 `panic_any`，由 [`crate::parse_file_with`]
    ///   的 `catch_unwind` 接住并转成 first error 返回（`F19`）。
    /// - `Continue` —— 入 [`rgoc_lex::diag::DiagBag`] 后继续。**M2 的主判据档**。
    ///
    /// 两档都**先把诊断记进 `diags`**，所以「`diags` 非空」在两档下都是可观察的，
    /// `StopOnFirst` 只是**恰好一条**。
    pub(crate) fn error_at(&mut self, pos: Pos, code: DiagCode, message: String) {
        let d = Diag { pos, code, message };
        if self.first.is_none() {
            self.first = Some(d.clone()); // F19：外层 recover 转成 first error 返回
        }
        self.errcnt += 1;
        // 记账**在分档之前** —— 两档都留痕，所以「`diags` 非空」在两档下都可观察，
        // `StopOnFirst` 只是恰好一条。顺序写反（先 panic 后记账）会让首错在流里凭空消失。
        self.diags.push(d);
        if self.mode == ErrMode::StopOnFirst {
            std::panic::panic_any(Abort);
        }
    }

    /// 报告一个**语法**错误（`F5`，官方 `syntaxErrorAt`）。
    ///
    /// 目前只落两件事：**EOF 抑制连环错** + `syntax error: ` 前缀。
    /// 消息体的完整复刻（`unexpected X, expected Y` 形态、`tokstring`）是 `T-AP16` 的活 ——
    /// 本任务不越界做它，否则消息复刻会散落在两个任务里。
    pub(crate) fn syntax_error_at(&mut self, pos: Pos, code: DiagCode, msg: &str) {
        // F5：已经在 EOF 且**已经有错**时不再报 —— 否则文件尾会喷一串无意义诊断。
        // 注意判据是「EOF **且已出错**」两个条件的**合取**：只判 EOF 会把
        // 「本来就只有 EOF 错」的正常情况也吞掉。
        if self.at(&TokenKind::Eof) && self.first.is_some() {
            return;
        }
        self.error_at(pos, code, format!("syntax error: {msg}"));
    }
}

#[cfg(test)]
mod tests {
    use rgoc_lex::token::SemiOrigin;

    use super::*;

    fn parser(src: &[u8]) -> Parser {
        Parser::new(src, "t.go", ErrMode::Continue)
    }

    // ── `advance` 的边界语义 ────────────────────────────────────────────────
    //
    // 为什么这些放在**单元测试**而不是 `tests/test_recovery.rs`：
    // `T-AP06` 还没有任何调用方用空 followlist（第一次出现要等 `T-AP07` 之后
    // 「当前词元不认识、只能硬啃一个」的分支），所以在 `parse_file` 外面**看不见**它们。
    // 而这两条恰恰最值得现在钉住 —— 写反了会让调用方原地死循环。

    /// 空 followlist ⇒ **恰好消费一个**词元（官方注释：`exactly one (non-EOF) token`）。
    #[test]
    fn advance_空表恰好消费一个词元() {
        let mut p = parser(b"a b c");
        assert!(matches!(p.tok.kind, TokenKind::Ident(_)));
        p.advance(&[]);
        match &p.tok.kind {
            TokenKind::Ident(bytes) => assert_eq!(bytes, b"b", "应前移一个词元到 `b`"),
            other => panic!("应停在第二个标识符，got {other:?}"),
        }
    }

    /// 空 followlist 遇上 EOF：**一个都不消费**（EOF 不跳过），也不能越界。
    #[test]
    fn advance_空表在_eof_处不动() {
        let mut p = parser(b"");
        p.advance(&[]);
        assert!(p.at(&TokenKind::Eof));
    }

    /// 空 followlist 在**非 EOF** 处一定前进 —— 这条是「不死循环」的保证。
    #[test]
    fn advance_空表保证前进() {
        let mut p = parser(b"} } }");
        let before = p.tok.pos;
        p.advance(&[]);
        assert_ne!(p.tok.pos, before, "空表必须消费一个词元");
    }

    /// 非空 followlist：**当前词元已在 followset 里 ⇒ 一个都不消费**。
    ///
    /// 这条与上一条是一对：官方是 `for !contains(followset, tok)` ——
    /// 先判后动，所以落在同步点上时 `advance` 是**空操作**。
    #[test]
    fn advance_非空表在_follow_上时是空操作() {
        let mut p = parser(b"} }");
        let before = p.tok.pos;
        p.advance(&[Follow::Kind(TokenKind::Delim(Delim::RBrace))]);
        assert_eq!(p.tok.pos, before, "已在 followset 上 ⇒ 不消费");
    }

    /// `fnest > 0`（函数体内）时 `stopset` **并入**：语句起始关键字也让它停下。
    #[test]
    fn advance_函数体内_stopset_生效() {
        let mut p = parser(b"} return");
        p.fnest = 1;
        p.advance(&[Follow::Kind(TokenKind::Keyword(Keyword::Import))]);
        assert!(
            p.at(&TokenKind::Keyword(Keyword::Return)),
            "`}}` 被吃掉，`return` 在 stopset 里 ⇒ 停在它上面，got {:?}",
            p.tok.kind
        );
    }

    /// 对照：`fnest == 0`（函数体外）时 `stopset` **不**并入 —— 只认 followlist。
    ///
    /// 少了这条对照，上面那条测试在「实现干脆无条件并入 stopset」时仍然是绿的。
    #[test]
    fn advance_函数体外_stopset_不生效() {
        let mut p = parser(b"} return import");
        p.advance(&[Follow::Kind(TokenKind::Keyword(Keyword::Import))]);
        assert!(
            p.at(&TokenKind::Keyword(Keyword::Import)),
            "`return` 不该被当同步点，应一路走到 followlist 里的 `import`，got {:?}",
            p.tok.kind
        );
    }

    // ── `Follow` 的**类别**语义 ──────────────────────────────────────────────
    //
    // 为什么值得单独钉：`Follow::Kind` 是**按值**比较，而三个类别成员是**按形状**比较。
    // 「类别判窄了」的症状是恢复时**提前停下**（错误消息位置与 oracle 分叉）；
    // 「判宽了」的症状是多啃词元。两者都不会 panic，只能靠断言抓。

    /// `Follow::Name` 认**任意**标识符 —— 包括**非法 UTF-8** 的原文
    /// （`Ident` 存 `Vec<u8>` 正是为了这个）。
    #[test]
    fn follow_类别_name_认任意标识符() {
        assert!(Follow::Name.matches(&TokenKind::Ident(b"x".to_vec())));
        assert!(
            Follow::Name.matches(&TokenKind::Ident(vec![0xff])),
            "非法 UTF-8 的标识符也是 `_Name`"
        );
        assert!(!Follow::Name.matches(&TokenKind::Keyword(Keyword::Var)));
        assert!(!Follow::Name.matches(&TokenKind::Semi(SemiOrigin::Explicit)));
    }

    /// `Follow::Semi` 认**任意来源**的分号（显式 / 换行自动补 / 文件尾补）。
    /// 若按值比较（只认 `Explicit`），换行自动补的分号会被漏掉 —— 而 Go 里
    /// **绝大多数分号都是自动补的**，这种漏法在真实语料上几乎必然发生。
    #[test]
    fn follow_类别_semi_认任意来源() {
        for o in [SemiOrigin::Explicit, SemiOrigin::Newline, SemiOrigin::Eof] {
            assert!(
                Follow::Semi.matches(&TokenKind::Semi(o)),
                "{o:?} 也是 `_Semi`"
            );
        }
    }

    /// `Follow::Operator` 的边界 == 官方的折叠边界（`tokens.go`）。
    ///
    /// **双向钉**：判窄了（漏掉 `&&`/`!` 等）要红，判宽了（把 `*`/`=`/`:=`/`<-`/`+=`/`++`
    /// 混进来）也要红。只写「`+` 能中」半边是抓不住「宽」那一侧的。
    #[test]
    fn follow_类别_operator_边界与官方一致() {
        let plain = |op| TokenKind::Op {
            form: OpForm::Plain,
            op,
            prec: 0,
        };
        for op in [Operator::Add, Operator::And, Operator::Not, Operator::Shl] {
            assert!(
                Follow::Operator.matches(&plain(op)),
                "{op:?} 是 `_Operator`"
            );
        }
        // 官方把这四个单独列为 _Star / _Assign / _Define / _Arrow ⇒ 不在 `_Operator` 里
        for op in [
            Operator::Mul,
            Operator::Assign,
            Operator::Define,
            Operator::Arrow,
        ] {
            assert!(
                !Follow::Operator.matches(&plain(op)),
                "{op:?} 不是 `_Operator`"
            );
        }
        // `_AssignOp` / `_IncOp` 也是独立 token
        for form in [OpForm::Assign, OpForm::IncDec] {
            assert!(
                !Follow::Operator.matches(&TokenKind::Op {
                    form,
                    op: Operator::Add,
                    prec: 0
                }),
                "{form:?} 不是 `_Operator`"
            );
        }
    }

    /// `Follow::Kind` 是**按值**比较：`}` 不认 `{`，也不认一个内容恰是 `}` 的标识符。
    #[test]
    fn follow_类别_kind_按值比较() {
        let f = Follow::Kind(TokenKind::Delim(Delim::RBrace));
        assert!(f.matches(&TokenKind::Delim(Delim::RBrace)));
        assert!(!f.matches(&TokenKind::Delim(Delim::LBrace)));
        assert!(!f.matches(&TokenKind::Ident(b"}".to_vec())));
    }

    /// 恢复点 ② / `blockStmt` 的两个 followlist 逐条对官方（`parser.go:2728` / `:2310`）。
    /// 写死断言是为了：改成员时这里会红，而不是悄悄改掉恢复行为。
    #[test]
    fn 两个恢复点的_followlist_与官方逐条一致() {
        assert_eq!(STMT_LIST_FOLLOW.len(), 4);
        assert!(STMT_LIST_FOLLOW.contains(&SEMI));
        assert!(STMT_LIST_FOLLOW.contains(&RBRACE));
        assert!(STMT_LIST_FOLLOW.contains(&Follow::Kind(TokenKind::Keyword(Keyword::Case))));
        assert!(STMT_LIST_FOLLOW.contains(&Follow::Kind(TokenKind::Keyword(Keyword::Default))));

        assert_eq!(BLOCK_STMT_FOLLOW.len(), 2);
        assert!(BLOCK_STMT_FOLLOW.contains(&Follow::Name));
        assert!(BLOCK_STMT_FOLLOW.contains(&RBRACE));
    }
}
