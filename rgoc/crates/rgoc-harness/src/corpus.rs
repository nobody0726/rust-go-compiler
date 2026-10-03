//! 语料枚举、平台过滤（`shouldTest`）与 unsupported 归类。
//!
//! 规则来源全部标注 `go_source_code/src/cmd/internal/testdir/testdir_test.go` 的行号：
//! - `shouldTest` / `match`（`:380-467`）—— 平台过滤的 tag 判定
//! - header 截断（`:517-519`）—— `strings.Cut(src, "\npackage")`
//! - 平台过滤在 switch 之前（`:522`）—— 顺序由 [`crate::instruction::dispatch`] 保证
//!
//! **tag 集合是 `go1.27.1` 的实测值**（容器内 `go list` / `go/build.Default` 得到），
//! 不是猜的。换了 oracle 版本必须重新采集 —— 这也是 T-H-06（版本不符即拒绝）存在的意义。
//!
//! **本模块不做 I/O 之外的判断**：它只回答「这个用例属于分母还是被排除、为什么」，
//! 跑不跑、判什么结果由 `oracle.rs` / `compare.rs`（T34/T35）负责。

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::instruction::{ParseError, is_go_build_line, is_plus_build_line, mode_of, parse_action};
use crate::ir::{Mode, Unsupported};

/// 平台与 tag 集合（官方 `testdir_test.go` 的 `context` + `go/build.Default`）。
///
/// 字段与官方 `context`（`:404-411`）一一对应：`cgoEnabled` / `noOptEnv` 在官方各自由
/// `-cgo` 开关与 `GO_GCFLAGS` 是否含 `-N`/`-l` 决定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorpusConfig {
    /// 目标 GOOS（M0 = `linux`）
    pub goos: String,
    /// 目标 GOARCH（M0 = `arm64`）
    pub goarch: String,
    /// oracle 的 Go 版本（M0 = `go1.27.1`，D-M0-1 的硬约束）
    pub go_version: String,
    /// `-cgo` 开关（官方默认 false）
    pub cgo_enabled: bool,
    /// `GO_GCFLAGS` 含 `-N` 或 `-l`（M0 为 false）
    pub no_opt_env: bool,
    /// `build.Default.ReleaseTags`（go1.27.1 = `go1.1` … `go1.27`）
    pub release_tags: Vec<String>,
    /// `build.Default.ToolTags`（go1.27.1 = 7 个 `goexperiment.*` + `arm64.v8.0`）
    pub tool_tags: Vec<String>,
}

impl CorpusConfig {
    /// M0 的目标配置（`go1.27.1` / linux / arm64 / `-cgo` 关 / `GO_GCFLAGS` 未设）。
    ///
    /// tag 集合取自容器内实测（2026-10-02）：
    /// `ReleaseTags` = `go1.1`…`go1.27`；`ToolTags` = `goexperiment.regabiwrappers`、
    /// `regabiargs`、`dwarf5`、`jsonv2`、`greenteagc`、`randomizedheapbase64`、
    /// `sizespecializedmalloc`、`arm64.v8.0`。
    pub fn m0() -> Self {
        Self {
            goos: "linux".into(),
            goarch: "arm64".into(),
            go_version: "go1.27.1".into(),
            cgo_enabled: false,
            no_opt_env: false,
            release_tags: (1..=27).map(|n| format!("go1.{n}")).collect(),
            tool_tags: [
                "goexperiment.regabiwrappers",
                "goexperiment.regabiargs",
                "goexperiment.dwarf5",
                "goexperiment.jsonv2",
                "goexperiment.greenteagc",
                "goexperiment.randomizedheapbase64",
                "goexperiment.sizespecializedmalloc",
                "arm64.v8.0",
            ]
            .iter()
            .map(|s| (*s).to_string())
            .collect(),
        }
    }

