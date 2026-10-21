//! 检查面的**版本源**（M4/Task 18）与**检查会话**（M4/Task 20）。
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
//!
//! # 检查会话（Task 20 起）
//!
//! [`Session`] 是检查主体的**诊断流与上报层**（官方 `Checker` 的错误面）：
//! 装载诊断经 [`Session::seed`] 先入流，检查面（Task 21–36）逐任务往
//! [`Session::report`] 上报。Cheap-trick 抑制与「次级消息留流」两条纪律
//! 见 [`Session`] 的文档。入口是 [`check_package`]。

use rgoc_ast::AstFile;
use rgoc_lex::{Diag, DiagBag, DiagCode, Pos};
use rgoc_load::Package;
use rgoc_types::TypeTable;
use rgoc_types::version::{Versions, pkg_versions};

use crate::diag::RawDiag;

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

// ═══ 检查会话（M4/Task 20）═══════════════════════════════════════════════════

/// 检查装置选项（CLI 旗标在检查面的投影）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CheckOptions {
    /// `--test-hooks`：注入官方测试内建（`assert` / `trace`，`F13`）。
    ///
    /// 对应官方 `types.DefPredeclaredTestFuncs()` 的装置级开关；
    /// **Task 29 起生效**（本任务只把它带进会话）。
    pub test_hooks: bool,
}

/// 检查结果：**原始诊断流** + 渲染入口。
///
/// 「原始」= 含次级链（`\t` 开头）与 soft 位；对外渲染与退出码判据都走
/// [`CheckedPackage::render`] / [`CheckedPackage::rendered_count`]（过滤后）
/// —— 与 oracle 的行集合等价。
#[derive(Clone, Debug, Default)]
pub struct CheckedPackage {
    /// 诊断流；装载诊断在前、检查诊断在后（官方 go/types 是**同一个**
    /// checker 错误流，装载即检查的一部分）。
    pub diags: Vec<RawDiag>,
}

impl CheckedPackage {
    /// `C8` 渲染入口（`--diag-dump`）；`file` = `# file:` 行的人读文本。
    #[must_use]
    pub fn render(&self, file: &str) -> String {
        crate::render::render(&self.diags, file)
    }

    /// 渲染得出来的数据行数 —— **退出码判据**（`> 0` ⇒ 调用方退出码 1）。
    ///
    /// 与 oracle 的「过滤后 `len(diags) > 0`」同语义：只含次级消息的文件
    /// 两侧都算「无诊断」。
    #[must_use]
    pub fn rendered_count(&self) -> usize {
        self.diags
            .iter()
            .filter(|d| crate::render::is_rendered(d))
            .count()
    }
}

/// 检查**会话**：诊断流 + 上报层（官方 `Checker` 的错误面）。
///
/// # 上报层三条纪律
///
/// 1. **Cheap-trick**（`F2`⑧；官方 `go/types/errors.go:111-123`）：
///    **已有首错**（`firstErr != nil`）且**首个子错误**（`\n\t` 之前的主句）
///    的消息里 `"invalid operand"` / `"invalid type"` 出现在位置 **> 0**
///    ⇒ 丢弃（不进流）。首错自己永不被吞（`firstErr == nil` 不检查）；
///    soft 同样把「首错已立」置起来（官方 `report` 不区分软硬）。
/// 2. **次级消息留流**：`\t` 开头的条目保留在 [`Session::diags`]（人读的
///    澄清链），渲染层过滤 —— 过滤点是渲染出口，不是上报层。
/// 3. **装载先于检查**：装载诊断以硬错误入流（[`Session::seed`]），与官方
///    「装载即检查的一部分」同序。
///
/// `pkg` / `opts` 由会话携带：Task 21–36 的各个检查面从这里取作用域、
/// 类型表与装置开关。
pub struct Session<'a> {
    /// 被检查的包（作用域 / 对象 / 常量值都从这里读）。
    pub pkg: &'a Package,
    /// 类型表（**调用方持有**：`Package` 不拥有类型 —— 装载的既定分工）。
    /// Task 21 起表达式检查要在上面挂新类型（`*T` / untyped bool / 默认型）。
    pub tt: &'a mut TypeTable,
    /// 装置选项。
    pub opts: &'a CheckOptions,
    /// **检查面折叠出的包级常量**（M4/Task 30）：名字 → `(类型, 值)`。
    ///
    /// 装载面对「需类型信息」的 `const` 初始化式一律拒绝且**不给值**
    /// （`D-M3-19`）；检查面能算 `len` / `cap` 的常量结果，折叠后写在这里，
    /// 由 `expr` 的 `ident` 顶替装载面留下的「常量无值」。见
    /// [`crate::constfold`]。
    pub(crate) consts: std::collections::BTreeMap<String, (rgoc_types::TypeId, rgoc_const::Const)>,
    diags: Vec<RawDiag>,
    reported_any: bool,
}

impl std::fmt::Debug for Session<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("pkg", &self.pkg.name)
            .field("opts", &self.opts)
            .field("diags", &self.diags)
            .field("reported_any", &self.reported_any)
            .finish()
    }
}

