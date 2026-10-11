//! 字符级对撞：`SourceReader` 与 oracle 的 `source.nextch` **逐事件**比较。
//!
//! # 为什么在 token 级之外还要这一层
//!
//! `M1-design.md` §8 切面 ① 的判据是「位置必须与 oracle 逐字节相等」。位置是**每个
//! 字节**推出来的，而 token 级 golden 只在词元边界上取样 —— 手写的几十条里未必有一条
//! 恰好落在出错的字节上。
//!
//! 这里改用**密集扫字节空间**：`fixtures/char_corpus.txt`（1177 个字节串，由
//! `go run . --gen-char-corpus` 生成）逐个跑，期望值来自
//! `fixtures/char_golden.txt`（由 `go run . --chars-golden` 生成）。
//!
//! 两份文件都是**committed 的**，两侧读同一份 ⇒ 单一事实来源。
//! 「两侧各维护一份清单 = 必然漂移」在本仓已三次同形（T38/T39/T47）。
//!
//! # 为什么事件流可以直接比、不需要排序
//!
//! "先报诊断、再让出下一个字符"是官方 `nextch` 里 `goto redo` 的直接后果：违规字节
//! **各自占一个列号**。那是语义，不是内部时序（与 token 级相反 —— 那里"诊断何时触发"
//! 属于实现细节，所以两条流要分开排序）。
//!
//! # 一处**已登记**的偏离（`is_known_deviation` + `M1-design.md` §12 R11）
//!
//! 输入**首字节是 0x00** 时我们与 oracle 不同：官方把那个 NUL 当作普通字符送出
//! （`C 1:1 U+0000`），随后由 `next()` 报 `LEX-BAD-CHAR`；我们报 `LEX-INVALID-NUL`
//! 并跳过。**两侧都报错**，差别只在代号与机制。
//!
//! 这不是"容忍差异"，而是**把差异的判据与形状都断言出来**：判据是谓词（不是写死的
//! hex 列表 —— 那会变成盲区），形状是"只差前缀，其后必须逐字节相同"。

use rgoc_lex::{FileId, SourceReader};
use std::path::{Path, PathBuf};

fn tool_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../tools/token-oracle")
}

/// 把一个字节串走一遍字符层，渲染成与 golden **逐字节相同**的文本。
///
/// 格式与 Go 侧 `renderCharEvents` 一一对应：
///
/// ```text
/// D <line>:<col>      <CODE>
/// C <line>:<col>      U+<HEX> w=<n>
/// ```
///
/// 位置字段宽 9 左对齐（与 Go 的 `%-9s` 对齐）。**宽度 `w` 必须进 golden** ——
/// 它是 byte-column 语义的直接证据：一个 3 字节的 CJK 字符会让下一个字符的列 +3。
///
/// ⚠️ 注意**合成**的 `U+FFFD`（非法编码的替身）**从不进入事件流**：官方在
/// `ch == RuneError && chw == 1` 时会 `goto redo` 重读，所以它只作为中间值存在。
/// 事件流里出现的 `U+FFFD` 一定是源码里**真实**的那个（`w=3`）。
fn render(bs: &[u8]) -> String {
    let mut r = SourceReader::new(bs.to_vec(), FileId::from_index(0));
    let mut out = String::new();
    loop {
        // ① 先出诊断：它们是**本次推进**里被跳过的字节各自报的
        for d in r.advance() {
            out.push_str(&format!(
                "D {:<9}{}\n",
                format!("{}:{}", d.pos.line, d.pos.col),
                d.code.as_str()
            ));
        }
        // ② 再出字符；EOF 不进事件流（见 Go 侧 renderCharEvents 的第 3 条说明）
        match r.ch() {
            None => break,
            Some(c) => {
                let p = r.pos();
                out.push_str(&format!(
                    "C {:<9}U+{:04X} w={}\n",
                    format!("{}:{}", p.line, p.col),
                    c as u32,
                    r.chw()
                ));
            }
        }
    }
    out
}

