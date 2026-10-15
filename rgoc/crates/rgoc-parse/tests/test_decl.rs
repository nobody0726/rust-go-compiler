//! `T-A-21` —— `ImportDecl` 全谱（`AST-04` / `AST-02`）与 `appendGroup` 的分组语义。
//!
//! 这里全部走 **`parse_file`**（官方唯一的公开入口），不看内部函数 ——
//! 分组、别名、缺路径这些行为的可观察面都在产出的树上与诊断流里。

use rgoc_ast::{Decl, ImportDecl, LitKind};
use rgoc_parse::{ParseResult, parse_file};

fn parse(src: &str) -> ParseResult {
    parse_file(src.as_bytes(), "t.go")
}

/// 取 `file` 里的全部 import 声明。
///
/// `file == None` 直接炸 —— 测试要能区分「根本没解析出文件」与
/// 「解析出了但声明不对」，否则后者会被前者的失败掩盖。
fn imports(r: &ParseResult) -> Vec<&ImportDecl> {
    r.file
        .as_ref()
        .unwrap_or_else(|| panic!("应产出 AstFile，诊断：{:?}", r.diags))
        .decls
        .iter()
        .filter_map(|d| match d {
            Decl::Import(i) => Some(i),
            _ => None,
        })
        .collect()
}

fn path_value(i: &ImportDecl) -> Option<&str> {
    i.path.as_ref().map(|p| p.value.as_str())
}

fn alias(i: &ImportDecl) -> Option<&str> {
    i.name.as_ref().map(|n| n.value.as_str())
}

#[test]
fn t_a_21_单行_import() {
    let r = parse("package p\nimport \"fmt\"\n");
    assert_eq!(r.errors, 0, "干净输入不该有诊断：{:?}", r.diags);

    let l = imports(&r);
    assert_eq!(l.len(), 1);
    assert!(l[0].group.is_none(), "单行 import 不属于任何分组");
    assert!(l[0].name.is_none(), "无别名");
    assert_eq!(path_value(l[0]), Some("\"fmt\""), "存**原文**（含引号）");
}

/// 分组里的**每条**声明各是一个 `ImportDecl`（不是一个节点装多条）——
/// 这正是 `appendGroup` 里 `list = append(list, x)` 每轮调一次 `f` 的直接后果。
///
/// 另一半是**共享**：同组声明必须指向**同一个** `Group`（官方是指针相等，
/// 我们是值相等，见 `rgoc_ast::aux::Group` 的文档）。
#[test]
fn t_a_21_分组_import_共享同一个_group() {
    let r = parse("package p\nimport (\n\"fmt\"\n\"os\"\n)\n");
    assert_eq!(r.errors, 0, "{:?}", r.diags);

    let l = imports(&r);
    assert_eq!(l.len(), 2, "两条声明 ⇒ 两个 ImportDecl");
    assert!(l[0].group.is_some(), "分组里的声明一定带 Group");
    assert_eq!(l[0].group, l[1].group, "同组必须共享同一个 Group 值");
    assert_eq!(path_value(l[0]), Some("\"fmt\""));
    assert_eq!(path_value(l[1]), Some("\"os\""));
}

/// **反向**半边：两个不同的组必须**不相等**。
///
/// 只断言「同组相等」是抓不住问题的 —— 若 `Group` 的判别退化成恒真
/// （比如它变成了零字段），"同组"这个信息就没了，而前向断言照样绿。
#[test]
fn t_a_21_两组不同则_group_不同() {
    let r = parse("package p\nimport (\n\"a\"\n)\nimport (\n\"b\"\n)\n");
    assert_eq!(r.errors, 0, "{:?}", r.diags);

    let l = imports(&r);
    assert_eq!(l.len(), 2);
    assert!(l[0].group.is_some() && l[1].group.is_some());
    assert_ne!(l[0].group, l[1].group, "两个不同的 `(` ⇒ 不同的组");
}

