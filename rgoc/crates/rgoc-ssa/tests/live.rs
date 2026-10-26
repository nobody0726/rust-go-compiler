//! 活跃性与 `SSA-LIVE v1`（`T-SSA-55` / `56` / `58` / `59`；门禁 `SSA-8`）。
//!
//! # 为什么手搓 IR
//!
//! 这三条判据要的是「`live_in/live_out` 与**手算**一致」。让构建器生成图的话，
//! 手算就成了「拿构建器的输出当标准答案」—— 构建器错了测试也绿。手搓的图小到
//! 可以逐块数值，而且每条 fixture 都先过一遍 `verify`（保证测的不是一张非法图）。

use std::collections::BTreeSet;

use rgoc_hir::types::ConstVal;
use rgoc_hir::{BinOp, BuiltinKind, SlotId};
use rgoc_ssa::cfg::Cfg;
use rgoc_ssa::ir::{BlockId, CalleeRef, Effect, Instr, Root, SsaFunc, Term, ValueId};
use rgoc_ssa::live::{self, Liveness};
use rgoc_ssa::testsupport::FnBuilder;
use rgoc_ssa::verify::verify;
use rgoc_types::{TypeId, TypeTable};

fn vset(xs: &[u32]) -> BTreeSet<ValueId> {
    xs.iter().map(|x| ValueId(*x)).collect()
}

/// 先验图、再算活跃性（图非法时测试立刻炸，而不是算出一堆无意义的集合）。
fn analyze(f: &SsaFunc, tt: &TypeTable) -> Liveness {
    if let Err(e) = verify(f, tt) {
        panic!(
            "fixture 本身不合法：{e}\n{}",
            rgoc_ssa::dump::dump_func(f, tt)
        );
    }
    let cfg = Cfg::build(f);
    live::analyze(f, &cfg, tt)
}

fn konst(fb: &mut FnBuilder, b: rgoc_ssa::ir::BlockId, c: ConstVal, t: TypeId) -> ValueId {
    let v = fb.value(t);
    fb.instr(b, Instr::Const { dst: v, c, typ: t });
    v
}

// ── T-SSA-55：手算对照 ──────────────────────────────────────────────

#[test]
fn t_ssa_55_直线_边实参在_live_out_而块参数不算_live_in() {
    // b0: v0=5; v1=1; goto b1(v1)
    // b1(p2): v3 = add v0 p2; return v3
    //
    // 手算：b1 用了 b0 的 v0 ⇒ live_in[b1]={v0}；b0 的出口要交 v1 与 v0
    // ⇒ live_out[b0]={v0,v1}。p2 在 **b1 入口**定义，所以不算 b1 的 live_in
    // —— 这一条就是「phi 是块参数」带来的差别。
    let mut fb = FnBuilder::new("f");
    let int = fb.basic("int");
    let e = fb.entry();
    let b1 = fb.block();
    let v0 = konst(&mut fb, e, ConstVal::Int(5), int);
    let v1 = konst(&mut fb, e, ConstVal::Int(1), int);
    let p2 = fb.value(int);
    let v3 = fb.value(int);
    fb.term(
        e,
        Term::Goto {
            to: b1,
            args: vec![v1],
        },
    );
    fb.set_params(b1, &[p2]);
    fb.instr(
        b1,
        Instr::Bin {
            dst: v3,
            op: BinOp::Add,
            lhs: v0,
            rhs: p2,
            typ: int,
        },
    );
    fb.term(b1, Term::Return(vec![v3]));
    let (f, tt) = fb.into_parts();
    let l = analyze(&f, &tt);
    assert_eq!(l.live_in[0], vset(&[]), "b0 的 use 都在本块定义");
    assert_eq!(
        l.live_out[0],
        vset(&[0, 1]),
        "出口要带 v0（b1 用）与 v1（边实参）"
    );
    assert_eq!(l.live_in[1], vset(&[0]), "b1 用到 b0 的 v0");
    assert_eq!(l.live_out[1], vset(&[]), "b1 没有后继");
    assert!(
        !l.live_in[1].contains(&ValueId(2)),
        "块参数 p2 在 b1 **入口**定义，不能算 live_in"
    );
}

