//! HIR → SSA 构建（`M6-design.md` §3.3；`M6-plan.md` Task 17 起）。
//!
//! # 当前进度（Task 17：骨架）
//!
//! 建**每个函数一个 `SsaFunc`**：入口块 + 形参（`Func::params`/`param_types` →
//! 入口块的块参数）+ 空体函数的 `Term::Return`。函数体的语句、表达式、
//! 取址分析、槽提升都还没进来 —— 遇到非空体就 `todo!()`，**不静默产出空函数**。
//!
//! # 为什么非空体用 `todo!()` 而不是「尽力而为」
//!
//! 「尽力而为」在这里等于**语义错**：一个丢掉了语句的函数仍然能过 verifier
//! （它是一张合法的空图），于是 `ssa --verify` 绿、`--via=ssa` 却算出错结果。
//! 项目纪律是「不用 `None` 冒充未实现」—— 这里同理：没实现就响，别静默降级。
//!
//! # `Module::funcs` 的下标 ≠ `FuncId`
//!
//! 被拒绝的函数**不进** `Module::funcs`（拒绝要显式上报，不许塞个空函数顶位）。
//! 所以「第 i 个 SsaFunc 是哪个 HIR 函数」只能看 [`crate::ir::SsaFunc::src`] ——
//! 调用指令里的 `CalleeRef::Func` 也仍然是 **HIR 编号**。

use std::collections::BTreeMap;

use rgoc_hir::{Func, FuncId, GlobalId, Program};
use rgoc_types::{TypeId, TypeTable};

use crate::diag::{Reject, SsaCode};
use crate::ir::{
    Block, BlockId, CalleeRef, Effect, Instr, Module, SYNTHETIC_FUNC, Sig, SsaFunc, Term, ValueId,
};

/// 空白标识符 `_` 的槽（HIR 约定：`_` 用 `SlotId(u32::MAX)` 占位）。
///
/// 两处要用它：**不当定义**（`_ = x` 不产生值）、**不提升**（它没有存储）。
pub const BLANK_SLOT: SlotId = SlotId(u32::MAX);

/// 构建结果：建成的模块 + 被拒绝的函数（**两条通道都要报**，不许只返回一半）。
#[derive(Clone, Debug)]
pub struct BuildOutcome {
    /// 建成的模块（`funcs` 只含建成的函数；顺序 == HIR 顺序去掉被拒的那些）。
    pub module: Module,
    /// 被拒绝的函数（每条带稳定码；`SSA-7` 要求它们出现在 stderr）。
    pub rejected: Vec<Reject>,
}

/// 把一个 HIR 程序建成 SSA 模块。
///
/// # 为什么是 `&mut TypeTable`（修订 `R-20-2`）
///
/// `&x` 要产出 `AddrOf { dst }`，而 `dst` 的静态类型必须是 `*T`。指针类型只能由
/// `TypeTable::add_pointer` **新建** —— 拿一张不可变的表就只能在 dump 里编一个
/// 「假类型」出来，那等于让 IR 里的类型与实际不符。表本来就是包级的、随 build
/// 一起长大，所以这里要 `&mut`。
#[must_use]
pub fn build(program: &Program, tt: &mut TypeTable) -> BuildOutcome {
    // 调用摘要（`Task 27`）：算一次、全函数共用。它只依赖 HIR，与建图进度无关。
    let effs = crate::effect::summarize(program);
    let mut funcs = Vec::with_capacity(program.funcs.len() + 1);
    let mut rejected = Vec::new();
    for (i, f) in program.funcs.iter().enumerate() {
        let id = FuncId(i as u32);
        match build_func(f, id, program, &effs, tt) {
            Ok(sf) => funcs.push(sf),
            Err(r) => rejected.push(r),
        }
    }

    // 包级初始化（`Task 32b`）：**先扫后建**。
    // 扫的理由：`init_order` 的右侧不在任何函数体里，`build_func` 的预扫描看不见它 ——
    // `var m = map[int]int{}` 会一路走进 `todo!()`（panic），而不是带码拒绝。
    let mut init = None;
    if let Some((code, detail)) = unsupported_init(program, tt) {
        rejected.push(Reject {
            func: SYNTHETIC_FUNC,
            code,
            detail,
        });
    } else if let Some(sf) = build_init(program, &effs, tt) {
        init = Some(funcs.len());
        funcs.push(sf);
    }

    BuildOutcome {
        module: Module {
            funcs,
            globals: (0..program.globals.len())
                .map(|i| GlobalId(i as u32))
                .collect(),
            main: program.main,
            init,
        },
        rejected,
    }
}

/// 扫**包级初始化式**（`Program::init_order` 的右侧）里的不支持面。
///
/// 与 `unsupported()` 同一套判据，只是入口不同：那边扫函数体，这边扫初始化式。
/// 报的 `func` 是哨兵 [`SYNTHETIC_FUNC`]，`detail` 里点明「包级初始化式」。
fn unsupported_init(program: &Program, tt: &TypeTable) -> Option<(SsaCode, String)> {
    for (i, g) in program.init_order.iter().enumerate() {
        if let Some((code, detail)) = g.rhs.iter().find_map(|e| unsup_expr(tt, e)) {
            return Some((code, format!("包级初始化式（第 {} 组）：{detail}", i + 1)));
        }
    }
    None
}

/// 合成包初始化函数（`Task 32b`；`Module::init` 指向它）。
///
/// # 顺序与解释器一致
///
/// `interp` 的顶层阶段是 `Globals(0..)` → `Inits(0..)` → `Main`，这里照抄：
/// 先按 `init_order` 逐组求值写全局根，再依次调用各文件的 `init()`。
///
/// # 为什么可以只建一个块
///
/// 初始化式是**表达式**，没有语句、没有分支（Go 规范里包级初始化式不能含控制流）。
/// 所以合成函数永远是单块直线图 —— 不需要块参数、不需要汇合。
fn build_init(
    program: &Program,
    effs: &[crate::effect::Summary],
    tt: &mut TypeTable,
) -> Option<SsaFunc> {
    if program.init_order.is_empty() && program.inits.is_empty() {
        return None;
    }
    // 合成函数在 HIR 里没有对应的 `Func`：造一个空壳（无参无结果无槽）。
    let fake = Func::placeholder("init");
    // 根表 = **全部**全局根（下标 == GlobalId，`RootId(g.0)` 的约定靠它）。
    // 不预先建槽根：初始化式里没有局部变量。
    let roots: Vec<Root> = (0..program.globals.len())
        .map(|i| Root::Global(GlobalId(i as u32)))
        .collect();
    let mut fb = Fb::new(
        tt,
        &fake,
        program,
        effs,
        FbInit {
            roots,
            slot_root: BTreeMap::new(),
            promoted: BTreeSet::new(),
            goto_mode: false,
        },
    );

    for g in &program.init_order {
        // 右侧**全部先求值**再逐个写（与赋值语句同一套语义）
        let vs = fb.rhs_values(&g.rhs, g.globals.len());
        for (gid, v) in g.globals.iter().zip(vs) {
            let root = RootId(gid.0);
            let t = fb.elem_type(root);
            let v = fb.coerce_to(t, v);
            let addr = fb.addr_of(root);
            fb.store_root(root, addr, v);
        }
    }

    for &fid in &program.inits {
        let (rtypes, sig, _) = fb.sig_of(fid, false);
        fb.call_sigs.insert(fid, sig);
        let eff = crate::effect::to_effect(&effs[fid.index()]);
        fb.emit_call(CalleeRef::Func(fid), Vec::new(), &rtypes, eff);
    }

    fb.set_term(Term::Return(Vec::new()));
    Some(fb.finish(SYNTHETIC_FUNC))
}

// ── 拒绝面（Task 31）────────────────────────────────────────────────

/// 这个函数里有没有 M6 面之外的构造？有就给出**第一个**命中。
///
/// # 为什么「先扫一遍」而不是「走到哪拒到哪」
///
/// `expr` / `stmt` 返回 `ValueId` / `()`，不是 `Result`。为了传一条拒绝就把
/// 3.5k 行构建代码全染上 `?`，代价远大于收益；而拒绝判据**只依赖 HIR 的
/// 形状与静态类型**、与建图状态无关，天然是一个独立小 pass（沿 `scan_addressed`）。
///
/// # 为什么只报第一个
///
/// 被拒的函数**整个**不进 `Module::funcs`，列十条也没人多看一眼。
/// 挑一条具体的（`defer` / `map[K]V` 的读）才能让人立刻定位。
fn unsupported(tt: &TypeTable, f: &Func) -> Option<(SsaCode, String)> {
    if !f.captured.is_empty() {
        return Some((
            SsaCode::Closure,
            format!(
                "被提升的闭包捕获了 {} 个外层变量（帧槽与外层 Cell 别名，值唯一定义不成立）",
                f.captured.len()
            ),
        ));
    }
    unsup_block(tt, &f.body)
}

/// `map` / `chan` 类型 ⇒ 对应的码。接口**不在这里**：接口值当数据传是支持的，
/// 只有「按动态类型派发/分支」不支持（那由调用点与 `TypeSwitch` 判）。
fn face_kind(tt: &TypeTable, t: TypeId) -> Option<SsaCode> {
    if t.index() >= tt.len() {
        return None;
    }
    match tt.type_of(rgoc_types::underlying(tt, t)) {
        rgoc_types::typ::Type::Map { .. } => Some(SsaCode::Map),
        rgoc_types::typ::Type::Chan { .. } => Some(SsaCode::Chan),
        _ => None,
    }
}

fn unsup_block(tt: &TypeTable, b: &HirBlock) -> Option<(SsaCode, String)> {
    b.stmts.iter().find_map(|s| unsup_stmt(tt, s))
}

fn unsup_stmt(tt: &TypeTable, s: &Stmt) -> Option<(SsaCode, String)> {
    match s {
        Stmt::Defer { .. } => Some((
            SsaCode::Defer,
            "defer（延迟调用链是运行期结构，不是 SSA 值）".to_string(),
        )),
        Stmt::TypeSwitch { .. } => Some((
            SsaCode::IfaceDyn,
            "type switch（按运行期动态类型分支）".to_string(),
        )),
        Stmt::Range { x, body, kind, .. } => match kind {
            RangeKind::Map => Some((SsaCode::Map, "range over map".to_string())),
            RangeKind::Str => Some((
                SsaCode::RangeStr,
                "range over string（要按 rune 解码，指令集里没有对应操作）".to_string(),
            )),
            RangeKind::Array | RangeKind::Slice | RangeKind::Int => {
                unsup_expr(tt, x).or_else(|| unsup_block(tt, body))
            }
        },
        Stmt::Assign { lhs, rhs, .. } => lhs
            .iter()
            .find_map(|p| unsup_place(tt, p))
            .or_else(|| rhs.iter().find_map(|e| unsup_expr(tt, e))),
        Stmt::ShortDecl { rhs, .. } => rhs.iter().find_map(|e| unsup_expr(tt, e)),
        Stmt::Expr(e) => unsup_expr(tt, e),
        Stmt::Return(es) => es.iter().find_map(|e| unsup_expr(tt, e)),
        Stmt::If {
            init,
            cond,
            then,
            els,
        } => init
            .as_deref()
            .and_then(|i| unsup_stmt(tt, i))
            .or_else(|| unsup_expr(tt, cond))
            .or_else(|| unsup_block(tt, then))
            .or_else(|| els.as_deref().and_then(|e| unsup_stmt(tt, e))),
        Stmt::For {
            init,
            cond,
            post,
            body,
            ..
        } => init
            .as_deref()
            .and_then(|i| unsup_stmt(tt, i))
            .or_else(|| cond.as_ref().and_then(|c| unsup_expr(tt, c)))
            .or_else(|| post.as_deref().and_then(|p| unsup_stmt(tt, p)))
            .or_else(|| unsup_block(tt, body)),
        Stmt::Switch {
            init, tag, cases, ..
        } => init
            .as_deref()
            .and_then(|i| unsup_stmt(tt, i))
            .or_else(|| tag.as_ref().and_then(|t| unsup_expr(tt, t)))
            .or_else(|| {
                cases.iter().find_map(|c| {
                    c.exprs
                        .iter()
                        .find_map(|e| unsup_expr(tt, e))
                        .or_else(|| unsup_block(tt, &c.body))
                })
            }),
        Stmt::Labeled { stmt, .. } => unsup_stmt(tt, stmt),
        Stmt::Block(b) => unsup_block(tt, b),
        Stmt::Branch { .. } | Stmt::Empty => None,
    }
}

fn unsup_place(tt: &TypeTable, p: &Place) -> Option<(SsaCode, String)> {
    match p {
        Place::Slot(_) | Place::Global(_) => None,
        Place::Field { base, .. } => unsup_place(tt, base),
        Place::Index { base, index } => unsup_expr(tt, base).or_else(|| unsup_expr(tt, index)),
        Place::Deref(e) => unsup_expr(tt, e),
    }
}

fn unsup_expr(tt: &TypeTable, e: &Expr) -> Option<(SsaCode, String)> {
    // 类型优先：`map` / `chan` 的**任何**出现（读、写、`len`、当实参）都不在面。
    // 先按类型判就不必给每个节点各写一支（`m[k]` / `delete(m,k)` / `range m` …）。
    if let Some(code) = face_kind(tt, e.typ) {
        let what = if code == SsaCode::Map { "map" } else { "chan" };
        return Some((
            code,
            format!("{what} 类型出现在表达式里（键相等 / 并发语义属宿主面）"),
        ));
    }
    match &e.kind {
        ExprKind::Const(_) | ExprKind::Slot(_) | ExprKind::Global(_) | ExprKind::Nil => None,
        ExprKind::Field { base, .. } => unsup_expr(tt, base),
        ExprKind::Index { base, index, .. } => {
            unsup_expr(tt, base).or_else(|| unsup_expr(tt, index))
        }
        ExprKind::Slice {
            base, lo, hi, max, ..
        } => unsup_expr(tt, base)
            .or_else(|| lo.as_deref().and_then(|x| unsup_expr(tt, x)))
            .or_else(|| hi.as_deref().and_then(|x| unsup_expr(tt, x)))
            .or_else(|| max.as_deref().and_then(|x| unsup_expr(tt, x))),
        ExprKind::Deref(x) | ExprKind::AddrOf(x) => unsup_expr(tt, x),
        ExprKind::Addr(p) => unsup_place(tt, p),
        ExprKind::Unary { op, x } => {
            if *op == rgoc_hir::UnOp::Recv {
                return Some((SsaCode::Chan, "`<-ch` 接收操作".to_string()));
            }
            unsup_expr(tt, x)
        }
        ExprKind::Binary { x, y, .. } => unsup_expr(tt, x).or_else(|| unsup_expr(tt, y)),
        ExprKind::Convert { to, x } => {
            if let Some(code) = face_kind(tt, *to) {
                return Some((code, "转换到 map/chan 类型".to_string()));
            }
            unsup_expr(tt, x)
        }
        ExprKind::Call { callee, args, .. } => match callee {
            Callee::Iface { .. } => Some((
                SsaCode::IfaceDyn,
                "接口方法调用（目标函数运行期才定）".to_string(),
            )),
            Callee::Method { recv, .. } => {
                unsup_expr(tt, recv).or_else(|| args.iter().find_map(|a| unsup_expr(tt, a)))
            }
            Callee::Func(_) => args.iter().find_map(|a| unsup_expr(tt, a)),
            // **被调方本身也是表达式**：`func(){...}()` 的被调方就是函数字面量。
            // 只扫实参表会漏掉它 —— 实测 `closure1.go` / `ddd.go` 因此进了
            // `expr(FuncLit)` 的 `todo!()`（panic 而不是拒绝）。
            Callee::Value(v) => {
                unsup_expr(tt, v).or_else(|| args.iter().find_map(|a| unsup_expr(tt, a)))
            }

            Callee::Builtin(k) => match k {
                BuiltinKind::Recover => Some((
                    SsaCode::Defer,
                    "recover（与 defer 同属延迟调用面）".to_string(),
                )),
                BuiltinKind::Close => Some((SsaCode::Chan, "close（chan 操作）".to_string())),
                _ => args.iter().find_map(|a| unsup_expr(tt, a)),
            },
        },
        ExprKind::Composite(c) => c.items.iter().find_map(|it| match it {
            CompositeItem::Value(x) | CompositeItem::Field(_, x) | CompositeItem::Index(_, x) => {
                unsup_expr(tt, x)
            }
            CompositeItem::Key(k, _) => Some((
                SsaCode::Map,
                format!("map 字面量的键 {k:?}（键相等是宿主语义）"),
            )),
        }),
        // 函数值：`FuncLit` 既覆盖 `func(){}`，也覆盖**具名函数当值用**（`f := g`），
        // 两者在 HIR 里同一个节点（`lower_name` 落成 `FuncLit(fid)`）。
        // IR 里没有「把函数变成值」的指令 ⇒ 整个函数值面拒掉。
        ExprKind::FuncLit(_) => Some((
            SsaCode::FuncValue,
            "函数字面量 / 函数名当值用（IR 里没有函数引用值）".to_string(),
        )),
        ExprKind::Bound { .. } => Some((SsaCode::FuncValue, "方法值（`x.M` 不调用）".to_string())),
        ExprKind::TypeAssert { .. } => {
            Some((SsaCode::IfaceDyn, "类型断言（运行期类型判定）".to_string()))
        }
        ExprKind::Make { kind, args } => match kind {
            MakeKind::Map => Some((SsaCode::Map, "make(map)".to_string())),
            MakeKind::Chan => Some((SsaCode::Chan, "make(chan)".to_string())),
            MakeKind::Slice => args.iter().find_map(|a| unsup_expr(tt, a)),
        },
        ExprKind::New(t) => face_kind(tt, *t).map(|code| (code, "`new` 的元素类型".to_string())),
        ExprKind::Builtin { kind, args, .. } => match kind {
            BuiltinKind::Recover => Some((SsaCode::Defer, "recover".to_string())),
            BuiltinKind::Close => Some((SsaCode::Chan, "close（chan 操作）".to_string())),
            _ => args.iter().find_map(|a| unsup_expr(tt, a)),
        },
    }
}

