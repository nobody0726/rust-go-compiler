# M1 测试先行清单（M1-tests.md）

> **阶段 ID**：`M1`　|　**状态**：**待冻结**（本稿产出后需一次性确认）　|　**日期**：2026-10-11
> **文档索引**：[`../README.md`](../README.md)　|　**上游**：[`M1-design.md`](./M1-design.md)（设计决策 D-M1-1~9、事实基线 F1–F20）、[`../03-roadmap.md`](../03-roadmap.md) §M1、[`../01-feature-set.md`](../01-feature-set.md) §1.1（LEX-01..13）、[`../02-test-inventory.md`](../02-test-inventory.md) §1
> **下游**：[`M1-plan.md`](./M1-plan.md)（RED-GREEN 任务分解）
>
> **本文档的效力**（`../03-roadmap.md` §6.2）：M1 开工前三件套之二。**必需测试 ID 100% 通过**是进入 M2 的条件；
> 白名单须**开工前冻结**；跳过项**不得记为 pass**。
>
> **ID 命名沿用 [`M0-tests.md`](./M0-tests.md) §2 的约定**：`T-L-<nn>` = M1 词法（lexer）。
> **切面写在列里，不写进 ID** —— 用例换组时 ID 不动（依据：本仓「写死的键列表会变成盲区」踩过三次）。

---

## 0. 冻结声明

| 项 | 冻结内容 | 冻结时点 | 变更规则 |
|---|---|---|---|
| **F1** | §2 的 **LEX-01..13 逐项测试 ID** | ⬜ 待冻结 | **不得有项无测试**；新增须单独记录 |
| **F2** | §3 的**分母**（必需 ID 总数） | ⬜ 待冻结 | **不得因跑不动而缩小**（`03` §3.3） |
| **F3** | §4 的负例清单与**双向断言**要求 | ⬜ 待冻结 | 同上 |
| **F4** | §6 的 unsupported 清单 | ⬜ 待冻结 | 同上 |
| **F5** | §7 的超时与资源上限 | ⬜ 待冻结 | 实测后可有记录地调整 |
| **F6** | §5 的 oracle 口径（adapter / golden 格式 / 两种来源） | ⬜ 待冻结 | 同上 |

---

## 1. 判据的三种来源

每个测试 ID 必须标明判据来源，**这决定了它能否被自动化**：

| 记号 | 含义 | 说明 |
|---|---|---|
| **`O`** | **oracle 对照** —— 期望值由 go1.27.1 的 scanner adapter 生成 | 最权威；需要容器 |
| **`S`** | **自证** —— 不变式断言，不需要 oracle | 测 oracle 测不到的性质（单调、一致性） |
| **`H`** | **手写** —— 期望值由人写，因为**官方没有这个行为** | **只用于实现限制**（超长），必须标注 |

> **`H` 是危险的记号**：见到 `H` 就要问「为什么官方没有」。目前**只有一条**（`T-L-63`）。

---

## 2. LEX-01..13 逐项覆盖（F1）

| 功能 ID | 内容（`01` §1.1） | 覆盖它的测试 ID | 判据 |
|---|---|---|---|
| **LEX-01** | UTF-8 源码表示（BOM / NUL / 非规范 UTF-8） | `T-L-13` `T-L-14` `T-L-15` `T-L-16` | O |
| **LEX-02** | Unicode 字符分类（字母 L\*、数字 Nd、`_`） | `T-L-11` `T-L-12` `T-L-19` `T-L-24` `T-L-25` | O |
| **LEX-03** | 行注释 / 块注释 | `T-L-45` `T-L-46` `T-L-52` `T-L-70` | O |
| **LEX-04** | Token 四分类与最长匹配 | `T-L-20`…`T-L-29` | O |
| **LEX-05** | 25 个保留关键字 | `T-L-22` `T-L-30` | O |
| **LEX-06** | 48 条操作符与标点 | `T-L-28` `T-L-31` | O |
| **LEX-07** | 分号自动插入 | `T-L-40`…`T-L-47` | O |
| **LEX-08** | 标识符词法 | `T-L-20` `T-L-21` `T-L-23` `T-L-24` | O |
| **LEX-09** | 整数字面量（4 进制 + `_` 分隔符） | `T-L-33` `T-L-38` | O |
| **LEX-10** | 浮点字面量（十进制 + 十六进制 p 指数） | `T-L-34` `T-L-38` | O |
| **LEX-11** | 虚数字面量（后缀 `i`） | `T-L-35` | O |
| **LEX-12** | Rune 字面量与转义 | `T-L-36` `T-L-53` | O |
| **LEX-13** | 字符串字面量（解释型 / 原始型） | `T-L-37` `T-L-53` | O |

