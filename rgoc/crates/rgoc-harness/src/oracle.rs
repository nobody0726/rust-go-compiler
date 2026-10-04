//! Oracle 调用与版本守门（T-H-06）。
//!
//! oracle 就是**容器内精确的 `go1.27.1`**（D-M0-1 的硬约束）。本模块负责：
//!
//! 1. **建 oracle 前先校验版本**，不符即拒绝（`M0-tests.md` T-H-06、`04` §7）；
//! 2. 按 R6 的**命令形态**构造 argv（`testdir_test.go:188` / `:787-790` / `:1069-1077`）；
//! 3. **超时后真正终止子进程**（不留孤儿）；
//! 4. 观测峰值 RSS 并与上限比较（读 `/proc/<pid>/status` 的 `VmHWM`，不需要 libc）。
//!
//! **本模块不做比较**（输出与诊断怎么算对错是 T35 的事），只负责「跑」与「看住」。
//!
//! # 为什么 `run` 层走 fast path（compile -> link -> 直跑 exe）
//!
//! 官方有两条路：没有 flags 时走 **fast path**（`:1069-1077`：compile + link + 直跑 exe），
//! 有 flags 或跨平台时才走 `go run`（`:1079-1085`）。M0 的 v0 样本**不带 flags**，
//! 所以走 fast path。这带来一个关键好处：
//!
//! **超时时 `child.kill()` 就够了，不需要杀进程组。**
//! `go run` 拉起的子进程是 `go`，由它再启动被测程序；只 kill `go` 会留下**孤儿进程**
//! —— 那正是 T-H-05 要防的事。fast path 下我们直接跑 exe，父子关系是直的。
//! 官方为了处理 `go run` 的情况要杀进程组（其注释说方案来自 `cmd/go/test.go`），
//! 而本仓 workspace 是 `unsafe_code = "forbid"`，`kill(-pgid, SIGKILL)` 需要 unsafe。
//! **选 fast path 既忠实于官方首选路径，又完全不需要 unsafe** —— 约束反过来改善了设计。

use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use crate::corpus::CorpusConfig;
use crate::ir::{Layer, Limits, Mode};

/// 同一进程内递增，用来让每个 oracle 实例的临时文件**互不覆盖**。
///
/// ⚠️ 本轮踩过：importcfg 原本写在固定路径 `/tmp/m0-importcfg-<pid>` 上，
/// 并行跑多个用例时互相覆盖，导致 `Oracle::new` 随机失败（单跑必过、并行挂）。
/// T38 的 harness 是并行的，所以这个必须是实例级唯一的。
static INSTANCE_SEQ: AtomicU32 = AtomicU32::new(0);

/// oracle 的配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OracleConfig {
    /// `go` 可执行文件（默认走 PATH）
    pub go_tool: PathBuf,
    /// 工作目录（官方 runcmd 默认是 `GOROOT/test`，`:654`）
    pub work_dir: PathBuf,
    /// 必须精确匹配的版本（T-H-06）
    pub expect_version: String,
    /// 必须匹配的 GOOS
    pub expect_goos: String,
    /// 必须匹配的 GOARCH
    pub expect_goarch: String,
    /// 超时与资源上限（**只从 [`Limits`] 取**，不在这里写死）
    pub limits: Limits,
    /// 轮询峰值 RSS 的间隔
    pub rss_poll_interval: Duration,
}

impl From<CorpusConfig> for OracleConfig {
    fn from(c: CorpusConfig) -> Self {
        Self {
            go_tool: PathBuf::from("go"),
            work_dir: PathBuf::from("."),
            expect_version: c.go_version,
            expect_goos: c.goos,
            expect_goarch: c.goarch,
            limits: Limits::for_layer(Layer::Corpus),
            rss_poll_interval: Duration::from_millis(2),
        }
    }
}