/// 解析 golden：`## <hex>` 开始一个用例，其后到下一个 `##` 之前是期望事件。
fn parse_golden(text: &str) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for line in text.lines() {
        if let Some(hex) = line.strip_prefix("## ") {
            out.push((hex.trim().to_string(), Vec::new()));
        } else if line.starts_with('#') || line.trim().is_empty() {
            continue;
        } else if let Some(last) = out.last_mut() {
            last.1.push(line.to_string());
        }
    }
    out
}

fn bytes_of_hex(s: &str) -> Vec<u8> {
    if s == "-" {
        return Vec::new();
    }
    assert!(s.len().is_multiple_of(2), "语料行不是偶数长度 hex：{s:?}");
    (0..s.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&s[i..i + 2], 16)
                .unwrap_or_else(|e| panic!("语料里有非法 hex {s:?}：{e}"))
        })
        .collect()
}

fn load(name: &str) -> String {
    let p = tool_dir().join("fixtures").join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("读不到 {}：{e}", p.display()))
}

/// 已知偏离的**判据** —— 输入首字节是 NUL。
///
/// # 机制（`M1-design.md` §12 R11）
///
/// 官方 `source.nextch` 的 NUL 检查**只写在 ASCII 快路径里**：
///
/// ```go
/// if s.ch = rune(s.buf[s.r]); s.ch < sentinel {   // ← 快路径
///     ...
///     if s.ch == 0 { s.error("invalid NUL character"); goto redo }
///     return
/// }
/// // ↓ 一般路径：DecodeRune —— **没有** NUL 检查
/// s.ch, s.chw = utf8.DecodeRune(s.buf[s.r:s.e])
/// ```
///
/// 而缓冲区是**懒填充**的：`init` 只把 `buf[0]` 设成 sentinel，所以**首次**
/// `nextch` 时 `buf[0]` 还不是数据 ⇒ 它落到一般路径 ⇒ 首字节的 NUL 检查被绕过。
///
/// 后续的填充发生在缓冲区边界上（`nextSize` 从 4 KiB 翻倍），所以受影响的偏移是
/// **0 和每个 refill 边界**。边界位置取决于 `nextSize` 的翻倍时机、
/// 是否有活动段（`s.b` 会让内容下移）、以及 reader 每次返回多少字节 ——
/// **这不是一条能写下来的语义规则，是填充时机的实现产物。**
///
/// 我们放弃了 `sentinel` / `fill`（设计 §5.1 的三处简化），位置直接算出 ⇒ 我们
/// **总是**报 NUL。这更接近那段代码的**意图**，且与"位置不依赖填充时机"一致。
/// 词元级两侧都报错（官方 `LEX-BAD-CHAR`，我们 `LEX-INVALID-NUL`），**差别只在代号**。
///
/// 判据用谓词而不是写死的 hex 列表：本仓已三次踩过「写死的键列表变成盲区」
/// （T38/T39/T47）。谓词覆盖全部 31 个以 `00` 开头的语料用例，且下面还会断言
/// **偏离的形状**（只差前缀），所以它不会变成"这里随便什么都行"的垃圾桶。
fn is_known_deviation(bs: &[u8]) -> bool {
    bs.first() == Some(&0x00)
}

/// 偏离在 golden 侧的样子：那个 NUL 被当作普通字符送出。
const DEVIATION_GOLDEN_PREFIX: &str = "C 1:1      U+0000 w=1\n";
/// 偏离在我们这侧的样子：报一条诊断并跳过它。
const DEVIATION_OURS_PREFIX: &str = "D 1:1      LEX-INVALID-NUL\n";

