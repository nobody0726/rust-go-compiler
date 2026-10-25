//! 调用约定：寄存器边界与栈实参（`G-M7-01`；`T-NAT-09` / `T-NAT-21`）。
//!
//! # 这一条挡的是什么
//!
//! M7 早先的实现只把前 8 个整数实参放进 `x0..x7`，第 9 个开始写 `[sp, #k]` —— 而
//! `[sp, #k]` 是**本方帧**内部，不是调用方为被调方预留的区。于是：
//!
//! - 第 9 个实参会覆盖本帧最低的槽（可能是还活着的值）；
//! - 被调方从 `[fp+16]` 读到的是垃圾。
//!
//! 这条错在 < 9 个实参的程序上**完全看不出来**，而语料里几乎全是 < 9 个 —— 所以
//! 它只能靠专门的 fixture 抓。
//!
//! # 两条守卫的强度不一样（读的时候要注意）
//!
//! | 用例 | 抓什么 | 强度 |
//! |---|---|---|
//! | `twelve_args`（4 个栈字 = 32 B） | **帧记账**：`Frame::outgoing` 有没有算进帧大小 | 硬 —— 32 B 远超「向上取 16」最多 16 B 的余量，突变实测变红 |
//! | `ten_args`（2 个栈字 = 16 B） | **边界值**：第 9、10 个实参的偏移与顺序 | 软 —— 帧底恰好有 16 B 对齐余量时它会「碰巧绿」 |
//!
//! 所以「帧记账」的判据是 `twelve_args`，别拿 `ten_args` 当它。
//!
//! # 判据
//!
//! 每条都 `native == 解释器`（逐字节）。解释器不涉及寄存器分配，是独立通路。
//! 另外每条**自己也算一遍期望值**：两边一起错时对撞仍然绿，所以能钉死的地方就钉死。
//!
//! # 这里**没有**「聚合按值传参」的用例（`G-M7-08`）
//!
//! 试过，红了，而且红在**上游**：`build.rs` 把一个 `func f(s S)` 的形参类型记成
//! `*S`（按指针传），它的 home 槽里存的是**指针**；但 `s.N` 的 place lowering 仍然
//! `addr-of r0`（取槽的地址）再 `field`，于是 `&slot` 被当成 `&S` ——
//! 读出来是**指针的位模式**，写进去会**覆盖指针**。
//!
//! 这是 M6 的表示问题，不是寄存器分配问题；语料里没有一个文件按值传结构体
//! （所以 39/39 全绿也没抓到）。**没有**写一条「把错误行为钉成期望值」的用例 ——
//! 那种测试比没有更坏。已登记为 `M7-manifest.json` 的 `G-M7-08`。

use std::path::{Path, PathBuf};

use rgoc_driver::build_cmd;
use rgoc_driver::cli::{BuildCommand, Emit, RunCommand, Via};

fn scratch() -> PathBuf {
    let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/native-args");
    std::fs::create_dir_all(&d).expect("建目录失败");
    d
}

/// 跑一条：返回 native 的 stdout；同时断言 `native == 解释器 == want`。
fn check(name: &str, src: &str, want: &str) {
    let dir = scratch();
    let go = dir.join(format!("{name}.go"));
    std::fs::write(&go, src).expect("写 fixture 失败");
    let exe = dir.join(name);

    let b = BuildCommand {
        files: vec![go.clone()],
        output: Some(exe.clone()),
        emit: Emit::Elf,
        keep_asm: false,
        opt: rgoc_ssa::opt::Config::all(),
        lang: None,
    };
    let out = build_cmd::run_build(&b).expect("build 不该失败");
    assert_eq!(out.exit, 0, "{name} 构建失败：{}", out.stderr);

    let run = std::process::Command::new(&exe)
        .output()
        .expect("应能启动产物");
    assert!(
        run.status.success(),
        "{name} 退出码 {:?}，stderr={}",
        run.status.code(),
        String::from_utf8_lossy(&run.stderr)
    );
    let native = String::from_utf8_lossy(&run.stdout).into_owned();

    let r = RunCommand {
        files: vec![go],
        dump_hir: false,
        max_steps: None,
        lang: None,
        via: Via::Hir,
        opt: rgoc_ssa::opt::Config::all(),
    };
    let hir = rgoc_driver::run_cmd::run_run(&r).expect("解释器不该失败");
    assert_eq!(hir.exit, 0, "{name} 解释器失败：{}", hir.stderr);
    let hir = hir.stdout;

    assert_eq!(native, hir, "{name}：native 与解释器不一致");
    assert_eq!(native, want, "{name}：与手算期望值不一致");
}

#[test]
fn t_nat_21_八个整数实参全在寄存器里() {
    // 边界内的那一侧：x0..x7 刚好用完，一个都不用上栈。
    let src = "package main\n\nfunc sum(a, b, c, d, e, f, g, h int) int {\n\treturn a + b*10 + c*100 + d*1000 + e*10000 + f*100000 + g*1000000 + h*10000000\n}\n\nfunc main() {\n\tprintln(sum(1, 2, 3, 4, 5, 6, 7, 8))\n}\n";
    check("eight_args", src, "87654321\n");
}

#[test]
fn t_nat_09_十个整数实参_第九第十走栈() {
    // 边界外：第 9、10 个实参上栈；被调方必须从 `[fp+16]` / `[fp+24]` 读。
    let src = "package main\n\nfunc sum(a, b, c, d, e, f, g, h, i, j int) int {\n\treturn a + b*10 + c*100 + d*1000 + e*10000 + f*100000 + g*1000000 + h*10000000 + i*100000000 + j*1000000000\n}\n\nfunc main() {\n\tprintln(sum(1, 2, 3, 4, 5, 6, 7, 8, 9, 1))\n}\n";
    check("ten_args", src, "1987654321\n");
}

#[test]
fn t_nat_21_混合类别的寄存器两套独立计数() {
    // 4 个 int + 4 个 float32 + 2 个 int：浮点不占 `x`，所以第 9、10 个 int 才上栈。
    // 如果两套计数被写成一套，这里一定错位。
    let src = "package main\n\nfunc mix(a int, f1 float32, b int, f2 float32, c int, f3 float32, d int, f4 float32, e int, g int) int {\n\ts := a + b + c + d + e*100 + g*1000\n\treturn s + int(f1+f2+f3+f4)\n}\n\nfunc main() {\n\tprintln(mix(1, 0.5, 2, 0.5, 3, 0.5, 4, 0.5, 5, 6))\n}\n";
    check("mixed_args", src, "6512\n");
}

#[test]
fn t_nat_09_字符串实参占两个字_跨越寄存器边界() {
    // `string` 占 2 个 `x`：4 个字符串刚好用完 x0..x7，第 5 个整串上栈（两个字）。
    let src = "package main\n\nfunc cat(a, b, c, d, e string) string {\n\treturn a + b + c + d + e\n}\n\nfunc main() {\n\tprintln(cat(\"a\", \"b\", \"c\", \"d\", \"e\"))\n\tprintln(len(cat(\"11\", \"22\", \"33\", \"44\", \"55\")))\n}\n";
    check("str_args", src, "abcde\n10\n");
}

#[test]
fn t_nat_09_第11个起连续走栈且偏移递增() {
    // 12 个 int：x0..x7 之后 4 个上栈，偏移 0/8/16/24 依次递增。
    let src = "package main\n\nfunc f(a, b, c, d, e, f2, g, h, i, j, k, l int) int {\n\treturn i + j*2 + k*3 + l*4\n}\n\nfunc main() {\n\tprintln(f(0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4))\n}\n";
    check("twelve_args", src, "30\n");
}
