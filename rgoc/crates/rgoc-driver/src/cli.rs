//! CLI 参数解析 —— **手写，不引 clap**。
//!
//! # 为什么不引解析库
//!
//! M0 的 CLI 只有 `harness` 一个子命令、三个动作（`list` / `run` / `report`），
//! 引 clap 要拉 4–5 个 crate 进 workspace，而 M0 阶段 workspace 是**零依赖**的
//! （`rgoc-harness/Cargo.toml` 的 `[dependencies]` 是空的）。为了让 3 个动作引入
//! 一棵依赖树不划算，且会让「离线可构建」这个属性破掉。
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
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // 用法说明是硬编码的常量：它必须与 `parse_args` 实际接受的形态一致，
        // 所以测试里有一条断言在核对「错误信息里出现的用法真能被解析成功」。
        const USAGE: &str = "用法：\n  \
             rgoc-driver harness list [--all]\n  \
             rgoc-driver harness run --all | <T-C-nn>...\n  \
             rgoc-driver harness report [--out <路径>]";
        match self {
            Self::NoCommand => write!(f, "缺少命令。{USAGE}"),
            Self::UnknownCommand(c) => write!(
                f,
                "未知命令 {c:?}：M0 只实现了 harness 一个子命令（spike 属 Phase 3，尚未开工）。{USAGE}"
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
        }
    }
}

impl std::error::Error for CliError {}

/// M0 支持的顶层命令（**只有这一个**）。
const TOP_COMMANDS: [&str; 1] = ["harness"];

/// `harness` 下的子命令。
const HARNESS_SUBCOMMANDS: [&str; 3] = ["list", "run", "report"];

/// 解析 `argv`（**不含**程序名）。
pub fn parse_args(argv: &[&str]) -> Result<Command, CliError> {
    let Some(top) = argv.first() else {
        return Err(CliError::NoCommand);
    };
    if !TOP_COMMANDS.contains(top) {
        return Err(CliError::UnknownCommand((*top).to_string()));
    }
    let Some(sub) = argv.get(1) else {
        return Err(CliError::MissingSubcommand);
    };
    if !HARNESS_SUBCOMMANDS.contains(sub) {
        return Err(CliError::UnknownSubcommand((*sub).to_string()));
    }
    let rest = &argv[2..];
    match *sub {
        "list" => Ok(Command::Harness(parse_list(rest)?)),
        "run" => Ok(Command::Harness(parse_run(rest)?)),
        "report" => Ok(Command::Harness(parse_report(rest)?)),
        // 上面已用 HARNESS_SUBCOMMANDS 挡住，这里只是让 match 穷尽
        other => Err(CliError::UnknownSubcommand(other.to_string())),
    }
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