**⚠️ LEX-06 的计数待裁决**：`01` 写「47 个」，spec 的 `<pre class="grammar">` 实测 **48 条**（`M1-design.md` F3）。
本表按 **48** 冻结。**若裁决为 47，须先改 `01` 再回改本表** —— 不得两处并存。

---

## 3. 六个切面的测试 ID（F2）

### 3.1 切面 ① 解码与位置（`T-L-1x`）—— 判据全 `O`

> **本切面的主力判据是「字节级密集对撞」（见 §5.2.1）**：1177 个字节串逐个跑，
> 与 oracle **逐事件**比较。下面这 10 条是**可读代表用例** —— 每条钉住一个机制，
> 对撞红了的时候先看它们，能立刻判断"错的是哪个机制"。
>
> 手写用例**不可能**替代密集对撞：切面 ① 的判据原文是「位置必须与 oracle
> **逐字节**相等」，而位置是**每个字节**推出来的。T-L07 实测：密集对撞一次就找出了
> 一处**文档从未提到**的官方行为（首字节 NUL，见 §5.2.1 与 `M1-design.md` §12 R11）。

| ID | 用例 | 断言要点 |
|---|---|---|
| `T-L-10` | 纯 ASCII 单行 `x := 1` | 5 个词元的位置逐个与 oracle 相等 |
| `T-L-11` | **中文标识符** `\t变量 := 1` | `变量` @ 4:2、`:=` @ **4:9**（字节列，非 rune 列） |
| `T-L-12` | **emoji**（4 字节 UTF-8） | 列号按 4 递增 |
| `T-L-13` | BOM 在**文件首** | **被跳过**（不作为字符送出），但那 3 个字节**各占一列** ⇒ 首个可见字符在 **1:4**；**无诊断** |
| `T-L-14` | BOM 在**文件中** | 报 `LEX-BOM-MIDFILE`（位置 = BOM 自己）；BOM **同样被跳过** —— 与文件首的差别**只在报不报错**；标识符层的原文仍含那三字节（F8） |
| `T-L-15` | NUL `x\0y` | 报 `LEX-INVALID-NUL`；NUL **被跳过**、但**仍占一列**（`y` @1:3）；连续两个 NUL ⇒ **两条**诊断（`advance() -> Vec<Diag>` 的依据） |
| `T-L-16` | 非法 UTF-8 `1\xff2` | 报 `LEX-INVALID-UTF8`；**只消费 1 个字节**（截断的 4 字节序列 ⇒ **2** 条、代理区 ⇒ **3** 条）；后续字符继续正确 |
| `T-L-17` | CRLF `x\r\ny` | `\r` 在**字符层只是普通字符**（"当空白"是 `next()` 的事）；只有 `\n` 改行号 |
| `T-L-18` | 多行 + 空行 | 行号推进与「换行归零」逐行相等 |
| `T-L-19` | 私有区字符 `x\ue000` | ★ **订正**：字符层**不报错**（合法 UTF-8、`w=3`）；`LEX-BAD-IDENT-CHAR` 由**标识符层**产出（`scanner.go:411` 的 `atIdentChar`）⇒ 归 `T-L-2x`（`T-L08`） |

### 3.2 切面 ② 切分边界（`T-L-2x`）—— 判据全 `O`

