//! parser 主体（`M2-design.md` §5）。

use rgoc_ast::{AstFile, BasicLit, Decl, LitKind, Name, Pragma};
use rgoc_lex::diag::{Diag, DiagBag, DiagCode};
use rgoc_lex::lexer::Lexer;
use rgoc_lex::message::Mode;
use rgoc_lex::pos::{FileTable, Pos, PosBases};
use rgoc_lex::token::{Delim, Keyword, OpForm, Operator, Token, TokenKind};

use crate::decl::DeclKind;
use crate::recovery::{
    Abort, CLOSE_RECOVER_FOLLOW, COMMA, Follow, PACKAGE, SEMI, TOP_DECL_FOLLOW,
    install_quiet_abort_hook,
};

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
    /// `//line` 基准链（`T-AP15`）。文件里**没有** `//line` 时链长恒为 1（只有 file base）。
    ///
    /// ⚠️ 暴露它是因为**重映射后的位置只有在带基准的情况下才算得出来** ——
    /// [`rgoc_lex::Pos::rel_line`] 这类函数要拿它当参数。AST 上的 `Pos`
    /// 仍是**绝对**坐标（官方 `Pos.Line()` 的语义），与 `C1` 的 `format_pos` 一致。
    pub bases: PosBases,
}

/// 解析一个 Go 源文件（**尽量继续**档，对应官方 `errh != nil`）。
///
/// 这是 M2 的默认入口 —— 设计上 M2 的主判据全在这一档（`M2-design.md` §5.3）。
///
/// ⚠️ **多文件的装载必须用 [`parse_file_in`]**：本入口自带一张新的 `FileTable`，
/// 于是每个文件的 `FileId` 都是 `0`（Task 23 Ruling 23-4）。
pub fn parse_file(src: &[u8], name: &str) -> ParseResult {
    parse_file_in(&mut FileTable::new(), src, name)
}

/// 解析一个 Go 源文件，登记进**调用方持有的**文件表 —— 多文件装载的入口。
pub fn parse_file_in(table: &mut FileTable, src: &[u8], name: &str) -> ParseResult {
    parse_file_with_in(table, src, name, ErrMode::Continue)
}

/// 解析一个 Go 源文件，显式指定 [`ErrMode`]（自带一张新表；理由见 [`parse_file`]）。
pub fn parse_file_with(src: &[u8], name: &str, mode: ErrMode) -> ParseResult {
    parse_file_with_in(&mut FileTable::new(), src, name, mode)
}

/// 解析一个 Go 源文件，显式指定 [`ErrMode`]，并登记进调用方的文件表。
///
/// # `StopOnFirst` 档为什么要 `catch_unwind`
///
/// 官方在 `errh == nil` 时 `panic(p.first)`（`F4`），由外层的 `recover` 转成
/// first error 返回（`F19`）。这里用 `catch_unwind` 直译。
///
/// ⚠️ **只接住我们自己的 [`Abort`]，其它 panic 原样上抛**：把「控制流」和「bug」
/// 混在一个 `Err(_)` 分支里，会让真正的崩溃被静默吞掉 —— 那正是 `AGENTS.md`
/// 记的「静默失败」形状。
pub fn parse_file_with_in(
    table: &mut FileTable,
    src: &[u8],
    name: &str,
    mode: ErrMode,
) -> ParseResult {
    let mut p = Parser::new_in(table, src, name, mode);

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
        bases: p.bases,
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
    pub(crate) top: bool,
    /// 函数嵌套层，**决定 [`crate::STOPSET`] 是否并入 `followset`**（`F3`）。
    pub(crate) fnest: usize,
    /// 表达式嵌套层，`complit` 二义消解用（§5.6）。
    ///
    /// 三条规则（都对齐官方）：`operand` 的 `(…)` 与 `pexpr` 的 `[…]` 里 **+1**
    /// （括号内允许复合字面量，`(T{})` 合法）；语句头部（`if`/`for`/`switch`）
    /// 置 **-1**（那里禁 complit，`T-AP13`）。判据只有一处消费者：
    /// [`Parser::complit_starts_here`] 的 `xnest >= 0`。
    pub(crate) xnest: isize,
    /// 当前待挂载的文件级指令（`T-AP15` 起由 [`crate::directive::pragma_handler`] 填充）。
    pub(crate) pragma: Pragma,
    /// `//line` 基准链（`T-AP15`；官方 `p.base` + `PosBase` 指针链的**表**形式）。
    ///
    /// ⚠️ **每个 [`Parser::next`] 的产出都会带上「当时的当前基准」**
    /// （等价官方 `MakePos(p.base, line, col)`）—— 所以基准**在构造那一刻**就钉进 `Pos`，
    /// 之后 base 再变也影响不到已造好的位置。这是官方的语义，不是我们的简化。
    pub(crate) bases: PosBases,
    /// 来自 `//go:build` 的版本（`F7`；**仅 `top` 时**取，见 [`crate::directive`]）。
    pub(crate) go_version: Option<String>,
    /// 诊断流（`Continue` 档在此累积）。
    pub(crate) diags: DiagBag,
    /// **第一条**错误（`F19`）。
    pub(crate) first: Option<Diag>,
    /// 报错次数（官方 `p.errcnt`；`F5` 的判据）。
    pub(crate) errcnt: usize,
    /// 递归嵌套深度计数（`T-A-96`，**`H`：官方无此行为**）。
    ///
    /// 官方编译器靠 Go 运行时的可增长栈，恶意/意外深嵌套只是慢；我们落在
    /// 宿主的固定栈上，必须自设上限。三个递归边各计一层：`expr`（表达式）、
    /// `stmt_or_nil`（语句）、`type_or_nil`（类型 —— `type_` 委托给它，直接
    /// 调用方如 `arrayOrTArgs` 也经过它，守这里才不漏）。
    pub(crate) depth: usize,
    /// `SYN-NESTING-TOO-DEEP` 是否已报（**每文件只报一次**：超限后每个祖先
    /// 都会看到超限，不挡住就是上千条重复诊断）。
    pub(crate) depth_reported: bool,
}

