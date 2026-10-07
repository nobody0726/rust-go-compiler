#!/usr/bin/env bash
# =============================================================================
# 无头调试链路冒烟测试 —— E5 的【可重放】证据
#
# 直接在容器里用 CodeLLDB 平台包自带的 lldb 跑一次完整会话：
#   下断点 → 命中 → 读变量 → 单步 → 读中间值 → 跑完测试
# 逐项断言，任一不满足即 exit 1。
#
# 它与 E5 的关系：
#   E5 的【验收】是 VSCode 里人工按 F5 的实测（T27/T28）—— 那验证的是编辑器链路，
#   已于 2026-10-02 通过（登记在 M0-manifest.json 的 gate.E5）。
#   本脚本验证的是它**下面那一层**（调试信息 + 断点解析 + ptrace + 平台包二进制），
#   出问题时会明确指出是环境层还是编辑器层，避免把两者混为一谈。
#   环境复发时（换机器 / 重装 / 升级宿主 VSCode / 容器重建）先跑本脚本定位层级。
#
# 用法
#   容器内：bash /work/scripts/debug-smoke-test.sh
#   宿主  ：docker exec <容器名> bash /work/scripts/debug-smoke-test.sh
#
# 退出码：0 = 全部通过；1 = 有断言失败
# =============================================================================
set -uo pipefail

WS="/work/rgoc"
SRC="${WS}/crates/rgoc-harness/src/lib.rs"

# ---------------------------------------------------------------------------
# 0. 前置：源码必须存在
# ---------------------------------------------------------------------------
if [ ! -f "${SRC}" ]; then
    echo "错误：找不到 ${SRC}" >&2
    exit 1
fi

# 断点行由源码推导，避免硬编码行号在编辑后失效。
# 必须锚定【行首只有空白】的代码行 —— 文件里的注释同样含有 'let sum = a + b;'
# 这串字符（说明为什么不是那个形状），宽松匹配会命中注释行（实测踩过）。
BREAK_LINE="$(grep -nE '^[[:space:]]*let sum = a \+ b;[[:space:]]*$' "${SRC}" | head -1 | cut -d: -f1)"
if [ -z "${BREAK_LINE}" ]; then
    echo "错误：在 ${SRC} 中找不到用于断点的语句 'let sum = a + b;'" >&2
    exit 1
fi
STEP_LINE=$((BREAK_LINE + 1))
echo "断点：lib.rs:${BREAK_LINE}   单步后应在：lib.rs:${STEP_LINE}"

# ---------------------------------------------------------------------------
# 1. 平台包自带的 lldb（不是系统那个）
# ---------------------------------------------------------------------------
EXT=""
for d in "${HOME}"/.vscode-server/extensions/vadimcn.vscode-lldb-*; do
    [ -d "$d" ] && { EXT="$d"; break; }
done
if [ -z "${EXT}" ]; then
    echo "错误：找不到 CodeLLDB 扩展目录 —— 先跑 scripts/install-codelldb.sh" >&2
    exit 1
fi
LLDB="${EXT}/lldb/bin/lldb"
if [ ! -x "${LLDB}" ]; then
    echo "错误：${LLDB} 不存在或不可执行（平台包未安装）—— 先跑 scripts/install-codelldb.sh" >&2
    exit 1
fi
LLDB_VERSION="$("${LLDB}" --version | head -1)"
case "${LLDB_VERSION}" in
    *codelldb*) echo "lldb：${LLDB_VERSION}" ;;
    *) echo "错误：lldb 版本串不含 'codelldb'：${LLDB_VERSION}" >&2; exit 1 ;;
esac

