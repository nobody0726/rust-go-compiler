//! SSA 的**中立 dump**（格式由 `C10-ssa-dump.md` 冻结）。
//!
//! # 六条规则怎么落在这里
//!
//! 1. 头三行固定（版本 / stage / `# func`）；`#` 行**不参与比对**（沿 `C7` 口径）。
//! 2. 块段按 `BlockId` 升序；`params`/`preds`/`succs` 三个字段**必须都出现**，
//!    空写 `[]`；`preds`/`succs` 由 [`crate::cfg::Cfg`] **现算**（IR 里不存）。
//! 3. 指令行 `v<k> = <op> <operands> : t<id>`，块内按程序顺序。
//! 4. 终止符行前缀 `term`；边上的实参**必须出现**（空写 `()`）——它是 `V7` 的比对面。
//! 5. 编号一律按创建顺序、升序输出；类型在末尾统一映射。
//! 6. 禁止地址 / 时间 / 路径 / 哈希序：所有容器是 `Vec` 或 `BTreeMap`，
//!    本文件里**不出现** `HashMap`（`T-SSA-13` 的机械保证）。
//!
//! # 三处「格式没规定、由实现定」的细节（已在 `C10` §2 的修订行 `R-16-1` 登记）
//!
//! * `load`/`store` 的**第一个操作数是 mem 令牌**（`V6` 的比对面）——
//!   不写它就等于 dump 里看不见 mem 链，`V6` 报错时无从对照。
//! * 可选操作数（`Slice` 的三索引、`Make` 的 `cap`）缺省写 `_`（不是省略、不是空串）。
//! * `Effect` **不进 dump**：它是「可能写哪些根」的摘要，属于 pass 的输入而不是
//!   指令的操作数；放进指令行会让同一张图因为摘要不同而 dump 不同（那是优化信息，不是 IR 形状）。

use rgoc_hir::types::ConstVal;
use rgoc_hir::{BinOp, BuiltinKind, UnOp};
use rgoc_types::{BasicId, Type, TypeId, TypeTable};

use crate::cfg::Cfg;
use crate::ir::{
    BlockId, CalleeRef, Instr, Module, Root, RootId, SsaFunc, Term, ValueId, kind_of_callee,
    kind_of_root,
};

/// dump 的版本行（`C10` 规则 1；改动等于契约变更）。
pub const DUMP_VERSION: &str = "# SSA v1";

/// 把一个模块渲染成中立 dump。
///
/// 结构：
///
/// ```text
/// # SSA v1
/// # stage: ssa
/// # func <name> blocks=<n> values=<n>
/// block b0 params=[] preds=[] succs=[b1]
///   v0 = const 1 : t1
///   term goto b1 ()
/// # root r0 = heap
/// # type t0 = bool
/// # type t1 = int
/// ```
#[must_use]
pub fn dump_module(m: &Module, tt: &TypeTable) -> String {
    let mut out = String::new();
    out.push_str(DUMP_VERSION);
    out.push('\n');
    out.push_str("# stage: ssa\n");
    let mut types: Vec<TypeId> = Vec::new();
    for f in &m.funcs {
        render_func(&mut out, f, tt);
        for &t in &f.value_types {
            if !types.contains(&t) {
                types.push(t);
            }
        }
    }
    types.sort_unstable();
    for t in types {
        out.push_str(&format!("# type t{} = {}\n", t.index(), render_type(tt, t)));
    }
    out
}

/// 渲染单个函数（`# func` 行 + 块段 + 根映射）。
#[must_use]
pub fn dump_func(f: &SsaFunc, tt: &TypeTable) -> String {
    let mut out = String::new();
    render_func(&mut out, f, tt);
    let mut types: Vec<TypeId> = f.value_types.clone();
    types.sort_unstable();
    types.dedup();
    for t in types {
        out.push_str(&format!("# type t{} = {}\n", t.index(), render_type(tt, t)));
    }
    out
}

