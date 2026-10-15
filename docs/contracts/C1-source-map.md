# C1 契约：SourceMap / 位置表示 / 诊断排序

> ## M1 起，本契约是**位置跟踪的证据**
>
> M0 时代它只固定**表示形式**（`Pos`/`Diag` 的字段与排序规则）—— 那时没有
> lexer，没有任何「从源码文本计算位置」的能力（历史见 §8 变更记录首行）。
> **M1（`rgoc-lex`）落地后这一半已实现**：
>
> - `Pos { file: FileId, line, col }` + `FileTable` —— `rgoc-lex/src/pos.rs`
> - `DiagCode`（代号的枚举）—— `rgoc-lex/src/diag.rs`。**当前 35 个**：
>   23 个 `LEX-`（M1，官方 scanner 可产出）+ 12 个 `SYN-`（M2 语法，全集已由 `T-AP16` 冻结、`T-AP20` 增 1 个 H 判据）
>
> **M2（`rgoc-parse`）的 parser 侧位置语义已落地**（见 §2.3，`T-AP15`/`T-AP18`）：
> `//line` 重映射、节点 `Pos` 的两套判据（`Pos()` / `StartPos`）、`File.eof`。
>
> **仍未实现**（§6）：SourceMap 的**多源位置映射**（宏展开/内联的一节点多位置）与
> offset ↔ `(line, col)` 的**随机访问换算** —— 归 M4。
> 不得因本契约完整就认为那两件事也完成了。

**状态**：**完整**（M1 / `T-L20` 升级）　|　**契约级别**：**完整**　|　**实现落点**：`rgoc-lex`（`pos.rs` / `diag.rs`）　|　**消费者**：M1（lexer）、M2（parser）、M4（诊断）、M5（HIR）

---

## 1. 范围

| 本契约**规定** | 本契约**不规定**（留给谁） |
|---|---|
| 位置的字段构成与基数（几基、字节还是 rune） | 位置**怎么算出来**（M1 已落地逐字符推进；**随机访问换算**仍留 M2/M4） |
| 诊断的字段构成 | 诊断的**具体文案**（各检查器自定，但排序键受本契约约束） |
| 诊断的**排序规则**（保证输出唯一 ⇒ E6 可复现） | 诊断的**抑制 / 去重 / 聚类**策略 |
| 位置的显示格式 | 跨文件的多行上下文展开 |
| — | **SourceMap 的实现**（本文件只固定它的**需求**，见 §6） |

---

## 2. 位置表示：`Pos`

```rust
pub struct Pos {
    pub file: FileId,   // 文件标识：FileTable 的下标，**不是路径字符串**
    pub line: u32,      // 行号，1 起
    pub col:  u32,      // 列号，1 起，**字节列**
}
```

### 2.0 `FileTable`：`file` 的解读方式

`Pos.file` 只是**一张表的下标**；文件名由 `FileTable` 持有：

| API | 语义 |
|---|---|
| `FileTable::intern(&str) -> FileId` | 登记文件名；**同名字重复调用返回同一个 `FileId`**（`Vec` 线性查，不依赖哈希行为） |
| `FileTable::name(FileId) -> &str` | 取回文件名；**越界 panic**（有意：静默返回 `""` 会让错误位置变成 `<空>:3:7` 这种看似正常的东西） |
| `FileTable::format_pos(Pos) -> String` | 按 §2.2 渲染 |

**`Pos` 自己不实现 `Display`** —— 它只有 `FileId`，**解析不出文件名**。若给它一个
`Display`，打出来会是 `0:3:7` 这种「看起来像位置、其实缺文件名」的字符串，
与 §2.2「必须与 oracle 的 `文件:行号` 前缀兼容」直接冲突。所以格式化一律经
`format_pos` —— **让漏掉文件名这件事无法发生**。

**一个已知局限（登记在案，不是隐藏缺陷）**：`FileId` **不是全局唯一**的 ——
不同表产出的 `FileId(0)` 是相等的。若把 A 表的 Id 拿去查 B 表，下标恰好落在
B 表范围内时**不会报错**，会取到 B 表里无关的文件名 —— 这是**检测不出来**的。
M1 的约束是**每次编译只有一张表**（`Lexer` 持有它），所以这个局限不会触发。
M3 引入多包多文件时若出现跨表混用，需要给 `FileId` 加上表标识（或改成
`(TableId, Index)` 二元组）—— **那是一次契约变更，不是修 bug。**