#[test]
fn t_ssa_55_分支_两臂共用的值在两臂的_live_in_里() {
    // b0: v0=true; v1=7; if v0 -> b1() | b2()
    // b1: v2 = add v1 v1; goto b3(v2)
    // b2: v3 = mul v1 v1; goto b3(v3)
    // b3(p4): return p4
    let mut fb = FnBuilder::new("f");
    let int = fb.basic("int");
    let bt = fb.basic("bool");
    let e = fb.entry();
    let b1 = fb.block();
    let b2 = fb.block();
    let b3 = fb.block();
    let v0 = konst(&mut fb, e, ConstVal::Bool(true), bt);
    let v1 = konst(&mut fb, e, ConstVal::Int(7), int);
    let v2 = fb.value(int);
    let v3 = fb.value(int);
    let p4 = fb.value(int);
    fb.term(
        e,
        Term::If {
            cond: v0,
            then_: (b1, Vec::new()),
            else_: (b2, Vec::new()),
        },
    );
    fb.instr(
        b1,
        Instr::Bin {
            dst: v2,
            op: BinOp::Add,
            lhs: v1,
            rhs: v1,
            typ: int,
        },
    );
    fb.term(
        b1,
        Term::Goto {
            to: b3,
            args: vec![v2],
        },
    );
    fb.instr(
        b2,
        Instr::Bin {
            dst: v3,
            op: BinOp::Mul,
            lhs: v1,
            rhs: v1,
            typ: int,
        },
    );
    fb.term(
        b2,
        Term::Goto {
            to: b3,
            args: vec![v3],
        },
    );
    fb.set_params(b3, &[p4]);
    fb.term(b3, Term::Return(vec![p4]));
    let (f, tt) = fb.into_parts();
    let l = analyze(&f, &tt);
    assert_eq!(l.live_in[0], vset(&[]));
    assert_eq!(l.live_out[0], vset(&[1]), "两臂都要 v1");
    assert_eq!(l.live_in[1], vset(&[1]));
    assert_eq!(l.live_out[1], vset(&[2]), "b1 的出口带它算出来的 v2");
    assert_eq!(l.live_in[2], vset(&[1]));
    assert_eq!(l.live_out[2], vset(&[3]));
    assert_eq!(l.live_in[3], vset(&[]), "p4 是块参数");
}

#[test]
fn t_ssa_55_循环_回边上变量活跃且不动点收敛() {
    // b0: v0=0; v1=1; goto b1(v0)
    // b1(p2): v3=10; v4 = lt p2 v3; if v4 -> b2() | b3()
    // b2: v5 = add p2 v1; goto b1(v5)      ← 回边
    // b3: return p2
    //
    // 手算（最小不动点）：
    //   live_in[b3]={p2}
    //   live_in[b1]={v1}      ← v1 在 b0 定义、在循环体 b2 用 ⇒ **回边上活跃**
    //   live_out[b1]={v1,p2}  ← p2 是块参数，不算 live_in，但出口要交给 b3
    //   live_out[b2]={v1,v5}  ← 交 v5 给回边，v1 还要继续用
    let mut fb = FnBuilder::new("f");
    let int = fb.basic("int");
    let bt = fb.basic("bool");
    let e = fb.entry();
    let b1 = fb.block();
    let b2 = fb.block();
    let b3 = fb.block();
    let v0 = konst(&mut fb, e, ConstVal::Int(0), int);
    let v1 = konst(&mut fb, e, ConstVal::Int(1), int);
    let p2 = fb.value(int);
    let v3 = fb.value(int);
    let v4 = fb.value(bt);
    let v5 = fb.value(int);
    fb.term(
        e,
        Term::Goto {
            to: b1,
            args: vec![v0],
        },
    );
    fb.set_params(b1, &[p2]);
    fb.instr(
        b1,
        Instr::Const {
            dst: v3,
            c: ConstVal::Int(10),
            typ: int,
        },
    );
    fb.instr(
        b1,
        Instr::Bin {
            dst: v4,
            op: BinOp::Lt,
            lhs: p2,
            rhs: v3,
            typ: bt,
        },
    );
    fb.term(
        b1,
        Term::If {
            cond: v4,
            then_: (b2, Vec::new()),
            else_: (b3, Vec::new()),
        },
    );
    fb.instr(
        b2,
        Instr::Bin {
            dst: v5,
            op: BinOp::Add,
            lhs: p2,
            rhs: v1,
            typ: int,
        },
    );
    fb.term(
        b2,
        Term::Goto {
            to: b1,
            args: vec![v5],
        },
    );
    fb.term(b3, Term::Return(vec![p2]));
    let (f, tt) = fb.into_parts();
    let l = analyze(&f, &tt);
    assert_eq!(l.live_in[0], vset(&[]));
    assert_eq!(l.live_out[0], vset(&[0, 1]), "v0 交给循环、v1 在体内用");
    assert_eq!(
        l.live_in[1],
        vset(&[1]),
        "v1 在回边上活跃（b0 定义、b2 用）"
    );
    assert_eq!(l.live_out[1], vset(&[1, 2]), "出口要带 v1 与块参数 p2");
    assert_eq!(l.live_in[2], vset(&[1, 2]), "循环体要 p2 与 v1");
    assert_eq!(l.live_out[2], vset(&[1, 5]), "回边交 v5，v1 还要继续用");
    assert_eq!(l.live_in[3], vset(&[2]));
    assert_eq!(l.live_out[3], vset(&[]));
    // 「回边上活跃」的正面判据：v1 在循环头与循环体的活跃集里都出现
    assert!(l.live_in[1].contains(&ValueId(1)));
    assert!(l.live_out[2].contains(&ValueId(1)));
}

