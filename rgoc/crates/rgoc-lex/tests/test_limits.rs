//! T-L17 的验收测试 —— 资源上限（`T-L-63`，判据 **`H`**）。
//!
//! # 为什么是 `H`（全仓唯一一条）
//!
//! 官方 scanner **没有任何长度上限常量**（F4：全树 grep `maxScanTokenSize` 无命中）
//! ——「超限行为」是本仓的**实现限制**（D-M1-4 / `M1-design.md` §7.3），期望值
//! **无 oracle 可对**，由**手写 golden**（`# source: hand-written`）登记。
//! `t63_exceeds.golden` 的内容：`tokens` 节 = 光秃秃一个 `EOF`；`diags` 节 =
//! 一条 `LEX-EXCEEDS-FILE-LIMIT`。格式对齐由其余 38 个 oracle golden 交叉背书；
//! 语义（「一条诊断 + 只含 Eof 的流」）出自 §7.3 原文。
//!
//! # 三条测试的杀点各不相同
//!
//! 1. `超过上限报错且只返回_eof` —— **行为**：诊断 + 流的形状（且不 panic、可重复取）；
//! 2. `检查在读入之前发生` —— **时机**：用「超限 + **不可读**」的文件把
//!    「先 `metadata` 再 `read`」逼出来（先 `read` 会得 io 错误而不是超限诊断）——
//!    这条防的是「读完 10 GB 再检查」（那时已经 OOM，等于没检查）；
//! 3. `上限边界值本身可以通过` —— **边界方向**：`>` 而非 `>=`。
//!
//! ⚠️ 两个测试依赖 **unix 权限语义**（chmod 000 对 root 无效）——本仓首发与唯一
//! 验证平台是容器内 Linux / uid 501（非 root）。不维护其他平台。
//!
//! ⚠️ `上限边界值` 与 `超过上限` **不跑 `next()` 扫内容**：sparse 文件的内容是
//! 64 MiB 的零字节，扫描它会产出 6.7e7 条 NUL 诊断（测试自伤）。它们只验收
//! 「读入 + 检查」这一段。

use rgoc_lex::{DiagCode, Lexer, MessageKind, Mode, SemiOrigin, TokenKind, render};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// 测试用的临时文件（目录带唯一后缀；`Drop` 负责清理）。
struct TempFile {
    path: PathBuf,
}

impl TempFile {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("rgoc-tl17-{tag}-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("建临时目录");
        Self {
            path: dir.join("f.go"),
        }
    }

    /// 造一个大小为 `len` 的**稀疏**文件（`set_len` 只记 inode 元数据，不占实际磁盘）。
    fn sparse(tag: &str, len: u64) -> Self {
        let t = Self::new(tag);
        let f = fs::File::create(&t.path).expect("建临时文件");
        f.set_len(len).expect("set_len（稀疏）");
        t
    }

    /// 造一个小文本文件。
    fn text(tag: &str, content: &[u8]) -> Self {
        let t = Self::new(tag);
        fs::write(&t.path, content).expect("写临时文件");
        t
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        if let Some(d) = self.path.parent() {
            let _ = fs::remove_dir(d);
        }
    }
}

/// 去掉读权限（`chmod 000`）。`metadata` 不需要读权限；`read` 会 `EACCES`。
fn make_unreadable(p: &Path) {
    fs::set_permissions(p, fs::Permissions::from_mode(0o000)).expect("chmod 000");
}

// ── ① 行为：超限 ⇒ 一条诊断 + 只含 Eof 的流（不 panic）────────────────

#[test]
fn 超过上限报错且只返回_eof() {
    let f = TempFile::sparse("exceed", rgoc_lex::MAX_FILE_BYTES + 1);

    let mut lx = Lexer::from_path(f.path(), Mode::NONE).expect(
        "超限是【词法层的拒绝】（Ok + 诊断），不是 io 错误 —— \
         不 panic、不静默截断（§7.3）",
    );

    // 恰好一条诊断，位置在文件起点
    let diags = lx.take_diags().into_sorted();
    assert_eq!(diags.len(), 1, "恰好一条（不多不少）：{diags:?}");
    assert_eq!(diags[0].code, DiagCode::ExceedsFileLimit);
    assert_eq!((diags[0].pos.line, diags[0].pos.col), (1, 1));

    // 流只含 Eof —— 且**可重复取**（稳定，不随调用次数改变）
    let t1 = lx.next();
    assert_eq!(t1.kind, TokenKind::Eof);
    assert_eq!((t1.pos.line, t1.pos.col), (1, 1));
    assert_eq!(lx.next().kind, TokenKind::Eof, "再取一次仍是 Eof");

    // 手写 golden 逐字节对撞（`# source: hand-written`，全仓唯一）
    let toks = [t1];
    assert_eq!(
        render::golden_body(&toks, &diags),
        include_str!("golden/t63_exceeds.golden"),
        "T-L-63 的手写 golden"
    );
}

// ── ② 时机：检查在读入**之前**发生 ─────────────────────────────────────