impl Parser {
    /// 递归嵌套上限（`T-A-96`，`H`）。
    ///
    /// 1000 = 「合法 Go 代码远达不到、恶意输入要挡住」的经验值：标准库全量
    /// （`T-A-94` 的 6670 个文件）实测最大嵌套远低于它；C 标准的翻译极限
    /// 传统上也是 127/256 这个量级。改它 = 行为变更，须同时改 `T-A-96`。
    pub(crate) const MAX_NEST_DEPTH: usize = 1000;

    /// 进入一层递归嵌套。返回 `false` = 已超限（**每文件只报一次**），
    /// 调用方必须立即返回零值节点（`bad_expr()` / `None`）。
    pub(crate) fn depth_enter(&mut self) -> bool {
        self.depth += 1;
        if self.depth > Self::MAX_NEST_DEPTH {
            if !self.depth_reported {
                self.depth_reported = true;
                let pos = self.tok.pos;
                // 消息以小写字母开头、不带特殊前缀 ⇒ `syntaxErrorAt` 的 default 支，
                // 原样透传不拼 `<tok>`（T-AP16 的三支判据）。
                self.syntax_error_at(pos, DiagCode::SynNestingTooDeep, "nesting depth too deep");
            }
            return false;
        }
        true
    }

    /// 离开一层递归嵌套（与 [`Self::depth_enter`] 成对）。
    pub(crate) fn depth_leave(&mut self) {
        self.depth -= 1;
    }
    /// **测试专用**：自带一张新表（本 crate 的单测都是单文件）。
    ///
    /// ⚠️ 多文件**不能**用它 —— 每个文件都会落在 `FileId(0)` 上（Ruling 23-4）。
    /// 生产入口只有 [`crate::parse_file_in`]。
    #[cfg(test)]
    pub(crate) fn new(src: &[u8], name: &str, mode: ErrMode) -> Self {
        Self::new_in(&mut FileTable::new(), src, name, mode)
    }

    /// 建一个 parser，登记进**调用方持有的**文件表（多文件装载走这条）。
    ///
    /// `pub(crate)` 是因为 [`crate::recovery`] 的单元测试要直接驱动
    /// `advance` 的边界语义（那些语义从 `parse_file` 外面看不见）。
    /// 没有「自带新表」的**非测试**版本：多文件各自建表会让所有文件落在
    /// `FileId(0)` 上（Ruling 23-4 的失效模式）。
    pub(crate) fn new_in(table: &mut FileTable, src: &[u8], name: &str, mode: ErrMode) -> Self {
        let mut lex = Lexer::new_in(table, src.to_vec(), name);
        let file_id = lex.file();
        // 官方 `parser.go:89`：scanner 以 `directives` 模式初始化 —— 只上报
        // `//go:` / `//line` 指令（**不是**全部注释，那要 `comments` 模式）。
        lex.set_mode(Mode::DIRECTIVES);
        let mut bases = PosBases::new();
        // 官方 `parser.go:92`：`p.base = file`（`NewFileBase(name)`）。
        // 必须在**第一次 `next()` 之前**建好：官方 `init` 的顺序是
        // `p.scanner.init(...)` → `p.base = file` → …，
        // 而词元是在 `parse()` 里才拉取的（我们这边同理）。
        bases.file_base(name);
        let mut p = Parser {
            lex,
            tok: Token {
                pos: Pos {
                    base: bases.current(),
                    file: file_id,
                    line: 0,
                    col: 0,
                },
                kind: TokenKind::Eof,
            },
            mode,
            top: true,
            fnest: 0,
            xnest: 0,
            pragma: Pragma::default(),
            bases,
            go_version: None,
            diags: DiagBag::new(),
            first: None,
            errcnt: 0,
            depth: 0,
            depth_reported: false,
        };
        p.next();
        p
    }