/// 一次 oracle 调用的结果。
///
/// 注意 `merged`：**stdout 与 stderr 合并**（R2b，官方 `:645-646` 写进同一个 buffer）。
/// 内建 `print` / `println` 写 stderr，所以只捕 stdout 的实现会让一批样本「输出为空」。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OracleOutput {
    /// 退出码（被信号杀死时为 `None`）
    pub exit_code: Option<i32>,
    /// stdout + stderr 的**合并流**
    pub merged: String,
    /// 是否超时（T-H-05）
    pub timed_out: bool,
    /// 是否超 RSS 上限
    pub resource_exceeded: bool,
    /// 观测到的峰值 RSS（字节）；观测不到时为 0
    pub peak_rss_bytes: u64,
    /// 实际耗时
    pub duration: Duration,
    /// 子进程 pid（测试用它验证「进程被回收」）
    pub pid: Option<u32>,
}

/// oracle 调用失败。
///
/// **这些都是基础设施类错误**：上层要判 `Verdict::ReferenceToolchainFailure` 或
/// `Verdict::HarnessFailure`，**不能**当成被测件的语义失败（`03` §3.3）。
#[derive(Debug)]
pub enum OracleError {
    /// 版本或平台不符，拒绝作基线（T-H-06、`04` §7）
    VersionMismatch {
        /// 期望的 `版本 GOOS/GOARCH`
        expected: String,
        /// 实际拿到的
        got: String,
    },
    /// 生成 stdlib importcfg 失败（`:218-220`）
    ImportCfg(String),
    /// 拉起子进程失败
    Spawn(String),
    /// 与子进程交互失败（读输出等）
    Io(String),
}

impl fmt::Display for OracleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::VersionMismatch { expected, got } => {
                write!(
                    f,
                    "oracle 版本不符：期望 {expected}，实际 {got} —— 拒绝作基线（不得降级）"
                )
            }
            Self::ImportCfg(e) => write!(f, "生成 stdlib importcfg 失败：{e}"),
            Self::Spawn(e) => write!(f, "拉起 oracle 子进程失败：{e}"),
            Self::Io(e) => write!(f, "与 oracle 子进程交互失败：{e}"),
        }
    }
}

impl std::error::Error for OracleError {}

/// 一个执行步骤：**程序 + 参数**。
///
/// 为什么要单独一个类型：fast path 的最后一步跑的是**产出的 exe**，不是 `go`。
/// 早先把它当成 `go` 的子命令，结果得到 `go /tmp/xxx.exe: unknown command`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// 要执行的程序
    pub program: PathBuf,
    /// 传给它的参数
    pub args: Vec<String>,
}

/// 已通过版本守门的 oracle。
#[derive(Debug, Clone)]
pub struct Oracle {
    cfg: OracleConfig,
    version: String,
    goos: String,
    goarch: String,
    importcfg: PathBuf,
    /// 本实例专属的临时目录（importcfg 与 fast path 的 pkg.a / exe 都在这里）
    temp_dir: PathBuf,
}

impl Oracle {
    /// 建 oracle 并**校验版本**（T-H-06）。
    ///
    /// 版本或平台不符直接 `Err`，**不返回任何可用实例** ——
    /// 「版本不符就降级跑别的 Go」是最危险的一种偏差，它会让整份基线失去意义。
    pub fn new(cfg: OracleConfig) -> Result<Self, OracleError> {
        let (version, goos, goarch) = probe_version(&cfg.go_tool, &cfg.work_dir)?;
        if version != cfg.expect_version || goos != cfg.expect_goos || goarch != cfg.expect_goarch {
            return Err(OracleError::VersionMismatch {
                expected: format!(
                    "{} {}/{}",
                    cfg.expect_version, cfg.expect_goos, cfg.expect_goarch
                ),
                got: format!("{version} {goos}/{goarch}"),
            });
        }
        // stdlib 的 export 缓存清单（`:218-220`）。compile 与 errorcheck 层都离不开它：
        // oracle 若没有预置的 stdlib 导出缓存，这两层根本跑不起来。
        // 每个实例一个专属临时目录：**绝不能**写进 work_dir ——
        // 真实语料（GOROOT/test）是**只读挂载**的（04 §7 / T11 实测过）
        let seq = INSTANCE_SEQ.fetch_add(1, Ordering::Relaxed);
        let temp_dir = std::env::temp_dir().join(format!("m0-oracle-{}-{seq}", std::process::id()));
        std::fs::create_dir_all(&temp_dir).map_err(|e| OracleError::Io(e.to_string()))?;
        let importcfg = build_importcfg(&cfg, &temp_dir)?;
        Ok(Self {
            cfg,
            version,
            goos,
            goarch,
            importcfg,
            temp_dir,
        })
    }

