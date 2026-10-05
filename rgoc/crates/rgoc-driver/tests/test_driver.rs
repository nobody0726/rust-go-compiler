//! T37 的验收测试：`rgoc-driver` 的 CLI 契约、20 样本冻结表、报告渲染。
//!
//! 三组断言，按「越靠前越致命」排：
//!
//! 1. **CLI 契约** —— `harness` 是 M0 唯一的子命令；**未实现的子命令必须报错而不是
//!    悄悄接受**（T37 明确「不预留」）。「预留了」和「忘了实现」在 CLI 表现上一样，
//!    但一个是好设计，一个是 bug。
//! 2. **20 样本表** —— 与 `M0-tests.md` §4.2–4.4 的冻结表格**逐条对账**。这是本仓
//!    一直在用的交叉校验口径（对照 T33 的「分母 == 279」）：清单与文档漂移时这里会红。
//! 3. **报告渲染** —— 分子/分母/八类分布必须齐全，且 `harness run` 的退出码
//!    由「有没有非 pass」决定（E4 门禁要靠它）。
//!
//! TDD 位置：**RED**。T37 实现前本文件编译不过（`rgoc_driver` 尚不存在）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rgoc_driver::cli::{CliError, Command, HarnessCommand, parse_args};
use rgoc_driver::report::{RunSummary, render_text, summarize, to_json};
use rgoc_driver::samples::{CORPUS_M0_SAMPLES, LoadedSamples, SampleEntry, index_of};
use rgoc_harness::ir::{Layer, Mode, Verdict};
use rgoc_harness::runner::{CaseOutcome, LayerReport};

// ══ 1. CLI 契约 ═════════════════════════════════════════════════════════════

#[test]
fn 解析_harness_三个子命令() {
    for (argv, want) in [
        (vec!["harness", "list"], HarnessCommand::List { all: false }),
        (
            vec!["harness", "list", "--all"],
            HarnessCommand::List { all: true },
        ),
        (
            vec!["harness", "run", "--all"],
            HarnessCommand::Run {
                ids: vec![],
                all: true,
            },
        ),
        (
            vec!["harness", "run", "T-C-01", "T-C-13"],
            HarnessCommand::Run {
                ids: vec!["T-C-01".into(), "T-C-13".into()],
                all: false,
            },
        ),
        (
            vec!["harness", "report"],
            HarnessCommand::Report { out: None },
        ),
        (
            vec!["harness", "report", "--out", "/tmp/r.json"],
            HarnessCommand::Report {
                out: Some(PathBuf::from("/tmp/r.json")),
            },
        ),
    ] {
        let got = parse_args(&argv).unwrap_or_else(|e| panic!("{argv:?} 应可解析：{e}"));
        assert_eq!(got, Command::Harness(want), "argv = {argv:?}");
    }
}

#[test]
fn 未实现的子命令必须报错() {
    // ★ T37 的纪律：**不预留**未实现的子命令。这三个都是「将来会有」的，
    // 现在接受它们等于让 CLI 承诺一个不存在的能力。
    for bad in ["spike", "hir", "compile", "build", "test", "s1", "s2", "s3"] {
        let e = parse_args(&[bad]).unwrap_err();
        assert!(
            matches!(e, CliError::UnknownCommand(_)),
            "{bad} 应报未知命令，实际 {e:?}"
        );
    }
    // 连拼错也一样
    assert!(parse_args(&["harnass"]).is_err());
    assert!(parse_args(&["harness", "lst"]).is_err());
}

#[test]
fn 空参数与缺参数报错() {
    assert!(matches!(parse_args(&[]), Err(CliError::NoCommand)));
    assert!(matches!(
        parse_args(&["harness"]),
        Err(CliError::MissingSubcommand)
    ));
    assert!(matches!(
        parse_args(&["harness", "run"]),
        Err(CliError::NoSelector)
    ));
    // --all 与显式 ID 同时给 ⇒ 矛盾，必须报错而不是二选一
    assert!(matches!(
        parse_args(&["harness", "run", "--all", "T-C-01"]),
        Err(CliError::ConflictingSelector)
    ));
    // --out 缺值
    assert!(parse_args(&["harness", "report", "--out"]).is_err());
    // 未知 flag
    assert!(parse_args(&["harness", "list", "--verbose"]).is_err());
}

