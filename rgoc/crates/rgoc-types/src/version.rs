//! Go **语言版本门控**（M3/Task 21）。
//!
//! # 判据来源
//!
//! `go/types/version.go`（`F3`）与 `go/types/check.go:312-348`（`F13`）。
//! 三条规则，逐条对齐：
//!
//! 1. **`-lang` 缺省/非法 ⇒ 不门控**（官方 `allowVersion`：
//!    `!check.version.isValid() || check.version.cmp(want) >= 0`）。
//! 2. **文件级 `//go:build go1.N` 抬高版本**：生效版本 =
//!    `max(fileVersion, go1.21)`，**默认值**是 `-lang` 的原样值。
//!    为什么是 `max(..., go1.21)`：go1.21 是「文件级版本」这个特性**本身**的引入版本
//!    （官方注释在 `check.go:332-341`），低于它的文件级版本没有意义。
//! 3. **两处 `TooNew`**（都报「应用比目标旧」）：
//!    * 包级 `-lang` 比当前工具链新 ⇒ 位置取 **`files[0]`**；
//!    * 文件级 `//go:build` 比当前工具链新 ⇒ 位置取**该文件的 `package` 名字**。
//!
//! 「当前工具链」= [`GO_CURRENT`]（语料快照 go1.27.1 ⇒ `go1.27`，
//! 官方那边是 `goversion.Version`）。**它必须钉死**，否则门禁会随宿主 Go 版本漂。
//!
//! # 版本字符串的形状（为什么会窄化，以及窄化了什么）
//!
//! 官方用 `go/version` + `internal/gover` 解析 `go1.20.1` / `go1.21rc1` / `go1.21.0`
//! 这一类**发行版本**，再把它们截成**语言版本**（`Lang`）来比较。
//! 本仓只需两件事：`==`（比 `go1.18` 这类阈值）与 `<`/`>`（比 `GO_CURRENT`）。
//!
//! 输入**只有两个来源**（都是本仓自己产出的）：① 命令行的 `-lang` 形参；
//! ② `//go:build go1.N`（`M2` 的 `go_version_from_build` 只产出这个形状）。
//! 所以 [`GoVersion::parse_lang`] 只认 `go1.N` 的**语言版本形态**：
//!
//! * `go1.20.1` ⇒ `go1.20`、`go1.21rc1` ⇒ `go1.21`（dot-release 与预发布**剥掉**，
//!   与官方 `Lang` 同结果 —— 见 [`GoVersion::parse_lang`] 的说明）；
//! * 主版本只认 `1`；非 `go` 前缀、缺次版本号、`go2.0` 一律判**非法**（`None`）。
//!
//! ⚠️ 与官方的**唯一**已知窄化：后缀（`.1` / `rc1` / `alpha2` / 垃圾）**不校验**，
//! 一律当语言版本后缀剥掉 ⇒ `go1.21zzz` 我们接受、官方判非法。
//! 由于输入形状由本仓产出（见上），这条窄化在现有数据上**不可能**产生差异；
//! 已登记在 `M3-manifest.json` 的 `open_gaps`。
//!
//! # 为什么位置由调用方给，而不是「表里查」
//!
//! 与 `Task 22` 的类型环不同：那边的诊断点在**类型定义**上（表里有），
//! 这里的诊断点在一次**语法特性**的用法上（`-lang` 检查时调用方才知道是哪一行）。
//! 所以 [`Versions::require`] 收一个 `at: Pos` —— 这也是官方
//! `verifyVersionf(at positioner, …)` 的形状。

use std::collections::HashMap;

use rgoc_ast::AstFile;
use rgoc_lex::diag::{Diag, DiagBag, DiagCode};
use rgoc_lex::{FileId, Pos};

/// 当前工具链的语言版本（**钉死**：语料快照 `go1.27.1` ⇒ `go1.27`）。
///
/// 对应官方 `goversion.Version`。**不许**从 `rustc`/宿主/环境变量推导 ——
/// 门禁会随宿主漂。
pub const GO_CURRENT: GoVersion = GoVersion(27);

/// go1.21 —— 「文件级 `//go:build` 版本」这个特性本身的引入版本（官方 `go1_21`）。
pub const GO1_21: GoVersion = GoVersion(21);

/// Go **语言版本**（`go1.N`）。
///
/// 只保次版本号：语言版本不含 dot-release（`go1.20.1` 的语言版本就是 `go1.20`）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct GoVersion(u32);

impl GoVersion {
    /// 直接用次版本号构造（`21` ⇒ `go1.21`）。主要给测试与 `open_gaps` 用。
    #[must_use]
    pub const fn new(minor: u32) -> Self {
        Self(minor)
    }

