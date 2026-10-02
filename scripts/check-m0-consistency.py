#!/usr/bin/env python3
"""M0 环境一致性自检（一条命令跑完，退出码即结论）。

用法：
    python3 scripts/check-m0-consistency.py

职责边界：只做【可判定的断言】，不修改任何文件。检查六类不变量：

1. `.devcontainer/devcontainer.json` / `.vscode/launch.json` 的关键配置存在
   —— 其中 `http.proxy: ""` 这条要按【保留但已知无效】理解：该值由宿主经 AHP
   `root/configChanged` 下发，远端 Machine settings 覆盖不了它（实测见
   M0-benchmarks.md §7）。保留是「无副作用」，不是修法。
   `launch.json` 的 4 条则对应 §10 的两个**静默**陷阱（`cargo.cwd` / `filter.name`）。
2. `docker/image.lock` 记录的各 `src.*_sha256` 与当前文件一致
   —— 否则说明锁文件已过期，E1 的证据链断了。
3. `image.lock` / `M0-manifest.json` / 实际 `rgoc:dev` 镜像三方的 image id 一致。
   （注意：image id 本身【不可复现】，这里只查「三方是否指向同一次构建」，
     不是查它稳定。）
4. 交付物不含尖括号占位符。
5. 调试链路的可判定前提（M0-benchmarks.md §7 / §8 / §9 / §10 的回归测试）：
   - `scripts/install-codelldb.sh` 必须绕开 VSCode 网络栈（`--noproxy '*'`）、
     以 `platform.ok` 幂等、校验 sha256、并按【输出】断言 lldb 可用；
   - `scripts/install-vscode-server.sh` 必须绕开 VSCode 网络栈、写进**持久卷**
     `/vscode`（否则容器重建后失效）、补 `~/.vscode-server/bin/<commit>` 符号链接、
     幂等、按 `uname -m` 解析架构；且它的**断言判据必须是语义**（`product.json` 的
     commit），不得改为 tarball 字节哈希（上游会重打包，见 §9）；
   - `M0-benchmarks.md` §9 与 manifest 里记录的 tarball 指纹**不得漂移**；
   - 调试目标必须保持「中间值被第二次读取」的形状，且不得退回
     `#[allow(clippy::let_and_return)]` —— 尾位置直接返回时 rustc 不生成
     DWARF 变量条目，T28 的第 4 项检查会变成不可能完成。
   - §5c：`debug-smoke-test.sh` 第 2 节必须**按 `.vscode/launch.json` 复刻**
     CodeLLDB 的 cargo 步骤（读真文件 / 替换变量 / **不经 shell** / 复刻 filter
     筛选）—— 退回「自己抄一套参数」的话，§10 的两个坑就重新变成测不到。
6. `AGENTS.md` 里记录的断言数 == 实际断言数（防文档漂移）。

   ⚠️ 第 5 节的断言一律作用于 `code_only()`（剔除整行注释）而非原文：
   `install-*.sh` 的头部注释里也写着 `--noproxy '*'` / `platform.ok` / `ln -sfn` /
   `[0-9a-f]{40}`，用原文做子串匹配会被注释满足、**变成恒真断言**。
   这一点曾由变异测试实测确认（见 `code_only` 的 docstring）。

依赖：python3（标准库）+ docker CLI。不依赖任何第三方包。
"""

from __future__ import annotations

import json
import os
import pathlib
import re
import subprocess
import sys

REPO_ROOT = pathlib.Path(__file__).resolve().parent.parent
FAILURES: list[str] = []
TOTAL = 0


def load_jsonc(rel: str) -> dict:
    """读取带 // 行注释的 JSON/JSONC（devcontainer.json 与 launch.json 用）。"""
    text = (REPO_ROOT / rel).read_text(encoding="utf-8")
    return json.loads(re.sub(r"^\s*//.*$", "", text, flags=re.M))