### 2.1 三条硬规定

1. **`line` 与 `col` 都是 1 起**（不是 0 起）。
   0 起是 JS/Python 的习惯，与 Go 的 `文件:行:列` 输出不一致 ——
   混用会让「工具报的行号」与「Go oracle 报的行号」差 1，而这种差 1 极难排查。
   **理由**：`02-test-inventory.md` 的 errorcheck 判定要求对齐 oracle 的
   `文件:行号` 前缀（R3/R4）；基数错了，那条对齐永远失败。

2. **`col` 是字节列，不是 rune 列**。
   定位以**字节**为准，跨语言比较才不会歧义。若按 rune 计数，
   同一行里含中文/emoji 的用例会与 Go 的列号错位。
   **代价**：一个 UTF-8 字符可能横跨多个 `col` 值 —— 这是**有意的取舍**，
   换取与 oracle 逐字节可比。

3. **`file` 是文件标识，不是路径本身**。
   M0 时代 spike 的 fixture 用手写标签（如 `s1_fixture`），因为它们**没有对应源文件**。
   M1 起 `Lexer::from_path` 把调用方传入的路径**原样** intern（driver 的
   `# golden:` 行因此能回显它）。契约**不规定**它长什么样，只规定：
   要变出名字必须经过 `FileTable`（§2.0）。

### 2.2 显示格式

```text
Pos  →  {file}:{line}:{col}          例：test/helloworld.go:3:7
```

（`{file}` 经 `FileTable::name` 解析；渲染入口是 `format_pos`，**不是** `Pos` 的 `Display`。）

这条格式**必须**与 Go oracle 的 `文件:行号` 前缀兼容（R3/R4 的匹配前提），
且 `col` 为可选（oracle 只输出到行号）。

### 2.3 parser 侧位置语义（M2 / `T-AP15` + `T-AP18` 落地）

M2 的 parser 在 §2 的三元组之上**增补**了三条语义。它们是官方行为的直译，
验证载体是 `T-A-95`（errcheck 的位置匹配）与 oracle 对撞。

#### 2.3.1 `//line` 重映射：基准链进 `Pos`

官方 `Pos = MakePos(base, line, col)` —— 基准是 `Pos` 的**一部分**。我们同构：

```rust
pub struct Pos {
    pub file: FileId,
    pub line: u32,
    pub col: u32,
    pub base: Option<BaseId>,   // `//line` 基准链节点（NonZeroU32 空位优化，4 字节）
}
```

- `base == None` ⇒ 无指令，`rel_line`/`rel_col` **退回绝对**行列（M1 行为逐条不变）。
- 基准链存 `PosBases`（官方指针链 → **表 + 下标**，`Pos` 保持 `Copy`/`Ord`/`Hash`）。
- **file base 的 `line`/`col` 必须是 `1`/`1`**（官方 `linebase`/`colbase`）；
  写成 0 会让全仓 `rel_*` 变 0。
- `Parser::next()` 的顺序是「取词元 → 处理指令消息 → 按当前 base 重算 pos」
  —— 反了会让 `//line` 后第一个词元用旧基准。
- ⚠️ **`Pos` 的 `Ord` 仍按绝对坐标**（§4 的排序键不变）：官方 `ErrorList.Less`
  用 rel 坐标，两者只在「同文件多基准且行号回退」时不同序 —— 已在 ledger
  登记为已知偏离，等待有用例需要时再改（M2 对撞 31 文件无一触发）。

#### 2.3.2 节点位置：`Pos()`（存储位）与 `StartPos`（修正位）是两套判据

| 判据 | 语义 | 例 |
|---|---|---|
| `Expr::pos()` | **存储位** = 官方 `Pos()`，节点构造时钉进的那个位置 | `x[i]` → `[`；`f(x)` → `(`；`a.b` → `a` |
| `Expr::start_pos()` | **修正位** = 官方 `StartPos`（`positions.go:10-111`），**按需**回溯到子节点 | `Index/Selector/Assert→X`；`Call→Fun`；二元 `Operation→X`；`CompositeLit→Type` |

- dump **不含位置**（C6），所以对撞抓不到位置分歧 —— 位置语义由
  errcheck 的位置匹配（`T-A-95`）与 @ 腿覆盖。