    /// 实际拿到的 Go 版本（形如 `go1.27.1`）。
    pub fn version(&self) -> &str {
        &self.version
    }

    /// 实际拿到的 GOOS / GOARCH。
    pub fn target(&self) -> (&str, &str) {
        (&self.goos, &self.goarch)
    }

    /// 工作目录（runner 用它把诊断里的全路径换回短名，R6 细节 3）
    pub fn work_dir(&self) -> &Path {
        &self.cfg.work_dir
    }

    /// 生效的超时与资源上限（只来自 [`Limits`]，oracle 不写死）
    pub fn limits(&self) -> Limits {
        self.cfg.limits
    }

    /// 某层实际使用的**全部**命令（按执行顺序，不含 `go` 本身）。
    ///
    /// 单独暴露是为了让测试能直接断言命令形态 —— T30 复核时踩过的坑
    /// （「`go build` 还是 `go tool compile`」）从运行结果里是看不出来的。
    pub fn steps(&self, mode: Mode, file: &str) -> Vec<Step> {
        let go = |args: Vec<String>| Step {
            program: self.cfg.go_tool.clone(),
            args,
        };
        let importcfg = format!("-importcfg={}", self.importcfg.display());
        match mode {
            // :188 compileFile：go tool compile -e -p=p -importcfg=… <flags> <file>
            Mode::Compile => vec![go(vec![
                "tool".into(),
                "compile".into(),
                "-e".into(),
                "-p=p".into(),
                importcfg,
                file.into(),
            ])],
            // :787-790 errorcheck：多 -d=panic 与 -C，R5 自动加 ssa/check/on
            Mode::ErrorCheck => vec![go(vec![
                "tool".into(),
                "compile".into(),
                "-p=p".into(),
                "-d=panic".into(),
                "-C".into(),
                "-e".into(),
                importcfg,
                "-o".into(),
                "a.o".into(),
                "-d=ssa/check/on".into(),
                file.into(),
            ])],
            // fast path 三步：compile -> link -> 直跑 exe（`:1069-1077` + `:249`）
            Mode::Run => vec![
                go(vec![
                    "tool".into(),
                    "compile".into(),
                    "-p=main".into(),
                    importcfg,
                    "-o".into(),
                    self.pkg_a_path().display().to_string(),
                    file.into(),
                ]),
                // :249 linkFile：go tool link -s -w -buildid=test -o exe -importcfg=… pkg.a
                go(vec![
                    "tool".into(),
                    "link".into(),
                    "-s".into(),
                    "-w".into(),
                    "-buildid=test".into(),
                    "-o".into(),
                    self.exe_path().display().to_string(),
                    format!("-importcfg={}", self.importcfg.display()),
                    self.pkg_a_path().display().to_string(),
                ]),
                // 最后一步直跑 exe —— program 是 exe 本身，**不经过 go**
                Step {
                    program: self.exe_path(),
                    args: vec![],
                },
            ],
            other => vec![go(vec!["run".into(), format!("<unsupported: {other:?}>")])],
        }
    }

    /// 第 1 步的参数（**不含程序本身**）—— 便于日志与测试断言命令形态。
    pub fn argv_for(&self, mode: Mode, file: &str) -> Vec<String> {
        self.steps(mode, file)
            .into_iter()
            .next()
            .map(|s| s.args)
            .unwrap_or_default()
    }

    /// fast path 的中间产物 `pkg.a`（官方 `:1069` 放在 tempDir，不是 test 目录）
    fn pkg_a_path(&self) -> PathBuf {
        self.temp_dir.join("m0-tmp.pkg.a")
    }

    /// fast path 产出的可执行文件
    fn exe_path(&self) -> PathBuf {
        self.temp_dir.join("m0-tmp.test.exe")
    }

