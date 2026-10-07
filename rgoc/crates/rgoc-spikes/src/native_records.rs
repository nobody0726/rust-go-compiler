//! **T-S3-04**：六项记录（调用约定 / 栈对齐 / 输出流 / 退出码 / runtime 桥接 / unwind 边界）。
//!
//! `M0-tests.md` §5.3 要求「记录**调用约定、栈对齐、输出流、退出码、runtime 桥接、unwind 边界**
//! —— **六项都要有记录**」。所以本文件按**六项**组织，每项都带**实测依据**，不给「应该…」。
//!
//! # 为什么要写成代码而不是 markdown
//!
//! 六项里有几项是**会被后续实现推翻的判断**（「不需要动栈」在有调用时就是错的）。
//! 写在文档里的判断不会被编译检查；写成结构体 + 断言，至少能保证
//! 「六项都在」这件事本身不会随时间丢失 —— `六项都齐备` 有专门的测试守着。

/// 六项记录中的一项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// 项名（固定六个，不增不减 —— 增删会让「六项齐备」的检查失去意义）。
    pub item: &'static str,
    /// 结论。
    pub finding: &'static str,
    /// 实测依据（命令或输出片段）。
    pub evidence: &'static str,
    /// 「没记录什么」—— 这一栏比 `finding` 更重要，见模块文档。
    pub gap: &'static str,
}

