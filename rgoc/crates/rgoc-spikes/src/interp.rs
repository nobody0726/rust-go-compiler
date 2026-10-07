//! S1 解释器 —— 宿主求值固定 HIR（`M0-plan.md` T42 / `M0-tests.md` §5.1）。
//!
//! 放在 lib 里而不是 bin 里，理由与 harness 侧的纪律同形：**判定逻辑只有一处**。
//! S2 要与 S1 交叉验证（`T-S2-01`），若 S1 的求值逻辑锁在 `main` 里，
//! S2 就只能靠「跑一遍 bin 比 stderr」来对照 —— 那是两份比较，不是一份实现。
//!
//! # T42 要求记录的三件事（写进代码而不只写进文档）
//!
//! 1. **值表示**：常量在 HIR 里是 `Const::Big(BigInt)`（任意精度），
//!    **只在内建调用点收敛到具体类型**，溢出即报错。对照证据是 Go 对 `println(1<<100)`
//!    报 `cannot use big (untyped int constant ...) (overflows)` —— Go 也是「先任意精度、
//!    后按类型收敛」。见 `T-S1-03`。
//! 2. **内建调用边界**：`Stmt::Print` 的实参是**已求值的 `Val`** 而非 `Expr`。
//!    边界画在 HIR 里（`rgoc_hir::Stmt`），解释器只负责按 print 家族渲染。
//! 3. **输出流走法**：`print` / `println` 写 **stderr**（实测 go1.27.1，`od -c` 逐字节，
//!    stdout 长度为 0）。**stdout 必须精确为空** —— 这条断言不是形式主义，
//!    它挡的是「实现顺手也往 stdout 写了一份」这种错误。

use rgoc_hir::{Diag, DiagBag, Pos, Program, Stmt, Stream, Val};

// 本文件在 rgoc-spikes **内部**，故引用同 crate 的模块要用 `crate::`。
// （写成 `rgoc_spikes::` 只有在 tests/ 与 bin/ 里才成立 —— 三个位置的可见性规则各不相同。）
use crate::fixtures::{FIXTURE_FILE, S1_UNSUPPORTED_CODE, s1_const_edges, s1_program};

/// 求值结果：两条流**分开**返回，不混。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutput {
    /// 要写 **stderr** 的字节（已含 `println` 的换行）。
    pub stderr: String,
    /// 要写 **stdout** 的字节。
    ///
    /// ⚠️ **不是恒为空**：由 fixture 声明的 [`Stream`] 决定。
    /// S1 的 fixture 用 [`Stream::Stderr`]（对齐 Go 内建 `println` 的实测行为），
    /// 所以 S1 的 stdout 才是空的；S3 的 fixture 用 [`Stream::Stdout`]。
    /// 「恒为空」曾经是第一版的写法 —— 那把 S3 的需求错当成了语言规则。
    pub stdout: String,
}

impl RunOutput {
    /// 供 S2 交叉验证的稳定摘要：**只取 stderr**。
    ///
    /// 为什么只取 stderr：S1 与 S2 的 fixture 都用 [`Stream::Stderr`]，
    /// stdout 两侧恒等，纳入比较不增加区分度。
    pub fn signature(&self) -> &str {
        &self.stderr
    }
}

/// 解释执行一个 `Program`。只支持子集内形状；遇到别的形状**返回诊断**，不 panic、不猜值。
pub fn interpret(p: &Program) -> Result<RunOutput, Vec<Diag>> {
    let Some(main) = p.main() else {
        let mut bag = DiagBag::new();
        bag.push(Diag::new(
            Pos::new(FIXTURE_FILE, 1, 1),
            "S1-NO-MAIN",
            "固定输入必须含 main",
        ));
        return Err(bag.into_sorted());
    };

    let mut bag = DiagBag::new();
    let mut stderr = String::new();
    let mut stdout = String::new();

    for (i, stmt) in main.body.iter().enumerate() {
        match stmt {
            Stmt::Print {
                newline,
                stream,
                args,
            } => {
                // 实测（容器内 go1.27.1，`od -c` 逐字节；程序见本文件下方的记录表）：
                //
                //   print("a", "b", 1, 2)   → ab12          ← **无分隔符**
                //   println("a", "b", 1, 2) → "a b 1 2\n"   ← 空格分隔 + 末尾换行
                //   print()                 → （无输出）
                //   println()               → "\n"
                //
                // 第一版这里对两者都用了 `join(" ")`，被
                // `print_不加空格与换行_println_都加` 抓到。**Go 规范只把 print/println
                // 列为内建名，没有规定分隔符**，所以这条行为只能问 oracle，不能凭直觉 ——
                // 与 `M0-tests.md` §5.1 修订 R1 同类（那里是「println 走 stderr」）。
                let rendered: Vec<String> = args.iter().map(Val::render).collect();
                let text = if *newline {
                    // println：操作数之间一个空格，末尾一个换行
                    let mut t = rendered.join(" ");
                    t.push('\n');
                    t
                } else {
                    // print：直接拼接，既不分隔也不换行
                    rendered.concat()
                };
                // **按 stream 写对应的流**。不写死 stderr：S3 的 fixture 要写 stdout，
                // 而把「永远写 stderr」和「永远写 stdout」都是把某个 fixture 的需求
                // 错当成语言规则 —— 参见 `rgoc_hir::Stream` 的文档。
                match stream {
                    Stream::Stderr => stderr.push_str(&text),
                    Stream::Stdout => stdout.push_str(&text),
                }
            }
            Stmt::Return(_) => {
                // 子集内不出现 main 的 return；显式拒绝而不是忽略
                // —— 忽略会让「多出来的语句」悄悄不生效。
                bag.push(Diag::new(
                    Pos::new(FIXTURE_FILE, (i + 1) as u32, 1),
                    S1_UNSUPPORTED_CODE,
                    "S1 子集不支持 main 里的 return",
                ));
            }
        }
    }

    let diags = bag.into_sorted();
    if diags.is_empty() {
        Ok(RunOutput { stderr, stdout })
    } else {
        Err(diags)
    }
}