# ---------------------------------------------------------------------------
# 2. 按 .vscode/launch.json【原样】执行 cargo —— 复刻 CodeLLDB 的那一步
#
#    这一节是 M0-benchmarks.md §10 两个坑的**端到端**回归：
#      · cargo 的工作目录必须取自 cargo.cwd（顶层 cwd 对 cargo 无效）
#      · filter.name 必须等于 cargo 的 target name（下划线），不是包名（连字符）
#    两者都只在「真的按 launch.json 跑一次」时才暴露，所以在这里跑，而不是复制参数。
#
#    注意：用 Python 的 argv 列表调用 cargo（**不经 shell**）—— CodeLLDB 也是
#    spawn 无 shell；经过 shell 会剥掉 `target.'cfg(all())'` 的单引号，得到假报错。
# ---------------------------------------------------------------------------
echo
echo "按 .vscode/launch.json 复刻 CodeLLDB 的 cargo 步骤…"
CONFIG_NAME="调试当前测试 (CodeLLDB)"
BIN="$(CONFIG_NAME="${CONFIG_NAME}" EXT="${EXT}" python3 - <<'PY'
import json, os, pathlib, re, subprocess, sys

WS = pathlib.Path("/work")
name = os.environ["CONFIG_NAME"]
ok = 0

def done(desc, detail=""):
    print("  \033[32m✓\033[0m %-38s %s" % (desc, detail), file=sys.stderr)

def die(msg, code=1):
    print("  \033[31m✗\033[0m %s" % msg, file=sys.stderr)
    sys.exit(code)

raw = re.sub(r"^\s*//.*$", "", (WS / ".vscode/launch.json").read_text(encoding="utf-8"), flags=re.M)
cfgs = [c for c in json.loads(raw)["configurations"] if c["name"] == name]
if len(cfgs) != 1:
    die("launch.json 里找不到唯一名为 %r 的配置" % name)
cargo = cfgs[0].get("cargo") or die("该配置没有 cargo 节")

# CodeLLDB 在 resolveDebugConfigurationWithSubstitutedVariables 里解析 cargo，
# 也就是【变量已替换】之后 —— 这里做同样的替换。
subst = lambda s: s.replace("${workspaceFolder}", str(WS))

cwd = subst(cargo.get("cwd") or "")
if not cwd:
    die("cargo.cwd 缺失：CodeLLDB 会回退到 workspaceFolder，那里没有 Cargo.toml（见 M0-benchmarks.md §10）")
if not (pathlib.Path(cwd) / "Cargo.toml").is_file():
    die("cargo.cwd=%s 下没有 Cargo.toml（见 M0-benchmarks.md §10）" % cwd)
done("A) cargo.cwd 来自 launch.json", cwd)  # 只走 stderr，stdout 留给结果

# 完全照 CodeLLDB 的方式拼参数：插到 `--` 之前，或追加到末尾
runner = str(pathlib.Path(os.environ["EXT"]) / "bin" / "codelldb-launch")
extra = ["--message-format=json", "--color=always",
         "--config=target.'cfg(all())'.runner=[%s]" % json.dumps(runner)]
argv = ["cargo"] + [subst(a) for a in cargo.get("args", [])]
at = argv.index("--") if "--" in argv else len(argv)
argv[at:at] = extra

proc = subprocess.run(argv, cwd=cwd, capture_output=True, text=True)
if proc.returncode != 0:
    print(proc.stderr[-2000:], file=sys.stderr)
    die("cargo 退出码 %d（cwd=%s）—— 与 VSCode 弹的 "
        "\"Cargo command did not complete successfully.\" 是同一件事" % (proc.returncode, cwd))
done("B) cargo 退出码 0", cwd)

# 逐行复刻 CodeLLDB 的 runCargoAndGetArtifacts + getProgramFromArtifacts
arts = []
for line in proc.stdout.splitlines():
    if not line.startswith("{"):
        continue
    ev = json.loads(line)
    if ev.get("reason") != "compiler-artifact":
        continue
    t = ev["target"]
    if ("bin" in t["crate_types"] and "custom-build" not in t["kind"]) or ev["profile"].get("test"):
        if ev.get("executable") is not None:
            arts.append({"fileName": ev["executable"], "name": t["name"], "kind": t["kind"][0]})

flt = cargo.get("filter") or {}
kept = [a for a in arts
        if not ((flt.get("name") is not None and a["name"] != flt["name"])
                or (flt.get("kind") is not None and a["kind"] != flt["kind"]))]
