//! S2 的验收测试（`T-S2-01` / `02` / `03`）。
//!
//! 重点是**交叉验证真的在验证**：既要比结果一致，也要能证明「如果 S2 走的是
//! 搬运 HIR 求值结果那条路，测试会红」。

use rgoc_hir::Stream;
use rgoc_hir::value::BigInt;
use rgoc_hir::{BinOp, Expr, Stmt, Val};
use rgoc_spikes::fixtures::{big_shift_expr, s1_program, sum_expr};
use rgoc_spikes::interp::check_t_s1_01;
use rgoc_spikes::ssa::{Instr, LowerError, SsaFunc, eval, lower, lower_expr_program};
use rgoc_spikes::ssa_needs::{needs, render_gap, render_needs, verifier_gap};

fn int(v: i64) -> Expr {
    Expr::Const(rgoc_hir::Const::Big(BigInt::from_i64(v)))
}

#[test]
fn t_s2_01_图求值结果与_s1_逐字节相同() {
    let s1 = check_t_s1_01().expect("S1 应通过");
    let f =
        lower_expr_program("main", &[sum_expr()], true, Stream::Stderr).expect("lowering 应成功");
    let s2 = eval(&f).expect("求值应成功");
    assert_eq!(
        s2, s1,
        "S2 与 S1 的 RunOutput 必须完全相等（含 stdout 字段）"
    );
}

#[test]
fn t_s2_01_图里真的有_bin_指令_而不是纯_const() {
    // 反向验证：若 S2 退化成「把 HIR 里已求值的 Val 搬进图」，
    // 本测试会红 —— 那样的 S2 与 S1 等价，交叉验证毫无意义。
    let f =
        lower_expr_program("main", &[sum_expr()], true, Stream::Stderr).expect("lowering 应成功");
    let b = f.entry_block().expect("入口块应存在");
    assert_eq!(
        b.instrs.len(),
        3,
        "1+2 应产生 3 条指令：两个 const + 一个 bin"
    );
    let bins: Vec<&Instr> = b
        .instrs
        .iter()
        .map(|(_, i)| i)
        .filter(|i| matches!(i, Instr::Bin { .. }))
        .collect();
    assert_eq!(bins.len(), 1, "必须有且只有一条 bin 指令");
    assert!(
        matches!(bins[0], Instr::Bin { op: BinOp::Add, .. }),
        "bin 指令应是 Add"
    );
}

#[test]
fn t_s2_01_定义顺序保证_使用先于定义() {
    // 后序遍历的结果：const(1) → const(2) → bin
    let f =
        lower_expr_program("main", &[sum_expr()], true, Stream::Stderr).expect("lowering 应成功");
    let b = f.entry_block().expect("入口块应存在");
    let ids: Vec<_> = b.instrs.iter().map(|(id, _)| *id).collect();
    // ID 必须严格递增 —— 这是「唯一定义点 + 使用先于定义」的结构性保证
    for w in ids.windows(2) {
        assert!(w[0] < w[1], "ID 应严格递增：{ids:?}");
    }
    // bin 指令引用的两个 ID 都必须在它之前
    if let Instr::Bin { lhs, rhs, .. } = &b.instrs[2].1 {
        let me = ids[2];
        assert!(
            *lhs < me && *rhs < me,
            "操作数必须先于定义：lhs={lhs} rhs={rhs} me={me}"
        );
    } else {
        panic!("第三条应是 bin");
    }
}

#[test]
fn t_s2_01_图求值与_hir_求值对_超宽常量也一致() {
    // 把交叉验证扩展到 1<<100：S1 的 eval_const 与 S2 的图求值必须给出同一个大整数。
    // 只测 1+2 的话，「大数在两条路上都错」是看不出来的。
    let s1v = big_shift_expr().eval_const().expect("S1 应能折叠");
    let f = lower_expr_program("main", &[big_shift_expr()], true, Stream::Stderr)
        .expect("lowering 应成功");
    let s2 = eval(&f).expect("S2 应能求值");
    let Val::Int(s1i) = s1v else {
        panic!("应是整数");
    };
    // s2.stderr 是已渲染的字节串（println 带末尾换行），比较时要剥掉换行
    assert_eq!(s2.stdout, "", "内建 print 家族不走 stdout");
    assert_eq!(s2.stderr.trim_end_matches('\n'), s1i.to_string());
    assert_eq!(s1i.to_string(), "1267650600228229401496703205376");
}

