//! pass 框架与开关（`T-SSA-64`；`M6-plan.md` Task 40）。
//!
//! # 判据
//!
//! 1. `Config::parse` 只认五个名字与 `all`/`none`；空串、未知名、尾随逗号一律 `None`
//!    （静默当 `all` 是**猜**，用户可能想写 `none` 却漏了字）。
//! 2. 每个 pass 之后 `verify` 都过（`SSA-5`）—— 用一个「fold 折出常量、dce 再收掉」
//!    的小函数走全流程，跑完自己再 verify 一遍。
//! 3. `none` 是**空操作**：IR 逐字节不变（它是 A/B 对照的另一半）。

use rgoc_ssa::ir::{Instr, Module, SsaFunc, Term};
use rgoc_ssa::opt::{self, Config};
use rgoc_ssa::testsupport::{FnBuilder, module_of};
use rgoc_ssa::verify::verify;
use rgoc_types::{TypeId, TypeTable};

#[test]
fn t_ssa_64_开关解析() {
    assert_eq!(Config::parse("all"), Some(Config::all()), "all");
    assert_eq!(Config::parse("none"), Some(Config::none()), "none");
    assert_eq!(
        Config::parse("fold"),
        Some(Config {
            fold: true,
            ..Config::none()
        }),
        "单个 pass"
    );
    assert_eq!(
        Config::parse("fold,dce"),
        Some(Config {
            fold: true,
            dce: true,
            ..Config::none()
        }),
        "逗号分隔"
    );
    // 三种「不该猜」的输入
    assert_eq!(Config::parse(""), None, "空串");
    assert_eq!(Config::parse("dcel"), None, "拼错的 pass 名");
    assert_eq!(Config::parse("fold,"), None, "尾随逗号");
    // 文本形态（诊断用）
    assert_eq!(Config::all().as_str(), "all");
    assert_eq!(Config::none().as_str(), "none");
    assert_eq!(
        Config {
            fold: true,
            dce: true,
            copy: false,
            cse: false,
            inline: false
        }
        .as_str(),
        "fold,dce"
    );
    // 内联是第五个开关（`M12` c 线）；只开它时文本里只有它
    assert_eq!(
        Config::parse("inline"),
        Some(Config {
            inline: true,
            ..Config::none()
        }),
        "内联单独可开"
    );
    assert_eq!(
        Config {
            inline: true,
            ..Config::none()
        }
        .as_str(),
        "inline"
    );
    assert_eq!(Config::default(), Config::all(), "默认全开");
}

/// `func f() int { return 1 + 2 }`（`fold` 折成 3，`dce` 收掉两个常量）。
fn add_consts() -> (SsaFunc, TypeTable) {
    let mut b = FnBuilder::new("f");
    let int = b.basic("int");
    let e = b.entry();
    let c1 = b.value(int);
    let c2 = b.value(int);
    let r = b.value(int);
    b.instr(
        e,
        Instr::Const {
            dst: c1,
            c: rgoc_hir::types::ConstVal::Int(1),
            typ: int,
        },
    );
    b.instr(
        e,
        Instr::Const {
            dst: c2,
            c: rgoc_hir::types::ConstVal::Int(2),
            typ: int,
        },
    );
    b.instr(
        e,
        Instr::Bin {
            dst: r,
            op: rgoc_hir::BinOp::Add,
            lhs: c1,
            rhs: c2,
            typ: int,
        },
    );
    b.term(e, Term::Return(vec![r]));
    b.into_parts()
}

fn dump(f: &SsaFunc, tt: &TypeTable) -> String {
    rgoc_ssa::dump::dump_func(f, tt)
}

#[test]
fn t_ssa_64_全开跑完之后仍然合法且更短() {
    let (f, tt) = add_consts();
    let mut m = module_of(vec![f]);
    opt::run(&mut m, &tt, Config::all()).expect("全开之后必须仍然合法");
    let f = &m.funcs[0];
    verify(f, &tt).expect("收口再验一次");
    assert_eq!(
        f.blocks[0].instrs.len(),
        1,
        "`1+2` 折成一条常量，两个操作数被 dce 收掉：{}",
        dump(f, &tt)
    );
}

#[test]
fn t_ssa_64_none_是空操作() {
    let (f, tt) = add_consts();
    let before = dump(&f, &tt);
    let mut m = module_of(vec![f]);
    opt::run(&mut m, &tt, Config::none()).expect("none 不该破坏任何东西");
    assert_eq!(
        dump(&m.funcs[0], &tt),
        before,
        "`--opt=none` 必须逐字节不变"
    );
}

#[test]
fn t_ssa_64_单个开关只做那一件事() {
    // 只开 fold：常量折了，但两个操作数常量还在（dce 没开）
    let (f, tt) = add_consts();
    let mut m = module_of(vec![f]);
    opt::run(
        &mut m,
        &tt,
        Config {
            fold: true,
            ..Config::none()
        },
    )
    .expect("只开 fold 也要合法");
    assert_eq!(
        m.funcs[0].blocks[0].instrs.len(),
        3,
        "fold 只换指令、不删指令：{}",
        dump(&m.funcs[0], &tt)
    );
    let _ = TypeId::from_index(0);
}

#[test]
fn t_ssa_64_没有_main_的模块也能跑_pass() {
    // `run` 只遍历 `Module::funcs`，不看 `main`
    let (f, tt) = add_consts();
    let mut m = Module {
        funcs: vec![f],
        ..module_of(Vec::new())
    };
    assert!(m.main.is_none());
    opt::run(&mut m, &tt, Config::all()).expect("无 main 也要能跑 pass");
}