    /// 官方 `match`（`:421-467`）—— 逐条对齐。
    ///
    /// ⚠️ `ToolTags` **只对 `goexperiment.` 前缀的 tag 查**（`:441-443`）——
    /// 这条很容易写成「任何名字都查 ToolTags」，那样 `arm64.v8.0` 会被错判为真。
    pub fn matches(&self, name: &str) -> bool {
        if name.is_empty() {
            return false;
        }
        // tag 名只允许字母、数字、下划线与点（`:430-436`）
        if !name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
        {
            return false;
        }
        if self.release_tags.iter().any(|t| t == name) {
            return true;
        }
        if name.starts_with("goexperiment.") {
            return self.tool_tags.iter().any(|t| t == name);
        }
        if name == "cgo" && self.cgo_enabled {
            return true;
        }
        if name == self.goos || name == "gc" {
            return true;
        }
        if name == self.goarch {
            return true;
        }
        if self.no_opt_env && name == "gcflags_noopt" {
            return true;
        }
        if name == "test_run" {
            return true;
        }
        false
    }
}

// ══ 平台过滤 ═══════════════════════════════════════════════════════════════

/// 官方 `shouldTest`（`:517-524` + `:380-420`）—— 判断这个用例在目标平台上**跑不跑**。
///
/// 语义（`go/build` 的规则）：
/// 1. `//go:build` 存在时**只**用它（`:522` 的 `shouldTest` 只看 `//go:build` 行）；
///    存在多条时它们之间是 **AND**；
/// 2. 没有任何 `//go:build` 时才用旧式 `// +build` 行，多条之间是 **AND**；
/// 3. 一条约束都没有 ⇒ 通过；
/// 4. **某行解析失败 ⇒ 该行不参与判定**（`constraint.Parse` 返回 err 后官方 `continue`），
///    所以解析失败只会「少一条约束」，不会把用例判死。
pub fn should_test(header: &str, cfg: &CorpusConfig) -> bool {
    let mut saw_go_build = false;
    let mut result_go_build = true;
    for line in header.lines() {
        if !is_go_build_line(line) {
            continue;
        }
        saw_go_build = true;
        let expr = match parse_go_build(line) {
            Some(e) => e,
            None => continue, // 解析失败 → 该行不参与（官方 :407-410）
        };
        if !eval(&expr, cfg) {
            result_go_build = false;
        }
    }
    if saw_go_build {
        return result_go_build;
    }
    // 没有 //go:build → 用旧式 +build 行
    let mut result_plus = true;
    let mut saw_plus = false;
    for line in header.lines() {
        if !is_plus_build_line(line) {
            continue;
        }
        saw_plus = true;
        if !eval_plus_build(line, cfg) {
            result_plus = false;
        }
    }
    let _ = saw_plus;
    result_plus
}

/// 取 `src` 中 `"\npackage"` 之前的部分作为 header；没有 `\npackage` 时返回 `fallback`
/// （官方 `:518-521` 的注释说「some files are intentionally malformed」）。
pub fn header_of<'a>(src: &'a str, fallback: &'a str) -> &'a str {
    match src.find("\npackage") {
        Some(i) => &src[..i],
        None => fallback,
    }
}

/// 抽出一行 `//go:build` 的表达式部分（`constraint.Parse` 自己会吃前缀，
/// 这里自己剥是为了纯粹）。
fn parse_go_build(line: &str) -> Option<Expr> {
    let rest = line.trim().strip_prefix("//go:build")?;
    parse_expr(rest.trim())
}

/// 解析旧式 `// +build` 行：**空格 = OR，逗号 = AND，`!` = 非**。
fn eval_plus_build(line: &str, cfg: &CorpusConfig) -> bool {
    let rest = line.trim();
    let rest = rest.strip_prefix("//").unwrap_or(rest);
    let rest = rest.trim();
    let Some(rest) = rest.strip_prefix("+build") else {
        return true;
    };
    let text = rest.trim();
    if text.is_empty() {
        return true;
    }
    // 多个空格分开的项是 OR；同一项内逗号分隔的是 AND；`!` 取反
    text.split_whitespace().any(|clause| {
        clause.split(',').all(|lit| match lit.strip_prefix('!') {
            Some(neg) => !cfg.matches(neg.trim()),
            None => cfg.matches(lit),
        })
    })
}

