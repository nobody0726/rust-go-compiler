//! `iota` 与常量声明组的展开（M3/Task 9，`T-TY-39`，`CON-06/07`）。
//!
//! # 官方把这件事放在哪
//!
//! parser **不做**展开：省略值的 `const` 规格在 AST 里就是 `Values == []`
//! （`T-AP` 侧的实测：`const ( A = iota; B; C )` 的三条 `ConstDecl`，
//! 第一条 `Values: [1 entries]`，后两条 `Values: [0 entries]`）。
//! 展开发生在**类型检查**：官方 `go/types` 的 `constDecl` 拿
//! 「上一条规格的 type/values」补上（`go/types/decl.go`）。
//! 本文件就是那一步 —— 所以它在 `rgoc-const` 而不是 `rgoc-parse`。
//!
//! # 组的身份
//!
//! [`rgoc_ast::ConstDecl::group`] 是 `Option<Group>`，`Group` 的身份是
//! **`(` 的位置**（`aux.rs`：同一文件里 `(` 的位置唯一 ⇒ 与官方的指针相等同义）。
//! 无括号的 `const A = 1` 是 `None` —— 而**每一条无括号的 `const` 自己就是一组**
//! （`const A = 1` 与 `const B = 2` 是两个组，`iota` 各自从 0 起）。
//! 这个「`None` 也开新组」的规则是**必须有**的：官方那边它们是**两条
//! `GenDecl`**，而我们这边都落在同一个 `Decls` 平铺列表里，只有 `group` 能区分。
//!
//! # 空行 / 注释
//!
//! 它们在 AST 里**不留痕**（`ConstDecl` 没有「前面有空行」这一维），
//! 所以「空行不重置 `iota`」在本层**结构上自动成立**：同一条
//! `const (` 组里的全部规格共享一个 `Group` 身份 ⇒ 不可能被空行切开。
//! ⚠️ 但这条**只能**在「真有源码」的地方被验证（空行得先能被读出来）⇒
//! 源码级的用例在 Task 12 的求值器上，本文件只做结构级用例。

use rgoc_ast::{ConstDecl, Expr};

/// 把一列 `const` 规格按组展开成 `(iota 值, 表达式)` 序列。
///
/// # 粒度与「一个表达式对多个名字」
///
/// 输出按**表达式**展开（每条值表达式一个元素），与 `Vec<Expr>` 的展开一致：
/// `const a, b = 1, 2` 给两个元素，**同一个** `iota`。
/// 名字与表达式的配对是调用方的事（两边个数相等时一一对应；不等是
/// `missing value in const declaration` 那类错误，由 Task 12 报）。
///
/// # 省略值
///
/// 省略 ⇒ **复用上一条规格的值表达式**（照官方）。所以 `iota` 的代入必须在
/// 展开**之后**做 —— 表达式的**文本**被复制，`iota` 的值由本函数给出。
/// 组里第一条就省略 ⇒ 没有可复用的东西，**不产出元素**（那是错误，由调用方报）。
///
/// # `_` 占槽位
///
/// `_` 是一条正常的规格（官方 `constDecl` 也照样给它算 `iota`）⇒
/// `const ( _ = iota; K )` 的 `K` 是 `1`。
#[must_use]
pub fn expand_const_group(decls: &[ConstDecl]) -> Vec<(usize, Expr)> {
    expand_const_group_specs(decls)
        .into_iter()
        .flat_map(|(iota, values)| values.into_iter().map(move |v| (iota, v)))
        .collect()
}

