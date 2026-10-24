//! `ssa_exec` 的判据（`M6-plan.md` Task 32 起；`M6-tests.md` 的 `O`/`S` 腿）。
//!
//! # 为什么这些测试手搓 IR
//!
//! 端到端的「Go 源码 → 跑 SSA」在 `rgoc-driver` 的测试里（那边才拿得到 HIR）。
//! 这一层要单独测**执行器**：给一张确定的图，看它算出什么。手搓 IR 才能
//! 精确控制「块参数 / 边实参 / 内存根」这些端到端不好构造的形态。
//! 工具用 `rgoc_ssa::testsupport`（`D-M6-11` 的地基）。

use rgoc_hir::BinOp;
use rgoc_hir::types::ConstVal;
use rgoc_interp::{Options, run_ssa};
use rgoc_ssa::ir::{CalleeRef, Effect, Instr, Module, Term};
use rgoc_ssa::testsupport::{FnBuilder, module_of};
use rgoc_types::TypeTable;

/// 空 `main`：入口块直接 `Return`。
#[test]
fn t_ssa_32_空函数立即返回() {
    let mut b = FnBuilder::new("main");
    let e = b.entry();
    b.term(e, Term::Return(Vec::new()));
    let (f, tt) = b.into_parts();
    let m = Module {
        main: Some(rgoc_hir::FuncId(0)),
        ..module_of(vec![f])
    };
    let out = run_ssa(&m, &tt, Options::default());
    assert_eq!(out.exit, 0, "空 main 正常返回：{out:?}");
    assert_eq!(out.output, "", "没有输出");
    assert_eq!(out.steps, 1, "一步：取到终止符并返回");
    assert!(out.diag.is_none(), "没有诊断");
}

#[test]
fn t_ssa_32_没有_main_要报错而不是假装成功() {
    let mut b = FnBuilder::new("f");
    let e = b.entry();
    b.term(e, Term::Return(Vec::new()));
    let (f, tt) = b.into_parts();
    let m = module_of(vec![f]);
    let out = run_ssa(&m, &tt, Options::default());
    assert_eq!(out.exit, 2, "没有 main ⇒ 退出码 2：{out:?}");
    assert!(
        out.diag.as_deref().unwrap_or("").contains("main"),
        "诊断要说明缺 main：{out:?}"
    );
}

#[test]
fn t_ssa_32_步数上限要能停住() {
    // 骨架期没有控制流，所以「跑很多步」要靠空函数 × 极小预算：
    // 预算 0 ⇒ 一步都不许走。
    let mut b = FnBuilder::new("main");
    let e = b.entry();
    b.term(e, Term::Return(Vec::new()));
    let (f, tt) = b.into_parts();
    let m = Module {
        main: Some(rgoc_hir::FuncId(0)),
        ..module_of(vec![f])
    };
    let out = run_ssa(
        &m,
        &tt,
        Options {
            max_steps: 0,
            ..Options::default()
        },
    );
    assert_eq!(out.exit, 2, "预算 0 ⇒ 超限：{out:?}");
    assert!(out.diag.is_some(), "要给原因");
}

// ── Task 36：预算与退出码（`T-SSA-40`）────────────────────────────

/// 一个自环的 `main`：`b0: goto b0`。
fn spin() -> (Module, TypeTable) {
    let mut b = FnBuilder::new("main");
    let e = b.entry();
    b.term(
        e,
        Term::Goto {
            to: e,
            args: Vec::new(),
        },
    );
    let (f, tt) = b.into_parts();
    (
        Module {
            main: Some(rgoc_hir::FuncId(0)),
            ..module_of(vec![f])
        },
        tt,
    )
}

#[test]
fn t_ssa_40_死循环按预算停下并给出原因() {
    let (m, tt) = spin();
    let out = run_ssa(
        &m,
        &tt,
        Options {
            max_steps: 10_000,
            ..Options::default()
        },
    );
    assert_eq!(out.exit, 2, "超预算 ⇒ 退出码 2：{out:?}");
    let diag = out.diag.unwrap_or_default();
    assert!(
        diag.contains("步数超限") && diag.contains("10000"),
        "诊断要说明原因与预算，实际：{diag:?}"
    );
    assert_eq!(out.steps, 10_000, "步数正好用满预算（不是提前也不是超出）");
    assert_eq!(out.output, "", "没有输出");
}

#[test]
fn t_ssa_40_预算耗尽是十秒内可控的() {
    // 「宿主不挂死」是这条的全部意义：预算必须真的能兜住死循环。
    //
    // 用**显式**预算 1e7 钉墙钟（实测约 1 s；见台账里的吞吐测量），而不是拿
    // 默认值 1e8 来断言 —— debug 构建的吞吐约 10M 步/秒，1e8 要 10.4 s，
    // 那条断言等于把「机器速度」写进测试（release 下又必然过）。
    // 默认值本身用一条常量断言钉住（它是与 M5 共享的口径，不许悄悄改小）。
    assert_eq!(
        Options::default().max_steps,
        100_000_000,
        "默认预算与 M5 同口径（1e8）"
    );
    let (m, tt) = spin();
    let t0 = std::time::Instant::now();
    let out = run_ssa(
        &m,
        &tt,
        Options {
            max_steps: 10_000_000,
            ..Options::default()
        },
    );
    let dt = t0.elapsed();
    assert_eq!(out.exit, 2);
    assert_eq!(out.steps, 10_000_000, "用满给定预算");
    assert!(
        dt < std::time::Duration::from_secs(10),
        "1e7 步的死循环必须 10 s 内停下（实测约 1 s），实际 {dt:?}"
    );
}