    /// 按模式跑一个用例。
    ///
    /// 聚合规则（**别把中间步骤的失败吞掉**）：
    /// - 任一步超时 / 超 RSS ⇒ 立刻早退，带着那一步的结果；
    /// - **非最后**一步失败（fast path 的 compile 或 link）⇒ 早退，后续步骤没意义；
    /// - 否则以**最后一步**的退出码为准。
    pub fn run_mode(&self, mode: Mode, file: &str) -> Result<OracleOutput, OracleError> {
        self.run_mode_path(mode, &self.cfg.work_dir.join(file))
    }

    /// 按模式跑一个用例，`file` 是**文件路径**。
    ///
    /// 真实语料在**只读**的 `GOROOT/test` 里，自测用例在临时目录里 —— 两种位置都指向
    /// 磁盘上真实存在的文件，所以这里只接收路径、**从不写工作目录**。
    /// 传绝对路径还有个好处：诊断里会带全路径，正好把 R6 细节 3
    /// （`replacePrefix` 要把路径换回短名，含续行）走到。
    pub fn run_mode_path(&self, mode: Mode, file: &Path) -> Result<OracleOutput, OracleError> {
        let token = file.to_string_lossy().into_owned();
        let steps = self.steps(mode, &token);
        let start = Instant::now();
        let mut merged = String::new();
        let mut peak = 0u64;
        let mut last: Option<OracleOutput> = None;

        for (i, step) in steps.iter().enumerate() {
            let out = self.run_one(step, start)?;
            merged.push_str(&out.merged);
            peak = peak.max(out.peak_rss_bytes);
            let is_last = i + 1 == steps.len();
            let stop =
                out.timed_out || out.resource_exceeded || (!is_last && out.exit_code != Some(0));
            last = Some(out);
            if stop {
                break;
            }
        }

        let out = last.ok_or_else(|| OracleError::Spawn("没有任何命令被执行".into()))?;
        Ok(OracleOutput {
            exit_code: out.exit_code,
            merged,
            timed_out: out.timed_out,
            resource_exceeded: out.resource_exceeded,
            peak_rss_bytes: peak,
            duration: start.elapsed(),
            pid: out.pid,
        })
    }

    /// 跑单条命令，带超时与 RSS 观测。
    fn run_one(&self, step: &Step, start: Instant) -> Result<OracleOutput, OracleError> {
        let mut cmd = Command::new(&step.program);
        cmd.args(&step.args)
            .current_dir(&self.cfg.work_dir)
            // 官方 runcmd 的固定环境（`:647` / `:656`）：清掉 GOENV 与 GOFLAGS，
            // 并让 PWD 与 cwd 一致（省掉子进程里 os.Getwd 的一次 syscall）
            .env("GOENV", "off")
            .env("GOFLAGS", "")
            .env("PWD", &self.cfg.work_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| OracleError::Spawn(e.to_string()))?;
        let pid = child.id();
        let mut pipe = child.stdout.take().expect("stdout 已配置为 piped");
        let mut err_pipe = child.stderr.take().expect("stderr 已配置为 piped");

        let deadline = start + self.cfg.limits.per_case;
        let mut timed_out = false;
        // spawn 之后**立刻**采一次：轮询有个固有限制 —— 短命进程可能在第一次采样前
        // 就退出了，/proc/<pid> 随之消失，峰值就采不到（表现为 peak=0，判不出超限）。
        // 真实负载（编译、链接、跑用例）都在百毫秒以上，2 ms 的间隔足够；
        // 真要精确到短命进程，得用 wait4 的 rusage（需要 unsafe）。
        let mut peak = read_peak_rss(pid).unwrap_or(0);
        let status = loop {
            match child.try_wait() {
                Ok(Some(st)) => break st,
                Ok(None) => {}
                Err(e) => return Err(OracleError::Io(e.to_string())),
            }
            if let Some(v) = read_peak_rss(pid) {
                peak = peak.max(v);
            }
            // ⚠️ 这里必须拿 **now** 去比 deadline。
            // 写成 `start >= deadline`（`deadline = start + per_case`）会**恒为 false** ——
            // `start` 是个不再变化的快照，永远小于 `start + per_case`。本轮真踩过：
            // 30 秒的 sleep 跑满全程都没触发超时。
            if Instant::now() >= deadline {
                timed_out = true;
                // fast path 下父子关系是直的，kill 父进程就等于杀掉被测程序本身。
                // （若将来改回 `go run`，这里就需要杀进程组 —— 那需要 unsafe，
                //   与 workspace 的 `unsafe_code = "forbid"` 冲突。）
                let _ = child.kill();
                match child.wait() {
                    Ok(st) => break st,
                    Err(e) => return Err(OracleError::Io(e.to_string())),
                }
            }
            std::thread::sleep(self.cfg.rss_poll_interval);
        };

        let mut so = Vec::new();
        let mut se = Vec::new();
        let _ = pipe.read_to_end(&mut so);
        let _ = err_pipe.read_to_end(&mut se);
        // 最后一采：子进程刚退出、/proc 还在的窗口很短，但尽力取到
        if let Some(v) = read_peak_rss(pid) {
            peak = peak.max(v);
        }

        // R2b：两个流**合并**（官方写进同一个 buffer）
        let merged = String::from_utf8_lossy(&so).into_owned() + &String::from_utf8_lossy(&se);
        let resource_exceeded =
            self.cfg.limits.max_rss_bytes > 0 && peak > self.cfg.limits.max_rss_bytes;
        Ok(OracleOutput {
            exit_code: status.code(),
            merged,
            timed_out,
            resource_exceeded,
            peak_rss_bytes: peak,
            duration: start.elapsed(),
            pid: Some(pid),
        })
    }
}