    /// 前移一个词元。
    ///
    /// # 这里同时是「指令处理」的时机点（官方 `init` 那个 scanner 回调的等价物）
    ///
    /// 官方 scanner 在**读到注释的下一个词元之前**同步回调 parser（`:51-88`），
    /// 所以 `//line` 的基准变更总是「在它影响到的第一个词元之前」生效。
    /// 我们是拉取式：先 `lex.next()` 拿到新词元，再把**这次扫描顺带产生的**
    /// 指令消息按顺序处理完 —— 此时新词元**还没有**被赋予基准，
    /// 所以下面的「按当前 base 重算 `pos`」拿到的就是**处理完之后**的基准。✓
    pub(crate) fn next(&mut self) {
        self.tok = self.lex.next();
        // 官方 scanner 的注释/指令通道（`take_messages`，与 `diags` 独立）。
        // ⚠️ **必须先处理完消息再给 `self.tok` 赋基准** —— 顺序反了，
        // 本行 `//line foo:100` 之后的第一个词元就会用**旧**基准（差一整段行号）。
        let msgs = self.lex.take_messages();
        for m in &msgs {
            self.handle_directive(m);
        }
        self.tok.pos = Pos {
            base: self.bases.current(),
            ..self.tok.pos
        };
    }

    /// 当前词元是不是 `k`。
    pub(crate) fn at(&self, k: &TokenKind) -> bool {
        &self.tok.kind == k
    }

    /// [`Follow`] 版的 [`Parser::at`] —— 判**类别**而不是单值。
    ///
    /// # 为什么需要它（`at` 不够用）
    ///
    /// 官方很多地方判的是**类别**，而有些类别没有等价的单值 `TokenKind`：
    /// `_Semi`（三种来源都算）、`_Operator`（`form == Plain` 且不是 `*`/`=`/`:=`/`<-`）、
    /// `_Name`。`got` 已经收 `&Follow`，但 `got` 会**消费** —— 凡是要"只看不吃"的地方
    /// （`simpleStmt` 的 `p.tok != _Assign`、`header` 的 `p.tok != _Semi`）都得用这个。
    ///
    /// 不写成 `Follow::Kind(...)` 的内联 `matches!`：那样每处都要重抄一遍"哪些来源算分号"
    /// 这类知识，抄漏一处**不报错**、只让恢复位置与 oracle 分叉。
    pub(crate) fn at_follow(&self, f: &Follow) -> bool {
        f.matches(&self.tok.kind)
    }

    /// 当前词元是不是**书写形式为 [`OpForm::Plain`]** 的那个运算符（官方 `p.op == X`）。
    ///
    /// # 为什么不写成 `self.at(&TokenKind::Op{ form: Plain, op, prec })`
    ///
    /// 那要多填一个 `prec`，而它是**词法层**知识（`Operator::prec()`）—— 在 parser 里
    /// 抄第二份，抄错就永远不中且**不报错**（正是 [`crate::recovery::Follow::Plain`] 的同一形状）。
    ///
    /// # 与官方的等价性
    ///
    /// 官方在判 `p.op == Or` 之前还有个 `p.tok == _Operator`。`_Operator` **不含**
    /// `*` / `=` / `:=` / `<-`；而 `form == Plain` 已排除复合赋值与自增自减。
    /// 对仅有的两个使用点（`Or`(`|`) 与 `Tilde`(`~`)）两者**逐值等价**。
    pub(crate) fn op_is(&self, want: Operator) -> bool {
        matches!(
            &self.tok.kind,
            TokenKind::Op {
                form: OpForm::Plain,
                op,
                ..
            } if *op == want
        )
    }

