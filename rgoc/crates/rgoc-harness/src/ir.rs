//! Test IR —— C2 契约（Test IR / 构建条件 / 比较器 / 结果格式）的载体。
//!
//! 字段清单的来源：`docs/milestones/M0-tests.md`（§1.2 Test IR 必录字段、§6.1 冻结口径、
//! §7.5 超时与资源上限）与 `docs/03-roadmap.md` §3.1。
//! 本文件是 **T31** 的产物；指令解析（T32）、语料枚举（T33）、oracle 调用（T34）、
//! 比较器（T35）在后续任务里补齐。
//!
//! 三条设计原则（都是踩过才知道的）：
//!
//! 1. **必录字段不是「建议填」**。缺字段必须让 [`TestCase::validate`] 报错，
//!    不能静默填默认值 —— 静默默认值会让「漏填」伪装成「通过」。
//! 2. **判定分类不得合并**（[`Verdict`] 八种）。合并 `harness-failure` 与
//!    `compiler-failure` 就等于把基建失败算成被测件的语义失败（`03` §3.3）。
//! 3. **超时与资源上限只在这里定义一次**（[`Limits::for_layer`]），取值照
//!    `M0-tests.md` §7.5 的冻结值。散落在各处就会出现两套预算，而门禁只看一套。

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

/// 用例 ID（如 `T-C-01`）。包一层新类型是为了避免把 ID 当自由字符串随手传。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CaseId(String);

impl CaseId {
    /// 取得 ID 文本。
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 是否为空（`validate` 用它挡「漏填 ID」）。
    pub fn is_empty(&self) -> bool {
        self.0.trim().is_empty()
    }
}

impl From<&str> for CaseId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for CaseId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl fmt::Display for CaseId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// 用例模式：v0 支持集 3 种 + `M0-tests.md` §6 已冻结的不支持模式。
///
/// 之所以把**不支持**的模式也建模进来：harness 遇到它们必须能**显式分类**
/// （`expected-unsupported`），而不是静默跳过（`03` §3.3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    // ── v0 支持集（M0-tests §6.1 的口径）──
    /// `// run`：编译并运行，比对输出。
    Run,
    /// `// compile`：只编译，编译成功即通过。
    Compile,
    /// `// errorcheck`：期望编译**失败**，逐条比对诊断。
    ErrorCheck,
    // ── 以下均为 §6 的 unsupported 清单（U1–U13）──
    /// `rundir`（U1）
    RunDir,
    /// `runindir`（U1）
    RunIndir,
    /// `runoutput`（U2）
    RunOutput,
    /// `errorcheckdir`（U3）
    ErrorCheckDir,
    /// `asmcheck`（U4）
    AsmCheck,
    /// `build`（U5）
    Build,
    /// `builddir`（U5）
    BuildDir,
    /// `buildrun`（U5）
    BuildRun,
    /// `buildrundir`（U5）
    BuildRunDir,
    /// `compiledir`（U5）
    CompileDir,
    /// `errorcheckoutput`（U6）
    ErrorCheckOutput,
    /// `errorcheckandrundir`（U6）
    ErrorCheckAndRunDir,
    /// `errorcheckwithauto`（U6）
    ErrorCheckWithAuto,
    /// `skip`：上游设计即跳过（U13，官方 `t.Skip("skip")`）
    Skip,
}

impl Mode {
    /// 全部模式变体（含 v0 支持集与 §6 已冻结的不支持模式）。
    ///
    /// 有了它，「**16 个指令名 + `skip` == 全部模式**」这条不变量才能被断言 ——
    /// 否则「加了 Mode 变体却忘了加指令名」（或反过来）会让真实语料里的文件
    /// 被误判成未知指令，而这种漏项在类型层面查不出来。
    pub const ALL: [Self; 17] = [
        Self::Run,
        Self::Compile,
        Self::ErrorCheck,
        Self::RunDir,
        Self::RunIndir,
        Self::RunOutput,
        Self::ErrorCheckDir,
        Self::AsmCheck,
        Self::Build,
        Self::BuildDir,
        Self::BuildRun,
        Self::BuildRunDir,
        Self::CompileDir,
        Self::ErrorCheckOutput,
        Self::ErrorCheckAndRunDir,
        Self::ErrorCheckWithAuto,
        Self::Skip,
    ];

    /// 是否属于 **v0 支持集**（决定它是否进入 M0 分母，见 `M0-tests.md` §6.1）。
    pub fn is_v0_supported(self) -> bool {
        matches!(self, Self::Run | Self::Compile | Self::ErrorCheck)
    }