/// 建一个函数。
///
/// 三步：① 扫出**需要根**的槽（取址 ∪ 部分写）；② 算**确定赋值**（读到必已写过），
/// 其余槽提升成纯值；③ 按语句走一遍，边建块边产指令。
fn build_func(
    f: &Func,
    id: FuncId,
    program: &Program,
    effs: &[crate::effect::Summary],
    tt: &mut TypeTable,
) -> Result<SsaFunc, Reject> {
    // ① 拒绝面先扫（`Task 31`）：**有码的拒绝**先于任何建图工作。
    //
    // 为什么是「先扫一遍」而不是「走到哪拒到哪」：`expr` / `stmt` 返回的是
    // `ValueId` / `()`，不是 `Result` —— 让它们全体改签名只为传一条拒绝，
    // 会把 3.5k 行构建代码都染上 `?`。而拒绝判据**只看 HIR 的形状与静态类型**，
    // 与建图状态无关，天然是一个独立的小 pass（沿 `scan_addressed` 的先例）。
    if let Some((code, detail)) = unsupported(tt, f) {
        return Err(Reject {
            func: id,
            code,
            detail,
        });
    }

    let unsafe_slots = unsafe_slots_of(f);
    // 需要根的槽 = 取址 ∪ 部分写 ∪ **读前可能未赋值**
    let mut rooted: BTreeSet<SlotId> = scan_rooted(tt, &f.body);
    rooted.extend(unsafe_slots.iter().copied());
    rooted.remove(&BLANK_SLOT);

    // 含 `goto` ⇒ **一律不提升**，所有槽进内存。
    //
    // 理由（`R-25-1`）：`goto` 是**非结构化**跳转，目标块的参数在「跳到它的
    // 那条边」出现时往往还没定（前向 goto 更是一个块都还没有）。如果把这个槽
    // 提升成 SSA 值，回边就必须把新值当块参数送进去 —— 而参数表那时已经建好，
    // 加不进去。于是「循环里更新的值在回边处丢失」，且类型/支配全对、verifier 全绿。
    //
    // 全走内存之后，值住在帧里，任何跳转都不需要携带值语义；mem 令牌仍然按
    // 标签块的参数传递（`ssa_exec` 是顺序执行，帧本身是共享的，所以 mem 只影响
    // 优化面，不影响运行结果）。
    //
    // 代价：含 `goto` 的函数纯值化全丢（语料里这类函数很少，`ken/label.go` 是典型）。
    let goto_mode = has_goto(&f.body);
    if goto_mode {
        for i in 0..f.slot_types.len() {
            rooted.insert(SlotId(i as u32));
        }
    }
    // 聚合类型（结构体 / 数组）的槽**不提升**：`Instr::Field` 只在**地址**上取字段，
    // SSA 没有「从一个结构体值里取出第 k 个字段」的指令。留在内存里之后
    // `s.f` 就是 `Load(Field(AddrOf(root_s), k))`，不需要新指令（`R-28-1`）。
    // 代价：结构体/数组局部变量全走内存（M6 不追这个优化）。
    for i in 0..f.slot_types.len() {
        if is_aggregate(tt, f.slot_types[i]) {
            rooted.insert(SlotId(i as u32));
        }
    }

    let ng = program.globals.len();
    // 根表顺序固定：先全局（按 GlobalId），再槽（按 SlotId 升序）—— dump 的 r<k> 靠它稳定
    let mut roots: Vec<Root> = (0..ng).map(|i| Root::Global(GlobalId(i as u32))).collect();
    let mut slot_root: BTreeMap<SlotId, RootId> = BTreeMap::new();
    for s in &rooted {
        slot_root.insert(*s, RootId(roots.len() as u32));
        roots.push(Root::Slot(*s));
    }

    // 提升 = 全部槽 − 有根的 − 读前可能未赋值的。
    // 「全部槽」按 `slot_types` 的长度取：不这么写就得另找一张槽清单，
    // 而 `slot_types` 本来就是「槽 → 类型」的全集（`Func::nslots` 与它同长）。
    let promoted: BTreeSet<SlotId> = if goto_mode {
        BTreeSet::new()
    } else {
        (0..f.slot_types.len())
            .map(|i| SlotId(i as u32))
            .filter(|s| !rooted.contains(s) && !unsafe_slots.contains(s))
            .collect()
    };

    let mut fb = Fb::new(
        tt,
        f,
        program,
        effs,
        FbInit {
            roots,
            slot_root,
            promoted,
            goto_mode,
        },
    );
    fb.entry_params();

    let body = &f.body;
    if !body.stmts.is_empty() {
        fb.stmts(&body.stmts);
    }
    if !fb.terminated {
        // Go 里只有「无结果的函数」可以落到函数尾
        fb.set_term(Term::Return(Vec::new()));
    }
    Ok(fb.finish(id))
}

// ── 取址分析（Task 18）──────────────────────────────────────────────

use std::collections::BTreeSet;

use rgoc_hir::{
    BinOp, Block as HirBlock, BranchKind, BuiltinKind, Callee, Case, CompositeItem, Expr, ExprKind,
    MakeKind, Place, RangeKind, SlotId, Stmt,
};

use crate::ir::{Root, RootId};

/// 函数体里有没有 `goto`（含嵌在任何语句里的）。
///
/// 只看 `BranchKind::Goto`：`break`/`continue` **不**触发「不提升」模式 ——
/// 它们的目标（循环的 `exit` / `latch`）在跳转发生时就已有确定的参数布局，
/// 可以正常携带值（`Task 25`）。
fn has_goto(b: &HirBlock) -> bool {
    b.stmts.iter().any(stmt_has_goto)
}

fn stmt_has_goto(s: &Stmt) -> bool {
    match s {
        Stmt::Branch { kind, .. } => matches!(kind, BranchKind::Goto),
        Stmt::Assign { .. } | Stmt::ShortDecl { .. } | Stmt::Expr(_) | Stmt::Return(_) => false,
        Stmt::If { then, els, .. } => has_goto(then) || els.as_deref().is_some_and(stmt_has_goto),
        Stmt::For { body, .. } => has_goto(body),
        Stmt::Range { body, .. } => has_goto(body),
        Stmt::Switch { cases, .. } => cases.iter().any(|c| has_goto(&c.body)),
        Stmt::TypeSwitch { cases, .. } => cases.iter().any(|c| has_goto(&c.body)),
        Stmt::Labeled { stmt, .. } => stmt_has_goto(stmt),
        Stmt::Block(b) => has_goto(b),
        Stmt::Defer { .. } | Stmt::Empty => false,
    }
}

/// 扫描结果：两类「不能提升成纯值」的槽。
///
/// 扫描只跑一次，同时收两类（`addressed` 与 `partial`）—— 它们的**根因不同**
/// （取过址 / 被部分写），但结论相同：这个槽必须留在内存里。
#[derive(Default)]
struct Scanned {
    /// 取过地址的槽（`&x` / `&x.f` / `&arr[i]`）。
    addressed: BTreeSet<SlotId>,
    /// 被**部分写**过的槽（`s.f = 1` / `arr[i] = 1`）。
    ///
    /// 这类槽没有 `&`，所以不在 `addressed` 里 —— 但它同样**不能**提升：
    /// SSA 没有「改一个字段」的指令（只有整值 `Composite`），把 `s` 提升成值
    /// 就再也写不进 `s.f` 了。M6 的口径是：部分写 ⇒ 整个槽走内存。
    partial: BTreeSet<SlotId>,
}

impl Scanned {
    fn rooted(&self) -> BTreeSet<SlotId> {
        self.addressed.union(&self.partial).copied().collect()
    }
}

/// 收集函数体里**被取过地址**的槽（`Task 18` / `T-SSA-27` 的前半）。
///
/// # 为什么必须递归到 `Place::Field` / `Place::Index` 里面
///
/// `&s.f` 取的是**槽 s 的一部分**的地址 —— 一旦有人能通过这个指针写 `s.f`，
/// 整个 `s` 就不再是「值」而是「可变存储」。只认 `&x` 这一种形态会把
/// `&s.f` 漏成「s 没取过址」，于是 `s` 被提升成 SSA 值、写穿丢效果 ——
/// 这正是 `M6-design.md` §8 风险第 2 条说的「语义错且大多数 fixture 仍绿」。
///
/// # 递归的两条边界
///
/// * `Place::Deref(e)`：`&(*p)` 取的是 `p` 指向的存储，**不是** `p` 这个槽的地址
///   —— 所以只扫 `e`（`p` 的读），不把任何槽记成「取过址」。指针目标属于
///   **堆根**（或别的函数的根），不是本函数的槽。
/// * `Place::Global(_)`：包级变量有它自己的根（`Root::Global`），不进 `slot_root`。
#[must_use]
pub fn scan_addressed(tt: &TypeTable, b: &HirBlock) -> BTreeSet<SlotId> {
    let mut sc = Scanned::default();
    scan_block(tt, b, &mut sc);
    sc.addressed
}

/// 收集**需要内存根**的槽（取址 ∪ 部分写）。
#[must_use]
pub fn scan_rooted(tt: &TypeTable, b: &HirBlock) -> BTreeSet<SlotId> {
    let mut sc = Scanned::default();
    scan_block(tt, b, &mut sc);
    sc.rooted()
}

/// 按槽号升序建根表（`Root::Slot`），并给出 `slot_root` 映射（`V9` 要求两向互逆）。
///
/// 顺序固定（槽号升序 ⇒ `RootId` 升序）：dump 要确定性，`RootId` 的分配顺序
/// 进了 `C10` 的 `r<k>` 编号，不能随遍历顺序漂。
#[must_use]
pub fn roots_of(tt: &TypeTable, b: &HirBlock) -> (Vec<Root>, BTreeMap<SlotId, RootId>) {
    let mut roots = Vec::new();
    let mut slot_root = BTreeMap::new();
    for slot in scan_rooted(tt, b) {
        let id = RootId(roots.len() as u32);
        roots.push(Root::Slot(slot));
        slot_root.insert(slot, id);
    }
    (roots, slot_root)
}

fn scan_block(tt: &TypeTable, b: &HirBlock, sc: &mut Scanned) {
    for s in &b.stmts {
        scan_stmt(tt, s, sc);
    }
}

fn scan_stmt(tt: &TypeTable, s: &Stmt, sc: &mut Scanned) {
    match s {
        Stmt::Assign { lhs, rhs, .. } => {
            for p in lhs {
                // 赋值目标**不是**取址：`a = 1` 不产生任何根。
                scan_lhs_place(tt, p, sc);
            }
            for e in rhs {
                scan_expr(tt, e, sc);
            }
        }
        Stmt::ShortDecl { rhs, .. } => {
            for e in rhs {
                scan_expr(tt, e, sc);
            }
        }
        Stmt::Expr(e) => scan_expr(tt, e, sc),
        Stmt::If {
            init,
            cond,
            then,
            els,
        } => {
            if let Some(i) = init {
                scan_stmt(tt, i, sc);
            }
            scan_expr(tt, cond, sc);
            scan_block(tt, then, sc);
            if let Some(e) = els {
                scan_stmt(tt, e, sc);
            }
        }
        Stmt::For {
            init,
            cond,
            post,
            body,
            ..
        } => {
            if let Some(i) = init {
                scan_stmt(tt, i, sc);
            }
            if let Some(c) = cond {
                scan_expr(tt, c, sc);
            }
            if let Some(p) = post {
                scan_stmt(tt, p, sc);
            }
            scan_block(tt, body, sc);
        }
        Stmt::Range {
            key,
            value,
            x,
            body,
            ..
        } => {
            if let Some(p) = key {
                scan_lhs_place(tt, p, sc);
            }
            if let Some(p) = value {
                scan_lhs_place(tt, p, sc);
            }
            scan_expr(tt, x, sc);
            scan_block(tt, body, sc);
        }
        Stmt::Switch {
            init, tag, cases, ..
        } => {
            if let Some(i) = init {
                scan_stmt(tt, i, sc);
            }
            if let Some(t) = tag {
                scan_expr(tt, t, sc);
            }
            for c in cases {
                for e in &c.exprs {
                    scan_expr(tt, e, sc);
                }
                scan_block(tt, &c.body, sc);
            }
        }
        Stmt::TypeSwitch { init, x, cases, .. } => {
            if let Some(i) = init {
                scan_stmt(tt, i, sc);
            }
            scan_expr(tt, x, sc);
            for c in cases {
                scan_block(tt, &c.body, sc);
            }
        }
        Stmt::Return(es) => {
            for e in es {
                scan_expr(tt, e, sc);
            }
        }
        Stmt::Branch { kind, .. } => {
            // `BranchKind` 不带表达式；显式 match 是为了加变体时编译报错
            match kind {
                BranchKind::Break
                | BranchKind::Continue
                | BranchKind::Goto
                | BranchKind::Fallthrough => {}
            }
        }
        Stmt::Labeled { stmt, .. } => scan_stmt(tt, stmt, sc),
        Stmt::Block(b) => scan_block(tt, b, sc),
        Stmt::Defer { call } => scan_expr(tt, call, sc),
        Stmt::Empty => {}
    }
}

fn scan_expr(tt: &TypeTable, e: &Expr, sc: &mut Scanned) {
    match &e.kind {
        ExprKind::Const(_) | ExprKind::Slot(_) | ExprKind::Global(_) | ExprKind::Nil => {}
        ExprKind::Field { base, .. } => scan_expr(tt, base, sc),
        ExprKind::Index { base, index, .. } => {
            scan_expr(tt, base, sc);
            scan_expr(tt, index, sc);
        }
        ExprKind::Slice {
            base, lo, hi, max, ..
        } => {
            scan_expr(tt, base, sc);
            for o in [lo, hi, max].into_iter().flatten() {
                scan_expr(tt, o, sc);
            }
        }
        ExprKind::Deref(x) => scan_expr(tt, x, sc),
        // 唯一产生指针的构造：`&x`
        ExprKind::Addr(p) => scan_addr_place(tt, p, sc),
        // `&T{...}` 取的是**新分配的**存储，不是任何槽
        ExprKind::AddrOf(x) => scan_expr(tt, x, sc),
        ExprKind::Unary { x, .. } => scan_expr(tt, x, sc),
        ExprKind::Binary { x, y, .. } => {
            scan_expr(tt, x, sc);
            scan_expr(tt, y, sc);
        }
        ExprKind::Call { args, .. } => {
            for a in args {
                scan_expr(tt, a, sc);
            }
        }
        ExprKind::Convert { x, .. } => scan_expr(tt, x, sc),
        ExprKind::Composite(c) => {
            for it in &c.items {
                match it {
                    CompositeItem::Value(x) => scan_expr(tt, x, sc),
                    CompositeItem::Key(k, v) => {
                        scan_expr(tt, k, sc);
                        scan_expr(tt, v, sc);
                    }
                    CompositeItem::Field(_, x) | CompositeItem::Index(_, x) => scan_expr(tt, x, sc),
                }
            }
        }
        ExprKind::FuncLit(_) => {
            // 函数字面量已提升为独立 `Func`，它的体**不属于**本函数 ——
            // 闭包里的 `&x` 会让 `x` 逃逸到闭包帧（`Root::Slot` 的边界），
            // 但 M6 把闭包整体拒掉（`SSA-UNSUPPORTED-CLOSURE`，Task 31），
            // 所以这里不扫。
        }
        ExprKind::Bound { recv, .. } => scan_expr(tt, recv, sc),
        ExprKind::TypeAssert { x, .. } => scan_expr(tt, x, sc),
        ExprKind::Make { kind, args } => {
            match kind {
                MakeKind::Slice | MakeKind::Map | MakeKind::Chan => {}
            }
            for a in args {
                scan_expr(tt, a, sc);
            }
        }
        ExprKind::New(_) => {}
        ExprKind::Builtin { args, .. } => {
            for a in args {
                scan_expr(tt, a, sc);
            }
        }
    }
}

/// `&place` 里的 place：**标记**被取址的槽。
fn scan_addr_place(tt: &TypeTable, p: &Place, sc: &mut Scanned) {
    match p {
        Place::Slot(s) => {
            sc.addressed.insert(*s);
        }
        Place::Global(_) => {}
        Place::Field { base, .. } => scan_addr_place(tt, base, sc),
        Place::Index { base, index } => {
            // `&arr[i]`：数组**元素在数组槽的存储里** ⇒ 数组槽本身被取址。
            // `&s[i]`（s 是 slice）元素在底层数组（堆根），s 这个槽不被取址 ——
            // 所以这里必须按**类型**分叉，不能一律保守地把 base 的槽记进来
            // （那会造出假根，把纯值化的机会白扔掉，还会让 dump 与预期不符）。
            if is_array(tt, base.typ) {
                scan_value_root(tt, base, sc, true);
            } else {
                scan_expr(tt, base, sc);
            }
            scan_expr(tt, index, sc);
        }
        // `&*p`：取的是 `p` 指向的存储，不是 `p` 这个槽
        Place::Deref(e) => scan_expr(tt, e, sc),
    }
}

/// 一个**值**的存储是否被整体取址（用于 `&arr[i]` 的底座）。
///
/// 只有「值直接住在某个槽里」的形态才算：`Slot`、以及嵌在结构体里的 `Field`。
/// 其余（`Deref` / `Index` / `Slice` / 调用结果）的存储都在别处（堆根或别人的帧），
/// 不产生本函数的 `Root::Slot`。
fn scan_value_root(tt: &TypeTable, e: &Expr, sc: &mut Scanned, addressed: bool) {
    match &e.kind {
        ExprKind::Slot(s) => {
            if addressed {
                sc.addressed.insert(*s);
            } else {
                sc.partial.insert(*s);
            }
        }
        ExprKind::Field { base, .. } => scan_value_root(tt, base, sc, addressed),
        // 其余形态：存储不在本函数的槽里，但仍要扫它们内部有没有 `&x`
        _ => scan_expr(tt, e, sc),
    }
}

/// 是否是**聚合类型**（结构体 / 数组，含具名与别名）。
fn is_aggregate(tt: &TypeTable, t: TypeId) -> bool {
    if t.index() >= tt.len() {
        return false;
    }
    matches!(
        tt.type_of(rgoc_types::underlying(tt, t)),
        rgoc_types::typ::Type::Struct(_) | rgoc_types::typ::Type::Array { .. }
    )
}

/// 是否是**数组**（含具名数组、别名；`[]T` 不算）。
fn is_array(tt: &TypeTable, t: TypeId) -> bool {
    if t.index() >= tt.len() {
        return false;
    }
    matches!(
        tt.type_of(rgoc_types::underlying(tt, t)),
        rgoc_types::typ::Type::Array { .. }
    )
}

/// 赋值目标里的 place：**不标记**槽（写不是取址），只递归找内部表达式里的 `&x`。
///
/// # 为什么必须与 [`scan_addr_place`] 分开
///
/// 实测踩过：把赋值目标也走「取址」那条路，`a = 1` 就让 `a` 变成了根 ——
/// 于是每个有局部变量的函数都会退化成「全走内存」，纯值化（`T-SSA-26`）永远不成立。
///
/// # 空白标识符 `_`（`SlotId(u32::MAX)`）为什么不用特判
///
/// HIR 用 `SlotId(u32::MAX)` 表示 `_`。它只可能出现在**赋值目标**位置
/// （`_ = x`、`_ = &x`），而这条路**不标记**任何槽 ⇒ 空白槽到不了 `roots_of`。
/// 初版在这里加过一条「跳过 `u32::MAX`」的判断，变异测试证明它**不可达**
/// （删掉判断，8 条测试全绿），于是按纪律删掉 —— 一条永远走不到的守卫等于没有。
fn scan_lhs_place(tt: &TypeTable, p: &Place, sc: &mut Scanned) {
    match p {
        Place::Slot(_) | Place::Global(_) => {}
        // `s.f = ...`：s 被**部分写** ⇒ s 不能提升
        Place::Field { base, .. } => scan_place_root(tt, base, sc, false),
        Place::Index { base, index } => {
            // `arr[i] = ...`（数组）改的是 arr 的存储 ⇒ arr 部分写；
            // `s[i] = ...`（slice）改的是底层数组（堆根）⇒ 与槽无关。
            if is_array(tt, base.typ) {
                scan_value_root(tt, base, sc, false);
            } else {
                scan_expr(tt, base, sc);
            }
            scan_expr(tt, index, sc);
        }
        Place::Deref(e) => scan_expr(tt, e, sc),
    }
}

/// `place` 的底座槽归入「被取址」（`addressed = true`）或「被部分写」（`false`）。
fn scan_place_root(tt: &TypeTable, p: &Place, sc: &mut Scanned, addressed: bool) {
    match p {
        Place::Slot(s) => {
            if addressed {
                sc.addressed.insert(*s);
            } else {
                sc.partial.insert(*s);
            }
        }
        Place::Global(_) => {}
        Place::Field { base, .. } => scan_place_root(tt, base, sc, addressed),
        Place::Index { base, index } => {
            if is_array(tt, base.typ) {
                scan_value_root(tt, base, sc, addressed);
            } else {
                scan_expr(tt, base, sc);
            }
            scan_expr(tt, index, sc);
        }
        Place::Deref(e) => scan_expr(tt, e, sc),
    }
}

// ── 确定赋值分析（Task 19 的判据）────────────────────────────────────

