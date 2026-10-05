//! xtask 的第 3 职责：**环境 manifest 生成**。
//!
//! 目标是把 `M0-manifest.json` 的 `environment` 节从**手填**改为**可重放生成**
//! （T37 明确的三职责之一）。
//!
//! # 为什么「可重放」比「准确」更重要
//!
//! 手填的环境值有个隐蔽问题：**半年后没人说得清某个 digest 是哪次测的**。
//! 重放生成的价值在于「现在跑一次拿到的就是当时的」—— 所以每个值都要么
//! **当场实测**，要么**明确标注为手填常量**并说明来源。**不许留空、不许写
//! `unknown` / `<TBD>`** —— 那会让 T16 / E2 门禁红。
//!
//! # 哪些能实测、哪些是常量
//!
//! | 字段 | 来源 |
//! |---|---|
//! | `container.*`（kernel / glibc / nproc / mem） | **实测**（`uname` / `ldd` / `/proc`）|
//! | `toolchain.go` / `toolchain.rust` / `toolchain.clang` | **实测**（各自 `--version`）|
//! | `toolchain.go.sha256` | **实测**（对 go 二进制做 sha256）|
//! | `image.digest` / `image.base_*_digest` | **实测**（读 `docker/image.lock`）|
//! | `host.*` | **手填常量** —— 宿主信息，容器内看不见（`os` / `arch` 除外）|
//! | `volumes.*` / `target.*` | **手填常量** —— 来自 D-M0-9 / D-M0-1 的决策 |
//! | `debug.*`（VSCode / CodeLLDB） | **手填常量** —— 来自 T23–T28 的实测记录 |
//!
//! 手填的部分集中在 [`consts`]，每条都注明出处，便于日后核对。

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

/// 仓库根目录（`CARGO_MANIFEST_DIR` = `<repo>/rgoc/xtask`）。
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .to_path_buf()
}

/// `M0-manifest.json` 的路径（`xtask manifest` 的写入目标）。
pub fn repo_manifest_path() -> PathBuf {
    repo_root().join("docs/milestones/M0-manifest.json")
}

/// **手填常量**：容器内测不到或属于决策值的字段。
///
/// 每条注明出处 —— 这是「手填」与「忘了测」的唯一区别。
mod consts {
    use serde_json::{Value, json};

    /// `host.*`：宿主信息。容器内能看见 `arch`，但 macOS 版本 / Docker Desktop
    /// 版本只能在宿主上测（2026-10-02 实测值，见 `M0-benchmarks.md` §1）。
    pub fn host() -> Value {
        json!({
            "os": "macOS 27.2",
            "arch": "arm64",
            "docker_desktop_version": "29.6.2",
            "source": "宿主实测（2026-10-02，M0-benchmarks.md section 1）；容器内不可见，故为常量"
        })
    }

    /// `volumes.*`：三个命名卷（D-M0-9 的 bind mount vs 命名卷实测结论）。
    pub fn volumes() -> Value {
        json!({
            "cargo_registry": "rgoc-cargo-registry -> /home/dev/.cargo/registry",
            "cargo_git": "rgoc-cargo-git -> /home/dev/.cargo/git",
            "target": "rgoc-target -> /work/rgoc/target",
            "source": "D-M0-9 实测结论（bind mount 慢 2.3 倍）；卷名属决策值"
        })
    }

    /// `target.*`：首发平台（D-M0-1 硬约束）。
    pub fn target() -> Value {
        json!({
            "triple": "aarch64-unknown-linux-gnu",
            "object_format": "ELF",
            "source": "D-M0-1：首发平台 Linux/arm64，镜像 golang:1.27.1-bookworm"
        })
    }