    /// 官方 `fileOrNil`（`parser.go:406`）：
    ///
    /// ```text
    /// SourceFile = PackageClause ";" { ImportDecl ";" } { TopLevelDecl ";" } .
    /// ```
    ///
    /// # T-AP08 在这里补掉的 T-AP05 欠账
    ///
    /// 骨架版只做了「有 `package` 就跳过、读个名字」—— 与官方差了**四处**，
    /// 而四处**都影响诊断流**（⇒ 影响 oracle 对撞）：
    ///
    /// 1. 缺 `package` 要报 `package statement must be first` 并**放弃文件**（`:417-420`）；
    /// 2. `f.Pragma` 在 package **之后**取（`:421`）—— 它在语义上属于这个文件，
    ///    而文件头之外的注释不该混进来；
    /// 3. **package 子句有任何错就整个放弃**（`:425-428` 的 `don't bother continuing`）；
    /// 4. 循环尾要 `clearPragma()` **在**吃分号**之前**（`:473-475`）——
    ///    「注释可能给**下一条**函数声明设 pragma」，顺序反了 pragma 就挂错节点。
    fn file(&mut self) -> Option<AstFile> {
        let pos = self.tok.pos;

        // 官方 `:415-416`：`f.GoVersion = p.goVersion`，然后 `p.top = false`。
        //
        // ⚠️ 这两行的**先后不可区分**（反向验证 M17 证实）—— `go_version` 早在
        // `Parser::new` 的**首次 `next()`** 里就设好了（`//go:build` 在文件第 1 行，
        // 而 lexer 是**懒**扫描的），所以两种顺序都拿到同一个值。
        //
        // ⚠️ 我曾在这里写「顺序反了 ⇒ 任何文件都取不到」，**那是错的**：
        // 那条注释看起来很有道理，但变异证明它不成立。留这句是为了提醒
        // 「注释里的因果断言也要用变异验」。
        //
        // 真正决定「哪些 `//go:build` 会被取」的是 [`crate::directive::handle_directive`]
        // 里的 `self.top` 判据（官方 `parser.go:79`）—— 但那一条同样**不可测**：
        // lexer 懒扫描 ⇒ package **之后**的 `//go:build` 在被扫到时 `top` 早已是 false。
        // 两处细节已登记在 `progress.md` 的 T-AP15 残留表。
        let go_version = self.go_version.take();
        self.top = false;

        // 官方 `:417-420`
        if !self.got(&PACKAGE) {
            self.syntax_error_at(
                pos,
                DiagCode::SynUnexpectedToken,
                "package statement must be first",
            );
            return None;
        }

        // 官方 `:421-423`：pragma 在 package 之后取；包名用 `name()` 而不是
        // 「读不到就 None」—— 官方会**造**一个 `_` 名字继续建文件。
        let pragma = self.take_pragma();
        let pkg_name = self.name();
        self.want(&SEMI);

        // 官方 `:425-428`
        if self.first.is_some() {
            return None;
        }

        // 官方 `:430-481`：接受 import 出现在任意位置（容错），但要报一条。
        let mut decls: Vec<Decl> = Vec::new();
        let mut prev_was_import = true; // 官方 `prev := _Import`
        while !self.at(&TokenKind::Eof) {
            if self.at(&TokenKind::Keyword(Keyword::Import)) && !prev_was_import {
                let p = self.tok.pos;
                self.syntax_error_at(
                    p,
                    DiagCode::SynUnexpectedToken,
                    "imports must appear before other declarations",
                );
            }
            prev_was_import = self.at(&TokenKind::Keyword(Keyword::Import));

            match self.tok.kind {
                TokenKind::Keyword(Keyword::Import) => {
                    self.next(); // 官方 `:441`
                    self.append_group(&mut decls, DeclKind::Import);
                }

                TokenKind::Keyword(Keyword::Const) => {
                    self.next(); // 官方 `:445`
                    self.append_group(&mut decls, DeclKind::Const);
                }

                TokenKind::Keyword(Keyword::Var) => {
                    self.next(); // 官方 `:453`
                    self.append_group(&mut decls, DeclKind::Var);
                }

                TokenKind::Keyword(Keyword::Type) => {
                    self.next(); // 官方 `:449`
                    self.append_group(&mut decls, DeclKind::Type);
                }

                // ⚠️ `func` 是唯一**不走 `appendGroup`** 的顶层声明（官方 `:456-460`）——
                // `funcDeclOrNil` 还带判空（`d != nil`），所以单独一支。
                TokenKind::Keyword(Keyword::Func) => {
                    self.next(); // 官方 `:457`
                    if let Some(d) = self.func_decl_or_nil() {
                        decls.push(d);
                    }
                }

                _ => {
                    // 官方 `:462-470`。两支共用同一个"非声明"恢复点，只有**消息**不同：
                    // `{` 紧跟一条**无体**函数声明 ⇒ 那个 `{` 是想给它补体（换行写歪了）。
                    let p = self.tok.pos;
                    let empty_func = self.at(&TokenKind::Delim(Delim::LBrace))
                        && matches!(decls.last(), Some(Decl::Func(f)) if f.body.is_none());
                    let msg = if empty_func {
                        "unexpected semicolon or newline before {"
                    } else {
                        "non-declaration statement outside function body"
                    };
                    self.syntax_error_at(p, DiagCode::SynUnexpectedToken, msg);
                    // 恢复点 ①（`parser.go:469`）
                    self.advance(TOP_DECL_FOLLOW);
                    // 官方这一支是 `continue`：错误路径**不**吃分号（`:470`）。
                    continue;
                }
            }

            // 官方 `:473-475`：**先**清 pragma，再吃分号。
            self.clear_pragma();

            if !self.at(&TokenKind::Eof) && !self.got(&SEMI) {
                let p = self.tok.pos;
                self.syntax_error_at(
                    p,
                    DiagCode::SynUnexpectedToken,
                    "after top level declaration",
                );
                // 恢复点 ①（`parser.go:479`）
                self.advance(TOP_DECL_FOLLOW);
            }
        }
        // `p.tok == _EOF`

        self.clear_pragma(); // 官方 `:484`
        let eof = self.tok.pos;
        Some(AstFile {
            pos,
            pragma,
            pkg_name,
            decls,
            eof,
            go_version,
        })
    }

