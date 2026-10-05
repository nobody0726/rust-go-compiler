//! T37 的验收测试（xtask 部分）：语料枚举、报告落盘、manifest environment 生成。
//!
//! # 为什么要给 xtask 写测试
//!
//! xtask 是**构建期工具**，「跑一下看起来对」很容易蒙混过关。但它有两条硬纪律：
//!
//! 1. **manifest 的 `environment` 节必须无占位符**（T16 / E2）—— 生成的 JSON 里
//!    出现 `"unknown"` 或 `"<TBD>"` 就会让 E2 门禁红，而这件事**只有 xtask 生成时
//!    才能测**（手填时靠 `check-m0-consistency.py` 事后抓）。
//! 2. **语料枚举的分母必须仍是 279** —— xtask 重做一遍枚举，若哪天口径漂了，
//!    这里会红。这与 T33 的「分母 == 279」是同一条线的第二道防线。
//!
//! TDD 位置：**RED**。T37 实现前本文件编译不过（`xtask` 尚不存在）。

use std::path::{Path, PathBuf};

use xtask::corpus::{enumerate_report, report_to_json};
use xtask::manifest::environment_json;
use xtask::report::write_report;

/// 语料目录。默认按 crate 位置推算，可用 `RGOC_CORPUS_TEST_DIR` 覆盖。
fn corpus_test_dir() -> PathBuf {
    if let Ok(d) = std::env::var("RGOC_CORPUS_TEST_DIR") {
        return PathBuf::from(d);
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../go_source_code/test")
        .to_path_buf()
}

// ══ 1. 语料枚举 ═════════════════════════════════════════════════════════════

#[test]
fn 枚举_分母仍是_279() {
    let dir = corpus_test_dir();
    assert!(dir.is_dir(), "语料目录不存在：{}", dir.display());
    let rep = enumerate_report(&dir).unwrap_or_else(|e| panic!("枚举失败：{e}"));
    assert_eq!(
        rep.denominator, 279,
        "M0 分母被改了 —— 它是 T29 冻结值，改它必须先改 M0-tests.md"
    );
    assert_eq!(rep.total, 356, "顶层 *.go 总数（356 = 279 + 77 排除）");
    assert_eq!(rep.excluded, 77);
}

#[test]
fn 枚举_json_含八类判定与_u_归类() {
    let rep = enumerate_report(&corpus_test_dir()).expect("枚举");
    let v = report_to_json(&rep).expect("序列化");
    assert_eq!(v["total"], 356);
    assert_eq!(v["denominator"], 279);
    assert_eq!(v["excluded"], 77);
    // U 归类：排除项必须能说清落在哪一条（`M0-tests.md` §6）
    let by_u = v["excluded_by_u"]
        .as_object()
        .expect("excluded_by_u 应是对象");
    assert!(!by_u.is_empty(), "排除项必须有 U 归类，不能只有一个总数");
    for key in by_u.keys() {
        assert!(
            key.starts_with('U'),
            "U 归类的键应是 U 编号，实际 {key:?} —— 「不支持」而不给编号等于把判断责任推给读者"
        );
    }
    // 分母内只有三种 v0 模式，且键名就是动作名（run / compile / errorcheck）
    let by_mode = v["by_mode"].as_object().expect("by_mode 应是对象");
    let mut keys: Vec<&String> = by_mode.keys().collect();
    keys.sort();
    assert_eq!(
        keys,
        vec!["compile", "errorcheck", "run"],
        "分母内只有 v0 三模式"
    );
    // ⚠️ **别把这里的数字当成「20 样本里各模式的个数」**（那是 8/4/8）。
    // 279 是**全量**分母，run 层远多于 20 个样本里的 8 个 —— 写死 8 会让
    // 「全量枚举」与「20 样本」两件事被混为一谈。
    for (k, n) in by_mode {
        assert!(n.as_u64().unwrap_or(0) > 0, "by_mode[{k}] 不该为 0");
    }
    let sum: u64 = by_mode.values().filter_map(|n| n.as_u64()).sum();
    assert_eq!(
        sum, 279,
        "by_mode 三项之和必须等于分母 279，否则有模式被漏进分母"
    );
}

// ══ 2. 报告落盘 ═════════════════════════════════════════════════════════════

#[test]
fn 报告_写到磁盘且是合法_json() {
    // xtask 不自己造报告，而是**转发** rgoc-driver 的执行 + 渲染 ——
    // 两处各写一份判定逻辑就会漂。这里只验「落盘 + 可解析」。
    let out = std::env::temp_dir().join(format!("xtask-report-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&out);
    let text = write_report(&out).unwrap_or_else(|e| panic!("生成报告失败：{e}"));
    assert!(!text.trim().is_empty(), "返回的文本不应为空");

    let raw = std::fs::read_to_string(&out).expect("报告文件应存在");
    let v: serde_json::Value = serde_json::from_str(&raw).expect("报告应是合法 JSON");
    assert_eq!(v["denominator"], 20, "E4 的分母是 20 个样本");
    assert_eq!(v["cases"].as_array().expect("cases").len(), 20);
    for key in ["denominator", "numerator", "success", "by_verdict", "cases"] {
        assert!(!v[key].is_null(), "报告缺字段 {key}");
    }

    std::fs::remove_file(&out).ok();
}

// ══ 3. manifest environment 生成 ════════════════════════════════════════════

#[test]
fn manifest_environment_无占位符且含关键节() {
    // ★ 这就是 T16 / E2 的前置检查搬进单测：xtask 生成的东西**当场**就要合格，
    // 而不是等 check-m0-consistency.py 事后从文件里抓。
    let v = environment_json();
    for key in [
        "host",
        "image",
        "container",
        "toolchain",
        "target",
        "volumes",
        "debug",
    ] {
        assert!(!v[key].is_null(), "environment 缺节 {key}");
    }
    // T16 的判据：整值等于占位词、含 `<`、空串、0、null 都算空缺。
    // ⚠️ 整值比较，不做子串匹配 —— `target.triple` 的合法字面量
    // `aarch64-unknown-linux-gnu` 本身含 "unknown"，子串匹配会误判。
    const PLACEHOLDERS: [&str; 6] = ["unknown", "n/a", "na", "todo", "tbd", "fixme"];
    fn walk(o: &serde_json::Value, p: &str, out: &mut Vec<String>) {
        match o {
            serde_json::Value::Object(m) => {
                for (k, v) in m {
                    walk(v, &format!("{p}.{k}"), out);
                }
            }
            serde_json::Value::Array(a) => {
                for (i, v) in a.iter().enumerate() {
                    walk(v, &format!("{p}[{i}]"), out);
                }
            }
            _ => {
                let bad = match o {
                    serde_json::Value::Null => true,
                    serde_json::Value::Number(n) => n.as_u64() == Some(0) || n.as_i64() == Some(0),
                    serde_json::Value::String(s) => {
                        s.is_empty()
                            || s.contains('<')
                            || PLACEHOLDERS.contains(&s.trim().to_lowercase().as_str())
                    }
                    _ => false,
                };
                if bad {
                    out.push(format!("{p} = {o}"));
                }
            }
        }
    }
    let mut bad = Vec::new();
    walk(&v, "environment", &mut bad);
    assert!(
        bad.is_empty(),
        "xtask 生成的 environment 有空缺/占位符字段：{bad:?}\n\
         （E2 门禁 T16 会红；生成器不许产出占位符）"
    );
}

#[test]
fn manifest_environment_版本与_toolchain_一致() {
    // 生成值必须与**实测**一致，而不是抄 manifest 里的旧值。
    // 这里只做交叉：target.triple 必须是 aarch64（首发平台 D-M0-1），
    // go 版本必须是 1.27.1（oracle 硬约束）。
    let v = environment_json();
    assert_eq!(v["target"]["triple"], "aarch64-unknown-linux-gnu");
    assert_eq!(v["target"]["object_format"], "ELF");
    let go = v["toolchain"]["go"]["version"]
        .as_str()
        .expect("go.version 应是字符串");
    assert!(
        go.contains("go1.27.1"),
        "oracle 必须是 go1.27.1（D-M0-1 硬约束），实际 {go}"
    );
    assert!(
        !v["toolchain"]["go"]["sha256"]
            .as_str()
            .unwrap_or("")
            .is_empty()
    );
}
