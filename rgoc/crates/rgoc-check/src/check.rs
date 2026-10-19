//! 检查面的**版本源**（M4/Task 18）：首行 harness 旗标 + 文件版本同源进入。
//!
//! # 两层
//!
//! 1. **首行旗标**（官方 `go/types/check_test.go:108-126` 的 `parseFlags`）：
//!    只读**第一个文件**；必须 `//` 后（允许空白）紧跟 `-`；认三个旗标
//!    `-lang` / `-goexperiment` / `-fakeImportC`。后两个在 M4 装置里
//!    **响亮失败**（`U-M4-04` / `U-M4-05`）—— 静默忽略会让对撞成绩虚高。
//! 2. **文件版本**（`F11`）：`//go:build go1.N` 由 M2 解析进
//!    [`AstFile::go_version`]，[`versions`] 沿 M3 [`pkg_versions`] 算
//!    「生效版本 = `max(file, go1.21)` 且**取代**包版本」。
//!
//! 为什么解析器放在 `rgoc-check` 而不是 CLI：`M4-plan` Task 18 的测试面在
//! `rgoc-check/tests/version_source.rs`，而 `rgoc-check` 不能反向依赖
//! `rgoc-driver`。CLI 只负责读第一个文件并把结果交给装载器（编排层纪律）。
//!
//! # `--lang` 与首行注释谁赢
//!
//! **显式 `--lang` 赢**（`Ruling 18-1`）：它是 `D-M3-15` 的显式覆盖路径；
//! 没有显式值时用首行注释。官方 harness 没有 CLI 旗标，所以这条合并规则是
//! 本仓的**扩展**，不改变「注释 ⇒ `Config.GoVersion`」的对撞面。

use rgoc_ast::AstFile;
use rgoc_lex::DiagBag;
use rgoc_types::version::{Versions, pkg_versions};

/// 首行注释里的三个旗标（官方 `parseFlags` 的 `flag.FlagSet`）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HarnessFlags {
    /// `-lang=go1.N`（缺省 = 不门控）。
    pub lang: Option<String>,
    /// `-goexperiment=…`（非空 ⇒ 本装置不支持，`U-M4-04`）。
    pub goexperiment: Option<String>,
    /// `-fakeImportC`（⇒ 本装置不支持，`U-M4-05`）。
    pub fake_import_c: bool,
}

/// 官方 `parseFlags` 的行长上限（**含**换行前的全部字节，`check_test.go:119`）。
const MAX_FLAG_LINE: usize = 256;

/// 解析**第一个文件首行**的 harness 旗标。
///
/// 与官方逐条对齐：
/// * 首行不是 `//` 行注释 ⇒ 无旗标（不是错）；
/// * `//` 之后到第一个 `-` 之间只允许空白，否则无旗标；
/// * 没有换行或行长 > 256 ⇒ `Err`（官方 `flags comment line too long`）；
/// * token 由空白切分；遇到第一个非旗标 token（含 `-` / `--`）⇒ 停止；
/// * 未知 flag / 缺值 / 非法布尔 ⇒ `Err`（官方 `flag.PanicOnError`）。
pub fn parse_harness_flags(src: &[u8]) -> Result<HarnessFlags, String> {
    let mut out = HarnessFlags::default();
    let Some(rest) = src.strip_prefix(b"//") else {
        return Ok(out);
    };
    let Some(dash) = rest.iter().position(|b| *b == b'-') else {
        return Ok(out);
    };
    if !rest[..dash].iter().all(u8::is_ascii_whitespace) {
        return Ok(out);
    }
    let Some(end) = rest.iter().position(|b| *b == b'\n') else {
        // 官方把「没有换行」也归进这一条（`end < 0`）。
        return Err("flags comment line too long".to_owned());
    };
    if end > MAX_FLAG_LINE {
        return Err("flags comment line too long".to_owned());
    }
    let line = String::from_utf8_lossy(&rest[..end]);

    let mut it = line.split_ascii_whitespace().peekable();
    while let Some(tok) = it.next() {
        // 官方 `flag.Parse` 在第一个非旗标参数处停下（`-` 是参数不是旗标）。
        if tok == "-" || tok == "--" || !tok.starts_with('-') {
            break;
        }
        let Some(body) = flag_body(tok) else {
            break;
        };
        let (name, value) = match body.split_once('=') {
            Some((n, v)) => (n, Some(v.to_owned())),
            None => (body, None),
        };
        match name {
            "lang" => {
                out.lang = Some(take_value(name, value, &mut it)?);
            }
            "goexperiment" => {
                out.goexperiment = Some(take_value(name, value, &mut it)?);
            }
            "fakeImportC" => match value {
                // 布尔旗标**不消费**下一个 token（Go `flag` 的布尔语义）。
                None => out.fake_import_c = true,
                Some(v) => {
                    out.fake_import_c = parse_bool(&v)
                        .ok_or_else(|| format!("invalid boolean value {v:?} for -fakeImportC"))?;
                }
            },
            _ => return Err(format!("flag provided but not defined: -{name}")),
        }
    }
    Ok(out)
}

/// 去掉一或两个前导 `-`（Go `flag` 的旗标形态）；更多 `-` 归入名字。
fn flag_body(tok: &str) -> Option<&str> {
    let t = tok.strip_prefix('-')?;
    Some(t.strip_prefix('-').unwrap_or(t))
}

/// 取旗标值：`-name=v` 用 `v`，否则消费下一个 token。
fn take_value(
    name: &str,
    value: Option<String>,
    it: &mut std::iter::Peekable<std::str::SplitAsciiWhitespace<'_>>,
) -> Result<String, String> {
    match value {
        Some(v) => Ok(v),
        None => it
            .next()
            .map(str::to_owned)
            .ok_or_else(|| format!("flag needs an argument: -{name}")),
    }
}

/// Go `flag` 的布尔字面量集合（`strconv.ParseBool`）。
fn parse_bool(v: &str) -> Option<bool> {
    match v {
        "1" | "t" | "T" | "true" | "TRUE" | "True" => Some(true),
        "0" | "f" | "F" | "false" | "FALSE" | "False" => Some(false),
        _ => None,
    }
}

/// 检查面的版本源：显式 `--lang` > 首行 `-lang=`；两个未支持旗标响亮失败。
///
/// 返回 `Ok(None)` = **不门控**（`-lang` 缺省/空）。
pub fn version_source(cli_lang: Option<&str>, first_src: &[u8]) -> Result<Option<String>, String> {
    let flags = parse_harness_flags(first_src)?;
    if let Some(exp) = flags.goexperiment.as_deref().filter(|e| !e.is_empty()) {
        return Err(format!("-goexperiment={exp} 未支持（U-M4-04）"));
    }
    if flags.fake_import_c {
        return Err("-fakeImportC 未支持（U-M4-05）".to_owned());
    }
    Ok(cli_lang.map(str::to_owned).or(flags.lang))
}

/// 逐文件生效版本表（检查面的**唯一入口**；Task 20 的会话直接消费）。
///
/// 语义全在 M3 [`pkg_versions`]（`F11`）：包级 `-lang` 缺省/非法 ⇒ 不门控；
/// 文件有 `//go:build go1.N` ⇒ `max(file, go1.21)` 且**取代**包版本；
/// 两处 `TooNew` 的诊断一并返回。
pub fn versions(lang: Option<&str>, files: &[&AstFile]) -> (Versions, DiagBag) {
    pkg_versions(lang, files)
}