fn render_func(out: &mut String, f: &SsaFunc, tt: &TypeTable) {
    out.push_str(&format!(
        "# func {} blocks={} values={}\n",
        f.name,
        f.block_count(),
        f.value_count()
    ));
    let cfg = Cfg::build(f);
    for (i, blk) in f.blocks.iter().enumerate() {
        let b = BlockId(i as u32);
        out.push_str(&format!(
            "block b{i} params={} preds={} succs={}\n",
            vlist(&blk.params),
            bs(cfg.preds(b)),
            bs(cfg.succs(b))
        ));
        for ins in &blk.instrs {
            out.push_str("  ");
            out.push_str(&render_instr(ins));
            out.push('\n');
        }
        out.push_str("  ");
        out.push_str(&render_term(&blk.term));
        out.push('\n');
    }
    for (ri, r) in f.roots.iter().enumerate() {
        out.push_str(&format!("# root r{ri} = {}\n", render_root(r)));
    }
    let _ = tt;
}

fn render_instr(i: &Instr) -> String {
    match i {
        Instr::Const { dst, c, typ } => {
            format!("{} = const {} : {}", v(*dst), literal(c), t(*typ))
        }
        Instr::Copy { dst, src } => format!("{} = copy {}", v(*dst), v(*src)),
        Instr::Bin {
            dst,
            op,
            lhs,
            rhs,
            typ,
        } => format!(
            "{} = bin {} {} {} : {}",
            v(*dst),
            bin_name(*op),
            v(*lhs),
            v(*rhs),
            t(*typ)
        ),
        Instr::Un { dst, op, x, typ } => {
            format!("{} = un {} {} : {}", v(*dst), un_name(*op), v(*x), t(*typ))
        }
        Instr::Convert { dst, x, typ } => format!("{} = convert {} : {}", v(*dst), v(*x), t(*typ)),
        Instr::AddrOf { dst, root } => format!("{} = addr-of {}", v(*dst), r(*root)),
        Instr::Field { dst, base, index } => {
            format!("{} = field {} {index}", v(*dst), v(*base))
        }
        Instr::Index { dst, base, i } => format!("{} = index {} {}", v(*dst), v(*base), v(*i)),
        Instr::Slice {
            dst,
            base,
            lo,
            hi,
            max,
        } => format!(
            "{} = slice {} {} {} {}",
            v(*dst),
            v(*base),
            ov(*lo),
            ov(*hi),
            ov(*max)
        ),
        Instr::Composite { dst, typ, items } => {
            format!("{} = composite ({}) : {}", v(*dst), vs(items), t(*typ))
        }
        Instr::Make { dst, typ, len, cap } => {
            format!("{} = make {} {} : {}", v(*dst), ov(*len), ov(*cap), t(*typ))
        }
        Instr::Load {
            dst,
            mem,
            root,
            addr,
            typ,
        } => format!(
            "{} = load {} {} {} : {}",
            v(*dst),
            r(*root),
            v(*mem),
            v(*addr),
            t(*typ)
        ),
        Instr::Store {
            dst,
            mem,
            root,
            addr,
            val,
        } => format!(
            "{} = store {} {} {} {}",
            v(*dst),
            r(*root),
            v(*mem),
            v(*addr),
            v(*val)
        ),
        Instr::Go { callee, args, .. } => {
            format!("go {}({})", render_callee(callee), vs(args))
        }
        Instr::Call {
            dsts,
            callee,
            args,
            dots,
            ..
        } => format!(
            "{} = call {}({}){}",
            vs(dsts),
            render_callee(callee),
            vs(args),
            // `...` 是**语义**（展开 vs 单个），不是排版（`R-35-1`）
            if *dots { " ..." } else { "" }
        ),
        // `C10` §3（M9/Task 37 追加）：`v3 = funcaddr func#2`
        Instr::FuncAddr { dst, func } => format!("{} = funcaddr func#{}", v(*dst), func.0),
        // `C10` §3（M9/Task 38 追加）：`v3 = closure func#2 (v1, v2)`
        Instr::MakeClosure {
            dst,
            func,
            captures,
        } => format!(
            "{} = closure func#{} ({})",
            v(*dst),
            func.0,
            captures
                .iter()
                .map(|c| v(*c))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        // `C10` §3（M9/Task 38 追加）：`v3 = capture#1`
        Instr::CapturePtr { dst, index } => format!("{} = capture#{index}", v(*dst)),
        // `C10` §3（M9/Phase 5 追加）：`defer v3`
        Instr::Defer { fv } => format!("defer {}", v(*fv)),
        // `C10` §3（M9/Phase 5c 追加）：`nil-check v3`
        Instr::NilCheck { addr } => format!("nil-check {}", v(*addr)),
        // `C10` §3（M9/Task 33 追加）：`v3 = assert v1 : t5` / `v3, v4 = assert v1 : t5`
        Instr::TypeAssert { dst, ok, x, typ } => match ok {
            Some(o) => format!("{}, {} = assert {} : {}", v(*dst), v(*o), v(*x), t(*typ)),
            None => format!("{} = assert {} : {}", v(*dst), v(*x), t(*typ)),
        },
    }
}

fn render_callee(c: &CalleeRef) -> String {
    match c {
        CalleeRef::Func(f) => format!("func#{}", f.0),
        CalleeRef::Method { recv, func } => format!("method({}, func#{})", v(*recv), func.0),
        CalleeRef::Builtin(b) => format!("builtin {}", builtin_name(*b)),
        // 运行期助手：`call runtime map_get(...)` —— 名字直接给 `rt.c` 的拼写
        // （不含 `rt_` 前缀，与 `builtin` 的写法同构）。
        CalleeRef::Runtime(f) => format!("runtime {}", f.as_str()),
        CalleeRef::Indirect(x) => format!("indirect {}", v(*x)),
        // `C10` §3（M9/Task 32 追加）：`call iface(v1, slot#2)(...)`
        CalleeRef::Iface { recv, slot } => format!("iface({}, slot#{})", v(*recv), slot),
    }
}

fn render_term(tm: &Term) -> String {
    match tm {
        Term::Goto { to, args } => format!("term goto b{} ({})", to.0, vs(args)),
        Term::If { cond, then_, else_ } => format!(
            "term if {} -> b{} ({}) | b{} ({})",
            v(*cond),
            then_.0.0,
            vs(&then_.1),
            else_.0.0,
            vs(&else_.1)
        ),
        Term::Return(vs_) => format!("term return ({})", vs(vs_)),
        Term::Panic(x) => format!("term panic ({})", v(*x)),
        Term::Unreachable => "term unreachable".to_string(),
    }
}

fn render_root(r: &Root) -> String {
    match r {
        Root::Global(g) => format!("{} g{}", kind_of_root(r), g.0),
        Root::Slot(s) => format!("{} s{}", kind_of_root(r), s.0),
        Root::Heap => kind_of_root(r).to_string(),
    }
}

fn v(x: ValueId) -> String {
    format!("v{}", x.0)
}

fn vs(xs: &[ValueId]) -> String {
    xs.iter().map(|x| v(*x)).collect::<Vec<_>>().join(",")
}

fn ov(x: Option<ValueId>) -> String {
    x.map_or_else(|| "_".to_string(), v)
}

fn vlist(xs: &[ValueId]) -> String {
    format!("[{}]", vs(xs))
}

fn bs(xs: &[BlockId]) -> String {
    format!(
        "[{}]",
        xs.iter()
            .map(|b| format!("b{}", b.0))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn r(x: RootId) -> String {
    format!("r{}", x.0)
}

fn t(x: TypeId) -> String {
    format!("t{}", x.index())
}

fn literal(c: &ConstVal) -> String {
    match c {
        ConstVal::Int(n) => n.to_string(),
        ConstVal::Float(x) => format!("{x}"),
        ConstVal::Complex(re, im) => format!("complex({re},{im})"),
        ConstVal::Str(s) => format!("{s:?}"),
        ConstVal::Bool(b) => b.to_string(),
    }
}

/// `BinOp` 的 dump 名（**不带通配的 `match`**）。
fn bin_name(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "add",
        BinOp::Sub => "sub",
        BinOp::Mul => "mul",
        BinOp::Div => "div",
        BinOp::Rem => "rem",
        BinOp::And => "and",
        BinOp::Or => "or",
        BinOp::Xor => "xor",
        BinOp::AndNot => "and-not",
        BinOp::Shl => "shl",
        BinOp::Shr => "shr",
        BinOp::Eq => "eq",
        BinOp::Ne => "ne",
        BinOp::Lt => "lt",
        BinOp::Le => "le",
        BinOp::Gt => "gt",
        BinOp::Ge => "ge",
        BinOp::LAnd => "land",
        BinOp::LOr => "lor",
    }
}

/// `UnOp` 的 dump 名。
fn un_name(op: UnOp) -> &'static str {
    match op {
        UnOp::Neg => "neg",
        UnOp::Pos => "pos",
        UnOp::Not => "not",
        UnOp::BitNot => "bit-not",
        UnOp::Recv => "recv",
    }
}

/// 内建名（与 `rgoc-interp` 的 `print` 名字一致；`C10` 只要求稳定）。
fn builtin_name(b: BuiltinKind) -> &'static str {
    match b {
        BuiltinKind::Print => "print",
        BuiltinKind::Println => "println",
        BuiltinKind::Len => "len",
        BuiltinKind::Cap => "cap",
        BuiltinKind::Append => "append",
        BuiltinKind::Copy => "copy",
        BuiltinKind::Delete => "delete",
        BuiltinKind::New => "new",
        BuiltinKind::Complex => "complex",
        BuiltinKind::Real => "real",
        BuiltinKind::Imag => "imag",
        BuiltinKind::Panic => "panic",
        BuiltinKind::Recover => "recover",
        BuiltinKind::Close => "close",
        BuiltinKind::Min => "min",
        BuiltinKind::Max => "max",
        BuiltinKind::Clear => "clear",
        BuiltinKind::ReflectCall => "rt_reflect_call",
        BuiltinKind::ReflectAlloc => "rt_reflect_alloc",
    }
}

