# ast oracle — 官方 parser 的「标准答案提供者」（M2）

把任意 `.go` 文件用 **vendor 的官方 `cmd/compile/internal/syntax`** 解析成 AST，
再渲染成**中立 dump**，供 `rgoc` 侧的 `rgoc-ast::dump` 做逐行 diff ——
这就是 M2 的 AST 验收判据（`D-M2-1` oracle 对撞）。

> **它不是参考实现，它是标准答案的提供者。** 与 oracle 不一致时，
> **先怀疑我们，不怀疑 oracle**（`M0-design.md` §1.0 第 2 步）。

---

## 1. 为什么必须写这个程序

官方 `syntax` 包**没有**任何现成命令能吐出 AST dump：

| 命令 | 它给的是 | 能不能用 |
|---|---|---|
| `go tool compile -G=3 -d=dump` 等内部开关 | 依赖 gc 现场环境，且格式随时变 | ✗ |
| 官方 `Fdump`（`dumper.go`） | 靠 **reflect** 的调试输出，格式是官方内部形态 | ✗ `M2-design.md` §9 明令不用（`F14`） |
| **本目录的 adapter** | 手写遍历器输出的中立 dump | ✅ |

`dump.go` 是**手写的**：按 `rgoc/crates/rgoc-ast/src/dump.rs` 的分支逐条直译，
两边任何一侧改输出形状都必须同步另一侧（见 §5 对齐纪律）。

## 2. 目录结构

```text
tools/ast-oracle/
├── README.md     本文件：来源、校验值、格式、再生成步骤
├── go.mod        module astoracle（仅标准库；go 1.27）
├── main.go       ★ 我们的：命令行入口（诊断 → stderr，dump → stdout）
├── dump.go       ★ 我们的：官方 AST → 中立格式的遍历器（dump.rs 的镜像）
└── gosyntax/
    ├── LICENSE           ┐ BSD-3-Clause 正文（随附，同 token-oracle）
    ├── branches.go       │
    ├── dumper.go         │
    ├── nodes.go          │
    ├── operator_string.go│
    ├── parser.go         │  官方 syntax 包 **16 个非测试 .go**，
    ├── pos.go            │  **原样复制，零改动**（sha256 见 §3）
    ├── positions.go      │
    ├── printer.go        │
    ├── scanner.go        │
    ├── source.go         │
    ├── syntax.go         │
    ├── testing.go        │
    ├── token_string.go   │
    ├── tokens.go         │
    ├── type.go           │
    └── walk.go           ┘
```

**★ 标记的两个文件是我们写的，其余是官方的。官方文件不得改动。**

### 2.1 为什么**没有** `export.go`（与 M1 的差异）

M1 的 token-oracle 必须加同包 shim，因为 scanner 的**全部**标识符未导出。
ast-oracle 走的全是**导出面**：`ParseFile` / `Parse` / `File` / 全部节点类型 /
`LitKind` / `Operator` / `Mode` —— 实测 `go build` 不需要任何转发。
**不预留空 shim**：一个空文件会让人误以为「这里有东西要转发」。
若将来真碰到未导出标识符（例如要读 `p.pragma`），再按 M1 的模式加。

### 2.2 为什么 vendor 16 个**非测试** `.go`

官方目录还有 7 个 `*_test.go` 与 `testdata/`。测试文件参与 `go test` 的编译，
会把官方 testdata 布局一起拖进来；adapter 只 `go run`/`go build`，
`*_test.go` 根本不参与编译。**不 vendor 它们**：少 7 个永远不会被编译的文件，
README 的校验值表也就少一截要维护的死表。

## 3. 来源与校验值

| 项 | 值 |
|---|---|
| 来源 | `go_source_code/src/cmd/compile/internal/syntax/` |
| 上游 | Go 官方源码树，`VERSION` = **go1.27.1**（同 token-oracle） |
| 容器内 Go | `go version go1.27.1 linux/arm64` |
| 许可 | BSD-3-Clause；许可头原样保留在源文件内，正文随附 `gosyntax/LICENSE`（sha256 `911f8f57…d9dad`，与 token-oracle 同一正本） |

**sha256（与官方原文逐字节一致，已用 `cmp` 校验）**：