#[test]
fn 图规模摘要反映实际内容() {
    let f =
        lower_expr_program("main", &[sum_expr()], true, Stream::Stderr).expect("lowering 应成功");
    let s = f.summary();
    assert_eq!(s.blocks, 1, "直线程序只有一个块");
    assert_eq!(s.instrs, 3);
    assert_eq!(s.succs, 0, "直线程序没有后继边");
    assert_eq!(s.params, 0, "直线程序没有块参数（phi 的容身处恒空）");
}

#[test]
fn 图可渲染成可读文本() {
    let f =
        lower_expr_program("main", &[sum_expr()], true, Stream::Stderr).expect("lowering 应成功");
    let text = f.render();
    assert!(text.contains("func main"), "{text}");
    assert!(text.contains("entry:"), "{text}");
    assert!(text.contains("const 1"), "{text}");
    assert!(text.contains("const 2"), "{text}");
    // bin 指令应显示成 v2 = v0 + v1 的形状
    assert!(text.contains("v2 = v0 + v1"), "{text}");
    assert!(text.contains("print args"), "{text}");
}

#[test]
fn 指令的_operands_对_const_为空_对_bin_为两个() {
    let c = Instr::Const(Val::Int(BigInt::from_i64(1)));
    assert!(c.operands().is_empty());
    let f =
        lower_expr_program("main", &[sum_expr()], true, Stream::Stderr).expect("lowering 应成功");
    let b = f.entry_block().expect("入口块应存在");
    match &b.instrs[2].1 {
        Instr::Bin { lhs, rhs, .. } => {
            assert_eq!(b.instrs[2].1.operands(), vec![*lhs, *rhs]);
        }
        other => panic!("应是 bin，实际 {other:?}"),
    }
}

#[test]
fn 从_hir_program_lowering_也能得到可求值的图() {
    // lower() 走 HIR 的已求值 Val（保底路径），lower_expr_program 走表达式（主路径）。
    // 两条都要能求值，且结果一致 —— 前者是 S1/S2 共用 fixture 时的实际入口。
    let f = lower(&s1_program()).expect("lowering 应成功");
    let s2 = eval(&f).expect("求值应成功");
    let s1 = check_t_s1_01().expect("S1 应通过");
    assert_eq!(s2, s1);
}

#[test]
fn lowering_失败分类明确_不是_panic() {
    // 无 main
    let p = rgoc_hir::Program {
        funcs: vec![rgoc_hir::FuncDecl {
            name: "helper".to_string(),
            body: vec![],
        }],
    };
    assert_eq!(lower(&p), Err(LowerError::NoMain));

    // 子集外语句
    let p = rgoc_hir::Program {
        funcs: vec![rgoc_hir::FuncDecl {
            name: "main".to_string(),
            body: vec![Stmt::Return(Val::Void)],
        }],
    };
    match lower(&p) {
        Err(LowerError::UnsupportedExpr(s)) => assert!(s.contains("return"), "{s}"),
        other => panic!("应报 UnsupportedExpr，实际 {other:?}"),
    }
}

#[test]
fn 入口块缺失时报错而不是_panic() {
    let f = SsaFunc {
        name: "main".to_string(),
        blocks: Default::default(),
        entry: "entry".to_string(),
        print_args: vec![],
        print_newline: true,
        print_stream: Stream::Stderr,
    };
    let err = f.entry_block().expect_err("应报错");
    assert!(err.contains("entry"), "{err}");
    assert!(eval(&f).is_err());
}

#[test]
fn 求值遇到子集外运算显式报错() {
    // 乘法在子集外 —— 必须报错，不能给个猜出来的值
    let e = Expr::Bin {
        op: BinOp::Mul,
        lhs: Box::new(int(2)),
        rhs: Box::new(int(3)),
    };
    let f =
        lower_expr_program("main", &[e], true, Stream::Stderr).expect("lowering 成功（形状合法）");
    let err = eval(&f).expect_err("求值应失败");
    assert!(err.contains("乘法"), "{err}");
}

#[test]
fn 求值遇到负移位量显式报错() {
    let e = Expr::Bin {
        op: BinOp::Shl,
        lhs: Box::new(int(1)),
        rhs: Box::new(int(-1)),
    };
    let f = lower_expr_program("main", &[e], true, Stream::Stderr).expect("lowering 应成功");
    let err = eval(&f).expect_err("求值应失败");
    assert!(err.contains("负移位"), "{err}");
}