/// 单类型渲染（**只给人看**：`#` 行不参与比对，`C10` 规则 1）。
///
/// 覆盖常用形态；`struct`/`interface`/`signature` 这类要回表展开的，写它们的
/// 编号形式（`struct#3`）——完整渲染是 `C7` 的职责，这里复制一份只会漂。
fn render_type(tt: &TypeTable, id: TypeId) -> String {
    if id.index() >= tt.len() {
        return format!("<越界 t{}>", id.index());
    }
    match tt.type_of(id) {
        Type::Basic(b) => basic_name(*b).to_string(),
        Type::Named(n) => format!("named#{}", n.index()),
        Type::Alias(a) => format!("alias#{}", a.index()),
        Type::Pointer(e) => format!("*{}", render_type(tt, *e)),
        Type::Slice(e) => format!("[]{}", render_type(tt, *e)),
        Type::Array { len, elem } => format!(
            "[{}]{}",
            len.map_or("-".to_string(), |c| format!("c{}", c.index())),
            render_type(tt, *elem)
        ),
        Type::Map { key, elem } => {
            format!("map[{}]{}", render_type(tt, *key), render_type(tt, *elem))
        }
        Type::Chan { elem, .. } => format!("chan {}", render_type(tt, *elem)),
        Type::Struct(s) => format!("struct#{}", s.index()),
        Type::Signature(s) => format!("signature#{}", s.index()),
        Type::Tuple(tp) => format!("tuple#{}", tp.index()),
        Type::Interface(i) => format!("interface#{}", i.index()),
        Type::TypeParam(p) => format!("typeparam#{}", p.index()),
        Type::Union(u) => format!("union#{}", u.index()),
        Type::Invalid => "invalid".to_string(),
    }
}

