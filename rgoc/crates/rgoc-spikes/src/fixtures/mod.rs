//! 三个 spike 共享的**固定** HIR fixture。
//!
//! # 为什么 fixture 放在 crate 里而不是各 bin 各自构造
//!
//! `T-S2-01` 要求「求值结果必须与 `T-S1-01` 一致」。如果两个 bin 各自构造 HIR，
//! 它们很快就会漂移（S1 改了节点形状、S2 没跟），于是**交叉验证变成两个不同程序的比较**，
//! 失去意义。共享 fixture 是这个交叉验证能成立的**前提**，不是省事。
//!
//! # 三个 fixture
//!
//! 1. [`s1_program`] —— `println(1 + 2)`。S1 求值它，S2 构造 SSA 后求值**同一份**。
//! 2. [`s1_const_edges`] —— 常量折叠边界：`1<<100`（超 i64）、`int64` 两个极值。
//!    对照 **T-C-04**（`printbig.go`）的冻结期望 `-9223372036854775808\n9223372036854775807\n`。
//! 3. [`s3_hello`] —— 输出 `hello` 的 native 函数体。S3 据此生成 arm64 汇编。

use rgoc_hir::value::BigInt;
use rgoc_hir::{BinOp, Const, Expr, FuncDecl, Program, Stmt, Stream, Val};

/// fixture 的位置标签文件名。位置是**手写**的 —— `rgoc-hir` 不做源码位置跟踪（M1/M2 的活）。
pub const FIXTURE_FILE: &str = "s1_fixture";

/// S1 / S2 的固定输入：`println(1 + 2)`。
///
/// 期望（`M0-tests.md` §5.1 **T-S1-01，含 2026-10-07 修订 R1**）：
/// **stderr 精确 `3\n`，且 stdout 精确为空**，退出码 `0`。
///
/// 修订 R1 的实测依据（容器内 go1.27.1，`od -c` 逐字节）：Go 的内建 `println`
/// 写**标准错误**而非标准输出 —— 原期望写的「stdout 精确 `3\n`」是错的。
/// 两条流都要断言：只断言 stderr 会漏掉「实现顺手也往 stdout 写了一份」。
pub fn s1_program() -> Program {
    Program {
        funcs: vec![FuncDecl {
            name: "main".to_string(),
            body: vec![Stmt::Print {
                newline: true,
                // S1 的 fixture 模拟 Go 内建 `println(1 + 2)`，故写 **stderr**
                //（实测依据见 `M0-tests.md` §5.1 修订 R1）。这也正是 T-S1-01
                // 要求「stdout 精确为空」的原因 —— 不是因为「stdout 恒空」，
                // 而是因为**这个 fixture 选了 stderr**。
                stream: Stream::Stderr,
                // `eval_const` 直接产出 `Val` —— 内建调用的参数在**进入内建之前**已求值完成，
                // 这就是「内建调用边界」在类型上的体现（`M0-plan.md` T42 要求记录的三件事之一）。
                args: vec![sum_expr().eval_const().expect("1 + 2 是子集内形状")],
            }],
        }],
    }
}

/// `1 + 2` 的 HIR 形状。**同时**供 S1 与 S2 使用（交叉验证的前提）。
pub fn sum_expr() -> Expr {
    Expr::Bin {
        op: BinOp::Add,
        lhs: Box::new(int(1)),
        rhs: Box::new(int(2)),
    }
}

/// `1 << 100` 的 HIR 形状 —— T-S1-03 的核心：它**装得下任意精度、装不下 i64**。
///
/// 实测对照（go1.27.1 对同一表达式的反应）：
///
/// ```text
/// cannot use big (untyped int constant 1267650600228229401496703205376)
///            as int value in argument to built-in println (overflows)
/// ```
///
/// 即 Go 也承认这个常量先以任意精度存在，**在被赋给具体类型时**才收敛并报溢出。
pub fn big_shift_expr() -> Expr {
    Expr::Bin {
        op: BinOp::Shl,
        lhs: Box::new(int(1)),
        rhs: Box::new(int(100)),
    }
}

/// T-S1-03 的期望：常量折叠边界的完整集合。
///
/// 与 **T-C-04**（`test/printbig.go`）的冻结期望 `-9223372036854775808\n9223372036854775807\n`
/// 对齐 —— 边界是同一个（`int64` 极值），所以 S1 只需证明「收敛点与 Go 一致」。
///
/// 每一项是 `(HIR 表达式, 期望的十进制渲染, 是否能收敛到 i64)`。
/// 第三项是**关键**：它把「超宽常量」与「恰好落在边界上」区分开，
/// 前者必须被拒绝、后者必须通过 —— 两者合成一个布尔就抓不到「全都拒绝」这种错实现。
pub fn s1_const_edges() -> Vec<(Expr, String, bool)> {
    vec![
        (
            big_shift_expr(),
            "1267650600228229401496703205376".to_string(),
            false,
        ),
        (int(i64::MAX), "9223372036854775807".to_string(), true),
        (
            Expr::Const(Const::Big(BigInt::from_i64(i64::MIN))),
            "-9223372036854775808".to_string(),
            true,
        ),
        (int(1 << 62), "4611686018427387904".to_string(), true),
        (int(-(1 << 62)), "-4611686018427387904".to_string(), true),
    ]
}

