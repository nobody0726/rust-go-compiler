//! S3 —— native spike：固定 HIR → arm64 汇编 → `clang` → ELF
//! （`M0-plan.md` T44 / `M0-tests.md` §5.3 的 **T-S3-01…04**）。
//!
//! # 本文件**生成**汇编，不是手写一段汇编贴进 `clang`
//!
//! `03` §4 第 6 条要求「固定 HIR/SSA 函数 → arm64 汇编 → `clang` → 可运行程序」。
//! 若汇编是手写的，这一步就退化成了「手写一个能跑的 asm 文件」，**验证不到 codegen**。
//! 所以本文件从 [`rgoc_hir::FuncDecl`] 出发生成指令序列；手写汇编只出现在**测试的对照**里。
//!
//! # 明确不做的事（`03` §2 / §4 的砍项）
//!
//! - **不设 `rgoc-linker`**：链接交给容器内 `clang`（`03` §2 明确「不设首发 linker」）。
//! - **不把官方 `.s` 喂给系统汇编器**：`03` §4 明确禁止。本文件生成的汇编是**自己的**。
//!
//! # T-S3-04 要记录��六项，全部来自容器内实测，不是从文档抄的
//!
//! | 项 | 结论 | 依据 |
//! |---|---|---|
//! | 调用约定 | 裸 `_start`，无 C 运行时；参数直接放 `x0..x7` | `clang -nostartfiles` 才能链上（否则 `Scrt1.o` 已定义 `_start`） |
//! | 栈对齐 | 本 spike **不需要动栈**（无局部变量、无调用） | 一旦有调用，`sp` 必须保持 16 字节对齐 |
//! | 输出流 | `write(2)` 系统调用，`x0`=fd（1=stdout，2=stderr） | `T-S3-03` 要 stdout |
//! | 退出码 | `exit_group(2)`，`x0`=状态码 | 实测 `exit=0` |
//! | 最小 runtime 桥接 | **零 libc**：`svc #0` 直达内核 | `readelf -d` 显示仍链了 `libc.so.6`，但**没有任何符号来自它** |
//! | unwind 边界 | 无 CFI ⇒ **不可 unwind** | 缺 `.eh_frame`；`panic`/backtrace 不可用 |

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Linux aarch64 系统调用号（本 spike 只用这四个）。
mod syscall {
    /// `write(fd, buf, count)`
    pub const WRITE: u64 = 64;
    /// `exit_group(status)`
    pub const EXIT_GROUP: u64 = 94;
}

/// 标准输出的文件描述符。
const FD_STDOUT: u64 = 1;
/// 标准错误的文件描述符。
const FD_STDERR: u64 = 2;

/// codegen 失败。**不静默降级成「生成一个近似程序」**。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodegenError {
    /// 函数体里有非 print 语句。
    UnsupportedStmt(String),
    /// 函数名不是 `main`。
    NotMain(String),
    /// 一个 print 语句里有多个值 —— 子集内只支持单值。
    MultipleArgs(usize),
    /// 实参不是字符串也不是整数。
    UnsupportedArg(String),
}

impl fmt::Display for CodegenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedStmt(s) => write!(f, "S3 子集不支持该语句：{s}"),
            Self::NotMain(s) => write!(f, "S3 子集只支持 main，收到 {s}"),
            Self::MultipleArgs(n) => write!(f, "S3 子集每个 print 只支持一个值，收到 {n} 个"),
            Self::UnsupportedArg(s) => write!(f, "S3 子集不支持该实参：{s}"),
        }
    }
}

impl std::error::Error for CodegenError {}

/// 生成的产物。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    /// 汇编源码。
    pub asm: String,
    /// 汇编文件路径（写入后回填）。
    pub asm_path: PathBuf,
    /// ELF 路径（链接后回填）。
    pub elf_path: PathBuf,
}

