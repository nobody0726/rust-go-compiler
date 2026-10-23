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
    let (program, mut tt) = program_of("empty-main", "package main\n\nfunc main() {}\n");
    let out = build(&program, &mut tt);

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
    let (program, mut tt) = program_of(
        "params",
        "package main\n\nfunc f(a int, b bool) {}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
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
    let (program, mut tt) = program_of("globals", "package main\n\nvar g int\n\nfunc main() {}\n");
    let out = build(&program, &mut tt);
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

// ── Task 19–21：提升、直线表达式、赋值 ───────────────────────────────

use rgoc_ssa::ir::{Instr, SsaFunc};

fn func_named<'a>(out: &'a rgoc_ssa::build::BuildOutcome, name: &str) -> &'a SsaFunc {
    out.module
        .funcs
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("没建出 {name}"))
}

/// 直接看指令（`kind_of_instr` 的白名单之外还有 `Instr` 的字段，所以这里用 enum 匹配）
fn instrs(f: &SsaFunc) -> Vec<&Instr> {
    f.blocks.iter().flat_map(|b| b.instrs.iter()).collect()
}

fn count_kind(f: &SsaFunc, kind: &str) -> usize {
    instrs(f)
        .iter()
        .filter(|i| rgoc_ssa::ir::kind_of_instr(i) == kind)
        .count()
}