/// 读 `/proc/<pid>/status` 的 `VmHWM`（峰值 RSS，**字节**）。
///
/// 为什么用 `VmHWM` 而不是 `VmRSS`：前者是内核维护的**峰值**，不会因为采样时机而低估；
/// 后者只是当前值，可能刚好错过峰值。
fn read_peak_rss(pid: u32) -> Option<u64> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    for line in s.lines() {
        if let Some(rest) = line.strip_prefix("VmHWM:") {
            let kb: u64 = rest.split_whitespace().next()?.parse().ok()?;
            return Some(kb * 1024);
        }
    }
    None
}

/// 跑 `go version` 并解析出 `(版本, GOOS, GOARCH)`。
fn probe_version(go: &Path, cwd: &Path) -> Result<(String, String, String), OracleError> {
    let out = Command::new(go)
        .arg("version")
        .current_dir(cwd)
        .env("GOENV", "off")
        .env("GOFLAGS", "")
        .output()
        .map_err(|e| OracleError::Spawn(format!("{} version: {e}", go.display())))?;
    // 形如 `go version go1.27.1 linux/arm64`
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    parse_go_version(&text).ok_or(OracleError::VersionMismatch {
        expected: "<可解析的 go version 输出>".into(),
        got: text,
    })
}

/// 解析 `go version` 的输出。
///
/// ⚠️ **格式陷阱**（本轮实测踩到）：输出是
/// ```text
/// go version go1.27.1 linux/arm64
/// ```
/// 平台部分 `linux/arm64` 是**斜杠连写的一个 token**，不是空格分隔的两个 ——
/// 按 `split_whitespace` 取第 2、3 段会拿到 `"linux/arm64"` 和 `None`。
fn parse_go_version(text: &str) -> Option<(String, String, String)> {
    let rest = text.strip_prefix("go version ")?;
    let mut it = rest.split_whitespace();
    let version = it.next()?.to_string();
    let (goos, goarch) = it.next()?.split_once('/')?;
    Some((version, goos.to_string(), goarch.to_string()))
}