| ID | 用例 | 断言要点 |
|---|---|---|
| `T-L-20` | `str1` / `str1x` / `str_` | **各为单个** `Ident` |
| `T-L-21` | `str x` / `str+x` / `a-b` | **各为两个**词元（成对边界） |
| `T-L-22` | `for` vs `forx` | 前者 `Keyword(For)`，后者 `Ident(b"forx")`（载荷是**字节**，见 D-M1-12） |
| `T-L-23` | `1str` | `Literal(Int)` + `Ident(b"str")`，**无诊断** |
| `T-L-24` | `a۰۱۸` / `foo६४` / `bar９８７６` / `ŝ` / `日本語` / `ab中cd` | 各为**单个** `Ident`（LEX-02 的 Nd 边界） |
| `T-L-25` | `_x` / `_` | `_` 属字母类 ⇒ 单个 `Ident` |
| `T-L-26` | `..x` / `..中` / `a..b` | **唯一回退点**：切出 `.` `.` 而不是 `...`。⚠️ **不要用 `1..2`** —— 实测它变成 `literal "1."` + `literal ".2"`（**两个 Float，根本没有 `..`**，F31）；`..中` 用来验"回退后紧跟多字节字符位置不漂移"（RF1） |
| `T-L-27` | `.` vs `..` vs `...` / `x.y` | `Dot` / `Dot`+`Dot` / `Delim(DotDotDot)` / `Dot` |
| `T-L-28` | `>>=` `<<=` `&^=` `!=` `<-` `++` | 各为**单个**词元（最长匹配） |
| `T-L-29` | `for(` `if{` `return(` | 关键字后**无空格**也能正确分界 |

> **`T-L08` 的实现比本表细**：上面 6 个 ID 在 `tests/test_ident.rs` 里落成 **17 条**断言，
> 另加一组 **F29 专项**（不占新 ID，因为它们仍属本切面）：
>
> | 输入 | 期望 | 依据 |
> |---|---|---|
> | `x²`（U+00B2，No） | `Ident(b"x\xc2\xb2")` + `LEX-BAD-IDENT-CHAR` | F29，oracle 实测 |
> | `²abc` | `Ident(b"\xc2\xb2abc")` + `LEX-BAD-IDENT-CHAR` @1:1（**不是** `LEX-ID-LEADING-DIGIT`） | F29 |
> | `x\u{0345}`（Mn） | `Ident` + `LEX-BAD-IDENT-CHAR` | F29（std 的 `is_alphabetic` 会静默通过） |
> | `x²1` | 吸收 `²` 后**继续**吃掉数字 | F29 |
> | `xµ`（U+00B5，Ll）/ `x１`（U+FF11，Nd） | `Ident`，**无诊断**（对照组） | F29 |
> | `x\xff y` / `x\0y` | `Ident` 的载荷**保留**非法字节 / NUL | F22 / F8 |
> | `1str` | `None` 且**零诊断** | 官方入口短路的直接后果 |
>
> ⚠️ **`¹³²` 那一类（原 T-L-24 的说法）要按上表读**：官方对它们是**吸收 + 报错**，
> 不是"切成两个词元"。

### 3.3 切面 ③ 词元载荷（`T-L-3x`）—— 判据全 `O`

| ID | 用例 | 断言要点 |
|---|---|---|
| `T-L-30` | **25 个关键字逐个** | 每个都是 `Keyword(k)`，不是 `Ident` |
| `T-L-31` | **48 条运算符与标点逐个** | 四元组 `(form, op, prec)` + 是否 `Delim`（`++ -- ) ] }` 额外带出一个自动分号，属正常）<br>★ 必须含 `+` / `+=` / `++` **三个都出现**，证明 `form` 无损（D-M1-10） |
| `T-L-32` | `1` / `1.5` / `1.5e3i` / `'x'` / `"s"` | `lit_kind` = Int/Float/Imag/Rune/String |
| `T-L-33` | `0b101` `0o17` `0x1f` `0123` `0` `1_000` | 进制与 `_` 全部合法路径 |
| `T-L-34` | `1e9` `1.5E-3` `0x1p-2` `0x.8p1` | 十进制 `e` / 十六进制 `p` 指数 |
| `T-L-35` | `3i` `1.5i` `0x1p-2i` | 后缀 `i` ⇒ `Imag` |
| `T-L-36` | `'\n'` `'\\'` `'\x41'` `'\u00e9'` `'\U0001F600'` `'\101'` | 转义全表 |
| `T-L-37` | `"a\tb"` 与 `` `a\tb` `` | 解释型解码 vs 原始型**不转义**；原始型可跨行；**文本保留 `\r`**（「丢弃」是**值**层面、属 M3 —— **F39**） |
| `T-L-38` | `0x` / `1__2` / `0b__1000` / `'ab'` / `"unterm` | `bad = true` + 对应诊断 |
| `T-L-39` | 对 `Op`/`Delim`/`Keyword` 断言 | **不读 `lit`**（该字段在这三类无效 —— 依据 F5/F16） |