def check(label: str, ok: bool, detail: str = "") -> None:
    global TOTAL
    TOTAL += 1
    print(("  \u2713 " if ok else "  \u2717 ") + label + (f"  {detail}" if detail else ""))
    if not ok:
        FAILURES.append(label)


def sha256_of(rel: str) -> str:
    out = subprocess.run(
        ["shasum", "-a", "256", str(REPO_ROOT / rel)],
        capture_output=True, text=True, check=True,
    ).stdout
    return out.split()[0]


def code_only(text: str) -> str:
    """剔除【整行注释】后的文本；断言应当作用于它。

    动机：本仓已两次踩到「断言被注释满足」的坑 ——
      ① 推导断点行的 `grep -n 'let sum = a + b;'` 命中了**注释行**，推出错误行号；
      ② `install-*.sh` 的头部注释里也写着 `--noproxy '*'` / `platform.ok` /
         `ln -sfn` / `[0-9a-f]{40}`，纯子串匹配会被注释满足，于是断言**恒真**。
    反向校验已验证（当时的变异测试，2026-10-02）：改用本函数后，把真实代码行改成
    注释形态会被抓住。**该变异测试脚本后按用户要求移除**，所以现在改这里的断言时，
    反向验证要人工做一遍（把目标行改成注释形态 / 删掉，确认断言报 ✗）。
    """
    return "\n".join(
        line for line in text.splitlines() if not line.lstrip().startswith("#")
    )


