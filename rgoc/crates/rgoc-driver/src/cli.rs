//! CLI 参数解析 —— **手写，不引 clap**。
//!
//! # 为什么不引解析库
//!
//! M1 起 CLI 有 `harness`（`list` / `run` / `report` 三动作）与 `lex`（词法输出，
//! `T-L18`）两个子命令，M2 又加了 `ast`（AST dump，`T-AP19`），M3 加了
//! `types`（类型检查入口，本阶段）—— 仍然不值得引
//! clap：M0 阶段 workspace 是**零依赖**的（`rgoc-harness`），为几个动作引入
//! 一棵依赖树会破掉「离线可构建」这个属性。
//!
//! 若将来 CLI 长到需要子命令级 `--help`、参数补全、配置文件，再换 clap —— 那时
//! 换掉这一整个模块即可，**解析结果是本模块的出口，不扩散到别处**。
//!
//! # 纪律：未实现的子命令必须报错
//!
//! T37 明确「**不预留**未实现的子命令」。三个 spike（M0 Phase 3）将来会加
//! `harness spike` 或独立子命令，但**现在**接受它们等于 CLI 承诺了一个不存在的能力
//! —— 用户敲下去得到「unknown command」比「命令存在但什么都没做」好得多。
//! 这里是本仓第一条「宁可直接报错也不假装支持」的 CLI 规则。

use std::fmt;
use std::path::PathBuf;

/// 解析出来的命令。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// `harness` 子命令
    Harness(HarnessCommand),
    /// `lex` 子命令（T-L18）
    Lex(LexCommand),
    /// `ast` 子命令（T-AP19）
    Ast(AstCommand),
    /// `types` 子命令（M3/Task 2 起登记；实现从 Task 28 起）
    Types(TypesCommand),
    /// `check` 子命令（M4/Task 2 起登记；真身从 Task 20 起）
    Check(CheckCommand),
}

/// `check` 子命令的参数（M4/Task 2）。
///
/// 与 `types` 的分工：`types` 是**装载面**（作用域/常量/导出），`check` 是
/// **检查面**（诊断的行/类别对撞，`C8`）。旗标面按 `M4-plan.md` Task 2 冻结：
/// 全部可解析，未实现的选项一律报错（T37 纪律）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckCommand {
    /// 组成**同一个包**的文件（按用户给出的顺序）。
    ///
    /// 首轮对撞面是**逐文件单跑**（`C8` §2 装置前提），多文件包见 `U-M4-11`；
    /// 装载器本身收多文件（单包多文件是 M3 的包模型），这里不额外设限。
    pub files: Vec<PathBuf>,
    /// `--diag-dump`：把诊断渲染成 dump（`C8` 的对撞入口；Task 20 起接 C8 类别行）。
    ///
    /// 不给时只报诊断（stderr）与摘要（stdout）—— 门禁读的是退出码与诊断流，
    /// dump 是给对撞看的中间产物，两条通道刻意分开（与 `types` 同一条纪律）。
    pub diag_dump: bool,
    /// `--lang go1.N`：版本门控覆盖（`D-M3-15` 的显式覆盖路径）。
    ///
    /// 优先级高于文件首行的 `// -lang=`（`Ruling 18-1`）；不给时用首行旗标。
    pub lang: Option<String>,
    /// `--test-hooks`：注入官方的测试内建（`assert`/`trace`，`F13`）。
    ///
    /// 对应官方 `types.DefPredeclaredTestFuncs()` 的装置级开关；
    /// **Task 20 起生效**（骨架期只解析）。
    pub test_hooks: bool,
}