#[test]
fn 字符层与_oracle_逐事件一致() {
    let corpus = load("char_corpus.txt");
    let golden = parse_golden(&load("char_golden.txt"));

    // ── 语料/golden 的完整性守卫（防空文件变成"永远通过"）──
    let hexes: Vec<String> = corpus
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect();
    assert!(
        hexes.len() >= 1000,
        "语料只有 {} 个用例 —— 太少了，可能是生成器坏了或文件被截断",
        hexes.len()
    );
    assert_eq!(
        hexes.len(),
        golden.len(),
        "语料用例数({}) 与 golden 用例数({}) 不一致 —— 重新生成 golden 即可（go run . --chars-golden）",
        hexes.len(),
        golden.len()
    );

    let mut exact = 0usize;
    let mut deviated = 0usize;
    let mut mismatches: Vec<String> = Vec::new();

    for (hex, want) in &golden {
        let bs = bytes_of_hex(hex);
        let got = render(&bs);
        let want_text: String = want.iter().map(|l| format!("{l}\n")).collect();

        if is_known_deviation(&bs) {
            // 偏离必须是**恰好这个形状**（只差首字节那一条），其余逐字节相同。
            // 少了这条断言，谓词就会变成"这里随便什么都行"的垃圾桶。
            assert!(
                want_text.starts_with(DEVIATION_GOLDEN_PREFIX),
                "【{hex}】命中了偏离判据，但 golden 不是预期形状：\n{}",
                indent(&want_text)
            );
            assert!(
                got.starts_with(DEVIATION_OURS_PREFIX),
                "【{hex}】命中了偏离判据，但我们这侧不是预期形状：\n{}",
                indent(&got)
            );
            assert_eq!(
                &got[DEVIATION_OURS_PREFIX.len()..],
                &want_text[DEVIATION_GOLDEN_PREFIX.len()..],
                "【{hex}】偏离只应发生在**首字节**，其后必须逐字节相同"
            );
            deviated += 1;
            continue;
        }

        if got == want_text {
            exact += 1;
        } else {
            mismatches.push(format!(
                "  【{hex}】期望：\n{}    实际：\n{}",
                indent(&want_text),
                indent(&got)
            ));
        }
    }

    assert!(
        mismatches.is_empty(),
        "字符层与 oracle 不一致的用例共 {} 个（只列前 10 个）：\n{}",
        mismatches.len(),
        mismatches
            .into_iter()
            .take(10)
            .collect::<Vec<_>>()
            .join("\n")
    );
    // 偏离必须是**少数**：若判据写松了（比如变成"首字节是任意字节"），这个断言会红。
    assert!(
        deviated > 0 && deviated * 20 < exact,
        "逐字节一致 {exact} 个 / 偏离 {deviated} 个 —— 偏离占比异常，判据可能写松了"
    );
    println!("  逐字节一致 {exact} 个；已登记偏离 {deviated} 个（首字节 NUL，见 R11）");
}

fn indent(s: &str) -> String {
    s.lines().map(|l| format!("      {l}\n")).collect()
}

// ── 偏离本身也要有守卫 ──────────────────────────────────────────────

#[test]
fn 偏离的判据不吞掉正常用例() {
    // 判据必须**只**作用于首字节 NUL
    assert!(is_known_deviation(&[0x00]));
    assert!(is_known_deviation(b"\x00y"));
    assert!(
        !is_known_deviation(b"x\x00y"),
        "非首字节的 NUL 两侧本来就一致"
    );
    assert!(
        !is_known_deviation(b"a\x00"),
        "非首字节的 NUL 两侧本来就一致"
    );
    assert!(!is_known_deviation(b""), "空输入不该命中");
    assert!(!is_known_deviation(b"\x0a\0"), "换个首字节不该命中");
}

#[test]
fn 首字节_nul_我们报诊断并跳过() {
    // 我们的语义：报一条 LEX-INVALID-NUL，且那个 NUL **不**作为字符送出。
    // 若这里变了，说明 source.rs 的 redo 语义被改了 —— 上面对撞测试的偏离
    // 断言会跟着变红，但那时需要的是**重新裁决**，不是顺手改断言。
    assert_eq!(render(&[0x00]), "D 1:1      LEX-INVALID-NUL\n");
    assert_eq!(
        render(b"\x00y"),
        "D 1:1      LEX-INVALID-NUL\nC 1:2      U+0079 w=1\n"
    );

    // 对照：非首字节的 NUL 行为（与官方一致）
    assert_eq!(
        render(b"x\x00y"),
        "C 1:1      U+0078 w=1\nD 1:2      LEX-INVALID-NUL\nC 1:3      U+0079 w=1\n"
    );
}
