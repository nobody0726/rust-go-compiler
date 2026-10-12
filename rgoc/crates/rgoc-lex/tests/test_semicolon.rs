//! `SemicolonPolicy` 的验收测试（T-L14；LEX-07 的规则表）。
//!
//! # 为什么独立成对象
//!
//! LEX-07 是本阶段**唯一跨调用保留状态**的地方（进 `next()` 取走、出 `next()` 按类别设置）。
//! 埋在 `next()` 里就只能靠端到端 golden 反推；独立出来后这张规则表可以**逐行断言**。
//!
//! # 官方依据：六处设置点（一条不多、一条不少）
//!
//! | # | 官方位置 | 产出什么 | 置位 |
//! |---|---|---|---|
//! | 1 | `setLit()`（`scanner.go:64`） | `_Literal` 一族（**不论 `ok` 与否**） | ✅ |
//! | 2 | `ident()` 尾部（`scanner.go:391`） | `_Name` | ✅ |
//! | 3 | `ident()` 关键字分支（`scanner.go:385`，bitmask） | 仅 `break`/`continue`/`fallthrough`/`return` | 4 ✅ / 21 ❌ |
//! | 4 | `case ')'` `']'` `'}'`（`scanner.go:159/164/169`） | 三个**右**定界符 | ✅ |
//! | 5 | `case '+'` `'-'` 的第二字符分支（`scanner.go:206/216`） | `++` / `--`（`_IncOp`） | ✅ |
//! | 6 | 其余一切 | 运算符 / 左定界符 / `;` / EOF / `(` `[` `{` `,` `:` `.` `...` | ❌（默认保持 false） |
//!
//! # 「逐类别穷举」的落地
//!
//! - 实现侧：对 `TokenKind` 的 match **不许有 `_` 通配** —— 新增变体必须**编译报错**。
//! - 测试侧：遍历 `Keyword::ALL`（25）/ `Delim::ALL`（10）/ `SemiOrigin::ALL`（3）/
//!   `OpForm::ALL × Operator::ALL`（72 个组合）—— 让「新增枚举成员却没进规则表」有地方可查。
//!
//! # 判据 `S`（自证）
//!
//! 本组不需要 oracle：规则表就是官方六处设置点的**直接枚举**，且这六处在官方
//! 是「默认 false + 显式置 true」的单向结构（没有"某些情况置 false"的分支）。
//! 端到端一致性由 `T-L15`/`T-L16` 的 golden 复验。

use rgoc_lex::{Delim, Keyword, LitKind, OpForm, Operator, SemiOrigin, SemicolonPolicy, TokenKind};

/// 给一个词元跑一次「设置 → 取走」，返回 `take()` 的值。
fn 置位(kind: TokenKind) -> bool {
    let mut p = SemicolonPolicy::new();
    p.set_after(&kind);
    p.take()
}

/// 造一个 `Literal` 词元（载荷对规则无影响，取最小样本）。
fn lit(k: LitKind, bad: bool) -> TokenKind {
    TokenKind::Literal {
        text: b"1".to_vec(),
        lit_kind: k,
        bad,
    }
}

/// 造一个 `Op` 词元。
fn op(form: OpForm, operator: Operator) -> TokenKind {
    TokenKind::Op {
        form,
        op: operator,
        prec: operator.prec(),
    }
}

// ── ① 置位的类别 ────────────────────────────────────────────────────

#[test]
fn 规则表_置位的类别() {
    // Ident（官方 `ident()` 尾部；含非法字节的样本 —— 载荷不影响规则）
    assert!(置位(TokenKind::Ident(b"x".to_vec())));
    assert!(置位(TokenKind::Ident(b"x\xff".to_vec())));

    // Literal × 全部 5 个子类（官方 `setLit()`；**bad 与否都置位**）
    for k in LitKind::ALL {
        assert!(置位(lit(k, false)), "{k:?}");
        assert!(置位(lit(k, true)), "{k:?} bad=true");
    }

    // 四个关键字（官方 `scanner.go:385` 的 bitmask）
    for k in [
        Keyword::Break,
        Keyword::Continue,
        Keyword::Fallthrough,
        Keyword::Return,
    ] {
        assert!(置位(TokenKind::Keyword(k)), "{k:?}");
    }

    // Op{form: IncDec} × 全部 Operator（`++`/`--` —— 规则只看 form，不看 op）
    for operator in Operator::ALL {
        assert!(置位(op(OpForm::IncDec, operator)), "++/-- {operator:?}");
    }

    // 三个右定界符
    for d in [Delim::RParen, Delim::RBrack, Delim::RBrace] {
        assert!(置位(TokenKind::Delim(d)), "{d:?}");
    }
}

// ── ② 不置位的类别 ──────────────────────────────────────────────────

#[test]
fn 规则表_不置位的类别() {
    // 其余 21 个关键字 —— 从 `ALL` 过滤，并先钉住基数（防"过滤后为空"的恒真断言）
    let 置位关键字 = [
        Keyword::Break,
        Keyword::Continue,
        Keyword::Fallthrough,
        Keyword::Return,
    ];
    let 其余: Vec<Keyword> = Keyword::ALL
        .into_iter()
        .filter(|k| !置位关键字.contains(k))
        .collect();
    assert_eq!(其余.len(), 21, "25 个关键字 = 4 置位 + 21 不置位");
    for k in 其余 {
        assert!(!置位(TokenKind::Keyword(k)), "{k:?}");
    }

    // Op 的其余两个 form × 全部 Operator（48 个组合全不置位）
    for form in [OpForm::Plain, OpForm::Assign] {
        for operator in Operator::ALL {
            assert!(!置位(op(form, operator)), "{form:?} {operator:?}");
        }
    }

    // Delim 其余 7 个（左定界符 × 3 / `,` / `:` / `.` / `...`）
    for d in [
        Delim::LParen,
        Delim::LBrack,
        Delim::LBrace,
        Delim::Comma,
        Delim::Colon,
        Delim::Dot,
        Delim::Ellipsis,
    ] {
        assert!(!置位(TokenKind::Delim(d)), "{d:?}");
    }

    // Semi × 三个来源（显式的 `;` 不该让下一个换行再补一条）
    for o in SemiOrigin::ALL {
        assert!(!置位(TokenKind::Semi(o)), "{o:?}");
    }

    // Eof
    assert!(!置位(TokenKind::Eof));
}

// ── ③ 取走并复位 ────────────────────────────────────────────────────

#[test]
fn take_取走并复位() {
    let mut p = SemicolonPolicy::new();

    // 初始 false（官方 `init()`：`s.nlsemi = false`）
    assert!(!p.take());

    // 置位 → `take()` 得 true；取走**同时复位** ⇒ 第二次 false
    p.set_after(&TokenKind::Ident(b"x".to_vec()));
    assert!(p.take());
    assert!(!p.take(), "take 必须取走并复位");

    // `set_after` 是**无条件赋值**（官方 `s.nlsemi = ...`），最后一次赢
    p.set_after(&TokenKind::Ident(b"x".to_vec()));
    p.set_after(&TokenKind::Eof);
    assert!(!p.take(), "Eof 覆盖了 Ident 的置位 —— 与官方赋值语义一致");

    // 反向：不置位的不该把已置位的状态"清"成奇怪的东西（覆盖即可，但顺序反过来）
    p.set_after(&TokenKind::Eof);
    p.set_after(&TokenKind::Keyword(Keyword::Return));
    assert!(p.take(), "Return 覆盖了 Eof 的不置位");
}