/// `types` 子命令的参数（M3/Task 2）。
///
/// # 为什么收**多个**文件
///
/// M3 的包模型是「**单包多文件**」（`D-M3-5`）—— 一个包的标准形态就是若干
/// 同属一个 `package` 子句的 `.go` 文件，所以这里天然是列表而不是单文件
/// （与 `lex`/`ast` 的单文件形态刻意不同）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypesCommand {
    /// 组成**同一个包**的文件（按用户给出的顺序；装载与编号依赖这个顺序）
    pub files: Vec<PathBuf>,
    /// `--dump`：输出 C7 中性类型 dump（`T-TY-91` 的对撞入口）。
    ///
    /// 不给时只报诊断与摘要 —— 「检查通过没有」是门禁要读的信号，
    /// dump 是给对撞看的中间产物，两者刻意分开。
    pub dump: bool,
    /// `--cmp`：输出 `//rgoc:cmp` 指令的判定行（`T-TY-92/93` 的对撞入口）。
    ///
    /// 与 `--dump` 互斥（两种比对面，输出形状不同）。
    pub cmp: bool,
    /// `-I <root>`：import 解析根（可给多次）。
    ///
    /// 有了它才有 import 解析（`Ruling 27-1`：`load_with_imports`）；
    /// 一个都不给时**没有根** ⇒ 一切 import 都报「不支持的 import 形式」
    /// （`D-M3-5`：M3 不读二进制 export data、不做模块解析）。
    pub roots: Vec<PathBuf>,
    /// `--lang go1.N`：版本门控覆盖（`D-M3-15` 的显式覆盖路径）
    pub lang: Option<String>,
}

/// `ast` 子命令的参数（T-AP19）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AstCommand {
    /// 要 dump 的文件（**原样保存用户给的路径** —— 输出的 `# golden:` 行要回显它）
    pub file: PathBuf,
}

/// `lex` 子命令的参数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexCommand {
    /// 要扫的文件（**原样保存用户给的路径** —— 输出的 `# golden:` 行要回显它）
    pub file: PathBuf,
    /// 注释/指令上报模式（缺省 = 一律不上报）
    pub mode: LexMode,
}

/// `lex` 的上报模式 —— 与 oracle 的 `--mode comments|directives` 同名同值同义。
///
/// 有意**不直接**用 `rgoc_lex::Mode`：本模块是「无依赖的纯解析层」
/// （见模块头「换 clap 时整个换掉」），`字符串 → 语义` 的映射在 `lex_cmd` 层做。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LexMode {
    /// 缺省：注释与指令都不上报（只出 tokens/diags 两节）
    None,
    /// 全部注释（含 `//go:`）都上报
    Comments,
    /// 只有 `//go:` 与 `//line` 上报
    Directives,
}

/// `harness` 的三个动作。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HarnessCommand {
    /// 列出样本（不执行）
    List {
        /// `--all`：连分母外的一起列；默认只列 20 个基线样本
        all: bool,
    },
    /// 跑样本
    Run {
        /// 显式给的 ID（可空，靠 `all` 区分）
        ids: Vec<String>,
        /// `--all` ⇒ 20 个全跑
        all: bool,
    },
    /// 跑完出报告（当前实现：跑全部并把 JSON 写到 `--out`）
    Report {
        /// 输出路径；`None` ⇒ 打印到 stdout
        out: Option<PathBuf>,
    },
}