- 官方 parser 全文件**只有一处**用 `StartPos`（`paramList` 的
  `errPos = StartPos(typ)`，`parser.go:2077`）—— `missing type parameter name`
  的报错列。直译时用 `pos()` 会**右偏一列**（T-AP18 实测、已修）。
  **新报错点必须先核对官方用的是哪一套。**

#### 2.3.3 `File.eof`

官方 `File.eof = p.tok.pos`（收尾时的 EOF 词元位置）。夹具 `package p\n` ⇒
`eof = (2, 1)`。载体：`test_smoke.rs` 的 `t_a_99`。

---

## 3. 诊断表示：`Diag`

```rust
pub struct Diag {
    pub pos: Pos,        // 位置
    pub code: DiagCode,  // 机器可读代号（**枚举**），稳定且可排序
    pub message: String, // 人类可读描述
}
```

### 3.1 `code` 是契约的一部分，不是注释

`code` 看起来只是个标识，但它**参与排序**（§4），因此**必须是稳定且可排序的**：

- **稳定**：改了 `code` 会改变诊断顺序 ⇒ 改变 E6 / golden 的比对结果 ⇒ 回归。
- **可排序**：统一 `大写-前缀-小写词`（`LEX-...`），无大小写混排陷阱。
- **可分组**：前缀标识来源子系统（`LEX-` = 词法阶段），
  这样「某一类问题」能被一次性筛出来。

**反例**（不要这样）：把 `message` 当排序键 —— 文案会随措辞调整变化，
一改就重排全部诊断。

**M1 落地时才发现、现升为契约级的两条规则**：

1. **`DiagCode` 的声明顺序 == `as_str()` 的字典序**。Rust 侧「同位置按 code 排序」
   走 `Ord`（= 声明顺序），golden 侧按字符串 —— 两者不一致时，**只在两条诊断
   同位置时才暴露**（F47/F48 就是这样的实例）。加新代号必须**插到它字典序
   该在的位置**，让那条断言告诉你有没有放对。

2. **码表是枚举出来的，且两侧必须逐字一致**。第一版凭印象从官方 scanner 的
   报错消息里挑，漏了 4 个代号（`LEX-BAD-DIGIT` / `LEX-BAD-RADIX-POINT` /
   `LEX-STRING-NEWLINE` / `LEX-ESCAPE-BAD-DIGIT`）—— 漏掉时**不会报错**，
   那几条消息出现时行为未定义。现在是枚举 `scanner.go` / `source.go` 全部
   `errorf` 调用点（26 处 ⇒ 去重 25 条模板）得到 **22 个官方代号**，
   再加 1 个本仓自己的限制（`LEX-EXCEEDS-FILE-LIMIT`，§7.3 的 64 MiB 上限），
   共 **23 个 `LEX-` 代号**。Go 侧 `tools/token-oracle/diagmap.go` 与 Rust 侧
   `DiagCode` 的对应关系由**跨语言对撞测试**钉住，不靠人记得同步。

3. **（M2 增补）前缀是「阶段」分区的，跨语言对撞只覆盖能对上的那一段**。
   M2 的 parser 需要自己的语法诊断，代号加 `SYN-` 前缀（同上，声明顺序
   == `as_str()` 字典序）。但 `tools/token-oracle` 的自检要求**每个代号都能被
   官方 `scanner.go` / `source.go` 的消息产出** —— `SYN-` 代号在词法表里
   永远不可能被产出，塞进去只会多一串要豁免的死条目。因此：
   - `tools/token-oracle/diagmap.go` 只镜像 `LEX-` 段，跨语言对撞断言
     **限定在 `LEX-` 子表**（两边都先过滤、再比对）；
   - `SYN-` 段由 `tools/ast-oracle` 的镜像表覆盖（`T-AP16` 冻结全集）。
   「阶段白名单」本身有断言钉住（两个前缀都必须**至少有一个代号在用**），
   否则留一个空前缀就是一条恒真的豁免。

**`message` 不参与任何比对**：`03` §3.4 规定诊断先对齐**拒绝/接受、位置和
预定义类别**，文本另列指标；golden 只存 `行:列 + DiagCode`（D-M1-9）。

### 3.2 显示格式

```text
Diag →  {pos}: {code}: {message}
        例：a.go:1:2: LEX-INVALID-UTF8: 非法 UTF-8 编码
```

---

## 4. 诊断排序规则（**现在就冻结**）

排序键，按优先级：

