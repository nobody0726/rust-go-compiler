//! arena 保留面守卫（`T-GC-11`，`C12` §6，M10/Task 27）。
//!
//! # 这条守卫防的是什么
//!
//! M10 把**会进 GC 视野**的东西全部搬进托管堆（对象 / 数组 / 闭包 / 裸字节 /
//! map 槽块）。剩下还走 `rt_arena_alloc` 的必须是**不含堆指针、或者指针有
//! 别的根**的结构 —— 因为 arena 块永不回收、也**不扫描**。
//!
//! 危险动作是「顺手」：新加一个 `rt_arena_alloc` 存一个含指针的结构，程序在
//! `off` 态完全正常（没有 GC），`on` 态阈值不触发时也正常，只有 `stress` 才会
//! 把那个指针指向的对象收掉 —— 而 `stress` 只跑语料，语料里不一定有那个形状。
//!
//! 所以这里把「保留面」变成**可执行的清单**：`rt.c` 里每个 `rt_arena_alloc`
//! 调用点都必须在下表登记，多出来的就红。
//!
//! # 为什么按「源码片段」而不是按行号
//!
//! 行号会随任何无关改动漂移，那样守卫每周都要修一次，最后没人维护。
//! 片段取的是**调用点那一行的关键部分**，改动它本身就该重新审一遍保留面。

use std::path::{Path, PathBuf};

/// 仓库根（`rgoc/crates/rgoc-driver` 往上三层）。
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("仓库根")
        .to_path_buf()
}

/// `C12` §6 的保留面：`(调用点片段, 理由, 里面有没有堆指针)`。
///
/// 「有堆指针」的那几条必须写清**谁替它保活** —— 否则它就是漏扫。
const ARENA_SITES: &[(&str, &str, Option<&str>)] = &[
    (
        "char *nb = rt_arena_alloc(nc, 8);",
        "`write_all` 的捕获缓冲（`g_cap_buf`）：纯字节",
        None,
    ),
    (
        "u8 *tab = (u8 *)rt_arena_alloc((2 + n) * 8, 8);",
        "`rt_itab` 的方法表：desc 指针都指向 `.rodata`，不是堆对象",
        None,
    ),
    (
        "DeferRec *r = rt_arena_alloc(sizeof(DeferRec), 8);",
        "`defer` 记录：**含一个堆指针 `fv`**，由 `mark_defer` 单独当根扫",
        Some("`mark_defer` 逐条 `mark_ptr(r->fv)`"),
    ),
    (
        "char *t = rt_arena_alloc(n + 1, 8);",
        "panic 文本的拷贝：纯字节",
        None,
    ),
    (
        "void *box = rt_arena_alloc(16, 8);",
        "`recover()` 的 string box：**含一个堆指针**（`g_saved_text`）",
        Some("它指向的文本本身也在 arena（永不回收）⇒ 不需要标记"),
    ),
];

#[test]
fn t_gc_11_arena_调用点必须登记() {
    let path = repo_root().join("runtime/native/aarch64-unknown-linux-gnu/rt.c");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("读 {} 失败：{e}", path.display()));

    // 声明与定义不算调用点。
    const DECLS: [&str; 2] = [
        "void *rt_arena_alloc(u64 size, u64 align);",
        "void *rt_arena_alloc(u64 size, u64 align) {",
    ];

    let mut found: Vec<(usize, String)> = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let t = line.trim();
        if !t.contains("rt_arena_alloc(") {
            continue;
        }
        // 注释里提到它不算（本文件与契约说明都会提）。
        if t.starts_with("//") || t.starts_with("/*") || t.starts_with('*') {
            continue;
        }
        if DECLS.contains(&t) {
            continue;
        }
        found.push((i + 1, t.to_string()));
    }

    assert!(
        found.len() >= 5,
        "只扫到 {} 个 arena 调用点（至少 5 个）—— 扫描面坏了",
        found.len()
    );

    // ① 每个调用点都必须登记。
    let mut unregistered: Vec<String> = Vec::new();
    for (ln, line) in &found {
        if !ARENA_SITES.iter().any(|(frag, _, _)| line == frag) {
            unregistered.push(format!("rt.c:{ln}: {line}"));
        }
    }
    assert!(
        unregistered.is_empty(),
        "有未登记的 `rt_arena_alloc` 调用点。arena **不扫描**，所以如果它存的\n\
         结构里有堆指针，那个对象会被 GC 当垃圾收掉。请先确认「不含堆指针」\n\
         或者「有别的根」，再登记进 `ARENA_SITES` 与 `C12` §6：\n  {}",
        unregistered.join("\n  ")
    );

    // ② 反向：表里登记的每一条都必须还在源码里（防止「登记了但删了」）。
    let mut missing: Vec<&str> = Vec::new();
    for (frag, _, _) in ARENA_SITES {
        if !found.iter().any(|(_, line)| line == frag) {
            missing.push(frag);
        }
    }
    assert!(
        missing.is_empty(),
        "`ARENA_SITES` 里这些条目在 rt.c 里找不到（改过写法？）：\n  {}",
        missing.join("\n  ")
    );

    // ③ 含堆指针的那几条必须写清「谁替它保活」。
    for (frag, why, keeper) in ARENA_SITES {
        if frag.contains("DeferRec") || frag.contains("box") {
            assert!(
                keeper.is_some(),
                "`{frag}`（{why}）里有堆指针，必须在表里写清谁替它保活"
            );
        }
    }
}
