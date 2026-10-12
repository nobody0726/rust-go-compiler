//! golden 正文渲染 —— 与 `tools/token-oracle` 的输出格式**逐字节一致**
//! （`M1-tests.md` §5.2 的冻结格式，对齐 `tools/token-oracle/main.go`）。
//!
//! # 为什么这个渲染器在 `rgoc-lex` 而不是 driver
//!
//! 腿乙（`T-L19`）的前提是「adapter 输出与 driver 输出**逐字节可 diff**」——
//! 也就是说这份格式在 Rust 侧只能有**一处**实现。`T-L15` 的语料验收需要它，
//! `T-L18` 的 `lex` 子命令直接复用 —— 放在这里让两处**天然是同一段代码**
//! （执行期裁决 R30：原计划把它拆在 T-L18 里写，但那样 T-L15 要么没判据、
//! 要么写第二份）。
//!
//! # 与 oracle `main.go` 的三条对齐纪律（README §5.5）
//!
//! 1. **只录有效域内的字段** —— `lit` 只对 `_Name`/`_Literal`/`_Semi` 记录；
//!    `op`/`prec` 只对 `_Operator`/`_Star`/`_AssignOp`/`_IncOp` 记录。
//!    ⚠️ 我们的 `(form: Plain, op: Assign/Define/Arrow)` 是**故意比官方多给的语义值**，
//!    它们**不在有效域内**（官方 `_Assign`/`_Define`/`_Arrow` 无 `op`）⇒ 渲染为 `lit=-`
//!    风格的「不输出」（T-L03 的已知缺口，见 `token.rs` 模块文档）。
//! 2. **诊断与词元分两条流、各自有序** —— 诊断请传 `DiagBag::into_sorted()` 的结果。
//! 3. **纯字节级转义** —— 不用 Rust 的 `{:?}`（与 Go 的 `%q` 规则不同），
//!    由 [`quote`] 自实现；非法 UTF-8 因此可以安全穿过（`\xNN`）。
//!
//! # 不含头两行
//!
//! `# golden: <path>` / `# source: oracle` 是 oracle 侧的来源信息；
//! Rust 侧输出它们与否属于 `T-L18`/`T-L19` 的口径（「逐字节 diff」的处理）。
//! 本函数输出**从 `== tokens ==` 开始**的正文。

use crate::diag::Diag;
use crate::message::Message;
use crate::token::{Delim, OpForm, Operator, SemiOrigin, Token, TokenKind};

/// 渲染 `== tokens ==` 与 `== diags ==` 两节。
///
/// 对齐 `main.go` 的：`fmt.Fprintf(out, "  %-7s%-9slit=%s%s\n", pos, name, lit, extra)`。
pub fn golden_body(tokens: &[Token], diags: &[Diag]) -> String {
    let mut out = String::new();

    out.push_str("== tokens ==\n");
    for t in tokens {
        out.push_str(&format!(
            "  {:<7}{:<9}lit={}{}\n",
            pos(t.pos.line, t.pos.col),
            token_name(&t.kind),
            lit_field(&t.kind),
            extra_fields(&t.kind),
        ));
    }

    out.push_str("== diags ==\n");
    for d in diags {
        out.push_str(&format!(
            "  {:<7}{}\n",
            pos(d.pos.line, d.pos.col),
            d.code.as_str()
        ));
    }

    out
}

/// 渲染**三节**：两节 + 可选的 `== messages ==`（`T-L18` 立）。
///
/// - `messages = None` ⇒ 与 [`golden_body`] **逐字节相同**（默认模式，无第三节）；
/// - `messages = Some` ⇒ 追加第三节 —— 对齐 oracle 的 `scanMode != 0` 分支：
///   **节总在**，哪怕一条消息都没有（与 oracle 的 `if scanMode != 0` 同构，
///   不要"顺手"改成"非空才输出"）。
///
/// # 排序由本函数负责
///
/// oracle 在渲染前按 `(line, col, text)`（**字节序**）排序（`main.go` 的
/// `sort.SliceStable`）—— 这是**格式的一部分**。调用方拿到的 `take_messages()`
/// 是**发生顺序**，不要在外面排（两处排序 ⇒ 迟早漂移）。
/// `sort_by` 是稳定排序：同位置同文本的相对次序保持。
pub fn golden_body_with_messages(
    tokens: &[Token],
    diags: &[Diag],
    messages: Option<&[Message]>,
) -> String {
    let mut out = golden_body(tokens, diags);
    if let Some(msgs) = messages {
        let mut sorted: Vec<&Message> = msgs.iter().collect();
        sorted.sort_by(|a, b| {
            (a.pos.line, a.pos.col, &a.text).cmp(&(b.pos.line, b.pos.col, &b.text))
        });
        out.push_str("== messages ==\n");
        for m in sorted {
            out.push_str(&format!(
                "  {:<7}{}\n",
                pos(m.pos.line, m.pos.col),
                quote(&m.text)
            ));
        }
    }
    out
}

fn pos(line: u32, col: u32) -> String {
    format!("{line}:{col}")
}