// ── T-SSA-56 / 58 / 59：协议 ────────────────────────────────────────

/// 一个带 safepoint 与指针的函数（`T-SSA-56` 用）。
///
/// ```text
/// b0:
///   v0 = const 1 : int
///   v1 = const 0 : *int          ← mem 令牌
///   v2 = addr-of r0 : *int
///   v3 = load v1 r0 v2 : int
///   call println(v0)             ← safepoint @ idx 4
///   v4 = load v1 r0 v2 : int
///   v5 = bin add v3 v4 : int
///   term return v5
/// ```
fn safepoint_fixture() -> (SsaFunc, TypeTable) {
    let mut fb = FnBuilder::new("f");
    let int = fb.basic("int");
    let pint = fb.ptr_to(int);
    let e = fb.entry();
    let r0 = fb.root(Root::Slot(SlotId(0)));
    fb.slot_root(SlotId(0), r0);
    fb.slot_typ(SlotId(0), int);
    let v0 = konst(&mut fb, e, ConstVal::Int(1), int);
    let v1 = konst(&mut fb, e, ConstVal::Int(0), pint);
    let v2 = fb.value(pint);
    let v3 = fb.value(int);
    let v4 = fb.value(int);
    let v5 = fb.value(int);
    fb.instr(e, Instr::AddrOf { dst: v2, root: r0 });
    fb.instr(
        e,
        Instr::Load {
            dst: v3,
            mem: v1,
            root: r0,
            addr: v2,
            typ: int,
        },
    );
    fb.instr(
        e,
        Instr::Call {
            dsts: Vec::new(),
            callee: CalleeRef::Builtin(BuiltinKind::Println),
            args: vec![v0],
            dots: false,
            effect: Effect::default(),
        },
    );
    fb.instr(
        e,
        Instr::Load {
            dst: v4,
            mem: v1,
            root: r0,
            addr: v2,
            typ: int,
        },
    );
    fb.instr(
        e,
        Instr::Bin {
            dst: v5,
            op: BinOp::Add,
            lhs: v3,
            rhs: v4,
            typ: int,
        },
    );
    fb.term(e, Term::Return(vec![v5]));
    fb.into_parts()
}

/// 解析 `live=[v0,v1]` / `ptrs=[]` 这类列表。
fn parse_list(s: &str) -> Vec<u32> {
    let body = s
        .trim()
        .strip_prefix('[')
        .and_then(|x| x.strip_suffix(']'))
        .unwrap_or_else(|| panic!("不是列表：{s:?}"));
    if body.is_empty() {
        return Vec::new();
    }
    body.split(',')
        .map(|x| {
            x.trim()
                .strip_prefix('v')
                .and_then(|n| n.parse::<u32>().ok())
                .unwrap_or_else(|| panic!("不是 v<k>：{x:?}"))
        })
        .collect()
}

/// 一行 `# safepoint b<i> <idx> live=[..] ptrs=[..]`。
fn parse_safepoint(line: &str) -> (u32, usize, Vec<u32>, Vec<u32>) {
    let rest = line
        .strip_prefix("# safepoint b")
        .unwrap_or_else(|| panic!("不是 safepoint 行：{line:?}"));
    let mut it = rest.split(' ');
    let block = it.next().unwrap().parse::<u32>().unwrap();
    let idx = it.next().unwrap().parse::<usize>().unwrap();
    let live = parse_list(it.next().unwrap().strip_prefix("live=").unwrap());
    let ptrs = parse_list(it.next().unwrap().strip_prefix("ptrs=").unwrap());
    (block, idx, live, ptrs)
}

