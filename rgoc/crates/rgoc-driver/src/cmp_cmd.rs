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
//! - `identical` ⇒ [`rgoc_types::identical`]（内部先 `unalias`）。`Err(code)` ⇒
//!   **`reject`**：这是 `D-M3-13` 划出的边界（约束接口 / 类型参数 / 并集），
//!   对撞时必须按白名单排除（`M3-tests.md` §6），**排除集本身也要被断言**。
//! - `representable` ⇒ [`rgoc_const::representable`]，取常量对象的**值**与目标的
//!   基本类型（目标是具名类型时取其**本体**，官方判据就是 `Underlying()` 的
//!   `*Basic`）。`Err(code)` ⇒ `false`（不可表示就是不可表示，**不是**拒绝）；
//!   目标连本体都不是基本类型 ⇒ **响亮报错** —— M3 只对基本类型给这个判定，
//!   样本违规不该伪装成 `reject`（Ruling 27-1 的同一条纪律）。
//!
//! # 来源标注
//!
//! 头几行是 `# source: rgoc` / `# files:` / `# package:` / `# cmp`（C7 §2 规则 1
//! 两侧都忽略 `#` 行）。`reject` 条目的**理由**紧跟本行，形如
//! `# reject <行> <CODE>` —— 同样以 `#` 开头，不进逐字节 diff，但白名单断言读它。

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use rgoc_const::{Const, representable};
use rgoc_lex::{Lexer, Mode, TokenKind};
use rgoc_load::{Package, Source, load_sources};
use rgoc_types::{BasicId, Sizes, Type, TypeId, TypeTable, identical, unalias};

use crate::types_cmd::TypesOutcome;

/// 合成文件在对撞坐标里的名字（与 oracle 侧的 `<rgoc:cmp>` 同位；两者不互相 diff，
/// 各自的诊断都靠它归位）。
const SYN_NAME: &str = "<rgoc:cmp>";

/// `//rgoc:cmp` 的 kind 不是 M3 支持的两个之一时的**稳定拒绝码**（`T-TY-82`）。
///
/// 为什么是驱动层的码、而不是 [`rgoc_lex::DiagCode`]：它说的是「这个**请求**
/// 我们不判」——没有官方对应代号，也不来自某个源码位置。它与 `T-TY-81` 的
/// `TYP-UNSUPPORTED-CONSTRAINT-IDENTITY` 同理不同层：那条是**语言层**的能力边界
/// （判同一性时撞上类型集），这条是**请求层**的能力边界（连问都没法问）。
///
/// 为什么「拼错的 kind」与「M3 不判的判定」共用一个码：语法上不可分
/// （`assignable` / `implements` / `instance` 都只是没见过的名字）。kind 集合
/// **与 oracle 侧共用且封闭**（都只认 `identical` / `representable`）—— 不发明
/// 新 kind 名：fixture 由两侧读**同一份**文件，输入语言分叉比少支持一个 kind 坏。
pub const KIND_UNSUPPORTED: &str = "TYP-CMP-KIND-UNSUPPORTED";

/// 指令种类（`D-M3-17` 的两种）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CmpKind {
    Identical,
    Representable,
}

impl CmpKind {
    fn as_str(self) -> &'static str {
        match self {
            CmpKind::Identical => "identical",
            CmpKind::Representable => "representable",
        }
    }
}

/// 一条 `//rgoc:cmp` 指令。
#[derive(Debug, Clone)]
struct CmpEntry {
    /// 指令所在的**物理**行号（`Pos::line` 是绝对行号，不带 `//line` 重映射）。
    line: u32,
    kind: CmpKind,
    /// 第一个操作数（`identical` 的 A / `representable` 的 C）
    a: String,
    /// 第二个操作数（`identical` 的 B / `representable` 的 T）
    b: String,
}

/// 合成文件里的一行对应哪个操作数（诊断归位用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Operand {
    A,
    B,
    C,
    T,
}

impl Operand {
    /// 这个操作数该被说成什么（错误消息用词）。
    fn noun(self) -> &'static str {
        match self {
            Operand::C => "常量表达式",
            _ => "类型表达式",
        }
    }
}