/// 六项记录。**顺序即 `M0-tests.md` §5.3 的原文顺序**。
pub fn records() -> Vec<Record> {
    vec![
        Record {
            item: "调用约定",
            finding: "裸 `_start`，**不经 C 运行时**。系统调用参数按 AAPCS64 走 `x0..x5`，\
                      返回值在 `x0`，系统调用号在 `x8`，`svc #0` 陷入内核。",
            evidence: "clang -nostartfiles -o s3-hello.elf s3-hello.s  →  链接成功、运行 exit=0",
            gap: "**只验证了「无参、无返回」的调用**。一旦有用户函数，参数传递、\
                  被调用方栈帧、 callee-saved 寄存器（x19-x28）保存义务全部未验证 ——\
                  那是 M7 的内容。",
        },
        Record {
            item: "栈对齐",
            finding: "本 fixture **完全不动栈**：没有 push、没有 `bl`、没有局部变量，\
                      所以 `sp` 始终是进程入口时的值，天然满足 16 字节对齐。",
            evidence: "生成的汇编里没有 sp 的读写（可 grep `\\bsp\\b` 验证为空）",
            gap: "「不需要动栈」**不等于**「栈对齐已验证」。AAPCS64 要求每次 `bl` 前 `sp` 16 字节对齐，\
                  且 leaf 函数不需保存 `fp`/`lr`、非 leaf 必须保存 —— 本 spike 两样都没测。",
        },
        Record {
            item: "输出流",
            finding: "走 `write(2)` 系统调用：`x0` = fd（**1 = stdout**、2 = stderr），\
                      `x1` = 缓冲区地址，`x2` = 字节数。fixture 声明 `Stream::Stdout` 故用 fd 1。",
            evidence: "运行 `./s3-hello.elf >o.txt 2>e.txt` → stdout = `hello\\n`（6 字节），\
                       stderr = 0 字节（`od -c` 实测）",
            gap: "**未处理 `write` 的部分写（短写）返回值**。Linux 上 write 可能只写一部分，\
                  真实实现必须循环。fixture 只有 6 字节，实践中不触发 ——\
                  故代码里留了 `TODO(S3-04)` 标记，而不是假装处理了。",
        },
        Record {
            item: "退出码",
            finding: "`exit_group(2)` 系统调用（号 **94**），`x0` = 状态码。\
                      正常结束写 0。**必须用 `exit_group` 而非 `exit`**：\
                      多线程下 `exit` 只退出当前线程。",
            evidence: "`./s3-hello.elf; echo $?` → 0；`od -c` 确认 stdout 恰为 6 字节后退出",
            gap: "**未验证非零退出码**，也未验证「panic 时应返回什么」（Go 的 panic 退出码是 2）。",
        },
        Record {
            item: "最小 runtime 桥接",
            finding: "**零 libc 符号**：`svc #0` 直达内核，不需要任何库函数。\
                      但链接产物**仍然是动态链接**，`readelf -d` 显示 `NEEDED libc.so.6` ——\
                      那是链接器默认加的，**我们一个符号都没引用**。\
                      两个必需的链接开关：`-nostartfiles`（否则 `_start` 冲突）与 \
                      `-Wl,-s`（见实测栏）。",
            evidence: "`clang -nostartfiles -Wl,-s`（不加 `-static`）；\
                       `readelf -d s3-hello.elf | grep NEEDED` → `libc.so.6`（未被引用）；\
                       不加 `-nostartfiles` 时链接**失败**：\
                       `multiple definition of _start; Scrt1.o: first defined here`；\
                       **不加 `-Wl,-s` 时产物不可复现**：同一份汇编三次链接的字节在第 66200 处不同，\
                       `readelf -p .strtab` 定位到 `hello-d9450b.o`（clang 汇编器的随机中间名，\
                       以 STT_FILE 符号留在符号表）。加 `-s` 后三次逐字节相同（66480 字节）。\
                       **佐证它是工具链噪声而非产物不稳**：三次 BuildID 完全相同\
                       （`84a05590…`，内容哈希）且差异只在 `.strtab`。",
            gap: "**「不引用 libc 符号」不等于「不需要 libc」**。要真正静态还得加 `-static`，\
                  本 spike 故意不加：加了会掩盖「到底依赖了什么」这个问题。\
                  ⚠️ **`-Wl,-s` 也意味着产物没有符号表** —— 对可执行文件的运行无影响，\
                  但**不可调试**（无函数名、无行号）。将来若要对生成物做符号级调试，\
                  必须去掉 `-s` 并改用「按 section 比对」而非「整文件逐字节比对」。\
                  另外 rgoc 的 runtime（map/scheduler/GC）**完全未涉及** —— \
                  M5 起的标准库需求是另一回事（`03` §M5 明确「宿主管理内存」）。",
        },
        Record {
            item: "unwind 边界",
            finding: "**不可 unwind**。生成的汇编没有 `.cfi_*` 指令，链接产物也没有 `.eh_frame`；\
                      任何栈回溯（panic 打印调用栈、`backtrace(3)`）都拿不到信息。",
            evidence: "汇编里无 `.cfi_startproc` / `.cfi_endproc`；`readelf -S` 无 `.eh_frame` 段",
            gap: "这是**当前形态的固有限制，不是待办**：要支持 unwind 就得在每条指令后维护 \
                  CFI 行号表并保证与 unwind 表同步 —— 那是独立的一块工作（M7 及以后）。\
                  现在写下来，是为了将来有人加 `-fasynchronous-unwind-tables` 时知道这是**新增能力**\
                  而非「本来就支持」。",
        },
    ]
}

/// 六项的固定名称（用于「六项都齐备」的检查）。
pub const SIX_ITEMS: [&str; 6] = [
    "调用约定",
    "栈对齐",
    "输出流",
    "退出码",
    "最小 runtime 桥接",
    "unwind 边界",
];

/// 渲染成可落盘的记录文本（T45 写进 benchmarks §12）。
pub fn render_records() -> String {
    let mut s = String::from("T-S3-04 native spike 六项记录\n");
    s.push_str("（每项含：结论 / 实测依据 / **没记录什么** —— 最后一栏与前两栏同等重要）\n");
    for r in records() {
        s.push_str(&format!(
            "\n## {}\n结论: {}\n实测: {}\n缺口: {}\n",
            r.item, r.finding, r.evidence, r.gap
        ));
    }
    s
}