/// 一行 `# interval v<k> def=b<i>:<j> last=b<i>:<j>` → `(v, def, last)`。
fn parse_interval(line: &str) -> (u32, (u32, i32), (u32, i32)) {
    let rest = line
        .strip_prefix("# interval v")
        .unwrap_or_else(|| panic!("不是 interval 行：{line:?}"));
    let mut it = rest.split(' ');
    let v = it.next().unwrap().parse::<u32>().unwrap();
    let pos = |s: &str| {
        let body = s.split('=').nth(1).unwrap();
        let (b, i) = body.split_once(':').unwrap();
        (
            b.trim_start_matches('b').parse::<u32>().unwrap(),
            i.parse::<i32>().unwrap(),
        )
    };
    (v, pos(it.next().unwrap()), pos(it.next().unwrap()))
}

#[test]
fn t_ssa_56_协议字段自洽() {
    let (f, tt) = safepoint_fixture();
    let l = analyze(&f, &tt);
    let text = live::render(&f, &l);
    let mut lines = text.lines();
    assert_eq!(lines.next(), Some("# SSA-LIVE v1"), "版本行");
    assert_eq!(
        lines.next(),
        Some(format!("# func f blocks=1 values={}", f.value_count()).as_str()),
        "头行要写块数与值数"
    );

    let mut sps = Vec::new();
    let mut ivs = Vec::new();
    for line in lines {
        if line.starts_with("# safepoint ") {
            sps.push(parse_safepoint(line));
        } else if line.starts_with("# interval ") {
            ivs.push(parse_interval(line));
        } else {
            panic!("不认识的协议行：{line:?}");
        }
    }
    assert_eq!(sps.len(), 1, "fixture 只有一个 safepoint：{text}");
    assert_eq!(ivs.len(), f.value_count(), "每个值都该有区间：{text}");

    let (blk, idx, live_vs, ptrs) = &sps[0];
    assert_eq!((*blk, *idx), (0, 4), "safepoint 是那条 `println` 调用");
    assert_eq!(
        live_vs,
        &vec![0, 1, 2, 3],
        "跨过调用还活着的是 v0（实参）、v1（mem）、v2（指针）、v3（后用的 Load 结果）"
    );

    // ① `live` 里每个值都有区间
    let have: BTreeSet<u32> = ivs.iter().map(|(v, _, _)| *v).collect();
    for v in live_vs {
        assert!(have.contains(v), "v{v} 在 live 里却没有区间：{text}");
    }
    // ② `ptrs` ⊆ `live`
    for v in ptrs {
        assert!(live_vs.contains(v), "ptrs 里的 v{v} 不在 live 里：{text}");
    }
    // ③ `ptrs` 里的值都**至少有一个根字**。
    //
    // M9/Phase 6 之前这里判的是「类型是 `*T`」。`RM-2` 之后粒度到字
    // （`R-9-31`）：`[]T` / `string` / `interface` / 含指针的聚合都是根，
    // 判据改为查 `ptr_mask` —— 与栈图的位图**同一个来源**。
    for v in ptrs {
        let t = f.value_types[*v as usize];
        assert!(
            rgoc_types::layout::ptr_mask(&tt, t).is_some_and(|m| m.iter().any(|b| *b)),
            "v{v} 的类型 {:?} 一个根字都没有：{text}",
            tt.type_of(t)
        );
    }
    assert_eq!(ptrs, &vec![2], "只有 v2（`addr-of`）是根；v1 是 mem 令牌");
    // ④ 区间自洽：def ≤ last，且 def 落在本块范围内
    for (v, def, last) in &ivs {
        assert!(def <= last, "v{v} 的区间反了：{def:?}..{last:?}\n{text}");
        assert!(def.0 < f.blocks.len() as u32, "v{v} 的 def 块号越界");
    }
}