    /// `debug.*`：VSCode / CodeLLDB 调试链路（E5 的三个实测前提）。
    pub fn debug() -> Value {
        json!({
            "editor": "VSCode Dev Containers",
            "editor_version": "1.140.0 (host)",
            "editor_commit": "07f806f999227108933c2e30515b26eecc1fda74",
            "codelldb": {
                // ⚠️ **不许在字段值里写 `<...>`**：T16 的空缺判据是「含 `<` 即空缺」。
                // 路径用文字描述（`platform.ok` 标记文件），别用尖括号占位。
                "platform_package": "已装（判据：扩展目录下的 platform.ok 标记文件存在）",
                "lldb_version": "22.1.8-codelldb",
                "installer": "scripts/install-codelldb.sh（curl --noproxy '*'，绕开宿主下发的死代理）"
            },
            "source": "T23–T28 实测（E5 门禁证据，见 manifest gate.E5）"
        })
    }
}

/// 读 `docker/image.lock` 里的镜像锁定值。
///
/// 为什么不写死：镜像**不是位级可复现**的（14 层里 6 层 digest 会变），
/// 钉子必须是 `base.index_digest` + `src.*_sha256` —— 详见 `M0-benchmarks.md` §5。
fn image_section() -> Value {
    let lock = repo_root().join("docker/image.lock");
    let mut out = json!({
        "tag": "rgoc:dev",
        "digest_stability": "per-build (not reproducible; the pin is base_index_digest + src.*_sha256, see M0-benchmarks.md section 5)",
        "base_tag": "golang:1.27.1-bookworm",
    });
    if let Ok(text) = std::fs::read_to_string(&lock) {
        // image.lock 是 `前缀.键<空格>= 值` 的扁平结构（见 [`lock_value`] 的形态说明）。
        // ⚠️ 只取**已知键**，不整份塞进去 —— image.lock 里还有构建时间戳之类的
        // 易变字段，全量搬运会让「environment 变了」这件事无法归因。
        //
        // ⚠️ **取不到时不静默跳过**：`base.index_digest` 是 E1 门禁的真正钉子，
        // 它读不到就说明 image.lock 格式变了（或文件被换过），必须显式暴露。
        let mut missing: Vec<&str> = Vec::new();
        for (k, target_key) in [
            ("local.image_id", "digest"),
            ("base.index_digest", "base_index_digest"),
            ("base.arm64_digest", "base_arm64_digest"),
            ("local.image_size", "size"),
            ("local.layer_count", "layer_count"),
            ("base.tag", "base_tag"),
        ] {
            match lock_value(&text, k) {
                Some(v) => {
                    // ⚠️ `local.layer_count` 的真实值是 `14（其中容器层 7 个：…）`
                    // —— **带中文括号说明**，不是纯数字。直接 `parse::<u64>()`
                    // 会失败 → 回退成字符串 → JSON 里 `layer_count` 从数字变字符串，
                    // 门禁按类型取值时静默拿到 null。必须取**开头连续数字**。
                    let val = match target_key {
                        "layer_count" => leading_u64(&v).map_or_else(|| json!(v), |n| json!(n)),
                        _ => json!(v),
                    };
                    out[target_key] = val;
                }
                None => missing.push(k),
            }
        }
        out["lock_file"] = json!("docker/image.lock");
        // 取不到就在 JSON 里留一条**可读的说明**而不是悄悄少个字段 ——
        // T16 会把空串判为空缺，这里用非空文本承���「为什么没取到」。
        if !missing.is_empty() {
            out["unresolved_keys"] = json!(missing.join(", "));
        }
    } else {
        out["lock_file"] = Value::Null;
    }
    out
}