#[test]
fn 错误信息要能指出正确用法() {
    let e = parse_args(&["spike"]).unwrap_err();
    let s = e.to_string();
    assert!(s.contains("spike"), "错误信息应复述用户输入：{s}");
    assert!(
        s.contains("harness list"),
        "错误信息应给出 M0 唯一合法的用法：{s}"
    );
}

// ══ 2. 20 样本冻结表 ═══════════════════════════════════════════════════════

/// 语料目录。默认按 crate 位置推算（`<repo>/go_source_code/test`），
/// 可用 `RGOC_CORPUS_TEST_DIR` 覆盖。**找不到就 panic** —— 交叉校验的价值全在这里。
fn corpus_test_dir() -> PathBuf {
    if let Ok(d) = std::env::var("RGOC_CORPUS_TEST_DIR") {
        return PathBuf::from(d);
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../go_source_code/test")
        .to_path_buf()
}

fn frozen_doc() -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../docs/milestones/M0-tests.md");
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("读不到 {}：{e}", p.display()))
}

/// 从 `M0-tests.md` 的表格里抽出 `T-C-nn -> test/xxx.go`。
///
/// 表格行长这样（§4.2）：
/// `| **T-C-01** | \`test/helloworld.go\` | 269 B | ... |`
///
/// ⚠️ 文档里写的是 `test/xxx.go`（相对仓库根），而 [`SampleEntry::path`] 存的是
/// **语料根下的文件名** `xxx.go` —— 两者差一个 `test/` 前缀。这里做**前缀归一**，
/// 否则对账永远在比 `test/helloworld.go` 与 `helloworld.go`，红了也不知道是谁错。
fn doc_pairs(doc: &str) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    for line in doc.lines() {
        if !line.starts_with("| **T-C-") {
            continue;
        }
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        // cells[0] 是空串（行首竖线之后），故 ID 在 1、路径在 2
        let Some(id) = cells.get(1) else { continue };
        let Some(path) = cells.get(2) else { continue };
        let id = id.trim_matches('*');
        let Some(path) = path.strip_prefix('`').and_then(|p| p.strip_suffix('`')) else {
            continue;
        };
        if id.starts_with("T-C-") && path.ends_with(".go") {
            m.insert(id.to_string(), strip_corpus_prefix(path).to_string());
        }
    }
    m
}

/// 去掉文档路径里的 `test/` 前缀，换成语料根下的相对名。
fn strip_corpus_prefix(p: &str) -> &str {
    p.strip_prefix("test/").unwrap_or(p)
}

#[test]
fn 样本表_恰好_20_条且编号连续() {
    assert_eq!(
        CORPUS_M0_SAMPLES.len(),
        20,
        "M0 的 E4 门禁分母冻结为 20（T-C-01..20），不多不少"
    );
    for (i, s) in CORPUS_M0_SAMPLES.iter().enumerate() {
        assert_eq!(s.id, format!("T-C-{:02}", i + 1), "第 {i} 条 ID 应连续");
    }
    // 反例：ID 必须唯一（否则 index_of 会指向最后一个，后面的 report 全错）
    let mut ids: Vec<&str> = CORPUS_M0_SAMPLES.iter().map(|s| s.id).collect();
    let n = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), n, "样本 ID 必须唯一");
}

