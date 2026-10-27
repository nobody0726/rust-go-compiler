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

    let set = scan_addressed(&tt, func);
    assert_eq!(
        set.iter().copied().collect::<Vec<_>>(),
        vec![addressed],
        "只有 a 被取址"
    );
    assert_ne!(declared, addressed, "被取址的是 a，不是 p");
    assert!(!set.contains(&declared), "p 本身没有被取址");

    let (roots, slot_root) = roots_of(&tt, func);
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
    let set = scan_addressed(&tt, func);
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
    let set = scan_addressed(&tt, func);
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
    let set = scan_addressed(&tt, func);
    assert_eq!(set.len(), 1, "数组槽必须成为根：{set:?}");
}

#[test]
fn t_ssa_27_没有取址就没有根() {
    let (program, tt) = program_of(
        "no-addr",
        "package main\n\nfunc m() {\n\tvar a int\n\ta = 1\n\t_ = a\n}\n\nfunc main() {}\n",
    );
    let func = program.funcs.iter().find(|x| x.name == "m").unwrap();
    assert!(scan_addressed(&tt, func).is_empty());
    let (roots, slot_root) = roots_of(&tt, func);
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
                // `go` 没有 dst
                Instr::Go { .. } => continue,
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
                Instr::TypeAssert { dst, .. } => Some(*dst),
                // `defer` / `nil-check` 没有 dst（`M9/Phase 5` / `5c`）
                Instr::Defer { .. } | Instr::NilCheck { .. } => None,
                Instr::FuncAddr { dst, .. } | Instr::CapturePtr { dst, .. } => Some(*dst),
                Instr::MakeClosure { dst, .. } => Some(*dst),
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
    // 出口块有自己的参数（`exit` 与 header 共用布局），所以返回值定义在出口块上；
    // 关键是它**由 header 的参数喂进来** —— 即「出口看到的是合流值」。
    assert_eq!(
        def_block(f, ret[0]),
        exit,
        "返回值必须定义在出口块（它拿的是合流值），实际在 b{}\n{}",
        def_block(f, ret[0]).0,
        rgoc_ssa::dump::dump_func(f, &tt)
    );
    let c = cfg_of(f);
    let cargs = c.edge_args(h, exit).to_vec();
    assert!(!cargs.is_empty(), "header→exit 必须有实参");
    for a in &cargs {
        assert_eq!(
            def_block(f, *a),
            h,
            "header→exit 的实参必须来自 header 自己的参数（合流值），实际来自 b{}",
            def_block(f, *a).0
        );
    }
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
    // `case 1, 2` 是**两个**判定，不是一条 `LOr`（`R-35-3`）：
    // `case f(), g():` 里 `f()` 命中就不该求值 `g()`，而一条 `bin lor` 意味着
    // 两个操作数都已经求值过了（副作用做了两次）。所以判据是「没有 `LOr`」。
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
    assert_eq!(
        ors, 0,
        "判定链里不许有 `LOr`（那是「两个操作数都已求值」的形态，会重复副作用）"
    );
    // `case 1, 2` 的两个判定必须是**两块**：第一个不中才去求第二个
    assert!(
        f.blocks.len() >= 5,
        "三个 case 判定 + 体块 + 出口，至少 5 块：{:#?}",
        f.blocks.len()
    );
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

// ── Task 25：goto 与 labeled ────────────────────────────────────────

#[test]
fn t_ssa_19_前向_goto() {
    let (program, mut tt) = program_of(
        "goto-forward",
        "package main\n\nfunc f() int {\n\tx := 0\n\tgoto L\n\tx = 1\nL:\n\treturn x\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let f = func_named(&out, "f");
    if let Err(e) = verify(f, &tt) {
        panic!(
            "必须过 verifier：{e}\n{}",
            rgoc_ssa::dump::dump_func(f, &tt)
        );
    }
    // 入口 + 标签块；`goto` 之后那句 `x = 1` 是死代码，不建
    assert_eq!(
        f.blocks.len(),
        2,
        "前向 goto 只该有两个块：{}\n{}",
        f.blocks.len(),
        rgoc_ssa::dump::dump_func(f, &tt)
    );
    assert!(
        matches!(f.blocks[0].term, rgoc_ssa::ir::Term::Goto { .. }),
        "入口块应以 goto 结束：{:?}",
        f.blocks[0].term
    );
    // 返回的是 `x` 的初值 0（`x = 1` 不可达）
    let ret = match &f.blocks[1].term {
        rgoc_ssa::ir::Term::Return(vs) => vs.clone(),
        other => panic!("标签块不是 return：{other:?}"),
    };
    assert_eq!(ret.len(), 1);
    assert!(
        f.blocks[1].instrs.iter().any(|i| matches!(
            i,
            Instr::Load { dst, .. } if *dst == ret[0]
        )),
        "goto 模式不提升 ⇒ 返回的是 Load 出来的值\n{}",
        rgoc_ssa::dump::dump_func(f, &tt)
    );
}

#[test]
fn t_ssa_19_反向_goto_构成循环() {
    let (program, mut tt) = program_of(
        "goto-backward",
        "package main\n\nfunc f() int {\n\ti := 0\n\ts := 0\nL:\n\ts = s + i\n\ti = i + 1\n\tif i < 3 {\n\t\tgoto L\n\t}\n\treturn s\n}\n\nfunc main() {}\n",
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
    assert_eq!(hs.len(), 1, "标签块该有 2 条前驱（入口 + 回边）：{hs:?}");
    let c = cfg_of(f);
    // 两条边都必须送同样的实参个数（都送全部根）
    let h = hs[0];
    let n = f.blocks[h.index()].params.len();
    assert!(
        n > 0,
        "标签块该有参数（全部根）：{}",
        rgoc_ssa::dump::dump_func(f, &tt)
    );
    for p in c.preds(h) {
        assert_eq!(
            c.edge_args(*p, h).len(),
            n,
            "b{}→b{} 实参个数不符",
            p.0,
            h.0
        );
    }
    // goto 模式：不提升 ⇒ 槽都有根、`val` 里没有槽的当前值
    assert!(
        !f.slot_root.is_empty(),
        "goto 模式必须把槽放进内存：{:?}",
        f.slot_root
    );
}

#[test]
fn t_ssa_19_labeled_break_与_continue() {
    let (program, mut tt) = program_of(
        "labeled",
        "package main\n\nfunc f() int {\n\ts := 0\nOuter:\n\tfor i := 0; i < 3; i++ {\n\t\tfor j := 0; j < 3; j++ {\n\t\t\tif j == 1 {\n\t\t\t\tcontinue Outer\n\t\t\t}\n\t\t\tif i == 2 {\n\t\t\t\tbreak Outer\n\t\t\t}\n\t\t\ts = s + 1\n\t\t}\n\t}\n\treturn s\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let f = func_named(&out, "f");
    if let Err(e) = verify(f, &tt) {
        panic!(
            "必须过 verifier：{e}\n{}",
            rgoc_ssa::dump::dump_func(f, &tt)
        );
    }
    // `continue Outer` 必须跳到外层循环的 latch（`i++` 所在块），
    // `break Outer` 跳到外层循环的 exit。两者都在，所以至少有两个「多前驱」块。
    let c = cfg_of(f);
    let multi = (0..f.blocks.len())
        .map(|i| BlockId(i as u32))
        .filter(|b| c.preds(*b).len() >= 2)
        .count();
    assert!(
        multi >= 2,
        "外层 header 与外层 exit 都该有多个前驱：{multi}\n{}",
        rgoc_ssa::dump::dump_func(f, &tt)
    );
}

#[test]
fn t_ssa_19_无标签_break_与_continue() {
    let (program, mut tt) = program_of(
        "plain-break",
        "package main\n\nfunc f() int {\n\ts := 0\n\tfor i := 0; i < 10; i++ {\n\t\tif i == 5 {\n\t\t\tbreak\n\t\t}\n\t\tif i == 2 {\n\t\t\tcontinue\n\t\t}\n\t\ts = s + 1\n\t}\n\treturn s\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let f = func_named(&out, "f");
    if let Err(e) = verify(f, &tt) {
        panic!(
            "必须过 verifier：{e}\n{}",
            rgoc_ssa::dump::dump_func(f, &tt)
        );
    }
    // break 的边与 header 的 else 边都要落到 exit 上 ⇒ exit 有两条前驱
    let c = cfg_of(f);
    let exit = (0..f.blocks.len())
        .map(|i| BlockId(i as u32))
        .find(|b| matches!(f.blocks[b.index()].term, rgoc_ssa::ir::Term::Return(_)))
        .expect("没有 return 块");
    assert!(
        c.preds(exit).len() >= 2,
        "exit 该有 header + break 两条前驱：{:?}\n{}",
        c.preds(exit),
        rgoc_ssa::dump::dump_func(f, &tt)
    );
}

#[test]
fn t_ssa_19_continue_必须走_post() {
    // `continue` 在 `for init;cond;post` 里**先执行 post**。若它直接跳 header，
    // 循环变量不递增 ⇒ 死循环；而且 verifier 全绿（参数个数、类型都对）。
    let (program, mut tt) = program_of(
        "continue-post",
        "package main\n\nfunc f() int {\n\ts := 0\n\tfor i := 0; i < 10; i++ {\n\t\tif i == 2 {\n\t\t\tcontinue\n\t\t}\n\t\ts = s + 1\n\t}\n\treturn s\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let f = func_named(&out, "f");
    if let Err(e) = verify(f, &tt) {
        panic!(
            "必须过 verifier：{e}\n{}",
            rgoc_ssa::dump::dump_func(f, &tt)
        );
    }
    let c = cfg_of(f);
    let h = headers(f)[0];
    // 判据：**有 post 时，循环头只能有两条前驱** —— preheader 与 latch。
    // `continue` 若直接跳 header，就会多出第三条（而 verifier 全绿：类型与个数都对）。
    assert_eq!(
        c.preds(h).len(),
        2,
        "循环头该只有 preheader + latch 两条前驱，实际 {:?} ⇒ 有 `continue` 绕过了 post\n{}",
        c.preds(h),
        rgoc_ssa::dump::dump_func(f, &tt)
    );
}

// ── Task 26：多返回值与结果直连 ─────────────────────────────────────

/// 按被调方找 `Call` 指令（返回 (dsts, args)）。
fn call_to(f: &SsaFunc, fid: rgoc_hir::FuncId) -> Option<(Vec<ValueId>, Vec<ValueId>)> {
    instrs(f).into_iter().find_map(|i| match i {
        Instr::Call {
            dsts,
            callee: rgoc_ssa::ir::CalleeRef::Func(x),
            args,
            ..
        } if *x == fid => Some((dsts.clone(), args.clone())),
        _ => None,
    })
}

fn func_id(program: &rgoc_hir::Program, name: &str) -> rgoc_hir::FuncId {
    rgoc_hir::FuncId(
        program
            .funcs
            .iter()
            .position(|f| f.name == name)
            .unwrap_or_else(|| panic!("没有 {name}")) as u32,
    )
}

/// 按**名字后缀**找函数（方法名形如 `T.M`，但前缀可能是包名，别硬编码）。
fn func_id_suffix(program: &rgoc_hir::Program, suffix: &str) -> rgoc_hir::FuncId {
    rgoc_hir::FuncId(
        program
            .funcs
            .iter()
            .position(|f| f.name.ends_with(suffix) || f.name == suffix)
            .unwrap_or_else(|| {
                let all: Vec<&str> = program.funcs.iter().map(|f| f.name.as_str()).collect();
                panic!("没有以 {suffix} 结尾的函数；现有：{all:?}")
            }) as u32,
    )
}

#[test]
fn t_ssa_20_多值接收建出多结果指令() {
    let (program, mut tt) = program_of(
        "multi-recv",
        "package main\n\nfunc two() (int, int) {\n\treturn 1, 2\n}\n\nfunc g() int {\n\ta, b := two()\n\treturn a*10 + b\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let g = func_named(&out, "g");
    if let Err(e) = verify(g, &tt) {
        panic!(
            "必须过 verifier：{e}\n{}",
            rgoc_ssa::dump::dump_func(g, &tt)
        );
    }
    let two = func_id(&program, "two");
    let (dsts, args) = call_to(g, two).expect("没有对 two 的调用");
    assert_eq!(dsts.len(), 2, "两个结果 ⇒ 两个 dst");
    assert!(args.is_empty(), "two 没有形参");
    // 两个 dst 都必须被用到（`a*10 + b`）
    let used = |v: ValueId| {
        instrs(g).into_iter().any(|i| match i {
            Instr::Bin { lhs, rhs, .. } => *lhs == v || *rhs == v,
            _ => false,
        })
    };
    assert!(
        used(dsts[0]) && used(dsts[1]),
        "两个结果都要被用上：{dsts:?}"
    );
}

#[test]
fn t_ssa_21_return_直连_无中间值() {
    // `return two()` 的 `Return` 操作数必须**就是** two 的 dsts。
    // 若中间插了 copy / 重新落槽，值是一样的，但结构上就不是「直连」了 ——
    // 这条判据是结构断言，不是看输出。
    let (program, mut tt) = program_of(
        "ret-direct",
        "package main\n\nfunc two() (int, int) {\n\treturn 1, 2\n}\n\nfunc h() (int, int) {\n\treturn two()\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let h = func_named(&out, "h");
    if let Err(e) = verify(h, &tt) {
        panic!(
            "必须过 verifier：{e}\n{}",
            rgoc_ssa::dump::dump_func(h, &tt)
        );
    }
    let two = func_id(&program, "two");
    let (dsts, _) = call_to(h, two).expect("没有对 two 的调用");
    let ret = match &h.blocks[0].term {
        rgoc_ssa::ir::Term::Return(vs) => vs.clone(),
        other => panic!("入口块不是 return：{other:?}"),
    };
    assert_eq!(
        ret,
        dsts,
        "Return 的操作数必须是被调方的 dsts（顺序也一致）\n{}",
        rgoc_ssa::dump::dump_func(h, &tt)
    );
    // 且没有额外的 copy / convert 插在中间
    assert_eq!(
        count_kind(h, "copy"),
        0,
        "直连不该有 copy：{}",
        rgoc_ssa::dump::dump_func(h, &tt)
    );
}

#[test]
fn t_ssa_22_实参直连_按_spread_展平() {
    // `sum(two())`：sum 的两个实参必须是 two 的两个 dst（顺序一致）。
    let (program, mut tt) = program_of(
        "arg-spread",
        "package main\n\nfunc two() (int, int) {\n\treturn 1, 2\n}\n\nfunc sum(a, b int) int {\n\treturn a + b\n}\n\nfunc k() int {\n\treturn sum(two())\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let k = func_named(&out, "k");
    if let Err(e) = verify(k, &tt) {
        panic!(
            "必须过 verifier：{e}\n{}",
            rgoc_ssa::dump::dump_func(k, &tt)
        );
    }
    let two = func_id(&program, "two");
    let sum = func_id(&program, "sum");
    let (two_dsts, _) = call_to(k, two).expect("没有对 two 的调用");
    let (_, sum_args) = call_to(k, sum).expect("没有对 sum 的调用");
    assert_eq!(
        sum_args,
        two_dsts,
        "`sum(two())` 的实参必须就是 two 的结果（按 spread 展平，顺序一致）\n{}",
        rgoc_ssa::dump::dump_func(k, &tt)
    );
    assert_eq!(
        count_kind(k, "copy"),
        0,
        "不该靠 copy 搬运：{}",
        rgoc_ssa::dump::dump_func(k, &tt)
    );
}

#[test]
fn t_ssa_20_单值调用与零结果调用() {
    // `println` 是 0 结果；`len` 是 1 结果。两者都不能让构建器崩。
    let (program, mut tt) = program_of(
        "call-arities",
        "package main\n\nfunc one(x int) int {\n\treturn x + 1\n}\n\nfunc p(s []int) {\n\tprintln(len(s))\n\tprintln(one(1))\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let p = func_named(&out, "p");
    if let Err(e) = verify(p, &tt) {
        panic!(
            "必须过 verifier：{e}\n{}",
            rgoc_ssa::dump::dump_func(p, &tt)
        );
    }
    let zero_dst = instrs(p)
        .into_iter()
        .filter(|i| matches!(i, Instr::Call { dsts, .. } if dsts.is_empty()))
        .count();
    assert!(zero_dst >= 2, "两次 println 都是 0 结果调用：{zero_dst}");
}

// ── Task 27：调用摘要（effect）──────────────────────────────────────

/// 找到 f 里对被调方 `fid` 的那次调用，取它的 `effect`。
fn effect_of_call(f: &SsaFunc, fid: rgoc_hir::FuncId) -> rgoc_ssa::ir::Effect {
    instrs(f)
        .into_iter()
        .find_map(|i| match i {
            Instr::Call {
                callee: rgoc_ssa::ir::CalleeRef::Func(x),
                effect,
                ..
            } if *x == fid => Some(effect.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("没找到对 func#{} 的调用", fid.0))
}

fn global_index(program: &rgoc_hir::Program, name: &str) -> u32 {
    program
        .globals
        .iter()
        .position(|g| g.name == name)
        .unwrap_or_else(|| panic!("没有全局 {name}")) as u32
}

#[test]
fn t_ssa_23_写全局的函数_摘要里写到了那个全局() {
    let (program, mut tt) = program_of(
        "effect-global",
        "package main\n\nvar g int\n\nfunc setg() {\n\tg = 1\n}\n\nfunc m() {\n\tsetg()\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let m = func_named(&out, "m");
    assert!(verify(m, &tt).is_ok());
    let e = effect_of_call(m, func_id(&program, "setg"));
    let gi = global_index(&program, "g");
    assert!(
        e.writes.contains(&rgoc_ssa::ir::RootId(gi)),
        "摘要必须含全局 g（RootId({gi})）：{:?}",
        e.writes
    );
    assert!(!e.writes_all, "只写一个已知全局，不该是 writes_all：{e:?}");
}

#[test]
fn t_ssa_23_只读函数_摘要为空() {
    let (program, mut tt) = program_of(
        "effect-readonly",
        "package main\n\nfunc rd(a int) int {\n\treturn a + 1\n}\n\nfunc m() int {\n\treturn rd(1)\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let m = func_named(&out, "m");
    let e = effect_of_call(m, func_id(&program, "rd"));
    assert!(e.writes.is_empty(), "只读函数不该写：{:?}", e.writes);
    assert!(!e.writes_all, "不该是 writes_all：{e:?}");
    assert!(!e.may_panic, "不该 panic：{e:?}");
}

#[test]
fn t_ssa_23_摘要沿调用链传递() {
    // `a()` → `b()` → 写 g：a 的摘要也要含 g（并集到不动点）。
    let (program, mut tt) = program_of(
        "effect-transitive",
        "package main\n\nvar g int\n\nfunc b() {\n\tg = 2\n}\n\nfunc a() {\n\tb()\n}\n\nfunc m() {\n\ta()\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let m = func_named(&out, "m");
    let e = effect_of_call(m, func_id(&program, "a"));
    let gi = global_index(&program, "g");
    assert!(
        e.writes.contains(&rgoc_ssa::ir::RootId(gi)),
        "经 b 传递的写必须出现在 a 的摘要里：{:?}",
        e.writes
    );
}

#[test]
fn t_ssa_23_纯递归函数不是_writes_all() {
    // 环检测 + `writes_all` 的写法会把 fib 误标成「什么都能写」。
    // 并集到不动点不会：fib 什么都没写，收敛后还是空。
    let (program, mut tt) = program_of(
        "effect-recursive",
        "package main\n\nfunc fib(n int) int {\n\tif n < 2 {\n\t\treturn n\n\t}\n\treturn fib(n-1) + fib(n-2)\n}\n\nfunc m() int {\n\treturn fib(5)\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let m = func_named(&out, "m");
    let e = effect_of_call(m, func_id(&program, "fib"));
    assert!(!e.writes_all, "纯递归函数不该被判成 writes_all：{e:?}");
    assert!(e.writes.is_empty(), "fib 什么都不写：{:?}", e.writes);
    assert!(!e.may_panic, "fib 不 panic：{e:?}");
}

#[test]
fn t_ssa_23_指针写与_panic_各自被标记() {
    let (program, mut tt) = program_of(
        "effect-heap-panic",
        "package main\n\nfunc st(p *int) {\n\t*p = 1\n}\n\nfunc boom() {\n\tpanic(\"x\")\n}\n\nfunc m(p *int) {\n\tst(p)\n\tboom()\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let m = func_named(&out, "m");
    let st = effect_of_call(m, func_id(&program, "st"));
    assert!(
        st.writes_all,
        "经指针写 ⇒ writes_all（可能别名任何全局）：{st:?}"
    );
    let boom = effect_of_call(m, func_id(&program, "boom"));
    assert!(boom.may_panic, "`panic(...)` ⇒ may_panic：{boom:?}");
}

// ── Task 28：变参与方法调用 ─────────────────────────────────────────

#[test]
fn t_ssa_29_变参在调用点打包成切片() {
    let (program, mut tt) = program_of(
        "variadic",
        "package main\n\nfunc sum(xs ...int) int {\n\ts := 0\n\tfor _, x := range xs {\n\t\ts = s + x\n\t}\n\treturn s\n}\n\nfunc m() int {\n\treturn sum(1, 2, 3)\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let m = func_named(&out, "m");
    if let Err(e) = verify(m, &tt) {
        panic!(
            "必须过 verifier：{e}\n{}",
            rgoc_ssa::dump::dump_func(m, &tt)
        );
    }
    let fid = func_id(&program, "sum");
    let (_, args) = call_to(m, fid).expect("没有对 sum 的调用");
    assert_eq!(
        args.len(),
        1,
        "变参打包后只应剩一个 slice 实参：{args:?}\n{}",
        rgoc_ssa::dump::dump_func(m, &tt)
    );
    // 那个实参必须是 Composite（三个元素）
    let items = instrs(m).into_iter().find_map(|i| match i {
        Instr::Composite { dst, items, .. } if *dst == args[0] => Some(items.clone()),
        _ => None,
    });
    assert_eq!(
        items.map(|v| v.len()),
        Some(3),
        "三个实参要装进一个 Composite：{}",
        rgoc_ssa::dump::dump_func(m, &tt)
    );
}

#[test]
fn t_ssa_29_变参用_dots_时不打包() {
    let (program, mut tt) = program_of(
        "variadic-dots",
        "package main\n\nfunc sum(xs ...int) int {\n\ts := 0\n\tfor _, x := range xs {\n\t\ts = s + x\n\t}\n\treturn s\n}\n\nfunc m() int {\n\tys := []int{1, 2}\n\treturn sum(ys...)\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let m = func_named(&out, "m");
    // `ys...` 已经把 slice 给出去了，不能再包一层
    let fid = func_id(&program, "sum");
    let (_, args) = call_to(m, fid).expect("没有对 sum 的调用");
    assert_eq!(args.len(), 1, "dots 形态只送一个实参：{args:?}");
    // 关键：`ys...` 已经把 slice 给出去了，**不许**再包一层 Composite。
    // （只数 `args.len()` 抓不到这个错：把 1 个 slice 打包成 1 个 slice 还是 1 个实参。）
    // 这个 fixture 里本来就有 1 个 Composite（`[]int{1,2}` 字面量）⇒ 只许有 1 个。
    assert_eq!(
        count_kind(m, "composite"),
        1,
        "dots 形态不该再包一层 Composite（只该有字面量那一个）：{}",
        rgoc_ssa::dump::dump_func(m, &tt)
    );
}

#[test]
fn t_ssa_30_方法调用带接收者() {
    let (program, mut tt) = program_of(
        "method",
        "package main\n\ntype T struct{ n int }\n\nfunc (t T) Get() int {\n\treturn t.n\n}\n\nfunc (t *T) Set(v int) {\n\tt.n = v\n}\n\nfunc m() int {\n\tvar t T\n\tt.Set(3)\n\treturn t.Get()\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let m = func_named(&out, "m");
    if let Err(e) = verify(m, &tt) {
        panic!(
            "必须过 verifier：{e}\n{}",
            rgoc_ssa::dump::dump_func(m, &tt)
        );
    }
    let get = func_id_suffix(&program, "Get");
    let set = func_id_suffix(&program, "Set");
    let gets = instrs(m)
        .into_iter()
        .filter(|i| matches!(i, Instr::Call { callee: rgoc_ssa::ir::CalleeRef::Method { func, .. }, .. } if *func == get))
        .count();
    let sets = instrs(m)
        .into_iter()
        .filter(|i| matches!(i, Instr::Call { callee: rgoc_ssa::ir::CalleeRef::Method { func, .. }, .. } if *func == set))
        .count();
    assert_eq!(gets, 1, "一次 `t.Get()`");
    assert_eq!(sets, 1, "一次 `t.Set(3)`");
    // 指针接收者的 `Set` 必须有接收者操作数
    let set_args = instrs(m).into_iter().find_map(|i| match i {
        Instr::Call {
            callee: rgoc_ssa::ir::CalleeRef::Method { func, recv },
            args,
            ..
        } if *func == set => Some((*recv, args.clone())),
        _ => None,
    });
    let (recv, sargs) = set_args.expect("没有 Set 调用");
    assert_eq!(sargs.len(), 1, "`Set(3)` 一个实参");
    // 接收者是指针：`AddrOf` 或 `Copy` 出来的值（不能是 `Invalid`）
    assert!(recv.0 < u32::MAX, "接收者必须是个真值");
}

// ── Task 28–29：range 与复合字面量 ──────────────────────────────────

#[test]
fn t_ssa_17_range_遍历切片() {
    let (program, mut tt) = program_of(
        "range-slice",
        "package main\n\nfunc f(xs []int) int {\n\ts := 0\n\tfor i, v := range xs {\n\t\ts = s + i + v\n\t}\n\treturn s\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let f = func_named(&out, "f");
    if let Err(e) = verify(f, &tt) {
        panic!(
            "必须过 verifier：{e}\n{}",
            rgoc_ssa::dump::dump_func(f, &tt)
        );
    }
    // 循环头：preheader + 回边
    assert_eq!(
        headers(f).len(),
        1,
        "一个循环头：{}",
        rgoc_ssa::dump::dump_func(f, &tt)
    );
    // `len(xs)` 用内建调用表达
    let lens = instrs(f)
        .into_iter()
        .filter(|i| {
            matches!(
                i,
                Instr::Call {
                    callee: rgoc_ssa::ir::CalleeRef::Builtin(rgoc_hir::BuiltinKind::Len),
                    ..
                }
            )
        })
        .count();
    assert_eq!(lens, 1, "一条 len 调用：{lens}");
    // `xs[i]` 要出现 Index + Load
    assert!(
        count_kind(f, "index") >= 1,
        "取元素要 Index：{}",
        rgoc_ssa::dump::dump_func(f, &tt)
    );
}

#[test]
fn t_ssa_17_range_没有_key_时用私有槽() {
    // `for range xs`：HIR 没有 key 位，但下标必须跨块携带 ⇒ 构建器私有槽。
    let (program, mut tt) = program_of(
        "range-nokey",
        "package main\n\nfunc f(xs []int) {\n\tfor range xs {\n\t\tprintln(1)\n\t}\n}\n\nfunc main() {}\n",
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
    assert!(
        !f.blocks[h.index()].params.is_empty(),
        "循环头必须携带下标：{}",
        rgoc_ssa::dump::dump_func(f, &tt)
    );
}

#[test]
fn t_ssa_24_结构体字面量按声明序() {
    let (program, mut tt) = program_of(
        "composite-struct",
        "package main\n\ntype S struct{\n\ta int\n\tb int\n}\n\nfunc f() int {\n\ts := S{a: 1, b: 2}\n\treturn s.a + s.b\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let f = func_named(&out, "f");
    if let Err(e) = verify(f, &tt) {
        panic!(
            "必须过 verifier：{e}\n{}",
            rgoc_ssa::dump::dump_func(f, &tt)
        );
    }
    let comp = instrs(f).into_iter().find_map(|i| match i {
        Instr::Composite { items, .. } => Some(items.len()),
        _ => None,
    });
    assert_eq!(
        comp,
        Some(2),
        "两个字段 ⇒ 两个 item：{}",
        rgoc_ssa::dump::dump_func(f, &tt)
    );
}

#[test]
fn t_ssa_24_keyed_数组字面量补零值() {
    // `[4]int{2: 7}`：4 个元素，只有下标 2 是 7，其余是零值。
    let (program, mut tt) = program_of(
        "composite-keyed",
        "package main\n\nfunc f() int {\n\ta := [4]int{2: 7}\n\treturn a[2]\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let f = func_named(&out, "f");
    if let Err(e) = verify(f, &tt) {
        panic!(
            "必须过 verifier：{e}\n{}",
            rgoc_ssa::dump::dump_func(f, &tt)
        );
    }
    let items = instrs(f).into_iter().find_map(|i| match i {
        Instr::Composite { items, .. } => Some(items.clone()),
        _ => None,
    });
    let items = items.expect("没有 Composite");
    assert_eq!(items.len(), 4, "keyed 也要补齐到 4 个元素");
    // 第 2 个是 7，其余是 0
    let consts: Vec<rgoc_hir::types::ConstVal> = instrs(f)
        .into_iter()
        .filter_map(|i| match i {
            Instr::Const { dst, c, .. } if items.contains(dst) => Some(c.clone()),
            _ => None,
        })
        .collect();
    assert!(
        consts.contains(&rgoc_hir::types::ConstVal::Int(7)),
        "下标 2 的 7 要在里面：{consts:?}"
    );
}

// ── Task 30：指针、取址与写穿（`T-SSA-25`）──────────────────────────

use rgoc_ssa::ir::RootId;

/// 返回块里的 `Return` 操作数（直线 fixture 只有一个）。
fn ret_vals(f: &SsaFunc) -> Vec<ValueId> {
    f.blocks
        .iter()
        .find_map(|b| match &b.term {
            Term::Return(v) => Some(v.clone()),
            _ => None,
        })
        .expect("fixture 里没有 return")
}

/// 全部 `Store` 的 `(根, 用的 mem, 产出的 mem)`。
fn stores(f: &SsaFunc) -> Vec<(RootId, ValueId, ValueId)> {
    instrs(f)
        .into_iter()
        .filter_map(|i| match i {
            Instr::Store { dst, mem, root, .. } => Some((*root, *mem, *dst)),
            _ => None,
        })
        .collect()
}

/// 某个值的定义指令（`None` = 块参数 / 越界）。
fn def_instr(f: &SsaFunc, v: ValueId) -> Option<&Instr> {
    instrs(f).into_iter().find(|i| match i {
        // `go` 没有 dst ⇒ 不可能是任何值的定义点
        Instr::Go { .. } => false,
        Instr::Load { dst, .. }
        | Instr::TypeAssert { dst, .. }
        | Instr::FuncAddr { dst, .. }
        | Instr::MakeClosure { dst, .. }
        | Instr::CapturePtr { dst, .. }
        | Instr::Const { dst, .. }
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
        | Instr::Store { dst, .. } => *dst == v,
        Instr::Call { dsts, .. } => dsts.contains(&v),
        // `defer` / `nil-check` 不定义值（`M9/Phase 5` / `5c`）
        Instr::Defer { .. } | Instr::NilCheck { .. } => false,
    })
}

#[test]
fn t_ssa_25_写穿之后读同一个槽走同一条_mem_链() {
    // `p := &a; *p = 2; return a` —— 关键不是「有个 Store」，而是
    // **读 `a` 的那条 Load 用的是那个 Store 产出的 mem**（否则静态上「写后读」不成立）。
    let (program, mut tt) = program_of(
        "ptr-write",
        "package main\n\nfunc f() int {\n\ta := 1\n\tp := &a\n\t*p = 2\n\treturn a\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let f = func_named(&out, "f");
    if let Err(e) = verify(f, &tt) {
        panic!(
            "必须过 verifier：{e}\n{}",
            rgoc_ssa::dump::dump_func(f, &tt)
        );
    }
    // `a` 取过址 ⇒ 它必须有一个槽根
    let slot_root = f
        .roots
        .iter()
        .position(|r| matches!(r, Root::Slot(_)))
        .map(|i| RootId(i as u32))
        .unwrap_or_else(|| {
            panic!(
                "`&a` 必须建槽根：{:?}\n{}",
                f.roots,
                rgoc_ssa::dump::dump_func(f, &tt)
            )
        });
    let ss = stores(f);
    // 槽的初始化也有一条 Store（`a := 1` 落进根），所以这里数不出「1 条」——
    // 判据是「**只有**这个根被写」（没有扇出到堆根）且读用的是最后一条的 mem。
    assert!(
        ss.iter().all(|s| s.0 == slot_root),
        "`*p = 2` 只该写在 `a` 的根上（扇出说明根丢了）：{}",
        rgoc_ssa::dump::dump_func(f, &tt)
    );
    let w = *ss.last().expect("至少一条 Store");
    assert_eq!(
        w.0,
        slot_root,
        "最后一条 Store 必须是写穿：{}",
        rgoc_ssa::dump::dump_func(f, &tt)
    );
    // return 的那个值必须是 Load，且 mem 就是 Store 产出的 mem
    let ret = ret_vals(f);
    assert_eq!(ret.len(), 1, "一个返回值");
    let Some(Instr::Load { mem, root, .. }) = def_instr(f, ret[0]) else {
        panic!(
            "`return a` 必须是 Load（`a` 在内存里）：{}",
            rgoc_ssa::dump::dump_func(f, &tt)
        );
    };
    assert_eq!(*root, slot_root, "读 `a` 与写 `*p` 必须是同一个根");
    assert_eq!(
        *mem,
        w.2,
        "读 `a` 必须用写穿产出的新 mem（`T-SSA-25` 的核心判据）：{}",
        rgoc_ssa::dump::dump_func(f, &tt)
    );
}

#[test]
fn t_ssa_25_写穿结构体字段() {
    // `&s.n` 走 `Field(AddrOf(槽根))`：地址是「根 + 字段偏移」，
    // 写穿与读回必须落同一个根、同一条 mem 链。
    let (program, mut tt) = program_of(
        "ptr-field",
        "package main\n\ntype S struct{ n int }\n\nfunc f() int {\n\tvar s S\n\tp := &s.n\n\t*p = 5\n\treturn s.n\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let f = func_named(&out, "f");
    if let Err(e) = verify(f, &tt) {
        panic!(
            "必须过 verifier：{e}\n{}",
            rgoc_ssa::dump::dump_func(f, &tt)
        );
    }
    let slot_root = f
        .roots
        .iter()
        .position(|r| matches!(r, Root::Slot(_)))
        .map(|i| RootId(i as u32))
        .expect("`&s.n` 必须建 `s` 的槽根");
    let ss = stores(f);
    assert!(
        ss.iter().all(|s| s.0 == slot_root),
        "`&s.n` 的写穿只该落在 `s` 的根上（扇出说明 `Field` 丢了根）：{}",
        rgoc_ssa::dump::dump_func(f, &tt)
    );
    let w = *ss.last().expect("至少一条 Store");
    assert!(
        count_kind(f, "field") >= 2,
        "取址与读回各要一条 Field：{}",
        rgoc_ssa::dump::dump_func(f, &tt)
    );
    let ret = ret_vals(f);
    let Some(Instr::Load { mem, root, .. }) = def_instr(f, ret[0]) else {
        panic!("`return s.n` 必须是 Load");
    };
    assert_eq!(*root, slot_root);
    assert_eq!(*mem, w.2, "读回必须用写穿的新 mem");
}

#[test]
fn t_ssa_25_数组元素写落在数组槽根上() {
    // `a[1] = 7` 与 `a[1]` 读必须**同根同链**。这里最容易出的错是
    // 写路径把 `a` 当值求（`Load` 出一个数组值）再去 `Index` —— 那样写进了堆根，
    // 读却在槽根上，静默丢写。
    let (program, mut tt) = program_of(
        "array-write",
        "package main\n\nfunc f() int {\n\ta := [3]int{}\n\ta[1] = 7\n\treturn a[1]\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let f = func_named(&out, "f");
    if let Err(e) = verify(f, &tt) {
        panic!(
            "必须过 verifier：{e}\n{}",
            rgoc_ssa::dump::dump_func(f, &tt)
        );
    }
    let slot_root = f
        .roots
        .iter()
        .position(|r| matches!(r, Root::Slot(_)))
        .map(|i| RootId(i as u32))
        .expect("数组局部变量必须是根");
    let ss = stores(f);
    assert!(
        ss.iter().all(|s| s.0 == slot_root),
        "元素写必须落在数组槽的根上（扇出说明底座求成了值）：{}",
        rgoc_ssa::dump::dump_func(f, &tt)
    );
    let w = *ss.last().expect("至少一条 Store");
    let ret = ret_vals(f);
    let Some(Instr::Load { mem, root, .. }) = def_instr(f, ret[0]) else {
        panic!("`return a[1]` 必须是 Load");
    };
    assert_eq!(*root, slot_root, "读元素与写元素必须同根");
    assert_eq!(*mem, w.2, "读元素必须用写元素的新 mem");
}

#[test]
fn t_ssa_25_取址全局变量写穿落在全局根上() {
    let (program, mut tt) = program_of(
        "ptr-global",
        "package main\n\nvar g int\n\nfunc f() {\n\tp := &g\n\t*p = 3\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let f = func_named(&out, "f");
    if let Err(e) = verify(f, &tt) {
        panic!(
            "必须过 verifier：{e}\n{}",
            rgoc_ssa::dump::dump_func(f, &tt)
        );
    }
    let ss = stores(f);
    assert_eq!(
        ss.len(),
        1,
        "一条写穿：{}",
        rgoc_ssa::dump::dump_func(f, &tt)
    );
    assert!(
        matches!(f.roots.get(ss[0].0.index()), Some(Root::Global(_))),
        "`&g` 的写必须落在全局根上，实际是 {:?}",
        f.roots.get(ss[0].0.index())
    );
}

#[test]
fn t_ssa_25_根未知的指针写扇出到所有根() {
    // 形参指针的根在 M6 是**未知**的（不做别名推理）。这时不能猜一个根：
    // 猜堆根会让调用方的槽根不前进（静默丢写）。判据 = 每个根各一条 Store。
    let (program, mut tt) = program_of(
        "ptr-unknown",
        "package main\n\nvar g0 int\n\nfunc g(p *int) {\n\t*p = 1\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let g = func_named(&out, "g");
    if let Err(e) = verify(g, &tt) {
        panic!(
            "必须过 verifier：{e}\n{}",
            rgoc_ssa::dump::dump_func(g, &tt)
        );
    }
    let ss = stores(g);
    assert!(
        g.roots.len() >= 2,
        "fixture 该有「全局 + 堆」两个根：{:?}",
        g.roots
    );
    assert_eq!(
        ss.len(),
        g.roots.len(),
        "根未知的写必须对每个根各发一条（扇出），实际 {} 条 / {} 个根：{}",
        ss.len(),
        g.roots.len(),
        rgoc_ssa::dump::dump_func(g, &tt)
    );
    let mut covered: Vec<RootId> = ss.iter().map(|s| s.0).collect();
    covered.sort();
    covered.dedup();
    assert_eq!(covered.len(), g.roots.len(), "每个根都要被写到一次");
}

#[test]
fn t_ssa_25_指针底座的字段读() {
    // `p.n`（`p *S`）：底座是**指针值**，不是槽 —— 初版这里直接 `todo!()`。
    let (program, mut tt) = program_of(
        "ptr-deref-field",
        "package main\n\ntype S struct{ n int }\n\nfunc f(p *S) int {\n\treturn p.n\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let f = func_named(&out, "f");
    if let Err(e) = verify(f, &tt) {
        panic!(
            "必须过 verifier：{e}\n{}",
            rgoc_ssa::dump::dump_func(f, &tt)
        );
    }
    assert_eq!(
        count_kind(f, "field"),
        1,
        "`p.n` 要一条 Field：{}",
        rgoc_ssa::dump::dump_func(f, &tt)
    );
    assert_eq!(
        count_kind(f, "load"),
        1,
        "取字段值要一条 Load：{}",
        rgoc_ssa::dump::dump_func(f, &tt)
    );
}

// ── Task 31：语料实测抓到的四个构建错（每条都先见到真错再修）──────────

#[test]
fn t_ssa_25_复合赋值读数组元素要按值类型建_load() {
    // `a[0] += 1`：`read_place` 初版拿**地址值的类型**（`*[3]int`）当 `Load.typ`，
    // 随后的 `bin add` 就成了 `*[3]int + int`。实测 `turing.go` 的
    // `a[pc] += 1` 被 `V3` 拒。
    let (program, mut tt) = program_of(
        "compound-index",
        "package main\n\nfunc f() int {\n\ta := [3]int{}\n\ta[1] += 2\n\treturn a[1]\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let f = func_named(&out, "f");
    if let Err(e) = verify(f, &tt) {
        panic!(
            "必须过 verifier：{e}\n{}",
            rgoc_ssa::dump::dump_func(f, &tt)
        );
    }
    // 那条喂给 `bin add` 的 Load，类型必须是 `int`，不是指针
    let bad = instrs(f).into_iter().find_map(|i| match i {
        Instr::Bin {
            op: rgoc_hir::BinOp::Add,
            lhs,
            ..
        } => match def_instr(f, *lhs) {
            Some(Instr::Load { typ, .. }) => {
                let is_ptr = matches!(tt.type_of(*typ), rgoc_types::typ::Type::Pointer(_));
                is_ptr.then_some(*typ)
            }
            _ => None,
        },
        _ => None,
    });
    assert!(
        bad.is_none(),
        "复合赋值的 Load 读了指针类型 {:?}：{}",
        bad,
        rgoc_ssa::dump::dump_func(f, &tt)
    );
}

#[test]
fn t_ssa_15_未定型常量参与运算要就地转成目标类型() {
    // `a + 1`：`1` 是 `untyped int`，`a` 是 `int`。不物化转换就是
    // `bin add int untyped-int`（`V3` 拒）。
    let (program, mut tt) = program_of(
        "untyped-operand",
        "package main\n\nfunc f(a int) int {\n\tif a < 3 {\n\t\treturn a + 1\n\t}\n\treturn a * 2\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let f = func_named(&out, "f");
    if let Err(e) = verify(f, &tt) {
        panic!(
            "必须过 verifier（操作数要同型）：{e}\n{}",
            rgoc_ssa::dump::dump_func(f, &tt)
        );
    }
}

#[test]
fn t_ssa_17_range_具名下标的类型跟槽走() {
    // `for i := range MyInt(4)`：`i` 是 `MyInt`，上界 `MyInt(4)` 也是 `MyInt`。
    // 初版拿 `int` 去比（`lt MyInt int`）⇒ `V3` 拒（实测 `range3.go`）。
    let (program, mut tt) = program_of(
        "range-named",
        "package main\n\ntype MyInt int\n\nfunc f() MyInt {\n\ts := MyInt(0)\n\tfor i := range MyInt(4) {\n\t\ts += i\n\t}\n\treturn s\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let f = func_named(&out, "f");
    if let Err(e) = verify(f, &tt) {
        panic!(
            "必须过 verifier：{e}\n{}",
            rgoc_ssa::dump::dump_func(f, &tt)
        );
    }
}

#[test]
fn t_ssa_22_间接调用的结果个数取自函数类型() {
    // `b, x := f(1000, 99)`，`f` 是**形参**（函数值）：签名在它的静态类型里，
    // 不读它就只能给一个 `dst`，于是「右侧 1 个值、左侧 2 个目标」panic
    // （实测 `method5.go`）。
    let (program, mut tt) = program_of(
        "indirect-call",
        "package main\n\nfunc call(f func(int, byte) (byte, int)) (byte, int) {\n\treturn f(1000, 99)\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    // 函数值本身在 M6 面外（`FuncLit`），但这里是**形参**，两面都不拒。
    let f = func_named(&out, "call");
    if let Err(e) = verify(f, &tt) {
        panic!(
            "必须过 verifier：{e}\n{}",
            rgoc_ssa::dump::dump_func(f, &tt)
        );
    }
    let n = instrs(f)
        .into_iter()
        .find_map(|i| match i {
            Instr::Call {
                dsts,
                callee: rgoc_ssa::ir::CalleeRef::Indirect(_),
                ..
            } => Some(dsts.len()),
            _ => None,
        })
        .expect("没有间接调用");
    assert_eq!(
        n,
        2,
        "签名两个结果 ⇒ 两个 dsts：{}",
        rgoc_ssa::dump::dump_func(f, &tt)
    );
}

// ── Task 32b：包级初始化（`G-M6-07`）─────────────────────────────────

/// 合成 init 函数（没有就 panic，消息里带上模块里有什么）。
fn init_func(out: &rgoc_ssa::build::BuildOutcome) -> &SsaFunc {
    let i = out
        .module
        .init
        .unwrap_or_else(|| panic!("没有合成 init（funcs = {:?}）", names(&out.module)));
    &out.module.funcs[i]
}

fn names(m: &rgoc_ssa::ir::Module) -> Vec<String> {
    m.funcs.iter().map(|f| f.name.clone()).collect()
}

#[test]
fn t_ssa_36_包级初始化式建出_init_函数() {
    // `var g = 7` 的右侧**不在任何函数体里**（`Program::init_order`）。
    // 不合成 init，`main` 读到的就是零值 —— 而且不会有任何一处报错。
    let (program, mut tt) = program_of(
        "init-global",
        "package main\n\nvar g = 7\n\nfunc main() {\n\tprintln(g)\n}\n",
    );
    let out = build(&program, &mut tt);
    assert!(out.rejected.is_empty(), "不该被拒：{:?}", out.rejected);
    let init = init_func(&out);
    assert_eq!(init.name, "init", "合成函数的名字");
    assert_eq!(init.src, rgoc_ssa::ir::SYNTHETIC_FUNC, "src 是哨兵");
    assert!(init.params.is_empty() && init.blocks.len() == 1, "无参单块");
    if let Err(e) = verify(init, &tt) {
        panic!(
            "init 必须过 verifier：{e}\n{}",
            rgoc_ssa::dump::dump_func(init, &tt)
        );
    }
    // 一条 Store 写到 `g` 的全局根，值是 7
    let ss = stores(init);
    assert_eq!(
        ss.len(),
        1,
        "一组初始化式一条 Store：{}",
        rgoc_ssa::dump::dump_func(init, &tt)
    );
    assert!(
        matches!(init.roots.get(ss[0].0.index()), Some(Root::Global(_))),
        "写的是全局根：{:?}",
        init.roots.get(ss[0].0.index())
    );
    let seven = instrs(init).into_iter().any(|i| {
        matches!(
            i,
            Instr::Const {
                c: rgoc_hir::types::ConstVal::Int(7),
                ..
            }
        )
    });
    assert!(
        seven,
        "值 7 要在 init 里：{}",
        rgoc_ssa::dump::dump_func(init, &tt)
    );
    // main 读 g 走 Load（同一个全局根）
    let m = func_named(&out, "main");
    if let Err(e) = verify(m, &tt) {
        panic!("main 必须过 verifier：{e}");
    }
    let reads_global = instrs(m).into_iter().any(|i| {
        matches!(i, Instr::Load { root, .. } if matches!(m.roots.get(root.index()), Some(Root::Global(_))))
    });
    assert!(reads_global, "`println(g)` 要 Load 全局根");
}

#[test]
fn t_ssa_36_初始化顺序按声明序且同根保序() {
    // `var b = a + 1` 依赖 `var a = 1`：两条 Store 必须同根、且第二条用第一条的 mem。
    let (program, mut tt) = program_of(
        "init-order",
        "package main\n\nvar a = 1\n\nvar b = a + 1\n\nfunc main() {\n\tprintln(b)\n}\n",
    );
    let out = build(&program, &mut tt);
    assert!(out.rejected.is_empty());
    let init = init_func(&out);
    if let Err(e) = verify(init, &tt) {
        panic!(
            "init 必须过 verifier：{e}\n{}",
            rgoc_ssa::dump::dump_func(init, &tt)
        );
    }
    let ss = stores(init);
    assert_eq!(
        ss.len(),
        2,
        "两组两条 Store：{}",
        rgoc_ssa::dump::dump_func(init, &tt)
    );
    assert!(
        ss[0].0.index() < ss[1].0.index(),
        "按**声明序**写：先 a 的根再 b 的根（根表顺序就是 GlobalId 顺序）：{}",
        rgoc_ssa::dump::dump_func(init, &tt)
    );
}

#[test]
fn t_ssa_36_初始化式读到的全局走_load() {
    // `var b = a + 1`：读 `a` 必须是一条 Load（而不是常量 1 直接折叠 —— 那是 M6d 的事）。
    let (program, mut tt) = program_of(
        "init-read",
        "package main\n\nvar a = 1\n\nvar b = a + 1\n\nfunc main() {\n\tprintln(b)\n}\n",
    );
    let out = build(&program, &mut tt);
    let init = init_func(&out);
    let loads = instrs(init)
        .into_iter()
        .filter(|i| matches!(i, Instr::Load { .. }))
        .count();
    assert_eq!(
        loads,
        1,
        "读一次全局：{}",
        rgoc_ssa::dump::dump_func(init, &tt)
    );
    // 第二条 Store 的 mem 必须是**第一条 Store 产出的**（同一个根）
    // 同根保序：读 `a` 的 Load 必须用写 `a` 的那条 Store 产出的 mem
    let (load_mem, load_root) = instrs(init)
        .into_iter()
        .find_map(|i| match i {
            Instr::Load { mem, root, .. } => Some((*mem, *root)),
            _ => None,
        })
        .expect("没有 Load");
    let write_a = stores(init)
        .into_iter()
        .find(|s| s.0 == load_root)
        .expect("`a` 必须有一条 Store");
    assert_eq!(
        load_mem,
        write_a.2,
        "读 `a` 要用写 `a` 产出的 mem（同根保序）：{}",
        rgoc_ssa::dump::dump_func(init, &tt)
    );
}

#[test]
fn t_ssa_36_多值初始化式展平成多个_store() {
    let (program, mut tt) = program_of(
        "init-multi",
        "package main\n\nfunc pair() (int, int) {\n\treturn 1, 2\n}\n\nvar a, b = pair()\n\nfunc main() {\n\tprintln(a, b)\n}\n",
    );
    let out = build(&program, &mut tt);
    assert!(out.rejected.is_empty(), "{:?}", out.rejected);
    let init = init_func(&out);
    if let Err(e) = verify(init, &tt) {
        panic!(
            "init 必须过 verifier：{e}\n{}",
            rgoc_ssa::dump::dump_func(init, &tt)
        );
    }
    let calls = instrs(init)
        .into_iter()
        .filter(|i| matches!(i, Instr::Call { .. }))
        .count();
    assert_eq!(calls, 1, "一次调用");
    assert_eq!(stores(init).len(), 2, "两个全局两条 Store");
}

#[test]
fn t_ssa_36_文件的_init_被合成_init_调用() {
    let (program, mut tt) = program_of(
        "init-func",
        "package main\n\nvar g int\n\nfunc init() {\n\tg = 3\n}\n\nfunc main() {\n\tprintln(g)\n}\n",
    );
    let out = build(&program, &mut tt);
    let init = init_func(&out);
    let fid = func_id(&program, "init");
    let called = instrs(init).into_iter().any(|i| {
        matches!(
            i,
            Instr::Call {
                callee: rgoc_ssa::ir::CalleeRef::Func(f),
                ..
            } if *f == fid
        )
    });
    assert!(
        called,
        "合成 init 要调用文件的 init()：{}",
        rgoc_ssa::dump::dump_func(init, &tt)
    );
}

#[test]
fn t_ssa_36_没有初始化式时不合成_init() {
    let (program, mut tt) = program_of("init-none", "package main\n\nfunc main() {}\n");
    let out = build(&program, &mut tt);
    assert!(out.module.init.is_none(), "没有初始化式就没有 init");
    assert_eq!(names(&out.module), vec!["main".to_string()]);
}

#[test]
fn t_ssa_36_初始化式里的不支持面要带码拒绝() {
    // `var c = make(chan int)`：init_order 的表达式**不在任何函数体里**，
    // 函数体的预扫描扫不到它 ⇒ 必须在 build() 里单独扫。
    //
    // M9/Phase 2 把这条判据的 map 版下线了（map 已实现，`D-M9-5`），换成
    // 仍然被拒的 chan —— 判据的形状没变，仍然是「初始化式里的拒绝要带码」。
    let (program, mut tt) = program_of(
        "init-chan",
        // `main` **不碰** chan：这样唯一该被拒的就是初始化式。
        //
        // ⚠️ M11b/Task 16 起 `make(chan T)` **放行**了（它只分配 hchan，没有
        // 等待）⇒ 原来那个 fixture（`var c = make(chan int)`）不再被拒。
        // 换成「chan 类型的值赋给 chan 类型的全局」—— `d` 的初始化式是
        // `c`，而 `c` 的类型是 `chan int` ⇒ `face_kind` 仍拒。
        "package main\n\nvar c = make(chan int)\n\nvar d chan int = c\n\nfunc main() {\n\tprintln(1)\n}\n",
    );
    let out = build(&program, &mut tt);
    assert_eq!(out.rejected.len(), 1, "一条拒绝：{:?}", out.rejected);
    assert_eq!(out.rejected[0].code, rgoc_ssa::diag::SsaCode::Chan);
    assert!(
        out.rejected[0].detail.contains("初始化"),
        "说明要指出是包级初始化式：{}",
        out.rejected[0].detail
    );
    assert!(out.module.init.is_none(), "被拒就不该有 init");
}

// ── Task 33b：`&&` / `||` 短路（`G-M6-08`）──────────────────────────

/// 入口块的 `if` 两条出边（`(then, else)`）。
fn if_edges(f: &SsaFunc) -> (BlockId, BlockId) {
    match &f.blocks[0].term {
        Term::If { then_, else_, .. } => (then_.0, else_.0),
        t => panic!("入口块不是 `if`：{t:?}"),
    }
}

/// 某块 `goto` 的实参。
fn goto_args(f: &SsaFunc, b: BlockId) -> Vec<ValueId> {
    match &f.blocks[b.index()].term {
        Term::Goto { args, .. } => args.clone(),
        t => panic!("b{} 不是 `goto`：{t:?}", b.0),
    }
}

/// 某块的前驱个数。
fn pred_count(f: &SsaFunc, b: BlockId) -> usize {
    let c = cfg_of(f);
    c.preds(b).len()
}

#[test]
fn t_ssa_33b_and_建成控制流而不是一条_bin() {
    // `a && b` 必须是 `if` + 两个块 + 汇合：`Bin` 表达不出「b 不被求值」。
    let (program, mut tt) = program_of(
        "short-and",
        "package main\n\nfunc f(a, b bool) bool {\n\treturn a && b\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let f = func_named(&out, "f");
    if let Err(e) = verify(f, &tt) {
        panic!(
            "必须过 verifier：{e}\n{}",
            rgoc_ssa::dump::dump_func(f, &tt)
        );
    }
    assert_eq!(
        count_kind(f, "bin"),
        0,
        "短路不该产生 `bin`：{}",
        rgoc_ssa::dump::dump_func(f, &tt)
    );
    assert_eq!(
        f.blocks.len(),
        4,
        "entry + 右支 + 短路支 + 汇合：{}",
        rgoc_ssa::dump::dump_func(f, &tt)
    );
    let (rhs_b, short_b) = if_edges(f);
    assert_eq!(
        pred_count(f, rhs_b),
        1,
        "右支只能从 entry 进来（否则「不求值」不成立）：{}",
        rgoc_ssa::dump::dump_func(f, &tt)
    );
    assert_eq!(
        pred_count(f, short_b),
        1,
        "短路支只能从 entry 进来：{}",
        rgoc_ssa::dump::dump_func(f, &tt)
    );
    // 短路支送出的实参必须是常量 `false`
    let sa = goto_args(f, short_b);
    assert_eq!(sa.len(), 1, "一个结果实参");
    assert!(
        matches!(
            def_instr(f, sa[0]),
            Some(Instr::Const {
                c: rgoc_hir::types::ConstVal::Bool(false),
                ..
            })
        ),
        "`&&` 的短路出口是 `false`：{:?}",
        def_instr(f, sa[0])
    );
}

#[test]
fn t_ssa_33b_or_的短路出口是_true_且两支对调() {
    let (program, mut tt) = program_of(
        "short-or",
        "package main\n\nfunc f(a, b bool) bool {\n\treturn a || b\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let f = func_named(&out, "f");
    if let Err(e) = verify(f, &tt) {
        panic!(
            "必须过 verifier：{e}\n{}",
            rgoc_ssa::dump::dump_func(f, &tt)
        );
    }
    let (then_b, else_b) = if_edges(f);
    // `||`：lhs 为**真**走短路支 ⇒ 短路支是 then 边
    let sa = goto_args(f, then_b);
    assert!(
        matches!(
            def_instr(f, sa[0]),
            Some(Instr::Const {
                c: rgoc_hir::types::ConstVal::Bool(true),
                ..
            })
        ),
        "`||` 的短路出口是 `true`：{:?}",
        def_instr(f, sa[0])
    );
    // 右支在 else 边
    assert_eq!(
        pred_count(f, else_b),
        1,
        "右支只能从 entry 进来：{}",
        rgoc_ssa::dump::dump_func(f, &tt)
    );
}

#[test]
fn t_ssa_33b_右支里的访存要在汇合处合上() {
    // `a && (*p == 1)`：右支里有 `Load`（用 heap 根的 mem），汇合块必须带
    // mem 参数 —— 只合值会把右支的访存从链上摘掉（`V6` 的同类错）。
    let (program, mut tt) = program_of(
        "short-mem",
        "package main\n\nfunc f(a bool, p *int) bool {\n\treturn a && *p == 1\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let f = func_named(&out, "f");
    if let Err(e) = verify(f, &tt) {
        panic!(
            "必须过 verifier：{e}\n{}",
            rgoc_ssa::dump::dump_func(f, &tt)
        );
    }
    // 汇合块 = 参数最多的那个（结果 + mem）
    let join = f
        .blocks
        .iter()
        .enumerate()
        .max_by_key(|(_, b)| b.params.len())
        .map(|(i, _)| BlockId(i as u32))
        .expect("至少一个块");
    assert_eq!(
        f.blocks[join.index()].params.len(),
        2,
        "汇合块要有「结果 + heap 的 mem」两个参数：{}",
        rgoc_ssa::dump::dump_func(f, &tt)
    );
}

#[test]
fn t_ssa_33b_嵌套短路() {
    let (program, mut tt) = program_of(
        "short-nest",
        "package main\n\nfunc f(a, b, c bool) bool {\n\treturn a && b && c\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let f = func_named(&out, "f");
    if let Err(e) = verify(f, &tt) {
        panic!(
            "必须过 verifier：{e}\n{}",
            rgoc_ssa::dump::dump_func(f, &tt)
        );
    }
    assert!(
        f.blocks.len() >= 6,
        "两次短路至少 6 块：{}",
        rgoc_ssa::dump::dump_func(f, &tt)
    );
    assert_eq!(count_kind(f, "bin"), 0, "一条 `bin` 都不该有");
}

#[test]
fn t_ssa_33b_短路出口要跟结果类型而不是缓存_bool() {
    // `x < 3 && y`：左操作数是**比较表达式**，Go 规范说比较产生 `untyped boolean`，
    // 于是 `&&` 的结果类型也是 `untyped bool`。短路支的常量若用缓存的 `bool`，
    // 汇合块两条边就是两种类型（`V3` 拒 —— 实测 5 个语料文件）。
    let (program, mut tt) = program_of(
        "short-untyped",
        "package main\n\nfunc f(x int, y bool) bool {\n\treturn x < 3 && y\n}\n\nfunc main() {}\n",
    );
    let out = build(&program, &mut tt);
    let f = func_named(&out, "f");
    if let Err(e) = verify(f, &tt) {
        panic!(
            "必须过 verifier：{e}\n{}",
            rgoc_ssa::dump::dump_func(f, &tt)
        );
    }
    // 汇合块的两条边实参类型必须一致（`V3` 已经查了，这里再钉一次具体形态）
    let join = f
        .blocks
        .iter()
        .enumerate()
        .max_by_key(|(_, b)| b.params.len())
        .map(|(i, _)| BlockId(i as u32))
        .expect("至少一个块");
    assert!(
        !f.blocks[join.index()].params.is_empty(),
        "汇合块要有结果参数：{}",
        rgoc_ssa::dump::dump_func(f, &tt)
    );
}
