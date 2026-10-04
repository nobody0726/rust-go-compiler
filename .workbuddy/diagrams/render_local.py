from __future__ import annotations
import argparse, json, sys
from pathlib import Path
from playwright.sync_api import sync_playwright

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("input", type=Path)
    ap.add_argument("--output", "-o", type=Path, required=True)
    ap.add_argument("--scale", "-s", type=int, default=2)
    a = ap.parse_args()
    data = json.loads(a.input.read_text(encoding="utf-8"))
    tpl = Path(__file__).with_name("render_template.html").resolve()
    els = [e for e in data["elements"] if not e.get("isDeleted")]
    xs, ys = [], []
    for e in els:
        if e.get("type") in ("arrow", "line") and e.get("points"):
            for px, py in e["points"]:
                xs.append(e["x"] + px); ys.append(e["y"] + py)
        else:
            xs += [e["x"], e["x"] + abs(e.get("width", 0))]
            ys += [e["y"], e["y"] + abs(e.get("height", 0))]
    vw = int(max(xs) - min(xs) + 200); vh = int(max(ys) - min(ys) + 200)
    with sync_playwright() as p:
        b = p.chromium.launch(headless=True)
        pg = b.new_page(viewport={"width": vw, "height": vh}, device_scale_factor=a.scale)
        errs = []
        pg.on("requestfailed", lambda r: errs.append(r.url[:100]))
        pg.goto(tpl.as_uri())
        pg.wait_for_function("window.__moduleReady === true", timeout=60000)
        res = pg.evaluate("payload => window.renderExcalidraw(payload)", data)
        if not res or not res.get("ok"):
            print("RENDER FAIL:", res, file=sys.stderr); b.close(); sys.exit(1)
        pg.wait_for_function("window.__renderDone === true", timeout=30000)
        pg.wait_for_timeout(1200)
        svg = pg.query_selector("#root svg")
        if svg is None:
            print("NO SVG", file=sys.stderr); b.close(); sys.exit(1)
        box = svg.bounding_box()
        need_w, need_h = int(box["width"] + 80), int(box["height"] + 80)
        if need_w > vw or need_h > vh:
            pg.set_viewport_size({"width": max(vw, need_w), "height": max(vh, need_h)})
            pg.wait_for_timeout(600)
            svg = pg.query_selector("#root svg")
        svg.screenshot(path=str(a.output))
        b.close()
    print(a.output, f"viewport={vw}x{vh}")
    if errs: print("REQ FAILED:", errs[:5], file=sys.stderr)

main()