/// 算「读到之前一定已经写过」的槽（这些才能在保持正确的前提下提升成纯值）。
///
/// # 为什么要这条分析，而不是「没取址就提升」
///
/// `var a int; print(a)` 里的 `a` 没被取址，但它**先读后写**（根本没写）。
/// 提升成纯值之后，读点找不到任何定义 —— 要么 panic、要么凭空造一个「零值」，
/// 而 SSA 里没有「零值」这个指令（`ConstVal` 只有 4 类具体值）。
/// 正确做法：**没有确定赋值就留在内存里**，让帧初始化（`C9` 的零值表）负责。
///
/// # 分支与循环的保守口径
///
/// * `if/else`：两条支路各自推进，汇合取**交集**（只在一支里写过的，汇合点不算定义）；
///   没有 `else` 时，`else` 支就是「没写」，所以交集含原状态。
/// * `for` / `range`：循环体可能 0 次，所以**体的推进不传出循环**；
///   但体内部的读仍按「进入循环前的状态」判定（首轮确实可能未定义）。
/// * `return` / `break` / `goto`：不改变 `must`（保守：只会少提升，不会多提升）。
///
/// 保守的代价只是「少提升几个槽」＝ 多一点 `Load/Store`，**不会算错**；
/// 反方向（多提升）才会算错，所以所有不确定处一律取保守。
struct Def<'a> {
    f: &'a Func,
    /// 读到过「当时还没写过」的槽 ⇒ 永远不提升。
    unsafe_slots: BTreeSet<SlotId>,
    /// 当前路径上已经确定写过的槽。
    must: BTreeSet<SlotId>,
}

/// 返回「**读到过尚未写过的值**」的槽。
///
/// # 判据为什么是「读前未写」而不是「函数末尾已写」
///
/// 初版用的是后者（`must` 在函数末尾的集合），被 `for i := 0; i < 3; i++ { x := i }`
/// 打穿：循环体的推进**不传出循环**（循环可能 0 次），所以 `x` 永远不在末尾的 `must` 里，
/// 于是 `x` 既不提升也没有根 —— 构建器在读 `x` 时 panic。
///
/// 但 `x` 是**循环体内声明、循环体内使用**的槽，它「循环结束后有没有值」根本无关紧要。
/// 正确的判据只需要一条：**每一次读，都保证在此前写过**。满足这条就能提升
/// （提升后每个读点都在支配它的位置上找到定义）；不满足就只能留在内存里，
/// 由帧初始化的零值表兜底。
///
/// 于是「循环体里推进出来的 `must` 传不传出循环」不再影响判据 —— 它只影响
/// **读点判定**（首轮确实可能未定义），那正是我们要的保守方向。
///
/// # 保守的代价
///
/// `for i := 0; i < 3; i++ { a = 1 }; print(a)` 里的 `a` 会被判成「读前未写」
/// （循环可能 0 次）⇒ 留在内存。正确，只是少提升一个槽。
fn unsafe_slots_of(f: &Func) -> BTreeSet<SlotId> {
    let mut d = Def {
        f,
        unsafe_slots: BTreeSet::new(),
        must: f.params.iter().copied().collect(),
    };
    d.stmts(&f.body.stmts);
    d.unsafe_slots
}

impl Def<'_> {
    fn read(&mut self, s: SlotId) {
        if s == BLANK_SLOT {
            return;
        }
        if !self.must.contains(&s) {
            self.unsafe_slots.insert(s);
        }
    }

    fn write(&mut self, s: SlotId) {
        if s != BLANK_SLOT {
            self.must.insert(s);
        }
    }

    fn stmts(&mut self, v: &[Stmt]) {
        for s in v {
            self.stmt(s);
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Assign { lhs, op, rhs } => {
                for e in rhs {
                    self.expr(e);
                }
                // 复合赋值（`a += 1`）先读左值
                if op.is_some() {
                    for p in lhs {
                        self.read_place(p);
                    }
                }
                for p in lhs {
                    self.write_place(p);
                }
            }
            Stmt::ShortDecl { slots, rhs } => {
                for e in rhs {
                    self.expr(e);
                }
                for s in slots {
                    self.write(*s);
                }
            }
            Stmt::Expr(e) => self.expr(e),
            Stmt::If {
                init,
                cond,
                then,
                els,
            } => {
                if let Some(i) = init {
                    self.stmt(i);
                }
                self.expr(cond);
                let base = self.must.clone();
                self.stmts(&then.stmts);
                let then_must = std::mem::replace(&mut self.must, base);
                if let Some(e) = els {
                    self.stmt(e);
                }
                // 交集：两条支路都写过的才算
                self.must = then_must.intersection(&self.must).copied().collect();
            }
            Stmt::For {
                init,
                cond,
                post,
                body,
                ..
            } => {
                if let Some(i) = init {
                    self.stmt(i);
                }
                if let Some(c) = cond {
                    self.expr(c);
                }
                let base = self.must.clone();
                self.stmts(&body.stmts);
                if let Some(p) = post {
                    self.stmt(p);
                }
                // 循环体可能一次都不跑
                self.must = base;
            }
            Stmt::Range {
                key,
                value,
                x,
                body,
                ..
            } => {
                self.expr(x);
                if let Some(p) = key {
                    self.write_place(p);
                }
                if let Some(p) = value {
                    self.write_place(p);
                }
                let base = self.must.clone();
                self.stmts(&body.stmts);
                self.must = base;
            }
            Stmt::Switch {
                init, tag, cases, ..
            } => {
                if let Some(i) = init {
                    self.stmt(i);
                }
                if let Some(t) = tag {
                    self.expr(t);
                }
                let base = self.must.clone();
                let mut acc: Option<BTreeSet<SlotId>> = None;
                let mut has_default = false;
                for c in cases {
                    self.must = base.clone();
                    for e in &c.exprs {
                        self.expr(e);
                    }
                    self.stmts(&c.body.stmts);
                    has_default |= c.is_default;
                    acc = Some(match acc {
                        None => self.must.clone(),
                        Some(a) => a.intersection(&self.must).copied().collect(),
                    });
                }
                self.must = if has_default {
                    acc.unwrap_or(base)
                } else {
                    // 没有 default ⇒ 「一条都没匹配」这条路径也在
                    acc.unwrap_or_default()
                        .intersection(&base)
                        .copied()
                        .collect()
                };
            }
            Stmt::TypeSwitch { init, x, cases, .. } => {
                if let Some(i) = init {
                    self.stmt(i);
                }
                self.expr(x);
                let base = self.must.clone();
                for c in cases {
                    self.must = base.clone();
                    if let Some(b) = c.bind {
                        self.write(b);
                    }
                    self.stmts(&c.body.stmts);
                }
                self.must = base;
            }
            Stmt::Return(es) => {
                if es.is_empty() {
                    // 裸 return 读命名结果槽
                    for s in &self.f.results {
                        self.read(*s);
                    }
                } else {
                    for e in es {
                        self.expr(e);
                    }
                }
            }
            Stmt::Branch { .. } => {}
            Stmt::Labeled { stmt, .. } => self.stmt(stmt),
            Stmt::Block(b) => self.stmts(&b.stmts),
            Stmt::Defer { call } => self.expr(call),
            Stmt::Empty => {}
        }
    }

    fn read_place(&mut self, p: &Place) {
        match p {
            Place::Slot(s) => self.read(*s),
            Place::Global(_) => {}
            Place::Field { base, .. } => self.read_place(base),
            Place::Index { base, index } => {
                self.expr(base);
                self.expr(index);
            }
            Place::Deref(e) => self.expr(e),
        }
    }

    fn write_place(&mut self, p: &Place) {
        match p {
            Place::Slot(s) => self.write(*s),
            // 写字段/元素写的是内存（整个槽已因「部分写」进根表），不是整槽定义
            Place::Global(_) | Place::Field { .. } => {}
            Place::Index { base, index } => {
                self.expr(base);
                self.expr(index);
            }
            Place::Deref(e) => self.expr(e),
        }
    }

    fn expr(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Const(_) | ExprKind::Nil | ExprKind::Global(_) => {}
            ExprKind::Slot(s) => self.read(*s),
            ExprKind::Field { base, .. } => self.expr(base),
            ExprKind::Index { base, index, .. } => {
                self.expr(base);
                self.expr(index);
            }
            ExprKind::Slice {
                base, lo, hi, max, ..
            } => {
                self.expr(base);
                for o in [lo, hi, max].into_iter().flatten() {
                    self.expr(o);
                }
            }
            ExprKind::Deref(x) => self.expr(x),
            ExprKind::Addr(p) => self.read_place(p),
            ExprKind::AddrOf(x) => self.expr(x),
            ExprKind::Unary { x, .. } => self.expr(x),
            ExprKind::Binary { x, y, .. } => {
                self.expr(x);
                self.expr(y);
            }
            ExprKind::Call { args, .. } => {
                for a in args {
                    self.expr(a);
                }
            }
            ExprKind::Convert { x, .. } => self.expr(x),
            ExprKind::Composite(c) => {
                for it in &c.items {
                    match it {
                        CompositeItem::Value(x) => self.expr(x),
                        CompositeItem::Key(k, v) => {
                            self.expr(k);
                            self.expr(v);
                        }
                        CompositeItem::Field(_, x) | CompositeItem::Index(_, x) => self.expr(x),
                    }
                }
            }
            ExprKind::FuncLit(_) | ExprKind::New(_) => {}
            ExprKind::Bound { recv, .. } => self.expr(recv),
            ExprKind::TypeAssert { x, .. } => self.expr(x),
            ExprKind::Make { args, .. } | ExprKind::Builtin { args, .. } => {
                for a in args {
                    self.expr(a);
                }
            }
        }
    }
}

// ── 函数体构建器（Task 20 起）────────────────────────────────────────

/// 一个块入口处的状态：`(槽 → 当前值, 根 → 当前 mem 令牌)`。
///
/// 别名是必须的 —— 不写它，`Option<(BlockId, (BTreeMap<..>, BTreeMap<..>))>`
/// 这种签名会被 `clippy::type_complexity` 拦（而且读起来也确实累）。
type BlockState = (BTreeMap<SlotId, ValueId>, BTreeMap<RootId, ValueId>);

/// 一个跳转目标块的参数布局：**槽在前、mem 在后**（`Task 25`）。
#[derive(Clone, Default)]
struct ParamLayout {
    slots: Vec<SlotId>,
    roots: Vec<RootId>,
}

/// `goto` 的标签位置。
struct LabelSite {
    /// 目标块（`goto` 先出现时懒建）。
    block: Option<BlockId>,
    /// 在块参数定下来之前跳进来的那些边。
    incoming: Vec<BranchOut>,
    /// 块参数定下来之后填上；有它就意味着「可以立即跳」。
    layout: Option<ParamLayout>,
}

/// 循环上下文（`break` / `continue` 的目标）。
struct LoopCtx {
    label: Option<String>,
    /// `header` / `latch` / `exit` 共用的参数布局。
    layout: ParamLayout,
    /// `exit`（`for{}` 没 `break` 时不建，所以是 `None`）。
    break_target: Option<BlockId>,
    /// `for{}` 且 `break` 先出现时，先记着，收尾再建 `exit`。
    break_pending: Vec<BranchOut>,
    /// `continue` 总是后补（目标可能是 `latch`，那时还没建）。
    continues: Vec<BranchOut>,
}

/// `switch` 上下文（`break` 的目标是它自己的合流块）。
struct SwitchCtx {
    label: Option<String>,
    breaks: Vec<BranchOut>,
}

/// 可被 `break` / `continue` 指到的区域。
enum Region {
    Loop(LoopCtx),
    Switch(SwitchCtx),
}

/// 一条「落到汇合点」的分支的状态快照。
struct BranchOut {
    /// 分支的尾块（还没有终止符）。
    block: BlockId,
    val: BTreeMap<SlotId, ValueId>,
    mem: BTreeMap<RootId, ValueId>,
}

/// 边建块边产指令的单函数构建器。
///
/// # 三个状态
///
/// * `val: 槽 → 当前值`（只对**提升**的槽；取址/部分写/未确定赋值的槽不在里面）
/// * `mem: 根 → 当前 mem 令牌`（每个根一条线；`Store` 产新令牌、`Load` 不动它）
/// * `terminated`：当前块已有终止符 ⇒ 后续语句是死代码
///
/// # 为什么 mem 令牌是懒建的
///
/// 一个没有任何 `Load/Store` 的函数不该出现 mem 常量（`T-SSA-26`：纯函数里
/// 连一条访存指令都不该有）。所以令牌在**第一次真正访存**时才建。
/// `Fb::new` 的「非默认输入」：两个入口（普通函数 / 合成 init）只有这几项不同。
struct FbInit {
    /// 根表（下标即 `RootId`）。
    roots: Vec<Root>,
    /// 取过地址的槽 → 根。
    slot_root: BTreeMap<SlotId, RootId>,
    /// 提升成纯值的槽。
    promoted: BTreeSet<SlotId>,
    /// 含 `goto` ⇒ 一律不提升。
    goto_mode: bool,
}

struct Fb<'a> {
    tt: &'a mut TypeTable,
    f: &'a Func,
    program: &'a Program,
    /// 全包的调用摘要（`Task 27`；`Instr::Call.effect` 从它来）。
    effs: &'a [crate::effect::Summary],
    globals: &'a [rgoc_hir::Global],
    /// 本函数调用到的已知签名（`V5` 的比对面；`Task 26` 起真的填）。
    call_sigs: BTreeMap<FuncId, Sig>,
    values: Vec<TypeId>,
    blocks: Vec<Block>,
    cur: BlockId,
    val: BTreeMap<SlotId, ValueId>,
    mem: BTreeMap<RootId, ValueId>,
    roots: Vec<Root>,
    slot_root: BTreeMap<SlotId, RootId>,
    promoted: BTreeSet<SlotId>,
    /// 每个根的地址类型缓存（`*T`；避免同一根建出多个相同的指针类型槽）
    ptr_ty: BTreeMap<RootId, TypeId>,
    /// 指针值 → 它指向的根（`Task 30`）。
    ///
    /// # 这不是别名分析，是**构造性**事实
    ///
    /// 只有 `AddrOf` 产出的值才有条目：它指向哪个根，是**造这条指令时就知道的**。
    /// 一旦值被合并（块参数）、被转换、来自形参或调用结果，就没有条目 ——
    /// 那时按「可能指向任何根」处理（见 `store_to_unknown`），**不是**猜一个根。
    ptr_root: BTreeMap<ValueId, RootId>,
    terminated: bool,
    /// `bool` 的 `TypeId` 缓存（`switch` 的比较与 `Or` 合并要用）。
    bool_ty: Option<TypeId>,
    /// `int` 的 `TypeId` 缓存（`range` 的下标与 `len` 用）。
    int_ty: Option<TypeId>,
    /// 构建器私有的槽类型（`range` 下标），见 `fresh_slot`。
    extra_slots: BTreeMap<SlotId, TypeId>,
    /// 下一个私有槽号（从 `u32::MAX-1` 往下）。
    next_tmp: u32,
    /// 当前 `case` 体的 `fallthrough` 目标（`Task 24`）；不在 case 体里时是 `None`。
    fallthrough: Option<BlockId>,
    /// `goto` 的标签位置（`Task 25`）。
    labels: BTreeMap<String, LabelSite>,
    /// `break` / `continue` 的区域栈（最内层在末尾）。
    regions: Vec<Region>,
    /// 本函数含 `goto` ⇒ **不提升**（见 `build_func` 的说明），标签块按「全部根」定参数。
    goto_mode: bool,
}

