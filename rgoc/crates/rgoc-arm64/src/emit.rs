//! 确定性文本汇编（`M7-design.md` §3.1；`C5` §9）。
//!
//! # 这个模块只有一个职责：**把指令序列拼成字节可复现的 `.s`**
//!
//! `T-NAT-48` 的判据是「同一份 SSA 两次生成逐字节相同」。要满足它，这里必须
//! **一个不确定源都不许有**：
//!
//! | 禁 | 为什么 |
//! |---|---|
//! | `HashMap` 迭代序 | 同一进程两次都可能不同（`RandomState` 每次启动换种子） |
//! | 地址 / 指针值 | 每次运行都不同 |
//! | 时间戳 / 主机名 / 绝对路径 | 换机器就变 |
//! | 内容哈希做标签名 | 同一段字节在不同上下文里可能撞标签，撞了就合并，顺序随之变 |
//!
//! 所以这里的状态只有三个计数器（字符串 / 浮点常量 / 通用标签），**按调用顺序**递增。
//!
//! # 为什么用 `.byte` 而不是 `.ascii`
//!
//! `.ascii` 要处理转义（`\n`、`\0`、非 ASCII 字节怎么写），而转义规则本身是
//! 一个「看起来对但没验过」的面。`.byte` 把每个字节原样写出来 —— 没有规则，
//! 也就没有规则错。

use std::fmt::Write as _;

/// 一段待链接的汇编（三段：`.text` / `.rodata` / `.data`）。
///
/// 三段分开累加、`render` 时按**固定顺序**拼接 —— 这样「先写代码后写常量」与
/// 「先写常量后写代码」得到同一份输出，调用方的书写顺序不影响产物。
#[derive(Debug, Default)]
pub struct Asm {
    /// `.text` 段（指令与函数标签）。
    text: String,
    /// `.rodata` 段（字符串与浮点常量）。
    rodata: String,
    /// `.data` 段（可写的包级变量）。
    data: String,
    /// 下一个字符串标签号（按**调用顺序**递增，不按内容）。
    next_str: u32,
    /// 下一个浮点常量标签号。
    next_f64: u32,
    /// 下一个通用标签号（分支目标等）。
    next_label: u32,
}

impl Asm {
    /// 空的一段汇编。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 声明一个全局可见的符号（`_start` / `main` / `init$k`）。
    pub fn global(&mut self, name: &str) {
        let _ = writeln!(self.text, ".global {name}");
    }

    /// 打一个标签（函数入口或分支目标）。
    pub fn label(&mut self, name: &str) {
        let _ = writeln!(self.text, "{name}:");
    }

    /// 追加一条指令（自动缩进两格，与 GNU as 的惯例一致）。
    pub fn ins(&mut self, line: &str) {
        let _ = writeln!(self.text, "  {line}");
    }

    /// 追加一条**伪指令**（`.p2align` / `.section` 之类，不缩进）。
    pub fn directive(&mut self, line: &str) {
        let _ = writeln!(self.text, "{line}");
    }

    /// 注释（`//` 形式，as 与 objdump 都认）。
    pub fn comment(&mut self, text: &str) {
        let _ = writeln!(self.text, "  // {text}");
    }

    /// 申请一个新的通用标签名（分支目标用）。
    ///
    /// 返回的名字**不含**函数名 —— 调用方自己拼前缀（`format!("{fn}.L{}", asm.fresh_label())`），
    /// 因为标签的作用域是整个文件，两个函数都叫 `.L0` 会撞。
    pub fn fresh_label(&mut self) -> u32 {
        let n = self.next_label;
        self.next_label += 1;
        n
    }

