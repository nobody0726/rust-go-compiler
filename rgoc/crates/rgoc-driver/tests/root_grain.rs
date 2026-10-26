//! 根协议的「字粒度」牙（`T-ROOT-06`）与 `U-M9-10` 的守卫（`Task 61/62`）。
//!
//! # 为什么这一条必须单独存在
//!
//! `T-ROOT-05` 用的是**手搓 IR**：它证明「给定这三种类型，位图算得对」。
//! 但 `RM-2` 的错法是「按**值**是不是指针类型判」—— 那种实现在手搓 IR 上
//! 也可能碰巧对（只要 fixture 里没有切片）。所以这一条用**真实源码**：
//! `s := make([]int, 3)` 之后 `s` 是**唯一**持有者，它活在 safepoint 上。
//!
//! 判据写成两半，缺一不可：
//!
//! 1. **正向**：`ptr_words` 里 `s` 恰有 1 个字（字 0），栈图里恰 1 位；
//! 2. **反向**（这条才是「有牙」）：把 `RM-2` 的错实现**直接算一遍** ——
//!    「`live` 里类型是 `*T` 的值的个数」在这个 fixture 上是 **0**。
//!    也就是说：错实现会让位图**全空**，而那正是「活对象被回收」的入口。
//!    只写正向的话，一个「什么都不标」的实现也能过（只要断言写得松）。
//!
//! # `U-M9-10`：interior pointer 是我们的边界
//!
//! `uintptr` 在 Go 语义里**不是引用**（`u := uintptr(unsafe.Pointer(&x))`
//! 之后 `x` 可以被回收），所以位图里没有它是**对的**。但它同时是我们的
//! 能力边界：M10 拿到一个指向对象**中部**的指针时，没有对象头 / 区间表可以
//! 把它映射回对象起点 —— 那件事记为 `U-M9-10`（不支持面），**不是**保证。
//! 这条守卫把「没有它」钉住，免得日后有人把「标了 `unsafe.Pointer`」误读成
//! 「interior pointer 安全」。

use std::path::{Path, PathBuf};

use rgoc_check::CheckOptions;
use rgoc_load::load_with_imports;
use rgoc_ssa::build::build;
use rgoc_ssa::ir::{Module, SsaFunc, ValueId};
use rgoc_types::typ::Type;
use rgoc_types::{BasicId, TypeTable};

/// 仓库根：`rgoc/crates/rgoc-driver` 往上三层。
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("仓库根不可达")
}

fn module_of_text(name: &str, src: &str) -> (Module, TypeTable) {
    let dir = repo_root().join("rgoc/crates/rgoc-driver/target/root-grain");
    std::fs::create_dir_all(&dir).expect("建目录失败");
    let go = dir.join(format!("{name}.go"));
    std::fs::write(&go, src).expect("写 fixture 失败");
    let mut tt = TypeTable::new();
    let (pkg, bag) = load_with_imports(&mut tt, &[go], &[], None);
    let (checked, lowered) = rgoc_check::check::check_package_with(
        &pkg,
        &mut tt,
        bag,
        &CheckOptions { test_hooks: false },
        |s| rgoc_hir::lower(&pkg, s),
    );
    assert_eq!(checked.rendered_count(), 0, "{name} 不该有静态诊断");
    let program = lowered.unwrap_or_else(|_| panic!("{name} 被 lowering 拒绝"));
    let out = build(&program, &mut tt);
    assert!(
        out.rejected.is_empty(),
        "{name} 被构建期拒绝：{:?}",
        out.rejected
    );
    (out.module, tt)
}

/// `main` 函数在 `Module::funcs` 里的下标。
fn main_func(m: &Module) -> usize {
    m.main
        .and_then(|fid| m.funcs.iter().position(|f| f.src == fid))
        .expect("有 main")
}

/// 第一个类型满足 `pred` 的值。
fn value_with(f: &SsaFunc, tt: &TypeTable, pred: impl Fn(&Type) -> bool) -> Option<ValueId> {
    (0..f.value_types.len())
        .map(|i| ValueId(i as u32))
        .find(|v| {
            let t = f.value_types[v.index()];
            pred(tt.type_of(rgoc_types::underlying(tt, t)))
        })
}

/// 该值所在槽的位图基位（`(frame_size + off) / 8`）。
fn base_bit(frame_size: u32, off: i32) -> usize {
    ((i64::from(frame_size) + i64::from(off)) / 8) as usize
}

// ── T-ROOT-06：slice 的 ptr 字是唯一根 ───────────────────────────────