```text
e94d78e61926b4df2731e3b9d66b37a97b9287560a18fc32b60b489ba8cb899d  branches.go
f4d27d4b834327604cb7a896cae4461d7abfb66825c5c22f0634da218754d236  dumper.go
f73adbd3d4e37fb7c84edd61e5f481a9989f3f25e9d473170aa827f9b8634039  nodes.go
59946110d83da0391a61e6344865858a18054f76c547ae1fdf29c9c452e1547d  operator_string.go
acbfca2f40fa2022739d35036831566dbcf3656ab2f835f91ef9e51718bdfbe9  parser.go
8aef7e7e79da418db94351539889b421bc414b004820d4af7c044697d1839923  pos.go
c23f2c9d9b418c7e8e66ca68870790f6991dc06e0bf513b9f7964eb5e2362fde  positions.go
d8b6d8e4b51b39191dd623545b506809eff6558136521a7fcd61cb0c5f8abac1  printer.go
eb774e9a58f9d29c0296c29360338e19eb4040977a39ecc5753fd1762a18ee49  scanner.go
d3696fb7dc2f63ebb9aeee224423426ce9f6cf47096029d36aed41763eea7349  source.go
8aece6eed03aa7c6a20a2893d1bd4b999e1d235d336d44259787f0c12de8caf1  syntax.go
aaf13f38493405602a46062b41cc62ff49d349343aa1bd060fa80af971af3353  testing.go
a59da3b0e566bba8dd963f2148ab0c5acbb93d1819886db6ffe7d6e9371be06e  token_string.go
d9667c96873a3e1ae84fce948ef80d269a009a787496f088ab643b14b8bec192  tokens.go
cf198a9e3d8385e14d486aa420586a0584bde44da90a736567c9432a2c71f3bb  type.go
310a5d10ea526bc6db4e3f2dcf6c9d7d5e6c3092234c13eab9ae75c3ade3a80a  walk.go
```

（scanner/source/tokens/token_string/operator_string 五项与
`tools/token-oracle/README.md` §3 记录的值相同 —— 同一批官方文件的两份副本。）

### 再生成步骤（升级语料版本时）

```bash
# 1. 覆盖 16 个文件（不要改内容）
SRC=go_source_code/src/cmd/compile/internal/syntax
for f in branches dumper nodes operator_string parser pos positions printer \
         scanner source syntax testing token_string tokens type walk; do
  cp "$SRC/$f.go" tools/ast-oracle/gosyntax/$f.go
done

# 2. 逐字节确认「零改动」
for f in branches dumper nodes operator_string parser pos positions printer \
         scanner source syntax testing token_string tokens type walk; do
  cmp "$SRC/$f.go" "tools/ast-oracle/gosyntax/$f.go" || echo "!! $f.go 与官方不一致"
done

# 3. 重算校验值并更新本文件的 §3 表格
shasum -a 256 tools/ast-oracle/gosyntax/*.go

# 4. 重跑验收（§6）；dump 形状若变，先判断是官方行为变了还是我们错了
```

## 4. 怎么跑

**必须在容器内**（`scripts/in-container.sh`），且 `cd` 进本目录
（自带 `go.mod`，本仓 G11）：

```bash
RGOC_TTY=0 scripts/in-container.sh bash -lc \
  'cd /work/tools/ast-oracle && go run . /work/go_source_code/src/fmt/print.go'
```

用法与输出：

```text
go run . <file.go>
  stdout: 中立 dump（纯文本，无头部 —— golden 的 `# source: oracle` 标注由
          生成脚本加，格式见 M2-tests.md §5.4）
  stderr: 语法诊断，按发生顺序，`file:line:col: msg`（官方 Pos.String()）
  exit 0  无语法错
  exit 1  有语法错（**dump 照常输出** —— 官方「报错但继续」的口径）
  exit 2  连 File 都没拿到（无正确 package 子句 / io 错误）
