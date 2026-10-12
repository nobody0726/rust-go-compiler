# token oracle — 词法 golden 的「出卷老师」

把一段 Go 源码扫成**逐字节可比较**的两条流（词元 / 诊断），供 `rgoc-lex` 做 golden 比对。

> **它不是参考实现，它是标准答案的提供者。** 本仓的判定规则只有一条：
> 与 oracle 不一致时，**先怀疑我们，不怀疑 oracle**（`M0-design.md` §1.0 第 2 步）。

---

## 1. 为什么必须写这个程序

官方 scanner 的 `next()` **不返回值** —— 结果写在自己的字段上：

```go
s.next()                        // 调用后没有返回值
// 结果在：s.tok / s.lit / s.kind / s.op / s.prec / s.line / s.col / s.bad
```

所以**没有任何现成命令能吐出词元流**：

| 命令 | 它给的是 | 能不能用 |
|---|---|---|
| `go build` | 机器码 | ✗ |
| `go tool compile -E` | **语法树 dump** | ✗ `03` §M1 明令禁止当 token oracle |
| `go/scanner`（标准库） | 词元流，但**粒度不同** | ✗ 见下 |
| **本目录的 adapter** | 词元流 + 诊断流 | ✅ |

**为什么不用标准库 `go/scanner`**：它把数字 / 字符 / 字符串拆成**独立词元类别**，
没有 `LitKind`、没有 `op` / `prec`；而 `01-feature-set.md` 的 PIPE-02 指定的基准是**编译器版**。
粒度对不上，golden 形状就对不上官方编译器单测 —— 后续 M2 也会对不上。

## 2. 目录结构