#[test]
fn t_root_06_slice_的_ptr_字是唯一根() {
    // `s` 是 `[]int` 的**唯一**持有者：`s[0] = 7` 之后只用 `len/cap/s[0]`。
    let src = r#"package main

func main() {
	s := make([]int, 3)
	s[0] = 7
	println(len(s), cap(s), s[0])
}
"#;
    let (module, tt) = module_of_text("slice_root", src);
    let mi = main_func(&module);
    let f = &module.funcs[mi];
    let cfg = rgoc_ssa::cfg::Cfg::build(f);
    let lv = rgoc_ssa::live::analyze(f, &cfg, &tt);

    let sv = value_with(f, &tt, |t| matches!(t, Type::Slice(_)))
        .unwrap_or_else(|| panic!("fixture 里应有 `[]int` 值：{:?}", f.value_types));
    let sv_ty = f.value_types[sv.index()];
    assert!(
        matches!(
            tt.type_of(rgoc_types::underlying(&tt, sv_ty)),
            Type::Slice(_)
        ),
        "v{} 应是切片",
        sv.0
    );

    // 找一个「`s` 活着」的 safepoint（`println` 那一条）
    let sp = lv
        .safepoints
        .iter()
        .find(|s| s.live.contains(&sv))
        .unwrap_or_else(|| panic!("`s` 应跨过至少一个 safepoint：{:?}", lv.safepoints));

    // ① 正向：恰一个字，且是字 0
    let words: Vec<u8> = sp
        .ptr_words
        .iter()
        .filter(|(v, _)| *v == sv)
        .map(|(_, w)| *w)
        .collect();
    assert_eq!(
        words,
        vec![0],
        "`[]int` 只有字 0 是根（len/cap 是整数）：{:?}",
        sp.ptr_words
    );

    // ② 反向（这条是牙）：`RM-2` 的错实现 = 「`live` 里类型是 `*T` 的值的个数」。
    //    在**这个** fixture 上它给出 0 —— 也就是说按值类型判会把 `s` 的根
    //    整个漏掉，位图全空。
    let old_rule: Vec<ValueId> = sp
        .live
        .iter()
        .copied()
        .filter(|v| {
            matches!(
                tt.type_of(rgoc_types::underlying(&tt, f.value_types[v.index()])),
                Type::Pointer(_)
            )
        })
        .collect();
    assert!(
        old_rule.is_empty(),
        "这个 fixture 上「值是指针类型」的判据必须给出 0 个根（否则它没有牙）：{old_rule:?}"
    );

    // ③ 落到**位图**上：那个值的槽只有 1 位被标，且后面两个字（len/cap）是 0。
    let siblings: std::collections::BTreeMap<rgoc_hir::FuncId, &SsaFunc> =
        module.funcs.iter().map(|x| (x.src, x)).collect();
    let fr = rgoc_arm64::frame::layout_with(f, &tt, &siblings).expect("帧布局");
    let fsm = rgoc_arm64::module::stack_map_of(mi, f, &fr, &lv, &tt, &siblings);
    let rec = fsm
        .sps
        .iter()
        .find(|r| r.block == sp.block.0 && r.idx == sp.idx)
        .expect("栈图里应有同一个位点");
    let off = fr.off_of(sv).expect("切片值有槽");
    let bit = base_bit(fsm.frame_size, off);
    assert!(
        (rec.words[bit / 64] >> (bit % 64)) & 1 == 1,
        "v{} 的 ptr 字（位 {bit}）必须被标：{:?}",
        sv.0,
        rec.words
    );
    for k in 1..3 {
        let b = bit + k;
        assert!(
            (rec.words[b / 64] >> (b % 64)) & 1 == 0,
            "v{} 的字 {k}（len/cap，位 {b}）不该被标：{:?}",
            sv.0,
            rec.words
        );
    }
}

// ── `U-M9-10` 的守卫：`uintptr` 不是根（已知边界）────────────────────

#[test]
fn u_m9_10_uintptr_不是根_守卫() {
    // `p` 是 `unsafe.Pointer`、`u` 是从它算出来的 `uintptr`。
    // 两者都跨过 `println`（`u` 被读、`p` 被 `!= nil` 读）。
    // ⚠️ `u` 必须**作为实参跨过调用**才活得下来：`println(u != 0)` 会在调用前
    // 就把 `u` 用完（活集里只剩那个 `bool`），于是这条守卫会退化成
    // 「断言一个不在 live 里的值没有位」—— 恒真。
    let src = r#"package main

import "unsafe"

func id(u uintptr) uintptr { return u }

func main() {
	x := 7
	p := unsafe.Pointer(&x)
	u := uintptr(p)
	println(id(u))
}
"#;
    let (module, tt) = module_of_text("unsafe_uintptr", src);
    let mi = main_func(&module);
    let f = &module.funcs[mi];
    let cfg = rgoc_ssa::cfg::Cfg::build(f);
    let lv = rgoc_ssa::live::analyze(f, &cfg, &tt);

    let uv = value_with(f, &tt, |t| matches!(t, Type::Basic(BasicId::Uintptr)))
        .unwrap_or_else(|| panic!("fixture 里应有 `uintptr` 值：{:?}", f.value_types));

    // ① `uintptr` 的活值在位图里**没有任何位**（Go 语义：它不是引用）。
    let sp = lv
        .safepoints
        .iter()
        .find(|s| s.live.contains(&uv))
        .unwrap_or_else(|| panic!("`u` 应跨过至少一个 safepoint：{:?}", lv.safepoints));
    assert!(
        !sp.ptr_words.iter().any(|(v, _)| *v == uv),
        "`uintptr` 不该是根（`U-M9-10` 的边界就在这里）：{:?}",
        sp.ptr_words
    );

    // ③ 落到栈图上：`u` 的槽一个位都没有。
    let siblings: std::collections::BTreeMap<rgoc_hir::FuncId, &SsaFunc> =
        module.funcs.iter().map(|x| (x.src, x)).collect();
    let fr = rgoc_arm64::frame::layout_with(f, &tt, &siblings).expect("帧布局");
    let fsm = rgoc_arm64::module::stack_map_of(mi, f, &fr, &lv, &tt, &siblings);
    let rec = fsm
        .sps
        .iter()
        .find(|r| r.block == sp.block.0 && r.idx == sp.idx)
        .expect("栈图里应有同一个位点");
    let off = fr.off_of(uv).expect("uintptr 值有槽");
    let bit = base_bit(fsm.frame_size, off);
    assert!(
        (rec.words[bit / 64] >> (bit % 64)) & 1 == 0,
        "`uintptr` 的槽（位 {bit}）不该被标：{:?}",
        rec.words
    );
}