    /// 次版本号（`go1.21` ⇒ `21`；`go1` ⇒ `0`）。
    #[must_use]
    pub const fn minor(self) -> u32 {
        self.0
    }

    /// 解析**语言版本**：`go1.21` ⇒ `go1.21`；`go1.20.1` / `go1.21rc1` ⇒ 剥掉后缀。
    ///
    /// 返回 `None` = **非法**（官方 `asGoVersion` 返回空串那一支）⇒ 调用方**不门控**。
    ///
    /// 与官方 `go/version`+`gover` 的关系：官方的 `Lang` 对一个带 dot-release 的
    /// 字符串给出**语言版本**（`"go1.20.1"` → `"go1.20"`），而 `Compare` 才看
    /// dot-release/patch。本仓只做 `Lang` 的那一步 —— 因为
    /// [`Versions::allow`] 的两种比较（阈值相等、与 `GO_CURRENT` 大小）
    /// 在只会变大的后缀面前给出相同答案（见模块文档）。
    ///
    /// 次版本号超 `u32` ⇒ **饱和**到 `u32::MAX`：官方用任意长度数字比较，
    /// 饱和后的比较结果与它一致（任何真实版本都小于 `u32::MAX`）。
    #[must_use]
    pub fn parse_lang(v: &str) -> Option<Self> {
        let rest = v.strip_prefix("go")?;
        let (major, rest) = cut_digits(rest)?;
        if major != 1 {
            return None;
        }
        if rest.is_empty() {
            return Some(Self(0)); // 「go1」= 「go1.0」（官方 `Lang` 对 1.0 也返回主版本）
        }
        let rest = rest.strip_prefix('.')?;
        let (minor, _) = cut_digits(rest)?;
        Some(Self(minor))
    }

    /// `go1.N` 文本（诊断消息用）。
    #[must_use]
    pub fn as_go_str(self) -> String {
        format!("go1.{}", self.0)
    }
}

impl std::fmt::Display for GoVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.as_go_str())
    }
}

/// 取前导十进制数字（官方 `gover.cutInt` 的对应物）。
///
/// 非空数字段 ⇒ `Some((值, 余下))`；无数字 ⇒ `None`。饱和见 [`GoVersion::parse_lang`]。
fn cut_digits(s: &str) -> Option<(u32, &str)> {
    let n = s.bytes().take_while(u8::is_ascii_digit).count();
    if n == 0 {
        return None;
    }
    let v = s[..n].parse::<u32>().unwrap_or(u32::MAX);
    Some((v, &s[n..]))
}

/// 每个文件**生效的**语言版本（官方 `check.versions` / `Info.FileVersions`）。
///
/// 值 `None` = 该文件**不门控**（`-lang` 缺省或非法，且文件没有 `//go:build` 版本）。
#[derive(Clone, Debug, Default)]
pub struct Versions(HashMap<FileId, Option<GoVersion>>);

impl Versions {
    /// 装配面：从 `(FileId, Option<GoVersion>)` 建表。
    ///
    /// 生产路径仍走 [`pkg_versions`]（`-lang` + `//go:build` 同源）；本构造器
    /// 给**测试**和将来的 `Info.FileVersions` 反序列化面用。
    pub fn from_entries(entries: impl IntoIterator<Item = (FileId, Option<GoVersion>)>) -> Self {
        Self(entries.into_iter().collect())
    }

    /// 某个文件生效的语言版本（`None` = 不门控）。
    #[must_use]
    pub fn get(&self, f: FileId) -> Option<GoVersion> {
        self.0.get(&f).copied().flatten()
    }

    /// 该文件是否**允许**使用 `want` 这个特性版本（官方 `allowVersion`）。
    ///
    /// 文件不在表里（例如跨表拿来的 `FileId`）⇒ 按「不门控」处理：**允许**。
    /// 这条兜底与官方的差别在于官方一定查得到；本仓取「宁可放过」是因为
    /// 门控是**能力边界**，误报会让合法程序编译不过（与 `D-M3-11` 同一取向）。
    #[must_use]
    pub fn allow(&self, f: FileId, want: GoVersion) -> bool {
        match self.get(f) {
            None => true,
            Some(v) => v >= want,
        }
    }

