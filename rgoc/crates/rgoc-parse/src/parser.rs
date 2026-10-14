//! parser 主体（`M2-design.md` §5）。

use rgoc_ast::{AstFile, Decl, Name, Pragma};
use rgoc_lex::diag::{Diag, DiagBag, DiagCode};
use rgoc_lex::lexer::Lexer;
use rgoc_lex::token::{Keyword, Token, TokenKind};

use crate::recovery::{Abort, Follow, TOP_DECL_FOLLOW, install_quiet_abort_hook};

/// 解析模式 —— 官方 `errh` 的两档（`F4`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ErrMode {
    /// `errh == nil`：遇**第一个**错即止（官方 `panic(p.first)`，`F4`）。
    ///
    /// 实现方式是**栈展开**而不是标志位 —— 理由见 [`crate::recovery::Abort`]。
    StopOnFirst,
    /// `errh != nil`：报错后**尽量继续**，产出畸形节点。**M2 的主判据档**。
    Continue,
}

/// 一次解析的结果。
#[derive(Debug)]
pub struct ParseResult {
    /// 成功解析出的编译单元。
    ///
    /// `None` 有两种成因，`first` 能区分：`StopOnFirst` 档撞错（`F4`），
    /// 或 package 子句缺失（`fileOrNil` 的 `?`）。
    pub file: Option<AstFile>,
    /// 诊断流（沿用 C1；取用只有 [`DiagBag::into_sorted`] 一条路）。
    pub diags: DiagBag,
    /// **第一条**错误（`F19`：官方外层 `recover` 转成的 first error）。
    ///
    /// 它与 `diags` 的首条是**同一条**（`error_at` 两档都先记账）。
    /// 单独留一个字段是因为 `StopOnFirst` 档下调用方要的是它，而不是整条流。
    pub first: Option<Diag>,
    /// 报错次数（官方 `p.errcnt`）—— `F5` 的 EOF 抑制是否生效，看它而不是看 `diags.len()`：
    /// 抑制生效时**计数不动**，所以两者会在抑制路径上分叉，正好可用作判据。
    pub errors: usize,
}

/// 解析一个 Go 源文件（**尽量继续**档，对应官方 `errh != nil`）。
///
/// 这是 M2 的默认入口 —— 设计上 M2 的主判据全在这一档（`M2-design.md` §5.3）。
pub fn parse_file(src: &[u8], name: &str) -> ParseResult {
    parse_file_with(src, name, ErrMode::Continue)
}

/// 解析一个 Go 源文件，显式指定 [`ErrMode`]。
///
/// # `StopOnFirst` 档为什么要 `catch_unwind`
///
/// 官方在 `errh == nil` 时 `panic(p.first)`（`F4`），由外层的 `recover` 转成
/// first error 返回（`F19`）。这里用 `catch_unwind` 直译。
///
/// ⚠️ **只接住我们自己的 [`Abort`]，其它 panic 原样上抛**：把「控制流」和「bug」
/// 混在一个 `Err(_)` 分支里，会让真正的崩溃被静默吞掉 —— 那正是 `AGENTS.md`
/// 记的「静默失败」形状。
pub fn parse_file_with(src: &[u8], name: &str, mode: ErrMode) -> ParseResult {
    let mut p = Parser::new(src, name, mode);

    let file = if mode == ErrMode::StopOnFirst {
        install_quiet_abort_hook();
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| p.file())) {
            Ok(f) => f,
            Err(payload) => {
                if !payload.is::<Abort>() {
                    std::panic::resume_unwind(payload);
                }
                None
            }
        }
    } else {
        p.file()
    };

    ParseResult {
        file,
        diags: p.diags,
        first: p.first,
        errors: p.errcnt,
    }
}

/// 手写递归下降 parser。**只有 `tok` 一个前瞻词元**（对齐官方，§5.1）。
///
/// # 字段可见性
///
/// 几个状态字段是 `pub(crate)` 而不是私有：错误恢复的四个函数在
/// [`crate::recovery`]（与 `parser` 平级的模块），拿不到本模块的私有字段。
/// 让它们 `pub(crate)` 比把恢复逻辑塞回 `parser.rs` 更划算 —— 恢复是一个**机制**，
/// 见 `recovery.rs` 的模块文档。
#[derive(Debug)]
pub struct Parser {
    lex: Lexer,
    /// 当前词元（唯一前瞻）。
    pub(crate) tok: Token,
    /// 解析模式（`F4` 双档）。
    pub(crate) mode: ErrMode,
    /// 是否在 package 子句之前（`//go:build` 只在此时取 `goVersion`，`F7`）。
    #[allow(dead_code)]
    pub(crate) top: bool,
    /// 函数嵌套层，**决定 [`crate::STOPSET`] 是否并入 `followset`**（`F3`）。
    pub(crate) fnest: usize,
    /// 表达式嵌套层，`complit` 二义消解用（§5.6）。
    #[allow(dead_code)]
    pub(crate) xnest: isize,
    /// 当前待挂载的文件级指令。
    #[allow(dead_code)]
    pragma: Pragma,
    /// 诊断流（`Continue` 档在此累积）。
    pub(crate) diags: DiagBag,
    /// **第一条**错误（`F19`）。
    pub(crate) first: Option<Diag>,
    /// 报错次数（官方 `p.errcnt`；`F5` 的判据）。
    pub(crate) errcnt: usize,
}

