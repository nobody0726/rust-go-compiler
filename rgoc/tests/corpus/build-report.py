#!/usr/bin/env python3
"""T38：给 E4 基线报告补上「全量 279 分母」一节。

**为什么要有这一节**：E4 门禁只看 20 个 T-C 样本，但 `M0-tests.md` §8 的判定
纪律第 3 条明令「不得通过缩小分母来达成门禁」。报告里必须能核验：
① 20 条**一个都没靠排除消失**（分母 = 20）；② 全量枚举的分母仍是 279，
排除项能说清落在 §6 的哪一条 U。
"""

import json
import subprocess
import sys


def main() -> int:
    corpus = subprocess.run(
        ["cargo", "run", "-q", "-p", "xtask", "--", "corpus"],
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    full = json.loads(corpus)
    with open("tests/corpus/T-C-report.json", encoding="utf-8") as f:
        tc = json.load(f)

    # 门禁判据：分母 20、全部 pass —— 不满足就**报错退出**，不产出报告。
    # 这样「报告文件存在」本身就蕴含「E4 通过」，读报告的人不必再核对一遍。
    if tc["denominator"] != 20:
        print(f"分母不是 20：{tc['denominator']}", file=sys.stderr)
        return 1
    if tc["success"] is not True:
        print("20 条未全部通过，拒绝产出报告", file=sys.stderr)
        return 1
    if full["denominator"] != 279:
        print(f"全量分母不是 279：{full['denominator']}", file=sys.stderr)
        return 1

    bt = chr(96)  # 反引号（避开 heredoc 里的 shell 展开）
    L: list[str] = []
    L.append("## 全量语料基线（**非门禁**，`M0-tests.md` §8）")
    L.append("")
    L.append("E4 门禁只看上面 20 个 T-C 样本。下表是顶层 `test/` 全量枚举的基线，")
    L.append("用来证明**分母没有被缩小** —— 排除项必须能说清落在 §6 的哪一条 U。")
    L.append("")
    L.append("| 项 | 值 |")
    L.append("|---|---|")
    # 用 bt 拼反引号是为了避开 heredoc 的 shell 展开；**外层不要再加反引号**，
    # 否则产出 ``` (双反引号) —— Markdown 渲染成字面反引号。
    L.append(f"| 顶层 {bt}*.go{bt} 总数 | {full['total']} |")
    L.append(f"| **M0 分母**（v0 集 ∧ 无排除参数） | **{full['denominator']}** |")
    L.append(f"| 排除项 | {full['excluded']} |")
    L.append(f"| 其中被平台过滤（**仍计入分母**） | {full['target_filtered']} |")
    L.append(f"| 实际执行 | {full['executed']} |")
    L.append("")
    L.append("分母内按模式：")
    L.append("")
    L.append("| 模式 | 条数 |")
    L.append("|---|---|")
    for k in ("run", "compile", "errorcheck"):
        L.append(f"| `{k}` | {full['by_mode'][k]} |")
    total = sum(full["by_mode"].values())
    L.append(f"| **合计** | **{total}** |")
    L.append("")
    if total != full["denominator"]:
        print(f"by_mode 之和 {total} != 分母 {full['denominator']}", file=sys.stderr)
        return 1
    L.append("排除项按 U 归类（`M0-tests.md` §6）：")
    L.append("")
    L.append("| U | 条数 |")
    L.append("|---|---|")
    for k in sorted(full["excluded_by_u"]):
        L.append(f"| **{k}** | {full['excluded_by_u'][k]} |")
    L.append("")

    with open("tests/corpus/T-C-report.md", "a", encoding="utf-8") as f:
        f.write("\n".join(L) + "\n")
    print(f"已补全量基线一节：分母 {full['denominator']}、排除 {full['excluded']} 项")
    return 0


if __name__ == "__main__":
    sys.exit(main())