#[test]
fn 求值遇到非整数操作数显式报错() {
    let e = Expr::Bin {
        op: BinOp::Add,
        lhs: Box::new(Expr::Const(rgoc_hir::Const::Str("x".to_string()))),
        rhs: Box::new(int(1)),
    };
    let f = lower_expr_program("main", &[e], true, Stream::Stderr).expect("lowering 应成功");
    let err = eval(&f).expect_err("求值应失败");
    assert!(err.contains("整数"), "{err}");
}

#[test]
fn 多参数与_print_的形态与_s1_一致() {
    // S2 必须独立得出与 S1 相同的分隔规则（oracle 实测：print 无分隔、println 空格+换行）
    let two = vec![int(-9223372036854775808), int(9223372036854775807)];
    for newline in [true, false] {
        let f = lower_expr_program("main", &two, newline, Stream::Stderr).expect("lowering 应成功");
        let s2 = eval(&f).expect("求值应成功");
        // 与 S1 对同样输入的解释结果比
        let prog = rgoc_hir::Program {
            funcs: vec![rgoc_hir::FuncDecl {
                name: "main".to_string(),
                body: vec![Stmt::Print {
                    newline,
                    stream: Stream::Stderr,
                    args: vec![
                        Val::Int(BigInt::from_i64(-9223372036854775808)),
                        Val::Int(BigInt::from_i64(9223372036854775807)),
                    ],
                }],
            }],
        };
        let s1 = rgoc_spikes::interp::interpret(&prog).expect("S1 应能求值");
        assert_eq!(s2, s1, "newline={newline} 时 S1/S2 应一致");
    }
}

#[test]
fn t_s2_02_需求清单含_memory_tuple_与调用边界() {
    let list = needs();
    let ids: Vec<&str> = list.iter().map(|n| n.id).collect();
    for want in ["S2-NEED-MEMORY", "S2-NEED-TUPLE", "S2-NEED-CALL"] {
        assert!(
            ids.contains(&want),
            "清单必须含 {want}（`03` §4 第 5 条要求识别这三项），实际 {ids:?}"
        );
    }
    // 每一条都必须有「触发形状」「为什么」「M0 处置」「归属」——
    // 缺任何一栏，这条需求就无法被 M6 接手
    for n in &list {
        assert!(!n.trigger.is_empty(), "{} 缺触发形状", n.id);
        assert!(n.trigger.len() > 10, "{} 的触发形状要具体到能写测试", n.id);
        assert!(n.why.len() > 30, "{} 的理由太短，说不清为什么图不够", n.id);
        assert!(
            n.m0_disposition.contains("不实现") || n.m0_disposition.contains("只记录"),
            "{} 必须写明 M0 为什么不实现",
            n.id
        );
        assert!(n.owner.starts_with('M'), "{} 必须写明归属里程碑", n.id);
    }
}

#[test]
fn t_s2_02_清单渲染含每条需求() {
    let text = render_needs();
    for id in ["S2-NEED-MEMORY", "S2-NEED-TUPLE", "S2-NEED-CALL"] {
        assert!(text.contains(id), "渲染文本须含 {id}");
    }
    assert!(text.contains("触发形状"));
    assert!(text.contains("M0 处置"));
    assert!(text.contains("归属"));
}

#[test]
fn t_s2_03_差距记录区分_不具备_与_从未验证过() {
    let gap = verifier_gap();
    assert!(!gap.is_empty());
    let text = render_gap();
    // 必须显式写出「从未被验证过」—— 单块图里支配关系是平凡的，
    // 不点明这一点，读者会以为「已经会了」
    assert!(
        text.contains("从未被验证过"),
        "差距记录必须点明「不具备」与「从未验证过」的区别"
    );
    for item in [
        "唯一定义点",
        "控制流图",
        "phi 正确性",
        "支配关系（dominator）",
        "类型一致性",
    ] {
        assert!(text.contains(item), "差距记录须覆盖 {item}");
    }
    // 每个条目都要有状态标记
    for (item, status, note) in &gap {
        assert!(
            status.contains('✅') || status.contains('❌'),
            "{item} 的状态必须显式标记：{status}"
        );
        assert!(!note.is_empty(), "{item} 必须有说明");
    }
}

#[test]
fn t_s2_03_差距记录承认调用边界尚未解决() {
    // 这是最容易被「忘记」的一条：M0 的形态让调用边界看不见，
    // 若不点明，M6 会误以为已经解决。
    let text = render_gap();
    assert!(text.contains("S2-NEED-CALL"), "须指向调用边界需求条目");
    assert!(
        text.contains("图里没有 call 指令") || text.contains("没有 call 指令"),
        "须明确说图里没有 call 指令"
    );
}