> **`T-L-39` 是一条"反向"测试**：它断言的是**我们在这些类别上不产出 `lit` 载荷**。
> 官方 `TestTokens` 也只对 `_Name`/`_Literal` 断言 `lit`（F16）—— 照它写。

### 3.4 切面 ④ 分号插入（`T-L-4x`）—— 判据 `O`（`T-L-49` 为 `S`）

| ID | 用例 | 断言要点 |
|---|---|---|
| `T-L-40` | `;` 显式 / 换行 / EOF 三处 | `Semi` 的 `origin` = Explicit / Newline / Eof |
| `T-L-41` | `)` `]` `}` 后换行 | **补**分号 |
| `T-L-42` | `(` `[` `{` `,` `:` `.` `...` 后换行 | **不补** |
| `T-L-43` | `i++` / `i--` 后换行 | **补** |
| `T-L-44` | `break` `continue` `fallthrough` `return` 后换行 | **补**；其余 **21 个关键字不补** |
| `T-L-45` | `x // c` ⏎ `y` | 行注释**不吃**换行 ⇒ 仍补分号 |
| `T-L-46` | `x /* c1` ⏎ `c2 */ y` | 跨行块注释在 `nlsemi` 语境下**等价于换行** |
| `T-L-47` | **语料 `test/syntax/semi1..7.go`** | 七个文件逐词元 diff |
| `T-L-48` | `test/eof.go` / `eof1.go` | EOF 处的分号补出行为 |
| `T-L-49` | `SemicolonPolicy::set_after` 规则表 | **逐行断言**（26 个类别 × 置位与否）—— 判据 `S` |

### 3.5 切面 ⑤ 错误恢复（`T-L-5x`）—— 判据全 `O`

> **本组的每一条都必须断言两件事**：①诊断（位置 + `DiagCode`）②**其后的词元流仍然正确**。
> 只断言"报错了"是假通过（`M1-design.md` §8.2）。

| ID | 用例 | 断言要点 |
|---|---|---|
| `T-L-50` | 全部负例的诊断 | 位置 + `DiagCode` 逐条等于 golden |
| `T-L-51` | 全部负例的**后续词元** | 报错**之后的**词元流与 oracle 相等 |
| `T-L-52` | `/*` 未终止 | `LEX-COMMENT-UNTERMINATED`，且不中止 |
| `T-L-53` | `"abc` / `'a` 未终止 | `LEX-STRING-UNTERMINATED` / `LEX-RUNE-UNTERMINATED` |
| `T-L-54` | 同位置多条诊断 | 按 C1 §4 排序（`file→line→col→code`），**输出唯一** |

### 3.6 切面 ⑥ 流层不变式（`T-L-6x`）—— 判据 `S`（`T-L-63` 为 `H`）

| ID | 用例 | 断言要点 | 判据 |
|---|---|---|---|
| `T-L-60` | 位置单调 | 同一文件内 `(line,col)` 不回退（`rewind` 的瞬时除外） | S |
| `T-L-61` | 拼接等价 | `lex(A + B)` 的前缀 == `lex(A)`（在边界处允许一个词元差异） | S |
| `T-L-62` | 三次一致 | 同一输入跑 3 次，词元流 + 诊断流**逐字节相同** | S |
| `T-L-63` | **超长输入** | 报 `LEX-EXCEEDS-FILE-LIMIT`，返回**只含 `Eof`** 的流；**不 panic** | **H** |
| `T-L-64` | `Eof` 唯一性 | 流中**恰好一个** `Eof` 且在最后 | S |
| `T-L-65` | 词元总数 | 与 oracle 的计数**相等**（数量不变量） | O |

