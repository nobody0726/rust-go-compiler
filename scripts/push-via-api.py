#!/usr/bin/env python3
"""`git push` 到 github.com 被网络拦住时的备用推送路径（Git Data API）。

**为什么有这个脚本**：`github.com:443` 在本机常被会话代理拦（`git push` 报
`CONNECT tunnel failed, response 502` 或 `Recv failure: Operation timed out`），
而 `api.github.com` 正常（`AGENTS.md` §6.2）。`AGENTS.md` 原先指向的
`~/.workbuddy/skills/github-push-via-api/push_via_api.py` 在本机**并不存在**
⇒ 这份脚本按同一原理实现，并入 `scripts/`（不依赖任何第三方包）。

**它与“随便造一个提交”的区别**：逐个提交**重建对象并校验 SHA**

  本地树 → `POST /git/trees`（`base_tree` = 父提交的树）→ **要求 API 回的 sha
  与本地 `git rev-parse <c>^{tree}` 逐字节相同**；
  本地提交元数据（tree / parents / author / committer / message）→
  `POST /git/commits` → **要求 API 回的 sha 与本地提交 sha 相同**。

全过 ⇒ 远端 `main` 与本地 `main` 是**同一个提交对象**（不是“内容看起来一样”），
后续 `git push` 也不会再造重复提交。任一处不符 ⇒ 立刻中止并报出是哪一步。
所有对象都在**最后一步**才把 ref 指过去；中途失败 ⇒ 远端不受影响（最多留下游离对象）。

用法：
    python3 scripts/push-via-api.py [--repo OWNER/NAME] [--branch main] [--dry-run]

  --repo    缺省从 `git remote get-url origin` 推（支持 https 与 ssh 形态）
  --branch  缺省 main
  --dry-run 只解析与打印，一次写 API 都不发
"""
from __future__ import annotations

import argparse
import base64
import json
import subprocess
import sys
import time
import urllib.error
import urllib.request
from datetime import datetime, timedelta, timezone

API = "https://api.github.com"


def git(*args: str, binary: bool = False):
    out = subprocess.run(("git",) + args, check=True, capture_output=True).stdout
    return out if binary else out.decode("utf-8")


def origin_repo() -> str:
    url = git("remote", "get-url", "origin").strip()
    for pat in ("https://github.com/", "git@github.com:", "ssh://git@github.com/"):
        if pat in url:
            return url.split(pat, 1)[1].removesuffix(".git").strip("/")
    raise SystemExit(f"从 origin 推不出 OWNER/NAME（{url}）—— 用 --repo 指定")


def gh_token() -> str:
    return subprocess.run(
        ("gh", "auth", "token"), check=True, capture_output=True, text=True
    ).stdout.strip()


class Client:
    def __init__(self, repo: str, dry: bool):
        self.repo = repo
        self.dry = dry
        self.calls = 0
        self.token = gh_token()

    def __call__(self, path, method="GET", payload=None):
        if self.dry and method != "GET":
            self.calls += 1
            return {"sha": "dry", "object": {"sha": "dry"}}
        data = json.dumps(payload).encode() if payload is not None else None
        last = None
        for attempt in range(6):
            req = urllib.request.Request(API + path, data=data, method=method)
            req.add_header("Authorization", "Bearer " + self.token)
            req.add_header("Accept", "application/vnd.github+json")
            if data is not None:
                req.add_header("Content-Type", "application/json")
            try:
                with urllib.request.urlopen(req, timeout=90) as r:
                    self.calls += 1
                    return json.loads(r.read().decode() or "{}")
            except urllib.error.HTTPError as e:
                body = e.read().decode(errors="replace")
                if e.code in (500, 502, 503, 504, 403):  # 5xx / 二级限流
                    last = f"HTTP {e.code}: {body[:200]}"
                    time.sleep(2 * (attempt + 1))
                    continue
                raise SystemExit(f"HTTP {e.code} on {method} {path}\n{body[:800]}")
            except Exception as e:  # 连接层
                last = repr(e)
                time.sleep(2 * (attempt + 1))
        raise SystemExit(f"重试耗尽：{method} {path} :: {last}")


def parse_commit(sha: str) -> dict:
    """解析裸提交对象（tree / parents / author / committer / message）。"""
    head, _, msg = git("cat-file", "-p", sha).partition("\n\n")
    tree, parents, who = None, [], {}
    for line in head.split("\n"):
        if line.startswith("tree "):
            tree = line[5:]
        elif line.startswith("parent "):
            parents.append(line[7:])
        elif line.startswith(("author ", "committer ")):
            kind, rest = line.split(" ", 1)
            ident, ts, tz = rest.rsplit(" ", 2)
            name, email = ident.rsplit(" <", 1)
            dt = datetime.fromtimestamp(
                int(ts), tz=timezone(timedelta(hours=int(tz[:3]), minutes=int(tz[3:5])))
            )
            who[kind] = {
                "name": name,
                "email": email.rstrip(">"),
                "date": dt.strftime("%Y-%m-%dT%H:%M:%S") + tz[:3] + ":" + tz[3:5],
            }
    return {
        "tree": tree,
        "parents": parents,
        "author": who.get("author"),
        "committer": who.get("committer"),
        "message": msg if msg.endswith("\n") else msg + "\n",
    }