/// 同 [`expand_const_group`]，但**按规格分组**返回：一条 `const` 规格一项。
///
/// # 为什么要有它（`Task 24` Ruling 24-6）
///
/// 展平版丢掉了「这条规格有几个名字」—— 而调用方要按名字配对
/// （`const a, b = 1, 2` 的 `a` 拿 `1`、`b` 拿 `2`）。分组版让配对就是
/// `specs[i]` ↔ `decls[i]`，**不必复制**本文件的分组逻辑（复制就会有两份
/// 「什么算新组」的判据，而它们会分叉）。
///
/// 越界的那一支（名字比表达式多）由调用方处理 —— 这里只**如实**返回
/// 每条规格产出的表达式（组首省略 ⇒ 空 `Vec`）。
#[must_use]
pub fn expand_const_group_specs(decls: &[ConstDecl]) -> Vec<(usize, Vec<Expr>)> {
    let mut out = Vec::new();
    // 上一条规格的「组身份」（`None` = 无括号）。第一条之前用 `first` 标出。
    let mut prev_group: Option<Option<rgoc_ast::Group>> = None;
    let mut iota = 0usize;
    // 上一条规格**生效**的值表达式（省略值复用它；换组即清空）
    let mut last_values: Vec<Expr> = Vec::new();

    for d in decls {
        // 开新组的三条判据：① 第一条；② 本条无括号（每条无括号 `const` 自成一组）；
        // ③ 与上一条的括号位置不同。
        let starts_new = match prev_group {
            None => true,
            Some(prev) => d.group.is_none() || prev != d.group,
        };
        if starts_new {
            iota = 0;
            last_values.clear();
        }

        let values = if d.values.is_empty() {
            last_values.clone()
        } else {
            d.values.clone()
        };
        out.push((iota, values.clone()));
        last_values = values;
        // 规格占一个 `iota` 槽位 —— **无论它是否产出了元素**（组首省略也占）。
        iota += 1;
        prev_group = Some(d.group);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rgoc_ast::{BasicLit, Group, LitKind, Name, Op, Operation};
    use rgoc_lex::{FileTable, Pos};

    fn pos(col: u32) -> Pos {
        let mut ft = FileTable::new();
        let file = ft.intern("t.go");
        Pos {
            base: None,
            file,
            line: 1,
            col,
        }
    }

    fn iota_expr() -> Expr {
        Expr::Name(Name {
            value: "iota".to_string(),
            pos: pos(1),
        })
    }

    fn lit(n: i64) -> Expr {
        Expr::BasicLit(BasicLit {
            value: n.to_string(),
            kind: LitKind::Int,
            bad: false,
            pos: pos(1),
        })
    }

    /// `1 << iota`
    fn shl_iota() -> Expr {
        Expr::Binary(Operation {
            op: Op::Shl,
            x: Box::new(lit(1)),
            y: Some(Box::new(iota_expr())),
            pos: pos(1),
        })
    }

    /// 一条规格；`group` 用 `(` 的列号表达身份。
    fn spec(group: Option<u32>, values: Vec<Expr>) -> ConstDecl {
        ConstDecl {
            group: group.map(|c| Group { open: pos(c) }),
            pragma: Default::default(),
            names: vec![Name {
                value: "x".to_string(),
                pos: pos(1),
            }],
            type_: None,
            values,
            pos: pos(1),
        }
    }

    fn iotas(out: &[(usize, Expr)]) -> Vec<usize> {
        out.iter().map(|(i, _)| *i).collect()
    }

    /// `T-TY-39`：`const ( A = iota; B; C )` ⇒ `0, 1, 2`。
    #[test]
    fn iota_组内递增() {
        let decls = vec![
            spec(Some(10), vec![iota_expr()]),
            spec(Some(10), vec![]),
            spec(Some(10), vec![]),
        ];
        let out = expand_const_group(&decls);
        assert_eq!(iotas(&out), vec![0, 1, 2]);
        // 省略 ⇒ **复用上一条的表达式**（`iota` 的文本被复制，值由 iota 给）
        assert!(out.iter().all(|(_, e)| *e == iota_expr()));
    }

    /// `T-TY-39`：`const ( A = 1 << iota; B; C )` ⇒ `1, 2, 4` ——
    /// 表达式复用，`iota` **不**被折进表达式里。
    #[test]
    fn iota_复用表达式而不是复用值() {
        let decls = vec![
            spec(Some(10), vec![shl_iota()]),
            spec(Some(10), vec![]),
            spec(Some(10), vec![]),
        ];
        let out = expand_const_group(&decls);
        assert_eq!(
            iotas(&out),
            vec![0, 1, 2],
            "`1 << iota` 的 iota 分别是 0/1/2"
        );
        assert_eq!(out.len(), 3);
        for (_, e) in &out {
            assert_eq!(*e, shl_iota(), "三条共用同一棵表达式树");
        }
    }

    /// 新的 `const (` 组 ⇒ `iota` 回到 0。
    #[test]
    fn 换组重置_iota() {
        let decls = vec![
            spec(Some(10), vec![iota_expr()]),
            spec(Some(10), vec![]),
            spec(Some(40), vec![iota_expr()]),
            spec(Some(40), vec![]),
        ];
        assert_eq!(iotas(&expand_const_group(&decls)), vec![0, 1, 0, 1]);
    }

    /// **无括号的 `const` 每条自成一组** —— 它们 `group` 都是 `None`，
    /// 只有「`None` 也开新组」这条规则能把它们分开。
    #[test]
    fn 无括号_每条各成一组() {
        let decls = vec![spec(None, vec![iota_expr()]), spec(None, vec![iota_expr()])];
        assert_eq!(
            iotas(&expand_const_group(&decls)),
            vec![0, 0],
            "两条独立的 `const` ⇒ 各自从 0 起"
        );
    }

    /// `_` 占**槽位**：`const ( _ = iota; K )` ⇒ `K` 是 `1`。
    #[test]
    fn 下划线占槽位() {
        let decls = vec![spec(Some(10), vec![iota_expr()]), spec(Some(10), vec![])];
        let out = expand_const_group(&decls);
        assert_eq!(iotas(&out), vec![0, 1]);
    }

    /// 一条规格多个名字/多个值 ⇒ 同一 `iota` 出多个元素。
    #[test]
    fn 一条规格多值() {
        let decls = vec![
            spec(Some(10), vec![lit(1), lit(2)]),
            spec(Some(10), vec![lit(3)]),
        ];
        let out = expand_const_group(&decls);
        assert_eq!(iotas(&out), vec![0, 0, 1]);
        assert_eq!(out[0].1, lit(1));
        assert_eq!(out[1].1, lit(2));
        assert_eq!(out[2].1, lit(3));
    }

    /// 组里第一条就省略 ⇒ 没有可复用的 ⇒ **不产出元素**（但槽位照占）。
    #[test]
    fn 组首省略不产出() {
        let decls = vec![spec(Some(10), vec![]), spec(Some(10), vec![iota_expr()])];
        let out = expand_const_group(&decls);
        assert_eq!(
            iotas(&out),
            vec![1],
            "第一条是错误（无处可复用），第二条 iota == 1"
        );
    }

    /// **跨组不复用**：上一组最后一条的表达式**不得**漏进下一组。
    #[test]
    fn 省略值不跨组复用() {
        let decls = vec![
            spec(Some(10), vec![lit(7)]),
            spec(Some(40), vec![]),
            spec(Some(40), vec![lit(9)]),
        ];
        let out = expand_const_group(&decls);
        assert_eq!(
            iotas(&out),
            vec![0, 1],
            "第二组首条省略 ⇒ 不产出；末条 iota == 1"
        );
        assert_eq!(out[0].1, lit(7));
        assert_eq!(out[1].1, lit(9));
    }

    /// 分组版（`Task 24` 的名字↔值配对入口）：**一规格一项**，且展平后与旧函数**逐项相同**。
    #[test]
    fn 分组版与展平版一致() {
        let mut g = spec(Some(10), vec![lit(1), lit(2)]);
        g.names = vec![
            Name {
                value: "a".to_string(),
                pos: pos(1),
            },
            Name {
                value: "b".to_string(),
                pos: pos(3),
            },
        ];
        let decls = vec![
            g,                        // `a, b = 1, 2` ⇒ 一条规格两个表达式
            spec(Some(10), vec![]),   // 省略 ⇒ 复用上一条（仍是两个）
            spec(None, vec![lit(9)]), // 无括号 ⇒ 新组、iota 归零
        ];
        let specs = expand_const_group_specs(&decls);
        assert_eq!(specs.len(), 3, "一规格一项");
        assert_eq!(specs[0].0, 0);
        assert_eq!(specs[0].1.len(), 2, "`a, b = 1, 2` 产出两个");
        assert_eq!(specs[1].0, 1);
        assert_eq!(specs[1].1.len(), 2, "省略 ⇒ 复用上一条的两个");
        assert_eq!(specs[2].0, 0, "无括号自成一组 ⇒ iota 归零");

        // 展平后必须与旧函数**逐项相同**（旧函数的语义与测试零改动的依据）
        let flat: Vec<(usize, Expr)> = specs
            .clone()
            .into_iter()
            .flat_map(|(i, vs)| vs.into_iter().map(move |v| (i, v)))
            .collect();
        assert_eq!(flat, expand_const_group(&decls));
    }
}