/// 参数错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliError {
    /// 一个参数都没给
    NoCommand,
    /// 顶层命令不认识
    UnknownCommand(String),
    /// 顶层命令认识但没给子命令
    MissingSubcommand,
    /// `harness` 下子命令不认识
    UnknownSubcommand(String),
    /// `run` 既没给 ID 也没给 `--all`
    NoSelector,
    /// `run --all` 同时又给了 ID
    ConflictingSelector,
    /// `--flag` 缺值
    MissingValue(&'static str),
    /// 未知 flag
    UnknownFlag(String),
    /// `lex` 没给文件参数
    LexNeedsFile,
    /// `--mode` 的值不认识（只接受 `comments` / `directives`）
    BadMode(String),
    /// `lex` 给了多于一个文件参数（M1 是单文件词法器）
    ExtraArg(String),
    /// `ast` 没给文件参数（T-AP19）
    AstNeedsFile,
    /// `types` 一个文件参数都没给（M3/Task 2）
    TypesNeedsFile,
    /// `types` 同时给了 `--dump` 与 `--cmp`（两种比对面，输出形状不同）
    DumpCmpConflict,
    /// `check` 一个文件参数都没给（M4/Task 2）
    CheckNeedsFile,
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // 用法说明是硬编码的常量：它必须与 `parse_args` 实际接受的形态一致。
        // ⚠️ 「一致性」靠 `tests/test_driver.rs` 的解析样例组人工对齐（改 USAGE
        // 或改 parse 时在那些样例表里同步）—— 先前注释宣称的「有一条断言在核对」
        // 实际并不存在（T-L18 订正；见账本 R39）。
        const USAGE: &str = "用法：\n  \
             rgoc-driver harness list [--all]\n  \
             rgoc-driver harness run --all | <T-C-nn>...\n  \
             rgoc-driver harness report [--out <路径>]\n  \
             rgoc-driver lex <file> [--mode comments|directives]\n  \
             rgoc-driver ast <file>\n  \
             rgoc-driver types [--dump | --cmp] [--lang go1.N] [-I <root>]... <file>...\n  \
             rgoc-driver check [--diag-dump] [--lang go1.N] [--test-hooks] <file>...";
        match self {
            Self::NoCommand => write!(f, "缺少命令。{USAGE}"),
            Self::UnknownCommand(c) => write!(
                f,
                "未知命令 {c:?}：只实现了 harness、lex、ast、types 与 check 五个子命令（spike 属 Phase 3，尚未开工）。{USAGE}"
            ),
            Self::MissingSubcommand => write!(f, "harness 需要一个子命令。{USAGE}"),
            Self::UnknownSubcommand(c) => write!(f, "harness 下没有 {c:?} 这个子命令。{USAGE}"),
            Self::NoSelector => write!(
                f,
                "run 必须指明范围：--all 或至少一个 T-C-nn（静默跑 0 条会被误读成「全挂」）。{USAGE}"
            ),
            Self::ConflictingSelector => {
                write!(f, "--all 与显式 ID 不能同时给。{USAGE}")
            }
            Self::MissingValue(flag) => write!(f, "{flag} 缺值。{USAGE}"),
            Self::UnknownFlag(flag) => write!(f, "未知选项 {flag:?}。{USAGE}"),
            Self::LexNeedsFile => write!(f, "lex 需要一个文件参数。{USAGE}"),
            Self::BadMode(m) => write!(
                f,
                "--mode 只接受 comments | directives（收到 {m:?}）。{USAGE}"
            ),
            Self::ExtraArg(a) => {
                write!(f, "lex 只接受一个文件参数（多余的：{a:?}）。{USAGE}")
            }
            Self::AstNeedsFile => write!(f, "ast 需要一个文件参数。{USAGE}"),
            Self::TypesNeedsFile => write!(f, "types 至少需要一个文件参数。{USAGE}"),
            Self::DumpCmpConflict => write!(
                f,
                "--dump 与 --cmp 不能同时给（C7 的 dump 与 `//rgoc:cmp` 的判定行是两个比对面）。{USAGE}"
            ),
            Self::CheckNeedsFile => write!(f, "check 至少需要一个文件参数。{USAGE}"),
        }
    }
}

impl std::error::Error for CliError {}

/// `harness` 下的子命令。
const HARNESS_SUBCOMMANDS: [&str; 3] = ["list", "run", "report"];

/// 解析 `argv`（**不含**程序名）。
pub fn parse_args(argv: &[&str]) -> Result<Command, CliError> {
    let Some(top) = argv.first() else {
        return Err(CliError::NoCommand);
    };
    let rest = &argv[1..];
    match *top {
        "harness" => Ok(Command::Harness(parse_harness(rest)?)),
        "lex" => Ok(Command::Lex(parse_lex(rest)?)),
        "ast" => Ok(Command::Ast(parse_ast(rest)?)),
        "types" => Ok(Command::Types(parse_types(rest)?)),
        "check" => Ok(Command::Check(parse_check(rest)?)),
        other => Err(CliError::UnknownCommand(other.to_string())),
    }
}