// ── //go:build 表达式的递归下降解析 ──────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
enum Expr {
    Tag(String),
    Not(Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
}

/// 解析 `//go:build` 表达式：与、或、非、括号、tag。
fn parse_expr(s: &str) -> Option<Expr> {
    let toks = tokenize(s);
    if toks.is_empty() {
        return None;
    }
    let mut p = Parser { toks, pos: 0 };
    let e = p.parse_or()?;
    if p.pos != p.toks.len() {
        return None; // 有尾随 token ⇒ 解析失败
    }
    Some(e)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    Tag(String),
    And,
    Or,
    Not,
    LParen,
    RParen,
}

fn tokenize(s: &str) -> Vec<Tok> {
    let b: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        match c {
            ' ' | '\t' | '\n' | '\r' => i += 1,
            '(' => {
                out.push(Tok::LParen);
                i += 1;
            }
            ')' => {
                out.push(Tok::RParen);
                i += 1;
            }
            '!' => {
                out.push(Tok::Not);
                i += 1;
            }
            '&' if b.get(i + 1) == Some(&'&') => {
                out.push(Tok::And);
                i += 2;
            }
            '|' if b.get(i + 1) == Some(&'|') => {
                out.push(Tok::Or);
                i += 2;
            }
            _ => {
                let start = i;
                while i < b.len()
                    && !matches!(b[i], ' ' | '\t' | '\n' | '\r' | '(' | ')' | '!')
                    && !(b[i] == '&' && b.get(i + 1) == Some(&'&'))
                    && !(b[i] == '|' && b.get(i + 1) == Some(&'|'))
                {
                    i += 1;
                }
                if i == start {
                    // 单个 `&` 或 `|`（不是 `&&` / `||`）⇒ 不是合法 token
                    return Vec::new();
                }
                out.push(Tok::Tag(b[start..i].iter().collect()));
            }
        }
    }
    out
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn parse_or(&mut self) -> Option<Expr> {
        let mut left = self.parse_and()?;
        while self.peek() == Some(&Tok::Or) {
            self.pos += 1;
            let right = self.parse_and()?;
            left = Expr::Or(Box::new(left), Box::new(right));
        }
        Some(left)
    }

    fn parse_and(&mut self) -> Option<Expr> {
        let mut left = self.parse_unary()?;
        while self.peek() == Some(&Tok::And) {
            self.pos += 1;
            let right = self.parse_unary()?;
            left = Expr::And(Box::new(left), Box::new(right));
        }
        Some(left)
    }

    fn parse_unary(&mut self) -> Option<Expr> {
        match self.peek().cloned() {
            Some(Tok::Not) => {
                self.pos += 1;
                Some(Expr::Not(Box::new(self.parse_unary()?)))
            }
            Some(Tok::LParen) => {
                self.pos += 1;
                let e = self.parse_or()?;
                if self.peek() != Some(&Tok::RParen) {
                    return None;
                }
                self.pos += 1;
                Some(e)
            }
            Some(Tok::Tag(t)) => {
                self.pos += 1;
                Some(Expr::Tag(t))
            }
            _ => None,
        }
    }
}

fn eval(e: &Expr, cfg: &CorpusConfig) -> bool {
    match e {
        Expr::Tag(t) => cfg.matches(t),
        Expr::Not(inner) => !eval(inner, cfg),
        Expr::And(a, b) => eval(a, cfg) && eval(b, cfg),
        Expr::Or(a, b) => eval(a, cfg) || eval(b, cfg),
    }
}

// ══ U7：排除参数 ═════════════════════════════════════════════════════════════

/// 判断一条指令是否带「编译器内部开关」参数（**M0-tests §6 的 U7 项目决策**，非官方规则）。
///
/// 这些开关（`-gcflags` / `-d` / `-goexperiment` / `-godebug`）测的是**编译器内部实现**
/// 而不是语言语义，M0 不做（`03` §3.1）。
///
/// ⚠️ **必须检查 `args` 而不只是 `action`**：`parse_action` 按官方的 `splitQuoted`
/// 把 `// errorcheck -d=panic` 切成 `action="errorcheck"` + `args=["-d=panic"]` ——
/// 开关在 **args** 里。只看 action 会把 31 个 U7 文件全漏掉，分母会从 279 虚到 310
/// （本轮实测踩过）。
pub fn has_exclusion_args(ins: &crate::instruction::Instruction) -> bool {
    let is_ex = |a: &str| {
        a.starts_with('-')
            && (a.starts_with("-gcflags")
                || a.starts_with("-d")
                || a.starts_with("-goexperiment")
                || a.starts_with("-godebug"))
    };
    is_ex(&ins.action) || ins.args.iter().any(|a| is_ex(a))
}

// ══ 枚举 ═════════════════════════════════════════════════════════════════════