#[test]
fn 样本表_与冻结文档逐条对账() {
    let doc = frozen_doc();
    let pairs = doc_pairs(&doc);
    assert_eq!(
        pairs.len(),
        20,
        "M0-tests.md §4.2–4.4 应恰好有 20 行 T-C 表格行，实际 {}",
        pairs.len()
    );
    for s in CORPUS_M0_SAMPLES.iter() {
        let want = pairs
            .get(s.id)
            .unwrap_or_else(|| panic!("M0-tests.md 里没有 {} 这一行", s.id));
        assert_eq!(
            s.path, want,
            "{} 的路径与冻结文档不一致 —— 改代码或改文档，二选一，但必须同步",
            s.id
        );
    }
}

#[test]
fn 样本表_分层与冻结文档一致() {
    // §4.2 run 8 个 / §4.3 compile 4 个 / §4.4 errorcheck 8 个
    let by = |m: Mode| CORPUS_M0_SAMPLES.iter().filter(|s| s.mode == m).count();
    assert_eq!(by(Mode::Run), 8, "§4.2 run 层 8 个");
    assert_eq!(by(Mode::Compile), 4, "§4.3 compile 层 4 个");
    assert_eq!(by(Mode::ErrorCheck), 8, "§4.4 errorcheck 层 8 个");
    // 分段边界：T-C-01..08 run、09..12 compile、13..20 errorcheck
    for (i, s) in CORPUS_M0_SAMPLES.iter().enumerate() {
        let want = match i {
            0..=7 => Mode::Run,
            8..=11 => Mode::Compile,
            _ => Mode::ErrorCheck,
        };
        assert_eq!(s.mode, want, "{} 的分层不对", s.id);
    }
}

#[test]
fn 样本表_文件的指令行与表里模式一致() {
    // ★ 这条最容易漂：表里写 run、文件首行却是 `// errorcheck` 时，
    // 报告会把结果归错层，E4 的「20/20」也就没了意义。
    let dir = corpus_test_dir();
    assert!(dir.is_dir(), "语料目录不存在：{}", dir.display());
    for s in CORPUS_M0_SAMPLES.iter() {
        let p = dir.join(s.path);
        let src = std::fs::read_to_string(&p)
            .unwrap_or_else(|e| panic!("读不到样本 {}：{e}", p.display()));
        let got = rgoc_harness::instruction::mode_of(
            &rgoc_harness::instruction::parse_action(&src)
                .unwrap_or_else(|e| panic!("{} 的 action 解析失败：{e}", s.id))
                .action,
        );
        assert_eq!(
            got,
            Some(s.mode),
            "{}（{}）表里写 {:?}，但文件首行是 {:?}",
            s.id,
            s.path,
            s.mode,
            got
        );
    }
}

#[test]
fn index_of_认得上全部编号且不认错() {
    for (i, s) in CORPUS_M0_SAMPLES.iter().enumerate() {
        assert_eq!(index_of(s.id), Some(i), "{}", s.id);
    }
    for bad in ["", "T-C-00", "T-C-21", "t-c-01", "T-C-1", "T-C-01x", "C-01"] {
        assert_eq!(index_of(bad), None, "{bad:?} 不该被认成样本 ID");
    }
}

// ══ 3. 装载：CaseSpec 必须带磁盘路径 ════════════════════════════════════════

#[test]
fn 装载_20_条并带上源码与_out() {
    let dir = corpus_test_dir();
    let loaded =
        LoadedSamples::load(&dir, &CORPUS_M0_SAMPLES).unwrap_or_else(|e| panic!("装载失败：{e}"));
    assert_eq!(loaded.len(), 20);
    let specs = loaded.specs(Layer::Corpus);
    assert_eq!(specs.len(), 20);
    for (i, sp) in specs.iter().enumerate() {
        let e = &CORPUS_M0_SAMPLES[i];
        assert_eq!(sp.name, e.path, "短名应与冻结文档一致");
        assert!(
            sp.path.is_absolute(),
            "CaseSpec 必须带磁盘路径（执行器只读它）"
        );
        assert!(sp.path.is_file(), "{} 的路径不存在", sp.path.display());
        assert!(
            !sp.src.is_empty(),
            "{} 的源码为空 —— 漏读文件会让 R1 解析出空 action",
            e.id
        );
        assert_eq!(sp.layer, Layer::Corpus);
    }
}