def main() -> int:
    print("M0 环境一致性自检")
    print("=" * 60)

    # ── 1. devcontainer / launch.json ──────────────────────────────────────
    print("[1] devcontainer 与 launch.json")
    dc = load_jsonc(".devcontainer/devcontainer.json")
    settings = dc["customizations"]["vscode"]["settings"]

    check("runArgs 含 ptrace 与 seccomp",
          "--cap-add=SYS_PTRACE" in dc["runArgs"] and "seccomp=unconfined" in dc["runArgs"])
    volumes = [m for m in dc["mounts"] if "type=volume" in m]
    check("三个命名卷", len(volumes) == 3, str(len(volumes)))
    target_mount = [m for m in dc["mounts"] if "/work/rgoc/target" in m]
    check("rgoc/target 挂 rgoc-target 卷",
          bool(target_mount) and "rgoc-target" in target_mount[0])
    check("postCreateCommand 修正命名卷属主",
          dc.get("postCreateCommand") == "sudo chown 501:20 /work/rgoc/target",
          repr(dc.get("postCreateCommand")))
    # 回归断言：这两行【保留但已知无效】—— 真正修法是 scripts/install-codelldb.sh
    # （宿主经 AHP root/configChanged 下发 http.proxy，远端设置覆盖不了；见 §7）
    check("http.proxy 显式置空（保留；已知不足以修好 CodeLLDB）",
          settings.get("http.proxy") == "", repr(settings.get("http.proxy")))
    check("http.proxySupport 为 off", settings.get("http.proxySupport") == "off")

    launch = load_jsonc(".vscode/launch.json")
    check("launch.json 全部为 lldb 配置",
          all(c["type"] == "lldb" for c in launch["configurations"]),
          str([c["name"] for c in launch["configurations"]]))

    # 回归（M0-benchmarks.md §10）：CodeLLDB 的 cargo 启动配置有两处**静默**陷阱 ——
    #   ① 工作目录取自 **cargo.cwd**，【不读顶层的 cwd】
    #      （源码：getCargoCwd(e){return e ?? this.workspaceFolder?.uri?.fsPath}，
    #       调用处传的是 e.cargo.cwd）。漏写就回退到 workspaceFolder = /work，
    #       而 /work 下没有 Cargo.toml → cargo 退出 101 →
    #       VSCode 报 "Cargo command did not complete successfully."。
    #   ② filter.name 比对的是 **target name（crate 名，下划线）**，不是包名（连字符）
    #      （产物入列用 e.target.name，再按 e.name != filter.name 过滤）。
    #       写成包名会 0 匹配 → "Cargo has produced no matching compilation artifacts."
    workspace_folder = dc["workspaceFolder"]
    for cfg in launch["configurations"]:
        if "cargo" not in cfg:
            continue
        raw = cfg["cargo"].get("cwd")
        check(f"「{cfg['name']}」设置了 cargo.cwd（顶层 cwd 对 cargo 无效）",
              isinstance(raw, str) and raw.startswith("${workspaceFolder}"),
              repr(raw))
        if not isinstance(raw, str):
            continue
        # 容器路径 → 仓库内相对路径，确认那底下真的有 Cargo.toml
        rel = raw.replace("${workspaceFolder}", "").strip("/")
        cargo_toml = REPO_ROOT / rel / "Cargo.toml"
        check(f"「{cfg['name']}」的 cargo.cwd 下有 Cargo.toml",
              cargo_toml.is_file(),
              f"{workspace_folder}/{rel}/Cargo.toml")
        if not cargo_toml.is_file():
            continue
        # filter.name 必须等于该 crate 的 lib target name；
        # 未写 [lib] name 时，cargo 用**包名把 `-` 换成 `_`**。
        flt = cfg["cargo"].get("filter")
        if not (isinstance(flt, dict) and "name" in flt):
            continue
        # 注意 rel/Cargo.toml 是 **workspace 的虚拟 manifest**（没有 [package].name），
        # 真正带 name 的是 crate 的 manifest —— 从 args 里的 --package= 定位它
        pkg = next((a.split("=", 1)[1] for a in cfg["cargo"].get("args", [])
                    if a.startswith("--package=")), None)
        expected = None
        if pkg:
            for cand in sorted((REPO_ROOT / rel).rglob("Cargo.toml")):
                text = cand.read_text(encoding="utf-8")
                if re.search(r'^name\s*=\s*"' + re.escape(pkg) + r'"', text, flags=re.M):
                    expected = pkg.replace("-", "_")
                    break
        check(f"「{cfg['name']}」的 filter.name 用 target name（下划线）",
              expected is not None and flt["name"] == expected,
              f"filter.name={flt['name']!r} 应为 {expected!r}（包 {pkg!r}）")

    # ── 2. image.lock 的源码指纹 ────────────────────────────────────────────
    print("[2] image.lock 的源码指纹")
    lock = (REPO_ROOT / "docker/image.lock").read_text(encoding="utf-8")
    fingerprints = {
        "src.dockerfile_sha256": "docker/Dockerfile",
        "src.toolchain_sha256": "rust-toolchain.toml",
        "src.dockerignore_sha256": ".dockerignore",
        "src.devcontainer_sha256": ".devcontainer/devcontainer.json",
        "src.in_container_sha256": "scripts/in-container.sh",
    }
    for key, path in fingerprints.items():
        m = re.search(r"^" + re.escape(key) + r"\s+=\s+(\S+)", lock, flags=re.M)
        recorded = m.group(1) if m else "(缺失)"
        actual = sha256_of(path)
        check(f"{key} == {path}", recorded == actual,
              "" if recorded == actual else f"记录={recorded[:12]} 实际={actual[:12]}")

    # ── 3. 三方 image id 一致 ──────────────────────────────────────────────
    print("[3] image.lock / manifest / 实际镜像 的 image id")
    recorded_id = re.search(r"^local\.image_id\s+=\s+(\S+)", lock, flags=re.M)
    recorded_id = recorded_id.group(1) if recorded_id else "(缺失)"
    actual_id = subprocess.run(
        ["docker", "image", "inspect", "rgoc:dev", "--format", "{{.Id}}"],
        capture_output=True, text=True,
    ).stdout.strip()
    if not actual_id:
        check("rgoc:dev 镜像存在", False, "docker image inspect 无输出")
    else:
        check("image.lock 的 id == 实际镜像 id", recorded_id == actual_id, actual_id[:24] + "…")
        manifest = json.loads((REPO_ROOT / "docs/milestones/M0-manifest.json").read_text(encoding="utf-8"))
        check("manifest 的 image.digest == 实际镜像 id",
              manifest["environment"]["image"]["digest"] == actual_id)
        # 四条门禁全部通过（E5 于 2026-10-02 由用户在 VSCode 中按 F5 实测确认）。
        # 注意 E5 是**人工**门禁：这里只校验 manifest 的登记状态，不校验实测本身
        # —— 实测证据在 gate.E5.evidence 里（含确认人与确认时间）。
        gate = {k: manifest["gate"][k]["status"] for k in ("E1", "E2", "E10", "E5")}
        check("gate: E1/E2/E10/E5 全部 pass",
              [gate[k] for k in ("E1", "E2", "E10", "E5")] == ["pass"] * 4,
              str(gate))
        check("gate.E5 登记了确认人与确认时间",
              bool(manifest["gate"]["E5"].get("confirmed_by"))
              and bool(manifest["gate"]["E5"].get("confirmed_at")),
              manifest["gate"]["E5"].get("confirmed_at", "(缺失)"))
        check("benchmarks 五节齐备", len(manifest["benchmarks"]) == 5)

        # ── 门禁状态在【文档】里也必须同步（2026-10-02 补，因一次真实漂移）────
        # 事故：gate.E5 改成 pass 之后，M0-plan.md 的门禁汇总表被回退成「☐ 待人工」，
        # 而自检当时只校验 manifest，**全绿放行**。manifest 是机器可读事实，
        # 文档是人读的入口 —— 两者不一致时，人会以文档为准，判定就是错的。
        plan_text = (REPO_ROOT / "docs/milestones/M0-plan.md").read_text(encoding="utf-8")
        drift = []
        for gid, status in gate.items():
            rows = [ln for ln in plan_text.splitlines() if ln.startswith(f"| **{gid}** |")]
            if not rows:
                drift.append(f"{gid}=缺行")
                continue
            row = rows[0]
            if status == "pass" and ("待人工" in row or "✅" not in row):
                drift.append(f"{gid}=manifest.pass 但文档行不是 ✅")
            elif status != "pass" and "待人工" not in row:
                drift.append(f"{gid}=manifest.{status} 但文档行没有「待人工」")
        check("M0-plan 门禁汇总表的状态 == manifest 的 gate（双向）",
              not drift, "；".join(drift) if drift else f"{len(gate)} 条一致")

        # T28 是 E5 的载体任务；它的状态列同样不能与 gate.E5 脱节
        t28 = [ln for ln in plan_text.splitlines() if ln.startswith("| T28 |")]
        t28_ok = bool(t28) and (
            (gate["E5"] == "pass" and "✅" in t28[0])
            or (gate["E5"] != "pass" and "✅" not in t28[0])
        )
        check("M0-plan 任务总表里 T28 的状态 == gate.E5",
              t28_ok, t28[0][:60] if t28 else "缺 T28 行")

        # 计划里【声称】的任务范围必须与实际写出来的任务标题连续无缺号。
        # 本轮一次补写 27 个任务（T29–T55），一个编号笔误不会被任何其他断言发现。
        heads = [int(m) for m in re.findall(r"^### 任务 T(\d+)[：:]", plan_text, flags=re.M)]
        expect_ids = list(range(1, max(heads) + 1)) if heads else []
        check("M0-plan 的任务标题 T01…Tmax 连续无缺号、无重复",
              heads == expect_ids,
              f"{len(heads)} 个任务，最大 T{max(heads) if heads else 0}"
              + ("" if heads == expect_ids else f"；缺号/重复：{sorted(set(expect_ids) - set(heads)) or '有重复'}"))
        # 任务总表里声称的区间上界必须与实际任务数一致（防止「说 T55 其实只写到 T52」）
        claimed = re.search(r"\| T48–T(\d+) \| Phase 4", plan_text)
        check("任务总表声称的 Phase 4 上界 == 实际最后一个任务号",
              claimed is not None and int(claimed.group(1)) == max(heads),
              claimed.group(0) if claimed else "总表缺 Phase 4 行")

        # ── M0-tests.md 的冻结状态（T29 冻结后不可回退）────────────────────
        # 冻结是门禁纪律的硬要求（`03` §3.3「白名单须开工前冻结」）。一旦回退成
        # 「待冻结」，Phase 2 就失去「只由这些 ID 判定」的依据，而没有任何其他断言会发现。
        tests_text = (REPO_ROOT / "docs/milestones/M0-tests.md").read_text(encoding="utf-8")
        check("M0-tests.md 已冻结（状态行）",
              "已冻结（2026-10-02，任务 T29）" in tests_text,
              tests_text.splitlines()[2][-40:] if len(tests_text.splitlines()) > 2 else "")
        # F1：20 个官方样本不得削减（`03` §4 门禁写死「至少 20 个」）
        n_tc = len(re.findall(r"^\| \*\*T-C-\d+", tests_text, flags=re.M))
        check("M0-tests.md 的 T-C 样本数 == 20（F1 不得削减）",
              n_tc == 20, f"实际 {n_tc} 个")
        # F4：M0 分母是冻结数据，改它必须先改文档并说明理由
        check("M0-tests.md 记录的 M0 分母 == 279（F4，已冻结）",
              "**279** = `run` 147 + `errorcheck` 120 + `compile` 12" in tests_text)
        # 分派顺序是冻结时实测得到的，漏掉会让 harness 在真实语料上误报 T-H-03
        check("M0-tests.md 记有「平台过滤先于未知指令判定」（R1b）",
              "R1b" in tests_text and "平台过滤**先于**未知指令判定" in tests_text)
        # T30 复核补的两条规则：R2b（合并流）与 R6（go tool compile 而非 go build）。
        # 这两条是 harness 的命门 —— 缺任何一条，20 个样本里就会有一片误判。
        check("M0-tests.md 记有 R2b（stdout 与 stderr 是合并流）",
              "R2b" in tests_text and "stdout` 与 `stderr` 的【合并流】" in tests_text)
        check("M0-tests.md 记有 R6（三层命令形态是 go tool compile）",
              "R6" in tests_text and "不是 `go build`" in tests_text)
        # T-C-20 的 ERROR 条数：文档曾写成 4，源码实为 5（L9/L10×2/L12/L13）。
        # 这条被改过，所以钉住它 —— 防止又被人「改回 4 条」而没人发现。
        check("M0-tests.md 的 T-C-20 ERROR 条数 == 5（T30 实测修正）",
              "| **T-C-20** | `test/mainsig.go` | 598 B | `main`/`init` 签名；**同一行两条 ERROR** | **5** |"
              in tests_text)
        # T30 的复核记录必须在 benchmarks §11（E4 的证据链）
        bench_text = (REPO_ROOT / "docs/milestones/M0-benchmarks.md").read_text(encoding="utf-8")
        check("M0-benchmarks.md 含 §11（T30 的 20 行复核表）",
              "## 11. T30 复核" in bench_text
              and "T-C-01" in bench_text and "T-C-20" in bench_text)
        # 复核结论查【机器可读】的 manifest（措辞不会因改写而漂），不查文档字面
        evr = manifest.get("test_contract", {}).get("expected_values_reviewed", {})
        check("T30 复核结论已入 manifest（19/20 一致 + 1 处文档错已改）",
              str(evr.get("result", "")).startswith("19/20")
              and "T-C-20" in str(evr.get("result", ""))
              and "R2b" in " ".join(evr.get("rules_added", [])),
              str(evr.get("result", "(缺失)"))[:52])

    # ── 4. 占位符 ──────────────────────────────────────────────────────────
    print("[4] 交付物占位符")
    for rel in ("docs/milestones/M0-manifest.json", "docker/image.lock"):
        check(f"{rel} 无尖括号占位符", "<" not in (REPO_ROOT / rel).read_text(encoding="utf-8"))

    # ── 5. 调试链路的可判定前提（M0-benchmarks.md §7 / §8 的回归测试）────────
    print("[5] 调试链路：平台包安装器与调试目标形状")

    installer_rel = "scripts/install-codelldb.sh"
    smoke_rel = "scripts/debug-smoke-test.sh"
    for rel in (installer_rel, smoke_rel):
        p = REPO_ROOT / rel
        check(f"{rel} 存在且可执行", p.is_file() and os.access(p, os.X_OK))

    installer = code_only((REPO_ROOT / installer_rel).read_text(encoding="utf-8"))
    # 绕开 VSCode 网络栈 —— 这是整个修法的核心，删掉就退回「被死代理卡住」
    check("安装器用 --noproxy '*' 绕开宿主下发的死代理", "--noproxy '*'" in installer)
    # 幂等判据必须与 CodeLLDB 的 ensurePlatformPackage() 用同一个标记
    check("安装器以 platform.ok 作为幂等判据", "platform.ok" in installer)
    check("安装器校验下载产物 sha256",
          "sha256sum" in installer and "EXPECTED_SHA256" in installer)
    # 损坏的 lldb 会打 traceback 但仍以 0 退出，所以必须查输出
    check("安装器按【输出】断言 lldb 含 codelldb", "*codelldb*" in installer)

    harness_rel = "rgoc/crates/rgoc-harness/src/lib.rs"
    harness = (REPO_ROOT / harness_rel).read_text(encoding="utf-8")
    # §8：尾位置直接返回的 let 绑定不进 DWARF → 中间值不可观察
    check("调试目标让中间值被第二次读取（sum 进入 DWARF）",
          re.search(r"^\s*let sum = a \+ b;\s*$", harness, flags=re.M) is not None
          and re.search(r"^\s*sum \* 2\s*$", harness, flags=re.M) is not None)
    check("调试目标不再需要 clippy allow",
          "allow(clippy::let_and_return)" not in harness)

    # ── T31 的产物：Test IR 骨架（真正「跑起来对不对」由 cargo test 负责）──────
    # 这里的四条是**静态**判据：防止 ir.rs 被删、被改名，或判定分类被悄悄合并。
    harness_ir = REPO_ROOT / "rgoc/crates/rgoc-harness/src/ir.rs"
    ir_text = code_only(harness_ir.read_text(encoding="utf-8")) if harness_ir.is_file() else ""
    harness_lib = code_only(harness)  # harness 已是 lib.rs 的文本
    check("T31 产物：ir.rs 存在且 lib.rs 声明 pub mod ir（Test IR 骨架在位）",
          bool(ir_text) and "pub mod ir;" in harness_lib)
    # 八种判定分类 —— 合并就等于把基建失败算成语术失败（03 §3.3）
    missing_v = [v for v in ("Pass", "CompilerFailure", "RuntimeFailure", "HarnessFailure",
                             "TargetFiltered", "Timeout", "ResourceFailure",
                             "ReferenceToolchainFailure")
                 if f"    {v}," not in ir_text]
    check("Test IR 的 Verdict 八种分类齐全（不得合并）",
          not missing_v, "缺：" + ", ".join(missing_v) if missing_v else "8 种")
    # 超时与资源上限只在 for_layer 一处定义（M0-tests §7.5）——
    # 散落在各处就会出现两套预算，而门禁只看其中一套
    check("冻结预算只有一个入口：Limits::for_layer + with_rss_override",
          "pub fn for_layer(layer: Layer) -> Self" in ir_text
          and "pub fn with_rss_override(" in ir_text)
    # C2 契约的可执行副本：测试文件在位且覆盖八种标识
    ir_test = REPO_ROOT / "rgoc/crates/rgoc-harness/tests/test_ir.rs"
    ir_test_text = ir_test.read_text(encoding="utf-8") if ir_test.is_file() else ""
    check("C2 契约的可执行副本 tests/test_ir.rs 在位（含八种判定标识）",
          all(s in ir_test_text for s in ("compiler-failure", "runtime-failure",
                                          "harness-failure", "target-filtered",
                                          "resource-failure", "reference-toolchain-failure")))
    check("冒烟测试从源码推导断点行（锚定行首纯代码行）",
          "grep -nE '^[[:space:]]*let sum = a \\+ b;" in
          code_only((REPO_ROOT / smoke_rel).read_text(encoding="utf-8")))

    # ── 5b. VS Code Server 离线安装器（M0-benchmarks.md §9 的回归测试）──────
    # 动机：宿主每次升级 VSCode 都会改变 commit，持久卷里没有就退回「宿主侧下载」，
    # 再被那个已停服的代理挡住。这个脚本是那一层的唯一修法，所以它的形状要钉住。
    print("[5b] VS Code Server 离线安装器与指纹一致性")
    server_rel = "scripts/install-vscode-server.sh"
    server_path = REPO_ROOT / server_rel
    check(f"{server_rel} 存在且可执行",
          server_path.is_file() and os.access(server_path, os.X_OK))
    server = code_only(server_path.read_text(encoding="utf-8"))

    # 绕开 VSCode 网络栈 —— 与 §7 同款手段，删掉就退回「被死代理卡住」
    check("server 安装器用 --noproxy '*' 绕开死代理", "--noproxy '*'" in server)
    # 必须写进【持久卷】：写到 ~ 或 /tmp 都只是「这次能用」，容器重建后失效
    check("server 安装器写进持久卷 /vscode（容器重建后仍命中）",
          'VOLUME_BIN="/vscode/vscode-server/bin/linux-${VSCODE_ARCH}"' in server)
    # Dev Containers 先查符号链接、再查卷内实体，两处都要在。
    # 锚定到【行首】的 ln 语句：否则 `true # ln -sfn ...` 这类形态也能满足断言。
    check("server 安装器补 ~/.vscode-server/bin/<commit> 符号链接",
          re.search(r'^\s*ln -sfn\b', server, flags=re.M) is not None
          and 'LINK="${HOME}/.vscode-server/bin/${COMMIT}"' in server)
    # 幂等：与 devcontainer 的 postCreateCommand 一样，重复执行必须零副作用
    check("server 安装器幂等（bin/code-server 存在即短路）",
          re.search(r'if \[ -x "\$\{DEST\}/bin/code-server" \]', server) is not None)
    # 架构按 uname -m 解析，不写死 arm64 —— 写死会静默装错架构
    check("server 安装器按 uname -m 解析架构（未写死 arm64）",
          'case "$(uname -m)" in' in server and "x86_64|amd64" in server)
    # 断言判据是【语义】而非字节：上游可能重打包 tarball，字节哈希会无辜失配
    check("server 安装器断言 product.json 的 commit（语义，非字节哈希）",
          "require('${DEST}/product.json').commit" in server
          and '[ "${ACTUAL}" != "${COMMIT}" ]' in server)
    check("server 安装器拒绝非 40 位小写 hex 的 commit",
          "[0-9a-f]{40}" in server)

    # 文档与 manifest 记录的指纹【不得两处漂移】
    benchmarks_text = (REPO_ROOT / "docs/milestones/M0-benchmarks.md").read_text(encoding="utf-8")
    debug_env = json.loads(
        (REPO_ROOT / "docs/milestones/M0-manifest.json").read_text(encoding="utf-8")
    )["environment"]["debug"]
    vcs = debug_env["vs_code_server"]
    check("M0-benchmarks.md 含 §9（VS Code Server 案例）",
          "## 9. 案例：VS Code Server" in benchmarks_text)
    check("§9 记录的 tarball sha256 与 manifest 一致",
          vcs["tarball_sha256"] in benchmarks_text, vcs["tarball_sha256"][:16] + "…")
    check("§9 记录的 tarball 字节数与 manifest 一致",
          str(vcs["tarball_bytes"]) in benchmarks_text, str(vcs["tarball_bytes"]))
    check("manifest 记录两处 test -d 均通过（下载会被跳过）",
          vcs["both_test_d_paths_exit_zero"] is True)
    check("manifest 记录的宿主 VSCode commit 与安装路径/符号链接一致",
          debug_env["editor_commit"] in vcs["install_location"]
          and debug_env["editor_commit"] in vcs["symlink"]
          and debug_env["editor_commit"] in vcs["tarball_url"])

    # ── 5c. 冒烟测试第 2 节的形状（M0-benchmarks.md §10 的端到端回归）────────
    # 动机：§10 的两个坑（cargo.cwd / filter.name）只在「真的按 launch.json 跑一次
    # cargo」时才暴露，所以第 2 节改成按 launch.json 复刻那一步。这里守住「它真的是
    # 在复刻」—— 一旦退回「自己抄一套参数」，那两个坑就重新变成测不到。
    #
    # 注：原先这里还有 9 条断言用于守一个变异测试脚本（往 launch.json 注入 §10 的坑
    # 再还原）。该脚本已按用户要求移除（2026-10-02，精简交付物），故这批断言一并
    # 删除。**代价要记着**：现在没有任何自动手段能证明下面这 6 条断言「真的会失败」，
    # 改动它们时需要人工反向验证（把目标改成注释形态或删掉，确认断言报 ✗）。
    print("[5c] 冒烟测试第 2 节的形状（§10 的复刻）")
    smoke_code = code_only((REPO_ROOT / smoke_rel).read_text(encoding="utf-8"))

    # 必须【按 launch.json 跑】而不是把参数抄一份 —— 抄一份就永远测不到漂移
    check("冒烟测试第 2 节读 .vscode/launch.json（非硬编码参数）",
          '".vscode/launch.json"' in smoke_code)
    check("冒烟测试第 2 节替换 ${workspaceFolder}（与 CodeLLDB 同序）",
          'subst = lambda s: s.replace("${workspaceFolder}"' in smoke_code)
    # 必须用 argv 列表调用 —— 经 shell 会剥掉 target.'cfg(all())' 的单引号，
    # 得到一个【假】的 TOML 报错（§10 真实踩过）
    check("冒烟测试第 2 节不经 shell 调用 cargo（argv 列表）",
          "subprocess.run(argv" in smoke_code and "shell=True" not in smoke_code)
    check("冒烟测试第 2 节复刻 filter 的产物筛选",
          'a["name"] != flt["name"]' in smoke_code and 'a["kind"] != flt["kind"]' in smoke_code)
    check("冒烟测试第 2 节逐项打印 A/B/C",
          all(k in smoke_code for k in ("A) cargo.cwd 来自 launch.json",
                                        "B) cargo 退出码 0",
                                        "C) filter 恰好选中 1 个产物")))
    check("冒烟测试第 2 节的失败信息互引 M0-benchmarks.md §10",
          "M0-benchmarks.md §10" in smoke_code)

    # ── 6. 文档记录的规模与实际一致 ────────────────────────────────────────
    # 动机：本轮把断言从 45 一路加到 60+，`AGENTS.md` 里的数字靠手工同步
    # （已经漏过一次）。数字漂了比没有数字更糟 —— 它会让人以为「覆盖了 N 条」
    # 而实际不是。所以让脚本自己守住。
    print("[6] AGENTS.md 记录的数量与实际一致")
    agents_text = (REPO_ROOT / "AGENTS.md").read_text(encoding="utf-8")
    # ⚠️ 这一条必须是**本文件的最后一条断言**：它把自己也数进去，
    #    所以「最终总数 = 此刻的 TOTAL + 1」只在没有后续 check() 时成立。
    # 数字放 detail 而不是 label，让 label 保持稳定。
    expected_total = TOTAL + 1
    check("AGENTS.md 记录的断言数 == 实际数",
          f"（{expected_total} 条断言" in agents_text,
          f"实际 {expected_total} 条（AGENTS.md §1 的 check-m0-consistency.py 那一行）")

    print("=" * 60)
    if FAILURES:
        print(f"结果：{len(FAILURES)} 项失败 —— {FAILURES}")
        return 1
    print("结果：全部通过")
    return 0


if __name__ == "__main__":
    sys.exit(main())
