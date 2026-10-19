//! `//rgoc:cmp` 指令的读取与判定 —— M3 的**第二类比对面**（`T-TY-92` 的同一性、
//! `T-TY-93` 的可表示性）。
//!
//! 与 oracle 侧（`tools/types-oracle/cmp.go`）**读同一份 fixture**、出**同构**的
//! 规范行 `<行号>\t<kind>\t<result>`（`result ∈ true|false|reject`）。语法与
//! 行号口径见那边的模块头；这里只说 rgoc 侧的取舍。
//!
//! # 为什么也走「回填成声明」
//!
//! 注释里写的是**类型表达式 / 常量表达式**的原文。rgoc 的类型层没有「从一个
//! 字符串求值」的入口（有也不该有：那等于把解析器再抄一份）。于是把原文回填
//! 成声明，交给**同一条**装载管线（[`rgoc_load::load_sources`]）：
//!
//! ```text
//! package <fixture 包名>
//! type  __rgoc_cmp_<行>_a = <A>       ← identical
//! type  __rgoc_cmp_<行>_b = <B>
//! const __rgoc_cmp_<行>_c = <C>       ← representable
//! type  __rgoc_cmp_<行>_t = <T>
//! ```
//!
//! ⚠️ 与 oracle 的合成源**不完全一样**（那边多一条 `var v T = C`）：那条在这里
//! **没用** —— 可表示性由 `rgoc_const::representable` 判，赋值检查（可赋值性）
//! 是 M4 的事（`U-M3-02`）。两侧合的是**操作数**与**坐标**，不是合成源本身。
//!
//! # 判定与拒绝
//!
//! 七种 kind（`R-32-2`，与 oracle 侧**同一套**名字与操作数个数）：
//!
//! | kind | 形状 | 判定 |
//! |---|---|---|
//! | `identical` | 二元 | [`rgoc_types::identical`] |
//! | `representable` | 二元 | [`rgoc_const::representable`] |
//! | `assignable` | 二元 | [`rgoc_types::assignable`]（M4/Task 15 起作答） |
//! | `implements` | 二元 | [`rgoc_types::implements`]`(…, false)` |
//! | `satisfies` | 二元 | [`rgoc_types::implements`]`(…, true)`（类型集参与） |
//! | `methodset` | **一元** | [`rgoc_types::methodset`]（名字升序、`,` 连接；空集 `-`） |
//! | `instance` | 二元 + 实参列表 | [`rgoc_types::instantiate`]（成功 ⇒ C7 单根 dump） |
//!
//! - `identical` ⇒ M4/Task 10 起**没有拒绝面**（`U-M3-05` 解除）。
//! - `representable` ⇒ `Err(code)` ⇒ `false`（不可表示就是不可表示，**不是**拒绝）；
//!   目标连本体都不是基本类型 ⇒ **响亮报错**（Ruling 27-1 的同一条纪律）。
//! - `assignable` / `implements` / `satisfies` / `methodset` ⇒ 语言层的
//!   `Err(code)` ⇒ **`reject`** + `# reject <行> <CODE>`：对撞时按白名单排除
//!   （`M3-tests.md` §6），**排除集本身也要被断言**（`collide_cmp.rs`）。
//! - `instance` ⇒ `Ok(v)` ⇒ `ok` + C7 单根 dump（每行前加一个 tab）；
//!   `Err(code)` ⇒ `reject:<category>`（类别 = `rgoc-check::diag` 的 26 类投影：
//!   计数错 ⇒ `type-arg`，约束不满足 ⇒ `implements`）。**不**走白名单：
//!   类别本身就是判定结果，两侧必须逐字一致。F 必须是**本包声明的泛型具名
//!   类型的名字**（非泛型 / 不存在 ⇒ 样本违规，响亮报错）。
//!
//! # 来源标注
//!
//! 头几行是 `# source: rgoc` / `# files:` / `# package:` / `# cmp`（C7 §2 规则 1
//! 两侧都忽略 `#` 行）。`reject` 条目的**理由**紧跟本行，形如
//! `# reject <行> <CODE>` —— 同样以 `#` 开头，不进逐字节 diff，但白名单断言读它。

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use rgoc_check::diag::category_of;
use rgoc_const::{Const, representable};
use rgoc_lex::{Lexer, Mode, TokenKind};
use rgoc_load::{Package, Source, load_sources};
use rgoc_types::{
    BasicId, Sizes, Type, TypeId, TypeTable, assignable, identical, implements, instantiate,
    methodset, try_dump_types, unalias,
};