```text
tools/token-oracle/
├── README.md              本文件：来源、校验值、格式、再生成步骤
├── go.mod                 module tokenoracle（仅标准库依赖）
├── main.go                ★ 我们的：命令行入口 + 格式化（三条纪律的落地点）
├── diagmap.go             ★ 我们的：官方消息 → DiagCode 映射表 + 码表自检
├── charcorpus.go          ★ 我们的：**字符级**对撞语料的生成（1177 个字节串）
├── chargolden.go          ★ 我们的：**字符级** golden 的渲染
├── fixtures/
│   ├── hello.txt          正向样本（48 字节）
│   ├── bad_utf8.txt       负向样本（40 字节，含裸 0xFF）
│   ├── char_corpus.txt    **字符级语料**：一行一个 hex（1177 行）
│   ├── char_golden.txt    **字符级 golden**：3279 行，由 --chars-golden 生成
│   ├── comment/           **注释行为探针**（p01–p45，T-L13 建立）
│   │                      文件名即场景与出处；`rgoc-lex` 的 test_comment.rs
│   │                      逐条断言都直引探针名。改这里 = 改测试的期望值依据
│   └── next/              **端到端探针**（n01–n17，T-L15 建立）
│                          `rgoc/crates/rgoc-lex/tests/golden/*.golden` 由这些
│                          探针 + `go_source_code/test/syntax/semi1..7.go` 生成
│                          （`go run . <file> | tail -n +3`）；测试输入与探针逐字节相同
└── gosyntax/
    ├── LICENSE           ┐ BSD-3-Clause 正文（随附，见 §3）
    ├── scanner.go        │
    ├── source.go         │ 官方 gc/syntax/ 五文件
    ├── tokens.go         │ **原样复制，零改动**
    ├── token_string.go   │
    ├── operator_string.go┘
    └── export.go         ★ 我们的：唯一的同包 shim
```

**★ 标记的五个文件是我们写的，其余是官方的。** 官方文件**不得改动** ——
`M1-design.md` §9.1 原本设想「`adapter.go` 是唯一手写部分」，实测后必须改成三个文件，
理由有两条：

> **① 为什么需要同包 shim（`gosyntax/export.go`）**：官方 scanner 的**全部**标识符都是未导出的
> （`scanner`、`next`、`tok`、`lit`、`kind`…）。外部包**无法访问**，
> 而改动官方源码就破坏了「原样复制」这条纪律 ⇒ 唯一的出路是在**同一个 package** 里加一层转发。
> 它**只做转发**、不含任何词法逻辑，但它**必须存在** —— 否则根本调不到 `next()`。
>
> **② 为什么映射表单独成文件（`diagmap.go`）**：它带一条 `--selfcheck`，
> 把「码表 ↔ 官方消息」的一致性变成**可执行断言**（见 §6 末尾）。
> 与格式化混在一起会让这条检查无处安放。

## 3. 来源与校验值

| 项 | 值 |
|---|---|
| 来源 | `go_source_code/src/cmd/compile/internal/syntax/` |
| 上游 | Go 官方源码树，`VERSION` = **go1.27.1**（`time 2026-08-28T16:20:06Z`） |
| 容器内 Go | `go version go1.27.1 linux/arm64` |
| 许可 | BSD-3-Clause；五个源文件的许可头**原样保留**，正文随附在 `gosyntax/LICENSE` |

> **为什么要连 LICENSE 一起入库**：五个文件的头两行写着
> "governed by a BSD-style license **that can be found in the LICENSE file**" ——
> 只抄代码不抄许可正文，等于把那句指向一个不存在的文件。
> 正文取自语料根的 `go_source_code/LICENSE`，逐字节一致
> （sha256 `911f8f57…d9dad`，27 行 / 1,453 字节）。

**sha256（与官方原文逐字节一致，已用 `cmp` 校验）**：

```text
59946110d83da0391a61e6344865858a18054f76c547ae1fdf29c9c452e1547d  operator_string.go
eb774e9a58f9d29c0296c29360338e19eb4040977a39ecc5753fd1762a18ee49  scanner.go
d3696fb7dc2f63ebb9aeee224423426ce9f6cf47096029d36aed41763eea7349  source.go
a59da3b0e566bba8dd963f2148ab0c5acbb93d1819886db6ffe7d6e9371be06e  token_string.go
d9667c96873a3e1ae84fce948ef80d269a009a787496f088ab643b14b8bec192  tokens.go
```

合计 **1,374 行**（scanner 881 / source 218 / tokens 159 / token_string 70 / operator_string 46）。

### 再生成步骤（升级语料版本时）

```bash
# 1. 覆盖五个文件（不要改内容）
SRC=go_source_code/src/cmd/compile/internal/syntax
cp $SRC/{scanner,source,tokens,token_string,operator_string}.go tools/token-oracle/gosyntax/

# 2. 逐字节确认「零改动」
for f in scanner source tokens token_string operator_string; do
  cmp "$SRC/$f.go" "tools/token-oracle/gosyntax/$f.go" || echo "!! $f.go 与官方不一致"
done

# 3. 重算校验值并更新本文件的 §3 表格
shasum -a 256 tools/token-oracle/gosyntax/*.go

# 4. 重跑验收（见 §7）。若黄金文本变了，先判断是「官方行为变了」
#    还是「我们错了」—— 前者要更新 golden 并记账，后者要修代码。
```

## 4. 怎么跑

**必须在容器内**（`scripts/in-container.sh`）。容器工作目录是 `/work`，
而本目录**自带 `go.mod`**，所以要 `cd` 进去（本仓 G11）：

```bash
scripts/in-container.sh bash -lc 'cd /work/tools/token-oracle && go run . fixtures/hello.txt'
```

用法：

```text
go run . [--mode comments|directives] <source-file>   # 词元级
go run . --selfcheck                                  # 映射表/码表自检
go run . --list-codes                                 # 打印对照表
go run . --gen-char-corpus                            # 生成字符级语料 → stdout
go run . --chars-golden [--corpus <path>]             # 生成字符级 golden → stdout
```

`--mode` 默认**不开启** ⇒ 注释与指令**一律不上报**，输出只有 `== tokens ==` / `== diags ==`
（即冻结的 golden 格式）。开启后会多出 `== messages ==` 一节（LEX-03 用）。

### 4.1 字符级通道（`DumpChars`）—— 与词元级**互补**，不是替代

词元级 golden 在**词元边界**上取样；而位置是**每个字节**推出来的，`source.nextch`
是 byte-column 语义的唯一来源。所以另有一条**字符级**通道：

```go
// gosyntax/export.go
func DumpChars(src string) []CharEvent
```

它**直接调 `nextch()`**（不走 `next()`），只观察字符层：位置、`chw`、
以及被跳过的字节各自报出的诊断。事件流**按发生顺序**，**不需要排序** ——
"违规字节先报诊断、再让出下一个字符"是 `goto redo` 的直接后果，
那是**语义**（每个被跳过的字节各占一列），不是内部时序
（与词元级恰好相反：那里"诊断何时触发"是实现细节，两条流必须分开排序）。

⚠️ **字符级的 golden 必须 `RGOC_TTY=0` 采集**（同 §8 第 4 条）。

⚠️ **一处已登记的偏离**：输入**首字节是 `0x00`** 时官方不报 NUL
（它的 NUL 检查只写在 ASCII 快路径里，而懒填充让首次 `nextch` 的 `buf[0]` 还是 sentinel）。
我们**总是**报。两侧都报错，差别只在代号。判为**实现产物**、不照抄
（`M1-design.md` §12 R11）；偏离由 `rgoc-lex/tests/test_char_golden.rs` 用
**谓词 + 形状**双重断言登记。

## 5. 输出格式（冻结）

```text
# golden: <命令行给出的路径>
# source: oracle
== tokens ==
  <pos>  <tok>  lit=<lit>
== diags ==
  <pos>  <LEX-CODE>
```

`<pos>` = `行:列`，**行列都从 1 起，列按字节**（C1 §2.1）。

对齐：位置占 7 列、词元名占 9 列（左侧对齐），即 `"  %-7s%-9slit=%s"`。

### 5.1 词元行的三个字段

| 字段 | 何时存在 | 取值 |
|---|---|---|
| `tok` | **总是** | 官方 `token.String()`：`EOF` `name` `literal` `op` `op=` `:=` `<-` `*` `(` `;` `package` … |
| `lit` | **总是**（无效域写 `-`） | 见 §5.2 |
| `kind=` `bad=` | **仅 `literal`** | `kind` 是官方数值（0=Int 1=Float 2=Imag 3=Rune 4=String），`bad` 是 `true`/`false` |
| `op=` `prec=` | **仅四类运算符**（官方 `_Operator` / `_AssignOp` / `_IncOp` / `_Star`） | 官方 `Operator.String()` 与优先级数值。注意 `_Operator` 与 `_IncOp` 打印出来**都是 `op`**，按 token 名分辨不了，判据是上面那四个内部常量 |

### 5.2 `lit` 的有效域 —— **这是本格式最重要的一条纪律**

官方结构体注释写着：

```go
lit string   // valid if tok is _Name, _Literal, or _Semi
```

**其余类别的 `lit` 是上一个词元的残留值**（实测 F5）。例如 `a >>= 1` 里那个 `>>=`
词元的 `lit` 显示为 `"a"` —— 字段有值、类型合法，**只有语义是错的**。

所以：**不在有效域内就写 `-`**，绝不把残留值录进 golden。
第一版无条件录 `lit`，结果 golden 里出现了 `( lit="main"` —— 那等于**把实现细节当契约**：
换一个实现（比如 Rust 的和类型）残留值必然不同，比对就会莫名其妙地红。

### 5.3 `lit` 的转义规则（两侧必须一字不差地实现）

`lit` 是**原始字节切片**，可能含任何字节 —— 包括**非法 UTF-8**和 NUL（实测 F9）。
所以**不用 Go 的 `%q`，也不许用 Rust 的 `{:?}`**（两者对控制字符的写法不同，且
`String` 根本存不了非法 UTF-8）。改为**自实现的字节级转义**，规则如下：

| 字节 | 输出 |
|---|---|
| `\` (0x5C) | `\\` |
| `"` (0x22) | `\"` |
| 换行 / 回车 / 制表 | `\n` `\r` `\t` |
| 可打印 ASCII（0x20–0x7E） | 原样 |
| **其余一切**（含 0x00–0x1F、0x7F、所有 ≥0x80 的字节） | `\xNN`（两位小写十六进制，**逐字节**） |

整体用双引号包起来。例：`"hello"`（含引号的原文）⇒ `"\"hello\""`；
非法字节 ⇒ `"1\xff2"`；中文标识符 ⇒ `"\xe5\x8f\x98\xe9\x87\x8f"`。

> **⚠️ 这条规则对 Rust 侧有一个硬性推论**：`rgoc-lex` 的词元载荷
> **必须是字节序列（`Vec<u8>`）而不是 `String`** —— `String` 在类型上就装不了 `\xff`。
> 该收敛点记在 `.superpowers/sdd/M1-plan/progress.md` 的 R4。

### 5.4 `== messages ==`（仅 `--mode` 开启时）

```text
== messages ==
  <pos>  <escaped-text>
```

文本用 §5.3 的同一套转义（注释可跨行，不转义就会破坏「一行一条」的结构）。

### 5.5 四条规范化纪律

| # | 纪律 | 不这么做的后果（实测） |
|---|---|---|
| 1 | **只录有效域内的字段**（§5.2） | golden 里出现 `( lit="main"`，实现细节变契约 |
| 2 | **两条流分开、各自排序**（`line→col→code`） | 按事件顺序输出时，诊断 `4:8` 出现在词元 `4:7` **之前** ⇒ golden 编码了「何时触发诊断」这种内部时序 |
| 3 | **诊断只存位置 + 代号**，不存消息文本 | D-M1-9：文本一改就重排全部诊断 |
| 4 | **确定性**：同一输入多次运行字节完全相同 | 已实测 3 次 `sha256` 一致 |

## 6. 诊断代号映射（官方消息 → `DiagCode`）

**由 scanner.go / source.go 里全部 `errorf` / `errorAtf` / `error` 调用点枚举而来** ——
不是凭印象列的：**26 处错误调用点**（scanner 23 / source 3），去重后得
**25 条消息模板**，再加 1 条本仓自己的限制（`LEX-EXCEEDS-FILE-LIMIT`）。映射按**前缀**匹配。

| 官方消息（格式化模板） | DiagCode |
|---|---|
| `invalid character %#U` | `LEX-BAD-CHAR` |
| `invalid digit %q in %s literal` | `LEX-BAD-DIGIT` |
| `%s literal has no digits` | `LEX-LIT-NO-DIGITS` |
| `invalid radix point in %s literal` | `LEX-BAD-RADIX-POINT` |
| `'_' must separate successive digits` | `LEX-BAD-SEP` |
| `exponent has no digits` | `LEX-BAD-EXPONENT` |
| `%q exponent requires decimal mantissa` | `LEX-BAD-EXPONENT` |
| `%q exponent requires hexadecimal mantissa` | `LEX-BAD-EXPONENT` |
| `hexadecimal mantissa requires a 'p' exponent` | `LEX-BAD-EXPONENT` |
| `identifier cannot begin with digit %#U` | `LEX-ID-LEADING-DIGIT` |
| `invalid character %#U in identifier` | `LEX-BAD-IDENT-CHAR` |
| `invalid UTF-8 encoding` | `LEX-INVALID-UTF8` |
| `invalid NUL character` | `LEX-INVALID-NUL` |
| `invalid BOM in the middle of the file` | `LEX-BOM-MIDFILE` |
| `comment not terminated` | `LEX-COMMENT-UNTERMINATED` |
| `string not terminated` | `LEX-STRING-UNTERMINATED` |
| `newline in string` | `LEX-STRING-NEWLINE` |
| `empty rune literal or unescaped '` | `LEX-RUNE-EMPTY` |
| `more than one character in rune literal` | `LEX-RUNE-MULTI` |
| `newline in rune literal` | `LEX-RUNE-NEWLINE` |
| `rune literal not terminated` | `LEX-RUNE-UNTERMINATED` |
| `unknown escape` | `LEX-ESCAPE-UNKNOWN` |
| `invalid character %q in %s escape` | `LEX-ESCAPE-BAD-DIGIT` |
| `octal escape value %d > 255` | `LEX-ESCAPE-OCTAL-OVER` |
| `escape is invalid Unicode code point %#U` | `LEX-ESCAPE-RANGE` |
| （**官方无对应**） | `LEX-EXCEEDS-FILE-LIMIT` |

**遇到映射表之外的消息，adapter 立刻 `exit 3` 并把原文与位置打到 stderr。**
不允许"猜一个最像的代号"或静默跳过 —— 那会让 golden 在**契约不完整**的情况下被冻结，
正是本仓反复吃亏的「静默失败」形状。

> ⚠️ 上表第一版（`M1-design.md` §7.2）只有 19 条，漏了 4 条
> （`LEX-BAD-DIGIT` / `LEX-BAD-RADIX-POINT` / `LEX-STRING-NEWLINE` / `LEX-ESCAPE-BAD-DIGIT`）。
> 原因是那张表**凭印象手写**，而不是**枚举全部报错点**得到 —— 与
> `AGENTS.md` 记的「写死的键列表会变成盲区」是同一个形状。
>
> ⇒ 对策：`DiagCode` 由 19 扩到 **23**，并且**两侧必须同步**
> （本文件 + `rgoc/crates/rgoc-lex/src/diag.rs`）。

### 6.1 两道守卫：`--selfcheck` 与「未映射即失败」

```bash
go run . --list-codes     # 打印映射表（人工核对用）
go run . --selfcheck      # 机器自检
```

`--selfcheck` 断言四件事：

| # | 断言 | 抓的是 |
|---|---|---|
| ① | 映射表用到的代号**都在码表里** | 拼错代号、映射到不存在的代号 |
| ② | 码表里每个代号**都有官方消息能产出它**（`LEX-EXCEEDS-FILE-LIMIT` 豁免） | **永远不会被产出的死代号** |
| ③ | 映射条目数 == 冻结的 **25** | 「少登记一条却以为齐了」 |
| ④ | 码表无重复、共 **23** 个 | 抄漏/抄重 |

运行时还有一道：**遇到映射表之外的消息立刻 `exit 3`**，并把原文与位置打到 stderr。

**反向验证**（人工注入，脚本按本仓要求不入库）——四道变异都必须变红：

| 变异 | 结果 |
|---|---|
| 删掉 `invalid UTF-8 encoding` 映射 | 自检 2 项红（`LEX-INVALID-UTF8` 变死代号 + 条目数 24）；跑 `bad_utf8.txt` **exit 3** 并打出原文 ✅ |
| 映射指向码表里不存在的 `LEX-BAD-SEPP` | 自检 2 项红（代号不在码表 + `LEX-BAD-SEP` 变死代号）✅ |
| `wantMappings` 由 25 改成 24 | 自检 1 项红 ✅ |
| 把 `LEX-BAD-DIGIT` 压成 `LEX-BAD-CHAR` | 自检 1 项红（`LEX-BAD-DIGIT` 变死代号）✅ |


## 7. 验收判据

**样本一对（`fixtures/hello.txt`）—— 正向路径：**

```text
① 输出以 # golden: / # source: oracle 两行开头；
② 含 == tokens == 与 == diags == 两节（默认模式**不得**有 == messages ==）；
③ tokens 节逐行等于 §9.1 的黄金文本，且 diags 节为空；
④ 连跑 3 次，sha256 完全相同（当前 = 239d8dd5…f7ffc）。
```

**样本二对（`fixtures/bad_utf8.txt`）—— 负向路径：**

```text
① diags 节**非空**，恰好一行 4:8 LEX-INVALID-UTF8；
② tokens 节里那个畸形字面量**照常产出**：lit="1\xff2"（非法字节留在原文里）；
③ 其 kind=0 bad=false —— 注意 bad **没有**被置真（见 §8 第 5 条）；
④ sha256 与 §9.2 一致（当前 = 276137cb…4571）。
```

> **为什么要两个样本**：只跑 `hello.txt` 的话，`== diags ==` 那一节**永远没被任何验收碰过** ——
> 映射表、排序键、`\xNN` 转义在负例上的行为全是"看起来写对了"。
> 本仓的教训是「没被断言覆盖的分支 = 不存在」。

**样本内容**（`hello.txt` 48 字节，第 4 行是**制表符**缩进；`bad_utf8.txt` 40 字节，含裸 `0xFF`）：

```go
// fixtures/hello.txt
package main

func main() {
	println("hello")
}

// fixtures/bad_utf8.txt（第 4 行的 \xff 是单个字节 0xFF，不是转义写法）
package main

func main() {
	x := 1<0xFF>2
}
```

## 8. 五个实测的坑

1. **样本文件不能叫 `.go`，也不能放在 adapter 的同一个包目录里** ——
   `go run .` 会把同目录的 `.go` 一起编译，于是 `package main` 与样本冲突
   （`main redeclared in this block`）。本目录把样本放在 `fixtures/` 且命名 `.txt`。
2. **必须 `cd` 进本目录再 `go run .`** —— 本目录自带 `go.mod`，仓库根没有。
3. **官方标识符全未导出** ⇒ 必须同包 shim（§2）。
4. **⚠️ 捕获 golden 时必须设 `RGOC_TTY=0`** —— 否则每行末尾会多一个 `\r`：

   ```text
   $ scripts/in-container.sh bash -lc '... go run . fixtures/hello.txt' | od -c
     ... l o . t x t \r \n #     ← 伪终端把 \n 变成了 \r\n
   $ RGOC_TTY=0 scripts/in-container.sh bash -lc '...' | od -c
     ... l o . t x t \n #         ← 干净
   ```

   `scripts/in-container.sh` 在 stdin 是 tty 时会加 `-it`，而**伪终端默认开启 ONLCR**
   把 `\n` 翻译成 `\r\n`。**这个差别在终端里肉眼看不出**（`\r\n` 与 `\n` 显示一样），
   但会让 golden 文件每一行都带 CR —— 比对时全线报差异，而 diff 里看不出哪里不同。
   实测：不加 `RGOC_TTY=0` 时 20 行输出含 20 个 CR，加了是 0 个。
   **生成/校验 golden 的脚本必须显式设它。**

5. **`bad=false` 不代表原文是好的**：`1\xff2` 实测为 `kind=0 bad=false` ——
   非法 UTF-8 是 `nextch` 报的，不是数字语法报的，所以 `bad` 不变真。
   `bad` 只表示"字面量的**语法**有问题"，**不等于**"原文可以被信任"。
   这意味着**两侧都不能拿 `bad` 当"原文合法"的判据**。


## 9. 黄金文本（oracle 的实测输出，**由 oracle 生成、不得手改**）

两份都由 `RGOC_TTY=0` 抓取（§8 第 4 条）。

### 9.1 `fixtures/hello.txt`

```text
# golden: fixtures/hello.txt
# source: oracle
== tokens ==
  1:1    package  lit=-
  1:9    name     lit="main"
  1:13   ;        lit="newline"
  3:1    func     lit=-
  3:6    name     lit="main"
  3:10   (        lit=-
  3:11   )        lit=-
  3:13   {        lit=-
  4:2    name     lit="println"
  4:9    (        lit=-
  4:10   literal  lit="\"hello\"" kind=4 bad=false
  4:17   )        lit=-
  4:18   ;        lit="newline"
  5:1    }        lit=-
  5:2    ;        lit="newline"
  6:1    EOF      lit=-
== diags ==
```

sha256 = `239d8dd5f821eebc261d3cb598c7eaf9de6729ccf41ec58e2c83b274709f7ffc`

**三个值得看的点**：

- 出现了**三个分号**（`1:13` / `4:18` / `5:2`），源码里**一个都没有** —— 都是换行处自动补的，`lit="newline"`。
  注意 `3:13` 的 `{` 后面**没有**补（`{` 不在触发集里），而 `)` `}` 后面都补。
- `4:2` 的 `println` 只是 **`name`（标识符）** —— M1 不认识它是内建函数。
- `4:10` 的 `lit` 是 `"\"hello\""` —— **含两端的双引号，转义未解码**。

### 9.2 `fixtures/bad_utf8.txt`

```text
# golden: fixtures/bad_utf8.txt
# source: oracle
== tokens ==
  1:1    package  lit=-
  1:9    name     lit="main"
  1:13   ;        lit="newline"
  3:1    func     lit=-
  3:6    name     lit="main"
  3:10   (        lit=-
  3:11   )        lit=-
  3:13   {        lit=-
  4:2    name     lit="x"
  4:4    :=       lit=-
  4:7    literal  lit="1\xff2" kind=0 bad=false
  4:10   ;        lit="newline"
  5:1    }        lit=-
  5:2    ;        lit="newline"
  6:1    EOF      lit=-
== diags ==
  4:8    LEX-INVALID-UTF8
```

sha256 = `276137cb19643a256431c2662306f31e6f9bc62ec27d0815e351d1fc87484571`

**三个值得看的点**：

- 词元**照常产出**（`lit="1\xff2"`）**且**附带一条诊断 —— M1 的恢复纪律是
  「报错但继续，把畸形的东西也交出去」，从不"失败退出"。
- 诊断位置 `4:8` 与被污染的字节位置一致（`4:7` 是 `1`，`4:8` 是那个 `0xFF`）。
- `4:4` 的 `:=` 只有 `lit=-`：官方对 `_Define` **不设 `op`**，
  `op`/`prec` 只在 `_Operator`/`_Star`/`_AssignOp`/`_IncOp` 上有效 —— 这正是
  §5.2「只录有效域内字段」的执行结果。