/// 基本类型名（与 Go 源码一致；`byte`/`rune` 折到本体，沿 `rgoc-types` 的口径）。
fn basic_name(b: BasicId) -> &'static str {
    match b {
        BasicId::Bool => "bool",
        BasicId::Int => "int",
        BasicId::Int8 => "int8",
        BasicId::Int16 => "int16",
        BasicId::Int32 => "int32",
        BasicId::Int64 => "int64",
        BasicId::Uint => "uint",
        BasicId::Uint8 => "uint8",
        BasicId::Uint16 => "uint16",
        BasicId::Uint32 => "uint32",
        BasicId::Uint64 => "uint64",
        BasicId::Uintptr => "uintptr",
        BasicId::Float32 => "float32",
        BasicId::Float64 => "float64",
        BasicId::Complex64 => "complex64",
        BasicId::Complex128 => "complex128",
        BasicId::String => "string",
        BasicId::UnsafePointer => "unsafe.Pointer",
        BasicId::UntypedBool => "untyped bool",
        BasicId::UntypedInt => "untyped int",
        BasicId::UntypedRune => "untyped rune",
        BasicId::UntypedFloat => "untyped float",
        BasicId::UntypedComplex => "untyped complex",
        BasicId::UntypedString => "untyped string",
        BasicId::UntypedNil => "untyped nil",
    }
}

/// `kind_of_callee` 在本文件里没有直接用到，但保留导入会触发 unused 警告 ——
/// 这里显式引用一次，把它留给「未来 dump 需要按类别分支」的位置。
#[allow(dead_code)]
fn callee_kind(c: &CalleeRef) -> &'static str {
    kind_of_callee(c)
}