use crate::types_cmd::TypesOutcome;

/// 合成文件在对撞坐标里的名字（与 oracle 侧的 `<rgoc:cmp>` 同位；两者不互相 diff，
/// 各自的诊断都靠它归位）。
const SYN_NAME: &str = "<rgoc:cmp>";

/// `//rgoc:cmp` 的 kind 不认识时的**稳定拒绝码**（`T-TY-82`）。
///
/// 为什么是驱动层的码、而不是 [`rgoc_lex::DiagCode`]：它说的是「这个**请求**
/// 我们不判」——没有官方对应代号，也不来自某个源码位置。它与语言层的能力边界
/// （如 `TYP-UNSUPPORTED-ASSIGNABILITY`：可赋值性的类型参数面）同理不同层：
/// 那条是**语言层**的能力边界（判可赋值性时撞上没做完的推理），这条是
/// **请求层**的能力边界（连问都没法问）。
///
/// M4/Task 15 起 kind 集合扩到七个（`R-32-2`）；**封闭性不变** —— 拼错的
/// kind（`unify` / `infer` / `Assignable`）仍走这个码。集合与 oracle 侧共用：
/// fixture 由两侧读**同一份**文件，输入语言分叉比少支持一个 kind 坏。
pub const KIND_UNSUPPORTED: &str = "TYP-CMP-KIND-UNSUPPORTED";

/// 支持的 kind（报错要说出支持面；顺序固定，便于人读与测试）。
pub const SUPPORTED_KINDS: &str =
    "identical / representable / assignable / implements / satisfies / methodset / instance";

/// 指令种类（`D-M3-17` 的两种 + M4/Task 15 的五个；`R-32-2`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CmpKind {
    Identical,
    Representable,
    Assignable,
    Implements,
    Satisfies,
    MethodSet,
    Instance,
}

impl CmpKind {
    fn as_str(self) -> &'static str {
        match self {
            CmpKind::Identical => "identical",
            CmpKind::Representable => "representable",
            CmpKind::Assignable => "assignable",
            CmpKind::Implements => "implements",
            CmpKind::Satisfies => "satisfies",
            CmpKind::MethodSet => "methodset",
            CmpKind::Instance => "instance",
        }
    }
}

/// 一条 `//rgoc:cmp` 指令。
#[derive(Debug, Clone)]
struct CmpEntry {
    /// 指令所在的**物理**行号（`Pos::line` 是绝对行号，不带 `//line` 重映射）。
    line: u32,
    kind: CmpKind,
    /// 第一个操作数（二元 kind 的左侧；`methodset` 的唯一操作数）
    a: String,
    /// 第二个操作数（二元 kind 的右侧；一元 kind 为空）
    b: String,
    /// `instance` 的实参列表（其他 kind 为空）
    args: Vec<String>,
}

/// 合成文件里的一行对应哪个操作数（诊断归位用）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct SynthRole {
    /// `entries` 里的下标。
    entry: usize,
    /// 该操作数的原文。
    text: String,
    /// 该操作数该被说成什么（错误消息用词）。
    noun: &'static str,
}

/// 合成源 + 逐行归属。
struct Synth {
    src: String,
    /// 下标 `i` 对应合成文件的第 `i + 1` 行（第 1 行是 `package` 子句 ⇒ `None`）。
    lines: Vec<Option<SynthRole>>,
}