    /// 官方 `got`（`parser.go:201`）：在就消费并返回 `true`。
    ///
    /// 参数取 [`Follow`] 而不是 [`TokenKind`]，因为官方 `got(_Semi)` 认的是**类别** ——
    /// 显式写的、换行自动补的、文件尾补的分号在官方是**同一个** token。
    ///
    /// 收 `&Follow` 而不是按值：`Follow` 装着 `TokenKind`（含 `Vec<u8>` 载荷）⇒
    /// **不是 `Copy`**，按值传在循环里会被 move 掉。而「类别描述」本来也不需要所有权。
    pub(crate) fn got(&mut self, f: &Follow) -> bool {
        if f.matches(&self.tok.kind) {
            self.next();
            true
        } else {
            false
        }
    }

    /// 官方 `want`（`parser.go:209-214`）：不在就报一条并 `advance()`（**空 followlist**）。
    ///
    /// # 为什么 `expected` 由 [`tokstring`] 派生而不是调用方手写（`T-AP16`）
    ///
    /// 官方是 `p.syntaxError("expected " + tokstring(tok))` —— **只有一个参数**。
    /// 我们早先的签名 `want(f, expected: &str)` 让每个调用点手写字面量，
    /// 于是同一处 `want` 在不同文件写出**不同的**词（`expected ;` / `expected keyword for`）
    /// —— 与 oracle 逐字分叉，而**语法上完全正常**。
    ///
    /// 收 [`Follow`] 一个参数后，「期望词元的可读名」只有 [`tokstring`] 一处真相：
    /// 加一个 [`Follow`] 变体 ⇒ 编译期就要求给它一个名字。
    pub(crate) fn want(&mut self, f: &Follow) {
        if !self.got(f) {
            let pos = self.tok.pos;
            let expected = crate::recovery::tokstring(f);
            self.syntax_error_at(
                pos,
                DiagCode::SynUnexpectedToken,
                &format!("expected {expected}"),
            );
            self.advance(&[]);
        }
    }

    // ── 公共产生式（官方 `parser.go:2756` 的 "Common productions" 区）──────

    /// 把位置渲染成 `文件:行:列`（官方 `Pos.String()`，`pos.go:57`）。
    ///
    /// 官方 `branches.go` 的两条消息直接内插位置：
    ///
    /// ```text
    /// :78  label %s already defined at %s          ← alt.lstmt.Label.Pos().String()
    /// :194 goto %s jumps over declaration of %s at %s   ← varPos
    /// ```
    ///
    /// ⚠️ **不能给 [`Pos`] 实现 `Display`** —— 它只持有 [`FileId`]，
    /// 打出来是 `3:7` 这种「看起来像位置、其实缺文件名」的串，
    /// 与 `C1` §2.2 直接冲突（`pos.rs` 的模块文档已明写这条）。
    /// 于是这里借 [`Lexer::table`] 走 `FileTable::format_pos` ——
    /// **让漏掉文件名这件事无法发生**。
    ///
    /// ⚠️ 用的是**绝对**坐标（`format_pos` 不做 `//line` 重映射）。
    /// 官方的 `Pos.String()` 用的也是**绝对** `Line()`/`Col()`
    /// （`RelLine` 只在诊断的显示层用）⇒ 一致。
    pub(crate) fn pos_str(&self, p: Pos) -> String {
        self.lex.table().format_pos(p)
    }

    /// 官方 `name`（`parser.go:2759`）。
    ///
    /// # 未命中时**造**一个 `"_"`，不是返回 `None`
    ///
    /// 官方是 `NewName(p.pos(), "_")` + `syntaxError("expected name")` + `p.advance()`。
    /// 这么写的目的是**让调用方继续建节点** —— 于是恢复路径上的树仍然完整
    /// （`const ( a = iota; b )` 里那个坏名字会在 dump 里以 `_` 出现）。
    /// 换成 `Option` 会让调用方各自决定"造什么"，树形立刻分叉。
    pub(crate) fn name(&mut self) -> Name {
        let pos = self.tok.pos;
        // 先取出载荷，结束对 `self.tok` 的借用（错误分支要 `&mut self`）。
        let value = match &self.tok.kind {
            TokenKind::Ident(bytes) => Some(lit_text(bytes)),
            _ => None,
        };
        match value {
            Some(value) => {
                self.next();
                Name { value, pos }
            }
            None => {
                self.syntax_error_at(pos, DiagCode::SynUnexpectedToken, "expected name");
                // 官方 `p.advance()` —— **空 followlist** ⇒ 消费恰好一个非 EOF 词元。
                self.advance(&[]);
                Name {
                    value: "_".to_string(),
                    pos,
                }
            }
        }
    }

    /// 官方 `nameList`（`parser.go:2776`）：`IdentifierList = identifier { "," identifier }`。
    ///
    /// 第一个名字**必须由调用方提供**（官方 `debug` 里对此有断言）——
    /// 因为各调用点是在自己的上下文里先 `p.name()` 再调它的。
    pub(crate) fn name_list(&mut self, first: Name) -> Vec<Name> {
        let mut l = vec![first];
        while self.got(&COMMA) {
            l.push(self.name());
        }
        l
    }