/// 枚举出来的一个用例。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumeratedFile {
    /// 文件名（如 `helloworld.go`）
    pub name: String,
    /// R1 解析出的 action（**可能不是合法指令** —— 例如 `linkmain.go`）
    pub action: String,
    /// R1b 解析出的参数
    pub instruction_args: Vec<String>,
    /// 模式（由 `mode_of(action)` 得到；未知 action 时是 `None` 的语义见 `unsupported`）
    pub mode: Mode,
    /// 该文件的 build tags（Test IR 必录字段之一）
    pub build_tags: Vec<String>,
    /// 是否被平台过滤（**仍在分母内**）
    pub target_filtered: bool,
    /// 是否带排除参数（U7）
    pub has_exclusion_args: bool,
    /// 若不在分母内：归到哪一条 U 及原因（**必填** —— 「每个不通过的用例都要能说清落在哪一条」）
    pub unsupported: Option<Unsupported>,
}

impl EnumeratedFile {
    /// 是否计入 M0 分母（v0 支持集 ∧ 无排除参数）。
    ///
    /// **平台过滤不影响这个判断** —— `03` §3.3：过滤项不计入分子、仍计入分母。
    pub fn in_denominator(&self) -> bool {
        self.mode.is_v0_supported() && !self.has_exclusion_args
    }
}

/// 语料枚举的结果。
#[derive(Debug, Clone)]
pub struct CorpusReport {
    /// 顶层 `*.go` 总数
    pub total: usize,
    /// M0 分母
    pub denominator: usize,
    /// 分母内按模式的计数
    pub by_mode: BTreeMap<Mode, usize>,
    /// 分母内被平台过滤的数量（仍在分母）
    pub target_filtered: usize,
    /// 排除总数（= total − denominator）
    pub excluded: usize,
    /// 排除项按 U 编号的计数
    pub excluded_by_u: BTreeMap<String, usize>,
    /// 逐个文件的结果（按文件名排序，保证可复现）
    pub files: Vec<EnumeratedFile>,
}

impl CorpusReport {
    /// 该文件是否在分母内。
    pub fn in_denominator(&self, f: &EnumeratedFile) -> bool {
        f.in_denominator()
    }

    /// 一份适合断言/记报告的扁平摘要（键名与 T33 验收测试一致）。
    pub fn summary(&self) -> BTreeMap<String, usize> {
        let mut s = BTreeMap::new();
        s.insert("total".into(), self.total);
        s.insert("denominator".into(), self.denominator);
        s.insert("excluded".into(), self.excluded);
        s.insert("target_filtered".into(), self.target_filtered);
        s.insert("executed".into(), self.denominator - self.target_filtered);
        for (m, n) in &self.by_mode {
            s.insert(format!("by_mode/{}", m.name()), *n);
        }
        for (u, n) in &self.excluded_by_u {
            s.insert(format!("u/{u}"), *n);
        }
        s
    }
}

/// 枚举失败。
#[derive(Debug)]
pub enum CorpusError {
    /// 目录读不了
    Io(std::io::Error),
    /// 有文件的 action 解析不出来（官方对 `:497`/`:514` 是硬失败）
    Unparsable {
        /// 文件名
        file: String,
        /// R1 的失败类型
        why: ParseError,
    },
    /// action 不是 16 个指令之一，且**没被平台过滤** ⇒ 官方会 `Fatalf`（`:558`）
    UnknownAction {
        /// 文件名
        file: String,
        /// 被截断的 action 文本
        action: String,
    },
}

impl fmt::Display for CorpusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "读语料目录失败：{e}"),
            Self::Unparsable { file, why } => write!(f, "{file} 的 action 解析失败：{why}"),
            Self::UnknownAction { file, action } => {
                write!(f, "unknown pattern（{file}）：{action:?}")
            }
        }
    }
}

impl std::error::Error for CorpusError {}