#[test]
fn t_ssa_58_派生指针活跃则根也活跃() {
    // b0:
    //   v0 = const 0 : *[2]int     ← mem
    //   v1 = addr-of r0 : *[2]int  ← 根
    //   v2 = const 1 : int
    //   v3 = index v1 v2 : *int    ← 派生指针（v1 的**最后一次**使用就在这里）
    //   call println(v2)           ← safepoint @ idx 4
    //   v4 = load v0 r0 v3 : int
    //   v5 = bin add v4 v4
    //   term return v5
    //
    // 纯值活跃性说：v1 在 idx 3 之后就死了。但 v3 活着 ⇒ v1 指向的存储必须活着，
    // 否则 v3 悬空。所以 safepoint 的 `live` 必须含 v1（保守，`D-M6-10`）。
    let mut fb = FnBuilder::new("f");
    let int = fb.basic("int");
    let arr = fb.array_of(2, int);
    let parr = fb.ptr_to(arr);
    let pint = fb.ptr_to(int);
    let e = fb.entry();
    let r0 = fb.root(Root::Slot(SlotId(0)));
    fb.slot_root(SlotId(0), r0);
    fb.slot_typ(SlotId(0), arr);
    let v0 = konst(&mut fb, e, ConstVal::Int(0), parr);
    let v1 = fb.value(parr);
    let v2 = konst(&mut fb, e, ConstVal::Int(1), int);
    let v3 = fb.value(pint);
    let v4 = fb.value(int);
    let v5 = fb.value(int);
    fb.instr(e, Instr::AddrOf { dst: v1, root: r0 });
    fb.instr(
        e,
        Instr::Index {
            dst: v3,
            base: v1,
            i: v2,
        },
    );
    fb.instr(
        e,
        Instr::Call {
            dsts: Vec::new(),
            callee: CalleeRef::Builtin(BuiltinKind::Println),
            args: vec![v2],
            dots: false,
            effect: Effect::default(),
        },
    );
    fb.instr(
        e,
        Instr::Load {
            dst: v4,
            mem: v0,
            root: r0,
            addr: v3,
            typ: int,
        },
    );
    fb.instr(
        e,
        Instr::Bin {
            dst: v5,
            op: BinOp::Add,
            lhs: v4,
            rhs: v4,
            typ: int,
        },
    );
    fb.term(e, Term::Return(vec![v5]));
    let (f, tt) = fb.into_parts();
    let l = analyze(&f, &tt);

    // 纯值活跃性：v1 的最后一次使用是那条 `index`（idx 3）
    // 指令序：0 = `const v0`、1 = `const v2`、2 = `addr-of v1`、3 = `index v3`
    let iv = l.intervals[&ValueId(1)];
    assert_eq!(iv.def, live::Pos::instr(0, 2));
    assert_eq!(iv.last, live::Pos::instr(0, 3), "v1 之后没人直接用它");

    let sp = &l.safepoints[0];
    assert_eq!((sp.block.0, sp.idx), (0, 4));
    assert!(
        sp.live.contains(&ValueId(1)),
        "派生指针 v3 活着 ⇒ 根 v1 必须被标活跃：{:?}",
        sp.live
    );
    assert!(
        sp.ptrs.contains(&ValueId(1)) && sp.ptrs.contains(&ValueId(3)),
        "协议里 `ptrs` 要含根与派生指针：{:?}",
        sp.ptrs
    );
    // 根被保守加进来之后，协议里也要有它的区间（消费方靠它算生命周期）
    let text = live::render(&f, &l);
    assert!(
        text.contains("# interval v1 def=b0:2 last=b0:3"),
        "根也要有区间行：{text}"
    );
}

#[test]
fn t_ssa_59_两次输出逐字节相同() {
    // 确定性：`BTreeMap` / `BTreeSet` 保证顺序，但「重算一次」才是真判据 ——
    // 谁哪天把某个容器换成 `HashMap`，这条会红。
    let (f, tt) = safepoint_fixture();
    let a = live::render(&f, &analyze(&f, &tt));
    let b = live::render(&f, &analyze(&f, &tt));
    assert_eq!(a, b, "同一函数两次渲染必须逐字节相同");
    // 块与值的顺序也必须是升序（排序键 = 块号 + 指令下标）
    let (f2, tt2) = {
        // 两块 + 两个 safepoint，用来验排序
        let mut fb = FnBuilder::new("g");
        let int = fb.basic("int");
        let e = fb.entry();
        let b1 = fb.block();
        let v0 = konst(&mut fb, e, ConstVal::Int(1), int);
        fb.instr(
            e,
            Instr::Call {
                dsts: Vec::new(),
                callee: CalleeRef::Builtin(BuiltinKind::Println),
                args: vec![v0],
                dots: false,
                effect: Effect::default(),
            },
        );
        fb.term(
            e,
            Term::Goto {
                to: b1,
                args: Vec::new(),
            },
        );
        let v1 = konst(&mut fb, b1, ConstVal::Int(2), int);
        fb.instr(
            b1,
            Instr::Call {
                dsts: Vec::new(),
                callee: CalleeRef::Builtin(BuiltinKind::Println),
                args: vec![v1],
                dots: false,
                effect: Effect::default(),
            },
        );
        fb.term(b1, Term::Return(vec![v1]));
        fb.into_parts()
    };
    let text = live::render(&f2, &analyze(&f2, &tt2));
    let keys: Vec<(u32, usize)> = text
        .lines()
        .filter(|l| l.starts_with("# safepoint "))
        .map(|l| {
            let (b, i, _, _) = parse_safepoint(l);
            (b, i)
        })
        .collect();
    // b1 的 idx 0 是那条 `const`，调用在 idx 1
    assert_eq!(keys, vec![(0, 1), (1, 1)], "safepoint 按块号+指令下标升序");
}