    /// 官方指令名（报告与 Test IR 里都写它，便于与语料对照）。
    pub fn name(self) -> &'static str {
        match self {
            Self::Run => "run",
            Self::Compile => "compile",
            Self::ErrorCheck => "errorcheck",
            Self::RunDir => "rundir",
            Self::RunIndir => "runindir",
            Self::RunOutput => "runoutput",
            Self::ErrorCheckDir => "errorcheckdir",
            Self::AsmCheck => "asmcheck",
            Self::Build => "build",
            Self::BuildDir => "builddir",
            Self::BuildRun => "buildrun",
            Self::BuildRunDir => "buildrundir",
            Self::CompileDir => "compiledir",
            Self::ErrorCheckOutput => "errorcheckoutput",
            Self::ErrorCheckAndRunDir => "errorcheckandrundir",
            Self::ErrorCheckWithAuto => "errorcheckwithauto",
            Self::Skip => "skip",
        }
    }

    /// 该模式在 `M0-tests.md` §6 的 unsupported 清单里的编号（v0 模式返回 `None`）。
    pub fn unsupported_code(self) -> Option<&'static str> {
        match self {
            Self::RunDir | Self::RunIndir => Some("U1"),
            Self::RunOutput => Some("U2"),
            Self::ErrorCheckDir => Some("U3"),
            Self::AsmCheck => Some("U4"),
            Self::Build
            | Self::BuildDir
            | Self::BuildRun
            | Self::BuildRunDir
            | Self::CompileDir => Some("U5"),
            Self::ErrorCheckOutput | Self::ErrorCheckAndRunDir | Self::ErrorCheckWithAuto => {
                Some("U6")
            }
            Self::Skip => Some("U13"),
            _ => None,
        }
    }
}

/// 比较器：拿什么去判「结果对不对」。
///
/// 三种形态直接对应官方的三条命令与比较方式（`M0-tests.md` R6）：
/// v0 支持集的模式与比较器**一一对应**，错配即 `validate` 报错。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comparator {
    /// `run` 层：把子进程的 **stdout 与 stderr 合并流**与 `.out` **严格相等**比较。
    ///
    /// 「合并」这件事是 R2b（`testdir_test.go:642-647`）：官方 `runcmd` 把两个流写进
    /// 同一个 buffer 再交给 `checkExpectedOutput`。`helloworld.go` / `printbig.go`
    /// 用内建 `print`（写 **stderr**），只捕 stdout 会让它们「输出为空」而误判失败。
    /// 另外 **缺 `.out` 即期望为空**（R2），不是「任意输出都通过」。
    MergedStreamStrictEq,
    /// `compile` 层：编译**成功**即通过（退出码 0 且无诊断），不比对输出。
    ExitCodeOnly,
    /// `errorcheck` 层：逐条诊断比对（诊断切分 R3 / ERROR 期望 R4）。
    ErrorRegexPerDiag,
}

/// 目标平台与 oracle 版本。
///
/// `go_version` 是**硬约束**：必须精确 `go1.27.1`（D-M0-1）。版本不符时 oracle
/// 必须被拒绝（`M0-tests.md` T-H-06），判 `reference-toolchain-failure`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// 目标 GOOS（首发固定 `linux`）
    pub goos: String,
    /// 目标 GOARCH（首发固定 `arm64`）
    pub goarch: String,
    /// oracle 的 Go 版本（必须精确匹配语料版本）
    pub go_version: String,
}

/// 一条期望诊断（`errorcheck` 层）。
///
/// 对应官方 `wantedError`（`testdir_test.go:1440-1499`）：**前缀是 `<短文件名>:<行号>`**，
/// 模式是该行注释里 `// ERROR "…"` 的**每一个**引号内容（同一行可有多个）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedDiag {
    /// 期望落在源码的哪一行（`LINE±n` 已在解析阶段折算成绝对行号）
    pub line: u32,
    /// 未锚定的正则模式（官方用 `regexp.MatchString`，**不**加 `^`/`$`）
    pub pattern: String,
}

impl ExpectedDiag {
    /// 构造一条期望诊断。
    pub fn at_line(line: u32, pattern: impl Into<String>) -> Self {
        Self {
            line,
            pattern: pattern.into(),
        }
    }
}

