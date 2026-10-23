//! HIR→SSA 构建的判据（`M6-plan.md` Task 17 起；`T-SSA-15` 及后续）。
//!
//! # 为什么这条测试住在 `rgoc-driver`
//!
//! 判据是**端到端**的：从 Go 源码 → load → check → lower → build。
//! `rgoc-ssa` 只看得到 `Program`，看不到前面四步怎么接起来；把测试写在
//! `rgoc-ssa` 里就得在测试里手搓 `Program`（那是 `testsupport` 的活，
//! 而 `Program` 有 5 个字段、`Func` 有 13 个，手搓出来的一定和真实 lowering 漂开）。
//!
//! # fixture 怎么放
//!
//! 内联字符串写到 `target/tmp/ssa-build` 下（沿 `run_semantics.rs` 的做法：
//! **不**往只读语料 `go_source_code/` 里写东西）。

use std::path::PathBuf;

use rgoc_check::CheckOptions;
use rgoc_hir::Program;
use rgoc_load::load_with_imports;
use rgoc_ssa::build::build;
use rgoc_ssa::ir::Term;
use rgoc_ssa::verify::verify;
use rgoc_types::TypeTable;

/// 把源码跑完整条前端，返回 `(Program, TypeTable)`。
fn program_of(name: &str, src: &str) -> (Program, TypeTable) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/ssa-build");
    std::fs::create_dir_all(&dir).expect("建不了 target/tmp/ssa-build");
    let path = dir.join(format!("{name}.go"));
    std::fs::write(&path, src).expect("写不了 fixture");

    let mut tt = TypeTable::new();
    let (pkg, bag) = load_with_imports(&mut tt, &[path], &[], None);
    let opts = CheckOptions { test_hooks: false };
    let (checked, lowered) =
        rgoc_check::check::check_package_with(&pkg, &mut tt, bag, &opts, |s| {
            rgoc_hir::lower(&pkg, s)
        });
    assert_eq!(
        checked.rendered_count(),
        0,
        "fixture 本身不该有诊断（有诊断就说明 fixture 写错了）"
    );
    let program = lowered.expect("fixture 不该被 lowering 拒绝");
    (program, tt)
}

#[test]
fn t_ssa_15_空函数建一块零指令() {
    let (program, tt) = program_of("empty-main", "package main\n\nfunc main() {}\n");
    let out = build(&program, &tt);

    assert!(out.rejected.is_empty(), "空函数不该被拒");
    assert_eq!(out.module.funcs.len(), 1, "一个函数一个 SsaFunc");
    let f = &out.module.funcs[0];
    assert_eq!(f.name, "main");
    assert_eq!(f.src.0, program.main.expect("可执行程序必有 main").0);
    assert_eq!(f.blocks.len(), 1, "空体只有一个入口块");
    assert!(f.blocks[0].instrs.is_empty(), "空体没有指令");
    assert!(f.blocks[0].params.is_empty(), "main 没有形参");
    assert_eq!(f.blocks[0].term, Term::Return(Vec::new()), "空体直接返回");
    assert!(f.roots.is_empty(), "空体不建根");
    assert_eq!(f.value_count(), 0, "空体没有值");
    assert!(verify(f, &tt).is_ok(), "建出来的图必须过 verifier");
}

#[test]
fn t_ssa_15_形参成为入口块参数() {
    // 形参槽 → 入口块块参数（`C10` 规则 2：入口块的 params 就是函数形参）
    let (program, tt) = program_of(
        "params",
        "package main\n\nfunc f(a int, b bool) {}\n\nfunc main() {}\n",
    );
    let out = build(&program, &tt);
    assert!(out.rejected.is_empty());
    let f = out
        .module
        .funcs
        .iter()
        .find(|x| x.name == "f")
        .expect("没建出 f");
    assert_eq!(f.params.len(), 2, "两个形参两个值");
    assert_eq!(f.blocks[0].params, f.params, "入口块参数 == 函数形参");
    assert_eq!(f.value_count(), 2);
    assert!(verify(f, &tt).is_ok(), "形参图必须过 verifier");
}

#[test]
fn t_ssa_15_模块保留_globals_与_main() {
    let (program, tt) = program_of("globals", "package main\n\nvar g int\n\nfunc main() {}\n");
    let out = build(&program, &tt);
    assert_eq!(out.module.globals.len(), 1, "包级变量要进 Module::globals");
    assert_eq!(
        out.module.globals[0].0, 0,
        "GlobalId 就是 Program::globals 的下标"
    );
    assert!(out.module.main.is_some());
}

// ── Task 18：取址分析（`T-SSA-27` 的前半）────────────────────────────

use rgoc_hir::{ExprKind, Place, SlotId, Stmt};
use rgoc_ssa::build::{roots_of, scan_addressed};
use rgoc_ssa::ir::Root;

