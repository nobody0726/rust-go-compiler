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
    /// `run` 子命令（M5/Task 27）
    Run(RunCommand),
    /// `ssa` 子命令（M6/Task 2 起登记；IR 从 Task 16 起才有内容可 dump）
    Ssa(SsaCommand),
    /// `build` 子命令（M7/Task 2 起登记；真身从 Task 50 起）
    Build(BuildCommand),
}

/// `run` 子命令的参数（M5/Task 27；`M5-design.md` §4.1）。
///
/// # 与 `check` 的分工
///
/// `check` 只出诊断；`run` 在诊断为空时**真的执行**，把程序的
/// `print` / `println` 输出（合并流）打到 stdout。两者共用同一条装载 + 检查
/// 通路，`run` 多一步 `rgoc_hir::lower` + `rgoc_interp::run`。
///
/// # `--dump-hir` 为什么**不跑**
///
/// 与 `--diag-dump` 同一条纪律：dump 是给对撞看的中间产物，程序输出是
/// 给字节比较看的成品。两者写同一个 stdout 会让「哪几个字节是程序的」变得
/// 不可判 —— 那样 `run-collide` 就只能靠猜。所以给了 `--dump-hir` 就**只**
/// 出 dump（退出码 0），跑程序请去掉这个旗标。
/// 执行通路（`run --via`，M6/Task 37）。
///
/// # 为什么要有这个开关
///
/// M6 之后同一个程序有**两条**执行通路：M5 的 HIR 解释器（`hir`，默认）与
/// M6 的 SSA 求值器（`ssa`）。默认必须是 `hir` —— 语料 69/69 的官方对撞是
/// 在它上面冻结的，换默认等于悄悄换判据。
///
/// `ssa` 存在的意义是**三方差分**（`T-SSA-32`）：同一个程序两条通路输出必须
/// 逐字节一致，不一致就说明 SSA 侧错了。所以它必须能单独选，而不是「都跑一遍」。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Via {
    /// M5 的 HIR 解释器（默认）。
    #[default]
    Hir,
    /// M6 的 SSA 求值器（`rgoc_ssa::build` → `verify` → `run_ssa`）。
    Ssa,
}

impl Via {
    /// 解析 `--via` 的值（只接受这两个词；别的一律报错，不猜）。
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "hir" => Some(Via::Hir),
            "ssa" => Some(Via::Ssa),
            _ => None,
        }
    }

    /// 稳定文本（进诊断与测试断言）。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Via::Hir => "hir",
            Via::Ssa => "ssa",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunCommand {
    /// 组成**同一个包**的文件。
    pub files: Vec<PathBuf>,
    /// `--dump-hir`：只输出 HIR 的确定性 dump（`T-EX-04` 的对撞入口）。
    pub dump_hir: bool,
    /// `--max-steps N`：步数预算覆盖（缺省 1e8；`D-M5-12`）。
    pub max_steps: Option<u64>,
    /// `--lang go1.N`：版本门控覆盖（与 `check` 同义）。
    pub lang: Option<String>,
    /// `--via hir|ssa`：走哪条执行通路（默认 `hir`）。
    pub via: Via,
    /// `--opt all|none|fold,dce,…`：SSA passes 的开关（只在 `via=ssa` 时有意义）。
    pub opt: rgoc_ssa::opt::Config,
}

/// `ssa` 子命令的参数（M6/Task 2；`M6-design.md` §3.1 / `C10`）。
///
/// # 为什么必须显式给 `--dump` 或 `--verify`
///
/// 两个旗标代表**两个不同的比对面**：dump 是给人/对撞看的结构文本，
/// verify 是给构建器看的自检。默认做哪一个都是猜 —— 猜错的代价是
/// 「跑通了但什么都没检查」。所以两个都不给就报错（`CliError::SsaNeedsMode`）。
///
/// # 通道纪律
///
/// dump 只进 stdout，verifier 错误只进 stderr（`T-SSA-38`）。两者同时给时
/// **先 dump 后 verify**，且两份输出都在（`M6-tests.md` §3.4）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsaCommand {
    /// 组成**同一个包**的文件。
    pub files: Vec<PathBuf>,
    /// `--dump`：输出 `C10` 中立 dump。
    pub dump: bool,
    /// `--verify`：跑 verifier 九条不变式。
    pub verify: bool,
    /// `--opt all|none|fold,dce,…`：跑哪些 pass（默认全开）。
    pub opt: rgoc_ssa::opt::Config,
}