/// 取字符串**开头**的连续数字。
///
/// 用途：`docker/image.lock` 的 `local.layer_count` 值是
/// `14（其中容器层 7 个：架构断言 / …）` —— 数字后面跟着中文说明。
/// 只认开头连续数字，`14（…）` ⇒ `Some(14)`，而 `共 14 层` ⇒ `None`
/// （保守：解析不出来就保留原字符串，让人看见，而不是猜一个数）。
fn leading_u64(s: &str) -> Option<u64> {
    let digits: String = s.trim().chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

/// 从 `key = value` 风格的文本里取一个值。
///
/// ⚠️ **`docker/image.lock` 的真实形态是 `<前缀>.<键><若干空格>= <值>`**：
/// ```text
/// base.index_digest       = sha256:69a7…
/// local.image_id          = sha256:21f5…
/// ```
/// 早先按 `key: value`（冒号）解析，结果**一个字段都没取到** —— 而
/// `image_section` 遇到取不到就跳过，生成出的 `image` 节少了 digest 却**不报错**。
/// 两层静默叠在一起：解析失败 → 字段缺失 → 没人看见。
/// 现在改成按 `=` 解析，并**只认行首**（防止 `base.index_digest` 被 `index_digest`
/// 之外的键误命中）。
fn lock_value(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        // 注释行与空行直接跳过（`#` 开头）
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // 取第一个 `=` 左边当键。**不能**用 `contains(key)`：那会让
        // `base.index_digest` 同时被 `index_digest` 与更短的键命中。
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        if k.trim() != key {
            continue;
        }
        let v = v.trim().trim_matches('"').trim();
        if !v.is_empty() {
            return Some(v.to_string());
        }
    }
    None
}

/// 实测容器节。
fn container_section() -> Value {
    // ⚠️ 一律用**绝对路径**跑外部命令：`cargo test` 的子进程 PATH 与交互 shell
    // 不同（实测裸 `ldd` 在测试里找不到），靠 PATH 解析命令名会静默变空串。
    //
    // ⚠️ **`uname` 必须带 `-r`**：裸 `uname` 只输出 `Linux`（不含版本），
    // 实测踩过 —— 于是 `kernel` 写进去的是 `"Linux"` 而不是 `6.12.76-linuxkit`。
    // 它**非空**，所以 T16 的空缺检查抓不到，只有逐字段比对 manifest 才发现。
    let uname = cmd_stdout_of("/usr/bin/uname", &["-r"]).unwrap_or_default();
    let kernel = uname.trim().to_string();
    let os_release = std::fs::read_to_string("/etc/os-release").ok().map(|t| {
        t.lines()
            .find_map(|l| l.strip_prefix("PRETTY_NAME="))
            .map(|v| v.trim_matches('"').to_string())
            .unwrap_or_default()
    });
    json!({
        "arch": std::env::consts::ARCH,
        "kernel": kernel,
        "os_release": os_release,
        "glibc": glibc_version(),
        "nproc": num_or_zero("/proc/cpuinfo", "processor"),
        "mem_total_kb": mem_total_kb(),
    })
}

/// glibc 版本。
///
/// ⚠️ **用 `ldd --version` 而不是从 `libc.so.6` 的字节里抠字符串** ——
/// 后者依赖「ELF 的 `.rodata` 里恰好有 `release version 2.36` 这行文本」，
/// 实测在 bookworm 上抠不到（glibc 把版本串换成了别的形态）。
/// `ldd --version` 是 glibc 自己的官方出口。
///
/// ⚠️ **用绝对路径 `/usr/bin/ldd`**：`cargo test` 里的子进程拿到的 PATH 与
/// 交互 shell 不同（实测 `which ldd` 在 shell 里有、在测试里 `Command::new("ldd")`
/// 找不到），靠 PATH 解析裸命令名会静默返回空串 —— 那是「退出码非 0 ⇒ None」
/// 分支把它变成空值的。绝对路径把这条脆弱依赖去掉。
fn glibc_version() -> String {
    cmd_stdout_of("/usr/bin/ldd", &["--version"])
        .as_deref()
        .and_then(parse_glibc)
        .unwrap_or_default()
}