// ── `R-11-1`：终止符的读必须进 `after[]`（M7/Task 11 抓到的 M6 缺陷） ─────

#[test]
fn r_11_1_只在_return_里用的值仍跨过_safepoint_活着() {
    // b0: v0 = 1; v1 = const 0 : *int; print(v0); return v1
    //
    // `v1` **只在终止符里被读**（`Return`）。它从定义点一直活到块尾，
    // 所以 `print` 这个 safepoint 的 `live` 里必须有它 —— 否则帧图漏根，
    // M10 扫栈时会把还活着的对象当垃圾回收掉。
    //
    // 修复前：`after[]` 从 `live_out[b0]` 起步，而 `live_out` 只由**后继**的
    // `live_in` 与边实参构成（`Return` 没有后继）⇒ `v1` 在 `after[1]` 里整个消失。
    // M6 的 5 条 liveness 测试没抓到，因为那些用例的返回值同时也被别的指令用过。
    let mut fb = FnBuilder::new("f");
    let int = fb.basic("int");
    let pint = fb.ptr_to(int);
    let e = fb.entry();
    let v0 = konst(&mut fb, e, ConstVal::Int(1), int);
    let v1 = konst(&mut fb, e, ConstVal::Int(0), pint);
    fb.instr(
        e,
        Instr::Call {
            dsts: Vec::new(),
            callee: CalleeRef::Builtin(BuiltinKind::Print),
            args: vec![v0],
            dots: false,
            effect: Effect::default(),
        },
    );
    fb.term(e, Term::Return(vec![v1]));
    let (f, tt) = fb.into_parts();
    let lv = analyze(&f, &tt);

    let sp = lv.safepoints.first().expect("print 是 safepoint");
    assert!(
        sp.live.contains(&v1),
        "只在 return 里用的 v1 必须跨过 safepoint 活着，实际 live={:?}",
        sp.live
    );
    assert!(
        sp.ptrs.contains(&v1),
        "v1 是指针 ⇒ 必须进 ptrs（帧图的根集），实际 ptrs={:?}",
        sp.ptrs
    );
}

#[test]
fn r_11_1_只在_goto_边实参里用的值也算活着() {
    // b0: v0 = 1; v1 = 2; print(v0); goto b1(v1)
    // b1(p2): return p2
    //
    // `v1` 只在 `goto` 的边实参里被读 —— 与上一条同源，但走的是边实参而不是
    // `Return`。`live_out[b0]` 会含 v1（边实参进 live_out），所以这条**修复前也过**；
    // 留着它是为了把「两条路都对」钉住（防止有人把修复写成只补 `Return`）。
    let mut fb = FnBuilder::new("f");
    let int = fb.basic("int");
    let e = fb.entry();
    let b1 = fb.block();
    let v0 = konst(&mut fb, e, ConstVal::Int(1), int);
    let v1 = konst(&mut fb, e, ConstVal::Int(2), int);
    let p2 = fb.value(int);
    fb.set_params(b1, &[p2]);
    fb.instr(
        e,
        Instr::Call {
            dsts: Vec::new(),
            callee: CalleeRef::Builtin(BuiltinKind::Print),
            args: vec![v0],
            dots: false,
            effect: Effect::default(),
        },
    );
    fb.term(
        e,
        Term::Goto {
            to: b1,
            args: vec![v1],
        },
    );
    fb.term(b1, Term::Return(vec![p2]));
    let (f, tt) = fb.into_parts();
    let lv = analyze(&f, &tt);
    let sp = lv.safepoints.first().expect("print 是 safepoint");
    assert!(
        sp.live.contains(&v1),
        "边实参里的 v1 也要活着：{:?}",
        sp.live
    );
}