#[test]
fn 装载_有_out_的两个样本拿到期望_其余为_none() {
    // §4.2 明确：只有 helloworld.out 与 printbig.out 存在，其余 6 个 run 样本
    // 期望输出为**空**（R2）。「文件不存在」与「内容为空」必须能区分开。
    let dir = corpus_test_dir();
    let loaded = LoadedSamples::load(&dir, &CORPUS_M0_SAMPLES).expect("装载");
    let specs = loaded.specs(Layer::Corpus);
    let by = |f: &str| {
        specs
            .iter()
            .find(|s| s.name == f)
            .unwrap_or_else(|| panic!("样本 {f} 不在表里"))
    };
    assert_eq!(
        by("helloworld.go").expected_out.as_deref(),
        Some("hello, world\n")
    );
    assert_eq!(
        by("printbig.go").expected_out.as_deref(),
        Some("-9223372036854775808\n9223372036854775807\n")
    );
    for f in [
        "closure1.go",
        "gc1.go",
        "closure4.go",
        "func6.go",
        "compos.go",
        "method3.go",
    ] {
        assert!(
            by(f).expected_out.is_none(),
            "{f} 没有 .out，期望应为 None（= 期望输出为空），不该是 Some(空串)"
        );
    }
    // compile / errorcheck 层同样没有 .out
    for f in ["eof.go", "rune.go", "mainsig.go"] {
        assert!(by(f).expected_out.is_none(), "{f} 不该有 .out");
    }
}

#[test]
fn 装载_语料目录缺失时报错而非静默空跑() {
    let r = LoadedSamples::load(Path::new("/nonexistent-corpus"), &CORPUS_M0_SAMPLES);
    assert!(
        r.is_err(),
        "语料缺失必须报错 —— 静默跑 0 条会被误读成 20 条全挂"
    );
}

#[test]
fn 装载_样本文件缺失时报错且指出是哪个编号() {
    let dir = corpus_test_dir();
    let bogus = [SampleEntry {
        id: "T-C-99",
        path: "definitely-not-here.go",
        mode: Mode::Run,
    }];
    let e = LoadedSamples::load(&dir, &bogus).unwrap_err().to_string();
    assert!(e.contains("T-C-99"), "错误信息应指出是哪个 ID：{e}");
    assert!(
        e.contains("definitely-not-here.go"),
        "错误信息应指出缺哪个文件：{e}"
    );
}

// ══ 4. 报告渲染 ═════════════════════════════════════════════════════════════

fn outcome(name: &str, v: Verdict, ms: u64) -> CaseOutcome {
    CaseOutcome {
        name: name.to_string(),
        verdict: v,
        detail: String::new(),
        duration: std::time::Duration::from_millis(ms),
        in_denominator: true,
        child_pid: None,
        peak_rss_bytes: 0,
    }
}

#[test]
fn 报告_八类分布齐全_含零计数() {
    let rep = LayerReport {
        outcomes: vec![
            outcome("a.go", Verdict::Pass, 10),
            outcome("b.go", Verdict::Pass, 20),
            outcome("c.go", Verdict::TargetFiltered, 0),
            outcome("d.go", Verdict::Timeout, 60_000),
        ],
        oracle_error: None,
    };
    let s = summarize(&rep);
    assert_eq!(s.denominator, 4);
    assert_eq!(s.numerator, 2);
    // 20/20 的门禁要能一眼看出「哪一类在挡」，所以零计数也要列出来
    assert_eq!(s.by_verdict.len(), Verdict::ALL.len(), "八类都要出现");
    assert_eq!(s.count(Verdict::Pass), 2);
    assert_eq!(s.count(Verdict::TargetFiltered), 1);
    assert_eq!(s.count(Verdict::Timeout), 1);
    assert_eq!(s.count(Verdict::HarnessFailure), 0);
    let text = render_text(&s);
    for v in Verdict::ALL {
        assert!(
            text.contains(v.as_str()),
            "报告里必须有 {} 这一行（哪怕是 0）",
            v.as_str()
        );
    }
    assert!(text.contains("分子 = 2"), "报告要给出分子：{text}");
    assert!(text.contains("分母 = 4"), "报告要给出分母：{text}");
}

