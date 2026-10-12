#!/usr/bin/env python3
"""M1 阶段一致性自检。

用法：
    python3 scripts/check-m1-consistency.py

职责边界：只做可判定的断言，不修改任何文件。
"""
from __future__ import annotations

import json
import pathlib
import re
import sys

REPO_ROOT = pathlib.Path(__file__).resolve().parent.parent
FAILURES: list[str] = []
TOTAL = 0


def check(label: str, ok: bool, detail: str = "") -> None:
    global TOTAL
    TOTAL += 1
    print(("  \u2713 " if ok else "  \u2717 ") + label + (f"  {detail}" if detail else ""))
    if not ok:
        FAILURES.append(label)


def main() -> int:
    print("M1 阶段一致性自检")
    print("=" * 60)

    # ── 1. M1-manifest.json 基本结构与占位符 ─────────────────────────────────
    print("[1] M1-manifest.json 存在且不含占位符")
    manifest_path = REPO_ROOT / "docs/milestones/M1-manifest.json"
    check("M1-manifest.json 存在", manifest_path.is_file())
    manifest_text = manifest_path.read_text(encoding="utf-8") if manifest_path.is_file() else ""
    manifest = json.loads(manifest_text) if manifest_text else {}
    # 占位符判据与 M0 一致，但 M1-manifest 里允许合法类型参数如 <u8>，
    # 故只把形如 <TODO> / <待填> / <image_digest> 当占位符。
    placeholder = re.compile(r"<[^\s<>]{1,24}>")
    found = sorted(set(placeholder.findall(manifest_text)) - {"<u8>"}) if manifest_text else []
    check("M1-manifest.json 无尖括号占位符", not found, f"命中 {found}" if found else "")
    check("stage == M1", manifest.get("stage") == "M1")
    check("generated_at 存在", bool(manifest.get("generated_at")))

    # ── 2. gate 动态枚举 + 七条门禁齐备 + 全 pass ───────────────────────────
    print("[2] gate 动态枚举与状态")
    gate = manifest.get("gate", {})
    all_gates = {k: v.get("status") for k, v in gate.items()}
    check("gate 至少含 L1–L7", bool(all_gates) and {"L1", "L2", "L3", "L4", "L5", "L6", "L7"}.issubset(set(all_gates)))
    check("gate 全部状态为 pass", bool(all_gates) and set(all_gates.values()) == {"pass"}, str(all_gates))
    required_fields = ("status", "name", "confirmed_at", "confirmed_by", "evidence")
    for gid in sorted(all_gates):
        g = gate.get(gid, {})
        missing = [f for f in required_fields if f not in g]
        check(f"gate.{gid} 字段齐全", not missing, f"缺 {missing}" if missing else "")

    # ── 3. test_ids 与分母 ───────────────────────────────────────────────────
    print("[3] test_ids 与分母")
    test_ids = manifest.get("test_ids", [])
    ids = [t.get("id") for t in test_ids]
    check("test_ids 数量 == 53", len(ids) == 53, f"实际 {len(ids)}")
    check("test_ids 无重复", len(ids) == len(set(ids)), f"重复 {len(ids) - len(set(ids))}")
    check("所有 test_ids 状态为 pass", all(t.get("verdict") == "pass" for t in test_ids))
    check("每个 test_id 都有 gate 与 evidence", all(t.get("gate") and t.get("evidence") for t in test_ids))
    # 与 M1-tests.md 中的 ID 对撞
    m1_tests_text = (REPO_ROOT / "docs/milestones/M1-tests.md").read_text(encoding="utf-8")
    doc_ids = sorted(set(re.findall(r"`(T-L-\d+)`", m1_tests_text)))
    check("manifest 的 ID 集合 == M1-tests.md 的 ID 集合", sorted(ids) == doc_ids,
          f"manifest 多/缺 {set(ids) ^ set(doc_ids)}")

    # ── 4. LEX-01..13 覆盖映射 ───────────────────────────────────────────────
    print("[4] LEX-01..13 覆盖映射")
    cov = gate.get("L1", {}).get("coverage", {})
    check("L1 的 coverage 含 13 个 LEX ID", set(cov) == {f"LEX-{n:02d}" for n in range(1, 14)})
    for lex_id in sorted(cov):
        check(f"{lex_id} 至少映射 1 个 T-L ID", len(cov[lex_id]) >= 1, f"实际 {len(cov[lex_id])}")
    # 反向：coverage 里出现的 ID 都真实存在
    flat = [tid for lst in cov.values() for tid in lst]
    check("coverage 引用的 ID 都在 test_ids", set(flat).issubset(set(ids)),
          f"缺失 {sorted(set(flat) - set(ids))}")

    # ── 5. unsupported 与 U12 lex 半解除 ─────────────────────────────────────
    print("[5] unsupported 清单")
    unsupported = manifest.get("unsupported", [])
    check("unsupported 至少含 U12-lex", any(u.get("id") == "U12-lex" for u in unsupported))
    u12 = next((u for u in unsupported if u.get("id") == "U12-lex"), {})
    if u12:
        check("U12-lex label 为 partially-resolved", u12.get("label") == "partially-resolved")
        check("U12-lex 说明含 M1 lex 已解除", "M1 已完成" in u12.get("why", ""))
    # M0-manifest.json 里的 U12 也同步改写
    m0_manifest = json.loads((REPO_ROOT / "docs/milestones/M0-manifest.json").read_text(encoding="utf-8"))
    m0_u12 = next((u for u in m0_manifest.get("unsupported", []) if u.get("id") == "U12"), {})
    check("M0-manifest.json 的 U12 label 为 partially-resolved", m0_u12.get("label") == "partially-resolved")
    check("M0-manifest.json 的 U12 planned_phase 说明 lex 已解除",
          "M1" in m0_u12.get("planned_phase", "") and "lex" in m0_u12.get("why", ""))

    # ── 6. phase_plan 与任务范围 ─────────────────────────────────────────────
    print("[6] phase_plan M1 收口")
    pp = manifest.get("phase_plan", {}).get("m1", {})
    check("phase_plan.m1.status == done", pp.get("status") == "done")
    done = " ".join(pp.get("done", []))
    check("phase_plan.m1.done 含 T-L01–T-L21 全部任务",
          all(f"T-L{n:02d}" in done for n in range(1, 22)),
          f"done 列了 {len(pp.get('done', []))} 条")
    check("phase_plan.m1.gate_commands 含 9 条命令", len(pp.get("gate_commands", [])) == 9)
    crates = pp.get("crates", [])
    check("phase_plan.m1 登记的 crate 存在", all((REPO_ROOT / "rgoc" / c).is_dir() for c in crates))

    # ── 7. GAP-01 文件与内容 ──────────────────────────────────────────────────
    print("[7] GAP-01 四维归属")
    gap_path = REPO_ROOT / "docs/milestones/GAP-01.md"
    check("GAP-01.md 存在", gap_path.is_file())
    gap_text = gap_path.read_text(encoding="utf-8") if gap_path.is_file() else ""
    rows = re.findall(r"\| LEX-\d+ \|", gap_text)
    check("GAP-01.md 含 13 行 LEX 归属", len(rows) == 13, f"实际 {len(rows)}")
    check("GAP-01.md 声明只填 13/210", "不填满 210" in gap_text or "13 / 210" in gap_text)

    # ── 8. 三件套状态同步 ─────────────────────────────────────────────────────
    print("[8] M1 三件套状态同步")
    design_text = (REPO_ROOT / "docs/milestones/M1-design.md").read_text(encoding="utf-8")
    tests_text = m1_tests_text
    plan_text = (REPO_ROOT / "docs/milestones/M1-plan.md").read_text(encoding="utf-8")
    check("M1-design.md 状态为 已确认", "状态**：**已确认**" in design_text)
    check("M1-tests.md 状态为 已冻结", "已冻结" in tests_text.splitlines()[2])
    check("M1-plan.md 状态为 已完成", "状态**：**已完成**" in plan_text)

    # ── 9. C1 仍是完整级（M1 升级结果不被回退）─────────────────────────────────
    print("[9] C1 契约保持完整级")
    c1_text = (REPO_ROOT / "docs/contracts/C1-source-map.md").read_text(encoding="utf-8")
    c1_head = "\n".join(c1_text.splitlines()[:20]).replace("**", "")
    check("C1 头部写明 契约级别：完整", "契约级别：完整" in c1_head)
    check("C1 头部不再出现 留位", "留位" not in c1_head)

    # ── 10. AGENTS.md 与 README.md 提及 M1-manifest/GAP-01 ─────────────────────
    print("[10] 文档索引同步")
    agents_text = (REPO_ROOT / "AGENTS.md").read_text(encoding="utf-8")
    readme_text = (REPO_ROOT / "docs/README.md").read_text(encoding="utf-8")
    check("AGENTS.md 提及 M1-manifest.json", "M1-manifest.json" in agents_text)
    check("AGENTS.md 提及 GAP-01.md", "GAP-01.md" in agents_text)
    check("docs/README.md 提及 M1-manifest.json", "M1-manifest.json" in readme_text)
    check("docs/README.md 提及 GAP-01.md", "GAP-01" in readme_text)

    print("=" * 60)
    if FAILURES:
        print(f"结果：{len(FAILURES)} 项失败 —— {FAILURES}")
        return 1
    print("结果：全部通过")
    return 0


if __name__ == "__main__":
    sys.exit(main())