#[test]
fn 检查在读入之前发生() {
    // 「超限 + 不可读」的文件把「先 metadata 再 read」逼出来：
    //   先 metadata（不看读权限）⇒ 正常报超限；
    //   先 read（会因权限失败）⇒ 变成 io 错误 —— 而那意味着慢路径已经读入过，
    //   换成 10 GB 的文件就是 OOM（「读完再检查 = 没检查」）。
    let f = TempFile::sparse("precheck", rgoc_lex::MAX_FILE_BYTES + 1);
    make_unreadable(f.path());

    let mut lx = Lexer::from_path(f.path(), Mode::NONE).expect(
        "先 metadata 再检查时，不可读不影响超限判定 —— read 根本没发生。\
         这里若 Err，说明实现在读入【之后】才检查",
    );

    let diags = lx.take_diags().into_sorted();
    assert_eq!(diags.len(), 1, "超限诊断必须存在：{diags:?}");
    assert_eq!(diags[0].code, DiagCode::ExceedsFileLimit);
}

// ── ③ 边界：恰等于上限 ⇒ 通过 ────────────────────────────────────────

#[test]
fn 上限值就是_64_mib_本身() {
    // 「恰等上限不超限」的测试**用常量造文件**（随之而变）——它杀「边界方向」
    // （`>` vs `>=`），但杀不掉「常量被改小」。这条独立把绝对值钉住：
    // 改它必须同时改这里，而依据（D-M1-4 / §7.3：语料最大 3.77 MB 的 17 倍余量）
    // 就在注释里。
    assert_eq!(
        rgoc_lex::MAX_FILE_BYTES,
        64 * 1024 * 1024,
        "§7.3：单文件上限 = 64 MiB"
    );
}

#[test]
fn 上限边界值本身可以通过() {
    let f = TempFile::sparse("edge", rgoc_lex::MAX_FILE_BYTES);

    let mut lx = Lexer::from_path(f.path(), Mode::NONE).expect("恰等于上限必须能构造");
    assert!(
        lx.take_diags().is_empty(),
        "恰等于上限不得报超限（边界方向：`>` 而非 `>=`）"
    );
    // 不跑 next()：文件内容是 64 MiB 零字节，扫描会产出海量 NUL 诊断（见文件头注意）。
}

// ── ④ 正常路径：小文件照常读入并解析（与 `new` 等价）─────────────────

#[test]
fn 正常小文件照常读入并解析() {
    let f = TempFile::text("normal", b"x := 1\n");

    let mut lx = Lexer::from_path(f.path(), Mode::NONE).expect("小文件正常读入");
    let mut toks = Vec::new();
    for _ in 0..100 {
        let t = lx.next();
        let last = matches!(t.kind, TokenKind::Eof);
        toks.push(t);
        if last {
            break;
        }
    }
    let diags = lx.take_diags().into_sorted();
    assert!(diags.is_empty(), "正常输入无诊断：{diags:?}");

    assert_eq!(toks.len(), 5, "x := 1 → 分号(换行) → EOF");
    assert_eq!(toks[0].kind, TokenKind::Ident(b"x".to_vec()));
    assert_eq!(toks[3].kind, TokenKind::Semi(SemiOrigin::Newline));
    assert_eq!(toks[4].kind, TokenKind::Eof);

    // 文件名进了文件表（`from_path` 用路径当名字）
    let name = lx.table().name(toks[0].pos.file);
    assert!(name.ends_with("f.go"), "文件表里的名字应来自路径：{name:?}");
}

// ── ⑤ io 错误面：如实上浮，不映射成超限 ───────────────────────────────

#[test]
fn io_错误如实上浮() {
    // ① 不存在 —— NotFound
    let r = Lexer::from_path("/definitely/not/exist/rgoc-tl17.go", Mode::NONE);
    assert!(r.is_err(), "不存在的文件是 io 错误，不是超限");

    // ② 小文件 + 不可读 —— read 失败必须如实返回 Err
    //    （若被吞成「空源」或映射成超限诊断，都会静默产出错误的流）
    let f = TempFile::text("ioerr", b"x := 1\n");
    make_unreadable(f.path());
    let r = Lexer::from_path(f.path(), Mode::NONE);
    assert!(r.is_err(), "读失败必须如实返回 Err");
}

// ── ⑥ 公共接口：`from_path` 的 `mode` 参数生效 ────────────────────────

#[test]
fn from_path_的_mode_生效() {
    // `from_path` 是 driver（T-L18）唯一会用的构造入口 —— 它的 `mode` 参数
    // 若不生效，`lex --mode comments` 会静默退回默认模式（丢整条上报流）。
    let f = TempFile::text("mode", b"// c\nx\n");

    let mut lx = Lexer::from_path(f.path(), Mode::COMMENTS).expect("读入");
    while !matches!(lx.next().kind, TokenKind::Eof) {}

    let msgs = lx.take_messages();
    assert_eq!(msgs.len(), 1, "comments 模式下注释必须上报：{msgs:?}");
    assert_eq!(msgs[0].kind, MessageKind::Comment);
    assert_eq!(msgs[0].text, b"// c");
}