/// 枚举 `dir` 顶层的 `*.go`（**不递归** —— U9：子目录不在 M0 范围）。
///
/// 归类规则（口径照 `M0-tests.md` §6.1 的冻结定义）：
/// ```text
/// 分母 = 顶层 *.go 中「action 属于 v0 支持集 ∧ action 不带排除参数」的文件集
/// ```
/// - 不在 v0 支持集 → 归到该模式的 U 编号（`Mode::unsupported_code`）
/// - 未知 action（如 `linkmain.go` 的 `Copyright …`）→ 平台过滤拦下记 filtered；
///   平台过滤放行则硬失败（官方 `:558`）→ 归 U14
/// - v0 模式但带排除参数 → U7
/// - `// skip` → U13（`skipped-by-design`）
pub fn enumerate(dir: &Path, cfg: &CorpusConfig) -> Result<CorpusReport, CorpusError> {
    let mut names: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(CorpusError::Io)? {
        let entry = entry.map_err(CorpusError::Io)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        // 只取顶层 *.go（U8/U9：子目录不纳入；U11：types2 的 testdata 不纳入）
        if entry.path().is_file() && name.ends_with(".go") {
            names.push(name);
        }
    }
    // 排序保证可复现（`03` §3.4「可重复性」）
    names.sort();

    let mut files = Vec::with_capacity(names.len());
    for name in names {
        let path = dir.join(&name);
        let src = std::fs::read_to_string(&path).map_err(CorpusError::Io)?;
        files.push(classify(&name, &src, cfg)?);
    }

    let total = files.len();
    let mut by_mode: BTreeMap<Mode, usize> = BTreeMap::new();
    let mut excluded_by_u: BTreeMap<String, usize> = BTreeMap::new();
    let mut denominator = 0usize;
    let mut target_filtered = 0usize;
    for f in &files {
        if f.in_denominator() {
            denominator += 1;
            *by_mode.entry(f.mode).or_insert(0) += 1;
            if f.target_filtered {
                target_filtered += 1;
            }
        } else {
            let code = f
                .unsupported
                .as_ref()
                .map(|u| u.code.clone())
                .unwrap_or_else(|| "?".into());
            *excluded_by_u.entry(code).or_insert(0) += 1;
        }
    }
    Ok(CorpusReport {
        total,
        denominator,
        by_mode,
        target_filtered,
        excluded: total - denominator,
        excluded_by_u,
        files,
    })
}

/// 判定单个文件。
fn classify(name: &str, src: &str, cfg: &CorpusConfig) -> Result<EnumeratedFile, CorpusError> {
    let ins = parse_action(src).map_err(|why| CorpusError::Unparsable {
        file: name.to_string(),
        why,
    })?;
    let header = header_of(src, &ins.action);
    let target_filtered = !should_test(header, cfg);
    let build_tags: Vec<String> = header
        .lines()
        .filter(|l| is_go_build_line(l) || is_plus_build_line(l))
        .map(str::trim)
        .map(|l| {
            l.trim_start_matches("//go:build")
                .trim_start_matches("//")
                .trim_start_matches("+build")
                .trim()
                .to_string()
        })
        .collect();
    let has_ex = has_exclusion_args(&ins);
    // ⚠️ **`target_filtered` 与 U 归类是正交的两件事**。
    // 官方顺序（`:522` 过滤先于 `:541` switch）只决定「未知 action 会不会被 Fatalf」，
    // **不**改变「这个文件算不算 U7」：分母的口径是「v0 集 ∧ 无排除参数」，
    // 与平台过滤无关。本轮实测踩过这个坑：把「被平台过滤」当成「不进排除清单」，
    // 于是 11 个既是 U7/U5/U6/U1、又带构建约束的文件（checkbce / linknameasm /
    // live / newinline / wasmmemsize / nilptr5_aix / nilptr5_wasm …）全部失去归类，
    // U7 从 31 掉到 24、多出一个 11 项的「?」桶。
    let unsupported = match mode_of(&ins.action) {
        // ① v0 支持集 ∧ 无排除参数 ⇒ 在分母内，不归任何 U
        Some(m) if m.is_v0_supported() && !has_ex => None,
        // ② v0 支持集 ∧ 带排除参数 ⇒ U7（**必须带 `is_v0_supported` 守卫**：
        //    否则非 v0 模式（如带 -gcflags 的 errorcheckwithauto）会被抢到 U7，
        //    而冻结口径是 U7 只收 v0 集内的用例）
        Some(m) if m.is_v0_supported() => Some(Unsupported {
            code: "U7".into(),
            reason: format!("带编译器内部开关参数（{}）", truncate(&ins.action, 32)),
        }),
        // ③ **非 v0 模式 ⇒ 按 §6 的 U 编号**（U1–U6 / U13）。
        //    注意即使它同时带排除参数也归这里 —— 「模式本身不支持」比「带内部开关」
        //    更根本。口径与 T29 冻结的归因表一致：U7 只收 v0 集内的用例。
        Some(m) => Some(Unsupported {
            code: m.unsupported_code().unwrap_or("?").into(),
            reason: format!("模式 {} 不在 v0 支持集", ins.action),
        }),
        // ④ `// skip`：不是 16 个之一（`mode_of` 返回 None），但要单列 U13
        None if crate::instruction::is_skip(&ins.action) => Some(Unsupported {
            code: "U13".into(),
            reason: "上游设计即跳过（官方 t.Skip(\"skip\")）".into(),
        }),
        // ⑤ 未知 action 且被平台过滤 ⇒ 官方在 switch 之前就 Skip 了，这是 U14
        None if target_filtered => Some(Unsupported {
            code: "U14".into(),
            reason: format!(
                "action 不是 16 个指令之一（{}），但文件被平台过滤 ⇒ 官方在 switch 之前 t.Skip",
                truncate(&ins.action, 40)
            ),
        }),
        // ⑥ 未被过滤的未知 action 是**硬错误**：官方会 `Fatalf`（`:558`），
        //    枚举阶段就该报出来，不能悄悄归个类当作没事
        None => {
            return Err(CorpusError::UnknownAction {
                file: name.to_string(),
                action: truncate(&ins.action, 60),
            });
        }
    };

    let mode = mode_of(&ins.action).unwrap_or(Mode::Skip);
    Ok(EnumeratedFile {
        name: name.to_string(),
        action: ins.action.clone(),
        instruction_args: ins.args.clone(),
        mode,
        build_tags,
        target_filtered,
        has_exclusion_args: has_ex,
        unsupported,
    })
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    let head: String = s.chars().take(n).collect();
    format!("{head}…")
}