---

## 4. 负例清单（F3）

| 负例 | 输入 | 期望诊断 | 期望的"之后还有词元" |
|---|---|---|---|
| N1 | `1\xff2` | `LEX-INVALID-UTF8` @ 4:8 | 后面紧跟 `Literal` 与分号 |
| N2 | `x\0y` | `LEX-INVALID-NUL` | 标识符含 NUL 且后续正确 |
| N3 | `x\ufeffy` | `LEX-BOM-MIDFILE` | 同上 |
| N4 | `x\ue000` | `LEX-BAD-IDENT-CHAR` | 同上 |
| N5 | `0x` | `LEX-LIT-NO-DIGITS` | `Literal bad=true` + 后续分号 |
| N6 | `1__2` | `LEX-BAD-SEP` | 同上 |
| N7 | `'ab'` | `LEX-RUNE-MULTI` | 同上 |
| N8 | `"abc`（EOF） | `LEX-STRING-UNTERMINATED` | 收尾出分号与 `Eof` |
| N9 | `/*`（EOF） | `LEX-COMMENT-UNTERMINATED` | 收尾出 `Eof` |
| N10 | `0x1` 而 spec 要求 `p` 指数（`0x1.8`） | `LEX-BAD-EXPONENT` | 同上 |
| N11 | `\q`（未知转义） | `LEX-ESCAPE-UNKNOWN` | 同上 |
| N12 | 超 64 MiB | `LEX-EXCEEDS-FILE-LIMIT` | 流中只有 `Eof` |

> **N1–N11 的期望值全部由 oracle 生成**；**N12 是 `H`**（官方无此行为）。

---

## 5. oracle 口径（F6）

### 5.1 adapter

| 项 | 约定 |
|---|---|
| 基准 | **编译器版** `gc/syntax/` 五文件（`M1-design.md` D-M1-3） |
| 位置 | `tools/token-oracle/`（含 `README.md` 记来源/版本/校验值/再生成步骤） |
| 入口 | `main.go` + `gosyntax/export.go`（同包 shim）+ `diagmap.go`（映射表）—— 见 `M1-design.md` §9.1 |
| 运行 | 必须在容器内（`scripts/in-container.sh`） |
| **禁止** | `go tool compile -E`（给的是语法树，不是词元流） |

### 5.2 golden 格式（冻结）

```text
# golden: <语料相对路径>
# source: oracle | hand-written
== tokens ==
  1:1    package  lit=-
  1:9    name     lit="main"
  1:13   ;        lit="newline"
== diags ==
  4:8    LEX-INVALID-UTF8
```

**四条规范化纪律**（`M1-design.md` §8.2）：

1. **只录有效域内的字段** —— `lit` 只对 `_Name`/`_Literal`/`_Semi` 记录
2. **诊断与词元分两条流、各自有序**（`line→col→code`）
3. **诊断只存 `行:列 + DiagCode`**，不存消息文本（D-M1-9）
4. **确定性** —— 同一输入多次运行字节相同

### 5.2.1 字符级 golden（`T-L07` 立）—— 切面 ① 的主力判据

词元级 golden 在**词元边界**上取样；而位置是**每个字节**推出来的，
`source.nextch` 是 byte-column 语义的唯一来源。所以另有一条**字符级**通道。

| 项 | 值 |
|---|---|
| 语料 | `tools/token-oracle/fixtures/char_corpus.txt` —— **1177 个字节串**（全部单字节 256 + 30 个"结构相关字节"的两两组合 + 27 个有名字的多字节用例；去重保序） |
| golden | `tools/token-oracle/fixtures/char_golden.txt`（3279 行），由 `go run . --chars-golden` 生成 |
| 生成器 | `tools/token-oracle/charcorpus.go`（语料）/ `chargolden.go`（渲染）/ `gosyntax/export.go` 的 `DumpChars` |
| 比对 | `rgoc-lex/tests/test_char_golden.rs`，**逐事件** |
| 实测结果 | **1146 例逐字节一致 + 31 例已登记偏离** |

