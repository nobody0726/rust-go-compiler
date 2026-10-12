//! 注释与指令的上报通道（`M1-design.md` D-M1-1 / D-M1-8、§5.5）。
//!
//! # 为什么不和诊断共用一个收集器
//!
//! 官方把注释与错误塞进**同一个回调** `errh`，靠「文本以 `/` 开头」区分
//! （`syntax/parser.go:52`）。**注释不是诊断**：混在一个集合里会让 `C1` §4 的
//! 排序规则被无意义地施加到注释上，且消费方要反复做字符串判断。
//!
//! # `MessageKind` 的判据是「上报原因」，不是「文本分类」
//!
//! - [`MessageKind::Comment`]：因为 `comments` 模式**要求上报全部注释**而出现；
//! - [`MessageKind::Directive`]：因为**匹配了指令前缀**（`go:` / `line `）而出现。
//!
//! `comments` 模式下 `//go:build` 也标 `Comment` —— 那个模式里官方**根本不做**
//! 前缀匹配，"它是不是指令"不是 scanner 层的问题（M2 的 parser 才做）。
//! 若按「文本分类」定义，就得发明一套官方没有的复合判定 —— 那是私货。

use crate::pos::Pos;

/// 注释上报模式。官方是 `mode uint` 的两个位（`scanner.go:25-28`）：
///
/// ```text
/// comments   uint = 1 << iota // call handler for all comments
/// directives                  // call handler for directives only
/// ```
///
/// 位语义照抄：**两个位可组合，且 `comments` 优先**（官方 `lineComment` /
/// `fullComment` 都先判 `mode&comments != 0`）。M1 的调用方只用到三个单值。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Mode(u8);

impl Mode {
    /// 不上报任何注释（官方 `mode == 0`）。
    pub const NONE: Mode = Mode(0);
    /// 全部注释都上报（官方 `comments`）。
    pub const COMMENTS: Mode = Mode(1);
    /// 只上报 `//go:` / `//line` 指令（官方 `directives`）。
    pub const DIRECTIVES: Mode = Mode(2);

    /// 是否包含 `other` 的全部位。
    #[must_use]
    pub fn contains(self, other: Mode) -> bool {
        self.0 & other.0 == other.0
    }

    /// 并上 `other` 的位（官方在 parser 里就是按需组合）。
    #[must_use]
    pub fn union(self, other: Mode) -> Mode {
        Mode(self.0 | other.0)
    }
}

/// 一条上报的类别（判据见模块文档：**上报原因**，不是文本分类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MessageKind {
    /// 普通注释（`comments` 模式上报的全部内容）
    Comment,
    /// 指令（`directives` 模式下匹配了 `go:` / `line ` 前缀的那部分）
    Directive,
}

/// 一条注释或指令的上报。
///
/// `text` 是**原始字节切片**（含 `//` 或 `/* */` 定界符；行注释**不含**结尾的
/// `\n`）：
///
/// - `\r` 留在文本里（p44；官方 TestComments 的 `"// want this one\r"` 同款）；
/// - 非法 UTF-8 也留在文本里（p14）⇒ 用 `Vec<u8>`，**不是** `String`。
///
/// **M1 只交出文本，不解析内容**（D-M1-1）：`//line` 的位置重映射、`//go:`
/// 的求值都是 M2+ 的事。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Message {
    /// 注释**起点**的位置（不是终点）—— 官方 `comment()` = `errorAtf(0, …)`，
    /// 报在 token 起点；跨行块注释产 `Semi` 用的也是这个位置（p24）。
    pub pos: Pos,
    /// 上报原因（见 [`MessageKind`]）。
    pub kind: MessageKind,
    /// 原始文本（含定界符）。
    pub text: Vec<u8>,
}