// ── T-ROOT-05：位图粒度到「字」（M9/Phase 6，`ROOT-1`）────────────────

/// 三个「字粒度与值粒度不同」的活值，跨过同一个调用。
///
/// - `s []int`：三个字，**只有字 0** 是根（`len`/`cap` 是整数）
/// - `str string`：两个字，**只有字 0** 是根
/// - `x any`：两个字，**两个字都是根**（tab + data）
///
/// 手写 IR 的理由与文件头相同：这三个形态的「应该是几位」是 `C5` §10 的
/// 契约值，让构建器生成就等于拿它的输出当标准答案。
fn grain_fixture() -> (SsaFunc, TypeTable, ValueId, ValueId, ValueId) {
    let mut fb = FnBuilder::new("g");
    let int = fb.basic("int");
    let string = fb.basic("string");
    let slice = fb.slice_of(int);
    let any = fb.empty_iface();
    let e = fb.entry();
    let vs = fb.value(slice);
    let vn = konst(&mut fb, e, ConstVal::Int(0), int);
    let vstr = konst(&mut fb, e, ConstVal::Str("x".to_string()), string);
    let vi = fb.value(int);
    let vx = fb.value(any);
    fb.instr(
        e,
        Instr::Make {
            dst: vs,
            typ: slice,
            len: Some(vn),
            cap: Some(vn),
        },
    );
    fb.term(e, Term::Return(Vec::new()));
    // 上面那条 `Make` 也是 safepoint，但它不读任何东西 ⇒ 活集为空。
    // 真正要断言的是**下一个** safepoint。
    let (mut f, tt) = fb.into_parts();
    // 把调用追加进同一个块（`term` 之后不能再加指令，所以重建）
    f.blocks[0].term = Term::Unreachable;
    f.blocks[0].instrs.push(Instr::Const {
        dst: vi,
        c: ConstVal::Int(1),
        typ: int,
    });
    f.blocks[0].instrs.push(Instr::Convert {
        dst: vx,
        x: vi,
        typ: any,
    });
    f.blocks[0].instrs.push(Instr::Call {
        dsts: Vec::new(),
        callee: CalleeRef::Builtin(BuiltinKind::Println),
        args: vec![vs, vstr, vx],
        dots: false,
        effect: Effect::default(),
    });
    f.blocks[0].term = Term::Return(Vec::new());
    let _ = vstr;
    (f, tt, vs, vstr, vx)
}

/// 某个值在 `ptr_words` 里的字下标（升序）。
fn words_of(sp: &live::Safepoint, v: ValueId) -> Vec<u8> {
    sp.ptr_words
        .iter()
        .filter(|(x, _)| *x == v)
        .map(|(_, w)| *w)
        .collect()
}

#[test]
fn t_root_05_位图粒度到字() {
    let (f, tt, vs, vstr, vx) = grain_fixture();
    let l = analyze(&f, &tt);
    // 读三个值的那条调用。
    //
    // ⚠️ 不能按「live 有 3 个」找：`Make` 也是 safepoint，它的活集里正好有
    // `len` / `cap` 与刚被它盖掉之前的切片值。按**只有调用才读的那个值**找。
    //
    // ⚠️ M10 起**必须**按「位点上是 `Call`」过滤：`Convert`（转接口 ⇒ `rt_box`
    // 会分配）现在也是 safepoint，它的活集里多了装箱的**源值** `v3`。
    // 只按 `live.contains(&vx)` 找会命中 `Convert` 那个位点（它在前面）。
    let sp = l
        .safepoints
        .iter()
        .find(|s| {
            s.live.contains(&vx)
                && matches!(
                    f.blocks[s.block.index()].instrs.get(s.idx),
                    Some(Instr::Call { .. })
                )
        })
        .unwrap_or_else(|| panic!("找不到读 iface 的那个 safepoint：{:?}", l.safepoints));
    assert_eq!(
        sp.live,
        vec![vs, vstr, vx],
        "live 应是三个值（切片 / string / iface）；value_types={:?}",
        f.value_types
    );
    assert_eq!(
        words_of(sp, vs),
        vec![0],
        "`[]int` 只有字 0 是根（len / cap 是整数）：{:?}",
        sp.ptr_words
    );
    assert_eq!(
        words_of(sp, vstr),
        vec![0],
        "`string` 只有字 0 是根（len 是整数）：{:?}",
        sp.ptr_words
    );
    assert_eq!(
        words_of(sp, vx),
        vec![0, 1],
        "`any` 两个字都是根（tab + data）：{:?}",
        sp.ptr_words
    );
    // 值级投影：三个值都在，且没有第四个
    assert_eq!(sp.ptrs, vec![vs, vstr, vx], "ptrs 是 ptr_words 的值级投影");
    // 位序是「值号升序、再字下标升序」—— 这条决定 `STACK-MAP` 的可复现性
    let keys: Vec<(u32, u8)> = sp.ptr_words.iter().map(|(v, w)| (v.0, *w)).collect();
    let mut sorted = keys.clone();
    sorted.sort_unstable();
    assert_eq!(keys, sorted, "ptr_words 必须已按 (值号, 字下标) 升序");
}