#[test]
fn t_ssa_15_直线表达式建出常量与算术() {
    // 注意：常量表达式会被**检查器**折叠（`1 + 2*3` 到 HIR 只剩一个 `Const(7)`），
    // 所以要看算术指令必须让操作数来自变量。
    let (program, mut tt) = program_of(
        "linear",
        "package main\n\nfunc f(n int) int {\n\ta := n + 1\n\treturn a * 2\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    assert!(out.rejected.is_empty());
    let f = func_named(&out, "f");
    if let Err(e) = verify(f, &tt) {
        panic!(
            "直线表达式必须过 verifier：{e}\n{}",
            rgoc_ssa::dump::dump_func(f, &tt)
        );
    }

    assert_eq!(count_kind(f, "const"), 2, "两条常量（1 与 2）");
    // `kind_of_instr` 只到「bin」这一层（加/乘都是 `bin`），所以按 `op` 细分
    let ops: Vec<rgoc_hir::BinOp> = instrs(f)
        .into_iter()
        .filter_map(|i| match i {
            Instr::Bin { op, .. } => Some(*op),
            _ => None,
        })
        .collect();
    assert_eq!(
        ops,
        vec![rgoc_hir::BinOp::Add, rgoc_hir::BinOp::Mul],
        "先 `n + 1` 再 `a * 2`（求值顺序也要对）"
    );
    assert_eq!(
        count_kind(f, "load") + count_kind(f, "store"),
        0,
        "纯表达式不该访存"
    );
    assert!(f.roots.is_empty(), "纯表达式不该建根");
    assert_eq!(count_kind(f, "load"), 0);
}

#[test]
fn t_ssa_19_无取址函数里没有访存也没有根() {
    // `T-SSA-26`：`a` 确定赋值且未取址 ⇒ 全程纯值
    let (program, mut tt) = program_of(
        "promote",
        "package main\n\nfunc f() int {\n\ta := 1\n\ta = a + 1\n\treturn a\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let f = func_named(&out, "f");
    assert!(verify(f, &tt).is_ok(), "提升后的图必须过 verifier");
    assert_eq!(count_kind(f, "load"), 0, "不该有 load：{f:#?}");
    assert_eq!(count_kind(f, "store"), 0, "不该有 store：{f:#?}");
    assert!(f.roots.is_empty(), "不该建任何根：{:?}", f.roots);
    assert!(f.slot_root.is_empty(), "slot_root 必须空");
}

#[test]
fn t_ssa_26_加一行取址就必须出现根与访存() {
    // 反向守卫：同一份函数体加 `p := &a; _ = p`，提升必须让位给内存
    let (program, mut tt) = program_of(
        "address-taken",
        "package main\n\nfunc f() int {\n\ta := 1\n\tp := &a\n\t_ = p\n\ta = a + 1\n\treturn a\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let f = func_named(&out, "f");
    assert!(verify(f, &tt).is_ok(), "取址后的图必须过 verifier");
    assert_eq!(f.roots.len(), 1, "一个槽根：{:?}", f.roots);
    assert_eq!(f.slot_root.len(), 1, "slot_root 一条");
    assert!(count_kind(f, "store") >= 1, "赋值必须走 store：{f:#?}");
    assert!(count_kind(f, "load") >= 1, "读必须走 load：{f:#?}");
}

#[test]
fn t_ssa_15_短声明与赋值更新当前值() {
    // `a, b := 1, 2; a = b; return a` ⇒ return 的就是 `const 2` 那个值
    let (program, mut tt) = program_of(
        "assign",
        "package main\n\nfunc f() int {\n\ta, b := 1, 2\n\ta = b\n\treturn a\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let f = func_named(&out, "f");
    assert!(verify(f, &tt).is_ok());

    // 找出 const 2 的值
    let two = instrs(f).into_iter().find_map(|i| match i {
        Instr::Const { dst, c, .. } if *c == rgoc_hir::types::ConstVal::Int(2) => Some(*dst),
        _ => None,
    });
    let two = two.expect("没有 const 2");
    match &f.blocks[0].term {
        rgoc_ssa::ir::Term::Return(vs) => {
            assert_eq!(
                vs,
                &vec![two],
                "return 的必须是 `2` 那个值（当前值映射）\n{}",
                rgoc_ssa::dump::dump_func(f, &tt)
            );
        }
        other => panic!("终止符不是 return：{other:?}"),
    }
    assert_eq!(count_kind(f, "copy"), 0, "不该靠 copy 搬运：{f:#?}");
}

#[test]
fn t_ssa_28_并行赋值a_b_eq_b_a() {
    // `a, b := 1, 2; a, b = b, a` 之后 `a` 必须是 2、`b` 是 1。
    // 判据落在**结构**上：`a*10+b` 的两个操作数分别是 const2 与 const1。
    let (program, mut tt) = program_of(
        "parallel",
        "package main\n\nfunc f() int {\n\ta, b := 1, 2\n\ta, b = b, a\n\treturn a*10 + b\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let f = func_named(&out, "f");
    assert!(verify(f, &tt).is_ok());

    let mut one = None;
    let mut two = None;
    for i in instrs(f) {
        if let Instr::Const { dst, c, .. } = i {
            match c {
                rgoc_hir::types::ConstVal::Int(1) => one = Some(*dst),
                rgoc_hir::types::ConstVal::Int(2) => two = Some(*dst),
                _ => {}
            }
        }
    }
    let (one, two) = (one.expect("缺 const 1"), two.expect("缺 const 2"));
    let mul = instrs(f).into_iter().find_map(|i| match i {
        Instr::Bin {
            dst, op, lhs, rhs, ..
        } if *op == rgoc_hir::BinOp::Mul => Some((*dst, *lhs, *rhs)),
        _ => None,
    });
    let (mul_dst, ml, _) = mul.expect("缺乘法");
    assert_eq!(ml, two, "`a*10` 的 `a` 必须是 const 2（交换成功）");
    let add = instrs(f).into_iter().find_map(|i| match i {
        Instr::Bin { dst, op, rhs, .. } if *op == rgoc_hir::BinOp::Add && *dst != mul_dst => {
            Some(*rhs)
        }
        _ => None,
    });
    assert_eq!(
        add,
        Some(one),
        "`+ b` 的 `b` 必须是 const 1（没被先写 a 串掉）"
    );
}

#[test]
fn t_ssa_21_字段赋值让槽进根表() {
    // `s.f = 1` 不是取址，但也是「部分写」⇒ 整个 s 走内存（SSA 没有改字段的指令）
    let (program, mut tt) = program_of(
        "partial-write",
        "package main\n\ntype S struct{ f int }\n\nfunc g() int {\n\tvar s S\n\ts.f = 1\n\treturn s.f\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let g = func_named(&out, "g");
    assert!(verify(g, &tt).is_ok(), "部分写的图必须过 verifier：{g:#?}");
    assert_eq!(g.roots.len(), 1, "s 必须成根：{:?}", g.roots);
    assert!(count_kind(g, "field") >= 1, "字段地址要出现 field：{g:#?}");
    assert!(count_kind(g, "store") >= 1, "`s.f = 1` 要写内存");
}

#[test]
fn t_ssa_19_未赋值的槽留在内存里() {
    // `var a int` 从没被写过就返回 ⇒ 读的是**零值**。SSA 没有「零值」指令，
    // 所以这个槽必须留在内存里，由帧初始化的零值表负责。
    let (program, mut tt) = program_of(
        "uninit",
        "package main\n\nfunc f() int {\n\tvar a int\n\treturn a\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let f = func_named(&out, "f");
    assert!(verify(f, &tt).is_ok(), "必须过 verifier：{f:#?}");
    assert_eq!(f.roots.len(), 1, "未赋值的槽必须成根：{:?}", f.roots);
    assert!(count_kind(f, "load") >= 1, "读它必须走 load：{f:#?}");
}

#[test]
fn t_ssa_21_只写不读的字段赋值也要建根() {
    // `s.f = 1` 之后再也不读 s：这时「读前未赋值」那条**兜不住**它，
    // 只有「部分写」这条能让 s 进根表 —— 否则写指令无处可写（会 panic）。
    let (program, mut tt) = program_of(
        "partial-only-write",
        "package main\n\ntype S struct{ f int }\n\nfunc g() {\n\tvar s S\n\ts.f = 1\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let g = func_named(&out, "g");
    assert!(verify(g, &tt).is_ok(), "必须过 verifier：{g:#?}");
    assert_eq!(g.roots.len(), 1, "s 必须成根：{:?}", g.roots);
    assert!(count_kind(g, "store") >= 1, "写字段要走 store");
}

// ── Task 22：if/else 与块参数 ────────────────────────────────────────

use rgoc_ssa::ir::{BlockId, ValueId};

/// 常量 `n` 在 IR 里的**最终值**：跟随写入点物化的 `Convert` 链。
///
/// `a := 1` 的 `1` 在 HIR 里是 `untyped int`，写进 `int` 槽时会插一条 `Convert`
/// （见 `Fb::coerce_to`）。所以「那个常量的值」不是 Const 的 dst，而是链尾。
fn const_of(f: &SsaFunc, n: i128) -> ValueId {
    let all = instrs(f);
    let mut v = all
        .iter()
        .find_map(|i| match i {
            Instr::Const { dst, c, .. } if *c == rgoc_hir::types::ConstVal::Int(n) => Some(*dst),
            _ => None,
        })
        .unwrap_or_else(|| panic!("缺 const {n}"));
    loop {
        let next = all.iter().find_map(|i| match i {
            Instr::Convert { dst, x, .. } if *x == v => Some(*dst),
            _ => None,
        });
        match next {
            Some(nv) => v = nv,
            None => return v,
        }
    }
}

#[allow(dead_code)]
fn raw_const_of(f: &SsaFunc, n: i128) -> ValueId {
    instrs(f)
        .into_iter()
        .find_map(|i| match i {
            Instr::Const { dst, c, .. } if *c == rgoc_hir::types::ConstVal::Int(n) => Some(*dst),
            _ => None,
        })
        .unwrap_or_else(|| panic!("缺 const {n}"))
}

fn goto_of(f: &SsaFunc, b: BlockId) -> (BlockId, Vec<ValueId>) {
    match &f.blocks[b.index()].term {
        Term::Goto { to, args } => (*to, args.clone()),
        other => panic!("b{} 的终止符不是 goto：{other:?}", b.0),
    }
}

#[test]
fn t_ssa_16_if_汇合块取两支都改过的值() {
    let (program, mut tt) = program_of(
        "if-join",
        "package main\n\nfunc f(c bool) int {\n\ta := 1\n\tif c {\n\t\ta = 2\n\t}\n\treturn a\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let f = func_named(&out, "f");
    if let Err(e) = verify(f, &tt) {
        panic!(
            "必须过 verifier：{e}\n{}",
            rgoc_ssa::dump::dump_func(f, &tt)
        );
    }
    // 入口块 if → then | else
    let (then_b, else_b) = match &f.blocks[0].term {
        Term::If { then_, else_, .. } => (then_.0, else_.0),
        other => panic!("入口块不是 if：{other:?}"),
    };
    let (join_t, args_t) = goto_of(f, then_b);
    let (join_e, args_e) = goto_of(f, else_b);
    assert_eq!(join_t, join_e, "两支必须汇到同一块");
    assert_eq!(
        f.blocks[join_t.index()].params.len(),
        1,
        "两支的 a 不同 ⇒ 汇合块一个参数"
    );
    assert_eq!(args_t.len(), 1, "then 边要给实参");
    assert_eq!(args_e.len(), 1, "else 边要给实参");
    assert_ne!(
        args_t[0], args_e[0],
        "两侧的 a 值必须不同（否则不该建参数）"
    );
    assert_eq!(args_t[0], const_of(f, 2), "then 侧的 a 是 2");
    assert_eq!(args_e[0], const_of(f, 1), "else 侧的 a 还是 1");
}

#[test]
fn t_ssa_16_两侧值相同就不建参数() {
    let (program, mut tt) = program_of(
        "if-nochange",
        "package main\n\nfunc f(c bool) int {\n\ta := 1\n\tif c {\n\t\t_ = a\n\t}\n\treturn a\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let f = func_named(&out, "f");
    assert!(verify(f, &tt).is_ok(), "必须过 verifier：{f:#?}");
    let (then_b, _) = match &f.blocks[0].term {
        Term::If { then_, else_, .. } => (then_.0, else_.0),
        other => panic!("入口块不是 if：{other:?}"),
    };
    let (join, args) = goto_of(f, then_b);
    assert!(
        args.is_empty(),
        "两侧的 a 是同一个值 ⇒ 不该有参数：{args:?}"
    );
    assert!(f.blocks[join.index()].params.is_empty(), "汇合块无参数");
}

#[test]
fn t_ssa_16_一支返回时汇合块只有一个前驱() {
    let (program, mut tt) = program_of(
        "if-return",
        "package main\n\nfunc f(c bool) int {\n\tif c {\n\t\treturn 1\n\t}\n\treturn 0\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let f = func_named(&out, "f");
    assert!(verify(f, &tt).is_ok(), "必须过 verifier：{f:#?}");
    // then 支 return ⇒ 只有 else 支落到汇合块
    let (then_b, else_b) = match &f.blocks[0].term {
        Term::If { then_, else_, .. } => (then_.0, else_.0),
        other => panic!("入口块不是 if：{other:?}"),
    };
    assert!(matches!(f.blocks[then_b.index()].term, Term::Return(_)));
    let (join, args) = goto_of(f, else_b);
    assert!(args.is_empty(), "单前驱不需要参数：{args:?}");
    assert!(f.blocks[join.index()].params.is_empty());
    assert!(matches!(f.blocks[join.index()].term, Term::Return(_)));
}

#[test]
fn t_ssa_16_两支都返回不建死块() {
    let (program, mut tt) = program_of(
        "if-both-return",
        "package main\n\nfunc f(c bool) int {\n\tif c {\n\t\treturn 1\n\t} else {\n\t\treturn 2\n\t}\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let f = func_named(&out, "f");
    assert!(
        verify(f, &tt).is_ok(),
        "必须过 verifier（死块会被 V8 拒）：{f:#?}"
    );
    assert_eq!(
        f.blocks.len(),
        3,
        "入口 + then + else，不该有汇合空块：{f:#?}"
    );
}

#[test]
fn t_ssa_28_跨块交换是并行的() {
    // `if c { a, b = b, a }`：then 边的两个实参必须是 (2, 1) 原值，
    // 而不是「先写 a 再读 a」得到的 (2, 2)。
    let (program, mut tt) = program_of(
        "if-swap",
        "package main\n\nfunc f(c bool) int {\n\ta, b := 1, 2\n\tif c {\n\t\ta, b = b, a\n\t}\n\treturn a*10 + b\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let f = func_named(&out, "f");
    if let Err(e) = verify(f, &tt) {
        panic!(
            "必须过 verifier：{e}\n{}",
            rgoc_ssa::dump::dump_func(f, &tt)
        );
    }
    let (one, two) = (const_of(f, 1), const_of(f, 2));
    let (then_b, else_b) = match &f.blocks[0].term {
        Term::If { then_, else_, .. } => (then_.0, else_.0),
        other => panic!("入口块不是 if：{other:?}"),
    };
    let (_, args_t) = goto_of(f, then_b);
    let (_, args_e) = goto_of(f, else_b);
    assert_eq!(args_t, vec![two, one], "then 边：a←2, b←1（并行交换）");
    assert_eq!(args_e, vec![one, two], "else 边：a←1, b←2（没交换）");
}

// ── Task 23：for 三形态与回边 ────────────────────────────────────────

extern crate rgoc_ssa;

fn cfg_of(f: &SsaFunc) -> rgoc_ssa::cfg::Cfg {
    rgoc_ssa::cfg::Cfg::build(f)
}

/// 找「有 2 条以上前驱」的块（回边的循环头）。
fn headers(f: &SsaFunc) -> Vec<BlockId> {
    let c = cfg_of(f);
    (0..f.blocks.len())
        .map(|i| BlockId(i as u32))
        .filter(|b| c.preds(*b).len() >= 2)
        .collect()
}

#[test]
fn t_ssa_17_for_空条件死循环_回边存在() {
    let (program, mut tt) = program_of(
        "for-ever",
        "package main\n\nfunc f(n int) {\n\tfor {\n\t\tn = n\n\t}\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let f = func_named(&out, "f");
    assert!(verify(f, &tt).is_ok(), "必须过 verifier：{f:#?}");
    let hs = headers(f);
    assert_eq!(hs.len(), 1, "恰有一个循环头（2 条前驱）：{hs:?}");
    let c = cfg_of(f);
    let preds = c.preds(hs[0]);
    assert_eq!(preds.len(), 2, "前驱 = preheader + latch：{preds:?}");
    // `for{}` 无出口：函数尾不可达，且没有「第三个」块从 header 出去
    assert!(
        matches!(
            &f.blocks[hs[0].index()].term,
            rgoc_ssa::ir::Term::Goto { .. }
        ),
        "无条件的 for 头不该是 if：{:?}",
        f.blocks[hs[0].index()].term
    );
}

#[test]
fn t_ssa_17_for_单条件是_if_头() {
    let (program, mut tt) = program_of(
        "for-cond",
        "package main\n\nfunc f(n int) int {\n\ts := 0\n\tfor n > 0 {\n\t\ts = s + n\n\t\tn = n - 1\n\t}\n\treturn s\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let f = func_named(&out, "f");
    if let Err(e) = verify(f, &tt) {
        panic!(
            "必须过 verifier：{e}\n{}",
            rgoc_ssa::dump::dump_func(f, &tt)
        );
    }
    let hs = headers(f);
    assert_eq!(hs.len(), 1, "一个循环头：{hs:?}");
    let h = hs[0];
    assert!(
        matches!(f.blocks[h.index()].term, rgoc_ssa::ir::Term::If { .. }),
        "循环头的终止符必须是 if：{:?}",
        f.blocks[h.index()].term
    );
    // 出口：header 的 else 边
    let (_, exit) = match &f.blocks[h.index()].term {
        rgoc_ssa::ir::Term::If { then_, else_, .. } => (then_.0, else_.0),
        other => panic!("{other:?}"),
    };
    assert_eq!(cfg_of(f).preds(exit), vec![h], "出口只有循环头一条前驱");
}

#[test]
fn t_ssa_17_for_三句式带_latch_且携带_i() {
    let (program, mut tt) = program_of(
        "for-three",
        "package main\n\nfunc f() int {\n\ts := 0\n\tfor i := 0; i < 3; i++ {\n\t\ts = s + i\n\t}\n\treturn s\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let f = func_named(&out, "f");
    if let Err(e) = verify(f, &tt) {
        panic!(
            "必须过 verifier：{e}\n{}",
            rgoc_ssa::dump::dump_func(f, &tt)
        );
    }
    let hs = headers(f);
    assert_eq!(hs.len(), 1, "一个循环头：{hs:?}");
    let h = hs[0];
    // 携带值：`s` 与 `i`（都在循环里被改，且进入循环时已有值）
    assert_eq!(
        f.blocks[h.index()].params.len(),
        2,
        "循环头该有 2 个参数（s / i）：{}\n{}",
        f.blocks[h.index()].params.len(),
        rgoc_ssa::dump::dump_func(f, &tt)
    );
    // preheader 的 args 与 latch 的 args 都得是 2 个（V7 已查，这里再钉一次语义）
    let c = cfg_of(f);
    for p in c.preds(h) {
        let args = c.edge_args(*p, h);
        assert_eq!(args.len(), 2, "b{}→b{} 要传 2 个实参：{args:?}", p.0, h.0);
    }
}

#[test]
fn t_ssa_17_循环体里声明的槽不当携带值() {
    // `x := i` 每轮新建 ⇒ 不能出现在循环头参数里。
    // 若把它当携带值，回边就会传入「上一轮的 x」—— 类型对、值可疑，
    // 而且大部分 fixture 仍绿（这正是设计 §8 说的「静默错」型）。
    let (program, mut tt) = program_of(
        "for-fresh-local",
        "package main\n\nfunc f() int {\n\ts := 0\n\tfor i := 0; i < 3; i++ {\n\t\tx := i\n\t\ts = s + x\n\t}\n\treturn s\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let f = func_named(&out, "f");
    assert!(verify(f, &tt).is_ok(), "必须过 verifier：{f:#?}");
    let h = headers(f)[0];
    assert_eq!(
        f.blocks[h.index()].params.len(),
        2,
        "只有 s 与 i 携带，x 不携带：{}",
        rgoc_ssa::dump::dump_func(f, &tt)
    );
}

#[test]
fn t_ssa_17_循环里写堆时_mem_令牌要携带() {
    // 循环里经指针写 ⇒ 堆根的 mem 必须在循环头合流（否则循环后的写与循环里的写
    // 在 mem 链上失去先后关系）。
    let (program, mut tt) = program_of(
        "for-heap-mem",
        "package main\n\nfunc f(p *int) {\n\tfor i := 0; i < 3; i++ {\n\t\t*p = i\n\t}\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let f = func_named(&out, "f");
    if let Err(e) = verify(f, &tt) {
        panic!(
            "必须过 verifier：{e}\n{}",
            rgoc_ssa::dump::dump_func(f, &tt)
        );
    }
    let h = headers(f)[0];
    // 携带：i（槽）+ 堆根的 mem
    assert_eq!(
        f.blocks[h.index()].params.len(),
        2,
        "i 与堆 mem 各一个参数：{}\n{}",
        f.blocks[h.index()].params.len(),
        rgoc_ssa::dump::dump_func(f, &tt)
    );
}

/// 一个值定义在哪个块（块参数或指令结果）。
fn def_block(f: &SsaFunc, v: ValueId) -> BlockId {
    for (i, b) in f.blocks.iter().enumerate() {
        if b.params.contains(&v) {
            return BlockId(i as u32);
        }
        for ins in &b.instrs {
            let dst = match ins {
                Instr::Const { dst, .. }
                | Instr::Copy { dst, .. }
                | Instr::Bin { dst, .. }
                | Instr::Un { dst, .. }
                | Instr::Convert { dst, .. }
                | Instr::AddrOf { dst, .. }
                | Instr::Field { dst, .. }
                | Instr::Index { dst, .. }
                | Instr::Slice { dst, .. }
                | Instr::Composite { dst, .. }
                | Instr::Make { dst, .. }
                | Instr::Load { dst, .. }
                | Instr::Store { dst, .. } => Some(*dst),
                Instr::Call { dsts, .. } => {
                    if dsts.contains(&v) {
                        Some(v)
                    } else {
                        None
                    }
                }
            };
            if dst == Some(v) {
                return BlockId(i as u32);
            }
        }
    }
    panic!("v{} 没有定义点", v.0);
}

#[test]
fn t_ssa_17_回边传的是本轮算出的新值() {
    // 这条打的是「回边把 header 参数原样传回」这种**看起来对**的错：
    // 类型对、个数对、verifier 绿，但循环体等于没执行（死循环或算错）。
    let (program, mut tt) = program_of(
        "for-backedge",
        "package main\n\nfunc f() int {\n\ts := 0\n\tfor i := 0; i < 3; i++ {\n\t\ts = s + i\n\t}\n\treturn s\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let f = func_named(&out, "f");
    assert!(verify(f, &tt).is_ok());
    let h = headers(f)[0];
    let c = cfg_of(f);
    // 回边 = 前驱里不是「入口块」的那个（preheader 是 b0）
    let latch = *c
        .preds(h)
        .iter()
        .find(|p| p.index() != 0)
        .expect("没有回边前驱");
    let args = c.edge_args(latch, h);
    assert_eq!(args.len(), 2);
    for (k, a) in args.iter().enumerate() {
        let db = def_block(f, *a);
        assert_ne!(
            db, h,
            "第 {k} 个回边实参 v{} 定义在循环头本身 ⇒ 等于把参数原样传回（循环体白跑）",
            a.0
        );
        assert!(
            db == latch || c.preds(latch).contains(&db) || db == c.preds(h)[0],
            "第 {k} 个回边实参 v{} 定义在 b{}，不在本轮路径上",
            a.0,
            db.0
        );
    }
}

#[test]
fn t_ssa_17_循环之后读到的是合流值() {
    // `return s` 里的 `s` 必须是**循环头的参数**（初值与回边的合流点），
    // 而不是循环前的那个值。若出口状态取错，函数会返回初值 —— 类型对、
    // verifier 绿、单测里「循环了几次」没人看，于是静默算错。
    let (program, mut tt) = program_of(
        "for-exit-value",
        "package main\n\nfunc f() int {\n\ts := 0\n\tfor i := 0; i < 3; i++ {\n\t\ts = s + i\n\t}\n\treturn s\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let f = func_named(&out, "f");
    assert!(verify(f, &tt).is_ok());
    let h = headers(f)[0];
    let exit = match &f.blocks[h.index()].term {
        rgoc_ssa::ir::Term::If { else_, .. } => else_.0,
        other => panic!("循环头不是 if：{other:?}"),
    };
    let ret = match &f.blocks[exit.index()].term {
        rgoc_ssa::ir::Term::Return(vs) => vs.clone(),
        other => panic!("出口块不是 return：{other:?}"),
    };
    assert_eq!(ret.len(), 1);
    assert_eq!(
        def_block(f, ret[0]),
        h,
        "返回值必须定义在循环头（合流点），实际在 b{}\n{}",
        def_block(f, ret[0]).0,
        rgoc_ssa::dump::dump_func(f, &tt)
    );
}

#[test]
fn t_ssa_07_if_之后_mem_必须合流() {
    // 只有一条分支写内存时，合流后的 mem 必须是**合的**（块参数），
    // 而不是「最后建的那条分支的 mem」。
    //
    // 后者看起来没事：V2/V6 都绿（旧 mem 同样支配、同块也不乱序），
    // 但它把「then 里那次 store」从 mem 链上摘掉了 —— 之后的写会被认为
    // 与它无序。这是**静默**型的错，只有结构断言抓得住。
    let (program, mut tt) = program_of(
        "if-mem-merge",
        "package main\n\nfunc f(c bool, p *int) int {\n\tif c {\n\t\t*p = 1\n\t}\n\treturn *p\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let f = func_named(&out, "f");
    if let Err(e) = verify(f, &tt) {
        panic!(
            "必须过 verifier：{e}\n{}",
            rgoc_ssa::dump::dump_func(f, &tt)
        );
    }
    // 找那个 if 的汇合块
    let (then_b, else_b) = match &f.blocks[0].term {
        rgoc_ssa::ir::Term::If { then_, else_, .. } => (then_.0, else_.0),
        other => panic!("入口块不是 if：{other:?}"),
    };
    let join = match &f.blocks[then_b.index()].term {
        rgoc_ssa::ir::Term::Goto { to, .. } => *to,
        other => panic!("then 支不是 goto：{other:?}"),
    };
    assert_eq!(
        match &f.blocks[else_b.index()].term {
            rgoc_ssa::ir::Term::Goto { to, .. } => *to,
            other => panic!("else 支不是 goto：{other:?}"),
        },
        join
    );
    // 汇合块之后的那条 load 的 mem 必须是汇合块的参数
    let load_mem = f.blocks[join.index()]
        .instrs
        .iter()
        .find_map(|i| match i {
            Instr::Load { mem, .. } => Some(*mem),
            _ => None,
        })
        .expect("合流块里没有 load");
    assert_eq!(
        def_block(f, load_mem),
        join,
        "load 的 mem 定义在 b{}，不是合流块 b{} ⇒ 分支里的 store 被摘出了 mem 链\n{}",
        def_block(f, load_mem).0,
        join.0,
        rgoc_ssa::dump::dump_func(f, &tt)
    );
}

// ── Task 24：switch ────────────────────────────────────────────────

#[test]
fn t_ssa_18_switch_有_tag() {
    let (program, mut tt) = program_of(
        "switch-tag",
        "package main\n\nfunc f(x int) int {\n\ts := 0\n\tswitch x {\n\tcase 1, 2:\n\t\ts = 10\n\tcase 3:\n\t\ts = 30\n\tdefault:\n\t\ts = 99\n\t}\n\treturn s\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let f = func_named(&out, "f");
    if let Err(e) = verify(f, &tt) {
        panic!(
            "必须过 verifier：{e}\n{}",
            rgoc_ssa::dump::dump_func(f, &tt)
        );
    }
    // 入口块是第一个判定（`if`）
    assert!(
        matches!(f.blocks[0].term, rgoc_ssa::ir::Term::If { .. }),
        "入口块应该是判定：{:?}",
        f.blocks[0].term
    );
    // `case 1, 2` 是一个 case ⇒ 一个 `LOr` 合并两个 `Eq`
    let eqs = instrs(f)
        .iter()
        .filter(|i| {
            matches!(
                i,
                Instr::Bin {
                    op: rgoc_hir::BinOp::Eq,
                    ..
                }
            )
        })
        .count();
    let ors = instrs(f)
        .iter()
        .filter(|i| {
            matches!(
                i,
                Instr::Bin {
                    op: rgoc_hir::BinOp::LOr,
                    ..
                }
            )
        })
        .count();
    assert_eq!(eqs, 3, "三个 case 表达式各一次 Eq");
    assert_eq!(ors, 1, "只有 `case 1, 2` 需要一次 Or");
}

#[test]
fn t_ssa_18_switch_无_tag_是_if_链() {
    let (program, mut tt) = program_of(
        "switch-notag",
        "package main\n\nfunc f(x int) int {\n\ts := 0\n\tswitch {\n\tcase x > 10:\n\t\ts = 1\n\tcase x > 0:\n\t\ts = 2\n\tdefault:\n\t\ts = 3\n\t}\n\treturn s\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let f = func_named(&out, "f");
    assert!(verify(f, &tt).is_ok(), "必须过 verifier：{f:#?}");
    // 无 tag ⇒ 没有任何 Eq（条件本身就是表达式）
    assert_eq!(
        instrs(f)
            .iter()
            .filter(|i| matches!(
                i,
                Instr::Bin {
                    op: rgoc_hir::BinOp::Eq,
                    ..
                }
            ))
            .count(),
        0,
        "无 tag 的 switch 不该产生 Eq：{}",
        rgoc_ssa::dump::dump_func(f, &tt)
    );
}

#[test]
fn t_ssa_18_fallthrough_直接进下一个体() {
    // `fallthrough` 不重测下一个 case 的条件，所以它的目标必须是
    // **下一个 case 的体块**，不是下一个测试块。
    let (program, mut tt) = program_of(
        "switch-fallthrough",
        "package main\n\nfunc f(x int) int {\n\ts := 0\n\tswitch x {\n\tcase 1:\n\t\ts = 1\n\t\tfallthrough\n\tcase 2:\n\t\ts = s + 2\n\tdefault:\n\t\ts = 9\n\t}\n\treturn s\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let f = func_named(&out, "f");
    if let Err(e) = verify(f, &tt) {
        panic!(
            "必须过 verifier：{e}\n{}",
            rgoc_ssa::dump::dump_func(f, &tt)
        );
    }
    // case1 的体：必须有一条 goto，且目标是某个**含加法的块**（case2 的体）
    let add_block = f
        .blocks
        .iter()
        .position(|b| {
            b.instrs.iter().any(|i| {
                matches!(
                    i,
                    Instr::Bin {
                        op: rgoc_hir::BinOp::Add,
                        ..
                    }
                )
            })
        })
        .map(|i| BlockId(i as u32))
        .expect("没有加法块");
    let from = f
        .blocks
        .iter()
        .position(|b| matches!(&b.term, rgoc_ssa::ir::Term::Goto { to, .. } if *to == add_block))
        .map(|i| BlockId(i as u32));
    assert!(
        from.is_some(),
        "case1 体必须直接 goto 到含加法的块（case2 体）\n{}",
        rgoc_ssa::dump::dump_func(f, &tt)
    );
    // case2 的体块有两条前驱：判定链 + case1 的 fallthrough（这就是「直接进体」的证据）
    let c = cfg_of(f);
    assert_eq!(
        c.preds(add_block).len(),
        2,
        "case2 体该有 2 条前驱（判定 + fallthrough）：{:?}\n{}",
        c.preds(add_block),
        rgoc_ssa::dump::dump_func(f, &tt)
    );
}

#[test]
fn t_ssa_18_没有_default_时一条都没匹配也算一条路径() {
    // `y := 0; switch x { case 1: y = 2 }; return y`
    // 若不合「一条都没匹配」这条路径，`return` 的 y 会变成 2。
    let (program, mut tt) = program_of(
        "switch-nomatch",
        "package main\n\nfunc f(x int) int {\n\ty := 0\n\tswitch x {\n\tcase 1:\n\t\ty = 2\n\t}\n\treturn y\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let f = func_named(&out, "f");
    if let Err(e) = verify(f, &tt) {
        panic!(
            "必须过 verifier：{e}\n{}",
            rgoc_ssa::dump::dump_func(f, &tt)
        );
    }
    let join = headers(f);
    assert_eq!(join.len(), 1, "一个合流块：{join:?}");
    let j = join[0];
    assert_eq!(
        f.blocks[j.index()].params.len(),
        1,
        "y 必须是合流参数（三路：case 体 / nomatch / —— ）\n{}",
        rgoc_ssa::dump::dump_func(f, &tt)
    );
    // return 的值必须定义在合流块上
    let ret = match &f.blocks[j.index()].term {
        rgoc_ssa::ir::Term::Return(vs) => vs.clone(),
        other => panic!("合流块不是 return：{other:?}"),
    };
    assert_eq!(def_block(f, ret[0]), j, "return 的必须是合流值");
    // nomatch 路径给的实参必须是 const 0（初值），不能是 2
    let c = cfg_of(f);
    let preds = c.preds(j).to_vec();
    let mut zero_seen = false;
    for p in preds {
        let args = c.edge_args(p, j);
        if args.is_empty() {
            continue;
        }
        let db = def_block(f, args[0]);
        let is_zero = f.blocks[db.index()].instrs.iter().any(|i| {
            matches!(i, Instr::Const { dst, c, .. }
                if *dst == args[0] && *c == rgoc_hir::types::ConstVal::Int(0))
        });
        zero_seen |= is_zero;
    }
    assert!(
        zero_seen,
        "「一条都没匹配」的那条路径必须传初值 0\n{}",
        rgoc_ssa::dump::dump_func(f, &tt)
    );
}