/// 跑 `--cmp`：读指令 → 合成 → 装载 → 逐条判定 → 规范行。
///
/// `Err` ⇒ 退出码 2（读不到 / 指令格式错 / fixture 不合法 / 样本违规）。
/// **`reject` 不是错误**：它是 `D-M3-13` 的边界，退出码仍是 0。
pub fn run_cmp(cmd: &super::cli::TypesCommand) -> Result<TypesOutcome, String> {
    // ① 读文件。`rgoc_load` 不读文件（官方 `go/types` 也不读）⇒ 读是调用方的活，
    //    「读不到」直接报错，**不**伪装成语言层面的诊断。
    let mut texts: Vec<(PathBuf, String)> = Vec::with_capacity(cmd.files.len());
    for f in &cmd.files {
        let text =
            std::fs::read_to_string(f).map_err(|e| format!("读 {} 失败：{e}", f.display()))?;
        texts.push((f.clone(), text));
    }

    // ② 指令（顺便用词法器证明 fixture 的词法层面是干净的）。
    let mut entries: Vec<CmpEntry> = Vec::new();
    for (p, _) in &texts {
        entries.extend(read_directives(p)?);
    }
    if entries.is_empty() {
        return Err("fixture 里没有 `//rgoc:cmp` 指令".into());
    }

    let names: Vec<String> = texts.iter().map(|(p, _)| p.display().to_string()).collect();
    let srcs: Vec<Source<'_>> = names
        .iter()
        .zip(texts.iter())
        .map(|(n, (_, t))| (n.as_str(), t.as_bytes()))
        .collect();

    // ③ 第一趟装载：只要**包名**，顺带断言 fixture 本身合法（合成声明要进同一个包）。
    let mut tt0 = TypeTable::new();
    let (pkg0, bag0) = load_sources(&mut tt0, &srcs, cmd.lang.as_deref());
    let diags0 = bag0.into_sorted();
    if let Some(d) = diags0.first() {
        return Err(format!(
            "fixture 必须合法，但装载报了 {} 条诊断（首条：{}: {} [{}]）",
            diags0.len(),
            pkg0.file_table.format_pos(d.pos),
            d.message,
            d.code.as_str()
        ));
    }
    if pkg0.name.is_empty() {
        return Err("fixture 没有可用的包名（全为 `_`？）".into());
    }

    // ④ 合成 + 第二趟装载。
    let synth = synth_source(&pkg0.name, &entries);
    let mut all: Vec<Source<'_>> = srcs.clone();
    all.push((SYN_NAME, synth.src.as_bytes()));
    let mut tt = TypeTable::new();
    let (pkg, bag) = load_sources(&mut tt, &all, cmd.lang.as_deref());

    // ⑤ 诊断归位：合成文件上的 ⇒ 样本违规（指名道姓）；其余 ⇒ fixture 的问题
    //    （③ 已经查过，这里是防御 —— 宁可报错也不要带着半个表去判定）。
    //
    //    只取**首条**：下面每条分支都是 `Err`，等价于「有诊断就报错」。写成遍历
    //    反而让 clippy 的 `never_loop` 有话说（循环体永远不回到下一轮）。
    let diags = bag.into_sorted();
    if let Some(d) = diags.first() {
        if pkg.file_table.name(d.pos.file) != SYN_NAME {
            return Err(format!(
                "fixture 诊断：{}: {} [{}]",
                pkg.file_table.format_pos(d.pos),
                d.message,
                d.code.as_str()
            ));
        }
        let idx = (d.pos.line as usize).checked_sub(1);
        return match idx.and_then(|i| synth.lines.get(i)).cloned().flatten() {
            Some(role) => {
                let e = &entries[role.entry];
                Err(format!(
                    "第 {} 行：`{}` 不是{}（合成文件第 {} 行：{} [{}]）",
                    e.line,
                    role.text,
                    role.noun,
                    d.pos.line,
                    d.message,
                    d.code.as_str()
                ))
            }
            None => Err(format!(
                "{SYN_NAME}:{}: {} [{}]",
                d.pos.line,
                d.message,
                d.code.as_str()
            )),
        };
    }

    // ⑥ 判定 + 渲染。
    let mut stdout = String::new();
    let _ = writeln!(stdout, "# source: rgoc");
    let _ = writeln!(stdout, "# files: {}", names.join(" "));
    let _ = writeln!(stdout, "# package: {}", pkg.name);
    let _ = writeln!(stdout, "# cmp");
    for e in &entries {
        let n = format!("__rgoc_cmp_{}", e.line);
        let kind = e.kind.as_str();
        match e.kind {
            CmpKind::Identical => {
                let ta = synth_type(&pkg, &format!("{n}_a"), e.line, &e.a)?;
                let tb = synth_type(&pkg, &format!("{n}_b"), e.line, &e.b)?;
                write_result(&mut stdout, e, kind, identical(&tt, ta, tb));
            }
            CmpKind::Representable => {
                let val = object_const(&pkg, &format!("{n}_c"), e.line, &e.a)?;
                let t = synth_type(&pkg, &format!("{n}_t"), e.line, &e.b)?;
                let basic = basic_of(&tt, t).ok_or_else(|| {
                    format!(
                        "第 {} 行：`{}` 不是基本类型（本体也不是）—— M3 只对基本类型给可表示性判定",
                        e.line, e.b
                    )
                })?;
                let ok = representable(&val, basic, &Sizes::HOST_ARM64).is_ok();
                let _ = writeln!(stdout, "{}\t{kind}\t{}", e.line, ok);
            }
            CmpKind::Assignable => {
                let v = synth_type(&pkg, &format!("{n}_a"), e.line, &e.a)?;
                let t = synth_type(&pkg, &format!("{n}_b"), e.line, &e.b)?;
                write_result(&mut stdout, e, kind, assignable(&tt, v, t));
            }
            CmpKind::Implements | CmpKind::Satisfies => {
                let v = synth_type(&pkg, &format!("{n}_a"), e.line, &e.a)?;
                let target = synth_type(&pkg, &format!("{n}_b"), e.line, &e.b)?;
                // `satisfies` = 官方 `types.Satisfies` = 约束面（`constraint = true`）；
                // `implements` 只看方法集。
                let constraint = e.kind == CmpKind::Satisfies;
                write_result(&mut stdout, e, kind, implements(&tt, v, target, constraint));
            }
            CmpKind::MethodSet => {
                let t = synth_type(&pkg, &format!("{n}_a"), e.line, &e.a)?;
                match methodset(&tt, t) {
                    Ok(list) => {
                        // 结果按名字升序（`methodset` 的实现已排）；空集写 `-`。
                        let names: Vec<&str> = list.iter().map(|m| m.name.as_str()).collect();
                        let res = if names.is_empty() {
                            "-".to_string()
                        } else {
                            names.join(",")
                        };
                        let _ = writeln!(stdout, "{}\t{kind}\t{res}", e.line);
                    }
                    Err(code) => write_reject(&mut stdout, e, kind, code),
                }
            }
            CmpKind::Instance => {
                // F 必须是**本包声明的泛型具名类型的名字**：不能合成
                // `type __x_f = F`（泛型类型不实例化不能当类型用）。
                let obj = pkg.lookup(&e.a).ok_or_else(|| {
                    format!(
                        "第 {} 行：`{}` 不是本包声明的类型名 —— instance 的 F 必须是泛型具名类型的名字",
                        e.line, e.a
                    )
                })?;
                let ft = obj.typ.ok_or_else(|| {
                    format!("第 {} 行：`{}` 没有类型（装载器给了占位对象）", e.line, e.a)
                })?;
                let origin = match tt.type_of(unalias(&tt, ft)) {
                    Type::Named(id) => *id,
                    _ => {
                        return Err(format!(
                            "第 {} 行：`{}` 不是具名类型 —— instance 的 F 必须是泛型具名类型",
                            e.line, e.a
                        ));
                    }
                };
                if tt.named_def(origin).type_params.is_empty() {
                    return Err(format!(
                        "第 {} 行：`{}` 不是泛型类型（没有类型形参）",
                        e.line, e.a
                    ));
                }
                let mut args = Vec::with_capacity(e.args.len());
                for (k, text) in e.args.iter().enumerate() {
                    // 与 oracle 侧 `types.Unalias(ta)` 同口径：实参是**指代的类型**，
                    // 合成别名那一层不进实例体（否则 dump 里会多一个 alias 条目）。
                    let t = synth_type(&pkg, &format!("{n}_a{k}"), e.line, text)?;
                    args.push(unalias(&tt, t));
                }
                match instantiate(&mut tt, origin, &args) {
                    Ok(inst) => {
                        let block = try_dump_types(&tt, &[inst]).map_err(|err| {
                            format!("第 {} 行：实例化结果的 dump 失败：{err}", e.line)
                        })?;
                        // `try_dump_types` 自带 `# source: rgoc` 头（C7 §2 规则 1）。
                        // 条目块里**不留**它：oracle 的 `dumpRoot` 也不产头 —— 两侧
                        // 的头部只出现在整份输出开头（`# source: rgoc` vs `# source:
                        // oracle`，是刻意保留的唯一一处不同）。
                        let body = block.strip_prefix("# source: rgoc\n").unwrap_or(&block);
                        let _ = writeln!(stdout, "{}\t{kind}\tok", e.line);
                        for ln in body.trim_end_matches('\n').split('\n') {
                            let _ = writeln!(stdout, "\t{ln}");
                        }
                    }
                    Err(code) => {
                        // 类别本身就是判定结果（两侧逐字一致）；**不**走白名单。
                        let cat = category_of(code);
                        let _ = writeln!(stdout, "{}\t{kind}\treject:{}", e.line, cat.as_str());
                    }
                }
            }
        }
    }

    Ok(TypesOutcome {
        stdout,
        stderr: String::new(),
        errors: 0,
    })
}