#[test]
fn 报告_全过时退出码为_0_有非_pass时为_1() {
    let all_pass = summarize(&LayerReport {
        outcomes: vec![outcome("a.go", Verdict::Pass, 1)],
        oracle_error: None,
    });
    assert!(all_pass.is_success(), "全 pass ⇒ 退出码 0");
    // 被平台过滤的不算失败（03 §3.3：不计入分子、仍计入分母，但 E4 门禁要求全过，
    // 所以 target-filtered 在「成功」语义上按失败处理 —— 20 个样本一个都不能被过滤）
    for v in [
        Verdict::CompilerFailure,
        Verdict::RuntimeFailure,
        Verdict::HarnessFailure,
        Verdict::TargetFiltered,
        Verdict::Timeout,
        Verdict::ResourceFailure,
        Verdict::ReferenceToolchainFailure,
    ] {
        let s = summarize(&LayerReport {
            outcomes: vec![outcome("a.go", v, 1)],
            oracle_error: None,
        });
        assert!(!s.is_success(), "{v} 必须让退出码非 0");
    }
}

#[test]
fn 报告_oracle_层错误单独标出且必定失败() {
    let s = summarize(&LayerReport {
        outcomes: vec![outcome("a.go", Verdict::ReferenceToolchainFailure, 0)],
        oracle_error: Some("go 版本不符".into()),
    });
    assert!(!s.is_success());
    assert_eq!(s.oracle_error.as_deref(), Some("go 版本不符"));
    let text = render_text(&s);
    assert!(
        text.contains("go 版本不符"),
        "层错误要显式打在报告里：{text}"
    );
}

#[test]
fn 报告_逐用例列出判定与耗时() {
    let mut o = outcome("helloworld.go", Verdict::Pass, 1234);
    o.detail = "耗时 1.2 s".into();
    let s = summarize(&LayerReport {
        outcomes: vec![o, outcome("mainsig.go", Verdict::Pass, 20)],
        oracle_error: None,
    });
    let text = render_text(&s);
    assert!(text.contains("helloworld.go"));
    assert!(text.contains("mainsig.go"));
    assert!(text.contains("1.234s"), "耗时用秒带三位小数：{text}");
}

#[test]
fn 报告_json_含门禁要看的每个数() {
    let s = summarize(&LayerReport {
        outcomes: vec![
            outcome("a.go", Verdict::Pass, 5),
            outcome("b.go", Verdict::HarnessFailure, 7),
        ],
        oracle_error: None,
    });
    let v = to_json(&s);
    assert_eq!(v["denominator"], 2);
    assert_eq!(v["numerator"], 1);
    assert_eq!(v["by_verdict"]["pass"], 1);
    assert_eq!(v["by_verdict"]["harness-failure"], 1);
    assert_eq!(v["cases"][0]["name"], "a.go");
    assert_eq!(v["cases"][0]["verdict"], "pass");
    assert_eq!(v["cases"][0]["duration_ms"], 5);
    assert_eq!(v["success"], false);
    // 八类都要出现，键用 Verdict::as_str（manifest 与报告共用同一套标识）
    for verdict in Verdict::ALL {
        assert!(
            v["by_verdict"][verdict.as_str()].is_number(),
            "JSON 里缺 {} 的计数",
            verdict.as_str()
        );
    }
}