if len(kept) != 1:
    die("filter 选中 %d 个产物（应为 1）：filter=%s 原始产物=%s（见 M0-benchmarks.md §10）"
        % (len(kept), flt, arts))
done("C) filter 恰好选中 1 个产物", kept[0]["name"])

print(kept[0]["fileName"])   # stdout 只输出这一行，供 shell 取用
PY
)" || { echo "错误：复刻 CodeLLDB 的 cargo 步骤失败（见上方 ✗ 行）"; exit 1; }

if [ -z "${BIN}" ] || [ ! -x "${BIN}" ]; then
    echo "错误：解析出的调试目标不可执行：'${BIN}'" >&2
    exit 1
fi
echo "二进制：${BIN}"

# ---------------------------------------------------------------------------
# 3. 无头调试会话
#    不加测试过滤器：让 harness 跑全部测试，double_sum 必然被调用到。
# ---------------------------------------------------------------------------
echo
echo "运行无头调试会话…"
OUT="$("${LLDB}" --batch \
    -o "breakpoint set --file lib.rs --line ${BREAK_LINE}" \
    -o "run" \
    -o "bt 3" \
    -o "frame variable a b" \
    -o "next" \
    -o "frame variable sum" \
    -o "continue" \
    "${BIN}" 2>&1)"

# ---------------------------------------------------------------------------
# 4. 逐项断言
# ---------------------------------------------------------------------------
FAIL=0
check() {
    local desc="$1" pattern="$2" actual="$3"
    if printf '%s\n' "${actual}" | grep -qE "${pattern}"; then
        printf '  \033[32m✓\033[0m %-42s %s\n' "${desc}" "$(printf '%s\n' "${actual}" | grep -oE "${pattern}" | head -1)"
    else
        printf '  \033[31m✗\033[0m %-42s 未匹配 /%s/\n' "${desc}" "${pattern}"
        FAIL=1
    fi
}

echo
echo "E5 环境层检查"
echo "------------------------------------------------------------"
check "① 断点按 file:line 解析到 double_sum" \
      "rgoc_harness::double_sum.*lib\.rs:${BREAK_LINE}" "${OUT}"
check "② 断点命中（未被跳过）" \
      "stop reason = breakpoint 1\.1" "${OUT}"
check "③ 调用栈含 double_sum 与测试函数" \
      "rgoc_harness::tests::double_sum_works" "${OUT}"
check "④ 形参 a = 1" \
      "a = 1" "${OUT}"
check "⑤ 形参 b = 2" \
      "b = 2" "${OUT}"
check "⑥ 单步生效（step over）" \
      "stop reason = step over" "${OUT}"
check "⑦ 单步后停在 lib\.rs:${STEP_LINE}" \
      "lib\.rs:${STEP_LINE}" "${OUT}"
check "⑧ 中间值 sum = 3 可读" \
      "sum = 3" "${OUT}"
#⚠️ 2026-10-07 修正：原先写死 `1 passed`，但 lib.rs 里实际有 17 个测试
#   （cargo test --lib 会跑整个 crate 的单测），实测输出是 `17 passed`。
#   分母随lib.rs 的测试数变化，把它写死会让这条断言在【测试数一变】时就误报，
#   而它本意只是「测试跑完了且没失败」。⇒ 只断言 ok + 0 failed。
check "⑨ 测试跑到结束（无失败）" \
      "test result: ok\. [0-9]+ passed; 0 failed" "${OUT}"
echo "------------------------------------------------------------"

if [ "${FAIL}" -ne 0 ]; then
    echo
    echo "原始输出（便于排查）："
    printf '%s\n' "${OUT}"
    echo
    echo "结果：存在失败项。"
    exit 1
fi

echo "结果：环境层 9 项全部通过。"
echo "（E5 已通过：2026-10-02 人工按 F5 实测确认；本脚本是它下层证据的可重放回归）"
echo "（环境复发时先跑本脚本定位是环境层还是编辑器层 —— 见 docs/milestones/M0-benchmarks.md §10）"
