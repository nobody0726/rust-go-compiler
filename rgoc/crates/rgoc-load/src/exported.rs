//! 导出标识符判定（M3/Task 26：`SCP-06`，**9 项测试缺口之一，必须自有测试**）。
//!
//! # 判据只有一条
//!
//! 规范（`go_spec.html`）：标识符若以**大写字母**开头就是导出的。
//! 官方的实现（`go/token/token.go:316`）逐字是：
//!
//! ```go
//! func IsExported(name string) bool {
//!     ch, _ := utf8.DecodeRuneInString(name)
//!     return unicode.IsUpper(ch)
//! }
//! ```
//!
//! 三点都要照抄，漏一点就是错的：
//!
//! 1. **只看首字符** —— 不是「名字里有没有大写」（`fĐoo` 不导出）；
//! 2. **按 Unicode 码点**，不是按字节、也不是 ASCII 上界（`École` 导出）；
//! 3. **`unicode.IsUpper` = 类别 `Lu`**，**不是** Rust std 的 `char::is_uppercase`
//!    （那是 derived `Uppercase` = `Lu` ∪ `Other_Uppercase`，实测**多 120 个码点**：
//!    罗马数字 `Ⅰ`、带圈 `Ⓐ`、带框 `🄰` …）⇒ 用 [`rgoc_lex::unicode_tables::is_upper`]，
//!    与 M1 的两张分类表同一个来源纪律（`D-M1-6`：表由 oracle 生成）。
//!
//! ⚠️ 空标识符 `_` **不是**导出标识符（`unicode.IsUpper('_')` 为假）——这与
//! 「`_` 不绑定」是两件事：前者是导出面的事实，后者是作用域的事实。

use rgoc_lex::unicode_tables::is_upper;

/// 名字是否以大写字母（Unicode 类别 `Lu`）开头 —— 官方 `go/token.IsExported`。
///
/// | 输入 | 结果 | 为什么 |
/// |---|---|---|
/// | `"Foo"` | `true` | ASCII 大写 |
/// | `"foo"` / `"_"` / `""` | `false` | 小写 / 空标识符 / 空串 |
/// | `"École"` `"Ł"` `"Ω"` | `true` | 非 ASCII 的 `Lu` |
/// | `"école"` `"ω"` | `false` | 非 ASCII 的小写 |
/// | `"日x"` | `false` | `L*` 但没有大小写之分（`Lo`） |
/// | `"Ⅰx"`（U+2160） | `false` | **`Nl`**：std 的 `is_uppercase` 会误判成 `true` |
pub fn is_exported(name: &str) -> bool {
    // 官方是 `utf8.DecodeRuneInString` 后判那**一个**码点：
    // 空串 ⇒ `RuneError` ⇒ 假；非法字节同理（本仓的名字都是 `String`，不会有后者）。
    name.chars().next().is_some_and(is_upper)
}