impl<'a> Session<'a> {
    /// 开一个会话（空流）。
    #[must_use]
    pub fn new(pkg: &'a Package, tt: &'a mut TypeTable, opts: &'a CheckOptions) -> Self {
        Self {
            pkg,
            tt,
            opts,
            consts: std::collections::BTreeMap::new(),
            diags: Vec::new(),
            reported_any: false,
        }
    }

    /// 装载诊断入流（作为硬错误；顺序在检查诊断之前）。
    ///
    /// 装载诊断也过一遍 [`Session::report`] 的上报层 —— 官方装载与检查是
    /// 同一个错误流，共用一条上报纪律；装载侧消息不含 Cheap-trick 的两个
    /// 关键词，实际是恒等变换。
    pub fn seed(&mut self, bag: DiagBag) {
        for d in bag.into_sorted() {
            self.report(RawDiag::hard(d));
        }
    }

    /// 装载诊断入流，**但丢掉检查面已经自己算出来的 `const` 拒绝**
    /// （M4/Task 30）。
    ///
    /// `foldable` 是 [`crate::constfold::fold_package_consts`] 给出的
    /// 「检查面能折成常量」的初始化式位置集合。装载面在那些位置报的
    /// `TypInvalidConstInit` 是 `U-M3-08` 的**装载面**半边 —— 检查面已经
    /// 有能力判了，就不该再转述一遍（否则 `const x = len("abc")` 在
    /// `rgoc check` 面永远红）。
    ///
    /// 只丢**恰好这些位置**的 `TypInvalidConstInit`：`const I = H + 1`
    /// （`H` 是变量）、`const x = f()`、`const x = pkg.C` 这些检查面也判不了
    /// 的形态，装载诊断**照旧入流**（`T-TY-83` 正向不回归）。
    pub fn seed_filtered(&mut self, bag: DiagBag, foldable: &std::collections::BTreeSet<Pos>) {
        for d in bag.into_sorted() {
            if d.code == DiagCode::TypInvalidConstInit && foldable.contains(&d.pos) {
                continue;
            }
            self.report(RawDiag::hard(d));
        }
    }

    /// 当前诊断条数（预演快照用）。
    pub(crate) fn diag_len(&self) -> usize {
        self.diags.len()
    }

    /// 当前 `reported_any` 位（预演快照用）。
    pub(crate) fn reported_any(&self) -> bool {
        self.reported_any
    }

    /// 回滚到快照（预演诊断丢弃用）。
    ///
    /// `reported_any` 必须一起回滚：它是 Cheap-trick 抑制的开关，预演把它
    /// 置真会让主检查的「invalid operand / invalid type」抑制**提前生效**，
    /// 从而静默吞掉本该报的诊断。
    pub(crate) fn rollback_diags(&mut self, diag_mark: usize, reported_mark: bool) {
        self.diags.truncate(diag_mark);
        self.reported_any = reported_mark;
    }

    /// 上报一条原始诊断（**唯一入口**；Cheap-trick 在这里）。
    pub fn report(&mut self, raw: RawDiag) {
        if self.reported_any {
            let primary = raw.diag.message.split("\n\t").next().unwrap_or("");
            let hits = |needle: &str| primary.find(needle).is_some_and(|i| i > 0);
            if hits("invalid operand") || hits("invalid type") {
                return;
            }
        }
        self.reported_any = true;
        self.diags.push(raw);
    }

    /// 上报一条**硬**错误。
    pub fn report_hard(&mut self, diag: Diag) {
        self.report(RawDiag::hard(diag));
    }

    /// 上报一条**软**错误（官方 `softErrorf` 面）。
    pub fn report_soft(&mut self, diag: Diag) {
        self.report(RawDiag::soft(diag));
    }

    /// 当前流（只读）。
    #[must_use]
    pub fn diags(&self) -> &[RawDiag] {
        &self.diags
    }

    /// 收会话取流。
    #[must_use]
    pub fn into_diags(self) -> Vec<RawDiag> {
        self.diags
    }

    /// 收会话取检查结果。
    #[must_use]
    pub fn into_checked(self) -> CheckedPackage {
        CheckedPackage { diags: self.diags }
    }
}

/// 检查一个**已装载**的包 —— 会话真身（Task 20 起）。
///
/// 当前内容 = 「装载诊断入流」+ Task 21 的**表达式运算符面**（包级 `var`
/// 初始化式；`Decl::Var.values`）+ Task 25 的**函数体语句面**（`check_package_bodies`）。
/// 语句 / 内建 / 门控点由 Task 22–36 逐个挂到会话上（每个判定走 [`Session::report`]）。
#[must_use]
pub fn check_package(
    pkg: &Package,
    tt: &mut TypeTable,
    load_diags: DiagBag,
    opts: &CheckOptions,
) -> CheckedPackage {
    let mut session = Session::new(pkg, tt, opts);
    // Task 30：先跑「折叠预演」，再按结果过滤装载面的 `const` 拒绝。
    let foldable = crate::constfold::fold_package_consts(&mut session);
    session.seed_filtered(load_diags, &foldable);
    crate::expr::check_package_decls(&mut session);
    crate::stmt::check_package_bodies(&mut session);
    session.into_checked()
}