/// 官方 `token.String()`（`token_string.go`，stringer 生成）的复刻。
///
/// ⚠️ 两处**不细分**是官方的真实行为，别"顺手"改成语义名：
/// - `_IncOp` → `"opop"`（四个字母；`++` 与 `--` 都显示 `opop`，靠 `op=` 字段区分）
/// - `_Operator` → `"op"`（`<` 与 `>>` 都是 `op`，靠 `op=` 字段区分）
///
/// 少数几个却有**专门拼写**：`_Star` → `"*"`（唯一不叫 `op` 的运算符族）、
/// `_Assign` → `"="`、`_Define` → `":="`、`_Arrow` → `"<-"`、`_Semi` → `";"`。
fn token_name(k: &TokenKind) -> &'static str {
    match k {
        TokenKind::Ident(_) => "name",
        TokenKind::Literal { .. } => "literal",
        TokenKind::Keyword(kw) => kw.as_str(),
        TokenKind::Op { form, op, .. } => match form {
            OpForm::IncDec => "opop",
            OpForm::Assign => "op=",
            OpForm::Plain => match op {
                Operator::Assign => "=",
                Operator::Define => ":=",
                Operator::Arrow => "<-",
                // `*` 在官方是 `_Star`（`case '*'` 特意 `don't goto assignop`），
                // 是唯一有专门拼写的 Plain 运算符
                Operator::Mul => "*",
                _ => "op",
            },
        },
        TokenKind::Delim(d) => match d {
            Delim::LParen => "(",
            Delim::RParen => ")",
            Delim::LBrack => "[",
            Delim::RBrack => "]",
            Delim::LBrace => "{",
            Delim::RBrace => "}",
            Delim::Comma => ",",
            Delim::Colon => ":",
            Delim::Dot => ".",
            Delim::Ellipsis => "...",
        },
        TokenKind::Semi(_) => ";",
        TokenKind::Eof => "EOF",
    }
}

/// `lit=` 字段。**只录有效域**（`_Name`/`_Literal`/`_Semi`），否则 `-`。
fn lit_field(k: &TokenKind) -> String {
    match k {
        TokenKind::Ident(name) => quote(name),
        TokenKind::Literal { text, .. } => quote(text),
        // 官方 `lit` 对 _Semi 的三种取值 —— 这是 `lit` 必须有效的真实理由（F5）
        TokenKind::Semi(origin) => match origin {
            SemiOrigin::Explicit => "\"semicolon\"".to_string(),
            SemiOrigin::Newline => "\"newline\"".to_string(),
            SemiOrigin::Eof => "\"EOF\"".to_string(),
        },
        _ => "-".to_string(),
    }
}

/// 附加字段（`kind=`/`bad=` 或 `op=`/`prec=`）。同样**只在有效域内**输出。
fn extra_fields(k: &TokenKind) -> String {
    match k {
        TokenKind::Literal { lit_kind, bad, .. } => {
            format!(" kind={} bad={}", lit_kind.as_official_code(), bad)
        }
        TokenKind::Op { form, op, prec } => {
            // 有效域：官方 `op` 只在 `_Operator`/`_Star`/`_AssignOp`/`_IncOp` 上有效。
            // 我们的 (Plain, Assign/Define/Arrow) 比官方多存了语义值 —— **不输出**。
            if matches!(form, OpForm::Plain)
                && matches!(op, Operator::Assign | Operator::Define | Operator::Arrow)
            {
                String::new()
            } else {
                format!(" op={} prec={}", op_spelling(*op), prec)
            }
        }
        _ => String::new(),
    }
}

/// 官方 `Operator.String()`（`operator_string.go`）的复刻。
///
/// `Assign`/`Define`/`Arrow` 的条目在有效域外**永不输出**（见 [`extra_fields`]），
/// 这里给的是"最接近的"官方值（`Def` 在官方表里就是 `":"`；`Recv` 是 `"<-"`）。
fn op_spelling(op: Operator) -> &'static str {
    match op {
        Operator::Not => "!",
        Operator::Tilde => "~",
        Operator::Assign => "=",
        Operator::Define => ":",
        Operator::Arrow => "<-",
        Operator::OrOr => "||",
        Operator::AndAnd => "&&",
        Operator::Eql => "==",
        Operator::Neq => "!=",
        Operator::Lss => "<",
        Operator::Leq => "<=",
        Operator::Gtr => ">",
        Operator::Geq => ">=",
        Operator::Add => "+",
        Operator::Sub => "-",
        Operator::Or => "|",
        Operator::Xor => "^",
        Operator::Mul => "*",
        Operator::Div => "/",
        Operator::Rem => "%",
        Operator::And => "&",
        Operator::AndNot => "&^",
        Operator::Shl => "<<",
        Operator::Shr => ">>",
    }
}

/// 包上双引号并按**纯字节级规则**转义（与 `main.go` 的 `quote`/`escape` 逐字节一致）。
///
/// 规则：`\\` `\"` `\n` `\r` `\t` 五个转义；`0x20..=0x7e` 原样；其余 `\xNN`（小写）。
/// **刻意不用 Rust 的 `{:?}`** —— 它对控制字符/非 ASCII 的写法与 Go 不同，
/// 一旦两端的转义不一致，golden 就成了"只有一侧能通过"的东西（README §5.3）。
pub fn quote(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() + 2);
    s.push('"');
    for &c in b {
        match c {
            b'\\' => s.push_str("\\\\"),
            b'"' => s.push_str("\\\""),
            b'\n' => s.push_str("\\n"),
            b'\r' => s.push_str("\\r"),
            b'\t' => s.push_str("\\t"),
            0x20..=0x7e => s.push(char::from(c)),
            _ => s.push_str(&format!("\\x{c:02x}")),
        }
    }
    s.push('"');
    s
}