/// S3 的固定输入：输出 `hello` 的 native 函数。
///
/// 期望（**T-S3-03**）：stdout **精确** `hello\n`，退出码 `0`。
///
/// 与 S1 的差别**在类型层面**：这里声明 [`Stream::Stdout`]，而 S1 声明 [`Stream::Stderr`]。
/// 这不是随意选择 —— Go 内建 `println` 实测走 stderr（`M0-tests.md` §5.1 修订 R1），
/// 而 T-S3-03 要求的是「真实 Go 程序输出 hello」的效果，对应 `os.Stdout`。
/// 做成两个枚举值（`rgoc_hir::Stream`）就是为了让编译器**无法**把两者混为一谈。
pub fn s3_hello() -> FuncDecl {
    FuncDecl {
        name: "main".to_string(),
        body: vec![Stmt::Print {
            newline: true,
            stream: Stream::Stdout,
            args: vec![Val::Str("hello".to_string())],
        }],
    }
}

/// T-S1-03 的诊断标签 —— 用于 `DiagBag`（C1 留位）。
pub const S1_UNSUPPORTED_CODE: &str = "S1-EVAL-UNSUPPORTED";

/// 整数常量表达式。
fn int(v: i64) -> Expr {
    Expr::Const(Const::Big(BigInt::from_i64(v)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rgoc_hir::Val;

    #[test]
    fn 固定输入的求值结果是_3() {
        // S1 与 S2 都要断言这一条 —— 交叉验证的锚点
        let got = s1_program()
            .main()
            .expect("fixture 必须有 main")
            .body
            .first()
            .cloned();
        let Some(Stmt::Print {
            newline,
            stream,
            args,
        }) = got
        else {
            panic!("fixture 的第一条语句应是 Print");
        };
        assert!(newline, "S1 的 fixture 用 println（带换行）");
        // 顺带钉住 stream：S1 走 stderr 是 **Go 内建 println 的实测行为**
        // （M0-tests.md §5.1 修订 R1）。若哪天这里变成 Stdout，
        // T-S1-01 的「stdout 精确为空」就会失败 —— 而那正是我们要尽早发现的。
        assert_eq!(
            stream,
            Stream::Stderr,
            "S1 的 fixture 必须写 stderr（Go 内建 println 的实测去向）"
        );
        assert_eq!(args, vec![Val::Int(BigInt::from_i64(3))]);
    }

    #[test]
    fn 固定输入不含命令行可变量() {
        // 本 crate 的 main 一律不读 env::args()；此测试是 E6「输入写死」的可执行声明。
        // 真正的检查在 tests/no_cli_input.rs（对源码做文本检查）。
        assert_eq!(s1_program().funcs.len(), 1);
    }

    #[test]
    fn 常量边界_超宽的拒绝_落在边界上的通过() {
        let edges = s1_const_edges();
        assert_eq!(edges.len(), 5, "边界用例数固定，改动须同步 T-S1-03 记录");
        for (expr, text, fits) in &edges {
            let v = expr.eval_const().expect("都是子集内形状");
            let Val::Int(b) = v else {
                panic!("应是整数");
            };
            assert_eq!(&b.to_string(), text, "十进制渲染须与 T-C-04 语义一致");
            assert_eq!(b.to_int64().is_some(), *fits, "收敛判定与预期不符：{text}");
        }
    }

    #[test]
    fn s3_fixture_是_hello_且写_stdout() {
        let f = s3_hello();
        assert_eq!(f.name, "main");
        assert_eq!(
            f.body,
            vec![Stmt::Print {
                newline: true,
                // T-S3-03 要求 stdout 精确 `hello\n` —— 与 S1 的 stderr 形成对照。
                // 这条断言的作用是：**两个 fixture 的 stream 必须不同**，
                // 一旦被「统一」，T-S1-01 与 T-S3-03 会有一个先失败。
                stream: Stream::Stdout,
                args: vec![Val::Str("hello".to_string())],
            }]
        );
    }

    #[test]
    fn 两个_fixture_的_stream_必须不同() {
        // 直接把「不得混淆」写成断言，而不是靠注释提醒。
        let s1 = s1_program().main().and_then(|f| f.body.first()).cloned();
        let s3 = s3_hello().body.first().cloned();
        let (Some(Stmt::Print { stream: a, .. }), Some(Stmt::Print { stream: b, .. })) = (s1, s3)
        else {
            panic!("两个 fixture 的第一条语句都应是 Print");
        };
        assert_ne!(
            a, b,
            "S1（Go 内建 println → stderr）与 S3（T-S3-03 → stdout）必须不同流"
        );
    }
}