/// 从 `p := &<place>` 这一句里取出 `(p 的槽, 被取址的最内层槽)`。
fn short_decl_addr(body: &rgoc_hir::Block) -> (SlotId, SlotId) {
    for s in &body.stmts {
        if let Stmt::ShortDecl { slots, rhs } = s
            && let Some(e) = rhs.first()
            && let ExprKind::Addr(p) = &e.kind
            && let Some(inner) = innermost_slot(p)
        {
            return (slots[0], inner);
        }
    }
    panic!("fixture 里没有 `p := &x` 形态");
}

fn innermost_slot(p: &Place) -> Option<SlotId> {
    match p {
        Place::Slot(s) => Some(*s),
        Place::Field { base, .. } => innermost_slot(base),
        Place::Index { .. } | Place::Deref(_) | Place::Global(_) => None,
    }
}

#[test]
fn t_ssa_27_取址分析认出被取址的槽() {
    let (program, tt) = program_of(
        "addressed",
        "package main\n\nfunc f() {\n\tvar a int\n\tp := &a\n\t*p = 2\n}\n\nfunc main() {}\n",
    );
    let func = program
        .funcs
        .iter()
        .find(|x| x.name == "f")
        .expect("没有 f");
    let (declared, addressed) = short_decl_addr(&func.body);

    let set = scan_addressed(&tt, &func.body);
    assert_eq!(
        set.iter().copied().collect::<Vec<_>>(),
        vec![addressed],
        "只有 a 被取址"
    );
    assert_ne!(declared, addressed, "被取址的是 a，不是 p");
    assert!(!set.contains(&declared), "p 本身没有被取址");

    let (roots, slot_root) = roots_of(&tt, &func.body);
    assert_eq!(roots, vec![Root::Slot(addressed)], "一个槽根");
    assert_eq!(slot_root.get(&addressed), Some(&rgoc_ssa::ir::RootId(0)));
}

#[test]
fn t_ssa_27_结构体字段取址也算槽被取址() {
    // `&s.f` 取的是 s 的一部分 ⇒ s 必须是根（漏判就会把 s 提升成值、写穿丢效果）
    let (program, tt) = program_of(
        "field-addr",
        "package main\n\ntype S struct{ f int }\n\nfunc g() {\n\tvar s S\n\tp := &s.f\n\t_ = p\n}\n\nfunc main() {}\n",
    );
    let func = program.funcs.iter().find(|x| x.name == "g").unwrap();
    let (_, addressed) = short_decl_addr(&func.body);
    let set = scan_addressed(&tt, &func.body);
    assert_eq!(set.len(), 1, "只有 s 一个槽：{set:?}");
    assert!(set.contains(&addressed), "被取址的槽必须是 s");
}

#[test]
fn t_ssa_27_slice_元素取址不建槽根() {
    // `&s[0]`（s 是 slice）的元素在**底层数组**（堆根），s 这个槽不被取址。
    // 若这里误建槽根，就是把「值」当成「存储」，纯值化机会白扔。
    let (program, tt) = program_of(
        "slice-addr",
        "package main\n\nfunc h() {\n\ts := []int{1}\n\tp := &s[0]\n\t_ = p\n}\n\nfunc main() {}\n",
    );
    let func = program.funcs.iter().find(|x| x.name == "h").unwrap();
    let set = scan_addressed(&tt, &func.body);
    assert!(set.is_empty(), "slice 元素取址不该产生槽根：{set:?}");
}

#[test]
fn t_ssa_27_数组元素取址要让数组槽成根() {
    // `&arr[0]`（arr 是**数组**）的元素就在 arr 的存储里 ⇒ arr 是根。
    // 与上一条对照：同样是 `&x[i]`，类型不同、结论相反。
    let (program, tt) = program_of(
        "array-addr",
        "package main\n\nfunc k() {\n\tvar arr [3]int\n\tp := &arr[0]\n\t_ = p\n}\n\nfunc main() {}\n",
    );
    let func = program.funcs.iter().find(|x| x.name == "k").unwrap();
    let set = scan_addressed(&tt, &func.body);
    assert_eq!(set.len(), 1, "数组槽必须成为根：{set:?}");
}

#[test]
fn t_ssa_27_没有取址就没有根() {
    let (program, tt) = program_of(
        "no-addr",
        "package main\n\nfunc m() {\n\tvar a int\n\ta = 1\n\t_ = a\n}\n\nfunc main() {}\n",
    );
    let func = program.funcs.iter().find(|x| x.name == "m").unwrap();
    assert!(scan_addressed(&tt, &func.body).is_empty());
    let (roots, slot_root) = roots_of(&tt, &func.body);
    assert!(roots.is_empty() && slot_root.is_empty());
}