def changed_entries(parent: str | None, sha: str) -> list[dict]:
    """相对第一父的改动。**关掉改名检测** ⇒ 每条记录只有一个路径。"""
    args = ["diff-tree", "-r", "--raw", "-z", "--no-renames"]
    if parent is None:
        args += ["--root", sha]
    else:
        args += ["--no-commit-id", parent, sha]
    fields = git(*args).split("\0")
    entries, i = [], 0
    while i < len(fields):
        meta = fields[i]
        if not meta.startswith(":"):
            i += 1
            continue
        src_mode, dst_mode, _src_sha, dst_sha, status = meta.split(" ")
        src_mode = src_mode.lstrip(":")
        i += 1
        path = fields[i]
        i += 1
        if status.startswith("D"):
            entries.append({"path": path, "mode": src_mode, "type": "blob", "sha": None})
        else:
            entries.append({"path": path, "mode": dst_mode, "type": "blob", "sha": dst_sha})
    return entries


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--repo", default=None, help="OWNER/NAME（缺省取 origin）")
    ap.add_argument("--branch", default="main")
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()

    repo = args.repo or origin_repo()
    api = Client(repo, args.dry_run)

    head = git("rev-parse", "HEAD").strip()
    ref = api(f"/repos/{repo}/git/ref/heads/{args.branch}")["object"]["sha"]
    print(f"repo      = {repo}\nbranch    = {args.branch}")
    print(f"远端 ref  = {ref}\n本地 HEAD = {head}")

    if ref == head:
        print("\n远端已指向本地 HEAD —— 无需推送。")
        return
    if subprocess.run(
        ("git", "merge-base", "--is-ancestor", ref, head), capture_output=True
    ).returncode != 0:
        raise SystemExit(
            f"远端 {ref[:10]} 不是本地 HEAD 的祖先（本地 ref 可能陈旧 / 远端有新提交）。\n"
            "先 `git fetch origin` 再看；fetch 也被拦时用：\n"
            f"  gh api repos/{repo}/git/ref/heads/{args.branch} --jq .object.sha"
        )

    shas = git("rev-list", "--reverse", "--topo-order", f"{ref}..{head}").split()
    merges = git("rev-list", "--merges", f"{ref}..{head}").split()
    if merges:
        raise SystemExit(f"区间内含合并提交 —— 本脚本只重放线性历史：{merges}")
    print(f"\n待重放：{len(shas)} 个提交\n")

    blobs: dict[str, str] = {}
    mapping = {ref: ref}
    for n, sha in enumerate(shas, 1):
        c = parse_commit(sha)
        parent = c["parents"][0] if c["parents"] else None
        payload: dict = {"tree": changed_entries(parent, sha)}
        for e in payload["tree"]:
            if e["sha"] is None:
                continue
            if e["sha"] not in blobs:
                body = git("cat-file", "blob", e["sha"], binary=True)
                if args.dry_run:
                    blobs[e["sha"]] = e["sha"]
                else:
                    got = api(
                        f"/repos/{repo}/git/blobs",
                        "POST",
                        {
                            "content": base64.b64encode(body).decode(),
                            "encoding": "base64",
                        },
                    )["sha"]
                    if got != e["sha"]:
                        raise SystemExit(f"blob sha 不符：{e['sha']} vs {got}")
                    blobs[e["sha"]] = got
            e["sha"] = blobs[e["sha"]]
        if parent:
            payload["base_tree"] = git("rev-parse", f"{mapping[parent]}^{{tree}}").strip()
        local_tree = git("rev-parse", f"{sha}^{{tree}}").strip()
        tree_sha = (
            local_tree
            if args.dry_run
            else api(f"/repos/{repo}/git/trees", "POST", payload)["sha"]
        )
        if tree_sha != local_tree:
            raise SystemExit(f"tree sha 不符（{sha[:10]}）：{local_tree} vs {tree_sha}")
        commit_payload = {
            "message": c["message"],
            "tree": tree_sha,
            "parents": [mapping[p] for p in c["parents"]],
            "author": c["author"],
            "committer": c["committer"],
        }
        new_sha = (
            sha
            if args.dry_run
            else api(f"/repos/{repo}/git/commits", "POST", commit_payload)["sha"]
        )
        if new_sha != sha:
            raise SystemExit(f"commit sha 不符：本地 {sha} vs API {new_sha}")
        mapping[sha] = new_sha
        print(f"  [{n:2d}/{len(shas)}] {new_sha[:10]}  {c['message'].splitlines()[0][:68]}")

    if args.dry_run:
        print(f"\n（dry-run）API 调用 {api.calls} 次；未改动远端。")
        return
    api(
        f"/repos/{repo}/git/refs/heads/{args.branch}",
        "PATCH",
        {"sha": mapping[head], "force": False},
    )
    same = mapping[head] == head
    print(f"\n远端 {args.branch} → {mapping[head]}（本地 HEAD = {head}）")
    print(f"API 调用 {api.calls} 次；比对：{'一致（同一个提交对象）' if same else '不一致！'}")
    if not same:
        sys.exit(1)


if __name__ == "__main__":
    main()