/// 从 `ldd --version` 的输出里取版本号。
///
/// 形态：`ldd (Debian GLIBC 2.36-9+deb12u14) 2.36`
///
/// ⚠️ **必须取行尾那个裸版本号（`2.36`），不能取括号里的 `2.36-9+deb12u14`**：
/// 后者是发行版修订号，写进 manifest 会让「glibc 2.36」这条环境事实看起来
/// 随补丁版本漂移，而门禁要比对的是 ABI 主次版本。
/// 实现上：按空白切词，**跳过所有带 `(` 或 `)` 的词**（那是括号里的包版本），
/// 剩下第一个形如 `2.N` 的词就是答案。
fn parse_glibc(s: &str) -> Option<String> {
    let first = s.lines().next()?;
    first
        .split_whitespace()
        .filter(|t| !t.contains('(') && !t.contains(')'))
        .find(|t| t.starts_with("2.") && t[2..].chars().all(|c| c.is_ascii_digit() || c == '.'))
        .map(str::to_string)
}

/// 数 `/proc/cpuinfo` 里的 `processor` 行。
fn num_or_zero(path: &str, key: &str) -> u64 {
    std::fs::read_to_string(path)
        .map(|t| t.lines().filter(|l| l.starts_with(key)).count() as u64)
        .unwrap_or(0)
}

/// 从 `/proc/meminfo` 读 `MemTotal`（单位 kB）。
fn mem_total_kb() -> u64 {
    std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|t| {
            t.lines()
                .find(|l| l.starts_with("MemTotal:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse::<u64>().ok())
        })
        .unwrap_or(0)
}

/// 跑 `<program> <args...>` 并取 stdout。
fn cmd_stdout_of(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// 跑 `<program> --version`（或 `version`）并取**首行**。
///
/// ⚠️ **必须显式带版本参数**：`rustc` / `cargo` 不带参数时会把**完整 help 文本**
/// 打到 stdout 且**退出码为 0** —— 一旦漏了这个参数，environment 里就会塞进几百行
/// 帮助文字，而「退出码为 0」让这类错误极难发现（本轮就踩了：三个字段全成了 help）。
/// 只取首行是第二道防线。
fn cmd_version(program: &str, args: &[&str]) -> String {
    cmd_stdout_of(program, args)
        .map(|s| s.lines().next().unwrap_or("").trim().to_string())
        .unwrap_or_default()
}

/// 实测工具链节。
fn toolchain_section() -> Value {
    let go_path = "/usr/local/go/bin/go";
    let go_sha = std::process::Command::new("/usr/bin/sha256sum")
        .arg(go_path)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_string()
        })
        .unwrap_or_default();
    let ld = if std::path::Path::new("/usr/bin/ld").exists() {
        "/usr/bin/ld".to_string()
    } else {
        String::new()
    };
    json!({
        "go": {
            "version": cmd_version(go_path, &["version"]),
            "path": go_path,
            "sha256": go_sha,
        },
        "rust": {
            "rustc": cmd_version("rustc", &["--version"]),
            "cargo": cmd_version("cargo", &["--version"]),
            "pinned_by": "rust-toolchain.toml",
        },
        "clang": {
            "version": cmd_version("clang", &["--version"]),
            "linker": ld,
        },
    })
}

/// 生成 `environment` 节。
///
/// **能实测的当场测，测不到的取 [`consts`] 的手填常量** —— 任何一项都不得为空，
/// 否则 T16 / E2 门禁会红（见模块头「为什么『可重放』比『准确』更重要」）。
pub fn environment_json() -> Value {
    json!({
        "host": consts::host(),
        "image": image_section(),
        "container": container_section(),
        "toolchain": toolchain_section(),
        "volumes": consts::volumes(),
        "target": consts::target(),
        "debug": consts::debug(),
    })
}