/// 生成 stdlib 的 importcfg（官方 `:218-220`）。
///
/// ⚠️ **这一步可能很慢**：实测高并发（10 个 oracle 同时建）时能到 45 s ——
/// 因为 `go list -export std` 要确保 std 的导出缓存在 build cache 里。
/// 官方用 `sync.OnceValue` 缓存（`:218`）只做一次；M0 的 harness 全程只建一个 oracle，
/// 所以暂不缓存（每实例一份也更不容易出状态问题）。
fn build_importcfg(cfg: &OracleConfig, temp_dir: &Path) -> Result<PathBuf, OracleError> {
    let out = Command::new(&cfg.go_tool)
        .args([
            "list",
            "-export",
            "-f",
            "{{if .Export}}packagefile {{.ImportPath}}={{.Export}}{{end}}",
            "std",
        ])
        .current_dir(&cfg.work_dir)
        .env("GOENV", "off")
        .env("GOFLAGS", "")
        .output()
        .map_err(|e| OracleError::Spawn(format!("go list -export std: {e}")))?;
    if !out.status.success() {
        return Err(OracleError::ImportCfg(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ));
    }
    let body = String::from_utf8_lossy(&out.stdout).into_owned();
    if !body.contains("packagefile ") {
        return Err(OracleError::ImportCfg(
            "go list -export std 没有产出任何 packagefile —— stdlib 导出缓存缺失？".into(),
        ));
    }
    let path = temp_dir.join("importcfg.cfg");
    std::fs::write(&path, body).map_err(|e| OracleError::Io(e.to_string()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 不依赖真实 go：只断言命令形态（R6）
    #[test]
    fn 命令形态照_r6() {
        let o = Oracle {
            cfg: OracleConfig::from(CorpusConfig::m0()),
            version: "go1.27.1".into(),
            goos: "linux".into(),
            goarch: "arm64".into(),
            importcfg: PathBuf::from("/tmp/ic.cfg"),
            temp_dir: PathBuf::from("/tmp"),
        };
        let c = o.steps(Mode::Compile, "x.go");
        assert_eq!(c.len(), 1, "compile 层只要一步");
        assert_eq!(c[0].args[0], "tool");
        assert_eq!(c[0].args[1], "compile");

        let e = o.steps(Mode::ErrorCheck, "x.go");
        assert!(
            e[0].args.iter().any(|a| a == "-C"),
            "errorcheck 必须带 -C（关掉首行列号）：{:?}",
            e[0].args
        );
        assert!(
            e[0].args.iter().any(|a| a == "-d=ssa/check/on"),
            "R5：必须带 -d=ssa/check/on：{:?}",
            e[0].args
        );
        assert!(
            e[0].args.iter().any(|a| a.starts_with("-importcfg=")),
            "必须带 stdlib 的 importcfg：{:?}",
            e[0].args
        );

        // run 层是 fast path 三步，最后一步**直跑 exe**（不是 go run）
        let r = o.steps(Mode::Run, "x.go");
        assert_eq!(r.len(), 3, "run 层是 compile -> link -> 直跑 exe：{r:?}");
        assert_eq!(r[0].args[0], "tool", "第 1 步是 go tool compile");
        assert_eq!(r[1].args[1], "link", "第 2 步是 go tool link");
        assert!(
            r[1].args.iter().any(|a| a == "-buildid=test"),
            "linkFile 带 -buildid=test：{:?}",
            r[1].args
        );
        assert!(
            r[2].args.is_empty(),
            "第 3 步没有参数（只有 program）：{:?}",
            r[2]
        );
        assert!(
            r[2].program.to_string_lossy().ends_with(".exe"),
            "第 3 步的 program 应当是 exe 本身：{:?}",
            r[2]
        );
    }

    /// 真实输出是 `go version go1.27.1 linux/arm64` —— 平台是**斜杠连写的一个 token**。
    #[test]
    fn 解析_go_version_输出() {
        assert_eq!(
            parse_go_version("go version go1.27.1 linux/arm64"),
            Some(("go1.27.1".into(), "linux".into(), "arm64".into()))
        );
        assert_eq!(
            parse_go_version("go version go1.24.5 darwin/arm64"),
            Some(("go1.24.5".into(), "darwin".into(), "arm64".into()))
        );
        // 平台写成两个空格分隔的 token（**不是** go 的真实格式）应当解析失败 ——
        // 宁可失败也不要悄悄给出错的 GOOS/GOARCH
        assert_eq!(parse_go_version("go version go1.27.1 linux arm64"), None);
        assert_eq!(parse_go_version("not a version line"), None);
        assert_eq!(parse_go_version(""), None);
    }
}