/// `build` 子命令的产物形态（M7/Task 2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Emit {
    /// 只到 ELF（默认）：`.s` 是中间件，除非 `--keep-asm` 否则删掉。
    #[default]
    Elf,
    /// 只到 `.s`：不出 ELF（用来做 `T-NAT-48` 的逐字节可复现判据）。
    Asm,
}

impl Emit {
    /// 解析 `--emit` 的值（只接受 `elf` / `asm`）。
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "elf" => Some(Self::Elf),
            "asm" => Some(Self::Asm),
            _ => None,
        }
    }
}

/// `build` 子命令的参数（M7/Task 2 登记；真身从 Task 50 起）。
///
/// # 与 `run` / `ssa` 的分工
///
/// `run` 在 Rust 进程里解释执行，`ssa` 只出结构文本；`build` 是**唯一产出可执行文件**的入口。
/// 三者的前半段（装载 → 检查 → HIR → SSA）完全共用，差别只在最后一步。
///
/// # 为什么默认输出路径要**确定**
///
/// `--emit=asm` 的判据是「同一输入两次逐字节相同」（`T-NAT-48`）。如果输出名里掺了
/// 进程号或时间，那条判据就只能靠 `cmp` 两份不同名的文件 —— 判据本身没变，但
/// 装置会开始「先找文件」而不再是「比内容」。所以默认名固定为 `<stem>`（无扩展名），
/// 与源文件同目录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildCommand {
    /// 组成**同一个包**的文件。
    pub files: Vec<PathBuf>,
    /// `-o PATH`：产物路径（缺省 `<stem>`，与源文件同目录）。
    pub output: Option<PathBuf>,
    /// `--emit elf|asm`：产物形态（默认 `elf`）。
    pub emit: Emit,
    /// `--keep-asm`：保留中间的 `.s`（`T-NAT-47`）。
    pub keep_asm: bool,
    /// `--opt all|none|fold,dce,…`：SSA passes 的开关（默认全开）。
    pub opt: rgoc_ssa::opt::Config,
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
    /// `run` 一个文件参数都没给（M5/Task 27）
    RunNeedsFile,
    /// `ssa` 一个文件参数都没给（M6/Task 2）
    SsaNeedsFile,
    /// `ssa` 既没给 `--dump` 也没给 `--verify`（M6/Task 2）
    SsaNeedsMode,
    /// `--max-steps` 的值不是非负整数
    BadSteps(String),
    /// `--via` 的值不认识（只接受 `hir` / `ssa`）
    BadVia(String),
    /// `--opt` 的值不认识（只接受 `all` / `none` / `fold,dce,copy,cse`）
    BadOpt(String),
    /// `build` 一个文件参数都没给（M7/Task 2）
    BuildNeedsFile,
    /// `--emit` 的值不认识（只接受 `elf` / `asm`）
    BadEmit(String),
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
             rgoc-driver check [--diag-dump] [--lang go1.N] [--test-hooks] <file>...\n  \
             rgoc-driver run [--dump-hir] [--max-steps N] [--lang go1.N] [--via hir|ssa] [--opt=…] <file>...\n  \
             rgoc-driver ssa [--dump] [--verify] [--opt=…] <file>...\n  \
             rgoc-driver build [-o PATH] [--emit elf|asm] [--keep-asm] [--opt=…] <file>...";
        match self {
            Self::NoCommand => write!(f, "缺少命令。{USAGE}"),
            Self::UnknownCommand(c) => write!(
                f,
                "未知命令 {c:?}：只实现了 harness、lex、ast、types、check、run、ssa 与 build 八个子命令（spike 属 Phase 3，尚未开工）。{USAGE}"
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
            Self::RunNeedsFile => write!(f, "run 至少需要一个文件参数。{USAGE}"),
            Self::SsaNeedsFile => write!(f, "ssa 至少需要一个文件参数。{USAGE}"),
            Self::SsaNeedsMode => write!(
                f,
                "ssa 需要 --dump 或 --verify（两个都不给就不知道要比对什么）。{USAGE}"
            ),
            Self::BadSteps(v) => write!(f, "--max-steps 需要一个非负整数（收到 {v:?}）。{USAGE}"),
            Self::BadVia(v) => write!(f, "--via 只接受 hir / ssa（收到 {v:?}）。{USAGE}"),
            Self::BadOpt(v) => write!(
                f,
                "--opt 只接受 all / none / fold,dce,copy,cse（收到 {v:?}）。{USAGE}"
            ),
            Self::BuildNeedsFile => write!(f, "build 至少需要一个文件参数。{USAGE}"),
            Self::BadEmit(v) => write!(f, "--emit 只接受 elf / asm（收到 {v:?}）。{USAGE}"),
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
        "run" => Ok(Command::Run(parse_run_sub(rest)?)),
        "ssa" => Ok(Command::Ssa(parse_ssa(rest)?)),
        "build" => Ok(Command::Build(parse_build(rest)?)),
        other => Err(CliError::UnknownCommand(other.to_string())),
    }
}