    /// 把一段字节放进 `.rodata`，返回它的标签。
    ///
    /// # 为什么**不**按内容去重
    ///
    /// 去重要么用哈希表（不确定源），要么用线性查找（O(n²) 且让标签分配依赖
    /// 已有内容 —— 于是「同一段字节第二次出现」会拿到**旧标签**，整个文件的
    /// 标签编号随内容重复情况而变）。而标签名是 `T-NAT-48` 的比对面的一部分：
    /// 一旦它依赖内容，两份「语义相同但书写顺序不同」的 IR 就会产出不同的 `.s`。
    ///
    /// 代价是常量池里可能有重复字节 —— 对 MVP 的体积完全不重要。
    pub fn rodata_bytes(&mut self, bytes: &[u8]) -> String {
        let label = format!(".Lstr{}", self.next_str);
        self.next_str += 1;
        // 8 字节对齐：字符串本身不需要，但紧随其后的 `.quad` 需要；
        // 统一对齐省掉「这一段要不要对齐」的判断（少一个可错的地方）。
        let _ = writeln!(self.rodata, ".p2align 3");
        let _ = writeln!(self.rodata, "{label}:");
        if bytes.is_empty() {
            // 空串也必须**有地址**（`&s[0]` 在空串上是合法的：Go 允许 len==0 时取地址）。
            // 写一个 0 字节保证标签后面不是「下一个标签」，否则 `adrp/add` 会指到别处。
            let _ = writeln!(self.rodata, "  .byte 0");
        } else {
            for chunk in bytes.chunks(16) {
                let list: Vec<String> = chunk.iter().map(u8::to_string).collect();
                let _ = writeln!(self.rodata, "  .byte {}", list.join(","));
            }
        }
        label
    }

    /// 把一个 `f64` 的**位模式**放进 `.rodata`，返回它的标签。
    ///
    /// 用位模式（`.quad 0x…`）而不是 `.double 1.5`：后者要经过汇编器的十进制解析，
    /// 而十进制→二进制的舍入是**另一个**可能出错的地方（`-0.0` / `NaN` / 次正规数
    /// 都不是「写出来就一样」）。位模式没有解析，也就没有舍入。
    pub fn rodata_f64(&mut self, bits: u64) -> String {
        let label = format!(".Lf64_{}", self.next_f64);
        self.next_f64 += 1;
        let _ = writeln!(self.rodata, ".p2align 3");
        let _ = writeln!(self.rodata, "{label}:");
        let _ = writeln!(self.rodata, "  .quad 0x{bits:016x}");
        label
    }

    /// 把一段字节放进 `.data`（可写），返回它的标签。
    pub fn data_bytes(&mut self, bytes: &[u8]) -> String {
        // 与 `rodata_bytes` 共用计数器是**故意**的：两个段里的标签同处一个命名空间，
        // 分开计数就可能出现 `.Lstr3` 同时在 `.rodata` 与 `.data` 里。
        let label = format!(".Ldata{}", self.next_str);
        self.next_str += 1;
        let _ = writeln!(self.data, ".p2align 3");
        let _ = writeln!(self.data, "{label}:");
        for chunk in bytes.chunks(16) {
            let list: Vec<String> = chunk.iter().map(u8::to_string).collect();
            let _ = writeln!(self.data, "  .byte {}", list.join(","));
        }
        label
    }

    /// 把一段字节放进 `.data` 并**用给定的名字**当标签（包级全局用）。
    ///
    /// 与 [`Asm::data_bytes`] 的区别只有标签来源：全局的符号名必须是稳定的
    /// `g<id>`（`adrp g3` 要能指到它），不能是 `.Ldata<N>` 这种按调用顺序的编号 ——
    /// 否则「同一个全局在两个函数里」会拿到两个名字。
    pub fn data_named(&mut self, label: &str, bytes: &[u8]) {
        let _ = writeln!(self.data, ".p2align 3");
        let _ = writeln!(self.data, "{label}:");
        for chunk in bytes.chunks(16) {
            let list: Vec<String> = chunk.iter().map(u8::to_string).collect();
            let _ = writeln!(self.data, "  .byte {}", list.join(","));
        }
    }

    /// 拼成最终文本。**固定顺序**：`.text` → `.rodata` → `.data`。
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("// 由 rgoc-arm64 生成，请勿手改\n");
        out.push_str(".text\n");
        out.push_str(&self.text);
        if !self.rodata.is_empty() {
            out.push_str(".section .rodata\n");
            out.push_str(&self.rodata);
        }
        if !self.data.is_empty() {
            out.push_str(".data\n");
            out.push_str(&self.data);
        }
        out
    }

    /// `.text` 段的当前内容（测试用：看指令序列，不必整份 `render`）。
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }
}
