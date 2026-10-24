//! `ssa_exec` 的判据（`M6-plan.md` Task 32 起；`M6-tests.md` 的 `O`/`S` 腿）。
//!
//! # 为什么这些测试手搓 IR
//!
//! 端到端的「Go 源码 → 跑 SSA」在 `rgoc-driver` 的测试里（那边才拿得到 HIR）。
//! 这一层要单独测**执行器**：给一张确定的图，看它算出什么。手搓 IR 才能
//! 精确控制「块参数 / 边实参 / 内存根」这些端到端不好构造的形态。
//! 工具用 `rgoc_ssa::testsupport`（`D-M6-11` 的地基）。

use rgoc_interp::{Options, run_ssa};
use rgoc_ssa::ir::{Module, Term};
use rgoc_ssa::testsupport::{FnBuilder, module_of};

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