```text
## <hex>                             ← 用例（输入字节串的 hex；空串为 -）
D <line>:<col>      <CODE>          ← 诊断
C <line>:<col>      U+<HEX> w=<n>   ← 读到一个字符；EOF 不进事件流
```

**与词元级唯一的格式差异：这里不需要排序。** 官方 `nextch` 的 `redo:` 标签在位置
推进**之前**，所以"违规字节先报诊断、再让出下一个字符"是**语义**（每个被跳过的字节
各占一列），不是内部时序 —— 词元级恰好相反（"诊断何时触发"是实现细节，必须分开排）。

**`w` 是 byte-column 语义的直接证据**，必须进 golden。
**合成**的 `U+FFFD`（非法编码的替身）**从不进入事件流** —— 官方 `goto redo` 会重读它，
只作中间值存在；事件流里的 `U+FFFD` 一定来自源码（`w=3`）。

**一处已登记的偏离（首字节为 `0x00`）**：官方**漏报**（它把 NUL 的检查只写在 ASCII
快路径里，而懒填充让首次 `nextch` 的 `buf[0]` 还是 sentinel），我们**总是**报
`LEX-INVALID-NUL`。**两侧都报错**，差别只在代号。
判为**实现产物**、按 D-M1-7 不照抄；偏离在测试里用**谓词 + 形状**双重断言登记
（判据是谓词而不是写死的 hex 列表 —— 本仓三次踩过「写死的键列表变成盲区」）。
详见 `M1-design.md` §5.1 / §8.2.1 / §12 R11。

### 5.3 两种来源必须标注

| 来源 | 覆盖 | 标记 |
|---|---|---|
| oracle 生成 | 除 `T-L-63` 外全部 | `# source: oracle` |
| **手写** | **仅 `T-L-63`**（官方无此行为） | `# source: hand-written` |

---

## 6. unsupported 清单（F4）

| ID | 不支持项 | 归属 | 本阶段处置 |
|---|---|---|---|
| **U12** | **真实 Go 源码的 lex/parse** | **M1/M2** | ✅ **M1 解除这一半**（lex）；`parse` 仍留 M2 |
| U11 | `src/internal/types/testdata/` | M4 | 不跑 |
| U8 | `test/fixedbugs/` 全目录 | 后续 | 不跑 |
| U1–U7、U9、U10、U13、U14 | 见 [`M0-tests.md`](./M0-tests.md) §6 | 后续 | 不跑 |

**⚠️ M1 不引入新的 unsupported 项**（除上表的外来项）。若必须引入，须**单独记录并保留原分母**。

---

## 7. 验证命令、超时与资源上限（F5）

### 7.1 腿甲（crate 内）

```sh
scripts/in-container.sh bash -lc 'cd /work/rgoc && cargo test -p rgoc-lex'                    # 全部
scripts/in-container.sh bash -lc 'cd /work/rgoc && cargo test -p rgoc-lex --test test_char_golden'  # 只看字符级对撞
```

> ⚠️ **紧密循环**里改完源文件马上跑，必须先在容器内 `touch` 那个文件（计划 **G13**）——
> 否则 cargo 可能仍看到缓存的 mtime，**跑的是上一版二进制**（内容/哈希/指纹都对，只有失败清单是旧的）。

### 7.2 腿乙（门禁级 golden）

```sh
scripts/in-container.sh cargo run -p rgoc-driver -- lex <file>   # 产出词元流
scripts/check-lex-golden.sh                                       # 与 golden 逐行 diff
```

### 7.3 四条门禁命令（**判据不变，调用形态有规范**）

四条命令本身不变（`cargo fmt --check` / `check --workspace --all-targets` /
`clippy --workspace --all-targets -- -D warnings` / `test --workspace`），
但**调用形态**必须按 `M1-plan.md` 的 **G14**：

