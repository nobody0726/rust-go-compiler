//! c 线优化的**司机面**判据（`T-OPT-01` / `G12-10`；`M12-plan.md` Task 37）。
//!
//! # 三半
//!
//! 1. **语义**：同一段源码 `--opt=none` 与 `--opt=all` 输出逐字节相同、退出码相同
//!    （语料级的全量对撞在 `opt_equiv.rs`，这里有 fixture 级的 + **三态**）；
//! 2. **真的发生了**：内联之后被调方的符号从 `.s` 里消失、调用点没了 ——
//!    否则「语义没变」这条会被「什么都别做」的实现白拿；
//! 3. **体积**：`.s` 行数与函数个数都下降（内联 + 收尸的净效果）。
//!
//! 第 2、3 半是**判据的判据**：只测第 1 半的优化可以完全不做任何事。

mod support;

use rgoc_ssa::opt::Config;
use support::native_harness::{Outcome, build_and_run_opt};

/// 两处调用同一个三行函数。
///
/// # 为什么是「两处」而不是「八处」
///
/// 实测读数（`M12-report.md` §11.1）：两处时 `191 → 164` 行（省 27）；
/// 八处时 `317 → 338` 行（**涨** 21）—— 一处内联的体比一条 `bl` 贵，
/// 只有「被调方被收尸」那一份收益能盖过去，而那份收益**只算一次**。
/// 所以 fixture 定在两处：它要证明的是「会省」，不是「永远省」。
const SRC: &str = r#"package main

func sq(x int) int {
	return x * x
}

func main() {
	println(sq(3) + sq(4))
}
"#;

/// `.s` 里定义了几个函数（`fn<FuncId>:` 形式的标签）。
fn fn_labels(asm: &str) -> usize {
    asm.lines()
        .filter(|l| {
            let t = l.trim_end();
            t.ends_with(':')
                && t.strip_prefix("fn")
                    .and_then(|r| r.strip_suffix(':'))
                    .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        })
        .count()
}

fn lines(o: &Outcome) -> usize {
    o.asm.lines().count()
}

#[test]
fn t_opt_01_内联之后调用点消失且体积下降() {
    let off =
        build_and_run_opt("m12opt-off", SRC, Config::none(), &[]).expect("`--opt=none` 应能跑");
    let on = build_and_run_opt("m12opt-on", SRC, Config::all(), &[]).expect("`--opt=all` 应能跑");

    assert_eq!(
        on.stdout, off.stdout,
        "内联改了输出（`--opt=all` 与 `none` 必须逐字节相同）"
    );
    assert_eq!(on.exit, off.exit, "退出码不同");

    let (n_off, n_on) = (fn_labels(&off.asm), fn_labels(&on.asm));
    assert!(
        n_off >= 2,
        "`--opt=none` 的 `.s` 里至少要有 `sq` 与 `main`：{n_off}"
    );
    assert_eq!(
        n_on + 1,
        n_off,
        "内联之后被调方应被收尸（函数个数少 1）：none={n_off} all={n_on}"
    );
    // 调用点：`none` 有两条 `bl fn<sq>`，`all` 一条不剩（只剩 `println` 那条）
    let bl = |s: &str| s.lines().filter(|l| l.contains("bl fn")).count();
    assert!(
        bl(&on.asm) < bl(&off.asm),
        "内联之后 `bl fn…` 应减少：none={} all={}",
        bl(&off.asm),
        bl(&on.asm)
    );
    assert!(
        lines(&on) < lines(&off),
        "`.s` 行数应下降：none={} all={}",
        lines(&off),
        lines(&on)
    );
}

#[test]
fn t_opt_01_内联不影响三态输出() {
    // 同一个产物要在 off / on / stress 三态下跑出同样的字节（`C12` §5.1）。
    // 内联是**编译期**的事，三态的差别在运行期 —— 所以「三态一致」必须与
    // 「两档 opt 一致」同时成立，且两档之间也要一致。
    let mut seen: Option<(String, i32)> = None;
    for (tag, opt) in [("none", Config::none()), ("all", Config::all())] {
        for (state, env) in [
            ("off", vec![("RGOC_GC", "0")]),
            ("on", vec![]),
            ("stress", vec![("RGOC_GC_STRESS", "1")]),
        ] {
            let o = build_and_run_opt(&format!("m12opt3-{tag}-{state}"), SRC, opt, &env)
                .unwrap_or_else(|e| panic!("{tag}/{state} 跑不起来：\n{e}"));
            match &seen {
                None => seen = Some((o.stdout.clone(), o.exit)),
                Some((s, x)) => {
                    assert_eq!(&o.stdout, s, "{tag}/{state} 的 stdout 与第一态不同");
                    assert_eq!(o.exit, *x, "{tag}/{state} 的退出码与第一态不同");
                }
            }
        }
    }
}

#[test]
fn t_opt_01_fn_labels_数得准() {
    // 判据自检：标签计数不能是「恰好数到 0 也算通过」。
    let asm = "\t.text\nfn3:\n\tret\nfn12:\n\tret\nmain:\n\tret\nfn:\n\tret\nfnx9:\n";
    assert_eq!(fn_labels(asm), 2, "只数 `fn<纯数字>:` 形式的标签");
}
