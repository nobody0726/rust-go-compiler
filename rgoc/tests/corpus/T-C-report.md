# T-C 官方语料基线报告（E4 门禁）

> 由 `cargo run -p rgoc-driver -- harness report` 生成（可重放）。
> 生成时间：2026-10-05　oracle：go1.27.1 linux/arm64　目标：aarch64-unknown-linux-gnu / ELF

═══════════════════════════════════════════
 rgoc harness 执行报告
═══════════════════════════════════════════
分母 = 20　分子 = 20
判定分布（八类，缺失的按 0 计）：
  pass                         20
  compiler-failure             0
  runtime-failure              0
  harness-failure              0
  target-filtered              0
  timeout                      0
  resource-failure             0
  reference-toolchain-failure  0
逐用例（20 条）：
整层 wall time = 0.3s　峰值 RSS = 15 MiB
  helloworld.go            pass                           0.027s     13 MiB
  closure1.go              pass                           0.027s     15 MiB
  gc1.go                   pass                           0.027s     13 MiB
  printbig.go              pass                           0.027s     13 MiB
  closure4.go              pass                           0.026s     13 MiB
  func6.go                 pass                           0.024s     15 MiB
  compos.go                pass                           0.028s     15 MiB
  method3.go               pass                           0.024s     13 MiB
  eof.go                   pass                           0.007s     13 MiB
  empty.go                 pass                           0.007s     13 MiB
  parentype.go             pass                           0.006s     13 MiB
  rune.go                  pass                           0.006s     13 MiB
  initloop.go              pass                           0.004s     13 MiB
  recover5.go              pass                           0.005s     13 MiB
  varerr.go                pass                           0.005s     15 MiB
  convlit1.go              pass                           0.005s     13 MiB
  init.go                  pass                           0.005s     13 MiB
  switch4.go               pass                           0.004s     13 MiB
  typecheck.go             pass                           0.004s     13 MiB
  mainsig.go               pass                           0.005s     13 MiB
───────────────────────────────────────────
结论：全部通过
## 全量语料基线（**非门禁**，`M0-tests.md` §8）

E4 门禁只看上面 20 个 T-C 样本。下表是顶层 `test/` 全量枚举的基线，
用来证明**分母没有被缩小** —— 排除项必须能说清落在 §6 的哪一条 U。

| 项 | 值 |
|---|---|
| 顶层 `*.go` 总数 | 356 |
| **M0 分母**（v0 集 ∧ 无排除参数） | **279** |
| 排除项 | 77 |
| 其中被平台过滤（**仍计入分母**） | 5 |
| 实际执行 | 274 |

分母内按模式：

| 模式 | 条数 |
|---|---|
| `run` | 147 |
| `compile` | 12 |
| `errorcheck` | 120 |
| **合计** | **279** |

排除项按 U 归类（`M0-tests.md` §6）：

| U | 条数 |
|---|---|
| **U1** | 5 |
| **U13** | 5 |
| **U14** | 1 |
| **U2** | 14 |
| **U3** | 1 |
| **U5** | 9 |
| **U6** | 11 |
| **U7** | 31 |