/// 二元判定的渲染：`Ok` ⇒ `true/false`；`Err` ⇒ `reject` + `# reject` 理由行。
fn write_result(out: &mut String, e: &CmpEntry, kind: &str, res: Result<bool, rgoc_lex::DiagCode>) {
    match res {
        Ok(v) => {
            let _ = writeln!(out, "{}\t{kind}\t{}", e.line, v);
        }
        Err(code) => write_reject(out, e, kind, code),
    }
}

/// `reject` 条目：判定行 + 理由行（白名单断言读 `# reject` 的代号）。
fn write_reject(out: &mut String, e: &CmpEntry, kind: &str, code: rgoc_lex::DiagCode) {
    let _ = writeln!(out, "{}\t{kind}\treject", e.line);
    let _ = writeln!(out, "# reject {} {}", e.line, code.as_str());
}

/// 读一个文件的 `//rgoc:cmp` 指令。
///
/// 用词法器的 `Mode::COMMENTS`（**不是**自己按行切字符串）：`//` 与 `/* */`
/// 的边界、以及「注释起点」的坐标都由词法器给，和 M1 的 oracle 同一条口径。
/// 指令**格式错要响亮地炸** —— 静默跳过会让样本悄悄消失。
fn read_directives(path: &Path) -> Result<Vec<CmpEntry>, String> {
    let mut lx = Lexer::from_path(path, Mode::COMMENTS)
        .map_err(|e| format!("读 {} 失败：{e}", path.display()))?;
    while !matches!(lx.next().kind, TokenKind::Eof) {}
    if let Some(d) = lx.take_diags().into_sorted().first() {
        return Err(format!(
            "{}:{}: 词法诊断 {} [{}]",
            path.display(),
            d.pos.line,
            d.message,
            d.code.as_str()
        ));
    }

    let mut out = Vec::new();
    for m in lx.take_messages() {
        let text = std::str::from_utf8(&m.text)
            .map_err(|_| format!("{}:{}: 注释不是 UTF-8", path.display(), m.pos.line))?;
        let Some(rest) = text.strip_prefix("//rgoc:cmp") else {
            continue;
        };
        let err = |what: String| format!("{}:{}: {what}", path.display(), m.pos.line);
        let Some(rest) = rest.strip_prefix(' ') else {
            return Err(err("`//rgoc:cmp` 后面要跟一个空格".into()));
        };
        let (kind_str, operands) = rest
            .split_once(' ')
            .ok_or_else(|| err("cmp 指令缺少操作数".into()))?;
        let kind = match kind_str {
            "identical" => CmpKind::Identical,
            "representable" => CmpKind::Representable,
            "assignable" => CmpKind::Assignable,
            "implements" => CmpKind::Implements,
            "satisfies" => CmpKind::Satisfies,
            "methodset" => CmpKind::MethodSet,
            "instance" => CmpKind::Instance,
            other => {
                // `T-TY-82`（M4 改写）：kind 集合扩到七个（`R-32-2`），**封闭性
                // 不变** —— 拼错的名字仍走这个稳定码。
                return Err(err(format!(
                    "未知的 cmp kind {other:?}（支持 {SUPPORTED_KINDS}）[{KIND_UNSUPPORTED}]"
                )));
            }
        };
        let (a, b, args) = match kind {
            CmpKind::MethodSet => {
                // 一元：`methodset ` 之后整段原文都是类型表达式。
                let t = operands.trim();
                if t.is_empty() {
                    return Err(err("cmp 指令缺少操作数".into()));
                }
                if t.contains("=>") {
                    return Err(err("methodset 是一元 kind，不接受 `=>`".into()));
                }
                (t.to_string(), String::new(), Vec::new())
            }
            CmpKind::Instance => {
                let (a, b) = split_operands(operands).map_err(err)?;
                let args = split_arg_list(b).map_err(err)?;
                (a.to_string(), b.to_string(), args)
            }
            _ => {
                let (a, b) = split_operands(operands).map_err(err)?;
                (a.to_string(), b.to_string(), Vec::new())
            }
        };
        out.push(CmpEntry {
            line: m.pos.line,
            kind,
            a,
            b,
            args,
        });
    }
    Ok(out)
}