impl Fb<'_> {
    /// 装配一个构建器（`build_func` 与合成的包初始化函数共用）。
    ///
    /// 抽出来是为了**不让两份字段清单漂开**：合成 init 的根表、提升集、
    /// `goto_mode` 与普通函数不同，但其余二十来个字段必须一模一样 ——
    /// 复制一份 struct literal 的那种写法，加字段时必然只改一处。
    fn new<'a>(
        tt: &'a mut TypeTable,
        f: &'a Func,
        program: &'a Program,
        effs: &'a [crate::effect::Summary],
        init: FbInit,
    ) -> Fb<'a> {
        Fb {
            tt,
            f,
            program,
            effs,
            globals: &program.globals,
            call_sigs: BTreeMap::new(),
            values: Vec::new(),
            blocks: vec![Block::default()],
            cur: BlockId(0),
            val: BTreeMap::new(),
            mem: BTreeMap::new(),
            roots: init.roots,
            slot_root: init.slot_root,
            promoted: init.promoted,
            ptr_ty: BTreeMap::new(),
            ptr_root: BTreeMap::new(),
            terminated: false,
            bool_ty: None,
            int_ty: None,
            extra_slots: BTreeMap::new(),
            next_tmp: u32::MAX - 1,
            fallthrough: None,
            labels: BTreeMap::new(),
            regions: Vec::new(),
            goto_mode: init.goto_mode,
        }
    }

    fn new_value(&mut self, t: TypeId) -> ValueId {
        self.values.push(t);
        ValueId((self.values.len() - 1) as u32)
    }

    fn emit(&mut self, i: Instr) {
        self.blocks[self.cur.index()].instrs.push(i);
    }

    fn set_term(&mut self, t: Term) {
        self.blocks[self.cur.index()].term = t;
        self.terminated = true;
    }

    /// 形参 → 入口块块参数；被取址的形参要把入参**存进它的根**。
    fn entry_params(&mut self) {
        let params = self.f.params.clone();
        let types = self.f.param_types.clone();
        for (s, t) in params.iter().zip(types) {
            let v = self.new_value(t);
            self.blocks[0].params.push(v);
            if self.promoted.contains(s) {
                self.val.insert(*s, v);
            } else if let Some(&root) = self.slot_root.get(s) {
                // 形参到手就是「已赋值」：rooted 的槽必须把它落进内存
                let addr = self.addr_of(root);
                self.store_root(root, addr, v);
            }
        }
    }

    fn elem_type(&self, root: RootId) -> TypeId {
        match self.roots.get(root.index()) {
            Some(Root::Slot(s)) => self.slot_typ(*s),
            Some(Root::Global(g)) => self
                .globals
                .get(g.index())
                .map_or_else(|| TypeId::from_index(0), |x| x.typ),
            Some(Root::Heap) | None => self
                .f
                .slot_types
                .first()
                .copied()
                .unwrap_or_else(|| TypeId::from_index(0)),
        }
    }

    fn ptr_ty_of(&mut self, root: RootId) -> TypeId {
        if let Some(&t) = self.ptr_ty.get(&root) {
            return t;
        }
        let elem = self.elem_type(root);
        let p = self.tt.add_pointer(elem);
        self.ptr_ty.insert(root, p);
        p
    }

    /// 根的地址（`AddrOf`）。整根读写都从它起步。
    fn addr_of(&mut self, root: RootId) -> ValueId {
        let t = self.ptr_ty_of(root);
        let dst = self.new_value(t);
        self.emit(Instr::AddrOf { dst, root });
        // 这条指针值指向哪个根，是**造它的时候就知道的** —— 记下来，
        // 之后 `*p` 才能落在同一个根上（`T-SSA-25`）。
        self.ptr_root.insert(dst, root);
        dst
    }

    /// 一个指针值「构造性地」指向哪个根（没有记录 ⇒ `None`）。
    fn ptr_hint(&self, v: ValueId) -> Option<RootId> {
        self.ptr_root.get(&v).copied()
    }

    /// 记下「这个指针值指向哪个根」（根未知就不记）。
    ///
    /// 地址类指令（`AddrOf` / `Field` / `Index`）的**结果仍是地址**，
    /// 根与底座相同 —— 不记的话 `&s.n` 一旦离开 `place_addr_root` 的返回值
    /// 就「失忆」了，写穿会退化成扇出。
    fn note_root(&mut self, v: ValueId, root: Option<RootId>) {
        if let Some(r) = root {
            self.ptr_root.insert(v, r);
        }
    }

    /// 根的当前 mem 令牌（懒建；类型只是占位，`C9` 的 mem 是抽象令牌）。
    fn mem_of(&mut self, root: RootId) -> ValueId {
        if let Some(&m) = self.mem.get(&root) {
            return m;
        }
        let t = self.ptr_ty_of(root);
        let dst = self.new_value(t);
        self.emit(Instr::Const {
            dst,
            c: rgoc_hir::types::ConstVal::Int(0),
            typ: t,
        });
        self.mem.insert(root, dst);
        dst
    }

    fn load_root(&mut self, root: RootId, typ: TypeId) -> ValueId {
        let mem = self.mem_of(root);
        let addr = self.addr_of(root);
        let dst = self.new_value(typ);
        self.emit(Instr::Load {
            dst,
            mem,
            root,
            addr,
            typ,
        });
        dst
    }

    fn store_root(&mut self, root: RootId, addr: ValueId, v: ValueId) {
        let mem = self.mem_of(root);
        let t = self.ptr_ty_of(root);
        let dst = self.new_value(t);
        self.emit(Instr::Store {
            dst,
            mem,
            root,
            addr,
            val: v,
        });
        self.mem.insert(root, dst);
    }

    /// 通过一个**根未知**的指针写（`*p = v`，`p` 是形参 / 调用结果 / 合并后的值）。
    ///
    /// # 为什么不「随便挑一个根」
    ///
    /// 挑堆根是最省事的写法，也是**静默错**：调用方的槽根没有前进，
    /// 之后的 `a` 读到的是旧 mem（静态上「写后读」不成立），优化器一旦拿它做
    /// load 消除就会读到旧值。M6 不做别名推理，那就**别假装知道**。
    ///
    /// # 扇出为什么是对的
    ///
    /// 对**每个根**各发一条 `Store`（同一个地址、同一个值）。运行期写的是真实地址，
    /// 重复写同值幂等；静态上每个根的 mem 都前进了 ⇒ 「写后读」对任何根都成立。
    /// 代价是指令数 × 根数 —— 而根数只有「取过址的槽 + 全局 + 堆」那么多。
    fn store_to_unknown(&mut self, addr: ValueId, v: ValueId) {
        self.heap_root(); // 堆根总在里面（未知指针完全可能指向堆）
        let n = self.roots.len();
        for i in 0..n {
            self.store_root(RootId(i as u32), addr, v);
        }
    }

    fn read_slot(&mut self, s: SlotId) -> ValueId {
        if let Some(&v) = self.val.get(&s) {
            return v;
        }
        let root = *self.slot_root.get(&s).unwrap_or_else(|| {
            panic!(
                "槽 s{} 既不提升也没有根（不该发生：promoted ∪ rooted == 全部槽）",
                s.0
            )
        });
        let t = self.slot_typ(s);
        self.load_root(root, t)
    }

    /// 把值的静态类型对齐到目标类型（不同就插一条 `Convert`）。
    ///
    /// # 为什么必须有这一步
    ///
    /// HIR 的表达式带着**未定型**的常量类型（`a := 1` 里 `1` 是 `untyped int`），
    /// 而槽有明确的静态类型（`int`）。Go 的这条隐式转换在 lowering 里没有落成节点，
    /// 所以 SSA 必须在**写入点**把它物化 —— 否则同一个槽在不同分支里会带两种类型，
    /// 汇合块的参数与边实参类型对不上（`V3` 会拒，而且拒得对）。
    ///
    /// 这条同时给出一条不变式：**槽的当前值，静态类型恒等于槽的静态类型**。
    fn coerce_to(&mut self, typ: TypeId, v: ValueId) -> ValueId {
        // 判等必须走 `identical`：同一个 `int` 在类型表里可以有多个条目
        if crate::typing::same_type(self.tt, self.values[v.index()], typ) {
            return v;
        }
        // **别往 `invalid` 上转**：那是「类型算不出来」的占位，不是一种目标类型。
        // 往它插 `Convert` 只会把「上游没给类型」伪装成「转换过了」。
        if self.is_invalid(typ) || self.is_invalid(self.values[v.index()]) {
            return v;
        }
        let dst = self.new_value(typ);
        self.emit(Instr::Convert { dst, x: v, typ });
        // 指针之间的转换**不动地址**（只改「指向什么类型」的看法）⇒ 根跟着走。
        // 不传的话 `&s.n`（`*named`）收敛到 `*int` 之后就「根未知」了，
        // 写穿会退化成扇出（实测：`p := &s.n; *p = 5` 出了两条 Store）。
        if let Some(root) = self.ptr_hint(v) {
            self.ptr_root.insert(dst, root);
        }
        dst
    }

    fn is_invalid(&self, t: TypeId) -> bool {
        t.index() >= self.tt.len() || matches!(self.tt.type_of(t), rgoc_types::typ::Type::Invalid)
    }

    fn slot_typ(&self, s: SlotId) -> TypeId {
        if let Some(t) = self.extra_slots.get(&s) {
            return *t;
        }
        self.f
            .slot_types
            .get(s.index())
            .copied()
            .unwrap_or_else(|| TypeId::from_index(0))
    }

    fn int_ty(&mut self) -> TypeId {
        if let Some(t) = self.int_ty {
            return t;
        }
        let b = rgoc_types::universe::basic_name_to_id("int").expect("int 是基本类型名");
        let t = self.tt.add_basic(b);
        self.int_ty = Some(t);
        t
    }

    /// 造一个**构建器私有**的槽（`range` 的下标用；`R-28-2`）。
    ///
    /// HIR 不保证 `range` 有关键字位（`for range xs` 就没有），而循环下标必须
    /// 跨块携带 —— 它得有个槽当载体。这类槽从 `u32::MAX-1` 往下发，不进
    /// `slot_types`，只在 `extra_slots` 里登记类型，并且**直接进 `promoted`**
    /// （它们只活在本函数内，永远不是根）。
    fn fresh_slot(&mut self, t: TypeId) -> SlotId {
        let s = SlotId(self.next_tmp);
        self.next_tmp -= 1;
        self.extra_slots.insert(s, t);
        self.promoted.insert(s);
        s
    }

    fn bool_ty(&mut self) -> TypeId {
        if let Some(t) = self.bool_ty {
            return t;
        }
        let b = rgoc_types::universe::basic_name_to_id("bool").expect("bool 是基本类型名");
        let t = self.tt.add_basic(b);
        self.bool_ty = Some(t);
        t
    }

    fn write_slot(&mut self, s: SlotId, v: ValueId) {
        if s == BLANK_SLOT {
            return;
        }
        let v = self.coerce_to(self.slot_typ(s), v);
        if self.promoted.contains(&s) {
            self.val.insert(s, v);
            return;
        }
        let Some(&root) = self.slot_root.get(&s) else {
            // **死写**：这个槽既不提升也没有根 ⇒ 全函数没人能观察它
            // （有读 ⇒ 它在 `definite`（提升）或 `unsafe`（成根）里）。丢掉是安全的。
            return;
        };
        let addr = self.addr_of(root);
        self.store_root(root, addr, v);
    }

    fn heap_root(&mut self) -> RootId {
        if let Some(i) = self.roots.iter().position(|r| *r == Root::Heap) {
            return RootId(i as u32);
        }
        self.roots.push(Root::Heap);
        RootId((self.roots.len() - 1) as u32)
    }

    /// 造一个**构建器私有**的根（`&T{...}` 的临时存储，`Task 31`）。
    ///
    /// 与 `fresh_slot` 的区别：私有槽是**值**（直接提升、不是根），
    /// 而这里要的是**存储**（`&T{}` 的结果是个指针，指向某块真存储）。
    /// 槽号同样从 `u32::MAX-1` 往下发，不进 `slot_types`，类型记在 `extra_slots`。
    fn temp_root(&mut self, t: TypeId) -> RootId {
        let s = SlotId(self.next_tmp);
        self.next_tmp -= 1;
        self.extra_slots.insert(s, t);
        let r = RootId(self.roots.len() as u32);
        self.roots.push(Root::Slot(s));
        self.slot_root.insert(s, r);
        r
    }

    /// 一个 place 的地址值。
    fn place_addr(&mut self, p: &Place) -> ValueId {
        self.place_addr_root(p).0
    }

    /// 一个 place 的「地址 + 根」，**一次求值**。
    ///
    /// # 为什么必须是同一个函数
    ///
    /// 初版是 `place_addr(p)` 与 `root_of_place(p)` 两个独立函数，读写处**各调一次**。
    /// 对 `Place::Deref(f())` 来说那就是把 `f()` 求值两遍 —— 副作用做两次，
    /// 而且两次拿到的是**两个不同的值**（地址与根来自不同的求值）。
    /// 合成一个返回值就结构性地消灭了这类错。
    ///
    /// # `None` 是什么意思
    ///
    /// **「根未知」**，不是「没有根」：`*p` 里 `p` 是形参、是调用结果、或经过块参数
    /// 合并时，M6 不做别名推理，就说不出它指向哪个根。读这种地址落堆根（保守），
    /// 写则**扇出到所有根**（见 `store_to_unknown`）—— 见返回类型是 `Option` 的理由。
    fn place_addr_root(&mut self, p: &Place) -> (ValueId, Option<RootId>) {
        match p {
            Place::Slot(s) => {
                let root = *self
                    .slot_root
                    .get(s)
                    .unwrap_or_else(|| panic!("槽 s{} 没有根", s.0));
                (self.addr_of(root), Some(root))
            }
            Place::Global(g) => {
                let root = RootId(g.0);
                (self.addr_of(root), Some(root))
            }
            Place::Field { base, index } => {
                let (base_v, root) = self.place_addr_root(base);
                let t = self.values[base_v.index()];
                let dst = self.new_value(t);
                self.emit(Instr::Field {
                    dst,
                    base: base_v,
                    index: *index,
                });
                self.note_root(dst, root);
                (dst, root)
            }
            Place::Index { base, index } => {
                let (base_v, root) = self.index_base(base);
                let idx = self.expr(index);
                let t = self.values[base_v.index()];
                let dst = self.new_value(t);
                self.emit(Instr::Index {
                    dst,
                    base: base_v,
                    i: idx,
                });
                self.note_root(dst, root);
                (dst, root)
            }
            Place::Deref(e) => {
                let addr = self.expr(e);
                let root = self.ptr_hint(addr);
                (addr, root)
            }
        }
    }

    /// 下标底座的「指针值 + 根」。
    ///
    /// 三种形态各有各的底座：
    /// * **数组槽**（`[N]T` 局部变量）：元素住在槽的存储里 ⇒ 底座是 `AddrOf(槽根)`；
    /// * **指向数组的指针**（`p[i]`，`p *[N]T`）：底座**就是** `p` 的值；
    /// * **slice**：元素在底层数组（堆根）⇒ 底座是 slice 值本身。
    ///
    /// 只按**类型**分叉，不按语法猜 —— `a[i]` 里 `a` 是数组还是 slice 决定根，
    /// 而写 `a[i] = v` 与读 `a[i]` 必须走同一条路，否则读写落在不同根上（静默丢写）。
    fn index_base(&mut self, base: &Expr) -> (ValueId, Option<RootId>) {
        if !self.is_pointer(base.typ) {
            match &base.kind {
                ExprKind::Slot(s)
                    if is_array(self.tt, base.typ) && self.slot_root.contains_key(s) =>
                {
                    let r = self.slot_root[s];
                    return (self.addr_of(r), Some(r));
                }
                ExprKind::Global(g) if is_array(self.tt, base.typ) => {
                    let r = RootId(g.0);
                    return (self.addr_of(r), Some(r));
                }
                _ => {}
            }
        }
        let v = self.expr(base);
        // 指针底座：`p` 指向哪就写哪（有记录才知道）；slice 底座：底层数组在堆根。
        let root = if self.is_pointer(base.typ) {
            self.ptr_hint(v)
        } else {
            Some(self.heap_root())
        };
        (v, root)
    }

    fn is_pointer(&self, t: TypeId) -> bool {
        if t.index() >= self.tt.len() {
            return false;
        }
        matches!(
            self.tt.type_of(rgoc_types::underlying(self.tt, t)),
            rgoc_types::typ::Type::Pointer(_)
        )
    }

    /// 一个**表达式**作为「存储位置」时的地址 + 根（`s.f` / `s.a.b` / `p.f` / `g.f`）。
    ///
    /// # 为什么 `Place` 之外还要有这一份
    ///
    /// HIR 的字段访问是 `ExprKind::Field { base: Expr }`，底座是**表达式**而不是
    /// `Place`（`p.f` 的底座是 `p` 这个指针**值**）。两者形状重叠但不互为子类型，
    /// 于是各自一个函数；判据（哪种底座落在哪个根）两边必须一致。
    ///
    /// 返回 `None` 表示**不可取址**：调用结果、常量、字面量 —— 它们没有存储。
    fn expr_addr_root(&mut self, e: &Expr) -> Option<(ValueId, Option<RootId>)> {
        match &e.kind {
            ExprKind::Slot(s) => {
                if let Some(&root) = self.slot_root.get(s) {
                    return Some((self.addr_of(root), Some(root)));
                }
                // 提升过的指针槽：值本身就是地址
                if self.is_pointer(e.typ) {
                    let v = self.expr(e);
                    return Some((v, self.ptr_hint(v)));
                }
                None
            }
            ExprKind::Global(g) => {
                let root = RootId(g.0);
                Some((self.addr_of(root), Some(root)))
            }
            ExprKind::Field { base, index } => {
                let (bv, hint) = self.expr_addr_root(base)?;
                let t = self.values[bv.index()];
                let dst = self.new_value(t);
                self.emit(Instr::Field {
                    dst,
                    base: bv,
                    index: *index,
                });
                self.note_root(dst, hint);
                Some((dst, hint))
            }
            ExprKind::Index { base, index, .. } => {
                let (bv, hint) = self.index_base(base);
                let i = self.expr(index);
                let t = self.values[bv.index()];
                let dst = self.new_value(t);
                self.emit(Instr::Index { dst, base: bv, i });
                self.note_root(dst, hint);
                Some((dst, hint))
            }
            ExprKind::Deref(x) => {
                let addr = self.expr(x);
                Some((addr, self.ptr_hint(addr)))
            }
            // 指针底座（`f().x` 之类）：值本身就是地址
            _ if self.is_pointer(e.typ) => {
                let v = self.expr(e);
                Some((v, self.ptr_hint(v)))
            }
            _ => None,
        }
    }

    /// 复合字面量（`Task 29`）：数组 / 结构体 / slice ⇒ 一条 `Composite`。
    ///
    /// # 顺序 == 源码顺序，但 keyed 形态要按**下标落位**
    ///
    /// `[5]int{2: 7}` 的 `items` 只有一项，而结果必须有 5 个元素（其余是零值）。
    /// 所以 keyed 形态先按 `Field(k)` / `Index(k)` 落到位，再用零值补空位 ——
    /// 不能让 `Composite` 的语义依赖「第几个 item 就是第几个元素」，否则
    /// 求值器还得知道哪些位置被跳过。
    ///
    /// # 空位的零值用 `Const Int(0)`
    ///
    /// 与 `R-20-1` 一致：`Int(0)` 在任意类型上表示「该类型的零值」。
    fn composite(&mut self, c: &rgoc_hir::Composite) -> ValueId {
        let arity = self.arity_of(c.typ);
        let positional = c
            .items
            .iter()
            .all(|it| matches!(it, CompositeItem::Value(_)));
        let mut items: Vec<ValueId> = Vec::new();
        if positional && arity.is_none() {
            // slice / map 字面量：元素个数就是字面量里写出来的个数
            for it in &c.items {
                if let CompositeItem::Value(x) = it {
                    items.push(self.expr(x));
                }
            }
        } else {
            let n = arity.unwrap_or(c.items.len());
            let elem_ty = self.elem_ty_of(c.typ);
            let mut slots: Vec<Option<ValueId>> = vec![None; n];
            let mut next = 0usize;
            for it in &c.items {
                let (idx, x) = match it {
                    CompositeItem::Value(x) => {
                        let i = next;
                        next += 1;
                        (i, x)
                    }
                    CompositeItem::Field(k, x) | CompositeItem::Index(k, x) => {
                        next = k + 1;
                        (*k, x)
                    }
                    // map 字面量不在 M6 面（`SSA-UNSUPPORTED-MAP`）
                    CompositeItem::Key(..) => {
                        todo!("M6/Task 31：map 字面量要报 `SSA-UNSUPPORTED-MAP`")
                    }
                };
                let v = self.expr(x);
                if idx < slots.len() {
                    slots[idx] = Some(v);
                }
            }
            for s in slots {
                items.push(match s {
                    Some(v) => v,
                    None => {
                        let t = elem_ty.unwrap_or(c.typ);
                        self.const_int(0, t)
                    }
                });
            }
        }
        let dst = self.new_value(c.typ);
        self.emit(Instr::Composite {
            dst,
            typ: c.typ,
            items,
        });
        dst
    }

    /// 聚合类型的「位置个数」：数组是长度，结构体是字段数；其余 `None`。
    fn arity_of(&self, t: TypeId) -> Option<usize> {
        if t.index() >= self.tt.len() {
            return None;
        }
        match self.tt.type_of(rgoc_types::underlying(self.tt, t)) {
            rgoc_types::typ::Type::Array { len, .. } => len
                .and_then(|cid| self.const_len(cid))
                .and_then(|n| usize::try_from(n).ok()),
            rgoc_types::typ::Type::Struct(sid) => self.struct_field_count(*sid),
            _ => None,
        }
    }

    /// 常量表里那个数组长度（`[N]T` 的 `N`）。
    ///
    /// `BigInt` 的取值交给 `rgoc_const`（它本来就是这个表的拥有者）——
    /// 这里只关心「装不装得进 `usize`」。
    fn const_len(&self, cid: rgoc_types::ConstId) -> Option<i64> {
        match self.tt.consts().get(cid) {
            rgoc_const::Const::Int(n) => n.to_i64(),
            _ => None,
        }
    }

    fn struct_field_count(&self, sid: rgoc_types::StructId) -> Option<usize> {
        Some(self.tt.struct_def(sid).fields.len())
    }

    /// 聚合类型的「元素类型」：数组/slice 取元素，结构体取第 0 个字段（只用于补零值）。
    fn elem_ty_of(&self, t: TypeId) -> Option<TypeId> {
        if t.index() >= self.tt.len() {
            return None;
        }
        match self.tt.type_of(rgoc_types::underlying(self.tt, t)) {
            rgoc_types::typ::Type::Array { elem, .. } | rgoc_types::typ::Type::Slice(elem) => {
                Some(*elem)
            }
            rgoc_types::typ::Type::Struct(sid) => {
                self.tt.struct_def(*sid).fields.first().map(|x| x.typ)
            }
            _ => None,
        }
    }

    /// 赋值/短声明的右侧：唯一右值是多值调用时展平，否则逐个求值。
    fn rhs_values(&mut self, rhs: &[Expr], want: usize) -> Vec<ValueId> {
        let vs = if rhs.len() == 1 && want > 1 {
            self.expr_multi(&rhs[0])
        } else {
            rhs.iter().map(|e| self.expr(e)).collect()
        };
        if vs.len() != want {
            todo!(
                "M6/Task 26：右侧产出 {} 个值，左侧有 {want} 个目标（检查器应已拒绝）",
                vs.len()
            );
        }
        vs
    }

    /// 一个表达式可能产出**多个**值（只有 `Call` 会）。
    ///
    /// 多值出现的位置只有三处（Go 规范）：赋值/短声明的右侧、`return`、
    /// 以及「唯一实参是多值调用」的调用实参表。这三处都走这个入口。
    fn expr_multi(&mut self, e: &Expr) -> Vec<ValueId> {
        match &e.kind {
            ExprKind::Call {
                callee,
                args,
                spread,
                variadic,
                ..
            } => self.call(e, callee, args, *spread, *variadic),
            // HIR 把 `len` / `append` / `println` 这类单独放一个变体（不带 `callee`）。
            // 它与 `Callee::Builtin` 是同一件事的两种写法，所以走同一个 `emit_builtin`。
            ExprKind::Builtin { kind, args, .. } => self.emit_builtin(e, *kind, args),
            _ => vec![self.expr(e)],
        }
    }

    /// 内建调用的公共出口（`Callee::Builtin` 与 `ExprKind::Builtin` 共用）。
    fn emit_builtin(&mut self, e: &Expr, kind: BuiltinKind, args: &[Expr]) -> Vec<ValueId> {
        let avs = self.eval_args(args, 0);
        let n = builtin_results(kind);
        let rtypes: Vec<TypeId> = if n == 0 { Vec::new() } else { vec![e.typ] };
        let eff = crate::effect::to_effect(&crate::effect::builtin_summary(kind));
        self.emit_call(CalleeRef::Builtin(kind), avs, &rtypes, eff)
    }

    /// 实参表：`spread > 0` 时唯一实参是多值调用，按它的结果数展平。
    fn eval_args(&mut self, args: &[Expr], spread: u8) -> Vec<ValueId> {
        if spread > 0 && args.len() == 1 {
            return self.expr_multi(&args[0]);
        }
        args.iter().map(|a| self.expr(a)).collect()
    }

    /// 建一条 `Call`，返回它的结果值（0 个 / 1 个 / n 个）。
    ///
    /// # `dst` 的个数与类型从**被调方的签名**来
    ///
    /// 这也是 `V5` 能查东西的前提：`Call.dsts.len()` 必须等于签名结果数。
    /// 如果把「结果个数」当成「调用点自己说几个就是几个」，`V5` 就没有比对面。
    ///
    /// # 方法的接收者单独放
    ///
    /// HIR 把接收者放在 `Callee::Method.recv`、且 `Func.params[0]` 就是它；
    /// 而 `CalleeRef::Method { recv, func }` 里 `recv` 是独立字段。
    /// 所以记进 `call_sigs` 的 `params` 要**减一**（否则 `V5` 会把每次方法调用
    /// 都判成「实参少一个」）。
    fn call(
        &mut self,
        e: &Expr,
        callee: &Callee,
        args: &[Expr],
        spread: u8,
        dots: bool,
    ) -> Vec<ValueId> {
        match callee {
            Callee::Func(fid) => {
                let (rtypes, sig, vty) = self.sig_of(*fid, false);
                self.call_sigs.insert(*fid, sig);
                let avs = self.eval_args(args, spread);
                let avs = self.pack_variadic(avs, &sig, vty, dots);
                let eff = crate::effect::to_effect(&self.effs[fid.index()]);
                self.emit_call(CalleeRef::Func(*fid), avs, &rtypes, eff)
            }
            Callee::Method { recv, func } => {
                let (rtypes, sig, vty) = self.sig_of(*func, true);
                self.call_sigs.insert(*func, sig);
                let rv = self.expr(recv);
                let avs = self.eval_args(args, spread);
                let avs = self.pack_variadic(avs, &sig, vty, dots);
                let eff = crate::effect::to_effect(&self.effs[func.index()]);
                self.emit_call(
                    CalleeRef::Method {
                        recv: rv,
                        func: *func,
                    },
                    avs,
                    &rtypes,
                    eff,
                )
            }
            Callee::Builtin(k) => self.emit_builtin(e, *k, args),
            Callee::Value(v) => {
                let fv = self.expr(v);
                let avs = self.eval_args(args, spread);
                // 间接调用的签名**静态可读**：被调方是个函数值，它的静态类型
                // 就是签名（`func(int, byte) (byte, int)`）。不读它就只能猜
                // 「一个结果」—— 实测 `method5.go` 的 `b, x := f(1000, 99)`
                // 因此 panic（右侧 1 个值、左侧 2 个目标）。
                let rtypes = self.func_result_types(v.typ).unwrap_or_else(|| vec![e.typ]);
                // 副作用未知 ⇒ 保守「什么都能写 + 可能回调」
                let eff = Effect {
                    writes: Vec::new(),
                    writes_all: true,
                    may_panic: true,
                    may_call_back: true,
                };
                self.emit_call(CalleeRef::Indirect(fv), avs, &rtypes, eff)
            }
            Callee::Iface { .. } => {
                todo!("M6/Task 31：接口动态派发要报 `SSA-UNSUPPORTED-IFACE-DYN`")
            }
        }
    }

    /// 函数类型的**结果类型表**（`func(...) (A, B)` ⇒ `[A, B]`）。
    ///
    /// 间接调用要用它定 `Call.dsts` 的个数与类型：被调方的值是运行期的，
    /// 但它的**静态类型**（签名）就在眼前，没有理由去猜。
    fn func_result_types(&self, t: TypeId) -> Option<Vec<TypeId>> {
        if t.index() >= self.tt.len() {
            return None;
        }
        let Some(rgoc_types::typ::Type::Signature(sid)) =
            Some(self.tt.type_of(rgoc_types::underlying(self.tt, t)))
        else {
            return None;
        };
        let results = self.tt.signature_def(*sid).results;
        match self.tt.type_of(results) {
            rgoc_types::typ::Type::Tuple(tid) => Some(
                self.tt
                    .tuple_def(*tid)
                    .vars
                    .iter()
                    .map(|(_, t)| *t)
                    .collect(),
            ),
            // 单结果签名可能直接是结果类型而不是一元组
            _ => Some(vec![results]),
        }
    }

    /// 取被调方的「结果类型 + 签名摘要 + 变参那一位的 slice 类型」。
    ///
    /// `drop_recv` 时 `params` 减一（方法：`Func.params[0]` 是接收者，
    /// 而 `CalleeRef::Method.recv` 是独立字段）。
    fn sig_of(&self, fid: FuncId, drop_recv: bool) -> (Vec<TypeId>, Sig, Option<TypeId>) {
        let f = &self.program.funcs[fid.index()];
        (
            f.result_types.clone(),
            Sig {
                params: f.params.len() - usize::from(drop_recv),
                results: f.result_types.len(),
                variadic: f.variadic,
            },
            if f.variadic {
                f.param_types.last().copied()
            } else {
                None
            },
        )
    }

    /// 变参打包（`Task 28`）：调用点没有 `...` 时，把多出来的实参装成一个 slice。
    ///
    /// # 为什么在**调用点**打包，而不是让被调方自己收
    ///
    /// 被调方的最后一个槽是 `[]T`（`Func.param_types` 的最后一项就是 slice 类型），
    /// 它的帧里只认一个值。所以「变参」在调用边界上必须变成一个 slice 实参 ——
    /// 这与 `rgoc-interp` 的做法一致（`T-EX-28` 的语义基准）。
    ///
    /// 打包用 `Composite { typ: 切片类型, items }`：它正是「用这些元素造一个复合值」，
    /// 与 `[]T{a, b, c}` 字面量同形，求值器不必为变参再造一条指令。
    fn pack_variadic(
        &mut self,
        mut avs: Vec<ValueId>,
        sig: &Sig,
        vty: Option<TypeId>,
        dots: bool,
    ) -> Vec<ValueId> {
        if !sig.variadic || dots {
            return avs;
        }
        let Some(slice_ty) = vty else { return avs };
        let fixed = sig.params.saturating_sub(1);
        let rest: Vec<ValueId> = if avs.len() > fixed {
            avs.split_off(fixed)
        } else {
            Vec::new()
        };
        let dst = self.new_value(slice_ty);
        self.emit(Instr::Composite {
            dst,
            typ: slice_ty,
            items: rest,
        });
        avs.push(dst);
        avs
    }

    /// 产 `Call` 指令：按结果类型建 `dst`，返回它们。`effect` 由 `Task 27` 的摘要给。
    fn emit_call(
        &mut self,
        callee: CalleeRef,
        args: Vec<ValueId>,
        rtypes: &[TypeId],
        effect: Effect,
    ) -> Vec<ValueId> {
        let mut dsts: Vec<ValueId> = Vec::with_capacity(rtypes.len());
        for t in rtypes {
            let d = self.new_value(*t);
            dsts.push(d);
        }
        self.emit(Instr::Call {
            dsts: dsts.clone(),
            callee,
            args,
            effect,
        });
        dsts
    }

    fn expr(&mut self, e: &Expr) -> ValueId {
        match &e.kind {
            ExprKind::Const(c) => {
                let dst = self.new_value(e.typ);
                self.emit(Instr::Const {
                    dst,
                    c: c.clone(),
                    typ: e.typ,
                });
                dst
            }
            // 有类型的 `nil`：`Int(0)` 在非整数类型上表示**该类型的零值**
            // （`R-20-1`：SSA 没有独立的 nil 常量，而 `ConstVal` 是 M5 契约的一部分，
            // 不为了 SSA 去加第 5 个变体）
            ExprKind::Nil => {
                let dst = self.new_value(e.typ);
                self.emit(Instr::Const {
                    dst,
                    c: rgoc_hir::types::ConstVal::Int(0),
                    typ: e.typ,
                });
                dst
            }
            ExprKind::Slot(s) => self.read_slot(*s),
            ExprKind::Global(g) => {
                let root = RootId(g.0);
                self.load_root(root, e.typ)
            }
            // 运算前把操作数对齐（`R-31-3`）：HIR 的操作数可能是**未定型常量**
            // （`a + 1` 里的 `1` 是 untyped int），而 `V3` 要求同型 —— 隐式转换
            // 在 lowering 里没有落成节点，只能在**使用点**物化（与 `coerce_to`
            // 在写入点做的事同源，`R-22-2`）。
            ExprKind::Binary { op, x, y } => {
                let mut lhs = self.expr(x);
                let mut rhs = self.expr(y);
                match op {
                    // 移位：只有左操作数要跟结果同型（计数可以是别的整数型）
                    BinOp::Shl | BinOp::Shr => {
                        lhs = self.coerce_to(e.typ, lhs);
                    }
                    // 比较与逻辑：结果是 `bool`，两侧只要**互相**同型
                    BinOp::Eq
                    | BinOp::Ne
                    | BinOp::Lt
                    | BinOp::Le
                    | BinOp::Gt
                    | BinOp::Ge
                    | BinOp::LAnd
                    | BinOp::LOr => {
                        let lt = self.values[lhs.index()];
                        rhs = self.coerce_to(lt, rhs);
                    }
                    // 算术 / 位运算：两侧都跟结果同型
                    _ => {
                        lhs = self.coerce_to(e.typ, lhs);
                        rhs = self.coerce_to(e.typ, rhs);
                    }
                }
                let dst = self.new_value(e.typ);
                self.emit(Instr::Bin {
                    dst,
                    op: *op,
                    lhs,
                    rhs,
                    typ: e.typ,
                });
                dst
            }
            ExprKind::Unary { op, x } => {
                let xv = self.expr(x);
                let xv = self.coerce_to(e.typ, xv);
                let dst = self.new_value(e.typ);
                self.emit(Instr::Un {
                    dst,
                    op: *op,
                    x: xv,
                    typ: e.typ,
                });
                dst
            }
            ExprKind::Convert { to, x } => {
                let xv = self.expr(x);
                let dst = self.new_value(*to);
                self.emit(Instr::Convert {
                    dst,
                    x: xv,
                    typ: *to,
                });
                dst
            }
            // `&x`：唯一产生指针的构造。`x` 一定在根表里（`scan_addressed` 保证）
            ExprKind::Addr(p) => self.place_addr(p),
            // `*p`：读指针指向的存储。`p` 是本函数 `&x` 造出来的 ⇒ 读**那个根**
            // （`T-SSA-25` 要的就是这条：写穿之后读 `x` 与 `*p` 走同一条 mem 链）。
            // 其余来源（形参 / 调用结果 / 合并值）根未知 ⇒ 堆根，M6 不做别名推理。
            ExprKind::Deref(x) => {
                let addr = self.expr(x);
                let root = self.ptr_hint(addr).unwrap_or_else(|| self.heap_root());
                let mem = self.mem_of(root);
                let t = e.typ;
                let dst = self.new_value(t);
                self.emit(Instr::Load {
                    dst,
                    mem,
                    root,
                    addr,
                    typ: t,
                });
                dst
            }
            // 读字段：先算字段地址，再 `Load`。底座有四种（槽 / 全局 / 指针 / 聚合值链）。
            ExprKind::Field { base, index } => {
                let Some((addr, hint)) = self.expr_addr_root(base) else {
                    todo!(
                        "M6/Task 30：字段底座不可取址（`{}` 不是地址形态）",
                        expr_kind_name(&base.kind)
                    );
                };
                let (root, mem) = match hint {
                    Some(r) => (r, self.mem_of(r)),
                    None => {
                        let r = self.heap_root();
                        (r, self.mem_of(r))
                    }
                };
                let at = self.values[addr.index()];
                let fa = self.new_value(at);
                self.emit(Instr::Field {
                    dst: fa,
                    base: addr,
                    index: *index,
                });
                let ty = e.typ;
                let dst = self.new_value(ty);
                self.emit(Instr::Load {
                    dst,
                    mem,
                    root,
                    addr: fa,
                    typ: ty,
                });
                dst
            }
            ExprKind::Call {
                callee,
                args,
                spread,
                variadic,
                ..
            } => {
                let vs = self.call(e, callee, args, *spread, *variadic);
                match vs.len() {
                    1 => vs[0],
                    0 => todo!("0 结果的调用不能当值使用（检查器应已拒绝）"),
                    n => todo!("{n} 个结果的调用出现在单值位置（检查器应已拒绝）"),
                }
            }
            ExprKind::Composite(c) => self.composite(c),
            // `a[i]`：数组元素在槽的存储里（走那个根），slice 元素在堆根上，
            // `p[i]`（`p *[N]T`）在指针背后。三种底座都由 `index_base` 归一，
            // 与 `a[i] = v` 的写路径**同一套**判据。
            ExprKind::Index { base, index, .. } => {
                let (base_ptr, hint) = self.index_base(base);
                let idx = self.expr(index);
                let root = hint.unwrap_or_else(|| self.heap_root());
                let pt = self.ptr_ty_of(root);
                let addr = self.new_value(pt);
                self.emit(Instr::Index {
                    dst: addr,
                    base: base_ptr,
                    i: idx,
                });
                let mem = self.mem_of(root);
                let t = e.typ;
                let dst = self.new_value(t);
                self.emit(Instr::Load {
                    dst,
                    mem,
                    root,
                    addr,
                    typ: t,
                });
                dst
            }
            ExprKind::Builtin { kind, args, .. } => {
                let vs = self.emit_builtin(e, *kind, args);
                match vs.len() {
                    1 => vs[0],
                    0 => todo!("0 结果的内建（`println` 等）不能当值使用"),
                    n => todo!("{n} 结果的内建出现在单值位置"),
                }
            }
            // `make([]T, len[, cap])`：`len` / `cap` 的求值顺序 == 源码顺序。
            // `make(map)` / `make(chan)` 到不了这里（拒绝面先拦）。
            ExprKind::Make { args, .. } => {
                let len = args.first().map(|a| self.expr(a));
                let cap = args.get(1).map(|a| self.expr(a));
                let dst = self.new_value(e.typ);
                self.emit(Instr::Make {
                    dst,
                    typ: e.typ,
                    len,
                    cap,
                });
                dst
            }
            // `new(T)`：`len` / `cap` 都为 `None` 且 `typ` 是指针（`Instr::Make` 的约定）
            ExprKind::New(_) => {
                let dst = self.new_value(e.typ);
                self.emit(Instr::Make {
                    dst,
                    typ: e.typ,
                    len: None,
                    cap: None,
                });
                dst
            }
            // `a[lo:hi:max]`：结果是新 slice 头，元素存储仍在底层数组（堆根）
            ExprKind::Slice {
                base, lo, hi, max, ..
            } => {
                let b = self.expr(base);
                let lo = lo.as_deref().map(|x| self.expr(x));
                let hi = hi.as_deref().map(|x| self.expr(x));
                let mx = max.as_deref().map(|x| self.expr(x));
                let dst = self.new_value(e.typ);
                self.emit(Instr::Slice {
                    dst,
                    base: b,
                    lo,
                    hi,
                    max: mx,
                });
                dst
            }
            // `&T{...}`：复合字面量是规范里唯一「不是位置却可寻址」的表达式
            // ⇒ 分配一块临时存储、把值存进去、返回它的地址。
            ExprKind::AddrOf(x) => {
                let v = self.expr(x);
                let root = self.temp_root(x.typ);
                let addr = self.addr_of(root);
                self.store_root(root, addr, v);
                addr
            }
            other => todo!(
                "M6/Task 27–31：表达式 `{}` 的构建还没实现",
                expr_kind_name(other)
            ),
        }
    }

    fn new_block(&mut self) -> BlockId {
        self.blocks.push(Block::default());
        BlockId((self.blocks.len() - 1) as u32)
    }

    /// `if` / `else` 与汇合块的块参数（`Task 22`）。
    fn stmt_if(&mut self, init: Option<&Stmt>, cond: &Expr, then: &HirBlock, els: Option<&Stmt>) {
        if let Some(i) = init {
            self.stmt(i);
        }
        let c = self.expr(cond);
        let then_b = self.new_block();
        let else_b = self.new_block();
        self.set_term(Term::If {
            cond: c,
            then_: (then_b, Vec::new()),
            else_: (else_b, Vec::new()),
        });

        let base = (self.val.clone(), self.mem.clone());
        let mut outs = Vec::new();

        // ── then 支
        self.cur = then_b;
        self.terminated = false;
        self.val = base.0.clone();
        self.mem = base.1.clone();
        self.stmts(&then.stmts);
        if !self.terminated {
            outs.push(BranchOut {
                block: self.cur,
                val: self.val.clone(),
                mem: self.mem.clone(),
            });
        }

        // ── else 支
        self.cur = else_b;
        self.terminated = false;
        self.val = base.0;
        self.mem = base.1;
        if let Some(e) = els {
            self.stmt(e);
        }
        if !self.terminated {
            outs.push(BranchOut {
                block: self.cur,
                val: self.val.clone(),
                mem: self.mem.clone(),
            });
        }

        self.join_branches(outs);
    }

    /// 三句式 `for`（`for{}` / `for cond{}` / `for init;cond;post{}`）与回边（`Task 23`）。
    ///
    /// # 块布局
    ///
    /// ```text
    ///   preheader:  ... init ...            （`init` 只跑一次）
    ///               goto header (carried…)
    ///   header:     params = [carried…]     ← 回边的目标，也是「不变式」
    ///               if cond -> body | exit
    ///   body:       ...
    ///               goto latch | header
    ///   latch:      ... post ...            （只有三句式才有；没有 `continue` 且体必终止时不建）
    ///               goto header (carried…)
    ///   exit:       ...                     （`for{}` 且没有 `break` 时不建）
    /// ```
    ///
    /// # header / latch / exit 共用**一套参数布局**
    ///
    /// 三者都是「跳转目标」，而跳转要送值。共用同一份 `ParamLayout`（槽在前、mem 在后）
    /// 让 `break` / `continue` / 回边都走同一个 `args_for`，不必各自记一套顺序。
    /// 代价是 `exit` 多几个参数（只有一个前驱时也算），换来的是「任何跳转都能算实参」。
    ///
    /// # 为什么 header 的参数必须在建 body **之前**定
    ///
    /// body 里读 `i` 必须读 header 的参数；如果等 body 建完再补参数，body 里
    /// 已经写死了循环前的 `ValueId`，只能回头重写一遍 —— 那是最容易漏的情形
    /// （漏一处就变成「循环里读的是初值」，而且大部分 fixture 仍绿）。
    /// 所以先 [`scan_loop_effects`] 静态算出会被改的东西。
    ///
    /// # 传哪些槽
    ///
    /// 「循环里被赋值 **且** 进入循环时已有当前值」的槽。第二个条件很重要：
    /// 循环体里 `x := 1` 声明的槽每轮都是新的，不能当循环携带值。
    ///
    /// # 传哪些 mem
    ///
    /// 循环里**可能被写**的根，各传一个 mem 令牌。这是 mem 的「phi」：
    /// 进循环时是循环前的令牌，回边是上一轮的令牌。不传的后果不是算错，
    /// 而是「循环后的写」与「循环里的写」在 mem 链上失去先后关系 ——
    /// 那是 `V6` 想保证的东西，等于给后续 pass 埋了一个静默的无序。
    ///
    /// # `break` / `continue` 的目标（`Task 25`）
    ///
    /// * `break` → `exit`（`for{}` 的 exit 是懒建的：没有 `break` 就一个死块都不建）
    /// * `continue` → `latch`（有 post 时）或 `header`（没有 post 时）。
    ///   `latch` 也是懒建的（体必终止且没有 `continue` 时，post 根本跑不到）。
    ///
    /// # `loopvars`（Go 1.22 per-iteration）不需要特判
    ///
    /// 那条语义只对**闭包捕获**可见（每轮换新 `Cell`），而 M6 在构建期就拒绝闭包
    /// （`SSA-UNSUPPORTED-CLOSURE`）⇒ 这里没有可观察差异。如实记进报告，不假装实现了。
    fn stmt_for(
        &mut self,
        init: Option<&Stmt>,
        cond: Option<&Expr>,
        post: Option<&Stmt>,
        body: &HirBlock,
        label: Option<&str>,
    ) {
        if let Some(i) = init {
            self.stmt(i);
        }

        // ── ① 静态算出「循环里会改什么」，据此定 header 的参数
        let eff = scan_loop_effects(self.tt, &self.slot_root, body, post);
        let carried: Vec<SlotId> = eff
            .slots
            .iter()
            .copied()
            .filter(|s| self.val.contains_key(s))
            .collect();
        let mut wroots: BTreeSet<RootId> = eff.roots.clone();
        if eff.calls {
            for i in 0..self.globals.len() {
                wroots.insert(RootId(i as u32));
            }
        }
        if eff.heap || eff.calls {
            let h = self.heap_root();
            wroots.insert(h);
        }
        let layout = ParamLayout {
            slots: carried.clone(),
            roots: wroots.into_iter().collect(),
        };

        let base = (self.val.clone(), self.mem.clone());
        let header = self.new_block();
        let hstate = self.layout_params(header, &layout);
        let pre_args = self.args_for(self.cur, &layout, &base.0, &base.1);
        self.set_term(Term::Goto {
            to: header,
            args: pre_args,
        });

        // ── ② header 的入口状态 = 循环前的状态，但携带项换成 header 自己的参数
        self.cur = header;
        self.terminated = false;
        let hs = Self::state_with(&base, (hstate.0.clone(), hstate.1.clone()));
        self.val = hs.0.clone();
        self.mem = hs.1.clone();

        // ── ③ 出口块：有条件时必有（cond 假直接出去）；`for{}` 时靠 break 才需要
        let body_b = self.new_block();
        let mut exit_pre: Option<(BlockId, BlockState)> = None;
        if cond.is_some() {
            let e = self.new_block();
            let st = self.layout_params(e, &layout);
            exit_pre = Some((e, st));
        }

        // ── ④ 条件（在 header 里求值）
        match cond {
            Some(c) => {
                let cv = self.expr(c);
                let e = exit_pre.as_ref().expect("有 cond 就有 exit").0;
                let pass = layout
                    .slots
                    .iter()
                    .map(|s| hstate.0[s])
                    .chain(layout.roots.iter().map(|r| hstate.1[r]))
                    .collect();
                self.set_term(Term::If {
                    cond: cv,
                    then_: (body_b, Vec::new()),
                    else_: (e, pass),
                });
            }
            None => {
                self.set_term(Term::Goto {
                    to: body_b,
                    args: Vec::new(),
                });
            }
        }

        // ── ⑤ 循环体（把 region 压栈，`break`/`continue` 才知道目标）
        let idx = self.regions.len();
        self.regions.push(Region::Loop(LoopCtx {
            label: label.map(ToString::to_string),
            layout: layout.clone(),
            break_target: exit_pre.as_ref().map(|(b, _)| *b),
            break_pending: Vec::new(),
            continues: Vec::new(),
        }));
        let state = (self.val.clone(), self.mem.clone());
        self.cur = body_b;
        self.terminated = false;
        self.val = state.0;
        self.mem = state.1;
        self.stmts(&body.stmts);
        let body_falls = !self.terminated;
        let Region::Loop(ctx) = self.regions.remove(idx) else {
            unreachable!("刚压进去的是 Loop")
        };

        // ── ⑥ latch（有 post 时）：体落空或有 `continue` 才需要
        let need_latch = post.is_some() && (body_falls || !ctx.continues.is_empty());
        let mut latch: Option<(BlockId, BlockState)> = None;
        let cont_target = if need_latch {
            let l = self.new_block();
            let st = self.layout_params(l, &layout);
            latch = Some((l, st));
            l
        } else {
            header
        };
        // 补 `continue` 的实参（它们在建 body 时还不知道目标是 latch 还是 header）
        for c in &ctx.continues {
            let args = self.args_for(c.block, &layout, &c.val, &c.mem);
            self.blocks[c.block.index()].term = Term::Goto {
                to: cont_target,
                args,
            };
        }
        if body_falls {
            let args = self.args_for(self.cur, &layout, &self.val.clone(), &self.mem.clone());
            self.set_term(Term::Goto {
                to: cont_target,
                args,
            });
        }

        // ── ⑦ post（在 latch 里）
        if let Some((l, st)) = latch {
            self.cur = l;
            self.terminated = false;
            let ls = Self::state_with(&base, st);
            self.val = ls.0;
            self.mem = ls.1;
            if let Some(p) = post {
                self.stmt(p);
            }
            if !self.terminated {
                let args = self.args_for(self.cur, &layout, &self.val.clone(), &self.mem.clone());
                self.set_term(Term::Goto { to: header, args });
            }
        }

        // ── ⑧ 出口：`for{}` 的 exit 到这一步才知道要不要建
        let exit_final = match exit_pre {
            Some((b, st)) => Some((b, st)),
            None => {
                if ctx.break_pending.is_empty() {
                    None
                } else {
                    let b = self.new_block();
                    let st = self.layout_params(b, &layout);
                    Some((b, st))
                }
            }
        };
        if !ctx.break_pending.is_empty() {
            let b = exit_final.as_ref().expect("有 break 就有 exit").0;
            for out in &ctx.break_pending {
                let args = self.args_for(out.block, &layout, &out.val, &out.mem);
                self.blocks[out.block.index()].term = Term::Goto { to: b, args };
            }
        }

        match exit_final {
            Some((b, st)) => {
                self.cur = b;
                self.terminated = false;
                let es = Self::state_with(&base, st);
                self.val = es.0;
                self.mem = es.1;
            }
            None => self.terminated = true,
        }
    }

    /// 表达式 `switch`（`Task 24`）。
    ///
    /// # 树形，不是链
    ///
    /// ```text
    ///   pre:      ...init... tag = eval(tag)
    ///             if tag==c0a || tag==c0b -> body0 | if tag==c1 -> body1 | ... | default
    ///   body_i:   ...   -> join   （或 fallthrough 到 body_{i+1}）
    ///   nomatch:  -> join          （**只有没有 default 时才存在**）
    ///   join:     params = [合流值…]
    /// ```
    ///
    /// # 为什么「没有 default」要多一条 `nomatch` 分支
    ///
    /// 「一条都没匹配」也是一条真实控制流：它落到 `switch` 之后，且看不到任何
    /// case 体里的赋值。若不把它当作一条分支参与合流，合流值就会变成
    /// 「某个 case 体里的值」—— 于是 `switch x { case 1: y = 2 }; print(y)`
    /// 在 `x != 1` 时会打出 2。这类错**能过 verifier**（类型、支配都对）。
    ///
    /// # `fallthrough` 不走判定链
    ///
    /// 它的语义是「直接进入下一个 case 的体」（**不**重测下一个 case 的表达式），
    /// 所以目标是 `bodies[i+1]`，而不是下一个测试块。
    ///
    /// # `break` 是「跳进合流点的一条分支」
    ///
    /// case 体里的 `break` 与「体落空」在结构上是同一件事：都是一条通往 `join` 的边。
    /// 所以 `break` 只记一条 `BranchOut`，由 `join_branches` 统一补实参与块参数。
    fn stmt_switch(
        &mut self,
        init: Option<&Stmt>,
        tag: Option<&Expr>,
        cases: &[Case],
        label: Option<&str>,
    ) {
        if let Some(i) = init {
            self.stmt(i);
        }
        let tagv = tag.map(|t| self.expr(t));
        let entry = (self.cur, self.val.clone(), self.mem.clone());

        // 预建全部 case 体块：`fallthrough` 要指向「下一个 case 的体」，
        // 而那时下一个体可能还没建。
        let bodies: Vec<BlockId> = cases.iter().map(|_| self.new_block()).collect();
        let default_idx = cases.iter().position(|c| c.is_default);
        let nomatch = if default_idx.is_none() && !cases.is_empty() {
            Some(self.new_block())
        } else {
            None
        };

        // ── 各 case 体：**都从入口状态起建**（case 之间没有落空关系）
        let idx = self.regions.len();
        self.regions.push(Region::Switch(SwitchCtx {
            label: label.map(ToString::to_string),
            breaks: Vec::new(),
        }));
        let mut outs: Vec<BranchOut> = Vec::new();
        for (i, c) in cases.iter().enumerate() {
            self.cur = bodies[i];
            self.terminated = false;
            self.val = entry.1.clone();
            self.mem = entry.2.clone();
            self.fallthrough = bodies.get(i + 1).copied();
            self.stmts(&c.body.stmts);
            self.fallthrough = None;
            if !self.terminated {
                outs.push(BranchOut {
                    block: self.cur,
                    val: self.val.clone(),
                    mem: self.mem.clone(),
                });
            }
        }
        if let Some(nb) = nomatch {
            self.cur = nb;
            self.terminated = false;
            self.val = entry.1.clone();
            self.mem = entry.2.clone();
            outs.push(BranchOut {
                block: nb,
                val: self.val.clone(),
                mem: self.mem.clone(),
            });
        }
        let Region::Switch(ctx) = self.regions.remove(idx) else {
            unreachable!("刚压进去的是 Switch")
        };
        outs.extend(ctx.breaks);

        // ── 合流点
        let join = self.join_branches(outs);
        let merged = (self.val.clone(), self.mem.clone());

        // ── 判定链：从入口块起，逐 case 生成 `if`
        self.cur = entry.0;
        self.terminated = false;
        self.val = entry.1;
        self.mem = entry.2;
        let fallback = default_idx.map(|i| bodies[i]).or(nomatch);
        let tested: Vec<usize> = (0..cases.len()).filter(|i| !cases[*i].is_default).collect();
        if tested.is_empty() {
            // 只有 `default`（或一个 case 都没有）：直接进 default 体
            if let Some(b) = fallback {
                self.set_term(Term::Goto {
                    to: b,
                    args: Vec::new(),
                });
            }
        } else {
            let mut chain = entry.0;
            let n = tested.len();
            for (k, &i) in tested.iter().enumerate() {
                let last = k + 1 == n;
                let no = if last {
                    fallback.expect("没有 default 时一定有 nomatch")
                } else {
                    self.new_block()
                };
                self.cur = chain;
                let bt = self.bool_ty();
                let mut cond: Option<ValueId> = None;
                for e in &cases[i].exprs {
                    let ev = self.expr(e);
                    let ev = match tagv {
                        // 有 tag：`tag == e`（两侧类型对齐，`untyped` 常量要物化转换）
                        Some(t) => {
                            let tt = self.values[t.index()];
                            let ev = self.coerce_to(tt, ev);
                            let dst = self.new_value(bt);
                            self.emit(Instr::Bin {
                                dst,
                                op: rgoc_hir::BinOp::Eq,
                                lhs: t,
                                rhs: ev,
                                typ: bt,
                            });
                            dst
                        }
                        // 无 tag：表达式本身就是条件（`switch { case x > 0: }`）
                        None => self.coerce_to(bt, ev),
                    };
                    cond = Some(match cond {
                        None => ev,
                        Some(p) => {
                            let dst = self.new_value(bt);
                            self.emit(Instr::Bin {
                                dst,
                                op: rgoc_hir::BinOp::LOr,
                                lhs: p,
                                rhs: ev,
                                typ: bt,
                            });
                            dst
                        }
                    });
                }
                let cond = cond.expect("非 default 的 case 至少有一个表达式");
                self.set_term(Term::If {
                    cond,
                    then_: (bodies[i], Vec::new()),
                    else_: (no, Vec::new()),
                });
                if !last {
                    chain = no;
                }
            }
        }

        // ── 之后
        match join {
            Some(j) => {
                self.cur = j;
                self.terminated = false;
                self.val = merged.0;
                self.mem = merged.1;
            }
            None => self.terminated = true,
        }
    }

    /// `range` 循环（`Task 28` 并入；计划里原先没有这一项）。
    ///
    /// # 为什么它必须并入 Task 28
    ///
    /// `func sum(xs ...int)` 的语义基准就是「用 `range` 把变参走一遍」——
    /// 没有 `range` 连变参打包都验不了。计划把它漏了（`R-28-3`）。
    ///
    /// # 去糖成「下标循环」
    ///
    /// ```text
    ///   idx = 0
    ///   header: if idx < len(xs) -> body | exit
    ///   body:   value = xs[idx]; ...body...
    ///           idx = idx + 1; goto header
    /// ```
    ///
    /// 下标放在**槽**里（HIR 的 key 位，或构建器私有槽 `fresh_slot`），
    /// 于是它天然成为循环携带值，复用 `stmt_for` 那一套布局与回边。
    ///
    /// # `len` 用 `Call Builtin(Len)` 表达
    ///
    /// `Instr` 里没有 `Len` 指令。用内建调用表达它有两个好处：求值器只认
    /// 一种「取长度」的形态，且 `effect` 天然是「纯读」。
    ///
    /// # 支持面
    ///
    /// `Array` / `Slice` / `Int`（`for i := range 10`）。
    /// **`Str` 与 `Map` 不在 M6 面**：字符串要按 rune 解码（当前指令集里没有
    /// 能表达「按 rune 取第 i 个」的操作），map 的键相等是宿主语义
    /// （`SSA-UNSUPPORTED-MAP`）。两者都**显式拒绝**，不静默降级。
    fn stmt_range(
        &mut self,
        key: Option<&Place>,
        value: Option<&Place>,
        x: &Expr,
        body: &HirBlock,
        kind: RangeKind,
        label: Option<&str>,
    ) {
        // `Str` / `Map` 到不了这里：`unsupported()` 在**建图前**就把它们拒了
        // （`SSA-UNSUPPORTED-RANGE-STR` / `-MAP`）。留 `unreachable!` 不用 `todo!`：
        // 「预扫描漏了」是实现 bug，不是「还没实现」。
        match kind {
            RangeKind::Array | RangeKind::Slice | RangeKind::Int => {}
            RangeKind::Str | RangeKind::Map => {
                unreachable!("range over str/map 应由 unsupported() 在建图前拒掉")
            }
        }
        let xs = self.expr(x);
        let int_ty = self.int_ty();

        // 下标槽：优先用 HIR 的 key；`_` 或没有 key 时用私有槽。
        let key_slot = match key {
            Some(Place::Slot(s)) if *s != BLANK_SLOT => *s,
            _ => self.fresh_slot(int_ty),
        };
        // 下标槽的类型**可能就是具名类型**：`for i := range MyInt(4)` 的 `i` 是
        // `MyInt`（`range3.go` 实测）。拿 `int` 去比它就成了一条
        // `lt MyInt int`，`V3` 拒。
        let key_ty = self.slot_typ(key_slot);

        // ── 初始化 idx = 0（在 preheader 里）
        let zero = self.const_int(0, key_ty);
        self.write_slot(key_slot, zero);

        let base = (self.val.clone(), self.mem.clone());

        // 循环体的副作用 + 下标 + value 槽
        let mut eff = scan_loop_effects(self.tt, &self.slot_root, body, None);
        eff.slots.insert(key_slot);
        let val_slot: Option<SlotId> = match value {
            Some(Place::Slot(s)) if *s != BLANK_SLOT => Some(*s),
            _ => None,
        };
        if let Some(s) = val_slot {
            eff.slots.insert(s);
        }
        let carried: Vec<SlotId> = eff
            .slots
            .iter()
            .copied()
            .filter(|s| self.val.contains_key(s))
            .collect();
        let mut wroots: BTreeSet<RootId> = eff.roots.clone();
        if eff.calls {
            for i in 0..self.globals.len() {
                wroots.insert(RootId(i as u32));
            }
        }
        if eff.heap || eff.calls {
            let h = self.heap_root();
            wroots.insert(h);
        }
        let layout = ParamLayout {
            slots: carried,
            roots: wroots.into_iter().collect(),
        };

        // ── header
        let header = self.new_block();
        let hstate = self.layout_params(header, &layout);
        let pre_args = self.args_for(self.cur, &layout, &base.0, &base.1);
        self.set_term(Term::Goto {
            to: header,
            args: pre_args,
        });
        self.cur = header;
        self.terminated = false;
        let hs = Self::state_with(&base, (hstate.0.clone(), hstate.1.clone()));
        self.val = hs.0.clone();
        self.mem = hs.1.clone();

        // cond: idx < len(xs)（`range n` 时上界直接是 n）
        let up = if matches!(kind, RangeKind::Int) {
            // `range 4` 的上界是**未定型常量**（`untyped int`），而下标槽是 `int`。
            // 不对齐的话这里就是一条 `lt int untyped-int`，`V3` 当场拒
            // （实测 `range3.go` / `turing.go`）。
            self.coerce_to(key_ty, xs)
        } else {
            let n = self.emit_len(xs);
            self.coerce_to(key_ty, n)
        };
        let idx_v = self.read_slot(key_slot);
        let bt = self.bool_ty();
        let cond = self.new_value(bt);
        self.emit(Instr::Bin {
            dst: cond,
            op: rgoc_hir::BinOp::Lt,
            lhs: idx_v,
            rhs: up,
            typ: bt,
        });

        let body_b = self.new_block();
        let exit_b = self.new_block();
        let exit_state = self.layout_params(exit_b, &layout);
        let pass = layout
            .slots
            .iter()
            .map(|s| hstate.0[s])
            .chain(layout.roots.iter().map(|r| hstate.1[r]))
            .collect();
        self.set_term(Term::If {
            cond,
            then_: (body_b, Vec::new()),
            else_: (exit_b, pass),
        });

        // ── body
        let idx = self.regions.len();
        self.regions.push(Region::Loop(LoopCtx {
            label: label.map(ToString::to_string),
            layout: layout.clone(),
            break_target: Some(exit_b),
            break_pending: Vec::new(),
            continues: Vec::new(),
        }));
        let bstate = (self.val.clone(), self.mem.clone());
        self.cur = body_b;
        self.terminated = false;
        self.val = bstate.0;
        self.mem = bstate.1;
        // value = xs[idx]
        if let (Some(p), false) = (value, matches!(kind, RangeKind::Int)) {
            let cur_idx = self.read_slot(key_slot);
            let vt = match val_slot {
                Some(s) => self.slot_typ(s),
                None => TypeId::from_index(0),
            };
            let elem = self.range_elem(xs, x, cur_idx, vt, kind);
            self.write_place(p, elem);
        }
        self.stmts(&body.stmts);
        let body_falls = !self.terminated;
        let Region::Loop(ctx) = self.regions.remove(idx) else {
            unreachable!("刚压进去的是 Loop")
        };

        // ── latch：idx = idx + 1
        let need_latch = body_falls || !ctx.continues.is_empty();
        let mut latch: Option<BlockId> = None;
        let cont_target = if need_latch {
            let l = self.new_block();
            latch = Some(l);
            l
        } else {
            header
        };
        for c in &ctx.continues {
            let args = self.args_for(c.block, &layout, &c.val, &c.mem);
            self.blocks[c.block.index()].term = Term::Goto {
                to: cont_target,
                args,
            };
        }
        if body_falls {
            let tail = self.cur;
            let args_state = (self.val.clone(), self.mem.clone());
            if let Some(l) = latch {
                self.blocks[tail.index()].term = Term::Goto {
                    to: l,
                    args: Vec::new(),
                };
                // latch 的入口状态 = 体落空处的状态（值都支配 latch）
                self.cur = l;
                self.terminated = false;
                self.val = args_state.0;
                self.mem = args_state.1;
                let cur_idx = self.read_slot(key_slot);
                // 自增也要同型（`key_ty` 可能是具名类型）
                let one = self.const_int(1, key_ty);
                let inc = self.new_value(key_ty);
                self.emit(Instr::Bin {
                    dst: inc,
                    op: rgoc_hir::BinOp::Add,
                    lhs: cur_idx,
                    rhs: one,
                    typ: key_ty,
                });
                self.write_slot(key_slot, inc);
                let args = self.args_for(l, &layout, &self.val.clone(), &self.mem.clone());
                self.set_term(Term::Goto { to: header, args });
            } else {
                let args = self.args_for(tail, &layout, &args_state.0, &args_state.1);
                self.set_term(Term::Goto {
                    to: cont_target,
                    args,
                });
            }
        }

        // ── 出口
        self.cur = exit_b;
        self.terminated = false;
        let es = Self::state_with(&base, exit_state);
        self.val = es.0;
        self.mem = es.1;
    }

    /// `range` 里取第 `i` 个元素的值。
    ///
    /// 数组：元素在槽的存储里 ⇒ 从 `AddrOf(root)` 起算地址；
    /// slice：元素在底层数组（堆根）⇒ 直接以 slice 值为 base。
    /// 两者都靠 `Index` 出地址、`Load` 取值 —— 与 `a[i]` 的读法完全一致，
    /// 求值器不必为 `range` 单独写一条路径。
    fn range_elem(
        &mut self,
        xs: ValueId,
        x: &Expr,
        i: ValueId,
        vt: TypeId,
        kind: RangeKind,
    ) -> ValueId {
        // 数组：元素在槽的存储里 ⇒ 从 `AddrOf(root)` 起算；slice：在堆根上。
        // 注意 `x` 只用来**判断**底座形态，不再求值 —— 求值会重复执行副作用
        // （`range f()` 只该调一次）。
        let (base, root) = match (kind, &x.kind) {
            (RangeKind::Array, ExprKind::Slot(s)) if self.slot_root.contains_key(s) => {
                let r = self.slot_root[s];
                (self.addr_of(r), r)
            }
            _ => (xs, self.heap_root()),
        };
        let pt = self.ptr_ty_of(root);
        let addr = self.new_value(pt);
        self.emit(Instr::Index { dst: addr, base, i });
        let mem = self.mem_of(root);
        let dst = self.new_value(vt);
        self.emit(Instr::Load {
            dst,
            mem,
            root,
            addr,
            typ: vt,
        });
        dst
    }

    /// 建一条 `len(x)`（用内建调用表达，见 `stmt_range` 的说明）。
    fn emit_len(&mut self, xs: ValueId) -> ValueId {
        let t = self.int_ty();
        let dst = self.new_value(t);
        let eff = crate::effect::to_effect(&crate::effect::builtin_summary(BuiltinKind::Len));
        self.emit(Instr::Call {
            dsts: vec![dst],
            callee: CalleeRef::Builtin(BuiltinKind::Len),
            args: vec![xs],
            effect: eff,
        });
        dst
    }

    /// 一个小整数常量（类型给定）。
    fn const_int(&mut self, n: i128, t: TypeId) -> ValueId {
        let dst = self.new_value(t);
        self.emit(Instr::Const {
            dst,
            c: rgoc_hir::types::ConstVal::Int(n),
            typ: t,
        });
        dst
    }

    /// 带标签的普通语句（`L: x = 1`）—— `goto` 的唯一合法目标（`Task 25`）。
    ///
    /// `M5` 的 lowering 已经把 `L: for/switch` 的标签转交给循环/`switch` 自己的
    /// `label` 字段，所以这里只会收到**非循环**的标签。
    ///
    /// # 前向 `goto` 靠「先占块、后补参数」
    ///
    /// `goto L` 出现在 `L:` 之前时，目标块的参数还不知道有哪些 —— 它取决于
    /// 有几条边进来、各自带什么状态。所以先建一个空块占位、记下跳转时的状态，
    /// 等真正走到 `L:` 时再用 `join_into` 一次性定参数并回填所有边。
    ///
    /// 反向 `goto`（标签在后面已经建好）直接按布局送实参，不走这条路径。
    fn stmt_labeled(&mut self, name: &str, inner: &Stmt) {
        let b = match self.labels.get(name).and_then(|x| x.block) {
            Some(b) => b,
            None => {
                let nb = self.new_block();
                self.labels.insert(
                    name.to_string(),
                    LabelSite {
                        block: Some(nb),
                        incoming: Vec::new(),
                        layout: None,
                    },
                );
                nb
            }
        };
        let mut outs: Vec<BranchOut> = self
            .labels
            .get_mut(name)
            .map(|x| std::mem::take(&mut x.incoming))
            .unwrap_or_default();
        if !self.terminated {
            outs.push(BranchOut {
                block: self.cur,
                val: self.val.clone(),
                mem: self.mem.clone(),
            });
        }
        if outs.is_empty() {
            // 既不可达、也没人跳进来 ⇒ 整段都是死代码。**不建块**（死块会被 `V8` 拒）。
            // 已知边界：此后若有 `goto` 指向它，会报「标签不可达」（语料里没有这种写法）。
            self.terminated = true;
            return;
        }
        let (layout, val, mem) = if self.goto_mode {
            // 固定布局：**全部根**、没有槽（goto 模式下没有槽被提升）。
            // 反向 `goto` 与这里的入边都用同一份布局，谁都不会漏送。
            let layout = ParamLayout {
                slots: Vec::new(),
                roots: (0..self.roots.len()).map(|i| RootId(i as u32)).collect(),
            };
            let (v, m) = self.layout_params(b, &layout);
            for o in &outs {
                let args = self.args_for(o.block, &layout, &o.val, &o.mem);
                self.blocks[o.block.index()].term = Term::Goto { to: b, args };
            }
            (layout, v, m)
        } else {
            self.join_into(b, outs)
        };
        if let Some(x) = self.labels.get_mut(name) {
            x.layout = Some(layout);
        }
        self.cur = b;
        self.terminated = false;
        self.val = val;
        self.mem = mem;
        self.stmt(inner);
    }

    /// `break [L]`。
    fn do_break(&mut self, label: Option<&str>) {
        let Some(idx) = self.find_region(label, false) else {
            todo!("M6/Task 25：`break` 没有可跳的目标（标签 {label:?}）")
        };
        let target = match &self.regions[idx] {
            Region::Loop(l) => l.break_target,
            Region::Switch(_) => None,
        };
        match &self.regions[idx] {
            Region::Switch(_) => {
                let out = BranchOut {
                    block: self.cur,
                    val: self.val.clone(),
                    mem: self.mem.clone(),
                };
                if let Region::Switch(s) = &mut self.regions[idx] {
                    s.breaks.push(out);
                }
                self.terminated = true;
            }
            Region::Loop(l) => match target {
                Some(b) => {
                    let layout = l.layout.clone();
                    let args =
                        self.args_for(self.cur, &layout, &self.val.clone(), &self.mem.clone());
                    self.set_term(Term::Goto { to: b, args });
                }
                None => {
                    let out = BranchOut {
                        block: self.cur,
                        val: self.val.clone(),
                        mem: self.mem.clone(),
                    };
                    if let Region::Loop(l) = &mut self.regions[idx] {
                        l.break_pending.push(out);
                    }
                    self.terminated = true;
                }
            },
        }
    }

    /// `continue [L]`。目标（`latch` 还是 `header`）要到循环收尾才知道，所以**总是**记下来后补。
    fn do_continue(&mut self, label: Option<&str>) {
        let Some(idx) = self.find_region(label, true) else {
            todo!("M6/Task 25：`continue` 没有可跳的循环（标签 {label:?}）")
        };
        let out = BranchOut {
            block: self.cur,
            val: self.val.clone(),
            mem: self.mem.clone(),
        };
        if let Region::Loop(l) = &mut self.regions[idx] {
            l.continues.push(out);
        }
        self.terminated = true;
    }

    /// `goto L`。
    fn do_goto(&mut self, name: &str) {
        // 标签可能还没出现（前向 `goto`）：先给它占个位置。
        if !self.labels.contains_key(name) {
            self.labels.insert(
                name.to_string(),
                LabelSite {
                    block: None,
                    incoming: Vec::new(),
                    layout: None,
                },
            );
        }
        let layout = self.labels.get(name).and_then(|x| x.layout.clone());
        if let Some(layout) = layout {
            // 反向 goto：目标已建好，直接按布局送实参（含自环）
            let b = self
                .labels
                .get(name)
                .and_then(|x| x.block)
                .expect("有 layout 就一定有块");
            let args = self.args_for(self.cur, &layout, &self.val.clone(), &self.mem.clone());
            self.set_term(Term::Goto { to: b, args });
            return;
        }
        let b = match self.labels.get(name).and_then(|x| x.block) {
            Some(b) => b,
            None => {
                let nb = self.new_block();
                if let Some(x) = self.labels.get_mut(name) {
                    x.block = Some(nb);
                }
                nb
            }
        };
        let out = BranchOut {
            block: self.cur,
            val: self.val.clone(),
            mem: self.mem.clone(),
        };
        // 先落终止符（实参等 `L:` 处定完布局再补）
        self.set_term(Term::Goto {
            to: b,
            args: Vec::new(),
        });
        if let Some(x) = self.labels.get_mut(name) {
            x.incoming.push(out);
        }
    }

    /// 找最内层的 region（`loop_only` 时只看循环）。
    fn find_region(&self, label: Option<&str>, loop_only: bool) -> Option<usize> {
        for (i, r) in self.regions.iter().enumerate().rev() {
            let (lab, is_loop) = match r {
                Region::Loop(l) => (l.label.as_deref(), true),
                Region::Switch(s) => (s.label.as_deref(), false),
            };
            match label {
                Some(want) => {
                    if lab == Some(want) && (!loop_only || is_loop) {
                        return Some(i);
                    }
                }
                None => {
                    if !loop_only || is_loop {
                        return Some(i);
                    }
                }
            }
        }
        None
    }

    /// 把若干条落空的分支合到**新建的汇合块**上（`Task 22` 的两路与 `Task 24` 的 N 路共用）。
    ///
    /// `outs` 为空 ⇒ 全部终止 ⇒ 不建块（`V8` 会拒死块）。
    fn join_branches(&mut self, outs: Vec<BranchOut>) -> Option<BlockId> {
        if outs.is_empty() {
            self.terminated = true;
            return None;
        }
        let join = self.new_block();
        let (_, val, mem) = self.join_into(join, outs);
        self.cur = join;
        self.terminated = false;
        self.val = val;
        self.mem = mem;
        Some(join)
    }

    /// 把若干条分支合到**已有的**块 `join` 上：定块参数、回填每条边的实参，返回布局与合流状态。
    ///
    /// # 两块内容都要合：值与 **mem**
    ///
    /// 只合值、把 mem 留在「最后建的那条分支」上，是 M6 最隐蔽的一类错：
    /// `V2`/`V6` 当时都**绿**（旧 mem 同样支配、同块也不乱序），但它把另一条分支里
    /// 的 `Store` 从 mem 链上摘掉了 —— 之后的写会被当成「与它无序」。
    /// 实测：`if c { *p = 1 }; return *p` 在只合值时会被 `V6` 抓（合流块里的 `Load`
    /// 用的 mem 不支配），一旦 mem 也被合上就绿。判据是
    /// `t_ssa_07_if_之后_mem_必须合流`（断言 load 的 mem 定义在汇合块上）。
    fn join_into(
        &mut self,
        join: BlockId,
        outs: Vec<BranchOut>,
    ) -> (
        ParamLayout,
        BTreeMap<SlotId, ValueId>,
        BTreeMap<RootId, ValueId>,
    ) {
        let mut layout = ParamLayout::default();
        let mut params: Vec<ValueId> = Vec::new();
        let mut args: Vec<Vec<ValueId>> = vec![Vec::new(); outs.len()];
        let mut merged_val: BTreeMap<SlotId, ValueId> = BTreeMap::new();
        let mut merged_mem: BTreeMap<RootId, ValueId> = BTreeMap::new();

        // ── 槽
        let mut slots: BTreeSet<SlotId> = BTreeSet::new();
        for o in &outs {
            slots.extend(o.val.keys().copied());
        }
        for s in slots {
            let vs: Vec<Option<ValueId>> = outs.iter().map(|o| o.val.get(&s).copied()).collect();
            let Some(first) = vs[0] else {
                // 第一条分支上就没有 ⇒ 这个槽只在别的分支里活着，不能当汇合值
                continue;
            };
            if vs.iter().any(|v| v.is_none()) {
                // 有分支缺它 ⇒ 不合并（后续读它必然已被判成「读前未写」⇒ 走内存）
                continue;
            }
            if vs.iter().all(|v| *v == Some(first)) {
                merged_val.insert(s, first);
            } else {
                let t = self.values[first.index()];
                let p = self.new_value(t);
                params.push(p);
                layout.slots.push(s);
                for (k, v) in vs.iter().enumerate() {
                    args[k].push(v.expect("上面已判过都是 Some"));
                }
                merged_val.insert(s, p);
            }
        }

        // ── mem
        let mut roots: BTreeSet<RootId> = BTreeSet::new();
        for o in &outs {
            roots.extend(o.mem.keys().copied());
        }
        for r in roots {
            let mut vs: Vec<ValueId> = Vec::new();
            for o in &outs {
                match o.mem.get(&r) {
                    Some(v) => vs.push(*v),
                    // 这条分支没碰过这个根 ⇒ 在**它自己的块里**补一条种子。
                    // 语义上等于「本分支对该根没有任何写」—— 与「入口状态」等价，
                    // 所以这不是编一个值，而是把缺失的初态显式化。
                    None => {
                        let v = self.seed_mem_in(o.block, r);
                        vs.push(v);
                    }
                }
            }
            let first = vs[0];
            if vs.iter().all(|v| *v == first) {
                merged_mem.insert(r, first);
            } else {
                let t = self.ptr_ty_of(r);
                let p = self.new_value(t);
                params.push(p);
                layout.roots.push(r);
                for (k, v) in vs.iter().enumerate() {
                    args[k].push(*v);
                }
                merged_mem.insert(r, p);
            }
        }

        self.blocks[join.index()].params = params;
        for (k, o) in outs.iter().enumerate() {
            debug_assert!(
                matches!(self.blocks[o.block.index()].term, Term::Unreachable),
                "落空的分支不该已有终止符（`Unreachable` 是默认值，表示「还没设」）"
            );
            self.blocks[o.block.index()].term = Term::Goto {
                to: join,
                args: std::mem::take(&mut args[k]),
            };
        }
        (layout, merged_val, merged_mem)
    }

    /// 按布局给块建参数，返回「参数 → 状态」的映射。
    fn layout_params(
        &mut self,
        b: BlockId,
        layout: &ParamLayout,
    ) -> (BTreeMap<SlotId, ValueId>, BTreeMap<RootId, ValueId>) {
        let mut params: Vec<ValueId> = Vec::new();
        let mut val: BTreeMap<SlotId, ValueId> = BTreeMap::new();
        let mut mem: BTreeMap<RootId, ValueId> = BTreeMap::new();
        for s in &layout.slots {
            let t = self.slot_typ(*s);
            let p = self.new_value(t);
            params.push(p);
            val.insert(*s, p);
        }
        for r in &layout.roots {
            let t = self.ptr_ty_of(*r);
            let p = self.new_value(t);
            params.push(p);
            mem.insert(*r, p);
        }
        self.blocks[b.index()].params = params;
        (val, mem)
    }

    /// 「循环前状态 + 参数覆盖」。
    ///
    /// # 为什么不能只用参数当状态
    ///
    /// 参数只覆盖**携带项**（槽与 mem 根）；其余值（形参、循环不变量）在
    /// `header` / `latch` / `exit` 里同样可见，而它们的 `ValueId` 就是循环前那个。
    /// 初版直接把 `self.val` 赋成「只有参数」，于是体内读一个未被循环修改的形参
    /// （`*p = i` 里的 `p`）就 panic 了 —— 变异测不出，只有真跑语料才露。
    fn state_with(base: &BlockState, over: BlockState) -> BlockState {
        let mut v = base.0.clone();
        for (k, x) in over.0 {
            v.insert(k, x);
        }
        let mut m = base.1.clone();
        for (k, x) in over.1 {
            m.insert(k, x);
        }
        (v, m)
    }

    /// 按布局算一条边的实参。`at` 是**来自哪个块**（缺 mem 时要在那个块里补种子）。
    fn args_for(
        &mut self,
        at: BlockId,
        layout: &ParamLayout,
        val: &BTreeMap<SlotId, ValueId>,
        mem: &BTreeMap<RootId, ValueId>,
    ) -> Vec<ValueId> {
        let mut out: Vec<ValueId> = Vec::with_capacity(layout.slots.len() + layout.roots.len());
        for s in &layout.slots {
            out.push(
                *val.get(s)
                    .unwrap_or_else(|| panic!("槽 s{} 在携带布局里，但当前状态没有它的值", s.0)),
            );
        }
        for r in &layout.roots {
            match mem.get(r) {
                Some(v) => out.push(*v),
                None => {
                    let v = self.seed_mem_in(at, *r);
                    out.push(v);
                }
            }
        }
        out
    }

    /// 在**指定块**里插一条 mem 种子（不改 `cur`）。
    fn seed_mem_in(&mut self, b: BlockId, root: RootId) -> ValueId {
        let t = self.ptr_ty_of(root);
        let dst = self.new_value(t);
        self.blocks[b.index()].instrs.push(Instr::Const {
            dst,
            c: rgoc_hir::types::ConstVal::Int(0),
            typ: t,
        });
        dst
    }

    fn stmts(&mut self, v: &[Stmt]) {
        for s in v {
            // 终止之后的语句是死代码 —— 但「死」只到**下一个标签**为止：
            // `goto L; x = 1; L: return x` 里 `x = 1` 不可达，而 `L:` 是活的。
            // 所以这里不能 `break`（那会把标签一起跳掉，标签块永远是空的 —— 实测踩过）。
            //
            // 循环 / switch 上的标签在它们自己的 `label` 字段里（由 `lowering` 转交），
            // 不经过 `Stmt::Labeled`；那种情形下标签块也真的不可达（`goto` 到循环标签
            // 已被 HIR 拒掉），跳过它是对的。
            if self.terminated && !matches!(s, Stmt::Labeled { .. }) {
                continue;
            }
            self.stmt(s);
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::ShortDecl { slots, rhs } => {
                // 右值**全部先求值**（Go 的赋值语义），再逐个写
                let vs = self.rhs_values(rhs, slots.len());
                for (slot, v) in slots.iter().zip(vs) {
                    self.write_slot(*slot, v);
                }
            }
            Stmt::Assign { lhs, op, rhs } => match op {
                None => {
                    // `a, b = b, a` 的正确性来自这里：右值先全部求值成 SSA 值，
                    // 写的时候已经不存在「先写 a 再读 b」的问题
                    let vs = self.rhs_values(rhs, lhs.len());
                    for (p, v) in lhs.iter().zip(vs) {
                        self.write_place(p, v);
                    }
                }
                Some(op) => {
                    // 复合赋值：`x op= y` ⇒ `x = x op y`
                    let [p] = lhs.as_slice() else {
                        todo!("M6/Task 21：多左值复合赋值");
                    };
                    let [e] = rhs.as_slice() else {
                        todo!("M6/Task 21：多右值复合赋值");
                    };
                    let cur = self.read_place(p);
                    let v = self.expr(e);
                    let t = self.values[cur.index()];
                    let dst = self.new_value(t);
                    self.emit(Instr::Bin {
                        dst,
                        op: *op,
                        lhs: cur,
                        rhs: v,
                        typ: t,
                    });
                    self.write_place(p, dst);
                }
            },
            Stmt::Expr(e) => {
                // 表达式语句可以没有值（`println(...)`）⇒ 走多值入口并丢弃
                let _ = self.expr_multi(e);
            }
            Stmt::Return(es) => {
                let mut vals: Vec<ValueId> = if es.is_empty() {
                    // 裸 return：读命名结果槽（`func f() (r int) { ...; return }`）
                    self.f
                        .results
                        .clone()
                        .into_iter()
                        .map(|s| self.read_slot(s))
                        .collect()
                } else if es.len() == 1 && self.f.result_types.len() > 1 {
                    // `return f()`：**结果直连** —— 不插任何中间值，
                    // `Return` 的操作数就是被调方的 `dsts`（`T-SSA-21` 判的就是这个）
                    self.expr_multi(&es[0])
                } else {
                    es.iter().map(|e| self.expr(e)).collect()
                };
                // 返回值也要对齐到签名类型（`return 1` 的 `1` 是 untyped int）
                if vals.len() == self.f.result_types.len() {
                    for (v, t) in vals.iter_mut().zip(self.f.result_types.clone()) {
                        *v = self.coerce_to(t, *v);
                    }
                }
                self.set_term(Term::Return(vals));
            }
            Stmt::Block(b) => self.stmts(&b.stmts),
            Stmt::Labeled { label, stmt } => self.stmt_labeled(label, stmt),
            Stmt::Empty => {}
            Stmt::If {
                init,
                cond,
                then,
                els,
            } => self.stmt_if(init.as_deref(), cond, then, els.as_deref()),
            Stmt::For {
                init,
                cond,
                post,
                body,
                label,
                ..
            } => self.stmt_for(
                init.as_deref(),
                cond.as_ref(),
                post.as_deref(),
                body,
                label.as_deref(),
            ),
            Stmt::Switch {
                init,
                tag,
                cases,
                label,
            } => self.stmt_switch(init.as_deref(), tag.as_ref(), cases, label.as_deref()),
            Stmt::Range {
                key,
                value,
                x,
                body,
                kind,
                label,
                ..
            } => self.stmt_range(
                key.as_ref(),
                value.as_ref(),
                x,
                body,
                *kind,
                label.as_deref(),
            ),
            Stmt::Branch { kind, label } => match kind {
                BranchKind::Fallthrough => {
                    let Some(t) = self.fallthrough else {
                        todo!("M6/Task 24：`fallthrough` 出现在 case 体之外或最后一个 case")
                    };
                    self.set_term(Term::Goto {
                        to: t,
                        args: Vec::new(),
                    });
                }
                BranchKind::Break => self.do_break(label.as_deref()),
                BranchKind::Continue => self.do_continue(label.as_deref()),
                BranchKind::Goto => {
                    let name = label.clone().expect("`goto` 一定有标签（HIR 已校验）");
                    self.do_goto(&name);
                }
            },
            other => todo!(
                "M6/Task 24–30：语句 `{}` 的构建还没实现",
                stmt_kind_name(other)
            ),
        }
    }

    /// 一个 place 的**值**类型（不是地址类型）。
    ///
    /// # 为什么不能用「地址值的类型」
    ///
    /// 地址值的静态类型是 `*T`（`AddrOf` 的约定），而 `Load{typ}` 要的是 **T**。
    /// 初版 `read_place` 直接拿 `values[addr]` 当 `Load.typ`，于是 `a[pc] += 1`
    /// 读出来的是 `*[30000]uint8` —— `V3` 在随后的 `bin add` 上当场拒
    /// （实测 `turing.go`）。类型只能从 **place 的结构 + HIR 的类型** 推。
    fn place_value_type(&self, p: &Place) -> TypeId {
        match p {
            Place::Slot(s) => self.slot_typ(*s),
            Place::Global(g) => self
                .globals
                .get(g.index())
                .map_or_else(|| TypeId::from_index(0), |x| x.typ),
            Place::Field { base, index } => self
                .struct_field_ty(self.place_value_type(base), *index)
                .unwrap_or_else(|| TypeId::from_index(0)),
            Place::Index { base, .. } => self
                .elem_ty_of(base.typ)
                .unwrap_or_else(|| TypeId::from_index(0)),
            Place::Deref(e) => e.typ,
        }
    }

    /// 结构体第 `index` 个字段的类型（含具名/别名，走 underlying）。
    fn struct_field_ty(&self, t: TypeId, index: usize) -> Option<TypeId> {
        if t.index() >= self.tt.len() {
            return None;
        }
        match self.tt.type_of(rgoc_types::underlying(self.tt, t)) {
            rgoc_types::typ::Type::Struct(sid) => {
                self.tt.struct_def(*sid).fields.get(index).map(|f| f.typ)
            }
            _ => None,
        }
    }

    fn read_place(&mut self, p: &Place) -> ValueId {
        match p {
            Place::Slot(s) => self.read_slot(*s),
            Place::Global(g) => self.load_root(RootId(g.0), self.elem_type(RootId(g.0))),
            other => {
                // 地址与根**一次**求出来（`Deref(f())` 不能求两遍）
                let (addr, hint) = self.place_addr_root(other);
                let root = hint.unwrap_or_else(|| self.heap_root());
                let mem = self.mem_of(root);
                let t = self.place_value_type(other);
                let dst = self.new_value(t);
                self.emit(Instr::Load {
                    dst,
                    mem,
                    root,
                    addr,
                    typ: t,
                });
                dst
            }
        }
    }

    fn write_place(&mut self, p: &Place, v: ValueId) {
        match p {
            Place::Slot(s) => self.write_slot(*s, v),
            Place::Global(g) => {
                let root = RootId(g.0);
                let addr = self.addr_of(root);
                self.store_root(root, addr, v);
            }
            other => {
                let (addr, hint) = self.place_addr_root(other);
                match hint {
                    Some(root) => self.store_root(root, addr, v),
                    // 根未知 ⇒ 扇出到所有根，而不是猜一个（见 `store_to_unknown`）
                    None => self.store_to_unknown(addr, v),
                }
            }
        }
    }

    fn finish(self, id: FuncId) -> SsaFunc {
        SsaFunc {
            src: id,
            name: self.f.name.clone(),
            params: self.blocks[0].params.clone(),
            blocks: self.blocks,
            entry: BlockId(0),
            roots: self.roots,
            slot_root: self.slot_root,
            value_types: self.values,
            call_sigs: self.call_sigs,
        }
    }
}