    /// 官方 `takePragma`（`parser.go:104`）：取走当前 pragma 并清空。
    pub(crate) fn take_pragma(&mut self) -> Pragma {
        std::mem::take(&mut self.pragma)
    }

    /// 官方 `clearPragma`（`parser.go:114`）。
    ///
    /// ⚠️ 官方的语义是「把**未使用**的 pragma 交回 `pragh` 上报」而不是「丢掉」。
    /// M2 只**解析并挂载**、不生效；未使用 pragma 的**上报**（`misplaced compiler
    /// directive` 一类诊断）与 pragma 的完整处理一起归 `T-AP15`。
    pub(crate) fn clear_pragma(&mut self) {
        self.pragma = Pragma::default();
    }

    /// 官方 `oliteral`（`parser.go:1744`）：当前词元是字面量就取走并前移。
    ///
    /// 未命中返回 [`None`] **且不报错**（官方 `:1754` 的 `return nil`）——
    /// 报错是**调用方**的事（`importDecl` 报 `missing import path`）。
    pub(crate) fn oliteral(&mut self) -> Option<BasicLit> {
        let pos = self.tok.pos;
        let got = match &self.tok.kind {
            TokenKind::Literal {
                text,
                lit_kind,
                bad,
            } => Some((lit_text(text), lit_kind_of(*lit_kind), *bad)),
            _ => None,
        };
        // 官方是 `b.Bad = p.bad` —— **透传**词法层的 bad（畸形字面量照常产出节点）。
        got.map(|(value, kind, bad)| {
            self.next();
            BasicLit {
                value,
                kind,
                bad,
                pos,
            }
        })
    }

    /// 官方 `list`（`parser.go:507`）。
    ///
    /// ```text
    /// list = [ f { sep f } [sep] ] close .
    /// ```
    ///
    /// 返回 **close 词元自身的位置**（官方先 `pos := p.pos()` 再 `p.want(close)`）。
    /// `sep` 在 `close` 前是**可选**的 —— 漏掉这个判断会把 `import ( "a"; )`
    /// 这种合法写法误报一条。
    ///
    /// `f` 收到 `&mut Parser` 而不是捕获 `self`：后者在 Rust 里是**借用冲突**
    /// （`self` 已被本方法可变借住）。所以闭包一律写 `|p| p.xxx()`，不写 `|_| self.xxx()`。
    pub(crate) fn list(
        &mut self,
        context: &str,
        sep: Follow,
        close: Follow,
        mut f: impl FnMut(&mut Parser) -> bool,
    ) -> rgoc_lex::pos::Pos {
        let mut done = false;
        while !self.at(&TokenKind::Eof) && !close.matches(&self.tok.kind) && !done {
            done = f(&mut *self);

            // `sep` 在 `close` 前可选（官方注释）。
            if !self.got(&sep) && !close.matches(&self.tok.kind) {
                let pos = self.tok.pos;
                // 官方 `:517`：`in %s; possibly missing %s or %s` —— 后面两个是
                // `tokstring(sep)` / `tokstring(close)`。
                // ⚠️ 这条走 `in ` 那一支 ⇒ 最终消息是
                // `unexpected <tok> in <ctx>; possibly missing <sep> or <close>`
                // （**一个空格**，不是 `", "`）。
                let sep_name = crate::recovery::tokstring(&sep);
                let close_name = crate::recovery::tokstring(&close);
                self.syntax_error_at(
                    pos,
                    DiagCode::SynUnexpectedToken,
                    &format!("in {context}; possibly missing {sep_name} or {close_name}"),
                );
                self.advance(CLOSE_RECOVER_FOLLOW);
                if !close.matches(&self.tok.kind) {
                    // 官方 `:521`：位置可以更好，但既然已经错了就不管了。
                    return self.tok.pos;
                }
            }
        }

        let pos = self.tok.pos;
        self.want(&close);
        pos
    }
}

/// 词法层的**字节原文** → AST 层的文本（`String`）。
///
/// # ⚠️ 这个转换是**有损**的（已知缺口，不是疏忽）
///
/// M1 实测：`1\xff2` 的字面量原文里**含非法 UTF-8 字节** —— 正因如此
/// `rgoc-lex` 才把 `Ident` / `Literal.text` 做成 `Vec<u8>`（见 `token.rs` 的
/// 那一大段理由）。官方的 `Name.Value` / `BasicLit.Value` 是 Go `string`
/// （= 任意字节序列），**装得下**；我们的 `String` **装不下** ——
/// 非法字节会被 `U+FFFD` 顶替。
///
/// # 为什么不现在改成 `Vec<u8>`
///
/// 改动本身不大（全仓只有 `ident` / `oliteral` / 将来的 `operand` **三处**转换点，
/// 晚补的代价**不随已写任务数增长** —— 与 PA-1 那类「字段形状错」不同），
/// 但 dump 里「非法字节怎么显示」的规则必须与 `tools/ast-oracle/dump.go`
/// （`T-AP17`）**两边逐字一致**，而那个文件还不存在 ⇒ 现在定了规则也要重对齐一次。
/// 故登记给 `T-AP17` 一并处理。
///
/// **M2 主判据不受影响**：对撞语料（`testdata` + 标准库）全是合法 UTF-8；
/// 受影响的只有负例里的字节保真。
///
/// 收敛成**一个有名字的函数**（而不是三处各写一遍 `from_utf8_lossy`）：
/// 这样缺口只有一处定义，`T-AP17` 改的时候不会漏。
///
/// `pub(crate)` 是因为 [`crate::recovery::Parser::tok_desc`]（`T-AP16`）拼
/// `unexpected name X` / `unexpected literal 42` 时要用**同一个**转换 ——
///
/// 两处各写一遍 `from_utf8_lossy` 就是两份真相，将来修 `lit_text()` 的有损
/// 转换时**只改一处**会让消息里的字面原文与树里的不一致。
pub(crate) fn lit_text(text: &[u8]) -> String {
    String::from_utf8_lossy(text).into_owned()
}