/// 合成源 + 逐行归属。
struct Synth {
    src: String,
    /// 下标 `i` 对应合成文件的第 `i + 1` 行（第 1 行是 `package` 子句 ⇒ `None`）。
    lines: Vec<Option<(usize, Operand)>>,
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
        return match idx.and_then(|i| synth.lines.get(i)).copied().flatten() {
            Some((i, op)) => {
                let e = &entries[i];
                let text = match op {
                    Operand::B | Operand::T => &e.b,
                    _ => &e.a,
                };
                Err(format!(
                    "第 {} 行：`{}` 不是{}（合成文件第 {} 行：{} [{}]）",
                    e.line,
                    text,
                    op.noun(),
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
                let ta = object_type(&pkg, &format!("{n}_a"), e, Operand::A)?;
                let tb = object_type(&pkg, &format!("{n}_b"), e, Operand::B)?;
                match identical(&tt, ta, tb) {
                    Ok(v) => {
                        let _ = writeln!(stdout, "{}\t{kind}\t{}", e.line, v);
                    }
                    Err(code) => {
                        // **拒绝**：理由必须写出来（白名单断言读它，人读也靠它）
                        let _ = writeln!(stdout, "{}\t{kind}\treject", e.line);
                        let _ = writeln!(stdout, "# reject {} {}", e.line, code.as_str());
                    }
                }
            }
            CmpKind::Representable => {
                let val = object_const(&pkg, &format!("{n}_c"), e)?;
                let t = object_type(&pkg, &format!("{n}_t"), e, Operand::T)?;
                let basic = basic_of(&tt, t).ok_or_else(|| {
                    format!(
                        "第 {} 行：`{}` 不是基本类型（本体也不是）—— M3 只对基本类型给可表示性判定",
                        e.line, e.b
                    )
                })?;
                let ok = representable(&val, basic, &Sizes::HOST_ARM64).is_ok();
                let _ = writeln!(stdout, "{}\t{kind}\t{}", e.line, ok);
            }
        }
    }

    Ok(TypesOutcome {
        stdout,
        stderr: String::new(),
        errors: 0,
    })
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
            other => {
                // `T-TY-82`：请求 `TYP-13/18/19/23..26` 的判定在 M3 的**唯一入口**
                // 就是这个 kind 名。往后 `[…]` 里那份稳定码是给 grep / 断言用的。
                return Err(err(format!(
                    "未知的 cmp kind {other:?}（M3 只判 identical / representable；\
                     TYP-13/18/19/23..26 的判定属 M4，U-M3-01..04）[{KIND_UNSUPPORTED}]"
                )));
            }
        };
        let (a, b) = split_operands(operands).map_err(err)?;
        out.push(CmpEntry {
            line: m.pos.line,
            kind,
            a: a.to_string(),
            b: b.to_string(),
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

/// 合成源：每条指令的声明**各占一行**（诊断按行归位）。
///
/// 与 oracle 侧一样从 `package` 子句起（行号从 1 起）—— 两侧的行号口径必须
/// 一样，否则「第 N 行」在两边指的不是同一处。
fn synth_source(pkg_name: &str, entries: &[CmpEntry]) -> Synth {
    let mut src = String::new();
    let mut lines: Vec<Option<(usize, Operand)>> = Vec::new();
    let _ = writeln!(src, "package {pkg_name}");
    lines.push(None); // 第 1 行：package 子句

    let push = |src: &mut String,
                lines: &mut Vec<Option<(usize, Operand)>>,
                text: String,
                role: Option<(usize, Operand)>| {
        src.push_str(&text);
        src.push('\n');
        lines.push(role);
    };

    for (i, e) in entries.iter().enumerate() {
        let n = format!("__rgoc_cmp_{}", e.line);
        match e.kind {
            CmpKind::Identical => {
                push(
                    &mut src,
                    &mut lines,
                    format!("type {n}_a = {}", e.a),
                    Some((i, Operand::A)),
                );
                push(
                    &mut src,
                    &mut lines,
                    format!("type {n}_b = {}", e.b),
                    Some((i, Operand::B)),
                );
            }
            CmpKind::Representable => {
                push(
                    &mut src,
                    &mut lines,
                    format!("const {n}_c = {}", e.a),
                    Some((i, Operand::C)),
                );
                push(
                    &mut src,
                    &mut lines,
                    format!("type {n}_t = {}", e.b),
                    Some((i, Operand::T)),
                );
            }
        }
    }
    Synth { src, lines }
}

/// 取合成别名声明的类型（`type __x = <A>` ⇒ 对象类型是 `alias`）。
fn object_type(pkg: &Package, name: &str, e: &CmpEntry, op: Operand) -> Result<TypeId, String> {
    let text = match op {
        Operand::B | Operand::T => &e.b,
        _ => &e.a,
    };
    let obj = pkg.lookup(name).ok_or_else(|| {
        format!(
            "第 {} 行：合成声明 {name} 不在包作用域里（`{text}` 没能成为类型？）",
            e.line
        )
    })?;
    obj.typ.ok_or_else(|| {
        format!(
            "第 {} 行：`{text}` 没有类型（装载器给了占位对象）—— 不是类型表达式",
            e.line
        )
    })
}

/// 取合成常量声明的**值**。
fn object_const(pkg: &Package, name: &str, e: &CmpEntry) -> Result<Const, String> {
    let obj = pkg.lookup(name).ok_or_else(|| {
        format!(
            "第 {} 行：合成声明 {name} 不在包作用域里（`{}` 没能成为常量？）",
            e.line, e.a
        )
    })?;
    match obj.val.clone() {
        // `Unknown` 是「值没算出来」——直接拿去判可表示性会**默认返回 true**
        // （官方 `representableConst` 的第一条），那是把「不知道」当「行」。
        // 宁可报错：样本契约要求 C 是常量表达式。
        Some(Const::Unknown) | None => Err(format!(
            "第 {} 行：`{}` 不是常量表达式（装载器没算出值）",
            e.line, e.a
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
