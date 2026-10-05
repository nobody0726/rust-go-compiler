#!/usr/bin/env python3
"""把 E4 门禁证据登记进 M0-manifest.json 的 gate.E4。

**为什么用脚本而不是手改 JSON**：manifest 是门禁的机器可读事实，手改容易
① 漏改别处（M0-plan 的门禁汇总表要与它**双向**一致，见 check-m0-consistency.py）
② 破坏格式。脚本只做「加一个键」，其余原样保留。
"""

import json
import sys
from pathlib import Path

MANIFEST = Path("docs/milestones/M0-manifest.json")
REPORT_MD = "rgoc/tests/corpus/T-C-report.md"
REPORT_JSON = "rgoc/tests/corpus/T-C-report.json"

EVIDENCE = {
    "name": "官方语料基线 T-C-01..20 全绿",
    "status": "pass",
    "confirmed_at": "2026-10-05",
    "confirmed_by": "cargo run -p rgoc-driver -- harness run --all（20/20，退出码 0）",
    "denominator": 20,
    "numerator": 20,
    "layer_wall_time_s": 5.8,
    "layer_budget_s": 300,
    "slowest_case_s": 0.031,
    "per_case_budget_s": 60,
    "peak_rss_mib": 15,
    "per_case_rss_budget_mib": 512,
    "rss_exception": "T-C-03 (gc1.go) 预算放宽到 768 MiB（M0-tests.md 7.5），实测 15 MiB",
    "full_corpus_baseline": {
        "note": "非门禁（M0-tests.md 8）：证明分母未被缩小",
        "total_go_files": 356,
        "denominator": 279,
        "excluded": 77,
        "target_filtered": 5,
        "target_filtered_note": "被平台过滤的仍计入分母、不计入分子（03 3.3）",
        "executed": 274,
        "by_mode": {"run": 147, "compile": 12, "errorcheck": 120},
        "excluded_by_u": {
            "U1": 5, "U2": 14, "U3": 1, "U5": 9, "U6": 11, "U7": 31, "U13": 5, "U14": 1,
        },
    },
    "reports": [REPORT_MD, REPORT_JSON],
    "negative_checks": [
        "把 helloworld.out 改成 WRONG => 判 runtime-failure、退出码 1（判定不是恒真）",
        "删掉 mainsig.go 的全部 ERROR 注释 => 判 compiler-failure、退出码 1",
    ],
    "discipline": (
        "M0-tests.md 8 的四条判定纪律均已遵守：① 20/20 全过、无基建失败；"
        "② 白名单 T29 已冻结，无新增跳过；③ 未放宽比较器（T-C-13 是**修 harness** 而非改判定）、"
        "未把跳过记为 pass、未缩小分母（20 与 279 都已核验）；④ 未削减样本数。"
    ),
    "fix_required_to_pass": {
        "case": "T-C-13 initloop.go",
        "symptom": "初判 compiler-failure：跨行正则 a refers to b\\n.*b refers to c\\n.*c refers to a 匹配不上",
        "root_cause": (
            "compare.rs 的正则子集把 \\ 后的字符一律当**字面量**（lit.push(escaped)），"
            "于是 \\n 变成字面字母 n，永远匹配不上换行。"
            "而 Go 的 regexp/syntax 把 \\n \\t \\r 当 Perl 类转义（真正的控制符），"
            "且 splitOutput（testdir_test.go:1205）正是用 \\n 续接 tab 开头的诊断行。"
        ),
        "fix": "按 Go regexp 语义区分两类转义：控制字符（\\n \\t \\r \\f \\v \\a）⇒ 真控制符；元字符（\\. \\* \\| \\\\ 等）⇒ 字面字符",
        "why_fix_not_new_u": (
            "属 M0 范围内的 harness 缺陷（规则 R4 要求忠实还原官方正则语义），"
            "按 T38 的指示「属于范围的修 harness」处理；样本本身完全在 M0 范围内。"
        ),
        "guards": [
            "t38_跨行正则_能匹配_tab_续接成的单条诊断（**刻意去掉 |initialization loop 备选分支** —— 保留会靠不含 \\n 的分支通过，掩盖缺陷）",
            "t38_错误检查_整体通过_initloop（端到端，用真实语料 + 真实 oracle 输出）",
            "t38_反斜杠转义_按_go_regexp_语义解释（含 \\. 不当通配符的反例）",
            "t38_反例_把_n_当字面量_的实现会失败（变异测试）",
        ],
    },
}


def main() -> int:
    with MANIFEST.open(encoding="utf-8") as f:
        m = json.load(f)
    # 交叉核验：报告文件必须在，且内容与要登记的数一致 —— 防止「报告与登记脱节」
    with open(REPORT_JSON, encoding="utf-8") as f:
        rep = json.load(f)
    for key, want in (("denominator", 20), ("numerator", 20)):
        if rep[key] != want:
            print(f"报告的 {key}={rep[key]}，与要登记的 {want} 不符", file=sys.stderr)
            return 1
    if rep["success"] is not True:
        print("报告 success != true，拒绝登记", file=sys.stderr)
        return 1
    if not Path(REPORT_MD).is_file():
        print(f"缺 {REPORT_MD}", file=sys.stderr)
        return 1

    m["gate"]["E4"] = EVIDENCE
    text = json.dumps(m, ensure_ascii=False, indent=2)
    MANIFEST.write_text(text + "\n", encoding="utf-8")
    print(f"已登记 gate.E4：分母 {rep['denominator']}、分子 {rep['numerator']}、"
          f"峰值 RSS {rep['peak_rss_bytes'] // 1024 // 1024} MiB")
    return 0


if __name__ == "__main__":
    sys.exit(main())