    /// [`Versions::allow`] 的门控版（官方 `verifyVersionf`）：不允许就**报一条**
    /// [`DiagCode::TypFeatureRequiresGoVersion`]（位置由 `at` 给），返回是否允许。
    ///
    /// `what` 是特性名（`"type parameters"` / `"generics"` / …），
    /// 消息形状对齐官方：`"%s requires %s or later"`。
    pub fn require(
        &self,
        f: FileId,
        want: GoVersion,
        at: Pos,
        what: &str,
        bag: &mut DiagBag,
    ) -> bool {
        if self.allow(f, want) {
            return true;
        }
        bag.push(Diag {
            pos: at,
            code: DiagCode::TypFeatureRequiresGoVersion,
            message: format!("{what} requires {want} or later"),
        });
        false
    }

    /// 表里登记了几个文件。
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// 表是否为空。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// 算每个文件的语言版本，并把两处 `TooNew` 报进 `bag`（官方 `check.go:312-348`）。
///
/// * `lang` = 命令行的 `-lang`（`None` = 缺省 ⇒ 不门控）；
/// * `files` = 同一个包的**全部**文件（`files[0]` 是包级 `TooNew` 的报告位置）；
/// * 文件标识从 `file.pos.file` 取（`AstFile` 不带 `FileId`，`Pos` 带）。
pub fn pkg_versions(lang: Option<&str>, files: &[&AstFile]) -> (Versions, DiagBag) {
    let mut bag = DiagBag::new();
    let mut map: HashMap<FileId, Option<GoVersion>> = HashMap::new();

    // 包级：`-lang` 比当前工具链新 ⇒ TooNew（位置取 `files[0]`）。
    let pkg_version = lang.and_then(GoVersion::parse_lang);
    if let Some(pv) = pkg_version
        && pv > GO_CURRENT
        && let Some(first) = files.first()
    {
        bag.push(Diag {
            pos: first.pos,
            code: DiagCode::TypPackageRequiresNewerGo,
            message: format!(
                "package requires newer Go version {pv} (application built with {GO_CURRENT})"
            ),
        });
    }

    // 逐文件
    for file in files {
        let mut v = pkg_version;
        if let Some(fv) = file.go_version.as_deref().and_then(GoVersion::parse_lang) {
            // 文件级版本 = max(fileVersion, go1.21)（官方 `check.go:332-341`）
            v = Some(if fv > GO1_21 { fv } else { GO1_21 });
            // 文件级版本比工具链新 ⇒ TooNew（位置取 `package` **名字**）
            if fv > GO_CURRENT {
                bag.push(Diag {
                    pos: file.pkg_name.pos,
                    code: DiagCode::TypFileRequiresNewerGo,
                    message: format!(
                        "file requires newer Go version {fv} (application built with {GO_CURRENT})"
                    ),
                });
            }
        }
        map.insert(file.pos.file, v);
    }

    (Versions(map), bag)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_lang_只认_go1_n() {
        assert_eq!(GoVersion::parse_lang("go1.21"), Some(GoVersion(21)));
        assert_eq!(GoVersion::parse_lang("go1.18"), Some(GoVersion(18)));
        // 语言版本：dot-release 与预发布**剥掉**（官方 `Lang` 同样）
        assert_eq!(GoVersion::parse_lang("go1.20.1"), Some(GoVersion(20)));
        assert_eq!(GoVersion::parse_lang("go1.21rc1"), Some(GoVersion(21)));
        assert_eq!(GoVersion::parse_lang("go1.21.0"), Some(GoVersion(21)));
        // 「go1」= 「go1.0」
        assert_eq!(GoVersion::parse_lang("go1"), Some(GoVersion(0)));
        assert_eq!(GoVersion::parse_lang("go1.0"), Some(GoVersion(0)));
        // 非法形态 ⇒ None（调用方据此**不门控**）
        assert_eq!(GoVersion::parse_lang(""), None);
        assert_eq!(GoVersion::parse_lang("1.21"), None);
        assert_eq!(GoVersion::parse_lang("go"), None);
        assert_eq!(GoVersion::parse_lang("go1."), None);
        assert_eq!(GoVersion::parse_lang("go2.0"), None);
        assert_eq!(GoVersion::parse_lang("gox"), None);
        // 超 u32 ⇒ 饱和（比较结果与官方的任意长度比较一致）
        assert_eq!(
            GoVersion::parse_lang("go1.99999999999999999999"),
            Some(GoVersion(u32::MAX))
        );
        assert!(GoVersion::parse_lang("go1.99999999999999999999").unwrap() > GO_CURRENT);
    }

    #[test]
    fn 显示成_go1_n() {
        assert_eq!(GoVersion(21).to_string(), "go1.21");
        assert_eq!(GoVersion(0).as_go_str(), "go1.0");
    }
}