/// `run [--dump-hir] [--max-steps N] [--lang go1.N] <file>...` —— M5 的执行子命令。
///
/// 选项与文件参数的先后无关；未知选项一律报错（T37 纪律，与 `types`/`check` 同）。
fn parse_run_sub(rest: &[&str]) -> Result<RunCommand, CliError> {
    let mut files: Vec<PathBuf> = Vec::new();
    let mut dump_hir = false;
    let mut max_steps: Option<u64> = None;
    let mut lang: Option<String> = None;
    let mut via = Via::Hir;
    let mut opt = rgoc_ssa::opt::Config::all();
    let mut i = 0;
    while i < rest.len() {
        // `--via=ssa` 与 `--via ssa` 两种写法都收（`run-collide.sh` 用前者，
        // 人敲命令习惯后者；只收一种就等于让另一个入口静默失灵）。
        if let Some(v) = rest[i].strip_prefix("--via=") {
            let Some(v2) = Via::parse(v) else {
                return Err(CliError::BadVia(v.to_string()));
            };
            via = v2;
            i += 1;
            continue;
        }
        if let Some(v) = rest[i].strip_prefix("--opt=") {
            let Some(c) = rgoc_ssa::opt::Config::parse(v) else {
                return Err(CliError::BadOpt(v.to_string()));
            };
            opt = c;
            i += 1;
            continue;
        }
        match rest[i] {
            "--dump-hir" => {
                dump_hir = true;
                i += 1;
            }
            "--max-steps" => {
                let Some(v) = rest.get(i + 1) else {
                    return Err(CliError::MissingValue("--max-steps"));
                };
                let n = v
                    .parse::<u64>()
                    .map_err(|_| CliError::BadSteps((*v).to_string()))?;
                max_steps = Some(n);
                i += 2;
            }
            "--lang" => {
                let Some(v) = rest.get(i + 1) else {
                    return Err(CliError::MissingValue("--lang"));
                };
                lang = Some((*v).to_string());
                i += 2;
            }
            "--via" => {
                let Some(v) = rest.get(i + 1) else {
                    return Err(CliError::MissingValue("--via"));
                };
                let Some(v2) = Via::parse(v) else {
                    return Err(CliError::BadVia((*v).to_string()));
                };
                via = v2;
                i += 2;
            }
            "--opt" => {
                let Some(v) = rest.get(i + 1) else {
                    return Err(CliError::MissingValue("--opt"));
                };
                let Some(c) = rgoc_ssa::opt::Config::parse(v) else {
                    return Err(CliError::BadOpt((*v).to_string()));
                };
                opt = c;
                i += 2;
            }
            a if a.starts_with('-') => return Err(CliError::UnknownFlag(a.to_string())),
            a => {
                files.push(PathBuf::from(a));
                i += 1;
            }
        }
    }
    if files.is_empty() {
        return Err(CliError::RunNeedsFile);
    }
    Ok(RunCommand {
        files,
        dump_hir,
        max_steps,
        lang,
        via,
        opt,
    })
}