/// 把 kind 之后的原文切成两个操作数（与 oracle 侧 `splitOperands` **同一条判据**）。
///
/// 两种拼写都认：
///
/// ```text
/// //rgoc:cmp identical <A> => <B>     ← 操作数里带空格时**只能**用这条
/// //rgoc:cmp identical <A> <B>        ← `M3-tests.md` §5.2 冻结的示意形态
/// ```
///
/// 判据：**先看有没有 `=>`**。有就按它切（`=>` 不是任何合法的 Go 类型表达式或
/// 常量表达式的子串，不会与操作数混淆）；没有就按空白切成**恰好两段**。
/// 两侧必须**逐字**一致 —— 不一致的话，同一条指令会在两边解析成不同的操作数，
/// 对撞出来的差异会指向一个根本不存在的类型错误。
fn split_operands(s: &str) -> Result<(&str, &str), String> {
    if s.contains("=>") {
        let mut parts = s.split("=>");
        let a = parts.next().unwrap_or("").trim();
        let b = parts.next().unwrap_or("").trim();
        if parts.next().is_some() {
            return Err("cmp 指令只能有一个 `=>`".into());
        }
        if a.is_empty() || b.is_empty() {
            return Err("cmp 指令有空的操作数".into());
        }
        return Ok((a, b));
    }
    let fields: Vec<&str> = s.split_whitespace().collect();
    if fields.len() != 2 {
        return Err(format!(
            "cmp 指令要有一个 `=>`，或**恰好两个**空白分隔的操作数（实际 {} 个）",
            fields.len()
        ));
    }
    Ok((fields[0], fields[1]))
}