/// 从固定 HIR 生成 arm64 汇编。
pub fn gen_asm(f: &rgoc_hir::FuncDecl) -> Result<String, CodegenError> {
    if f.name != "main" {
        return Err(CodegenError::NotMain(f.name.clone()));
    }

    // 数据段：每个字符串常量一条
    let mut data = String::new();
    // text 段里要 emit 的指令
    let mut text = String::new();

    // label 序号直接用语句下标（`enumerate`）—— 不用「自增计数器」那套：
    // 计数器在函数中段出错时容易漏加，而下标天然与语句一一对应。
    // 顺带保证「每条 print 拿到的 label 唯一」，这正是多条 print 不冲突的依据。
    for (label_id, stmt) in f.body.iter().enumerate() {
        let rgoc_hir::Stmt::Print {
            newline,
            stream,
            args,
        } = stmt
        else {
            return Err(CodegenError::UnsupportedStmt(format!("{stmt:?}")));
        };
        if args.len() != 1 {
            return Err(CodegenError::MultipleArgs(args.len()));
        }
        let val = &args[0];
        let label = format!("str{label_id}");

        // 渲染成字节。这与 S1/S2 共用 `Val::render` 的规则，
        // 但 S3 走的是**真实 write 系统调用**而非内建 print 家族 ——
        // 差别是流（stdout/stderr，见 rgoc_hir::Stream）而非渲染方式。
        let mut bytes = val.render();
        if *newline {
            bytes.push('\n');
        }
        let text_bytes = bytes.as_bytes();

        // .data 段条目
        data.push_str(&format!("{label}:\n"));
        for chunk in text_bytes.chunks(16) {
            let hex: Vec<String> = chunk.iter().map(|b| format!("0x{b:02x}")).collect();
            data.push_str(&format!("\t.byte {}\n", hex.join(", ")));
        }
        data.push_str(&format!("\t.text_len_{label} = {}\n", text_bytes.len()));

        let fd = match stream {
            rgoc_hir::Stream::Stdout => FD_STDOUT,
            rgoc_hir::Stream::Stderr => FD_STDERR,
        };

        // 代码：write(fd, label, len) 然后 exit_group(0)
        text.push_str("  // write(fd, buf, len)\n");
        text.push_str(&format!("  mov x0, #{fd}\n"));
        // adrp + add 拿数据段地址。PIE 下不能写绝对地址 ——
        // 这一点是本 spike 踩出来的（见下方注释）。
        text.push_str(&format!("  adrp x1, {label}\n"));
        text.push_str(&format!("  add x1, x1, :lo12:{label}\n"));
        // 长度用 `mov` 的立即数形式；超过 65535 时汇编器会报错，
        // 子集内不会遇到（fixture 是 "hello\n" = 6 字节）。
        text.push_str(&format!("  mov x2, #{}\n", text_bytes.len()));
        text.push_str(&format!("  mov x8, #{}\n", syscall::WRITE));
        text.push_str("  svc #0\n\n");

        // ⚠️ **必须检查 write 的返回值**：Linux 的 write 可能是**部分写**（短写），
        // 也会因信号而返回负值。子集内的 fixture 只有 6 字节，实践中不会短写；
        // 但真实实现必须循环处理。TODO 记入 T-S3-04 的「最小 runtime 桥接」缺口。
        text.push_str("  // TODO(S3-04): 需处理 write 的部分写返回值（当前 fixture 不会触发）\n\n");
    }

    // exit_group(0)
    text.push_str("  // exit_group(0)\n");
    text.push_str("  mov x0, #0\n");
    text.push_str(&format!("  mov x8, #{}\n", syscall::EXIT_GROUP));
    text.push_str("  svc #0\n");

    Ok(format!(
        "// rgoc S3 native spike —— 由 rgoc_spikes::native::gen_asm 自动生成，请勿手改\n\
         // 目标：aarch64-unknown-linux-gnu（T-S3-01 的产物）\n\
         //\n\
         // 链接方式：clang -nostartfiles（必须！否则 Scrt1.o 已定义 _start，见 T-S3-04）\n\
         .text\n\
         .global _start\n\
         _start:\n\
         {text}\n\
         .data\n\
         {data}"
    ))
}

/// 把汇编写到磁盘。
pub fn write_asm(asm: &str, dir: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let p = dir.join("s3-hello.s");
    std::fs::write(&p, asm)?;
    Ok(p)
}