fn parse_ssa(rest: &[&str]) -> Result<SsaCommand, CliError> {
    let mut files: Vec<PathBuf> = Vec::new();
    let mut dump = false;
    let mut verify = false;
    let mut opt = rgoc_ssa::opt::Config::all();
    let mut i = 0;
    while i < rest.len() {
        if let Some(v) = rest[i].strip_prefix("--opt=") {
            let Some(c) = rgoc_ssa::opt::Config::parse(v) else {
                return Err(CliError::BadOpt(v.to_string()));
            };
            opt = c;
            i += 1;
            continue;
        }
        match rest[i] {
            "--dump" => {
                dump = true;
                i += 1;
            }
            "--verify" => {
                verify = true;
                i += 1;
            }
            "--opt" => {
                let Some(v) = rest.get(i + 1) else {
                    return Err(CliError::MissingValue("--opt"));
                };
                let Some(c) = rgoc_ssa::opt::Config::parse(v) else {
                    return Err(CliError::BadOpt((*v).to_string()));
                };
                opt = c;
                i += 2;
            }
            a if a.starts_with('-') => return Err(CliError::UnknownFlag(a.to_string())),
            a => {
                files.push(PathBuf::from(a));
                i += 1;
            }
        }
    }
    if files.is_empty() {
        return Err(CliError::SsaNeedsFile);
    }
    if !dump && !verify {
        return Err(CliError::SsaNeedsMode);
    }
    Ok(SsaCommand {
        files,
        dump,
        verify,
        opt,
    })
}

/// `build [-o PATH] [--emit elf|asm] [--keep-asm] [--opt=…] <file>...` —— M7 的产物入口。
///
/// 选项与文件参数的先后无关；未知选项一律报错（T37 纪律，与 `ssa` 同）。
/// `--emit=asm` 与 `--emit asm` 两种写法都收（与 `--via`/`--opt` 同一条理由：
/// 只收一种就等于让另一个入口静默失灵）。
fn parse_build(rest: &[&str]) -> Result<BuildCommand, CliError> {
    let mut files: Vec<PathBuf> = Vec::new();
    let mut output: Option<PathBuf> = None;
    let mut emit = Emit::Elf;
    let mut keep_asm = false;
    let mut opt = rgoc_ssa::opt::Config::all();
    let mut i = 0;
    while i < rest.len() {
        if let Some(v) = rest[i].strip_prefix("--emit=") {
            let Some(e) = Emit::parse(v) else {
                return Err(CliError::BadEmit(v.to_string()));
            };
            emit = e;
            i += 1;
            continue;
        }
        if let Some(v) = rest[i].strip_prefix("--opt=") {
            let Some(c) = rgoc_ssa::opt::Config::parse(v) else {
                return Err(CliError::BadOpt(v.to_string()));
            };
            opt = c;
            i += 1;
            continue;
        }
        match rest[i] {
            "--emit" => {
                let Some(v) = rest.get(i + 1) else {
                    return Err(CliError::MissingValue("--emit"));
                };
                let Some(e) = Emit::parse(v) else {
                    return Err(CliError::BadEmit((*v).to_string()));
                };
                emit = e;
                i += 2;
            }
            "--opt" => {
                let Some(v) = rest.get(i + 1) else {
                    return Err(CliError::MissingValue("--opt"));
                };
                let Some(c) = rgoc_ssa::opt::Config::parse(v) else {
                    return Err(CliError::BadOpt((*v).to_string()));
                };
                opt = c;
                i += 2;
            }
            "-o" => {
                let Some(v) = rest.get(i + 1) else {
                    return Err(CliError::MissingValue("-o"));
                };
                output = Some(PathBuf::from(*v));
                i += 2;
            }
            "--keep-asm" => {
                keep_asm = true;
                i += 1;
            }
            a if a.starts_with('-') => return Err(CliError::UnknownFlag(a.to_string())),
            a => {
                files.push(PathBuf::from(a));
                i += 1;
            }
        }
    }
    if files.is_empty() {
        return Err(CliError::BuildNeedsFile);
    }
    Ok(BuildCommand {
        files,
        output,
        emit,
        keep_asm,
        opt,
    })
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