/// 期望结果。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Expected {
    /// 期望的退出码。**`errorcheck` 模式必须非 0** —— 官方 `wantError` 语义下，
    /// 编译「成功」反而是失败（`testdir_test.go:792-798`）。
    pub exit_code: i32,
    /// 期望的输出（`run` 层）。**空串表示「必须无输出」**（R2：缺 `.out` 即期望为空）。
    pub stdout: String,
    /// 是否存在 `.out` 文件。它只决定**报错措辞**（R2：有 `.out` = 内容不符 /
    /// 无 `.out` = 本应为空却非空），不改变比较逻辑。
    pub has_out_file: bool,
    /// 期望的诊断（`errorcheck` 层）
    pub diagnostics: Vec<ExpectedDiag>,
}

/// 超时与资源上限。
///
/// **整个 M0 只有这一个定义处**（[`Limits::for_layer`]），数值照 `M0-tests.md` §7.5
/// 的冻结值。单个用例可以放宽 RSS（如 `T-C-03` 的 `gc1.go` 分配测试），
/// 但必须显式调用 [`Limits::with_rss_override`]，不能就地改全局。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// 单个用例的 wall time 上限
    pub per_case: Duration,
    /// 整层的 wall time 上限
    pub layer_total: Duration,
    /// 单用例峰值 RSS 上限（字节）
    pub max_rss_bytes: u64,
}

impl Limits {
    /// 取某一层的**冻结**预算（`M0-tests.md` §7.5）。
    pub fn for_layer(layer: Layer) -> Self {
        let (per_case, layer_total) = match layer {
            // T-H-*：30 s / 3 min（含一个人为的超时用例）
            Layer::HarnessSelfTest => (30, 180),
            // T-C-*：60 s / 5 min（对齐 03 §6.2 smoke ≤ 5 min）
            Layer::Corpus => (60, 300),
            // T-S1 / T-S2：30 s / 1 min（极小程序）
            Layer::SpikeFast => (30, 60),
            // T-S3：120 s / 5 min（含 clang 编译与链接）
            Layer::SpikeNative => (120, 300),
            // 整体 M0 门禁：15 min（远低于 milestone ≤ 30 min）
            Layer::Overall => (900, 900),
        };
        // 单用例峰值 RSS ≤ 512 MiB（T-C-03 单独放宽，见 with_rss_override）
        Self {
            per_case: Duration::from_secs(per_case),
            layer_total: Duration::from_secs(layer_total),
            max_rss_bytes: 512 * 1024 * 1024,
        }
    }

    /// 只放宽 RSS 上限，**不动超时**（如 `T-C-03` 用 768 MiB）。
    pub fn with_rss_override(mut self, bytes: u64) -> Self {
        self.max_rss_bytes = bytes;
        self
    }
}

/// 测试分层。决定用哪一套冻结预算。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Layer {
    /// `T-H-*`：harness 六类自验
    HarnessSelfTest,
    /// `T-C-*`：官方语料基线
    Corpus,
    /// `T-S1-*` / `T-S2-*`：解释与 SSA spike
    SpikeFast,
    /// `T-S3-*`：native spike
    SpikeNative,
    /// 整体 M0 门禁
    Overall,
}

impl Layer {
    /// 分层的可读名（报告里用）。
    pub fn name(self) -> &'static str {
        match self {
            Self::HarnessSelfTest => "T-H",
            Self::Corpus => "T-C",
            Self::SpikeFast => "T-S1/S2",
            Self::SpikeNative => "T-S3",
            Self::Overall => "M0",
        }
    }
}

/// 为什么要排除这个用例（必须能说清落在 §6 清单的哪一条）。
///
/// 「每个不通过的用例都要能说清落在哪一条」（`M0-tests.md` §6）——
/// 只写「不支持」而不给编号，等于把判断责任推给读报告的人。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unsupported {
    /// §6 的编号（`U1`…`U13`）
    pub code: String,
    /// 人可读的原因
    pub reason: String,
}