/// 表达式节点名（`todo!` 消息里用；**不带通配的 `match`**）。
fn expr_kind_name(k: &ExprKind) -> &'static str {
    match k {
        ExprKind::Const(_) => "const",
        ExprKind::Slot(_) => "slot",
        ExprKind::Global(_) => "global",
        ExprKind::Field { .. } => "field",
        ExprKind::Index { .. } => "index",
        ExprKind::Slice { .. } => "slice",
        ExprKind::Deref(_) => "deref",
        ExprKind::Addr(_) => "addr",
        ExprKind::AddrOf(_) => "addrof",
        ExprKind::Unary { .. } => "unary",
        ExprKind::Binary { .. } => "binary",
        ExprKind::Call { .. } => "call",
        ExprKind::Convert { .. } => "convert",
        ExprKind::Composite(_) => "composite",
        ExprKind::FuncLit(_) => "funclit",
        ExprKind::Bound { .. } => "bound",
        ExprKind::TypeAssert { .. } => "typeassert",
        ExprKind::Make { .. } => "make",
        ExprKind::New(_) => "new",
        ExprKind::Builtin { .. } => "builtin",
        ExprKind::Nil => "nil",
    }
}

fn stmt_kind_name(s: &Stmt) -> &'static str {
    match s {
        Stmt::Assign { .. } => "assign",
        Stmt::ShortDecl { .. } => "shortdecl",
        Stmt::Expr(_) => "expr",
        Stmt::If { .. } => "if",
        Stmt::For { .. } => "for",
        Stmt::Range { .. } => "range",
        Stmt::Switch { .. } => "switch",
        Stmt::TypeSwitch { .. } => "typeswitch",
        Stmt::Return(_) => "return",
        Stmt::Branch { .. } => "branch",
        Stmt::Labeled { .. } => "labeled",
        Stmt::Block(_) => "block",
        Stmt::Defer { .. } => "defer",
        Stmt::Empty => "empty",
    }
}