/// 词法层的字面量类别 → AST 层的类别（`AST-11`）。
///
/// 两个 crate 各有一个 `LitKind`（`rgoc_lex::token::LitKind` 在词法层，
/// `rgoc_ast::LitKind` 在 AST 层）。用**穷尽 `match`** 写，任一侧加变体都会在
/// 编译期炸 —— 不会像"两边各维护一份映射表"那样静默漏掉。
fn lit_kind_of(k: rgoc_lex::token::LitKind) -> LitKind {
    use rgoc_lex::token::LitKind as Lex;
    match k {
        Lex::Int => LitKind::Int,
        Lex::Float => LitKind::Float,
        Lex::Imag => LitKind::Imag,
        Lex::Rune => LitKind::Rune,
        Lex::String => LitKind::String,
    }
}

#[cfg(test)]
mod tests {
    use rgoc_ast::LitKind;
    use rgoc_lex::token::{Delim, Operator};

    use super::*;
    use crate::recovery::{COMMA, LPAREN, RPAREN, SEMI};

    fn parser(src: &[u8]) -> Parser {
        Parser::new(src, "t.go", ErrMode::Continue)
    }

    // ── `name`（官方 `parser.go:2759`）────────────────────────────────────
    //
    // `name` 是**唯一一处**官方在未命中时**造一个节点**（`"_"`）而不是返回 nil 的
    // 公共产生式。它决定了 `const ( a = iota; b )` 这类恢复路径上节点的形状，
    // 所以在 oracle 对撞里是可见的。

    #[test]
    fn name_命中标识符() {
        let mut p = parser(b"foo bar");
        let n = p.name();
        assert_eq!(n.value, "foo");
        assert_eq!(n.pos.col, 1, "位置是标识符**起点**");
        assert_eq!(p.errcnt, 0);
        assert!(
            matches!(p.tok.kind, TokenKind::Ident(_)),
            "应消费掉 `foo`，got {:?}",
            p.tok.kind
        );
    }

    /// 未命中：**造**一个 `"_"` 名字（官方 `:2768`）→ 报一条 → `advance()`。
    #[test]
    fn name_未命中造下划线名并报错() {
        let mut p = parser(b"1 2");
        let n = p.name();
        assert_eq!(n.value, "_", "官方 `NewName(p.pos(), \"_\")`");
        assert_eq!(p.errcnt, 1);
        let first = p.first.as_ref().expect("首错要留下");
        assert!(first.message.contains("expected name"), "got {first:?}");
        // 官方是 `p.advance()`（**空 followlist**）⇒ 恰好消费一个非 EOF 词元
        assert!(
            matches!(p.tok.kind, TokenKind::Literal { .. }),
            "应停在第二个词元上，got {:?}",
            p.tok.kind
        );
    }

    // ── `nameList`（官方 `parser.go:2776`）───────────────────────────────

    #[test]
    fn name_list_收逗号分隔的名字() {
        let mut p = parser(b"a, b, c");
        let first = p.name();
        let l = p.name_list(first);
        assert_eq!(l.len(), 3);
        assert_eq!(l[2].value, "c");
    }