/// 把 `instance` 右侧的**实参列表**按顶层逗号切开（与 oracle 侧 `splitArgList`
/// **同一条判据**）：只按括号/方括号/花括号的嵌套深度与引号状态切 ——
/// `map[string]int, func(int, string) bool` 是两条实参，不是四条。
fn split_arg_list(s: &str) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    let mut depth: i32 = 0;
    let mut start = 0usize;
    let mut quote: Option<char> = None;
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        if let Some(q) = quote {
            if q == '"' && c == b'\\' {
                i += 2; // 跳过转义的下一个字节（类型表达式里只有 struct tag 可能出现）
                continue;
            }
            if c as char == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match c {
            b'"' | b'`' => quote = Some(c as char),
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth -= 1;
                if depth < 0 {
                    return Err(format!("实参列表的括号不配对：{s:?}"));
                }
            }
            b',' if depth == 0 => {
                out.push(s[start..i].trim().to_string());
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if quote.is_some() || depth != 0 {
        return Err(format!("实参列表的括号/引号不配对：{s:?}"));
    }
    out.push(s[start..].trim().to_string());
    if let Some(k) = out.iter().position(|a| a.is_empty()) {
        return Err(format!("实参列表里有空实参（第 {} 条）：{s:?}", k + 1));
    }
    Ok(out)
}

/// 合成源：每条指令的声明**各占一行**（诊断按行归位）。
///
/// 与 oracle 侧一样从 `package` 子句起（行号从 1 起）—— 两侧的行号口径必须
/// 一样，否则「第 N 行」在两边指的不是同一处。
fn synth_source(pkg_name: &str, entries: &[CmpEntry]) -> Synth {
    let mut src = String::new();
    let mut lines: Vec<Option<SynthRole>> = Vec::new();
    let _ = writeln!(src, "package {pkg_name}");
    lines.push(None); // 第 1 行：package 子句

    let role = |entry: usize, text: &str, noun: &'static str| SynthRole {
        entry,
        text: text.to_string(),
        noun,
    };
    let push = |src: &mut String,
                lines: &mut Vec<Option<SynthRole>>,
                text: String,
                r: Option<SynthRole>| {
        src.push_str(&text);
        src.push('\n');
        lines.push(r);
    };

    const TYP: &str = "类型表达式";
    for (i, e) in entries.iter().enumerate() {
        let n = format!("__rgoc_cmp_{}", e.line);
        match e.kind {
            CmpKind::Identical => {
                push(
                    &mut src,
                    &mut lines,
                    format!("type {n}_a = {}", e.a),
                    Some(role(i, &e.a, TYP)),
                );
                push(
                    &mut src,
                    &mut lines,
                    format!("type {n}_b = {}", e.b),
                    Some(role(i, &e.b, TYP)),
                );
            }
            CmpKind::Representable => {
                push(
                    &mut src,
                    &mut lines,
                    format!("const {n}_c = {}", e.a),
                    Some(role(i, &e.a, "常量表达式")),
                );
                push(
                    &mut src,
                    &mut lines,
                    format!("type {n}_t = {}", e.b),
                    Some(role(i, &e.b, TYP)),
                );
                // 判定行：这里的诊断是**结果**（false），不是样本违规 ⇒ 角色不会被渲染。
                push(
                    &mut src,
                    &mut lines,
                    format!("var {n}_v {n}_t = {n}_c"),
                    Some(role(i, &e.b, TYP)),
                );
            }
            CmpKind::Assignable | CmpKind::Implements | CmpKind::Satisfies => {
                push(
                    &mut src,
                    &mut lines,
                    format!("type {n}_a = {}", e.a),
                    Some(role(i, &e.a, TYP)),
                );
                push(
                    &mut src,
                    &mut lines,
                    format!("type {n}_b = {}", e.b),
                    Some(role(i, &e.b, TYP)),
                );
            }
            CmpKind::MethodSet => {
                push(
                    &mut src,
                    &mut lines,
                    format!("type {n}_a = {}", e.a),
                    Some(role(i, &e.a, TYP)),
                );
            }
            CmpKind::Instance => {
                // F 不合成（泛型类型不实例化不能当类型用）⇒ 按包作用域解析。
                for (k, arg) in e.args.iter().enumerate() {
                    push(
                        &mut src,
                        &mut lines,
                        format!("type {n}_a{k} = {arg}"),
                        Some(role(i, arg, TYP)),
                    );
                }
            }
        }
    }
    Synth { src, lines }
}