/// 用容器内 `clang` 链接成 ELF。
///
/// # `-nostartfiles` 不是可选项（实测）
///
/// 默认链接会引入 `/lib/aarch64-linux-gnu/Scrt1.o`，它**已经定义了 `_start`**，
/// 于是链接报：
///
/// ```text
/// /usr/bin/ld: multiple definition of `_start'; /lib/aarch64-linux-gnu/Scrt1.o: first defined here
/// /usr/bin/ld: undefined reference to `main'
/// ```
///
/// 即：自带的 C 运行时启动代码与我们手写的 `_start` 冲突。
/// 裸汇编程序必须显式 `-nostartfiles` 去掉它。
///
/// # `-Wl,-s` 为什么必需（**实测发现的不可复现性**，T-S3-05 的关键）
///
/// 不加 strip 时，**同一份汇编重复链接三次，产物的字节不一致**（实测差在第 66200 字节）。
/// 根因由 `readelf -p .strtab` 定位到：
///
/// ```text
/// String dump of section '.strtab':
///   [     1]  hello-d9450b.o     ← ★ 随机后缀
///   [    10]  $x.0
///   [    15]  m
/// ```
///
/// 那个 `hello-d9450b.o` 是 **clang 汇编器生成的中间目标文件名**，后缀每次不同，
/// 以 STT_FILE 符号的形式留在 `.strtab` 里。**它与我们的 codegen 无关**，
/// 但足以让「三次逐字节相同」永远不成立。
///
/// 三条佐证它是纯工具链噪声、而非我们的产物不稳定：
///
/// 1. **BuildID 三次完全相同**（`84a05590a25c902d64f170197b7ab94b7dcb508a`）——
///    BuildID 是内容哈希，相同即说明**代码内容可复现**；
/// 2. 差异**只出现在 `.strtab`**，`.text` / `.data` 一致；
/// 3. 加 `-Wl,-s`（去符号表）后**三次逐字节相同**，且程序照常运行
///    （66480 字节、stdout `hello\n`、`exit=0`）。
///
/// 所以这里 strip 不是「为了让测试通过而放宽判定」——
/// 符号表对这个可执行文件的**运行**毫无作用，剥掉它反而让「结果一致」这条门禁
/// 真正约束到 **rgoc 自己的产物**上。`-s` 的取舍记在 T-S3-04 的「最小 runtime 桥接」项。
///
/// # `-static` 为什么不用
///
/// 也实测过：`-static` 能链，但会让 ELF 变成 static-pie 且体积变大，
/// 而本 spike 要验证的是「**零 runtime 依赖**能否跑通」，用 dynamic + 不引用任何
/// libc 符号已足够。加上 `-static` 反而会掩盖「我们到底依赖了什么」。
pub fn link_elf(asm_path: &Path, dir: &Path) -> Result<PathBuf, String> {
    // ⚠️ 文件名**不带扩展名**，且 bin 的输出目录就是 `target/` ——
    // 这是为了让 `M0-plan.md` T44 的验收命令**原样可用**：
    //
    // ```sh
    // scripts/in-container.sh bash -lc '/work/rgoc/target/s3-hello; echo "exit=$?"'
    // ```
    //
    // 门禁命令与文档不一致时，文档就成了「没人跑过的那份」。所以这里刻意对齐。
    let elf = dir.join("s3-hello");
    let out = Command::new("clang")
        .arg("-nostartfiles")
        // 去符号表：剥掉 clang 内部中间产物的随机 STT_FILE 名，
        // 使同一份汇编的链接产物**逐字节可复现**（实测依据见本函数文档）。
        .arg("-Wl,-s")
        .arg("-o")
        .arg(&elf)
        .arg(asm_path)
        .output()
        .map_err(|e| format!("应能启动 clang：{e}"))?;
    if !out.status.success() {
        return Err(format!(
            "clang 链接失败（exit={:?}）\nstderr: {}\nstdout: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr),
            String::from_utf8_lossy(&out.stdout)
        ));
    }
    Ok(elf)
}