/// 语料根目录（`go_source_code/`）。
///
/// 默认按 crate 位置推算（`<repo>/go_source_code`）—— 测试与 `xtask` 都用它；
/// 可用 `RGOC_CORPUS_ROOT` 覆盖。
pub fn corpus_root() -> PathBuf {
    if let Ok(d) = std::env::var("RGOC_CORPUS_ROOT") {
        return PathBuf::from(d);
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../go_source_code")
        .to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m0() -> CorpusConfig {
        CorpusConfig::m0()
    }

    /// tag 集合是 go1.27.1 的实测值 —— 数字写死当锚，换 oracle 版本时要显式改。
    #[test]
    fn tag_集合锚在_go1271_实测值上() {
        let c = m0();
        assert_eq!(c.release_tags.len(), 27, "go1.1 … go1.27");
        assert_eq!(c.release_tags.first().map(String::as_str), Some("go1.1"));
        assert_eq!(c.release_tags.last().map(String::as_str), Some("go1.27"));
        assert!(c.matches("go1.27"));
        assert!(!c.matches("go1.28"), "go1.28 尚未发布，不该匹配");
        assert!(c.matches("goexperiment.dwarf5"));
        assert!(!c.matches("goexperiment.simd"), "simd 默认关闭");
        // ToolTags 只对 goexperiment.* 前缀查（官方 :441-443）
        assert!(
            !c.matches("arm64.v8.0"),
            "arm64.v8.0 虽在 ToolTags 里，但官方不查它"
        );
    }

    /// `&&` 优先于 `||`（Go 的语法约定）
    #[test]
    fn 运算优先级() {
        let c = m0();
        assert!(should_test("//go:build windows || linux && arm64", &c));
        assert!(!should_test("//go:build windows || linux && amd64", &c));
    }

    /// 解析失败只「少一条约束」，不判死用例
    #[test]
    fn 解析失败不判死() {
        let c = m0();
        assert!(should_test("//go:build ((linux", &c));
        assert!(should_test("//go:build linux &&", &c));
    }

    #[test]
    fn u7_检查_args_而不只是_action() {
        let ins = |s: &str| parse_action(s).unwrap();
        assert!(has_exclusion_args(&ins("run -gcflags -l=4")));
        // 这条最容易被漏：开关在 args 里，action 只是 "errorcheck"
        assert!(has_exclusion_args(&ins("errorcheck -d=panic")));
        assert!(!has_exclusion_args(&ins("errorcheck -0 -l")));
        assert!(!has_exclusion_args(&ins("run")));
    }
}