/// 把生成的 `environment` 写进 `M0-manifest.json`。
///
/// **只替换 `environment` 一节**，其余（`gate` / `benchmarks` / `decisions` …）原样保留
/// —— 那些是人工登记的门禁证据，xtask 无权覆盖。
pub fn update_manifest(manifest: &Path) -> Result<(), String> {
    let raw = std::fs::read_to_string(manifest)
        .map_err(|e| format!("读 {} 失败：{e}", manifest.display()))?;
    let mut v: Value = serde_json::from_str(&raw)
        .map_err(|e| format!("{} 不是合法 JSON：{e}", manifest.display()))?;
    if !v.is_object() {
        return Err(format!("{} 的顶层不是对象", manifest.display()));
    }
    v["environment"] = environment_json();
    let text = serde_json::to_string_pretty(&v).map_err(|e| format!("JSON 序列化失败：{e}"))?;
    std::fs::write(manifest, format!("{text}\n"))
        .map_err(|e| format!("写 {} 失败：{e}", manifest.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ★ 本轮真踩过的坑：`rustc` / `cargo` 不带参数会把**完整 help 文本**打到 stdout
    /// 且**退出码为 0**。所以「跑一下看退出码」抓不到它，必须在测试里钉住版本串形态。
    #[test]
    fn 版本串只取首行且不含_help_文本() {
        assert_eq!(
            cmd_version("rustc", &["--version"]),
            "rustc 1.98.1 (48a229cea 2026-09-01)"
        );
        assert!(!cmd_version("rustc", &["--version"]).contains("Usage:"));
        // 真实形态（含日期与 hash），用 starts_with 更稳
        assert!(cmd_version("cargo", &["--version"]).starts_with("cargo 1."));
        assert!(!cmd_version("cargo", &["--version"]).contains("Usage:"));
    }

    /// glibc 版本必须取**行尾裸版本**，不能取括号里的发行版修订号。
    #[test]
    fn glibc_取行尾裸版本而非括号里的修订号() {
        // bookworm 实测形态
        assert_eq!(
            parse_glibc("ldd (Debian GLIBC 2.36-9+deb12u14) 2.36"),
            Some("2.36".to_string()),
            "括号里的 2.36-9+deb12u14 是发行版修订号，不是 ABI 版本"
        );
        // 上游 glibc（无发行版后缀）
        assert_eq!(parse_glibc("ldd (GNU libc) 2.39"), Some("2.39".to_string()));
        // 只有一行、后面没版本号的异常形态 ⇒ None（宁可空着让人看见，也不填错）
        assert_eq!(parse_glibc("ldd (GNU libc)"), None);
        assert_eq!(parse_glibc(""), None);
    }

    /// `lock_value` 必须按 `docker/image.lock` 的**真实形态**解析。
    ///
    /// ⚠️ 两个坑叠在一起：
    /// ① 分隔符是 `=`（带空格）**不是** `:` —— 早先按冒号解析，一个字段都取不到；
    /// ② 键**带 `base.` / `local.` 前缀** —— 用 `contains(key)` 会让
    /// `base.index_digest` 被更短的键误命中。
    /// 而这两层失败都是**静默**的：取不到就跳过 ⇒ JSON 少字段却仍然合法。
    #[test]
    fn lock_value_按真实格式解析_只认完整键() {
        // ↓ 这是 docker/image.lock 里真实的行形态（含注释行与对齐空格）
        let t = "\
# rgoc 镜像锁定信息（门禁 E1）
base.tag                = golang:1.27.1-bookworm
base.index_digest       = sha256:69a7b978
base.arm64_digest       = sha256:1668bbf8
local.image_id          = sha256:21f55802
local.layer_count       = 14
";
        assert_eq!(
            lock_value(t, "base.index_digest").as_deref(),
            Some("sha256:69a7b978")
        );
        assert_eq!(
            lock_value(t, "base.arm64_digest").as_deref(),
            Some("sha256:1668bbf8")
        );
        assert_eq!(
            lock_value(t, "local.image_id").as_deref(),
            Some("sha256:21f55802")
        );
        assert_eq!(lock_value(t, "local.layer_count").as_deref(), Some("14"));
        // 注释行里出现的键不得被误命中
        assert_eq!(lock_value(t, "digest"), None, "注释行不是赋值");
        // 缺键返回 None（不是空串 —— 空串会被 T16 判成空缺）
        assert_eq!(lock_value(t, "local.size"), None);
    }

    /// `local.layer_count` 的真实值带中文说明，数字必须从**开头**取。
    #[test]
    fn leading_取开头连续数字() {
        assert_eq!(leading_u64("14（其中容器层 7 个：架构断言）"), Some(14));
        assert_eq!(leading_u64("14"), Some(14));
        assert_eq!(leading_u64("  7 层"), Some(7));
        // 开头不是数字 ⇒ None（不猜）
        assert_eq!(leading_u64("共 14 层"), None);
        assert_eq!(leading_u64(""), None);
        assert_eq!(leading_u64("abc"), None);
    }

    /// `image` 节必须真的取到 digest —— E1 的钉子取不到就是**失败**，不是「少个字段」。
    #[test]
    fn image_节取到_digest() {
        let img = image_section();
        for k in ["digest", "base_index_digest", "base_arm64_digest"] {
            let v = img[k].as_str().unwrap_or("");
            assert!(
                v.starts_with("sha256:"),
                "image.{k} 应是 sha256 串，实际 {v:?}（image.lock 解析是不是坏了？）"
            );
        }
        assert_eq!(img["layer_count"], 14);
        assert_eq!(img["base_tag"], "golang:1.27.1-bookworm");
        assert!(
            img.get("unresolved_keys").is_none(),
            "所有已知键都该取到，实际未解析：{:?}",
            img.get("unresolved_keys")
        );
    }

    /// `container.kernel` 必须是**版本号**，不是裸 `uname` 的 `Linux`。
    #[test]
    fn kernel_是版本号而非_uname_的字面输出() {
        let c = container_section();
        let k = c["kernel"].as_str().unwrap_or("");
        // 裸 `uname` 输出 `Linux`（非空，T16 抓不到），带 `-r` 才有版本号
        assert_ne!(k, "Linux", "`uname` 漏了 -r：kernel 会写成 'Linux'");
        assert!(
            k.chars().next().is_some_and(|c| c.is_ascii_digit()),
            "kernel 应是版本号（形如 6.12.76-linuxkit），实际 {k:?}"
        );
    }

    /// 生成值里不许出现 `unknown` / `<...>` 之类 —— 那是 T16 会抓的空缺。
    /// 这里只查**手填常量**那几节（实测节由上面的集成测试在真容器里查）。
    #[test]
    fn 手填常量_无占位符() {
        // ⚠️ 判据必须与 T16 一致：**整值等于**占位词，不做子串匹配。
        // `target.triple` 的合法字面量 `aarch64-unknown-linux-gnu` 本身就含
        // "unknown" —— 子串匹配会把这个**正确**的值判成空缺（T16 的注释里
        // 专门记了这个坑：子串匹配是 M0-plan v1 的缺陷）。
        const PLACEHOLDERS: [&str; 6] = ["unknown", "n/a", "na", "todo", "tbd", "fixme"];

        fn walk(o: &serde_json::Value, p: &str, out: &mut Vec<String>, placeholders: &[&str]) {
            match o {
                serde_json::Value::Object(m) => {
                    for (k, v) in m {
                        walk(v, &format!("{p}.{k}"), out, placeholders);
                    }
                }
                serde_json::Value::String(s) => {
                    let bad = s.trim().is_empty()
                        || s.contains('<')
                        || placeholders.contains(&s.trim().to_lowercase().as_str());
                    if bad {
                        out.push(format!("{p} = {s}"));
                    }
                }
                _ => {}
            }
        }

        for (name, v) in [
            ("host", consts::host()),
            ("volumes", consts::volumes()),
            ("target", consts::target()),
            ("debug", consts::debug()),
        ] {
            let mut bad = Vec::new();
            walk(&v, name, &mut bad, &PLACEHOLDERS);
            assert!(bad.is_empty(), "{name} 有空缺/占位符字段：{bad:?}");
        }
        // target 必须是首发平台（D-M0-1）—— 且它的合法字面量含 "unknown"，
        // 这正说明判据不能用子串匹配
        assert_eq!(consts::target()["triple"], "aarch64-unknown-linux-gnu");
    }
}