/// 取合成别名声明的类型（`type __x = <A>` ⇒ 对象类型是 `alias`）。
fn synth_type(pkg: &Package, name: &str, line: u32, text: &str) -> Result<TypeId, String> {
    let obj = pkg.lookup(name).ok_or_else(|| {
        format!("第 {line} 行：合成声明 {name} 不在包作用域里（`{text}` 没能成为类型？）")
    })?;
    obj.typ.ok_or_else(|| {
        format!("第 {line} 行：`{text}` 没有类型（装载器给了占位对象）—— 不是类型表达式")
    })
}

/// 取合成常量声明的**值**。
fn object_const(pkg: &Package, name: &str, line: u32, text: &str) -> Result<Const, String> {
    let obj = pkg.lookup(name).ok_or_else(|| {
        format!("第 {line} 行：合成声明 {name} 不在包作用域里（`{text}` 没能成为常量？）")
    })?;
    match obj.val.clone() {
        // `Unknown` 是「值没算出来」——直接拿去判可表示性会**默认返回 true**
        // （官方 `representableConst` 的第一条），那是把「不知道」当「行」。
        // 宁可报错：样本契约要求 C 是常量表达式。
        Some(Const::Unknown) | None => Err(format!(
            "第 {line} 行：`{text}` 不是常量表达式（装载器没算出值）"
        )),
        Some(v) => Ok(v),
    }
}

/// 目标类型的**基本类型**：别名与具名逐层穿透（官方 `Underlying().(*Basic)`）。
///
/// `seen` 是环卫：`type A B; type B A` 在报环之前本体互指，不能跟着绕。
fn basic_of(tt: &TypeTable, t: TypeId) -> Option<BasicId> {
    let mut seen: Vec<rgoc_types::NamedId> = Vec::new();
    let mut cur = unalias(tt, t);
    loop {
        match tt.type_of(cur) {
            Type::Basic(b) => return Some(*b),
            Type::Named(n) => {
                if seen.contains(n) {
                    return None;
                }
                seen.push(*n);
                cur = tt.named_underlying(*n);
            }
            _ => return None,
        }
    }
}
