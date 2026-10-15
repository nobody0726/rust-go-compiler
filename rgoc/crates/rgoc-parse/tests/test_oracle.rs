//! T-AP18 —— oracle 对撞回路（`M2-tests.md` §5.2/§5.3 / §4.2）。
//!
//! 两组验收，全部对着 `rgoc/tests/ast/` 下的 31 个官方 testdata 副本：
//!
//! | 组 | ID | 判据 |
//! |---|---|---|
//! | [`collide_*`] | `T-A-92`/`T-A-93` | rgoc dump 与 oracle golden **逐行 diff 为空** |
//! | [`errcheck_*`] | `T-A-95` | `// ERROR` **双向**匹配：声明的都报出、报出的都声明 |
//!
//! golden 由 `tools/ast-oracle` 生成（`# source: oracle`，见该 README §6），
//! **不得手改**；再生成走 `scripts/check-ast-golden.sh`。
//!
//! `errcheck_*` 是官方 `error_test.go`（`errorcheck` 驱动器）的直译：
//! 行注释 `// ERROR rx` 只对**行号**、块注释 `/* ERROR rx */` 对**下一个词元的
//! 行+列**（`F12`）；`rx` 按 Go regexp **子串**匹配（`F11`）。
//! 声明提取用 rgoc-lex 的 `Mode::COMMENTS` + 词元位置 —— 与官方用官方
//! scanner 的做法同构，**不手撕文本**（字符串字面量里的 `ERROR` 不算）。

use std::collections::BTreeMap;
use std::fmt::Write as _;

use rgoc_ast::dump;
use rgoc_lex::Lexer;
use rgoc_lex::message::Mode;
use rgoc_lex::token::TokenKind;
use rgoc_parse::parse_file;

/// T-A-92：rgoc dump 与 oracle golden 逐行 diff 必须为空。
macro_rules! collision_tests {
    ($( $test:ident : $file:literal ),* $(,)?) => {
        $(
            #[test]
            fn $test() {
                let file: &str = $file;
                let src = include_str!(concat!("../../../tests/ast/", $file));
                let golden = include_str!(concat!("../../../tests/ast/golden/", $file, ".dump"));
                let res = parse_file(src.as_bytes(), file);
                let f = res.file.unwrap_or_else(|| {
                    panic!("{file}: parse_file 返回 None（package 子句都解析不出）")
                });
                let ours = dump(&f);
                assert_no_diff(file, &ours, golden);
            }
        )*
    };
}

/// T-A-95：`// ERROR` 双向匹配（官方 error_test.go 直译）。
macro_rules! errcheck_tests {
    ($( $test:ident : $file:literal ),* $(,)?) => {
        $(
            #[test]
            fn $test() {
                let file: &str = $file;
                let src = include_str!(concat!("../../../tests/ast/", $file));
                let failures = errorcheck(file, src);
                assert!(
                    failures.is_empty(),
                    "{file}: {} 条不匹配:\n{}",
                    failures.len(),
                    failures.join("\n")
                );
            }
        )*
    };
}

collision_tests! {
    t_collide_chans: "chans.go",
    t_collide_fallthrough: "fallthrough.go",
    t_collide_interface: "interface.go",
    t_collide_issue20789: "issue20789.go",
    t_collide_issue23385: "issue23385.go",
    t_collide_issue23434: "issue23434.go",
    t_collide_issue31092: "issue31092.go",
    t_collide_issue43527: "issue43527.go",
    t_collide_issue43674: "issue43674.go",
    t_collide_issue46558: "issue46558.go",
    t_collide_issue47704: "issue47704.go",
    t_collide_issue48382: "issue48382.go",
    t_collide_issue49205: "issue49205.go",
    t_collide_issue49482: "issue49482.go",
    t_collide_issue52391: "issue52391.go",
    t_collide_issue56022: "issue56022.go",
    t_collide_issue60599: "issue60599.go",
    t_collide_issue63835: "issue63835.go",
    t_collide_issue65790: "issue65790.go",
    t_collide_issue68589: "issue68589.go",
    t_collide_issue69506: "issue69506.go",
    t_collide_issue70957: "issue70957.go",
    t_collide_issue70974: "issue70974.go",
    t_collide_linalg: "linalg.go",
    t_collide_map: "map.go",
    t_collide_map2: "map2.go",
    t_collide_sample: "sample.go",
    t_collide_slices: "slices.go",
    t_collide_smoketest: "smoketest.go",
    t_collide_tparams: "tparams.go",
    t_collide_typeset: "typeset.go",
}