#[test]
fn 报告_json_可被_xtask_再解析() {
    // to_json 产出的是真 JSON（不是手工拼串）—— 用 serde_json 自己 round-trip 验。
    let s = summarize(&LayerReport {
        outcomes: vec![outcome("a.go", Verdict::Pass, 5)],
        oracle_error: None,
    });
    let text = serde_json::to_string_pretty(&to_json(&s)).expect("序列化");
    let back: serde_json::Value = serde_json::from_str(&text).expect("反序列化");
    assert_eq!(back, to_json(&s));
}

#[test]
fn summarize_分母取_outcomes_长度_不因过滤而变小() {
    // ★ 门禁纪律：被平台过滤的**仍计入分母**。若这里改成「只数 pass」，
    // E4 的分母就会悄悄从 20 变成 18 —— 这正是 §8 禁止的「缩小分母」。
    let s = summarize(&LayerReport {
        outcomes: vec![
            outcome("a.go", Verdict::Pass, 1),
            outcome("b.go", Verdict::TargetFiltered, 0),
            outcome("c.go", Verdict::Timeout, 1),
        ],
        oracle_error: None,
    });
    assert_eq!(s.denominator, 3);
    assert_eq!(s.numerator, 1);
}

/// T38 的报告要求含「耗时与**峰值 RSS**」（`M0-tests.md` §7.5 定了单用例 512 MiB、
/// `T-C-03` 放宽到 768 MiB）。报告里没这个数就看不出预算够不够用。
#[test]
fn 报告_含每条用例的_峰值_rss_与整层_耗时() {
    let mib = |n: u64| n * 1024 * 1024;
    let rep = LayerReport {
        outcomes: vec![
            CaseOutcome {
                peak_rss_bytes: mib(12),
                ..outcome("a.go", Verdict::Pass, 30)
            },
            // gc1.go 是 §7.5 里唯一放宽到 768 MiB 的样本
            CaseOutcome {
                peak_rss_bytes: mib(700),
                ..outcome("gc1.go", Verdict::Pass, 27)
            },
        ],
        oracle_error: None,
    };
    let s = summarize(&rep);
    // 整层峰值 = **最大值**，不是求和（逐条串行执行，峰值不叠加）
    assert_eq!(s.peak_rss_bytes, mib(700));
    assert_eq!(s.total_duration_ms, 57);
    assert_eq!(s.cases[1].peak_rss_bytes, mib(700));

    let text = render_text(&s);
    assert!(text.contains("峰值 RSS"), "报告要有整层峰值：{text}");
    assert!(
        text.contains("700 MiB"),
        "gc1.go 的 700 MiB 应出现在报告里：{text}"
    );

    let v = to_json(&s);
    assert_eq!(v["peak_rss_bytes"], mib(700));
    assert_eq!(v["total_duration_ms"], 57);
    assert_eq!(v["cases"][0]["peak_rss_bytes"], mib(12));
}

/// ★ 变异测试：确认 RSS 与耗时**真的进了报告**，不是被静默丢掉。
/// 若 `summarize` 忘了填 `peak_rss_bytes`，这里会红。
#[test]
fn 报告_rss_字段_不是恒零() {
    let rep = LayerReport {
        outcomes: vec![CaseOutcome {
            peak_rss_bytes: 12345,
            ..outcome("a.go", Verdict::Pass, 7)
        }],
        oracle_error: None,
    };
    let v = to_json(&summarize(&rep));
    assert_eq!(v["cases"][0]["peak_rss_bytes"], 12345, "逐用例 RSS 丢了");
    assert_eq!(v["peak_rss_bytes"], 12345, "整层 RSS 丢了");
}

#[test]
fn run_summary_字段可直接被外部断言() {
    // 这个测试的作用是把 `RunSummary` 的形状固定住 —— 它是 report 的公开契约，
    // xtask 与将来的 M0-report 生成都要读它的字段。
    let s: RunSummary = summarize(&LayerReport::default());
    assert_eq!(s.denominator, 0);
    assert_eq!(s.numerator, 0);
    assert!(s.cases.is_empty());
    assert!(s.oracle_error.is_none());
    assert_eq!(s.by_verdict.len(), Verdict::ALL.len());
}