impl Parser {
    /// 建一个 parser。`pub(crate)` 是因为 [`crate::recovery`] 的单元测试要直接驱动
    /// `advance` 的边界语义（那些语义从 `parse_file` 外面看不见）。
    pub(crate) fn new(src: &[u8], name: &str, mode: ErrMode) -> Self {
        let mut lex = Lexer::new(src.to_vec(), name);
        let tok = lex.next();
        Parser {
            lex,
            tok,
            mode,
            top: true,
            fnest: 0,
            xnest: 0,
            pragma: Pragma::default(),
            diags: DiagBag::new(),
            first: None,
            errcnt: 0,
        }
    }

    /// 前移一个词元。
    pub(crate) fn next(&mut self) {
        self.tok = self.lex.next();
    }

    /// 当前词元是不是 `k`。
    pub(crate) fn at(&self, k: &TokenKind) -> bool {
        &self.tok.kind == k
    }

    /// 当前词元是不是一个**顶层声明**的起始关键字。
    fn at_top_decl_start(&self) -> bool {
        matches!(
            &self.tok.kind,
            TokenKind::Keyword(
                Keyword::Import | Keyword::Const | Keyword::Type | Keyword::Var | Keyword::Func
            )
        )
    }

    /// `fileOrNil` 骨架：`package` 子句 → 顶层声明循环。
    fn file(&mut self) -> Option<AstFile> {
        let pos = self.tok.pos;

        // package 子句
        if self.at(&TokenKind::Keyword(Keyword::Package)) {
            self.next();
        }
        let pkg_name = self.ident()?;
        self.expect_semi();
        self.top = false;

        // 顶层声明循环
        let mut decls: Vec<Decl> = Vec::new();
        while !self.at(&TokenKind::Eof) {
            if let Some(d) = self.top_decl() {
                decls.push(d);
                continue;
            }
            // 是声明起始关键字，但 `top_decl` 还是骨架（`T-AP08`/`T-AP09` 填实）。
            // 这里**不消费也不报错**地停下：骨架的「还没实现」不是语法错误，
            // 报出来会污染诊断流、让 oracle 对撞出现假分叉。
            if self.at_top_decl_start() {
                break;
            }
            // 恢复点 ①：顶层声明之间（官方 `parser.go:469` / `479`）
            self.syntax_error_at(
                self.tok.pos,
                DiagCode::SynUnexpectedToken,
                "non-declaration statement outside function body",
            );
            self.advance(TOP_DECL_FOLLOW);
        }

        let eof = self.tok.pos;
        Some(AstFile {
            pos,
            pragma: std::mem::take(&mut self.pragma),
            pkg_name,
            decls,
            eof,
            go_version: None,
        })
    }

    /// 顶层声明分派（`T-AP08`/`T-AP09` 实现；骨架先返回 `None`）。
    fn top_decl(&mut self) -> Option<Decl> {
        None
    }

    /// 读一个标识符；不是标识符时报错并返回 `None`。
    ///
    /// # 为什么先把载荷取出来再分派
    ///
    /// `match &self.tok.kind` 的借用会覆盖整个 `match`，而错误分支要 `&mut self`
    /// （`syntax_error_at`）。先取出 `Option<String>` 结束借用，再决定走哪条路 ——
    /// 这样借用检查既不需要复制模式，也不会有「在 match 臂里改 self」的歧义。
    fn ident(&mut self) -> Option<Name> {
        let pos = self.tok.pos;
        let value = match &self.tok.kind {
            TokenKind::Ident(bytes) => Some(String::from_utf8_lossy(bytes).into_owned()),
            _ => None,
        };
        match value {
            Some(value) => {
                self.next();
                Some(Name { value, pos })
            }
            None => {
                self.syntax_error_at(pos, DiagCode::SynUnexpectedToken, "expected package name");
                None
            }
        }
    }

    /// 消费一个分号（若在）。
    ///
    /// ⚠️ 只认**显式**分号 —— 换行自动补出的（`SemiOrigin::Newline`）不认。
    /// 这是 `package` 子句收尾的语义（官方 `eof`/`semi` 相关处理），**不要**拿它当
    /// 通用「吃分号」。语句列表层要用 [`Parser::got`] 配 [`crate::recovery::SEMI`]（认类别）。
    fn expect_semi(&mut self) {
        if matches!(self.tok.kind, TokenKind::Semi(_)) {
            self.next();
        }
    }

    /// 官方 `got`（`parser.go:201`）：在就消费并返回 `true`。
    ///
    /// 参数取 [`Follow`] 而不是 [`TokenKind`]，因为官方 `got(_Semi)` 认的是**类别** ——
    /// 显式写的、换行自动补的、文件尾补的分号在官方是**同一个** token。
    pub(crate) fn got(&mut self, f: Follow) -> bool {
        if f.matches(&self.tok.kind) {
            self.next();
            true
        } else {
            false
        }
    }

    /// 官方 `want`（`parser.go:209`）：不在就报一条并 `advance()`（**空 followlist**）。
    ///
    /// `expected` 是给消息用的可读名（官方用 `tokstring(tok)`）；`tokstring` 的完整复刻
    /// 与错误消息复刻一起归 `T-AP16`，此处不越界。
    pub(crate) fn want(&mut self, f: Follow, expected: &str) {
        if !self.got(f) {
            let pos = self.tok.pos;
            self.syntax_error_at(
                pos,
                DiagCode::SynUnexpectedToken,
                &format!("expected {expected}"),
            );
            self.advance(&[]);
        }
    }
}