errcheck_tests! {
    t_errcheck_chans: "chans.go",
    t_errcheck_fallthrough: "fallthrough.go",
    t_errcheck_interface: "interface.go",
    t_errcheck_issue20789: "issue20789.go",
    t_errcheck_issue23385: "issue23385.go",
    t_errcheck_issue23434: "issue23434.go",
    t_errcheck_issue31092: "issue31092.go",
    t_errcheck_issue43527: "issue43527.go",
    t_errcheck_issue43674: "issue43674.go",
    t_errcheck_issue46558: "issue46558.go",
    t_errcheck_issue47704: "issue47704.go",
    t_errcheck_issue48382: "issue48382.go",
    t_errcheck_issue49205: "issue49205.go",
    t_errcheck_issue49482: "issue49482.go",
    t_errcheck_issue52391: "issue52391.go",
    t_errcheck_issue56022: "issue56022.go",
    t_errcheck_issue60599: "issue60599.go",
    t_errcheck_issue63835: "issue63835.go",
    t_errcheck_issue65790: "issue65790.go",
    t_errcheck_issue68589: "issue68589.go",
    t_errcheck_issue69506: "issue69506.go",
    t_errcheck_issue70957: "issue70957.go",
    t_errcheck_issue70974: "issue70974.go",
    t_errcheck_linalg: "linalg.go",
    t_errcheck_map: "map.go",
    t_errcheck_map2: "map2.go",
    t_errcheck_sample: "sample.go",
    t_errcheck_slices: "slices.go",
    t_errcheck_smoketest: "smoketest.go",
    t_errcheck_tparams: "tparams.go",
    t_errcheck_typeset: "typeset.go",
}

/// 逐行 diff；不一致时 panic 并给出**第一处**差异的上下文
/// （全文 assert_eq! 的输出太长，反而看不见第一案发现场）。
fn assert_no_diff(file: &str, ours: &str, golden: &str) {
    if ours == golden {
        return;
    }
    let a: Vec<&str> = ours.lines().collect();
    let b: Vec<&str> = golden.lines().collect();
    let mut ctx = String::new();
    let n = a.len().max(b.len());
    for i in 0..n {
        let l = a.get(i).copied();
        let r = b.get(i).copied();
        if l != r {
            let _ = write!(ctx, "\n  第一处差异在第 {} 行（0 起）：", i);
            let _ = write!(ctx, "\n    rgoc  : {:?}", l.unwrap_or("<缺行>"));
            let _ = write!(ctx, "\n    oracle: {:?}", r.unwrap_or("<缺行>"));
            let _ = write!(ctx, "\n  rgoc 上下文:");
            for (j, l) in a.iter().enumerate().skip(i.saturating_sub(3)).take(7) {
                let _ = write!(ctx, "\n    {:>5} | {}", j, l);
            }
            let _ = write!(ctx, "\n  oracle 上下文:");
            for (j, l) in b.iter().enumerate().skip(i.saturating_sub(3)).take(7) {
                let _ = write!(ctx, "\n    {:>5} | {}", j, l);
            }
            panic!(
                "{file}: dump 与 oracle golden 不一致（rgoc {} 行 / oracle {} 行）{}",
                a.len(),
                b.len(),
                ctx
            );
        }
    }
    // 行内容全等但行数不同（结尾空行差异）——也按不一致处理。
    panic!(
        "{file}: dump 与 golden 行数不同（{} vs {}）",
        a.len(),
        b.len()
    );
}