```sh
scripts/in-container.sh bash -lc 'set -eo pipefail; cd /work/rgoc
fail=0
cargo fmt --check > /tmp/g1.log 2>&1 || { echo "  fmt FAIL";    head -20 /tmp/g1.log; fail=1; }
cargo check   --workspace --all-targets > /tmp/g2.log 2>&1 || { echo "  check FAIL";  grep -E "^error" -A6 /tmp/g2.log | head -20; fail=1; }
cargo clippy  --workspace --all-targets -- -D warnings > /tmp/g3.log 2>&1 || { echo "  clippy FAIL"; grep -E "^error" -A8 /tmp/g3.log | head -24; fail=1; }
cargo test    --workspace > /tmp/g4.log 2>&1 || { echo "  test FAIL";   grep -E "^error|FAILED|panicked" /tmp/g4.log | head -20; fail=1; }
if [ $fail -eq 0 ]; then echo "  四条门禁全过"; else echo "  有门禁未通过"; fi
[ $fail -eq 0 ]'
```

**为什么不能逐条调、也不能靠 `&&`**（两条都是实测）：

1. **容器内 `set -e` 生效时，任何显式 `exit N`（含 `exit 0`）都被压成 1**
   （`bash -lc 'set -e; exit 7'` → 1）⇒ 退出码信息全毁，所以末条写成 `[ $fail -eq 0 ]`。
2. **`cmd && echo OK` 形态不中止脚本**（`set -e` 对 AND-OR 列表的非末项不生效），
   于是**早期失败被后续成功掩盖**（实测 `false && echo OK; true` → 退出码 **0**）。
3. 取退出码时**不要 `cmd | grep`** —— 管道的状态会被覆盖。用变量捕获。

### 7.4 资源上限（冻结）

| 项 | 值 | 依据 |
|---|---|---|
| 单文件上限 | **64 MiB** | `M1-design.md` §7.3（语料最大文件 3.77 MB 的 17 倍） |
| 腿甲总耗时 | ≤ **60 s** | 推断自 `03` §6.1 的 smoke ≤5 min |
| 腿乙总耗时 | ≤ **300 s** | `03` §6.1：smoke ≤5 分钟 |
| 单文件处理 | ≤ **10 s** | 留余量 |

> **超时的处置**：记 `Timeout` / `ResourceFailure`，**仍在分母里**（`03` §3.3）。

---

## 8. 门禁判定与冻结规则

| 门禁 | 由哪些 ID 判定 | 分母 | 通过条件 |
|---|---|---|---|
| **LEX-01..13** | §2 表中全部 | **13 项** | **每项至少一条 ID**，且全部通过 |
| **词法 golden** | `T-L-10`…`T-L-65` | ⬜ 冻结时填入 | **100%** 通过 |
| **负例** | `T-L-50`…`T-L-54` + §4 的 N1–N12 | ⬜ 冻结时填入 | 100%，且**双向断言齐备** |
| **`SM-M1-LEX-GOLDEN`** | 腿乙全量 diff | 语料文件数 | diff 全空 |
| **`SM-M0-NATIVE-HELLO`** | `T-S3-03`（M0 的） | 1 | **继续通过**；**不得改写** |
| **GAP-01** | LEX 13 项的四维归属表 | 13 | 已填（**不填完 210 个**） |

**判定纪律**（沿用 [`M0-tests.md`](./M0-tests.md) §8）：

1. 必需 ID **100% 通过**、**无基建失败 / 不稳定失败**（`03` §3.3）
2. 白名单**开工前冻结**，任何新增跳过须**单独记录**并保留原分母报告
3. **不得**通过放宽比较器、把跳过记为 pass、或缩小分母来「达成」门禁
4. **断言必须反向验证** —— 写完一条断言要故意制造它能抓的 bug，确认会变红；
   不红就是恒真断言，必须重写（本仓 T37/T38/T39/T47 连续四轮同形的坑）

---

## 9. 待办

| # | 事项 | 归属 |
|---|---|---|
| 1 | **冻结本文档**（F1–F6 六项） | 用户确认 |
| 2 | 裁决 **LEX-06 的 47 / 48**（本表按 48 写的） | 用户 |
| 3 | 裁决 **GAP-02 正则子集**（不阻塞 M1，但 M1 结束前必须定） | 用户（M1 退出前） |
| 4 | 产出 `M1-plan.md` | 下一步 |
| 5 | 填 §3 / §8 里的「⬜ 冻结时填入」两处分母 | 冻结时 |