/// `types [--dump] [--cmp] [--lang go1.N] [-I <root>]... <file>...` —— 与 `lex`/`ast`
/// 同族，但**收多个文件**（M3 的包模型是单包多文件）。
///
/// 选项与文件参数的先后无关。**没实现的长选项一律报错**（T37 的纪律）。
/// `--dump` 与 `--cmp` **互斥**：分别是 C7 的 dump 与 `//rgoc:cmp` 的判定行，
/// 两种输出的形状不同，同时给只会产生一份谁也读不懂的混合文本。
fn parse_types(rest: &[&str]) -> Result<TypesCommand, CliError> {
    let mut files: Vec<PathBuf> = Vec::new();
    let mut roots: Vec<PathBuf> = Vec::new();
    let mut dump = false;
    let mut cmp = false;
    let mut lang: Option<String> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--dump" => {
                dump = true;
                i += 1;
            }
            "--cmp" => {
                cmp = true;
                i += 1;
            }
            "--lang" => {
                let Some(v) = rest.get(i + 1) else {
                    return Err(CliError::MissingValue("--lang"));
                };
                lang = Some((*v).to_string());
                i += 2;
            }
            "-I" => {
                let Some(v) = rest.get(i + 1) else {
                    return Err(CliError::MissingValue("-I"));
                };
                roots.push(PathBuf::from(*v));
                i += 2;
            }
            // `--help` 也走这里：报错文本里带 USAGE，等价于「列出用法」。
            a if a.starts_with('-') => return Err(CliError::UnknownFlag(a.to_string())),
            a => {
                files.push(PathBuf::from(a));
                i += 1;
            }
        }
    }
    if files.is_empty() {
        return Err(CliError::TypesNeedsFile);
    }
    if dump && cmp {
        return Err(CliError::DumpCmpConflict);
    }
    Ok(TypesCommand {
        files,
        dump,
        cmp,
        roots,
        lang,
    })
}

/// `check [--diag-dump] [--lang go1.N] [--test-hooks] <file>...` —— M4 的检查子命令。
///
/// 比 `types` **少** `-I` / `--dump` / `--cmp`：首轮对撞面逐文件单跑（`C8` §2），
/// import 解析与 dump 形态的差异各自归位（`--diag-dump` 是 C8 入口，不是 C7 的）。
/// 未知选项一律报错（T37 纪律，与 `types` 同一条）。
fn parse_check(rest: &[&str]) -> Result<CheckCommand, CliError> {
    let mut files: Vec<PathBuf> = Vec::new();
    let mut diag_dump = false;
    let mut lang: Option<String> = None;
    let mut test_hooks = false;
    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--diag-dump" => {
                diag_dump = true;
                i += 1;
            }
            "--test-hooks" => {
                test_hooks = true;
                i += 1;
            }
            "--lang" => {
                let Some(v) = rest.get(i + 1) else {
                    return Err(CliError::MissingValue("--lang"));
                };
                lang = Some((*v).to_string());
                i += 2;
            }
            // `--help` 也走这里：报错文本里带 USAGE，等价于「列出用法」。
            a if a.starts_with('-') => return Err(CliError::UnknownFlag(a.to_string())),
            a => {
                files.push(PathBuf::from(a));
                i += 1;
            }
        }
    }
    if files.is_empty() {
        return Err(CliError::CheckNeedsFile);
    }
    Ok(CheckCommand {
        files,
        diag_dump,
        lang,
        test_hooks,
    })
}