/// 一个测试用例的完整描述 —— Test IR 的核心类型。
///
/// 字段对应 `M0-tests.md` §1.2 的「Test IR 必录字段」清单，一项不落。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestCase {
    /// 用例 ID（`T-C-01` 等）
    pub id: CaseId,
    /// 相对 `GOROOT/test` 的路径
    pub rel_path: PathBuf,
    /// 输入文件集合（单文件用例就是它自己）
    pub input_files: Vec<PathBuf>,
    /// 模式（v0 支持集或已冻结的不支持模式）
    pub mode: Mode,
    /// 指令参数（`// run` 之后的参数；无参用例为空）
    pub instruction_args: Vec<String>,
    /// build tags（平台过滤用；`M0-tests.md` R1b）
    pub build_tags: Vec<String>,
    /// 目标平台与 oracle 版本
    pub target: Target,
    /// import 需求（`errorcheck` 层要用它算 `-importcfg`）
    pub imports: Vec<String>,
    /// 功能依赖（挂到 `01-feature-set.md` 的功能 ID 上）
    pub feature_deps: Vec<String>,
    /// 比较器
    pub comparator: Comparator,
    /// 期望结果
    pub expected: Expected,
    /// 超时与资源上限
    pub limits: Limits,
    /// 随机种子（固定语料下恒为 0；保留字段是为了将来引入随机化用例）
    pub seed: u64,
    /// 归属阶段（`M0` / `M1` / …）
    pub milestone: String,
    /// 排除原因（不在 v0 支持集时**必填**）
    pub unsupported: Option<Unsupported>,
}

/// [`TestCase::validate`] 的失败原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IrError {
    /// 用例 ID 为空
    EmptyId,
    /// 相对路径为空
    EmptyPath,
    /// 比较器与模式不匹配
    ComparatorMismatch {
        /// 模式名
        mode: &'static str,
        /// 错配的组合
        detail: &'static str,
    },
    /// `errorcheck` 却期望退出码 0（官方 `wantError` 语义下编译成功即失败）
    ErrorcheckExpectsSuccess,
    /// 不在 v0 支持集，但没给 unsupported 说明
    MissingUnsupported {
        /// 模式名
        mode: &'static str,
    },
    /// 给了 unsupported 但缺 U 编号或原因
    IncompleteUnsupported,
}

impl fmt::Display for IrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyId => write!(f, "用例 ID 为空"),
            Self::EmptyPath => write!(f, "相对路径为空"),
            Self::ComparatorMismatch { mode, detail } => {
                write!(f, "模式 {mode} 的比较器不匹配：{detail}")
            }
            Self::ErrorcheckExpectsSuccess => {
                write!(
                    f,
                    "errorcheck 模式期望退出码 0（官方 wantError：编译成功反而是失败）"
                )
            }
            Self::MissingUnsupported { mode } => {
                write!(
                    f,
                    "模式 {mode} 不在 v0 支持集，必须给 unsupported 说明（U 编号 + 原因）"
                )
            }
            Self::IncompleteUnsupported => write!(f, "unsupported 说明缺 U 编号或原因"),
        }
    }
}

impl std::error::Error for IrError {}

impl TestCase {
    /// 校验必录字段与字段间的一致性。
    ///
    /// **只校验下列几条**——它们是会让「结果不可信」的那些：
    /// 空 ID、空路径、比较器/模式错配、`errorcheck` 却期望成功、
    /// 非 v0 模式缺 unsupported 说明。其余字段（imports / feature_deps / build_tags
    /// 等）目前只**记录**不校验，等 T33（枚举）与 T35（比较器）真正用到时再补 ——
    /// 与其现在就写没人验证的分支，不如等有消费者时再加。
    pub fn validate(&self) -> Result<(), IrError> {
        if self.id.is_empty() {
            return Err(IrError::EmptyId);
        }
        if self.rel_path.as_os_str().is_empty() {
            return Err(IrError::EmptyPath);
        }
        if let Some(detail) = comparator_mismatch(self.mode, self.comparator) {
            return Err(IrError::ComparatorMismatch {
                mode: self.mode.name(),
                detail,
            });
        }
        if self.mode == Mode::ErrorCheck && self.expected.exit_code == 0 {
            return Err(IrError::ErrorcheckExpectsSuccess);
        }
        if !self.mode.is_v0_supported() && self.unsupported.is_none() {
            return Err(IrError::MissingUnsupported {
                mode: self.mode.name(),
            });
        }
        if let Some(u) = &self.unsupported
            && (u.code.trim().is_empty() || u.reason.trim().is_empty())
        {
            return Err(IrError::IncompleteUnsupported);
        }
        Ok(())
    }
}

