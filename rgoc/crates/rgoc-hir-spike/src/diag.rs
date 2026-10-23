//! 位置与诊断 —— **C1 契约在此处只留位**。
//!
//! # ⚠️ 这里没有实现「源码位置跟踪」
//!
//! `M0-plan.md` T40 的要求是：「C1 契约在此处**只留位**：位置表示与诊断排序规则固定下来，
//! 但**不得**被误读为『M0 已实现源码位置跟踪』」。所以：
//!
//! - 本模块**没有任何从源码文本计算位置的能力**（那是 **M1 lexer / M2 parser** 的活，
//!   且 AST 保真是 M2 的门禁内容）；
//! - 位置由**构造调用方直接给出**（spike 的 fixture 是手写的，位置是手写的标签）；
//! - 存在的唯一目的是：三个 spike 在记录「这个值从哪条语句来」时有个统一说法，
//!   以及**诊断排序规则现在就定下来**，避免 M2 之后各写各的。
//!
//! 排序规则（**现在就冻结**，C1 会把它固化为契约）：
//!
//! 1. 先按 [`Pos::file`]，**按路径字典序**（不按解析顺序 —— 诊断输出不依赖求值顺序）；
//! 2. 再按 [`Pos::line`]、[`Pos::col`] 升序；
//! 3. 同一位置的多条诊断，按 [`Diag::code`] 升序，**保证同一输入的输出唯一**。
//!
//! 第 3 条不是可有可无的：如果同一位置的两条诊断顺序依赖哈希表迭代序，
//! 那么 E6 的「重复执行 3 次结果一致」会随机失败，而这种失败极难定位。

use std::fmt;

/// 源码位置。**由调用方直接给出**，本 crate 不从文本计算（见模块文档）。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pos {
    /// 文件标识（spike 用 fixture 名，如 `s1_fixture`）。
    pub file: String,
    /// 行号，**1 起**（与 Go 与 `01-feature-set.md` 的约定一致，0 起是 JS 的习惯，别混）。
    pub line: u32,
    /// 列号，**1 起**（字节列，非 rune 列 —— 定位以字节为准，跨语言比较才不会歧义）。
    pub col: u32,
}

impl Pos {
    /// 构造一个位置。
    pub fn new(file: &str, line: u32, col: u32) -> Self {
        Self {
            file: file.to_string(),
            line,
            col,
        }
    }
}

impl fmt::Display for Pos {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // 形如 `s1_fixture:3:7`，与 `02-test-inventory.md` 里 errorcheck 的
        // 「`文件:行号` 前缀」对齐（R3/R4 的匹配前提）。
        write!(f, "{}:{}:{}", self.file, self.line, self.col)
    }
}

/// 一条诊断。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diag {
    /// 位置。
    pub pos: Pos,
    /// 机器可读代号（如 `S1-EVAL-UNSUPPORTED`）。**稳定且可排序** —— 见模块文档第 3 条。
    pub code: String,
    /// 人类可读描述。
    pub message: String,
}

impl Diag {
    /// 构造一条诊断。
    pub fn new(pos: Pos, code: &str, message: &str) -> Self {
        Self {
            pos,
            code: code.to_string(),
            message: message.to_string(),
        }
    }
}

impl fmt::Display for Diag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}: {}", self.pos, self.code, self.message)
    }
}

/// 诊断集合：收集 + 按 C1 规则排序。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiagBag {
    items: Vec<Diag>,
}

impl DiagBag {
    /// 空集合。
    pub fn new() -> Self {
        Self::default()
    }

    /// 追加一条。
    pub fn push(&mut self, d: Diag) {
        self.items.push(d);
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 按 C1 规则排序后的切片：**消费自身**（避免调用方拿到未排序的视图）。
    ///
    /// 消费是刻意的：若返回 `&[Diag]`，调用方可能拿它去渲染而**忘记先排序** ——
    /// 那就绕过了本模块存在的唯一理由。
    pub fn into_sorted(mut self) -> Vec<Diag> {
        self.items
            .sort_by(|a, b| a.pos.cmp(&b.pos).then_with(|| a.code.cmp(&b.code)));
        self.items
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(file: &str, line: u32, col: u32) -> Pos {
        Pos::new(file, line, col)
    }

    #[test]
    fn 位置按_文件_行_列_排序() {
        let mut bag = DiagBag::new();
        bag.push(Diag::new(pos("b.go", 1, 1), "X", "第二文件"));
        bag.push(Diag::new(pos("a.go", 9, 1), "X", "第一文件后段"));
        bag.push(Diag::new(pos("a.go", 2, 5), "X", "第一文件前段"));
        bag.push(Diag::new(pos("a.go", 2, 1), "X", "同行更小列"));
        let got: Vec<String> = bag
            .into_sorted()
            .iter()
            .map(|d| d.message.clone())
            .collect();
        assert_eq!(
            got,
            vec!["同行更小列", "第一文件前段", "第一文件后段", "第二文件"],
            "排序必须是 文件→行→列，且与插入顺序无关"
        );
    }

    #[test]
    fn 同位置的诊断按_code_排序_保证输出唯一() {
        // 这是 E6 可复现性的前提：顺序若依赖哈希迭代序，重复执行会随机失败
        let mut bag = DiagBag::new();
        bag.push(Diag::new(pos("a.go", 1, 1), "S1-Z", "z"));
        bag.push(Diag::new(pos("a.go", 1, 1), "S1-A", "a"));
        bag.push(Diag::new(pos("a.go", 1, 1), "S1-M", "m"));
        let got: Vec<String> = bag.into_sorted().iter().map(|d| d.code.clone()).collect();
        assert_eq!(got, vec!["S1-A", "S1-M", "S1-Z"]);
    }

    #[test]
    fn 空集合的排序是空() {
        assert!(DiagBag::new().is_empty());
        assert_eq!(DiagBag::new().len(), 0);
        assert!(DiagBag::new().into_sorted().is_empty());
    }

    #[test]
    fn 位置_display_形如_文件_行_列() {
        assert_eq!(pos("s1_fixture", 3, 7).to_string(), "s1_fixture:3:7");
    }

    #[test]
    fn 诊断_display_含位置_code_与消息() {
        let d = Diag::new(pos("a.go", 1, 2), "S1-EVAL", "子集外形状");
        assert_eq!(d.to_string(), "a.go:1:2: S1-EVAL: 子集外形状");
    }
}
