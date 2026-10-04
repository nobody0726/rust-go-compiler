#!/usr/bin/env python3
"""生成 rgoc 测试基础设施的四层架构图（输入 / 编排 / 存储 / 输出）。"""

import json
import pathlib

OUT = pathlib.Path(__file__).with_name("rgoc-harness-four-layer-architecture.excalidraw")

elements = []
_id = [0]


def _next(prefix):
    _id[0] += 1
    return f"{prefix}{_id[0]}"


def base(eid, etype, x, y, w, h, **kw):
    el = {
        "id": eid,
        "type": etype,
        "x": x,
        "y": y,
        "width": w,
        "height": h,
        "strokeColor": kw.get("strokeColor", "#1F2937"),
        "backgroundColor": kw.get("backgroundColor", "transparent"),
        "fillStyle": "solid",
        "strokeWidth": kw.get("strokeWidth", 1),
        "strokeStyle": kw.get("strokeStyle", "solid"),
        "roughness": 0,
        "opacity": 100,
        "angle": 0,
        "seed": 1000 + _id[0] * 7,
        "version": 1,
        "versionNonce": 2000 + _id[0] * 13,
        "isDeleted": False,
        "boundElements": kw.get("boundElements", None),
        "updated": 0,
        "link": None,
        "locked": False,
    }
    return el


def rect(x, y, w, h, stroke, fill, sw=2, ss="solid"):
    eid = _next("rect_")
    elements.append(base(eid, "rectangle", x, y, w, h,
                         strokeColor=stroke, backgroundColor=fill,
                         strokeWidth=sw, strokeStyle=ss, boundElements=[]))
    return eid


def ellipse(x, y, w, h, stroke, fill, sw=2):
    eid = _next("dot_")
    elements.append(base(eid, "ellipse", x, y, w, h,
                         strokeColor=stroke, backgroundColor=fill,
                         strokeWidth=sw, boundElements=[]))
    return eid


def text(x, y, w, s, size=14, color="#111827", ff=2, align="left"):
    lines = s.split("\n")
    h = len(lines) * size * 1.25
    eid = _next("txt_")
    elements.append({
        **base(eid, "text", x, y, w, h, strokeColor=color),
        "text": s,
        "fontSize": size,
        "fontFamily": ff,
        "textAlign": align,
        "verticalAlign": "top",
        "containerId": None,
        "originalText": s,
        "lineHeight": 1.25,
    })
    return eid


def arrow(x, y, w, h, color="#475467", sw=2):
    eid = _next("arr_")
    el = base(eid, "arrow", x, y, w, h,
              strokeColor=color, strokeWidth=sw)
    el["points"] = [[0, 0], [w, h]]
    el["lastCommittedPoint"] = None
    el["startBinding"] = None
    el["endBinding"] = None
    el["startArrowhead"] = None
    el["endArrowhead"] = "arrow"
    elements.append(el)
    return eid


# ---------------------------------------------------------------- 标题
text(60, 40, 900, "rgoc 测试基础设施 · 四层架构", 28)
text(60, 84, 1200,
     "论点：口径逐层收窄，判定权集中在存储层 —— 356 个文件 → 279 分母 → 20 样本 → 只有 pass 进分子；输出层只做呈现，不做计算。",
     15, "#667085")

# ---------------------------------------------------------------- 四层定义
LAYERS = [
    dict(
        key="in", y=130, label="输入层", en="INPUT",
        fill="#ECFDF3", stroke="#027A48",
        note="外部世界：冻结规格、官方语料、被信任的参考工具链",
        cols=[
            ("冻结清单 M0-tests.md", "F1 20 样本 / F2 U1–U14", "F4 M0 分母 = 279（T29）"),
            ("官方语料（只读）", "go_source_code/test/ · 356 文件", "sha256 清单锁 15,618 条"),
            ("指令行与期望文件", "// run  // errorcheck  // compile", "16 个指令 · .out / ERROR 行"),
            ("oracle 工具链（外部）", "go1.27.1 · linux/arm64", "版本不符即拒作基线"),
        ],
    ),
    dict(
        key="orch", y=430, label="编排层", en="ORCHESTRATION",
        fill="#F4F3FF", stroke="#6941C6",
        note="唯一的执行面：把一份文件变成一条判定流水线，全程无状态",
        cols=[
            ("instruction.rs", "parse_action() → dispatch()", "R1b：:522 过滤先于 :541 switch"),
            ("corpus.rs", "should_test() · enumerate()", "ToolTags 只查 goexperiment.* 前缀"),
            ("oracle.rs", "run_mode() · argv_for()", "R6：go tool compile，非 go build"),
            ("compare.rs", "error_check() · unanchored_match()", "R2b：stdout+stderr 合并再比"),
        ],
    ),
    dict(
        key="store", y=730, label="存储层", en="STATE",
        fill="#F0F9FF", stroke="#026AA2",
        note="唯一状态载体：预算、期望、判定都在这里定死，编排层不许自己拍",
        cols=[
            ("TestCase（ir.rs）", "16 个必录字段一项不落", "id / mode / expected / limits …"),
            ("Limits::for_layer()", "冻结预算的唯一入口", "with_rss_override 防「顺手调大」"),
            ("Verdict（八种不得合并）", "Pass / RuntimeFailure / Timeout …", "ReferenceToolchainFailure 也在内"),
            ("TestCase::validate()", "IrError 六种", "先确认失败来自缺失行为而非环境"),
        ],
    ),
    dict(
        key="out", y=1030, label="输出层", en="OUTPUT",
        fill="#EFF8FF", stroke="#175CD3",
        note="呈现与门禁：只消费存储层已定的判定，不重新解释一遍",
        cols=[
            ("分子 / 分母", "counts_toward_numerator()", "只有 pass；过滤项不从分母拿掉"),
            ("门禁 E1–E10", "M0-manifest.json 的 gate", "E1 / E2 / E10 / E5 已 pass"),
            ("能力边界自陈", "UNSUPPORTED-REGEX: 前缀", "harness-failure ≠ 编译器有 bug"),
            ("报告与验收测试", "tests/*.rs 共 78 条", "11+14+15+12+26，RED→GREEN 留痕"),
        ],
    ),
]