// ── 循环影响扫描（Task 23）───────────────────────────────────────────

/// 循环里「可能被改」的东西。
///
/// # 为什么必须在建 header **之前**算出来
///
/// 循环头块的参数决定**循环体的入口状态**：body 里读 `i`，读到的必须是 header
/// 的参数，而不是循环前的那个值。所以「哪些东西要成为参数」不能等 body 建完
/// 再回头补 —— 那时 body 里已经写死了旧的 `ValueId`。
///
/// 这与 `if` 的汇合块相反（那里汇合块**后建**，因为两侧的映射建完才知道）。
/// 两条路的差别是结构性的：`if` 是「先有两条路，再合并」，`for` 是「先定不变式，
/// 再跑一条回头路」。
#[derive(Default)]
struct LoopEffects {
    /// 被赋值 / 短声明的槽（含循环体内部声明的 —— 调用方会用「是否已在入口映射里」过滤）。
    slots: BTreeSet<SlotId>,
    /// 被直接写的根（`g = 1`、`arr[i] = 1`、`s.f = 1`）。
    roots: BTreeSet<RootId>,
    /// 经指针 / slice / map 的写 ⇒ 堆根被动过。
    heap: bool,
    /// 循环里有调用 ⇒ 保守认为**所有全局根与堆根**都可能被写。
    ///
    /// 这是唯一一处「用保守换简单」：真正的做法是看被调方的 `Effect::writes`，
    /// 而那要求跨函数摘要（M6 单包可以做到，但 `Call` 的 effect 目前是构建期
    /// 从 `may_panic` 推的粗粒度摘要）。保守的代价只是多几个 mem 参数。
    calls: bool,
}