/// v0 支持集的模式与比较器必须一一对应；非 v0 模式不做要求（它们本来就不跑）。
fn comparator_mismatch(mode: Mode, cmp: Comparator) -> Option<&'static str> {
    let want = match mode {
        Mode::Run => Comparator::MergedStreamStrictEq,
        Mode::Compile => Comparator::ExitCodeOnly,
        Mode::ErrorCheck => Comparator::ErrorRegexPerDiag,
        _ => return None,
    };
    (cmp != want).then_some(match want {
        Comparator::MergedStreamStrictEq => {
            "run 层必须用 MergedStreamStrictEq（stdout+stderr 合并流严格相等）"
        }
        Comparator::ExitCodeOnly => "compile 层必须用 ExitCodeOnly（编译成功即通过）",
        Comparator::ErrorRegexPerDiag => "errorcheck 层必须用 ErrorRegexPerDiag（逐条诊断比对）",
    })
}

/// 一个用例的判定结果 —— **八种，不得合并**。
///
/// 「只有 `pass` 计入分子」是门禁纪律（`03` §3.3）：过滤项、基建失败、
/// 超时、资源不足都**不得**记为 pass，也**不得**从分母里拿掉。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Verdict {
    /// 通过（唯一计入分子）
    Pass,
    /// 被测件**编译**失败（errorcheck 层的预期失败也算通过，不落在这里）
    CompilerFailure,
    /// 被测件编译过了但**运行**出错
    RuntimeFailure,
    /// **harness 自己**坏了（指令解析失败、oracle 调用异常、比较器崩了）
    HarnessFailure,
    /// 被平台过滤（`03` §3.3：不计入分子、仍计入分母）
    TargetFiltered,
    /// 超时（子进程必须已被真正回收）
    Timeout,
    /// 超出资源上限（峰值 RSS 等）
    ResourceFailure,
    /// **oracle 工具链**版本不符（`04` §7：版本不符即拒绝作基线）
    ReferenceToolchainFailure,
}

impl Verdict {
    /// 全部八种（顺序固定，报告按它聚合）。
    pub const ALL: [Self; 8] = [
        Self::Pass,
        Self::CompilerFailure,
        Self::RuntimeFailure,
        Self::HarnessFailure,
        Self::TargetFiltered,
        Self::Timeout,
        Self::ResourceFailure,
        Self::ReferenceToolchainFailure,
    ];

    /// 稳定的标识（报告与 manifest 的 `test_ids` 用它）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::CompilerFailure => "compiler-failure",
            Self::RuntimeFailure => "runtime-failure",
            Self::HarnessFailure => "harness-failure",
            Self::TargetFiltered => "target-filtered",
            Self::Timeout => "timeout",
            Self::ResourceFailure => "resource-failure",
            Self::ReferenceToolchainFailure => "reference-toolchain-failure",
        }
    }

    /// 是否计入分子（**只有** `pass` 计入）。
    pub fn counts_toward_numerator(self) -> bool {
        self == Self::Pass
    }

    /// 是否属「基建类」问题（判卷机或 oracle 的问题，**不是**被测件的 bug）。
    ///
    /// 之所以要能区分：`harness-failure` 若被当成 `compiler-failure`，
    /// 就会把「我们的测试工具坏了」误报成「我们的编译器有 bug」。
    pub fn is_infrastructure(self) -> bool {
        matches!(
            self,
            Self::HarnessFailure | Self::ReferenceToolchainFailure | Self::ResourceFailure
        )
    }
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §7.5 的冻结预算：数值写死在这里当锚，防止有人「顺手调大」。
    #[test]
    fn limits_锚在冻结值上() {
        assert_eq!(
            Limits::for_layer(Layer::Corpus).per_case,
            Duration::from_secs(60)
        );
        assert_eq!(
            Limits::for_layer(Layer::SpikeNative).layer_total,
            Duration::from_secs(300)
        );
        assert_eq!(
            Limits::for_layer(Layer::Overall).layer_total,
            Duration::from_secs(900)
        );
    }

    #[test]
    fn verdict_八种标识互不相同() {
        let mut v: Vec<&str> = Verdict::ALL.iter().map(|x| x.as_str()).collect();
        v.sort_unstable();
        let n = v.len();
        v.dedup();
        assert_eq!(v.len(), n, "判定标识必须唯一");
    }

    #[test]
    fn mode_的_unsupported_编号与_m0_tests_一致() {
        assert_eq!(Mode::RunOutput.unsupported_code(), Some("U2"));
        assert_eq!(Mode::ErrorCheckWithAuto.unsupported_code(), Some("U6"));
        assert_eq!(Mode::Skip.unsupported_code(), Some("U13"));
        assert_eq!(Mode::Run.unsupported_code(), None);
    }
}
