#!/usr/bin/env python3
"""M5 候选语料筛选器 —— `T-EX-41` 的「口径可重放」证据。

用法（容器内，仓库根）::

    python3 tools/run-oracle/survey.py                 # 打印候选（相对路径，一行一个）
    python3 tools/run-oracle/survey.py --stats         # 打印分母漏斗
    python3 tools/run-oracle/survey.py --diff FILE     # 与冻结清单比对（不通过则 exit 1）

口径（`M5-design.md` `F1` / `M5-tests.md` §6.1）::

    test/*.go ∪ test/ken/*.go
      → 首 80 行内出现 `// run` 或 `// runoutput` 指令
      → 零 import（`import "x"` 与 `import ( ... )` 都算有 import）
      → 不含并发面：`go <name>(` / `chan` / `select {` / `sync.` / `time.`

**为什么是「首 80 行」而不是「文件头注释块」**：官方 testdir 的指令必须在包子句之前，
但有的文件在指令块与 `package` 之间夹了大段版权注释。取 80 行是**上界**，
比「精确解析注释块」更保守（宁可多认一个指令，也不能漏）—— 而且它可复现。
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

# 指令必须在文件头部（包子句之前）。80 行是上界，见模块 docstring。
HEADER_LINES = 80

RUN_DIRECTIVE = re.compile(r"^//[ \t]*(run|runoutput)([ \t]|$)", re.MULTILINE)
IMPORT_ANY = re.compile(r"^[ \t]*import[ \t]*(\(|\")", re.MULTILINE)
# 并发面：与 `M5-design.md` F1 的筛选口径逐字一致
CONCURRENCY = re.compile(
    r"(^|[^A-Za-z0-9_])go[ \t]+[A-Za-z0-9_]+[ \t]*\("
    r"|\bchan\b"
    r"|\bselect[ \t]*\{"
    r"|\bsync\."
    r"|\btime\."
)

ROOT = Path("go_source_code/test")
SUBDIR = "ken"


def corpus_files() -> list[Path]:
    """顶层 + `ken/` 的 `.go`（不递归其它子目录：那些是多文件包，不在 M5 候选面）。"""
    files = sorted(p for p in ROOT.glob("*.go"))
    files += sorted(p for p in (ROOT / SUBDIR).glob("*.go"))
    return files


def has_run_directive(src: str) -> bool:
    head = "\n".join(src.splitlines()[:HEADER_LINES])
    return RUN_DIRECTIVE.search(head) is not None


def survey() -> tuple[list[Path], dict[str, int]]:
    total = corpus_files()
    stats = {"total": len(total), "run": 0, "no_import": 0, "candidate": 0}
    out: list[Path] = []
    for p in total:
        src = p.read_text(encoding="utf-8", errors="replace")
        if not has_run_directive(src):
            continue
        stats["run"] += 1
        if IMPORT_ANY.search(src):
            continue
        stats["no_import"] += 1
        if CONCURRENCY.search(src):
            continue
        out.append(p)
    stats["candidate"] = len(out)
    return out, stats


def main(argv: list[str]) -> int:
    out, stats = survey()
    if "--stats" in argv:
        print(
            f"total={stats['total']} run={stats['run']} "
            f"no_import={stats['no_import']} candidate={stats['candidate']}"
        )
        return 0
    if "--diff" in argv:
        frozen = Path(argv[argv.index("--diff") + 1])
        want = [ln.strip() for ln in frozen.read_text().splitlines() if ln.strip()]
        got = [str(p) for p in out]
        if want == got:
            print(f"OK 候选清单与冻结一致（{len(got)} 行）")
            return 0
        only_frozen = [x for x in want if x not in got]
        only_got = [x for x in got if x not in want]
        print(f"FAIL 候选清单与冻结不一致：冻结 {len(want)} / 实跑 {len(got)}", file=sys.stderr)
        for x in only_frozen:
            print(f"  只在冻结里: {x}", file=sys.stderr)
        for x in only_got:
            print(f"  只在实跑里: {x}", file=sys.stderr)
        return 1
    for p in out:
        print(p)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