/// **T-S1-01**：stderr 精确 `3\n` 且 stdout 精确为空。
pub fn check_t_s1_01() -> Result<RunOutput, String> {
    let out = interpret(&s1_program()).map_err(|d| {
        d.iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    })?;
    if out.stderr != "3\n" {
        return Err(format!(
            "T-S1-01 失败：stderr 应精确为 \"3\\n\"，实际 {:?}",
            out.stderr
        ));
    }
    if !out.stdout.is_empty() {
        return Err(format!(
            "T-S1-01 失败：内建 println 走 stderr，stdout 应精确为空，实际 {:?}",
            out.stdout
        ));
    }
    Ok(out)
}

/// **T-S1-02**：求值是纯函数 —— 同一输入连续两次结果逐字节相同。
///
/// 完整 E6（三次独立进程 + 环境比对）在 T45；这里只守「同一进程内可重复」这一半。
pub fn check_t_s1_02() -> Result<(), String> {
    let first = check_t_s1_01()?;
    let second = check_t_s1_01()?;
    if first != second {
        return Err(format!(
            "T-S1-02 失败：两次求值结果不同\n  第一次 {:?}\n  第二次 {:?}",
            first, second
        ));
    }
    Ok(())
}

/// **T-S1-03**：常量折叠边界与 T-C-04（`printbig.go`）语义一致。
///
/// 返回每项的 `(十进制渲染, 是否能收敛到 i64)`，供打印成可复核记录。
pub fn check_t_s1_03() -> Result<Vec<(String, bool)>, String> {
    let mut rows = Vec::new();
    for (expr, expected, fits) in s1_const_edges() {
        let Some(v) = expr.eval_const() else {
            return Err(format!(
                "T-S1-03 失败：{expected} 不在 S1 子集内（eval_const 返回 None）"
            ));
        };
        let Val::Int(b) = v else {
            return Err(format!("T-S1-03 失败：{expected} 求值结果不是整数"));
        };
        let got = b.to_string();
        if got != expected {
            return Err(format!(
                "T-S1-03 失败：期望 {expected}，实际 {got} —— 常量折叠丢位了"
            ));
        }
        // 收敛判定必须逐项一致：超宽的必须拒绝、落在边界的必须通过。
        // 「全都拒绝」和「全都通过」都要被抓到，所以不能只比总数。
        if b.to_int64().is_some() != fits {
            return Err(format!(
                "T-S1-03 失败：{got} 的收敛判定应为 fits={fits}（须与 Go 的 overflows 一致）"
            ));
        }
        rows.push((got, fits));
    }
    Ok(rows)
}

/// 把 T-S1-03 的边界记录渲染成可复核文本（供 T45 落盘进 benchmarks）。
pub fn render_records(rows: &[(String, bool)]) -> String {
    let mut s = String::from("T-S1-03 常量折叠边界（对照 T-C-04 printbig.go 语义）\n");
    s.push_str("十进制渲染\t收敛到 i64\t说明\n");
    for (text, fits) in rows {
        let note = if *fits {
            "在 int64 边界内，可作为类型化值"
        } else {
            "超出 int64 —— Go 在此处报 overflows，必须拒绝收敛"
        };
        s.push_str(&format!("{text}\t{fits}\t{note}\n"));
    }
    s
}