/// `ast <file>` —— 与 `lex` 同形：文件参数一个、无 flag（T-AP19）。
fn parse_ast(rest: &[&str]) -> Result<AstCommand, CliError> {
    let mut file: Option<PathBuf> = None;
    for a in rest {
        if let Some(flag) = a.strip_prefix("--") {
            // `--help` 也走这里：报错文本里带 USAGE，等价于「列出用法」。
            // 与 T37 的纪律一致：不为单一 flag 开特例通道。
            return Err(CliError::UnknownFlag(format!("--{flag}")));
        }
        if file.is_some() {
            return Err(CliError::ExtraArg(a.to_string()));
        }
        file = Some(PathBuf::from(a));
    }
    let Some(file) = file else {
        return Err(CliError::AstNeedsFile);
    };
    Ok(AstCommand { file })
}

fn parse_harness(rest: &[&str]) -> Result<HarnessCommand, CliError> {
    let Some(sub) = rest.first() else {
        return Err(CliError::MissingSubcommand);
    };
    if !HARNESS_SUBCOMMANDS.contains(sub) {
        return Err(CliError::UnknownSubcommand((*sub).to_string()));
    }
    let rest = &rest[1..];
    match *sub {
        "list" => parse_list(rest),
        "run" => parse_run(rest),
        "report" => parse_report(rest),
        // 上面已用 HARNESS_SUBCOMMANDS 挡住，这里只是让 match 穷尽
        other => Err(CliError::UnknownSubcommand(other.to_string())),
    }
}

/// `lex <file> [--mode comments|directives]` —— flag 与文件参数的先后无关。
fn parse_lex(rest: &[&str]) -> Result<LexCommand, CliError> {
    let mut file: Option<PathBuf> = None;
    let mut mode = LexMode::None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--mode" => {
                let Some(v) = rest.get(i + 1) else {
                    return Err(CliError::MissingValue("--mode"));
                };
                mode = match *v {
                    "comments" => LexMode::Comments,
                    "directives" => LexMode::Directives,
                    other => return Err(CliError::BadMode(other.to_string())),
                };
                i += 2;
            }
            a if a.starts_with("--") => return Err(CliError::UnknownFlag(a.to_string())),
            a => {
                if file.is_some() {
                    return Err(CliError::ExtraArg(a.to_string()));
                }
                file = Some(PathBuf::from(a));
                i += 1;
            }
        }
    }
    let Some(file) = file else {
        return Err(CliError::LexNeedsFile);
    };
    Ok(LexCommand { file, mode })
}

fn parse_list(rest: &[&str]) -> Result<HarnessCommand, CliError> {
    let mut all = false;
    for a in rest {
        match *a {
            "--all" => all = true,
            other => return Err(CliError::UnknownFlag(other.to_string())),
        }
    }
    Ok(HarnessCommand::List { all })
}

fn parse_run(rest: &[&str]) -> Result<HarnessCommand, CliError> {
    let mut ids: Vec<String> = Vec::new();
    let mut all = false;
    for a in rest {
        if *a == "--all" {
            all = true;
        } else if let Some(flag) = a.strip_prefix("--") {
            // 未知的长选项要说清是哪个，别笼统报「参数错误」
            return Err(CliError::UnknownFlag(format!("--{flag}")));
        } else {
            ids.push((*a).to_string());
        }
    }
    if all && !ids.is_empty() {
        return Err(CliError::ConflictingSelector);
    }
    if !all && ids.is_empty() {
        return Err(CliError::NoSelector);
    }
    Ok(HarnessCommand::Run { ids, all })
}

fn parse_report(rest: &[&str]) -> Result<HarnessCommand, CliError> {
    let mut out: Option<PathBuf> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--out" => {
                let Some(v) = rest.get(i + 1) else {
                    return Err(CliError::MissingValue("--out"));
                };
                out = Some(PathBuf::from(*v));
                i += 2;
            }
            other => return Err(CliError::UnknownFlag(other.to_string())),
        }
    }
    Ok(HarnessCommand::Report { out })
}