fn scan_loop_effects(
    tt: &TypeTable,
    sr: &BTreeMap<SlotId, RootId>,
    body: &HirBlock,
    post: Option<&Stmt>,
) -> LoopEffects {
    let mut e = LoopEffects::default();
    eff_block(tt, sr, body, &mut e);
    if let Some(p) = post {
        eff_stmt(tt, sr, p, &mut e);
    }
    e
}

fn eff_block(tt: &TypeTable, sr: &BTreeMap<SlotId, RootId>, b: &HirBlock, e: &mut LoopEffects) {
    for s in &b.stmts {
        eff_stmt(tt, sr, s, e);
    }
}

fn eff_stmt(tt: &TypeTable, sr: &BTreeMap<SlotId, RootId>, s: &Stmt, e: &mut LoopEffects) {
    match s {
        Stmt::Assign { lhs, rhs, .. } => {
            for x in rhs {
                eff_expr(tt, sr, x, e);
            }
            for p in lhs {
                eff_place(tt, sr, p, e);
            }
        }
        Stmt::ShortDecl { slots, rhs } => {
            for x in rhs {
                eff_expr(tt, sr, x, e);
            }
            for s in slots {
                e.slots.insert(*s);
            }
        }
        Stmt::Expr(x) => eff_expr(tt, sr, x, e),
        Stmt::If {
            init,
            cond,
            then,
            els,
        } => {
            if let Some(i) = init {
                eff_stmt(tt, sr, i, e);
            }
            eff_expr(tt, sr, cond, e);
            eff_block(tt, sr, then, e);
            if let Some(x) = els {
                eff_stmt(tt, sr, x, e);
            }
        }
        Stmt::For {
            init,
            cond,
            post,
            body,
            ..
        } => {
            if let Some(i) = init {
                eff_stmt(tt, sr, i, e);
            }
            if let Some(c) = cond {
                eff_expr(tt, sr, c, e);
            }
            if let Some(p) = post {
                eff_stmt(tt, sr, p, e);
            }
            eff_block(tt, sr, body, e);
        }
        Stmt::Range {
            key,
            value,
            x,
            body,
            ..
        } => {
            eff_expr(tt, sr, x, e);
            if let Some(p) = key {
                eff_place(tt, sr, p, e);
            }
            if let Some(p) = value {
                eff_place(tt, sr, p, e);
            }
            eff_block(tt, sr, body, e);
        }
        Stmt::Switch {
            init, tag, cases, ..
        } => {
            if let Some(i) = init {
                eff_stmt(tt, sr, i, e);
            }
            if let Some(t) = tag {
                eff_expr(tt, sr, t, e);
            }
            for c in cases {
                for x in &c.exprs {
                    eff_expr(tt, sr, x, e);
                }
                eff_block(tt, sr, &c.body, e);
            }
        }
        Stmt::TypeSwitch { init, x, cases, .. } => {
            if let Some(i) = init {
                eff_stmt(tt, sr, i, e);
            }
            eff_expr(tt, sr, x, e);
            for c in cases {
                if let Some(b) = c.bind {
                    e.slots.insert(b);
                }
                eff_block(tt, sr, &c.body, e);
            }
        }
        Stmt::Return(es) => {
            for x in es {
                eff_expr(tt, sr, x, e);
            }
        }
        Stmt::Branch { .. } | Stmt::Empty => {}
        Stmt::Labeled { stmt, .. } => eff_stmt(tt, sr, stmt, e),
        Stmt::Block(b) => eff_block(tt, sr, b, e),
        Stmt::Defer { call } => eff_expr(tt, sr, call, e),
    }
}

