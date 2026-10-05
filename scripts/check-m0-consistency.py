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

    ⚠️ **两种注释前缀都要剥**（2026-10-02 补）：`#`（shell / YAML / TOML）与
    `//`（Rust / JS）。原先只剥 `#`，于是所有针对 **Rust 文件**的断言其实是在
    「含注释的文本」上匹配的 —— 注释里写着一句 `unsafe` 就能让
    「代码里不该有 unsafe」这条断言永远失败；反过来，注释里写着一句目标代码
    也会让断言永远通过（恒真）。教训与上面那两次同源，只是更深一层。
    """
    return "\n".join(
        line
        for line in text.splitlines()
        if not line.lstrip().startswith(("#", "//"))
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
        # E3 / E4 是 Phase 2 的两条门禁（T36 / T38）。它们也必须 pass ——
        # 「环境门禁过了就算 Phase 2 过了」是错的，D-M0-2 要求的是**全部门禁**。
        # ⚠️ 上面的 gate 字典**只列了 E1/E2/E10/E5 四个**（Phase 0/1 的），
        # 所以下面这四条断言此前**从未真正校验过 E3/E4** —— 加门禁时忘了同步这里。
        # 教训与 T37 的六连坑同形：**「漏了」不会报错，只会让断言恒真**。
        phase2 = {k: manifest["gate"].get(k, {}).get("status") for k in ("E3", "E4")}
        check("gate: E3/E4 全部 pass（Phase 2 出口）",
              list(phase2.values()) == ["pass", "pass"], str(phase2))
        check("gate.E4 登记了 20/20 与确认时间",
              manifest["gate"]["E4"].get("denominator") == 20
              and manifest["gate"]["E4"].get("numerator") == 20
              and bool(manifest["gate"]["E4"].get("confirmed_at"))
              and bool(manifest["gate"]["E4"].get("confirmed_by")),
              f"分母 {manifest['gate']['E4'].get('denominator')} / "
              f"分子 {manifest['gate']['E4'].get('numerator')}")
        # E4 的全量基线：分母 279 且 by_mode 三项之和等于它 —— 防止「分母被缩小」
        full = manifest["gate"]["E4"].get("full_corpus_baseline", {})
        check("gate.E4 的全量基线分母 == 279 且 by_mode 合计相符",
              full.get("denominator") == 279
              and sum(full.get("by_mode", {}).values()) == full.get("denominator"),
              f"分母 {full.get('denominator')}，by_mode {full.get('by_mode')}")
        # E4 报告文件必须在（报告与登记脱节时，登记就失去证据支撑）
        check("gate.E4 登记的报告文件存在",
              all((REPO_ROOT / p).is_file() for p in manifest["gate"]["E4"].get("reports", [])),
              str(manifest["gate"]["E4"].get("reports")))
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

    # ── T32 的产物：指令行解析（R1）+ 分派顺序（R1b）──────────────────────
    instr_rs = REPO_ROOT / "rgoc/crates/rgoc-harness/src/instruction.rs"
    instr_text = code_only(instr_rs.read_text(encoding="utf-8")) if instr_rs.is_file() else ""
    check("T32 产物：instruction.rs 存在且 lib.rs 声明 pub mod instruction",
          bool(instr_text) and "pub mod instruction;" in harness_lib)
    # 16 个官方指令一个都不能少（M0-tests §1.2；少一个就会把真实语料误判成未知指令）。
    # ⚠️ 必须**先截出 KNOWN_COMMANDS 块再逐名核对** —— 光在全文里找 `"buildrun"` 会被
    # `mode_of` 里的同名匹配骗过（T32 实测：注入漏项后断言仍显示 ✓）。
    # 数组长度本身是编译期属性，`cargo check` 也会拦一道；这里是更早、更直白的一层。
    known16 = ["compile", "compiledir", "build", "builddir", "buildrundir", "run",
               "buildrun", "runoutput", "rundir", "runindir", "asmcheck",
               "errorcheck", "errorcheckdir", "errorcheckoutput",
               "errorcheckandrundir", "errorcheckwithauto"]
    block = ""
    if "pub const KNOWN_COMMANDS" in instr_text:
        start = instr_text.index("pub const KNOWN_COMMANDS")
        end = instr_text.index("];", start) + 2
        block = instr_text[start:end]
    missing_cmd = [n for n in known16 if f'"{n}"' not in block]
    check("KNOWN_COMMANDS 列出 M0-tests §1.2 的 16 个指令（截块后逐名核对）",
          bool(block) and not missing_cmd,
          "缺：" + ", ".join(missing_cmd) if missing_cmd else f"16 个齐全（块长 {len(block)}）")
    # Mode::ALL 让「16 + skip == 17 个变体」成为可断言的不变量（否则漏项查不出来）
    check("Mode::ALL 在位（16 个指令 + skip 的双射靠它兜住）",
          "pub const ALL: [Self; 17] = [" in ir_text)
    # **顺序契约**：platform_ok 是必填参数，调用方无法省略平台过滤这一步。
    # 顺序颠倒会让 harness 在 linkmain.go 上误报 T-H-03（T29 冻结时查出的坑）。
    check("R1b 顺序契约：dispatch 的 platform_ok 是必填参数（过滤先于未知指令判定）",
          "pub fn dispatch(ins: &Instruction, platform_ok: bool)" in instr_text
          and instr_text.index("if !platform_ok")
          < instr_text.index("mode_of(&ins.action)"),
          "platform_ok 检查必须排在 mode_of 之前")
    # 构建约束判定必须照 go/build/constraint 的边界。**不能把断言锚在注释上** ——
    # `code_only()` 会剔掉整行注释（这正是它的用途，T32 实测踩到过）。
    # 所以只锚代码：前缀判定直接作用在未 trim 的 `s` 上，中间没有 `let line = line.trim()`。
    check("构建约束判定照 go/build/constraint：HasPrefix 判未经 trim 的整行",
          's.strip_prefix("//go:build")' in instr_text
          and "let line = line.trim();" not in instr_text)
    # T32 的验收测试在位，且必须含「顺序」与「linkmain.go」两处关键断言
    instr_test = REPO_ROOT / "rgoc/crates/rgoc-harness/tests/test_instruction.rs"
    it_text = instr_test.read_text(encoding="utf-8") if instr_test.is_file() else ""
    check("T32 验收测试在位（含顺序契约与 linkmain.go 真实 fixture）",
          "linkmain.go" in it_text and "Dispatch::TargetFiltered" in it_text
          and "DispatchError::UnknownAction" in it_text)

    # ── T33 的产物：平台过滤（shouldTest）+ 语料枚举 + unsupported 归类 ──────
    corpus_rs = REPO_ROOT / "rgoc/crates/rgoc-harness/src/corpus.rs"
    corpus_text = code_only(corpus_rs.read_text(encoding="utf-8")) if corpus_rs.is_file() else ""
    check("T33 产物：corpus.rs 存在且 lib.rs 声明 pub mod corpus",
          bool(corpus_text) and "pub mod corpus;" in harness_lib)
    # 平台过滤必须在「未知指令」之前判（T29 查出的坑），且要照官方的 tag 语义
    check("平台过滤照官方 tag 语义（ToolTags 只查 goexperiment.* 前缀）",
          'name.starts_with("goexperiment.")' in corpus_text
          and corpus_text.index("goexperiment.") < corpus_text.index("self.goarch"))
    # tag 集合是 go1.27.1 的实测值：ReleaseTags 恰好 27 项（go1.1…go1.27）
    check("CorpusConfig::m0 的 ReleaseTags 是 go1.1…go1.27（27 项）",
          "(1..=27).map(|n| format!(\"go1.{n}\"))" in corpus_text)
    # 枚举器必须暴露 M0 分母这个概念（279 是 T29 冻结值）
    check("枚举器暴露 denominator（分母 == 279 是 T29 冻结口径）",
          "pub denominator: usize" in corpus_text
          and "pub fn in_denominator(&self)" in corpus_text)
    # U7 只收 v0 集内的用例（冻结归因表），不能被非 v0 模式抢走
    check("U7 归因带 is_v0_supported 守卫（冻结口径：U7 只收 v0 集内）",
          "Some(m) if m.is_v0_supported() => Some(Unsupported" in corpus_text)
    # T33 的验收测试在位，且必须含「分母 == 279」这条交叉校验
    corpus_test = REPO_ROOT / "rgoc/crates/rgoc-harness/tests/test_corpus.rs"
    ct_text = corpus_test.read_text(encoding="utf-8") if corpus_test.is_file() else ""
    check("T33 验收测试在位（含分母 == 279 的交叉校验与语料缺失时的硬失败）",
          "Some(279)" in ct_text
          and "语料目录不存在" in ct_text
          and "RGOC_CORPUS_TEST_DIR" in ct_text)

    # ── T34 的产物：oracle 调用与版本守门 ───────────────────────────────
    oracle_rs = REPO_ROOT / "rgoc/crates/rgoc-harness/src/oracle.rs"
    or_text = code_only(oracle_rs.read_text(encoding="utf-8")) if oracle_rs.is_file() else ""
    check("T34 产物：oracle.rs 存在且 lib.rs 声明 pub mod oracle",
          bool(or_text) and "pub mod oracle;" in harness_lib)
    # 版本守门：建 oracle 前先校验 go version，不符即 Err（T-H-06）
    check("版本守门：Oracle::new 先查 go version 再建实例（T-H-06）",
          "probe_version(&cfg.go_tool" in or_text
          and "VersionMismatch {" in or_text)
    # **超时判断必须拿 now 比 deadline**：`start >= deadline` 恒为 false（T34 实测踩过，
    # 30 秒的 sleep 跑满全程都没触发超时）
    check("超时判断用 Instant::now() >= deadline（不是恒假的 start >= deadline）",
          "Instant::now() >= deadline" in or_text
          and "if start >= deadline" not in or_text)
    # 命令形态：errorcheck 必须是 go tool compile + -C + R5 的 ssa/check（T30 踩过 go build 的坑）
    check("命令形态：errorcheck 带 -C 与 -d=ssa/check/on（不是 go build）",
          '"-C".into()' in or_text and '"-d=ssa/check/on".into()' in or_text)
    # run 层走 fast path（compile -> link -> 直跑 exe），超时时 kill 父进程即杀掉被测程序，
    # 不需要 unsafe 的 kill(-pgid) —— 而 workspace 是 unsafe_code = "forbid"
    check("run 层走 fast path 三步（超时不留孤儿，且不需要 unsafe）",
          "Mode::Run => vec![" in or_text and '"link".into()' in or_text
          and "unsafe" not in or_text)
    # T34 的验收测试在位：必须真的调 go1.27.1（版本守门与超时回收对 mock 没有意义）
    oracle_test = REPO_ROOT / "rgoc/crates/rgoc-harness/tests/test_oracle.rs"
    ot_text = oracle_test.read_text(encoding="utf-8") if oracle_test.is_file() else ""
    check("T34 验收测试在位（含版本拒绝 / 超时回收 / RSS 上限 / 合并流）",
          "版本_不符时_拒绝建oracle" in ot_text
          and "子进程被回收" in ot_text
          and "rss_超上限" in ot_text
          and "合并流" in ot_text)

    # ── T35 的产物：比较器（R2 / R3 / R4）───────────────────────────────
    cmp_rs = REPO_ROOT / "rgoc/crates/rgoc-harness/src/compare.rs"
    cmp_text = code_only(cmp_rs.read_text(encoding="utf-8")) if cmp_rs.is_file() else ""
    check("T35 产物：compare.rs 存在且 lib.rs 声明 pub mod compare",
          bool(cmp_text) and "pub mod compare;" in harness_lib)
    # R2：缺 .out 即期望为空（**不是**「任意输出都通过」），且两种措辞能区分
    check("R2：缺 .out 时期望为空，且两种不匹配措辞不同",
          "Option<&str>" in cmp_text and "本应为空" in cmp_text and "不匹配" in cmp_text)
    # R3：制表符续行必须拼进上一条（诊断的多行补充全靠它）
    check("R3：制表符开头的行拼进上一条诊断",
          "strip_prefix('\\t')" in cmp_text or "strip_prefix('\t')" in cmp_text)
    # R4：同行多引号 ⇒ 多条期望；LINE±n 折算；//// 禁用
    check("R4：一行多引号产生多条期望 + LINE 折算 + 四斜杠禁用",
          "quoted_patterns" in cmp_text
          and "substitute_line" in cmp_text
          and '"////"' in cmp_text)
    # **正则子集的关键性质**：不支持的构造必须**报错**，不能静默当成不匹配 ——
    # 语料里 5435 条模式用到 {n,m}/[]/+/?/^/$，静默不匹配会把它们全误判成「编译器有 bug」
    # ⚠️ 断言必须锚在**真的会 return Err** 的那一句上。
    # 第一版只查「UnsupportedRegex 类型存在 + 元字符字面量存在」，结果把
    # `return Err(unsupported(c))` 换成静默 `lit.push(c)` 也照样显示 ✓ —— 恒真断言。
    check("正则子集：不支持的构造明确报错（不是静默不匹配）",
          "UnsupportedRegex" in cmp_text
          and "return Err(unsupported(c))" in cmp_text
          and all(f"'{c}'" in cmp_text for c in "+?[](){}"))
    # errorCheck：未命中的诊断放回池子、剩余判 Unmatched（官方 :1272 / :1281-1304）
    check("errorCheck：未命中放回池子 + 剩余判 Unmatched Errors",
          "pool.push(msg)" in cmp_text and "Unmatched Errors" in cmp_text)
    # T35 的验收测试在位
    cmp_test = REPO_ROOT / "rgoc/crates/rgoc-harness/tests/test_compare.rs"
    ct2_text = cmp_test.read_text(encoding="utf-8") if cmp_test.is_file() else ""
    check("T35 验收测试在位（续行拼接 / 多引号 / LINE 折算 / 正则子集拒绝）",
          all(k in ct2_text for k in
              ("续行拼到上一条", "一行多个引号", "line_占位符", "不支持的构造")))

    # ── T36：六类自测（E3 门禁）─────────────────────────────────────────
    run_rs = REPO_ROOT / "rgoc/crates/rgoc-harness/src/runner.rs"
    run_text = code_only(run_rs.read_text(encoding="utf-8")) if run_rs.is_file() else ""
    check("T36 产物：runner.rs 存在且 lib.rs 声明 pub mod runner",
          bool(run_text) and "pub mod runner;" in harness_lib)
    # 端到端顺序不可颠倒：平台过滤必须排在 dispatch 之前（R1b，T29 查出的坑）
    # ⚠️ 用 find() 而不是 index()：锚点缺失时 index() 会**抛异常**，
    # 脚本崩掉既不是干净的 ✗、也容易被误当成「检查没跑」。-1 要显式判掉。
    _f = run_text.find("should_test(header_of")
    _d = run_text.find("dispatch(&ins, true)")
    check("runner：平台过滤先于指令判定（R1b 顺序契约）",
          0 <= _f < _d, f"should_test@{_f} 必须早于 dispatch@{_d}")
    # 「harness 能力不足」与「真的不匹配」必须分开判
    # 「harness 能力不足」必须排在「语义失败」之前判 —— 顺序反了就会把
    # 「正则子集不认识」误报成「编译器有 bug」
    _u = run_text.find("UNSUPPORTED-REGEX")
    _c = run_text.find("errs.join")
    check("runner：UNSUPPORTED-REGEX 判 harness-failure 而不是 compiler-failure",
          _u > 0 and _c > _u, f"UNSUPPORTED-REGEX@{_u} 必须在语义失败分支@{_c} 之前")
    # 六类自测：六条 ID 齐备 + 每类都有反例
    hst = REPO_ROOT / "rgoc/crates/rgoc-harness/tests/harness_self_test.rs"
    hst_text = hst.read_text(encoding="utf-8") if hst.is_file() else ""
    six = [f"th0{i}" for i in range(1, 7)]
    check("E3 六类自测齐备（T-H-01~06 各有测试）",
          all(any(f"fn {k}" in line for line in hst_text.splitlines()) for k in six),
          "缺：" + ", ".join(k for k in six if not any(f"fn {k}" in l for l in hst_text.splitlines())))
    check("E3 六类自测**每类都有反例**（M0-plan §8 执行纪律）",
          hst_text.count("反例") >= 4 and "仍计入分母但不计入分子" in hst_text
          and "版本不符_整层判" in hst_text)
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

    # ── 5d. T39：门禁登记与**可重放产物**一致 ──────────────────────────────
    # 动机：T38 登记 gate.E4 时，证据是「人写的 JSON 字段」。若报告文件被重跑、
    # 被手改、或与 manifest 脱节，登记就变成一句无法核验的话。
    # 这里把 E4 的报告当**可执行证据**：从报告读回数字，与 manifest 对撞。
    # 报告是 driver/xtask 生成的，所以这条断言同时也在验证「生成器没坏」。
    print("[5d] T39 门禁登记与 E4 报告一致（可重放证据）")
    rep_path = REPO_ROOT / "rgoc/tests/corpus/T-C-report.json"
    if not rep_path.is_file():
        check("E4 报告 JSON 存在（缺则无法复核登记）", False, str(rep_path))
    else:
        rep = json.loads(rep_path.read_text(encoding="utf-8"))
        e4 = manifest["gate"]["E4"]
        # ① 报告的三个核心数与 manifest 登记的一致
        check("E4 报告的 20/20 == manifest 登记",
              rep["denominator"] == e4["denominator"] == 20
              and rep["numerator"] == e4["numerator"] == 20,
              f"报告 {rep['numerator']}/{rep['denominator']}，"
              f"登记 {e4['numerator']}/{e4['denominator']}")
        # ② 报告自称 success ⇒ 八类里除 pass 外全 0（**不得靠排除凑数**）
        nonzero = {k: v for k, v in rep["by_verdict"].items() if v and k != "pass"}
        check("E4 报告 success=true 且非 pass 判定全为 0",
              rep["success"] is True and nonzero == {},
              f"success={rep['success']}，非 pass 分布={nonzero or '空'}")
        # ③ 分母不被缩小：20 条逐条列出，且全量分母仍是 279
        check("E4 报告逐条列出 20 条（分母未缩小）",
              len(rep["cases"]) == 20, f"cases={len(rep['cases'])} 条")
        # ④ 峰值 RSS 与耗时都在（E4 的登记要求含这两项）
        check("E4 报告含峰值 RSS 与整层耗时（E4 的登记要求）",
              rep["peak_rss_bytes"] > 0 and rep["total_duration_ms"] > 0,
              f"peak_rss={rep['peak_rss_bytes'] // 1024 // 1024} MiB，"
              f"total={rep['total_duration_ms']} ms")
        # ⑤ 峰值 RSS 必须在 M0-tests §7.5 的预算内（512 MiB / 单用例）
        rss_mib = rep["peak_rss_bytes"] / 1024 / 1024
        check("E4 峰值 RSS 在 512 MiB 预算内",
              0 < rss_mib <= 512, f"{rss_mib:.0f} MiB（上限 512）")
        # ⑥ 逐条 RSS 也不超（整层取 max，逐条要各自看）
        worst = max((c["peak_rss_bytes"] for c in rep["cases"]), default=0) / 1024 / 1024
        check("E4 逐用例峰值 RSS 都在 512 MiB 预算内",
              worst <= 512, f"最坏 {worst:.0f} MiB")
        # ⑦ 登记里声明的两条反向校验（T38 实测过）必须在案
        check("E4 登记了反向校验证据（判定不是恒真）",
              len(e4.get("negative_checks", [])) >= 2,
              f"{len(e4.get('negative_checks', []))} 条")
        # ⑧ 「修了 harness 而非登记新 U」的理由必须在案 —— T38 的核心判定。
        # ⚠️ 判据要按**字段实际内容**写：`root_cause` 里有 `compare.rs`（定位到文件）、
        # `why_fix_not_new_u` 里有「修 harness」（明确不登记新 U）。
        # 早先写成查「放宽判定」—— 那句话在 `discipline` 节而不在 `fix` 节里，
        # 断言恒红。**断言必须锚定真实字段，不能凭印象写关键词。**
        fix = e4.get("fix_required_to_pass", {})
        fix_text = json.dumps(fix, ensure_ascii=False)
        check("E4 登记了 T-C-13 的根因与修法（修 harness 而非登记新 U）",
              "compare.rs" in fix_text and "修 harness" in fix_text
              and "why_fix_not_new_u" in fix,
              f"root_cause 片段：{str(fix.get('root_cause'))[:40]}…")
        # ⑧b 判定纪律本身必须在案（不得通过放宽比较器 / 记跳过为 pass / 缩小分母）
        check("E4 登记了判定纪律四条（未放宽、未记 pass、未缩分母、未削减样本）",
              all(k in e4.get("discipline", "")
                  for k in ("放宽比较器", "记为 pass", "缩小分母", "未削减样本数")),
              "discipline 节")
        # ⑨ 跨行正则必须真的有守卫测试（否则下次重构会静默回归）
        guards = " ".join(fix.get("guards", []))
        check("E4 登记了跨行正则的守卫测试（含变异测试）",
              "t38_跨行正则" in guards and "变异测试" in guards,
              f"{len(fix.get('guards', []))} 条守卫")
        # ⑩ 人读报告里必须含全量 279 分母一节（证明分母没被缩小的第二处证据）
        md = (REPO_ROOT / "rgoc/tests/corpus/T-C-report.md").read_text(encoding="utf-8")
        check("E4 人读报告含全量 279 分母一节",
              "279" in md and "全量语料基线" in md and "U7" in md,
              "含 279 / 全量语料基线 / U 归类表")

    # ── 5e. T39：phase_plan 与实际产物一致 ─────────────────────────────────
    # 动机：`phase_plan.phase2.done` 是「做了哪些任务」的登记位。T38 只补了
    # phase2 的 E4 证据，**忘了把 T37/T38 写进 done 列表** —— 于是 manifest
    # 一边说 gate.E4 pass，一边列出的 done 只到 T36。这类漂移不报错，只是
    # 让「登记」失去意义（读的人会以为 T37/T38 没做）。
    print("[5e] T39 phase_plan 与实际产物一致")
    pp = manifest.get("phase_plan", {}).get("phase2", {})
    done_text = " ".join(pp.get("done", []))
    check("phase_plan.phase2.done 含 T29–T38 全部任务",
          all(f"T{n}" in done_text for n in range(29, 39)),
          f"done 列了 {len(pp.get('done', []))} 条")
    check("phase_plan.phase2 状态 == done（E3/E4 已全过）",
          pp.get("status") == "done", f"status={pp.get('status')}")
    # 登记的三个 crate 必须真的在磁盘上（登记与 workspace 脱节也要抓）
    crates = pp.get("crates", [])
    check("phase_plan.phase2 登记的 crate 都在磁盘上",
          len(crates) == 3
          and all((REPO_ROOT / "rgoc" / c).is_dir() for c in crates),
          f"{crates}")
    # 登记的报告文件必须存在
    check("phase_plan.phase2 登记的报告文件都存在",
          all((REPO_ROOT / p).is_file() for p in pp.get("reports", [])),
          f"{pp.get('reports')}")
    # 登记的测试数必须与 E3 的 total_tests 一致（两处都写 153 就不会互相矛盾）
    gr = pp.get("gate_result", {})
    check("phase_plan 的测试数 == gate.E3.total_tests",
          gr.get("tests_passed") == manifest["gate"]["E3"].get("total_tests") == 153,
          f"phase_plan {gr.get('tests_passed')} / "
          f"E3 {manifest['gate']['E3'].get('total_tests')}")
    # E4 门禁要求的耗时预算，逐条在案（§7.5：整层 5 min / 单项 60 s）
    check("E4 整层耗时在 300 s 预算内（03 §6.2 smoke ≤ 5 min）",
          0 < manifest["gate"]["E4"].get("layer_wall_time_s", 1e9) <= 300,
          f"{manifest['gate']['E4'].get('layer_wall_time_s')} s")
    check("E4 单项最慢在 60 s 预算内（M0-tests §7.5）",
          0 < manifest["gate"]["E4"].get("slowest_case_s", 1e9) <= 60,
          f"{manifest['gate']['E4'].get('slowest_case_s')} s")
    # Phase 3 的阻塞条件必须已解除（D-M0-2）
    check("phase_plan.phase3 的前置阻塞已标记解除",
          "已全部通过" in manifest["phase_plan"]["phase3"].get("blocked_by", ""),
          manifest["phase_plan"]["phase3"].get("blocked_by", "(缺失)")[:40])

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