/// 把 Go regexp（RE2）的模式转成 Rust regex 接受的等价模式。
///
/// 唯一已知的分歧：Go 把**不构成 `{n,m}` 重复**的裸 `{` 当**字面量**
/// （`unexpected {`）；Rust regex 对裸 `{` 直接报错。对策：转义一切
/// 「后面不是数字」的未转义 `{`。本仓库 31 个文件的 ERROR 模式没用过
/// `{n,m}` 重复（`interface{}` 里的 `{` 全是这条规则要转义的字面量）。
/// 若将来出现真重复（`{2,}`），这里的「后随数字才保留」会放行它。
fn go_rx_to_rust(pat: &str) -> String {
    let mut out = String::with_capacity(pat.len() + 8);
    let mut chars = pat.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            out.push(c);
            if let Some(n) = chars.next() {
                out.push(n);
            }
        } else if c == '{' {
            match chars.peek() {
                Some(d) if d.is_ascii_digit() => out.push('{'), // 保留 {n…} 重复
                _ => out.push_str("\\{"),                       // 裸 `{` → 字面量
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// 官方 `declaredErrors` 的直译：用 rgoc-lex 的 COMMENTS 模式扫注释 +
/// 词元位置，产出 `(行, 列) → rx`。
///
/// 行注释 `// ERROR rx` → `(行, 0)`；块注释 `/* ERROR rx */` → **下一个词元**
/// 的 `(行, 列)`（官方 `error_test.go:85-107`）。
fn declared_errors(src: &str) -> BTreeMap<(u32, u32), String> {
    let mut lex = Lexer::new(src.as_bytes().to_vec(), "errcheck.go");
    lex.set_mode(Mode::COMMENTS);
    let mut declared = BTreeMap::new();
    let mut pattern = String::new();
    loop {
        let tok = lex.next();
        // 注释在词元**之前**被上报（官方同序）—— 先收注释，再把挂起的
        // 块注释 pattern 落到**本词元**的位置上。
        for m in lex.take_messages() {
            let text = String::from_utf8_lossy(&m.text).to_string();
            if let Some(rest) = text.strip_prefix("// ERROR ") {
                // 行注释：只有行号参与匹配（col = 0）。
                declared.insert((m.pos.line(), 0), rest.trim().to_string());
            } else if let Some(rest) = text.strip_prefix("/* ERROR ") {
                // 官方 `msg[9 : len(msg)-2]`：剥掉尾部两个字节 `*/` 再 TrimSpace。
                pattern = rest.strip_suffix("*/").unwrap_or(rest).trim().to_string();
            }
        }
        if !pattern.is_empty() {
            declared.insert((tok.pos.line(), tok.pos.col()), pattern.clone());
            pattern.clear();
        }
        if matches!(tok.kind, TokenKind::Eof) {
            break;
        }
    }
    declared
}

/// 官方 `testSyntaxErrors` 的直译：正向（每条报出的错误都要被某条声明覆盖，
/// 且消息匹配 rx）+ 反向（每条声明都要被报出）。返回失败清单。
fn errorcheck(file: &str, src: &str) -> Vec<String> {
    let mut remaining = declared_errors(src);
    let mut failures = Vec::new();
    let res = parse_file(src.as_bytes(), file);
    for d in res.diags.into_sorted() {
        let line = d.pos.rel_line(&res.bases);
        let col = d.pos.rel_col(&res.bases);
        // 先试精确 (行,列)（块注释声明），再退到 (行,0)（行注释声明）。
        let (key, pattern) = if let Some(p) = remaining.get(&(line, col)) {
            ((line, col), p.clone())
        } else if let Some(p) = remaining.get(&(line, 0)) {
            ((line, 0), p.clone())
        } else {
            failures.push(format!(
                "{file}:{line}:{col}: unexpected error: {}",
                d.message
            ));
            continue;
        };
        // F11：Go `rx.MatchString(e.Msg)` 是无锚点**子串**匹配。
        let rx = match regex::Regex::new(&go_rx_to_rust(&pattern)) {
            Ok(rx) => rx,
            Err(e) => {
                failures.push(format!("{file}:{line}:{col}: bad pattern {pattern:?}: {e}"));
                continue;
            }
        };
        if rx.is_match(&d.message) {
            remaining.remove(&key);
        } else {
            failures.push(format!(
                "{file}:{line}:{col}: {:?} does not match {:?}",
                d.message, pattern
            ));
        }
    }
    for ((l, c), p) in &remaining {
        failures.push(format!("{file}:{l}:{c}: missing error: {p}"));
    }
    failures
}