/// 官方 `importDecl` 的 `switch p.tok` 有三条路径：`_Name` / `_Dot` / 其它。
/// 三种别名写法都要能留下别名，**且一个诊断都不该有**。
#[test]
fn t_a_21_别名与点与下划线_import() {
    for (src, want, what) in [
        ("import f \"fmt\"", "f", "普通别名"),
        ("import . \"fmt\"", ".", "点 import"),
        ("import _ \"fmt\"", "_", "下划线 import"),
    ] {
        let r = parse(&format!("package p\n{src}\n"));
        assert_eq!(r.errors, 0, "{what} 不该报错：{:?}", r.diags);
        let l = imports(&r);
        assert_eq!(l.len(), 1, "{what}");
        assert_eq!(alias(l[0]), Some(want), "{what}：别名必须留下");
        assert_eq!(path_value(l[0]), Some("\"fmt\""), "{what}");
    }
}

/// `import x`（缺路径）—— 官方 `:571-575`：报一条 `missing import path`、
/// `advance(_Semi, _Rparen)`，然后**照常返回这个 `ImportDecl`**。
///
/// 这条同时钉住 `switch p.tok` 的**第一臂**：`x` 是 `_Name` ⇒ 被当作**别名**吃掉，
/// 而不是"既没别名也没路径"。少写那一臂，`name` 就会变成 `None`。
#[test]
fn t_a_21_缺路径报错但节点仍产出且_path_为_none() {
    let r = parse("package p\nimport x\n");
    assert_eq!(r.errors, 1, "恰好一条：{:?}", r.diags);

    let l = imports(&r);
    assert_eq!(l.len(), 1, "官方**照常返回**这个 ImportDecl");
    assert!(l[0].path.is_none(), "`nil means no path`（`nodes.go:68`）");
    assert_eq!(
        alias(l[0]),
        Some("x"),
        "`_Name` 臂把它当别名吃掉了（官方 `switch` 的第一臂）"
    );
}

/// 路径不是字符串字面量：官方 `:576-579` 报一条并把 **`Path.Bad` 置 true**。
///
/// ⚠️ 「置 `Bad`」这一步单独存在是有意义的：`Bad` 是**下游**（noder）判断
/// "这个字面量可信吗"的依据，不是给 dump 看的。漏掉它，节点看起来是"好"的。
#[test]
fn t_a_21_非字符串路径报错并置_bad() {
    let r = parse("package p\nimport 1\n");
    assert_eq!(r.errors, 1, "{:?}", r.diags);

    let l = imports(&r);
    assert_eq!(l.len(), 1);
    let p = l[0]
        .path
        .as_ref()
        .expect("`1` 是字面量 ⇒ Path 非 nil（nil 只留给「没有字面量」）");
    assert_ne!(p.kind, LitKind::String, "它不是字符串字面量");
    assert!(p.bad, "官方把 `Path.Bad` **置 true**");
}

/// 空分组 `import ()`：`list` 立刻见到 `)` ⇒ 零条声明、零诊断。
#[test]
fn t_a_21_空分组不产出声明() {
    let r = parse("package p\nimport ()\n");
    assert_eq!(r.errors, 0, "{:?}", r.diags);
    assert!(imports(&r).is_empty());
}

/// 分组里缺分隔符：`list` 报一条并用 `advance(_Rparen,_Rbrack,_Rbrace)` 同步。
///
/// 断言「只留下第一条」是关键 —— 若缺了那条同步，`"b"` 会被当成新元素的开始，
/// 于是**悄悄多出一条**（而 `errors` 也会多，但两条错与一条错的区别在于
/// **树形**，只看诊断数是分不清的）。
#[test]
fn t_a_21_分组里缺分隔符报错并恢复() {
    let r = parse("package p\nimport (\n\"a\" \"b\"\n)\n");
    assert_eq!(r.errors, 1, "{:?}", r.diags);
    let l = imports(&r);
    assert_eq!(l.len(), 1, "同步到 `)` ⇒ 只留下第一条，got {l:?}");
    assert_eq!(path_value(l[0]), Some("\"a\""));
}