1. `Pos.file` —— **按路径字典序**（**不按解析顺序**）
2. `Pos.line` —— 升序
3. `Pos.col` —— 升序
4. `Diag.code` —— 升序

### 4.1 为什么第 1 条不按解析顺序

诊断输出**不应依赖求值顺序**。若按解析/执行顺序排，
那么两条语义等价的程序只要语句顺序不同，诊断顺序就不同 ——
E6 的「三次结果一致」会随机失败，且这种失败**极难定位**（看起来像随机性）。

### 4.2 为什么必须有第 4 条（`code`）

**同一位置的多条诊断**，若顺序取决于哈希表迭代序，则输出不确定。
第 4 条把顺序**完全钉死**：`sort_by(pos, then code)` 是全序，不留自由度。

> 这条不是可有可无的洁癖：E6 要求「重复执行 3 次结果一致」，
> 而「同一位置两条诊断的顺序」正是最容易泄漏哈希随机性的地方。

### 4.3 消费式 API：`DiagBag::into_sorted()`

```rust
impl DiagBag {
    pub fn push(&mut self, d: Diag);
    pub fn into_sorted(self) -> Vec<Diag>;   // ← 消费 self
}
```

**为什么 `into_sorted` 消费 `self` 而不是返回 `&[Diag]`**：

若返回引用，调用方可能拿它去渲染而**忘记先排序** ——
那就绕过了本契约存在的唯一理由。所以排序是**类型上不可绕过**的一步。

> **落地证据**（M1 起）：`rgoc_lex::DiagBag`（`diag.rs`）+
> 测试「同位置的诊断按 code 排序_保证输出唯一」与「diag_bag_into_sorted_按_文件_行_列_code」。
> （M0 时代的那份在 `rgoc-hir`，随 M5 替换 `rgoc-hir` 而退场。）

---

## 5. 与 harness 的接口：⚠️ **当前是缺口，不是已实现的接口**

**`TestCase`（`rgoc_harness::ir::TestCase`）当前有 15 个字段，其中没有 `Pos`。**

```text
id / rel_path / input_files / mode / instruction_args / build_tags / target /
imports / feature_deps / comparator / expected / limits / seed / milestone / unsupported
```

> ⚠️ **本节曾写成「`TestCase` 持有一个 `pos` 字段」—— 那是错的，代码里没有。**
> 现如实记录为**缺口**：一条用例失败时，「哪个文件、哪条用例、期望是什么」中的
> **位置**只能由 `rel_path` 加上**行号未知**来回答。

**缺口与归属**：

| 缺口 | 影响 | 归属 |
|---|---|---|
| `TestCase` 无位置字段 | 失败报告只能定位到**文件级**，不能到行 | **M4**（静态检查闭环产出诊断时） |
| 用例**内部**的位置（哪一行出错） | 同上 | **M2**（parser 保真后才有行号） |

> **M1 没有改这张表**：C2 按 `M1-design.md` §10.2 判定**不动**，
> 缺口原样留到 M4。补字段不会让任何东西变好，只会让两套字段数说法并存 ——
> **如实记录缺口，比加一个占位字段更有用。**

### 5.1 比较器的位置改写

比较器（R3/R4）产出的诊断位置先做 `replacePrefix`
（把 oracle 的绝对路径换成用例的相对路径），再进入本契约的排序 ——
详见 `C2-test-ir.md` §比较器规格。**这一步是已实现的**（`rgoc_harness::compare`）。

---

## 6. SourceMap：本契约只提出**需求**

M1/M2 需要回答的三个问题（本契约的输入）：

| # | 问题 | 归属 | 现状 |
|---|---|---|---|
| 1 | 一个生成物节点如何映射回**多个**源码位置？（宏展开、内联） | M1/M2 | 未涉及 |
| 2 | 字节偏移 ↔ `(line, col)` 的双向换算，谁来缓存？ | M1 | **部分落地**：lexer 逐字符**推进** `line`/`col`（`test_source.rs` 的中文/emoji 用例即证据），但**没有随机访问换算 API** —— `FileTable` 明确不做（§2.0）。「给定 offset 反查行列」仍是 M2/M4 的活 |
| 3 | 跨文件的映射（如 inlining 后的调用点）如何表达？ | M2/M5 | 未涉及 |

**M1 只固定了「位置是一个三元组」这一前提**（§2），
它对上面三个问题都足够 —— 因为无论映射多复杂，**诊断最终仍要落到一个 `Pos` 上**。