    /// 单个名字时不消费后面的词元（`const a = 1` 里 `a` 之后是 `=`）。
    #[test]
    fn name_list_单个名字() {
        let mut p = parser(b"a = 1");
        let first = p.name();
        let l = p.name_list(first);
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].value, "a");
        assert!(
            matches!(
                p.tok.kind,
                TokenKind::Op {
                    op: Operator::Assign,
                    ..
                }
            ),
            "`=` 不该被 nameList 吃掉，got {:?}",
            p.tok.kind
        );
    }

    // ── `oliteral`（官方 `parser.go:1744`）──────────────────────────────

    #[test]
    fn oliteral_命中字面量() {
        let mut p = parser(b"\"math\" x");
        let l = p.oliteral().expect("双引号串是字面量");
        assert_eq!(l.value, "\"math\"", "存**原文**（含引号）");
        assert_eq!(l.kind, LitKind::String);
        assert!(!l.bad);
        assert_eq!(p.errcnt, 0);
    }

    /// 非字面量 → `None`，**且不报错**（官方 `:1754` 的 `return nil`）。
    ///
    /// 报错是**调用方**的事（`importDecl` 报 `missing import path`）。
    /// 在这里顺手报一条，诊断流会多出官方没有的条目 ⇒ 对撞假红。
    #[test]
    fn oliteral_未命中返回_none_且不报错() {
        let mut p = parser(b"x");
        assert!(p.oliteral().is_none());
        assert_eq!(p.errcnt, 0);
        assert!(
            matches!(p.tok.kind, TokenKind::Ident(_)),
            "未命中不该消费任何词元"
        );
    }

    /// `bad` 要**透传**（官方 `b.Bad = p.bad`）：`"unterm` 这类畸形字面量
    /// 照常产出词元但带 `bad=true`，AST 侧必须看得到。
    #[test]
    fn oliteral_透传_bad() {
        let mut p = parser(b"\"unterm");
        let l = p.oliteral().expect("畸形字面量**仍然**是字面量");
        assert!(l.bad, "官方 `b.Bad = p.bad`");
    }

    // ── `list`（官方 `parser.go:507`）───────────────────────────────────

    #[test]
    fn list_空表立刻返回且不调用_f() {
        let mut p = parser(b")");
        let mut calls = 0;
        let pos = p.list("test", SEMI, RPAREN, |_| {
            calls += 1;
            false
        });
        assert_eq!(calls, 0, "见到 close 就不该调 f");
        assert_eq!(pos.col, 1, "返回的是 close 词元自身的位置");
        // ⚠️ `)` 之后**不是** `Eof`，而是词法层在**文件末尾自动补的分号**
        // （`SemiOrigin::Eof`，Go 规范：文件尾自动插入分号）。
        // 断言 `Eof` 会红 —— 这个坑在每条"读到最后"的测试上都会出现。
        assert!(
            matches!(p.tok.kind, TokenKind::Semi(_)),
            "`want` 消费了 `)`，接着是文件尾自动补的分号，got {:?}",
            p.tok.kind
        );
    }

    #[test]
    fn list_分号分隔两元素() {
        let mut p = parser(b"a; b)");
        let mut seen = 0;
        p.list("test", SEMI, RPAREN, |p| {
            p.next();
            seen += 1;
            false
        });
        assert_eq!(seen, 2);
        assert_eq!(p.errcnt, 0);
        assert!(
            matches!(p.tok.kind, TokenKind::Semi(_)),
            "`)` 已被 `want` 消费，接着是文件尾自动补的分号，got {:?}",
            p.tok.kind
        );
    }

    /// close 前的 `sep` **可选**（官方注释 `sep is optional before close`）。
    /// 若漏了这个判断，`import ( "a"; )` 这种合法写法会被误报一条。
    #[test]
    fn list_末尾分隔符可选() {
        let mut p = parser(b"a;)");
        let mut seen = 0;
        p.list("test", SEMI, RPAREN, |p| {
            p.next();
            seen += 1;
            false
        });
        assert_eq!(seen, 1);
        assert_eq!(p.errcnt, 0, "close 前的 sep 可选，不该报错");
    }

    /// 缺分隔符 ⇒ 报一条带 context 的错，并按 `advance(_Rparen,_Rbrack,_Rbrace)` 同步。
    #[test]
    fn list_缺分隔符时报错并按闭括号同步() {
        let mut p = parser(b"a b)");
        let mut seen = 0;
        p.list("grouped declaration", SEMI, RPAREN, |p| {
            p.next();
            seen += 1;
            false
        });
        assert_eq!(seen, 1);
        assert_eq!(p.errcnt, 1, "恰好报一条");
        let first = p.first.as_ref().expect("首错要留下");
        assert!(
            first.message.contains("in grouped declaration"),
            "消息要带上 context，got {first:?}"
        );
        assert!(first.message.contains("possibly missing"), "got {first:?}");
    }

    /// `f` 返回 `true` ⇒ 不再收元素（官方 `done = f()`）。
    #[test]
    fn list_f_返回_true_即停止() {
        let mut p = parser(b"a; b; c)");
        let mut seen = 0;
        p.list("test", SEMI, RPAREN, |p| {
            p.next();
            seen += 1;
            true
        });
        assert_eq!(seen, 1, "f 返回 true 后不再收元素");
    }

    /// `COMMA` / `LPAREN` 两个新常量要真的认对词元（防止常量写错却没人发现）。
    #[test]
    fn 逗号与左括号常量认对词元() {
        assert!(COMMA.matches(&TokenKind::Delim(Delim::Comma)));
        assert!(LPAREN.matches(&TokenKind::Delim(Delim::LParen)));
        assert!(!LPAREN.matches(&TokenKind::Delim(Delim::RParen)));
        assert!(!COMMA.matches(&TokenKind::Delim(Delim::LBrack)));
    }
}