/// 用 `file` 确认产物形态（**T-S3-02**）。
///
/// 判定必须含**两个**条件：`ELF 64-bit LSB` 且 `ARM aarch64`。
/// 只判其一都会漏 —— x86-64 的 ELF 同样含 `ELF 64-bit LSB`。
pub fn check_t_s3_02(elf: &Path) -> Result<String, String> {
    let out = Command::new("file")
        .arg(elf)
        .output()
        .map_err(|e| format!("应能启动 file：{e}"))?;
    if !out.status.success() {
        return Err(format!(
            "file 执行失败：{}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    for want in ["ELF 64-bit LSB", "ARM aarch64"] {
        if !text.contains(want) {
            return Err(format!(
                "T-S3-02 失败：`file` 输出缺 {want:?}\n实际：{text}"
            ));
        }
    }
    Ok(text)
}

/// 运行 ELF，捕获两条流与退出码（**T-S3-03**）。
pub fn run_elf(elf: &Path) -> Result<(String, String, i32), String> {
    let out = Command::new(elf)
        .output()
        .map_err(|e| format!("应能运行 ELF：{e}"))?;
    Ok((
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
        out.status.code().unwrap_or(-1),
    ))
}

/// **T-S3-03**：stdout 精确 `hello\n`、stderr 精确为空、退出码 `0`。
pub fn check_t_s3_03(elf: &Path) -> Result<(), String> {
    let (stdout, stderr, code) = run_elf(elf)?;
    if stdout != "hello\n" {
        return Err(format!(
            "T-S3-03 失败：stdout 应精确为 \"hello\\n\"，实际 {stdout:?}"
        ));
    }
    if !stderr.is_empty() {
        return Err(format!("T-S3-03 失败：stderr 应精确为空，实际 {stderr:?}"));
    }
    if code != 0 {
        return Err(format!("T-S3-03 失败：退出码应为 0，实际 {code}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::s3_hello;

    #[test]
    fn 生成的汇编含_start_与系统调用号() {
        let asm = gen_asm(&s3_hello()).expect("应能生成");
        assert!(asm.contains(".global _start"), "{asm}");
        assert!(asm.contains("_start:"), "{asm}");
        // write = 64, exit_group = 94
        assert!(asm.contains("mov x8, #64"), "{asm}");
        assert!(asm.contains("mov x8, #94"), "{asm}");
    }

    #[test]
    fn 生成的汇编用_adrp_加_lo12_取地址而非绝对地址() {
        // PIE 下写绝对地址会链接失败；必须用 adrp + :lo12: 组合
        let asm = gen_asm(&s3_hello()).expect("应能生成");
        assert!(asm.contains("adrp x1, str0"), "{asm}");
        assert!(asm.contains("add x1, x1, :lo12:str0"), "{asm}");
        assert!(
            !asm.contains("ldr x1, ="),
            "不能用伪指令取绝对地址（PIE 下会失败）：{asm}"
        );
    }

    #[test]
    fn hello_的字节序列正确() {
        let asm = gen_asm(&s3_hello()).expect("应能生成");
        // "hello\n" = 0x68 0x65 0x6c 0x6c 0x6f 0x0a
        assert!(asm.contains("0x68, 0x65, 0x6c, 0x6c, 0x6f, 0x0a"), "{asm}");
        assert!(asm.contains(".text_len_str0 = 6"), "{asm}");
        assert!(asm.contains("mov x2, #6"), "{asm}");
    }

    #[test]
    fn stdout_用_fd_1_stderr_用_fd_2() {
        let s3 = gen_asm(&s3_hello()).expect("S3 应能生成");
        assert!(s3.contains("mov x0, #1"), "S3 走 stdout：{s3}");

        // 构造一个走 stderr 的函数，确认 fd 不同 —— 这是 stream 字段存在的意义
        let err_fn = rgoc_hir::FuncDecl {
            name: "main".to_string(),
            body: vec![rgoc_hir::Stmt::Print {
                newline: true,
                stream: rgoc_hir::Stream::Stderr,
                args: vec![rgoc_hir::Val::Str("x".to_string())],
            }],
        };
        let e = gen_asm(&err_fn).expect("应能生成");
        assert!(e.contains("mov x0, #2"), "stderr 应是 fd 2：{e}");
    }

    #[test]
    fn 生成结果_可重复_字节级相同() {
        // T-S3-05 可复现性的基础：codegen 是纯函数
        let a = gen_asm(&s3_hello()).expect("应能生成");
        let b = gen_asm(&s3_hello()).expect("应能生成");
        assert_eq!(a, b, "codegen 必须是纯函数（否则 E6 无从判定）");
    }

    #[test]
    fn 多值_被显式拒绝而不是悄悄只取第一个() {
        let f = rgoc_hir::FuncDecl {
            name: "main".to_string(),
            body: vec![rgoc_hir::Stmt::Print {
                newline: true,
                stream: rgoc_hir::Stream::Stdout,
                args: vec![
                    rgoc_hir::Val::Str("a".to_string()),
                    rgoc_hir::Val::Str("b".to_string()),
                ],
            }],
        };
        assert_eq!(gen_asm(&f), Err(CodegenError::MultipleArgs(2)));
    }

    #[test]
    fn 非_main_被拒绝() {
        let f = rgoc_hir::FuncDecl {
            name: "helper".to_string(),
            body: vec![],
        };
        assert_eq!(
            gen_asm(&f),
            Err(CodegenError::NotMain("helper".to_string()))
        );
    }

    #[test]
    fn 非_print_语句被拒绝() {
        let f = rgoc_hir::FuncDecl {
            name: "main".to_string(),
            body: vec![rgoc_hir::Stmt::Return(rgoc_hir::Val::Void)],
        };
        assert!(matches!(gen_asm(&f), Err(CodegenError::UnsupportedStmt(_))));
    }

    #[test]
    fn 错误信息可读且非空() {
        // spike 记录会直接引用这些错误文本，不能是「Error」
        assert!(!CodegenError::MultipleArgs(2).to_string().is_empty());
        assert!(CodegenError::MultipleArgs(2).to_string().contains("2"));
        assert!(!CodegenError::NotMain("x".into()).to_string().is_empty());
    }

    #[test]
    fn 汇编文件带_do_not_edit_头() {
        let asm = gen_asm(&s3_hello()).expect("应能生成");
        assert!(
            asm.contains("自动生成"),
            "汇编应标明是生成的，避免有人手改后与 codegen 漂移：{asm}"
        );
    }
}