---

## 7. 本契约的验证方式

| 断言 | 载体 |
|---|---|
| `Pos` 的 `Ord` 顺序是 文件→行→列 | `rgoc-lex/tests/test_pos.rs` `pos_的_ord_顺序是_文件_行_列` |
| 同名字重复 intern 得到同一个 `FileId` | `test_pos.rs` `file_table_同名字重复_intern_得到同一个_id` |
| 显示格式 `文件:行:列`（经 `format_pos`） | `test_pos.rs` `format_pos_形如_文件_行_列` |
| 越界 `FileId` 必须 panic 而不是返回空名 | `test_pos.rs` `越界的_file_id_必须炸而不是返回空名` |
| `col` 是**字节列**（中文占 3 列 / emoji 占 4 列） | `test_source.rs` `中文标识符让列按字节前进` / `emoji_占四个字节列` |
| 换行让行号加一且列归零 | `test_source.rs` `换行让行号加一且列归零` |
| 排序 文件→行→列→code | `test_diag.rs` `diag_bag_into_sorted_按_文件_行_列_code` |
| 同位置诊断按 `code` 排序，**保证输出唯一** | `test_diag.rs` `同位置的诊断按_code_排序_保证输出唯一` |
| 代号表恰为冻结总数且无重复 | `test_diag.rs` `代号表恰为冻结总数且无重复` |
| `as_str()` 前缀落在**阶段白名单**内，且每个前缀都**有人在用** | `test_diag.rs` `diag_code_的_as_str_前缀落在阶段白名单内` |
| 声明顺序 == `as_str()` 字典序 | `test_diag.rs` `diag_code_的声明顺序与_as_str_字典序一致` |
| 码表与 Go 侧映射表逐字一致（跨语言，**限定 `LEX-` 子表**） | `test_diag.rs` `diag_code_的码表与_go_侧映射表逐字一致` |

> 这些测试随 `rgoc-lex` 一起入库。M5 替换 `rgoc-hir` 时，M0 时代那份
> spike 级副本随之退场（§4.3 的注记）。

---

## 8. 变更记录

| 日期 | 变更 | 依据 |
|---|---|---|
| 2026-10-07 | **初稿创建**（T48）。从 `rgoc-hir/src/diag.rs` 反推位置表示与排序规则；显式标注**留位**（那时没有 lexer，本文件不构成「已实现位置跟踪」的证据） | Phase 3 的 `rgoc-hir`（T40）+ 门禁 E6/E7 |
| 2026-10-12 | **由留位升为完整**（T-L20）。`Pos.file`：`String` → `FileId`；新增 `FileTable`（§2.0）与 `DiagCode`（`code`：`String` → 枚举，§3.1 两条新规则）；§6 第 2 问登记为**部分落地**；验证落点由 `rgoc-hir` 迁到 `rgoc-lex`（§7）。**排序规则不变** | M1 的 `rgoc-lex`（T-L01/T-L02 + 全部切面测试）；`M1-design.md` §10.1 预定的变更范围 |
| 2026-10-14 | **`DiagCode` 按阶段分区**（M2 / `T-AP06`）。新增 3 个 `SYN-` 前缀的语法代号（工作集）⇒ 23 → **26**；§3.1 新增第 3 条；跨语言对撞由「全表逐字一致」**收窄为 `LEX-` 子表**（`tools/token-oracle` 只镜像词法段），`SYN-` 段归 `tools/ast-oracle`（`T-AP16`）；§7 两行断言随测试改名同步。**排序规则不变** | M2 的 parser 需要自己的语法诊断；`M2-design.md` `D-M2-2`/`D-M2-4` |
| 2026-10-15 | **parser 侧位置语义收口**（M2 / `T-AP15`+`T-AP18`+`T-AP20`）。新增 §2.3（`//line` 基准链进 `Pos`、`Pos()`/`StartPos` 两套判据、`File.eof`）；头部注记：`DiagCode` 26 → **35**（`T-AP16` 冻结 8 个 + `T-AP20` 的 H 判据 1 个）；§6 「随机访问换算」归属 M2→**M4**（M2 无需它）。**排序规则不变**；`Pos.Ord` 按绝对坐标不变（偏离已登记） | `M2-plan.md` `T-AP21` Step 1；§2.3 的每条都有测试载体 |