#[test]
fn t_ssa_40_无限递归按帧深上限停下() {
    // `main` 调自己：压帧不设限就是宿主栈溢出（段错误），所以 `max_frames`
    // 必须真的拦住。判据是「退出码 2 + 诊断里有帧深上限」，而不是「没崩」——
    // 「没崩」在宿主栈够大时也可能成立（然后 OOM）。
    let mut b = FnBuilder::new("main");
    let e = b.entry();
    b.instr(
        e,
        Instr::Call {
            dsts: Vec::new(),
            callee: CalleeRef::Func(rgoc_hir::FuncId(0)),
            args: Vec::new(),
            dots: false,
            effect: Effect::default(),
        },
    );
    b.term(e, Term::Return(Vec::new()));
    let (f, tt) = b.into_parts();
    let m = Module {
        main: Some(rgoc_hir::FuncId(0)),
        ..module_of(vec![f])
    };
    let out = run_ssa(&m, &tt, Options::default());
    assert_eq!(out.exit, 2, "栈溢出 ⇒ 退出码 2：{out:?}");
    let diag = out.diag.unwrap_or_default();
    assert!(
        diag.contains("栈溢出") && diag.contains("1000"),
        "诊断要说明帧深上限，实际：{diag:?}"
    );
}

// ── Task 49（T-SSA-52）：块参数交接必须是**并行**的 ─────────────────

#[test]
fn t_ssa_52_回边上对调两个块参数() {
    // 手搓一个「回边把两个块参数对调」的图：
    //
    //   b0: v0=1; v1=2; v2=0; goto b1(v0,v1,v2)
    //   b1(p0,p1,pc): v4 = lt pc 3; if v4 -> b2() | b3(p0,p1)
    //   b2: v6 = add pc 1; goto b1(p1,p0,v6)      ← 实参是**本块参数自己**
    //   b3(pa,pb): println(pa,pb); return
    //
    // # 为什么必须手搓
    //
    // 构建器**不会**产出这种边：HIR 的 `a, b = b, a` 靠「改名」解决
    // （实测 dump：`goto b4 (v4,v3,v5)`，交的是 latch 自己的参数），
    // 所以端到端 fixture 打不到 `jump` 的并行语义。
    //
    // # 判据
    //
    // 三轮交换 ⇒ 1,2 → 2,1 → 1,2 → 2,1。顺序赋值会得到 2,2（先写 p0←p1，
    // 再读 p0 已经变了）—— 差一个数，但那是**静默**的。
    let mut b = FnBuilder::new("main");
    let int = b.basic("int");
    let bt = b.basic("bool");
    let b0 = b.entry();
    let b1 = b.block();
    let b2 = b.block();
    let b3 = b.block();

    let v0 = b.value(int);
    let v1 = b.value(int);
    let v2 = b.value(int);
    b.instr(
        b0,
        Instr::Const {
            dst: v0,
            c: ConstVal::Int(1),
            typ: int,
        },
    );
    b.instr(
        b0,
        Instr::Const {
            dst: v1,
            c: ConstVal::Int(2),
            typ: int,
        },
    );
    b.instr(
        b0,
        Instr::Const {
            dst: v2,
            c: ConstVal::Int(0),
            typ: int,
        },
    );
    b.term(
        b0,
        Term::Goto {
            to: b1,
            args: vec![v0, v1, v2],
        },
    );

    let p0 = b.value(int);
    let p1 = b.value(int);
    let pc = b.value(int);
    b.set_params(b1, &[p0, p1, pc]);
    let three = b.value(int);
    let lt = b.value(bt);
    b.instr(
        b1,
        Instr::Const {
            dst: three,
            c: ConstVal::Int(3),
            typ: int,
        },
    );
    b.instr(
        b1,
        Instr::Bin {
            dst: lt,
            op: BinOp::Lt,
            lhs: pc,
            rhs: three,
            typ: bt,
        },
    );
    b.term(
        b1,
        Term::If {
            cond: lt,
            then_: (b2, Vec::new()),
            else_: (b3, vec![p0, p1]),
        },
    );

    let one = b.value(int);
    let next = b.value(int);
    b.instr(
        b2,
        Instr::Const {
            dst: one,
            c: ConstVal::Int(1),
            typ: int,
        },
    );
    b.instr(
        b2,
        Instr::Bin {
            dst: next,
            op: BinOp::Add,
            lhs: pc,
            rhs: one,
            typ: int,
        },
    );
    // 这一条就是判据：实参 (p1, p0) 与块参数 (p0, p1) 互为别名
    b.term(
        b2,
        Term::Goto {
            to: b1,
            args: vec![p1, p0, next],
        },
    );

    let pa = b.value(int);
    let pb = b.value(int);
    b.set_params(b3, &[pa, pb]);
    b.instr(
        b3,
        Instr::Call {
            dsts: Vec::new(),
            callee: CalleeRef::Builtin(rgoc_hir::BuiltinKind::Println),
            args: vec![pa, pb],
            dots: false,
            effect: Effect::default(),
        },
    );
    b.term(b3, Term::Return(Vec::new()));

    let (f, tt) = b.into_parts();
    let m = Module {
        main: Some(rgoc_hir::FuncId(0)),
        ..module_of(vec![f])
    };
    let out = run_ssa(&m, &tt, Options::default());
    assert_eq!(out.exit, 0, "正常退出：{out:?}");
    assert_eq!(
        out.output, "2 1\n",
        "三轮交换后应是 2,1；顺序赋值会得到 2,2（静默算错）"
    );
}