COL_X = [84, 466, 848, 1230]
COL_W = 366
BAND_W = 1580
BAND_H = 196

for L in LAYERS:
    rect(60, L["y"], BAND_W, BAND_H, L["stroke"], L["fill"], sw=2)
    ellipse(84, L["y"] + 18, 18, 18, L["stroke"], "#FFFFFF", sw=2)
    text(114, L["y"] + 14, 300, f'{L["label"]}  {L["en"]}', 19, L["stroke"])
    text(114, L["y"] + 44, 1200, L["note"], 13, "#667085")
    for i, (name, l1, l2) in enumerate(L["cols"]):
        cx = COL_X[i]
        yy = L["y"] + 80
        text(cx, yy, COL_W, name, 16, "#111827")
        text(cx, yy + 30, COL_W, l1, 13, "#475467", ff=3)
        text(cx, yy + 52, COL_W, l2, 13, "#667085", ff=3)

# ---------------------------------------------------------------- 层间箭头 + 说明
GAPS = [
    (320, 430, "解析 → 平台过滤 → switch：顺序被写进函数签名，颠倒在类型层面就不可能"),
    (620, 730, "汇总为 TestCase：编排层不保存任何状态，用完即弃"),
    (920, 1030, "每条用例产出且仅产出一个 Verdict，输出层不得改写"),
]
for top, bottom, label in GAPS:
    arrow(340, top + 8, 0, bottom - top - 16, "#475467")
    arrow(1300, top + 8, 0, bottom - top - 16, "#475467")
    text(380, (top + bottom) / 2 - 11, 880, label, 13, "#667085")

# ---------------------------------------------------------------- 口径收窄轨
text(60, 1268, 400, "口径逐层收窄", 15, "#111827")
rail = [
    (84, "356 文件", "corpus::enumerate"),
    (400, "279 分母", "T29 冻结值 = 枚举实算值"),
    (716, "20 样本", "F1 · E4 的验证范围"),
    (1032, "分子 = pass", "counts_toward_numerator"),
]
for x, big, small in rail:
    text(x, 1300, 300, big, 18, "#111827")
    text(x, 1326, 300, small, 12, "#667085", ff=3)
for x in (240, 556, 872):
    arrow(x, 1312, 130, 0, "#475467")

# ---------------------------------------------------------------- 证据块
rect(60, 1400, 780, 196, "#344054", "#101828", sw=1)
text(84, 1422, 740,
     "// R1b 顺序契约写进了函数签名（instruction.rs:305）\n"
     "dispatch(ins: &Instruction, platform_ok: bool) -> Result<Dispatch, DispatchError>\n"
     "// platform_ok 必填 —— 官方 :522 平台过滤先于 :541 switch\n"
     "// 三出口：Proceed(Mode) / TargetFiltered / SkippedByDesign",
     13, "#86EFAC", ff=3)

rect(880, 1400, 760, 196, "#344054", "#101828", sw=1)
text(904, 1422, 720,
     "// 只有 pass 计入分子 —— 门禁纪律 03 §3.3（ir.rs:498）\n"
     "Verdict::ALL: [Self; 8] = [Pass, CompilerFailure, RuntimeFailure,\n"
     "    HarnessFailure, TargetFiltered, Timeout,\n"
     "    ResourceFailure, ReferenceToolchainFailure]\n"
     "fn counts_toward_numerator(self) -> bool { self == Self::Pass }",
     13, "#86EFAC", ff=3)

doc = {
    "type": "excalidraw",
    "version": 2,
    "source": "https://excalidraw.com",
    "elements": elements,
    "appState": {"gridSize": None, "viewBackgroundColor": "#FFFFFF"},
    "files": {},
}
OUT.write_text(json.dumps(doc, ensure_ascii=False, indent=2), encoding="utf-8")
print(f"wrote {OUT}  ({len(elements)} elements)")