```

## 5. dump 格式与对齐纪律

格式权威 = `M2-tests.md` §5.2/§5.3；**逐字节契约 = `rgoc-ast::dump.rs`**
（它已被 rgoc 侧 899 条测试冻结；§5.2 的示例是 C6 草案旧稿，`Expr:`/带引号的
`GoVersion` 均以 dump.rs 实际输出为准：`X:`、不带引号）。要点：

| 规则 | oracle 侧落点（dump.go） |
|---|---|
| 列表标 `[n entries]` | `d.list(...)`；官方「单 Expr（可 nil / ListExpr）」字段用 `exprListFrom` 归一（§5.3 扁平化） |
| 可选字段 `-` | `exprField` 的 nil 分支 / `nameStr(nil)` / `optLitStr(nil)` |
| 位置不 dump | 遍历器根本不碰 `Pos()` |
| 字面量只 dump 原文 | `BasicLit(小写kind) "原文"`，原文用 `rustDebugQuote`（模拟 Rust `{:?}`，**不是** `%q`） |
| `Operation` 统一 | 一元 `Y: null`；`Op` 用官方 `Operator.String()`（裸 `+`） |
| `AssignStmt` 统一 | `=`→Op 0、`:=`→`Def`、复合赋值→基底运算符拼 `=`、`++/--`→`Rhs==nil`（nodes.go 注释 + parser.go `simpleStmt`） |
| `ListExpr` 扁平化 | `expr()` 的 `*ListExpr` 分支直接展开，无 ListExpr 行 |
| struct tag | `StructType` 把 `TagList[i]` 按位 zip 进第 i 个 `Field` 的 `Tag:` 行；其余上下文恒 `Tag: -` |

**对齐纪律**：改 dump.rs 或 dump.go 的任何一行输出形状，**必须同一任务里**
改另一侧并跑对撞（T-AP18）；只改一侧 = 对撞全线假红，且假红本身不含信息。

## 6. 验收判据（T-A-91，2026-10-15 实测通过）

- [x] `go build` 通过（容器内需 `-buildvcs=false`，见 §7）。
- [x] 探针文件（覆盖 if/for range/type switch/select/goroutine/composite lit/
      struct tag/泛型内嵌指针）输出含 `File` / `PkgName: Name "p"` /
      `GoVersion: -` / `Decls: [3 entries]` 等行，全部节点形状与 dump.rs 规则一致。
- [x] `go run . go_source_code/src/fmt/print.go`：exit 0、dump 9490 行、
      `File`/`PkgName: Name "fmt"`/`Decls: [77 entries]` 起头；
      连跑 3 次 sha256 = `6f0f08ab…752f` 完全相同（确定性）。
- [x] 错误路径：`func f(){ break }` → exit 1，stderr 恰好一行
      `…:4:2: break is not in a loop, switch, or select`，**stdout 仍有完整 dump**。
- [x] `CheckBranches` 生效：`goto L; x := 1; L:` →
      `…:4:7: goto L jumps over declaration of x at …:5:4`。
- [x] `gofmt -l` 干净。

## 7. 实测的坑（M1 四坑的增量）

1. **容器内 `go build` 要 `-buildvcs=false`** —— 容器里的 git 对 `/work`
   报 `exit status 128`（dubious ownership），`go build` 默认要盖 VCS 戳所以炸；
   `go run` 不盖戳，不受影响。脚本一律写 `go build -buildvcs=false`。
2. M1 的四坑原样适用：样本不能与本 adapter 同目录同名 `.go`；必须 `cd` 进本
   目录；`RGOC_TTY=0`（否则伪终端 `\n`→`\r\n`）；捕获输出时**不要**在
   `set -e` 下对「预期 exit 1」的用例直接断链（先 `set +e`）。

## 8. 已知边界

- `main.go` 传 `pragh = nil`：官方仅在 `pragh != nil` 时把 `//go:` 指令装进
  `p.pragma`（parser.go `:84`），`clearPragma` 的回调因此不可达 —— 传 nil 安全，
  且指令效果（`nolint` 等）本就不属于 M2 的判定面。
- `File.GoVersion` 由官方 parser 从 `//go:build` 约束推导（`constraint.GoVersion`），
  oracle 侧自动获得；`rgoc` 侧同口径（T-AP15 已登记 `constraint.Parse` 的已知缺口）。
- 基本字面量的 `Value` 在 Rust 侧是有损 UTF-8（`from_utf8_lossy`，T-AP17/18 已登记
  的 C6 缺口）：非法 UTF-8 输入上两侧 dump 可能分叉，属**已知且已登记**，不在本
  adapter 内打补丁。