// ── T-ROOT-04：回边也是 safepoint（`M-M9-10` 的牙）────────────────────

/// 一个「循环 100 次、循环体里**没有调用也没有分配**」的函数。
///
/// ```text
/// b0:      v0 = 0; v1 = 1; v2 = 100; goto b1(v0)
/// b1(p3):  v4 = p3 < v2; if v4 → b2 else b3
/// b2:      v5 = p3 + v1; goto b1(v5)      ← 回边：b1 支配 b2
/// b3:      return p3
/// ```
///
/// 整个图里 `Call` / `Make` **一条都没有**，所以按 M6 的判据这里一个 safepoint
/// 都不会有 —— 可这个循环要跑 100 次，M11 的抢占随时可能落在回边上。
fn loop_fixture() -> (SsaFunc, TypeTable) {
    let mut fb = FnBuilder::new("loop");
    let int = fb.basic("int");
    let boolean = fb.basic("bool");
    let e = fb.entry();
    let head = fb.block();
    let body = fb.block();
    let exit = fb.block();
    let v0 = konst(&mut fb, e, ConstVal::Int(0), int);
    let v1 = konst(&mut fb, e, ConstVal::Int(1), int);
    let v2 = konst(&mut fb, e, ConstVal::Int(100), int);
    let p3 = fb.value(int);
    fb.term(
        e,
        Term::Goto {
            to: head,
            args: vec![v0],
        },
    );
    fb.set_params(head, &[p3]);
    let v4 = fb.value(boolean);
    fb.instr(
        head,
        Instr::Bin {
            dst: v4,
            op: BinOp::Lt,
            lhs: p3,
            rhs: v2,
            typ: boolean,
        },
    );
    fb.term(
        head,
        Term::If {
            cond: v4,
            then_: (body, Vec::new()),
            else_: (exit, Vec::new()),
        },
    );
    let v5 = fb.value(int);
    fb.instr(
        body,
        Instr::Bin {
            dst: v5,
            op: BinOp::Add,
            lhs: p3,
            rhs: v1,
            typ: int,
        },
    );
    fb.term(
        body,
        Term::Goto {
            to: head,
            args: vec![v5],
        },
    );
    fb.term(exit, Term::Return(vec![p3]));
    fb.into_parts()
}

#[test]
fn t_root_04_回边也是_safepoint() {
    let (f, tt) = loop_fixture();
    let l = analyze(&f, &tt);
    assert_eq!(
        l.safepoints.len(),
        1,
        "这个图里没有调用也没有分配 ⇒ 唯一的 safepoint 必须是那条回边；实际 {:?}",
        l.safepoints
    );
    let sp = &l.safepoints[0];
    assert_eq!(sp.block, BlockId(2), "回边在 b2（循环体）");
    assert_eq!(
        sp.idx,
        f.blocks[2].instrs.len(),
        "终止符位点写 `idx = instrs.len()`（与区间里的块尾同一个哨兵）"
    );
    // 活集 = `live_out ∪ 终止符的读`：边实参 v5 与循环不变量 v1 / v2
    assert!(
        sp.live.contains(&ValueId(5)),
        "回边实参必须活着（它就是下一次迭代的循环变量）：{:?}",
        sp.live
    );
    // 用 O(1) 判据反向确认「这个图确实没有指令级 safepoint」
    let has_instr_sp = f.blocks.iter().enumerate().any(|(bi, b)| {
        b.instrs.iter().enumerate().any(|(ii, _)| {
            l.safepoints
                .iter()
                .any(|s| s.block.0 as usize == bi && s.idx == ii)
        })
    });
    assert!(!has_instr_sp, "不该有指令级 safepoint：{:?}", l.safepoints);
}