fn eff_expr(tt: &TypeTable, sr: &BTreeMap<SlotId, RootId>, x: &Expr, e: &mut LoopEffects) {
    match &x.kind {
        ExprKind::Const(_) | ExprKind::Slot(_) | ExprKind::Global(_) | ExprKind::Nil => {}
        ExprKind::Field { base, .. } => eff_expr(tt, sr, base, e),
        ExprKind::Index { base, index, .. } => {
            eff_expr(tt, sr, base, e);
            eff_expr(tt, sr, index, e);
        }
        ExprKind::Slice {
            base, lo, hi, max, ..
        } => {
            eff_expr(tt, sr, base, e);
            for o in [lo, hi, max].into_iter().flatten() {
                eff_expr(tt, sr, o, e);
            }
        }
        ExprKind::Deref(y) => eff_expr(tt, sr, y, e),
        ExprKind::Addr(p) => eff_place(tt, sr, p, e),
        ExprKind::AddrOf(y) => eff_expr(tt, sr, y, e),
        ExprKind::Unary { x, .. } => eff_expr(tt, sr, x, e),
        ExprKind::Binary { x, y, .. } => {
            eff_expr(tt, sr, x, e);
            eff_expr(tt, sr, y, e);
        }
        ExprKind::Call { args, .. } => {
            e.calls = true;
            for a in args {
                eff_expr(tt, sr, a, e);
            }
        }
        ExprKind::Convert { x, .. } => eff_expr(tt, sr, x, e),
        ExprKind::Composite(c) => {
            for it in &c.items {
                match it {
                    CompositeItem::Value(y) => eff_expr(tt, sr, y, e),
                    CompositeItem::Key(k, v) => {
                        eff_expr(tt, sr, k, e);
                        eff_expr(tt, sr, v, e);
                    }
                    CompositeItem::Field(_, y) | CompositeItem::Index(_, y) => {
                        eff_expr(tt, sr, y, e)
                    }
                }
            }
        }
        ExprKind::FuncLit(_) => {}
        ExprKind::Bound { recv, .. } => eff_expr(tt, sr, recv, e),
        ExprKind::TypeAssert { x, .. } => eff_expr(tt, sr, x, e),
        ExprKind::Make { args, .. } => {
            // `make` 分配堆 ⇒ 堆根被动过（值本身是新分配的，但 mem 顺序要接上）
            e.heap = true;
            for a in args {
                eff_expr(tt, sr, a, e);
            }
        }
        ExprKind::New(_) => e.heap = true,
        ExprKind::Builtin { args, .. } => {
            // `append` / `delete` / `copy` 都会写堆；`len`/`cap` 不会。
            // 不做逐个判断：保守一次，代价只是多一个 mem 参数。
            e.calls = true;
            for a in args {
                eff_expr(tt, sr, a, e);
            }
        }
    }
}

fn eff_place(tt: &TypeTable, sr: &BTreeMap<SlotId, RootId>, p: &Place, e: &mut LoopEffects) {
    match p {
        Place::Slot(s) => {
            e.slots.insert(*s);
            if let Some(&r) = sr.get(s) {
                e.roots.insert(r);
            }
        }
        Place::Global(g) => {
            e.roots.insert(RootId(g.0));
        }
        Place::Field { .. } | Place::Index { .. } | Place::Deref(_) => {
            match place_base_root(tt, sr, p) {
                Some(r) => {
                    e.roots.insert(r);
                }
                None => e.heap = true,
            }
            // 底座表达式里的调用也要算
            eff_place_inner_exprs(tt, sr, p, e);
        }
    }
}

/// 写一个 place 动的是哪个根的存储（`None` ⇒ 堆，或者本函数管不到）。
fn place_base_root(tt: &TypeTable, sr: &BTreeMap<SlotId, RootId>, p: &Place) -> Option<RootId> {
    match p {
        Place::Slot(s) => sr.get(s).copied(),
        Place::Global(g) => Some(RootId(g.0)),
        Place::Field { base, .. } => place_base_root(tt, sr, base),
        // `Place::Index` 的底座是**表达式**（`arr[i]` 里 `arr` 是 `Expr`），
        // 不是 `Place` —— 这与 `Place::Field` 的底座是 `Place` 不同。
        Place::Index { base, .. } => expr_base_root(tt, sr, base),
        Place::Deref(_) => None,
    }
}

/// 一个**表达式**落在哪个根的存储里（`None` ⇒ 堆，或者本函数管不到）。
fn expr_base_root(tt: &TypeTable, sr: &BTreeMap<SlotId, RootId>, x: &Expr) -> Option<RootId> {
    match &x.kind {
        ExprKind::Slot(s) => sr.get(s).copied(),
        ExprKind::Global(g) => Some(RootId(g.0)),
        ExprKind::Field { base, .. } => expr_base_root(tt, sr, base),
        ExprKind::Index { base, .. } => {
            if is_array(tt, base.typ) {
                expr_base_root(tt, sr, base)
            } else {
                None
            }
        }
        // `*p` / 调用结果 / 复合字面量 / `make`：存储不在本函数的槽里
        _ => None,
    }
}

fn eff_place_inner_exprs(
    tt: &TypeTable,
    sr: &BTreeMap<SlotId, RootId>,
    p: &Place,
    e: &mut LoopEffects,
) {
    match p {
        Place::Slot(_) | Place::Global(_) => {}
        Place::Field { base, .. } => eff_place_inner_exprs(tt, sr, base, e),
        Place::Index { base, index } => {
            eff_expr(tt, sr, base, e);
            eff_expr(tt, sr, index, e);
        }
        Place::Deref(x) => eff_expr(tt, sr, x, e),
    }
}

/// 内建返回几个值。
///
/// # 为什么是穷尽 `match` 而不是「返回 1 的例外表」
///
/// 这张表决定 `Call.dsts` 的个数，也就决定 `V5` 能不能查。写成 `_ => 1`
/// 会让「以后加了个 0 结果的内建」静默变成「多一个 dst」，而 `V5` 只会说
/// 「结果个数不符」—— 报错报在构建器上，原因却在表里。穷尽 `match` 让加变体
/// 时编译就红。
fn builtin_results(k: BuiltinKind) -> usize {
    match k {
        // 没有结果
        BuiltinKind::Delete
        | BuiltinKind::Clear
        | BuiltinKind::Print
        | BuiltinKind::Println
        | BuiltinKind::Panic
        | BuiltinKind::Close => 0,
        // 一个结果
        BuiltinKind::Len
        | BuiltinKind::Cap
        | BuiltinKind::Append
        | BuiltinKind::Copy
        | BuiltinKind::New
        | BuiltinKind::Min
        | BuiltinKind::Max
        | BuiltinKind::Complex
        | BuiltinKind::Real
        | BuiltinKind::Imag
        | BuiltinKind::Recover => 1,
    }
}
