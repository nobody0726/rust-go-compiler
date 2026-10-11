# M1 词法与位置 · 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 `rgoc-lex` —— 把 Go 源文件的字节流切成带位置的词元流，并产出诊断流；覆盖 LEX-01..13。

**Architecture:** 三层拆分（`SourceReader` 字符层 / `SemicolonPolicy` 状态机 / `Lexer` 分派层），
分派与判定顺序照官方 `gc/syntax/scanner.go` 的 `next()` 写以保证行为对齐，但**结构上拆开以便单测**。
词元用和类型（`TokenKind`），`lit` 的有效域从注释变成类型约束。

**Tech Stack:** Rust 1.98.1（edition 2024，`unsafe_code = "forbid"`）、cargo workspace、容器内构建（`scripts/in-container.sh`）、Go 1.27.1 作 oracle。

**Spec:** [`M1-design.md`](./M1-design.md)（设计决策 D-M1-1~10、事实基线 F1–F20）
**测试清单:** [`M1-tests.md`](./M1-tests.md)（冻结的测试 ID 与判据口径）

---

## Global Constraints

以下每条都是**全局要求**，每个任务隐含包含：

| # | 约束 | 出处 |
|---|---|---|
| G1 | **所有构建与测试必须在容器内**：`scripts/in-container.sh <命令>`。daemon 不可用时**报错不降级** | `04` §7 |
| G2 | 四条门禁命令必须全过：`cargo fmt --check` / `cargo check --workspace --all-targets` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` | `AGENTS.md` §4.3 |
| G3 | `unsafe_code = "forbid"`（workspace 级 lint） | `rgoc/Cargo.toml` |
| G4 | **位置：`line`/`col` 都从 1 起；`col` 的单位是字节** | `C1` §2.1 |
| G5 | **`rgoc-lex` 不得依赖任何其他 `rgoc-*` crate**（反向依赖 `driver → lex` 是允许的） | `M1-design.md` §4 |
| G6 | **不得使用 `go tool compile -E`** 当 token oracle | `03` §M1 |
| G7 | **不引入第三方依赖**（Unicode 表由 oracle 生成） | `M1-design.md` D-M1-6 |
| G8 | **单文件上限 64 MiB**，超限报 `LEX-EXCEEDS-FILE-LIMIT`，返回只含 `Eof` 的流 | `M1-design.md` §7.3 |
| G9 | **分母不缩 / 八类判定不合并 / 断言必须反向验证** | `03` §3.3、`M0-tests.md` §8 |
| G10 | Rust 侧命名：**中文标识符不得夹大写 ASCII**（会触发 `non_snake_case` 打爆 `-D warnings`） | 本仓 T37 复发过 |
| G11 | **容器内 cargo 命令的确切形态**：`scripts/in-container.sh bash -lc 'cd /work/rgoc && cargo <args>'`。容器的工作目录是 `/work`（仓库根），而 cargo workspace 在 `/work/rgoc`；`-l` 必需（工具链在 `/etc/profile.d/50-rgoc-toolchains.sh` 里加进 PATH）。**本文档正文里的 `cargo` 命令一律按本条解释** | 执行期实测（R1） |
| G12 | **凡是要做「逐字节比较」的 stdout，必须设 `RGOC_TTY=0`**。`scripts/in-container.sh` 在 stdin 是 tty 时加 `-it`，而伪终端默认 ONLCR 把 `\n` 翻成 `\r\n` —— **终端里肉眼看不出**，但会让 golden 每行带 CR。G11 那类 `cargo test` 的 grep 不受影响（CR 在行尾），**只有产出被冻结的文件时才会咬人** | 执行期实测（R6） |

---

## Review Focus

以下五类是**设计里蕴含、但没有任务的测试直接覆盖**的输入 ——
它们最可能在真实使用中咬人。**每一行都必须在下面指定的任务里加一条测试钉住。**

| # | 输入 / 条件 | 合理的人会期待什么 | 钉在哪个任务 |
|---|---|---|---|
| RF1 | `..` 回退之后紧跟**多字节字符**（如 `1..中`） | 回退后 `col` 不漂移；`中` 的位置仍与 oracle 相等 | **T-L09** |
| RF2 | 跨行块注释出现在 **`nlsemi` 为假**的语境（如 `(` 之后） | **不**产出分号（注释只在 `nlsemi` 为真时等价于换行） | **T-L13** |
| RF3 | `_` 出现在**指数部分**：`1e1_0` / `0x1p1_0` / `1_0e1_0` | 与 oracle 一致（合法或报 `LEX-BAD-SEP`，不能自己发明规则） | **T-L10** |
| RF4 | 文件在**词元中间**结束：`"abc`（EOF）+ 前面 `nlsemi` 为真 | 先产出未终止字符串的诊断与词元，**再**补 EOF 分号 —— 顺序不能反 | **T-L15** |
| RF5 | EOF 补出的分号的**位置** | 与 oracle 相同（不在文件长度之外凭空造列号） | **T-L15** |

---

## 阶段 A：基础设施

### Task T-L01: 创建 `rgoc-lex` crate 与 `pos.rs`

**Files:**
- Create: `rgoc/crates/rgoc-lex/Cargo.toml`
- Create: `rgoc/crates/rgoc-lex/src/lib.rs`
- Create: `rgoc/crates/rgoc-lex/src/pos.rs`
- Create: `rgoc/crates/rgoc-lex/tests/test_pos.rs`
- Modify: `rgoc/Cargo.toml`（`members` 加 `crates/rgoc-lex`）

**Interfaces:**
- Consumes: 无
- Produces:
  - `rgoc_lex::FileId`（`Copy + Ord + Hash`，带 `index(self) -> u32`）
  - `rgoc_lex::FileTable`（`new()` / `intern(&mut self, &str) -> FileId` / `name(&self, FileId) -> &str` / **`format_pos(&self, Pos) -> String`**）
  - `rgoc_lex::Pos { pub file: FileId, pub line: u32, pub col: u32 }`（`Copy + Ord + Hash + Debug`）
  - ⚠️ **`Pos` 不实现 `Display`**（R2）：它只有 `FileId`，单独打印会漏掉文件名，与 `C1` §2.2 冲突。
    格式化一律经 `FileTable::format_pos`。

- [ ] **Step 1: 写失败测试**

`tests/test_pos.rs`：

```rust
use rgoc_lex::{FileId, FileTable, Pos};

#[test]
fn file_table_同名字重复_intern_得到同一个_id() {
    let mut t = FileTable::new();
    let a = t.intern("a.go");
    let b = t.intern("a.go");
    let c = t.intern("b.go");
    assert_eq!(a, b);
    assert_ne!(a, c);
    assert_eq!(t.name(a), "a.go");
}

#[test]
fn format_pos_形如_文件_行_列() {
    // ★ R2：格式由 FileTable 提供，不是 Pos::to_string
    let mut t = FileTable::new();
    let f = t.intern("test/helloworld.go");
    let p = Pos { file: f, line: 3, col: 7 };
    assert_eq!(t.format_pos(p), "test/helloworld.go:3:7");
}

#[test]
fn pos_的_ord_顺序是_文件_行_列() {
    let mut t = FileTable::new();
    let a = t.intern("a.go");
    let b = t.intern("b.go");
    let mut v = vec![
        Pos { file: b, line: 1, col: 1 },
        Pos { file: a, line: 2, col: 1 },
        Pos { file: a, line: 1, col: 9 },
        Pos { file: a, line: 1, col: 2 },
    ];
    v.sort();
    assert_eq!(v[0], Pos { file: a, line: 1, col: 2 });
    assert_eq!(v[1], Pos { file: a, line: 1, col: 9 });
    assert_eq!(v[2], Pos { file: a, line: 2, col: 1 });
    assert_eq!(v[3], Pos { file: b, line: 1, col: 1 });
}
```

- [ ] **Step 2: 跑测试确认失败**

Run:（按 G11）`scripts/in-container.sh bash -lc 'cd /work/rgoc && cargo test -p rgoc-lex'`
Expected: FAIL —— `package ID specification \`rgoc-lex\` did not match any packages`（exit 101）

- [ ] **Step 3: 实现**

`Cargo.toml`（workspace 成员）：`version.workspace = true` / `edition.workspace = true` /
`rust-version.workspace = true` / `license.workspace = true` / `[lints] workspace = true`，无依赖。
`lib.rs`：`pub mod pos;` + `pub use pos::{FileId, FileTable, Pos};`
`pos.rs`：三个类型按 Interfaces 实现。`FileTable` 用 `Vec<String>` 线性查（M1 的文件数是个位数；
不要提前上 `HashMap` —— 那会让"同名字得同 Id"的断言依赖哈希行为）。
**`Pos` 只 derive `Debug`，不实现 `Display`**（R2）。

- [ ] **Step 4: 跑测试确认通过**

Run: 同 Step 2
Expected: 3 passed

- [ ] **Step 5: 提交**

```bash
git add rgoc/Cargo.toml rgoc/crates/rgoc-lex
git commit -m "feat(lex): 创建 rgoc-lex crate 与 Pos/FileId/FileTable（C1 契约）"
```

---

### Task T-L02: `diag.rs` —— Diag / DiagBag / DiagCode

**Files:**
- Create: `rgoc/crates/rgoc-lex/src/diag.rs`
- Create: `rgoc/crates/rgoc-lex/tests/test_diag.rs`
- Modify: `rgoc/crates/rgoc-lex/src/lib.rs`

**Interfaces:**
- Consumes: `T-L01` 的 `Pos`、`FileId`
- Produces:
  - `rgoc_lex::DiagCode`（19 个变体，见 `M1-design.md` §7.2；`Copy + Ord + Debug`，`as_str(self) -> &'static str`）
  - `rgoc_lex::Diag { pub pos: Pos, pub code: DiagCode, pub message: String }`
  - `rgoc_lex::DiagBag`（`push(&mut self, Diag)` / `into_sorted(self) -> Vec<Diag>` / `len()` / `is_empty()`）

- [ ] **Step 1: 写失败测试**

`tests/test_diag.rs`：

```rust
use rgoc_lex::{Diag, DiagBag, DiagCode, FileTable, Pos};

fn p(f: u32, line: u32, col: u32) -> Pos { /* 用 FileId 的测试构造器 */ }

#[test]
fn diag_bag_into_sorted_按_文件_行_列_code() { /* 乱序 push 4 条，断言排序后顺序 */ }

#[test]
fn 同位置的诊断按_code_排序_保证输出唯一() {
    // 同一 Pos 上放两个不同 code，正反两个插入顺序，结果必须相同
}

#[test]
fn diag_code_的_as_str_前缀均为_LEX_() {
    for c in DiagCode::ALL { assert!(c.as_str().starts_with("LEX-")); }
}

#[test]
fn diag_code_的声明顺序与_as_str_字典序一致() {
    // Ord 是 declaration order，golden 按字符串排 —— 两者必须一致，
    // 否则「同位置按 code 排序」在 Rust 侧与 golden 侧会给出不同顺序
    let mut by_ord = DiagCode::ALL.to_vec();
    by_ord.sort();
    let mut by_str = DiagCode::ALL.to_vec();
    by_str.sort_by_key(|c| c.as_str());
    assert_eq!(by_ord, by_str);
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `scripts/in-container.sh cargo test -p rgoc-lex --test test_diag`
Expected: 编译失败 —— `DiagBag` / `DiagCode` 不存在

- [ ] **Step 3: 实现**

`DiagCode`：19 个变体，**声明顺序按 `as_str()` 的字典序排列**（这条由 `diag_code_的声明顺序与_as_str_字典序一致` 钉住）。
`DiagBag::into_sorted` **消费 `self`** 并 `sort()`（`Pos` 的 `Ord` 已按 file→line→col，再按 `code`）——
返回引用会让调用方忘记排序，那就绕过了这个类型存在的唯一理由（`C1` §4.3）。

- [ ] **Step 4: 跑测试确认通过**

Run: `scripts/in-container.sh cargo test -p rgoc-lex --test test_diag`
Expected: 4 passed

- [ ] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-lex
git commit -m "feat(lex): Diag/DiagBag/DiagCode（C1 排序规则 + 23 个诊断代号）"
```

---

### Task T-L03: `token.rs` —— TokenKind 及配套枚举

**Files:**
- Create: `rgoc/crates/rgoc-lex/src/token.rs`
- Create: `rgoc/crates/rgoc-lex/tests/test_token.rs`
- Modify: `rgoc/crates/rgoc-lex/src/lib.rs`

**Interfaces:**
- Consumes: `T-L01` 的 `Pos`
- Produces:
  - `rgoc_lex::Token { pub pos: Pos, pub kind: TokenKind }`
  - `rgoc_lex::TokenKind`（7 个变体）
  - `rgoc_lex::LitKind`（`Int`/`Float`/`Imag`/`Rune`/`String`）
  - `rgoc_lex::OpForm`（`Plain`/`Assign`/`IncDec`）★ D-M1-10
  - `rgoc_lex::SemiOrigin`（`Explicit`/`Newline`/`Eof`）
  - `rgoc_lex::Keyword`（25 个；`lookup(&str) -> Option<Keyword>`、`as_str(self) -> &'static str`）
  - `rgoc_lex::Operator`（语义枚举；`prec(self) -> u8`）
  - `rgoc_lex::Delim`（10 个）

- [ ] **Step 1: 写失败测试**

`tests/test_token.rs`：

```rust
use rgoc_lex::{Keyword, OpForm, Operator};

#[test]
fn keyword_lookup_25_个全部命中() { /* 25 个字符串逐个断言 lookup 有值且 as_str 往返一致 */ }

#[test]
fn keyword_lookup_对预声明标识符返回_None() {
    // 实测事实 F7：这些在官方也是 _Name
    for s in ["true", "false", "nil", "iota", "_", "println", "append", "int", "error", "any"] {
        assert_eq!(Keyword::lookup(s), None, "{s} 不是关键字");
    }
}

#[test]
fn op_form_无损_加号三种写法互不相等() {
    // D-M1-10 的核心断言
    let plus       = (OpForm::Plain,  Operator::Add);
    let plus_assign= (OpForm::Assign, Operator::Add);
    let plus_plus  = (OpForm::IncDec, Operator::Add);
    assert_ne!(plus, plus_assign);
    assert_ne!(plus, plus_plus);
    assert_ne!(plus_assign, plus_plus);
}

#[test]
fn operator_prec_与_spec_一致() {
    // spec:L268-282 的五级优先级：|| 1 / && 2 / 比较 3 / 加减 4 / 乘除模移位与位 5
    assert_eq!(Operator::OrOr.prec(), 1);
    assert_eq!(Operator::AndAnd.prec(), 2);
    assert_eq!(Operator::Eql.prec(), 3);
    assert_eq!(Operator::Add.prec(), 4);
    assert_eq!(Operator::Shr.prec(), 5);
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `scripts/in-container.sh cargo test -p rgoc-lex --test test_token`
Expected: 编译失败 —— 类型不存在

- [ ] **Step 3: 实现**

按 `M1-design.md` §3.2 / §3.3 的清单定义。
`Keyword` 的 `lookup` 用 `match` 而不是哈希表 —— 25 个分支的 `match` 编译成跳转表，
比"完美哈希 + 验证"更简单且没有构造期 panic 的可能（官方那套 `keywordMap` 需要 `init()` 校验，
Rust 的 `match` 把这件事交给编译器）。

> **执行期裁决 R3**：本方法原名 `from_str`，因 `clippy::should_implement_trait`
> 报「method `from_str` can be confused for `std::str::FromStr::from_str`」而改名 `lookup`。
> **改的是名字不是语义** —— 返回 `Option` 是刻意的：「不是关键字」不是错误，
> 是绝大多数标识符的正常归宿；做成 `Result` 会把正常路径写成错误路径。
> 因此**不加 `#[allow]`**，改叫 `lookup`（"查关键字表"）比 `from_str` 更贴近它做的事。
> 同时新增 `Operator::ALL` / `Keyword::ALL` / `Delim::ALL` / `LitKind::ALL` / `SemiOrigin::ALL` /
> `OpForm::ALL` 与 `LitKind::as_official_code`（钉住 F6 的官方数值编码）。

- [ ] **Step 4: 跑测试确认通过**

Run: `scripts/in-container.sh cargo test -p rgoc-lex --test test_token`
Expected: 17 passed（原计划写 4 —— 实际写了 17 条，见下）

- [ ] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-lex
git commit -m "feat(lex): TokenKind 及配套枚举（25 关键字 / 48 项运算符 / OpForm）"
```

---

### Task T-L04: vendor `tools/token-oracle/` 与 adapter

**Files:**
- Create: `tools/token-oracle/README.md`
- Create: `tools/token-oracle/gosyntax/{scanner.go,source.go,tokens.go,token_string.go,operator_string.go}`
- Create: `tools/token-oracle/{main.go,diagmap.go}` + `gosyntax/export.go`
- Create: `tools/token-oracle/go.mod`

> **执行期裁决 R4（文件清单修正）**：实际落地是 **`main.go` + `diags.go` + `gosyntax/export.go`**，
> 不是单个 `adapter.go`，另加 `fixtures/{hello.txt,bad_utf8.txt}`。理由有两条，都不可绕：
> ① 官方 scanner 的**全部标识符都是未导出的**（`scanner` / `next` / `tok` / `lit` / `kind` …），
> 外部包在语言层面访问不到；而"官方文件零改动"是硬纪律 ⇒ 必须在**同一个 package** 里加一层
> 纯转发 shim（`gosyntax/export.go`）。`M1-design.md` §9.1 的「`adapter.go` 是唯一手写部分」
> 因此不成立，已同步修正。
> ② 只跑 `hello.txt` 的话 `== diags ==` 那一节**永远没被任何验收碰过** ——
> 映射表、排序键、`\xNN` 转义在负例上的行为全是"看起来写对了"。
> 故增加 `fixtures/bad_utf8.txt`（含裸 `0xFF`）作为负向样本。

**Interfaces:**
- Consumes: 无（Go 侧独立）
- Produces: 一个可运行命令，把 `<file>` 扫成 `M1-tests.md` §5.2 格式的两条流

- [ ] **Step 1: 写「测试」—— 这个任务的验收是一条可复现命令**

先写 README 的验收节，声明判据：

```text
验收：对 tools/token-oracle/fixtures/hello.txt（内容见 README）运行
      scripts/in-container.sh go run ./tools/token-oracle <file>
      必须输出 == tokens == 与 == diags == 两节，且 tokens 节逐行等于 README 里的黄金文本。
```

- [ ] **Step 2: 跑一遍确认失败**

Run: `scripts/in-container.sh go run ./tools/token-oracle tools/token-oracle/fixtures/hello.txt`
Expected: 失败 —— 目录不存在

> **执行期修正**：命令形态与 G11 同理，实际可运行的是
> `RGOC_TTY=0 scripts/in-container.sh bash -lc 'cd /work/tools/token-oracle && go run . fixtures/hello.txt'`
> （本目录自带 `go.mod`，仓库根没有 ⇒ 必须 `cd`；`RGOC_TTY=0` 见 **G12**）。
> 实测失败输出：`no Go files in /work/tools/token-oracle`。

- [ ] **Step 3: 实现**

1. 从 `go_source_code/src/cmd/compile/internal/syntax/` 复制 **5 个文件**到 `gosyntax/`，
   **不动内容**（保留 BSD-3-Clause 许可头）。
2. `main.go`（经 `gosyntax/export.go` 的同包 shim 调 `next()`）：按 `M1-tests.md` §5.2 的格式输出，
   并实现**四条规范化纪律**：
   只录有效域内的 `lit`、两条流各自排序、诊断只输出 `行:列 + DiagCode`（需要一张
   「官方消息 → DiagCode」映射，照 `M1-design.md` §7.2 的表）、输出确定性。
3. `README.md` 记：来源路径、go1.27.1、五文件的 sha256、**再生成步骤**、以及
   **「样本文件不能叫 `.go` 且与 adapter 同目录」**这条坑。

> **⚠️ 执行期发现：§7.2 的诊断代号表不全**（见 `M1-design.md` D-M1-11 / §12 的 R8）。原表 19 条是**凭印象手写**的，
> 而把 `scanner.go` / `source.go` 里**全部** `errorf`/`errorAtf`/`error` 调用点枚举出来
> 得到 **26 处错误点 / 25 条消息模板**，比原表多 4 条：
> `LEX-BAD-DIGIT`、`LEX-BAD-RADIX-POINT`、`LEX-STRING-NEWLINE`、`LEX-ESCAPE-BAD-DIGIT`。
> **`DiagCode` 由 19 扩到 23**，两侧（Go 映射表 + Rust `diag.rs`）必须同步。
> 这与 `AGENTS.md` 记的「写死的键列表会变成盲区」是同一个形状。
>
> **另两条执行期定下的格式口径**（README §5 已冻结）：
> - `lit` 用**自实现的字节级转义**，**不用 Go `%q`、不用 Rust `{:?}`** ——
>   两者对控制字符的写法不同，且 Rust `String` 根本装不了非法 UTF-8（F9）。
>   推论：`rgoc-lex` 的词元载荷必须是 `Vec<u8>`（见 R6）。
> - 运算符行在 `lit` 之后追加 ` op=<Op> prec=<N>`（官方对 `_Operator`/`_Star`/`_AssignOp`/`_IncOp`
>   四类**确实设置**这两个字段）；`=`/`:=`/`<-` 官方不设 `op`，故不追加。

- [ ] **Step 4: 跑一遍确认通过**

Run: 同 Step 2 的命令
Expected: 两节输出；tokens 节逐行等于 README 的黄金文本


- [ ] **Step 5: 提交**

```bash
git add tools/token-oracle
git commit -m "feat(tools): vendor 官方 scanner 五文件 + token oracle adapter"
```

---

### Task T-L05: 生成 `unicode_tables.rs`

**Files:**
- Create: `tools/token-oracle/gen_unicode/main.go`
- Create: `rgoc/crates/rgoc-lex/src/unicode_tables.rs`（生成物）
- Create: `rgoc/crates/rgoc-lex/tests/test_unicode_tables.rs`
- Modify: `rgoc/crates/rgoc-lex/src/lib.rs`

**Interfaces:**
- Consumes: `T-L04` 的容器内 Go 环境
- Produces: `rgoc_lex::unicode_tables::{is_letter(char) -> bool, is_digit(char) -> bool}`

- [ ] **Step 1: 写失败测试**

`tests/test_unicode_tables.rs`：

```rust
use rgoc_lex::unicode_tables::{is_digit, is_letter};

#[test]
fn 字母表覆盖_官方样本里的非_ascii_标识符() {
    // M0-tests F15 的 sampleTokens 明确含这几个
    for c in ['ŝ', '日', '本', '語', '中'] { assert!(is_letter(c), "{c}"); }
    for c in ['۰', '۱', '۸'] { assert!(is_digit(c), "Arabic-Indic {c}"); }
    for c in ['६', '४'] { assert!(is_digit(c), "Devanagari {c}"); }
}

#[test]
fn 字母表_拒绝_Rust_std_会接受的那批() {
    // ★ 这条是 D-M1-6 的守卫：Rust 的 is_alphabetic 会放行 1749 个 Mn 码点
    assert!(!is_letter('\u{0345}'));   // COMBINING GREEK YPOGEGRAMMENI（Mn）
    assert!('\u{0345}'.is_alphabetic()); // 反证：Rust std 认为它是字母
}

#[test]
fn 数字表_拒绝_Rust_std_会接受的那批() {
    // ★ Rust 的 is_numeric 是 N*，多出 1154 个 No 码点
    for c in ['²', '³', '¹', '¼', '½', '¾'] {
        assert!(!is_digit(c), "{c} 不是 Nd");
        assert!(c.is_numeric());  // 反证：Rust std 认为它是数字
    }
}

#[test]
fn 两张表都含_ascii_基本段() {
    for c in 'a'..='z' { assert!(is_letter(c)); }
    for c in 'A'..='Z' { assert!(is_letter(c)); }
    assert!(is_letter('_'));
    for c in '0'..='9' { assert!(is_digit(c)); }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `scripts/in-container.sh cargo test -p rgoc-lex --test test_unicode_tables`
Expected: 编译失败 —— `unicode_tables` 模块不存在

- [ ] **Step 3: 实现**

`gen_unicode/main.go`：遍历 `0x0..=0x10FFFF`（跳过代理区 `0xD800..=0xDFFF`），
分别用 `unicode.IsLetter` / `unicode.IsDigit` 归并连续区间，输出 `&[(u32, u32)]`。
生成 `unicode_tables.rs`，文件头注明：**来源（go1.27.1 的 `unicode` 包）、Unicode 版本 17.0.0、生成命令、勿手改**。
查找用 `binary_search`。

> **执行期的四点补充**（都记在 `M1-design.md` §6）：
> 1. **整文件生成物**：连 `is_letter` / `is_digit` / `lookup` 一起生成 —— 单一事实来源，
>    且实测对 `rustfmt` **稳定**（生成后 `cargo fmt --check` 通过，再生成哈希不变）。
>    Go 的原始字符串字面量里不能出现反引号 ⇒ 模板用 `¶` 占位、输出前替换成 `` ` ``。
> 2. **`_`（U+005F）不进表**，由 `is_letter` 用 `c == '_' ||` 补上。
>    这样表**严格等于** `unicode.IsLetter` 的区间（684 条，一条不多一条不少），
>    "重跑生成器应逐字节相同"这条验收才有意义。
> 3. **`binary_search_by` 的比较器方向**：闭包接收的是**表里的元素**，返回的是
>    「元素相对**目标**的大小」（区间整体落在目标之下 ⇒ `Less`）。写反了是**静默**的错。
> 4. **代码围栏**：文件头里的 ```text 需要三个哨兵（`¶¶¶`），只写一个会变成单反引号。

- [ ] **Step 4: 跑测试确认通过**

Run: `scripts/in-container.sh cargo test -p rgoc-lex --test test_unicode_tables`
Expected: 11 passed（原计划写 4 条 —— 实写 11 条，见下）

> **为什么是 11 条而不是 4 条**：原计划只覆盖"哪些码点算字母/数字"。
> 但这条链上有**三类**会静默出错的东西，各需要自己的断言：
> ① **表的内容**（覆盖 + 拒真 Rust std 的那批，含**反证** `assert!(c.is_alphabetic())`
> —— 否则那两条断言可能毫无信息量）
> ② **表的不变式**（升序、互不重叠 —— 这是二分查找正确性的**前提**；
> 乱了之后不 panic、只是**偶尔答错**）
> ③ **表与 oracle 的等价性**（区间数 684/72 **与**码点数 145,672/770 两道计数 ——
> 只钉区间数抓不到"把 `hi` 写成 `lo`"；`_` 不在表里）
>
> **另加第三层验收**：重跑生成器必须**逐字节相同**（sha256 `4b977ea0…2934d`）。
> 三层各自的 7 道变异反向验证记录在 `M1-design.md` §6.3。

- [ ] **Step 5: 提交**

```bash
git add tools/token-oracle/gen_unicode rgoc/crates/rgoc-lex
git commit -m "feat(lex): 由 oracle 生成 Unicode 分类表（字母 684 / 数字 72 区间）"
```

---

## 阶段 B：字符层

### Task T-L06: `SourceReader` —— 解码与位置推进

**Files:**
- Create: `rgoc/crates/rgoc-lex/src/source.rs`
- Create: `rgoc/crates/rgoc-lex/tests/test_source.rs`
- Modify: `rgoc/crates/rgoc-lex/src/lib.rs`

**Interfaces:**
- Consumes: `T-L01` `Pos`/`FileTable`；`T-L02` `DiagBag`/`DiagCode`
- Produces:
  - `rgoc_lex::SourceReader::new(src: Vec<u8>, file: FileId) -> Self`
    ← **预检裁决 P2**：**不收 `FileTable`**（`SourceReader` 造 `Pos` 只需要 `FileId`；
    把表交给它会让 `Lexer` 拿不到所有权）。表归 `Lexer`（T-L15）持有。
  - `ch(&self) -> Option<char>`（**尚未归属**的字符）
  - `chw(&self) -> u8`（`ch` 的字节宽度）
  - `pos(&self) -> Pos`（**当前 `ch` 的位置**，1 起）
  - `advance(&mut self) -> Option<Diag>`
    ← **预检裁决 P1**：**`SourceReader` 不持有诊断集合**。字符层若在一次推进里
    发现错误（NUL / 非法 UTF-8），返回一条 `Diag` 由 `Lexer` 收进自己的 `DiagBag`；
    两个 bag 需要合并而排序键相同、来源不同 ⇒ 无谓复杂度。
    （代价：将来若一次推进可能产出**多条**诊断，需改签名为 `Vec<Diag>` —— 目前不会。）
  - `segment(&self) -> &[u8]`（**字节**，见 D-M1-12）
  - `start_segment(&mut self)` / `stop_segment(&mut self)`
  - `rewind(&mut self)`（**只服务 `..`**，活动段内不得含换行）

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn 纯_ascii_逐字符的位置() {
    // "ab\nc" → a@1:1 b@1:2 换行@1:3 c@2:1
}

#[test]
fn 换行让行号加一且列归零() { }

#[test]
fn 中文标识符让列按字节前进() {
    // "变量 := 1"：变量@1:1，':=' 的起点 @1:7（变量占 6 字节）
    // ★ 这条坐实 G4「col 是字节列」
}

#[test]
fn segment_不含当前字符() {
    // start_segment 后 advance 两次，segment() 只含前两个字符
}

#[test]
fn rewind_回到段起点() { }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `scripts/in-container.sh cargo test -p rgoc-lex --test test_source`
Expected: 编译失败 —— `SourceReader` 不存在

- [ ] **Step 3: 实现**

按 `M1-design.md` §5.1 的状态定义。**三处相对官方的简化**（D-M1-7）：
① 一次性读入，不要 `io::Read` 流式填充；② 不要 sentinel 哨兵字节；③ 保留 `b`/`r` 与 `rewind()`。
位置推进：`advance()` 里 `col += chw as u32`；若**上一个** `ch == '\n'` 则 `line += 1; col = 0`。

- [ ] **Step 4: 跑测试确认通过**

Run: `scripts/in-container.sh cargo test -p rgoc-lex --test test_source`
Expected: 5 passed

- [ ] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-lex
git commit -m "feat(lex): SourceReader 字符层（UTF-8 解码 + 位置推进 + 段切片）"
```

---

### Task T-L07: 字符层的边界输入（LEX-01/02）

**Files:**
- Modify: `rgoc/crates/rgoc-lex/src/source.rs`
- Modify: `rgoc/crates/rgoc-lex/tests/test_source.rs`

**Interfaces:**
- Consumes: `T-L06`
- Produces: 无新接口；`SourceReader` 在边界输入上产出诊断并继续

- [ ] **Step 1: 写失败测试（对应 `M1-tests.md` 的 `T-L-13`…`T-L-19`）**

```rust
#[test]
fn bom_在文件首被吃掉且计入字节列() {
    // "\u{feff}x" → x@1:4，无诊断
}

#[test]
fn bom_在文件中报错但仍并入标识符() {
    // "x\u{feff}y" → 一条 LEX-BOM-MIDFILE，且 segment 含 BOM
}

#[test]
fn nul_报错但仍并入() {
    // "x\0y" → LEX-INVALID-NUL，segment 含 NUL
}

#[test]
fn 非法_utf8_报错但仍并入() {
    // b"1\xff2" → LEX-INVALID-UTF8 @正确的列，segment 含该字节
}

#[test]
fn crlf_的_r_当空白() { }

#[test]
fn 私有区字符在标识符里报错() {
    // "x\u{e000}" → LEX-BAD-IDENT-CHAR
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `scripts/in-container.sh cargo test -p rgoc-lex --test test_source`
Expected: FAIL —— 边界分支未实现

- [ ] **Step 3: 实现**

BOM：仅允许 `line == 0 && col == 0`，否则报 `BomMidFile` 并**继续读**（但段内保留那三个字节）。
NUL：报 `InvalidNul` 后继续。
非法 UTF-8：`utf8::decode` 失败 ⇒ 报 `InvalidUtf8`，**前进 1 个字节**（不是整个序列），继续。
CRLF：`\r` 当作空白；`\n` 才推进行号。

- [ ] **Step 4: 跑测试确认通过**

Run: `scripts/in-container.sh cargo test -p rgoc-lex --test test_source`
Expected: 11 passed

- [ ] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-lex
git commit -m "feat(lex): 字符层边界输入（BOM/NUL/非法 UTF-8/CRLF/私有区）"
```

---

## 阶段 C：词元层

### Task T-L08: `ident` —— 标识符与关键字

**Files:**
- Create: `rgoc/crates/rgoc-lex/src/lexer.rs`（本任务先只放 `ident` 与最小的 `Lexer` 壳）
- Create: `rgoc/crates/rgoc-lex/tests/test_ident.rs`
- Modify: `rgoc/crates/rgoc-lex/src/lib.rs`

**Interfaces:**
- Consumes: `T-L05` `unicode_tables`；`T-L06` `SourceReader`；`T-L03` `Keyword`
- Produces: `Lexer::{new, next_ident_kind}`（内部）

- [ ] **Step 1: 写失败测试（`T-L-20`…`T-L-25`）**

```rust
#[test]
fn 标识符的续接谓词_能续接的是一类() {
    for s in ["str1", "str1x", "str_", "_x", "日本語", "ab中cd", "a۰۱۸", "foo६४", "bar９８７６", "ŝ"] {
        assert_eq!(kind_of(s), Ident(s.as_bytes().to_vec()), "{s}");  // 载荷是字节（D-M1-12）
    }
}

#[test]
fn 标识符的续接谓词_不能续接的又是一类() {
    // "str x" → 两个；"str+x" → 两个；"a-b" → 两个
}

#[test]
fn 关键字在切完之后查_for_vs_forx() {
    assert_eq!(kind_of("for"), Keyword(Keyword::For));
    assert_eq!(kind_of("forx"), Ident(b"forx".to_vec()));
}

#[test]
fn 数字开头的_1str_切成_int_加_ident_且无诊断() { }

#[test]
fn 下划线属于字母类() { }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `scripts/in-container.sh cargo test -p rgoc-lex --test test_ident`
Expected: 编译失败 —— `Lexer` 不存在

- [ ] **Step 3: 实现**

两段循环（照官方）：先 ASCII 快路径（`is_letter || is_digit`），再用 `unicode_tables` 走一般路径。
**切完再查关键字**（`Keyword::lookup`）。**谓词必须用 `unicode_tables`，不得用 Rust std**（G7/D-M1-6）。

> **⚠️ 载荷是 `Vec<u8>`（D-M1-12）** —— 切出来的原文**直接是字节**，不要先转 `String`。
> 取 `&str` 一律显式 `str::from_utf8`，**不得** `from_utf8_lossy`：
> 实测 `x\xff y` 的标识符原文里就有非法字节，`from_utf8_lossy` 会把它换成 U+FFFD ⇒ 静默丢信息。
> 查关键字天然安全：关键字全是 ASCII，非法 UTF-8 转换必然失败 ⇒ 直接 `None`。

- [ ] **Step 4: 跑测试确认通过**

Run: `scripts/in-container.sh cargo test -p rgoc-lex --test test_ident`
Expected: 5 passed

- [ ] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-lex
git commit -m "feat(lex): 标识符扫描与关键字判定（切完再查）"
```

---

### Task T-L09: 运算符最长匹配与 `..` 回退

**Files:**
- Modify: `rgoc/crates/rgoc-lex/src/lexer.rs`
- Create: `rgoc/crates/rgoc-lex/tests/test_punct.rs`

**Interfaces:**
- Consumes: `T-L03` `OpForm`/`Operator`/`Delim`；`T-L06` `rewind()`
- Produces: `Lexer::next_punct_kind`（内部）

- [ ] **Step 1: 写失败测试（`T-L-26`…`T-L-29`，含 **RF1**）**

```rust
#[test]
fn 四十八项运算符与标点_逐条是单个词元() {
    // 表驱动：每行 (spelling, 期望 kind)
}

#[test]
fn 加号三种写法给出三个不同的_kind() {
    // D-M1-10 的端到端版：+ / += / ++
}

#[test]
fn 点号的三种长度() { /* "." ".." "..." */ }

#[test]
fn 两点回退_不吞掉点号() {
    // "1..2" → Int(1) Dot Dot Int(2)；不是 DotDotDot
}

#[test]
fn 回退之后紧跟多字节字符_位置不漂移() {
    // ★ RF1：b"1..中"
    // 断言 "中" 的 pos.col 与 oracle 相同（回退只影响本段，不得让 col 偏移）
}

#[test]
fn 关键字与标点紧邻也能分界() { /* "for(" "if{" "return(" */ }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `scripts/in-container.sh cargo test -p rgoc-lex --test test_punct`
Expected: FAIL

- [ ] **Step 3: 实现**

用 `match` 对首字符分派 + 显式前瞻，**`+=` 一族收敛到同一段逻辑**（`assignop` 汇合点）。
`..` 是**唯一**需要 `rewind()` 的地方：先吃掉第一个 `.`，若下一个是 `.` 则再看第三个；
不是第三个就 `rewind()` 回退到段起点。
`*` 不需要单独照搬官方的 `_Star` —— `(Plain, Mul)` 已经唯一（D-M1-10）。

- [ ] **Step 4: 跑测试确认通过**

Run: `scripts/in-container.sh cargo test -p rgoc-lex --test test_punct`
Expected: 6 passed

- [ ] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-lex
git commit -m "feat(lex): 运算符最长匹配与 .. 回退（唯一回退点）"
```

---

### Task T-L10: `number` —— 数字字面量全家族

**Files:**
- Modify: `rgoc/crates/rgoc-lex/src/lexer.rs`
- Create: `rgoc/crates/rgoc-lex/tests/test_number.rs`

**Interfaces:**
- Consumes: `T-L03` `LitKind`
- Produces: `Lexer::next_number_kind`（内部）

- [ ] **Step 1: 写失败测试（`T-L-33`…`T-L-35`、`T-L-38`，含 **RF3**）**

```rust
#[test]
fn 四种进制与裸零() { /* 0b101 0o17 0x1f 0123 0 */ }

#[test]
fn 下划线分隔符的合法位置() { /* 1_000 0x_1f 非法的是 1__2 / _1 / 1_ */ }

#[test]
fn 十进制与十六进制浮点() { /* 1e9 1.5E-3 0x1p-2 0x.8p1 */ }

#[test]
fn 十六进制浮点缺_p_指数要报错() { /* 0x1.8 → LEX-BAD-EXPONENT */ }

#[test]
fn 虚数后缀() { /* 3i 1.5i 0x1p-2i */ }

#[test]
fn 下划线出现在指数部分() {
    // ★ RF3：1e1_0 / 0x1p1_0 / 1_0e1_0
    // 期望值必须来自 oracle（合法或 LEX-BAD-SEP），不得自行发明规则
}

#[test]
fn 畸形字面量仍产出词元且_bad_为真() { /* 0x / 1__2 / 0b__1000 */ }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `scripts/in-container.sh cargo test -p rgoc-lex --test test_number`
Expected: FAIL

- [ ] **Step 3: 实现**

照官方结构：前缀定 `base`；`digits(base)` 返回 **`digsep` 位集**（bit0=有数字、bit1=有 `_`）；
指数分支校验进制匹配（`e` 要十进制尾数、`p` 要十六进制尾数）；后缀 `i` ⇒ `Imag`；
**最后**用 `invalidSep()` **事后**定位 `_` 的非法位置 —— 扫描时不维护错误状态。

- [ ] **Step 4: 跑测试确认通过**

Run: `scripts/in-container.sh cargo test -p rgoc-lex --test test_number`
Expected: 7 passed

- [ ] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-lex
git commit -m "feat(lex): 数字字面量（4 进制 + 分隔符 + 指数 + 虚数后缀）"
```

---

### Task T-L11: `escape` 与 `rune`

**Files:**
- Modify: `rgoc/crates/rgoc-lex/src/lexer.rs`
- Create: `rgoc/crates/rgoc-lex/tests/test_rune.rs`

**Interfaces:**
- Consumes: `T-L03` `LitKind`
- Produces: `Lexer::{next_escape_ok, next_rune_kind}`（内部）

- [ ] **Step 1: 写失败测试（`T-L-36`、`T-L-53`）**

```rust
#[test]
fn 简单转义七个加两种引号() { /* \a \b \f \n \r \t \v \\ \' \" */ }

#[test]
fn 数值转义的四种长度() { /* \101 \x41 \u00e9 \U0001F600 */ }

#[test]
fn 八进制转义超过_255_报错() { /* \400 → LEX-ESCAPE-OCTAL-OVER */ }

#[test]
fn 代理区的_u_转义报错() { /* \ud800 → LEX-ESCAPE-RANGE */ }

#[test]
fn 未知转义报错() { /* \q → LEX-ESCAPE-UNKNOWN */ }

#[test]
fn rune_字面量的四种错误() { /* '' 多字符 含换行 未终止 */ }

#[test]
fn rune_的_bad_标记() { /* 'ab' → lit_kind=Rune bad=true，仍产出词元 */ }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `scripts/in-container.sh cargo test -p rgoc-lex --test test_rune`
Expected: FAIL

- [ ] **Step 3: 实现**

`escape(quote)` 用 `(n, base, max)` 表驱动：简单转义 / `\ooo`=(3,8,255) / `\x`=(2,16,255) /
`\u`=(4,16,MaxRune) / `\U`=(8,16,MaxRune)；最后统一查代理区与 `x > max`。
`rune` 复用 `escape`，并统计字符数（`n != 1` 报 `RuneMulti`）。

- [ ] **Step 4: 跑测试确认通过**

Run: `scripts/in-container.sh cargo test -p rgoc-lex --test test_rune`
Expected: 7 passed

- [ ] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-lex
git commit -m "feat(lex): rune 字面量与转义表（含代理区与八进制超限）"
```

---

### Task T-L12: `std_string` 与 `raw_string`

**Files:**
- Modify: `rgoc/crates/rgoc-lex/src/lexer.rs`
- Create: `rgoc/crates/rgoc-lex/tests/test_string.rs`

**Interfaces:**
- Consumes: `T-L11` 的 `escape`
- Produces: `Lexer::{next_std_string_kind, next_raw_string_kind}`（内部）

- [ ] **Step 1: 写失败测试（`T-L-37`、`T-L-53`）**

```rust
#[test]
fn 解释型字符串支持全部转义且不可跨行() { }

#[test]
fn 解释型遇换行报错() { }

#[test]
fn 原始字符串不转义() { /* `a\tb` 的载荷含反斜杠与 t 两个字面字符 */ }

#[test]
fn 原始字符串可跨行且丢弃_cr() { }

#[test]
fn 未终止的字符串报错但仍产出词元() {
    // "\"unterminated → LEX-STRING-UNTERMINATED，bad=true，仍产出 Literal
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `scripts/in-container.sh cargo test -p rgoc-lex --test test_string`
Expected: FAIL

- [ ] **Step 3: 实现**

解释型：遇 `"` 收尾；遇 `\` 走 `escape`；遇 `\n` 或 EOF 报未终止。
原始型：遇 `` ` `` 收尾；**可跨行**；**`\r` 被丢弃**（不进入载荷）。

- [ ] **Step 4: 跑测试确认通过**

Run: `scripts/in-container.sh cargo test -p rgoc-lex --test test_string`
Expected: 5 passed

- [ ] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-lex
git commit -m "feat(lex): 解释型与原始型字符串字面量"
```

---

### Task T-L13: 注释处理与两种 mode 的上报通道

**Files:**
- Modify: `rgoc/crates/rgoc-lex/src/lexer.rs`
- Create: `rgoc/crates/rgoc-lex/src/message.rs`
- Create: `rgoc/crates/rgoc-lex/tests/test_comment.rs`

**Interfaces:**
- Consumes: `T-L06` `SourceReader`
- Produces:
  - `rgoc_lex::Mode`（`Comments` / `Directives` 两个标志位）
  - `rgoc_lex::Message { pub pos: Pos, pub kind: MessageKind, pub text: String }`，`MessageKind = Comment | Directive`
  - `Lexer::{take_messages(&mut self) -> Vec<Message>, take_diags(&mut self) -> Vec<Diag>}`

- [ ] **Step 1: 写失败测试（`T-L-45`、`T-L-46`、`T-L-52`、`T-L-70`，含 **RF2**）**

```rust
#[test]
fn 默认模式不上报任何注释() { }

#[test]
fn comments_模式上报全部注释() { }

#[test]
fn directives_模式只上报_go_与_line() {
    // "// hi" 不上报；"//go:build linux" 上报；"//line a.go:1" 上报
}

#[test]
fn 行注释不吃换行() {
    // "x // c\ny" 仍产出 Semi(Newline)
}

#[test]
fn 跨行块注释在_nlsemi_为真时等价于换行() { }

#[test]
fn 跨行块注释在_nlsemi_为假时不产出分号() {
    // ★ RF2：如 "f(/* a\nb */x)" —— ( 之后 nlsemi 为假，不得补分号
}

#[test]
fn 行首之外的_line_不是指令() { }

#[test]
fn 未终止的块注释报错但不中止() { }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `scripts/in-container.sh cargo test -p rgoc-lex --test test_comment`
Expected: FAIL

- [ ] **Step 3: 实现**

行注释：`//` 到行尾，**不消费 `\n`**（`\n` 要留给分号逻辑）。
块注释：跨行时**只有在 `nlsemi` 为真**才等价于换行。
`directives` 模式只认后随 `g`/`l` 的注释，并校验 `//line` 必须在行首（`col == 1`）。
**M1 只交出文本，不解析内容**（D-M1-1）。

- [ ] **Step 4: 跑测试确认通过**

Run: `scripts/in-container.sh cargo test -p rgoc-lex --test test_comment`
Expected: 8 passed

- [ ] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-lex
git commit -m "feat(lex): 注释处理与 comments/directives 两种 mode"
```

---

## 阶段 D：状态机与组装

### Task T-L14: `SemicolonPolicy`

**Files:**
- Create: `rgoc/crates/rgoc-lex/src/semicolon.rs`
- Create: `rgoc/crates/rgoc-lex/tests/test_semicolon.rs`
- Modify: `rgoc/crates/rgoc-lex/src/lib.rs`

**Interfaces:**
- Consumes: `T-L03` `TokenKind`/`Keyword`/`OpForm`
- Produces: `rgoc_lex::SemicolonPolicy::{take(&mut self) -> bool, set_after(&mut self, &TokenKind)}`

- [ ] **Step 1: 写失败测试（`T-L-49`）**

```rust
#[test]
fn 规则表_置位的类别() {
    // Ident / Literal / Keyword(break|continue|fallthrough|return) / Op{IncDec} / Delim( ) ] }
}

#[test]
fn 规则表_不置位的类别() {
    // 其余 21 个关键字 / Op 其余 / Delim 其余 / Semi / Eof
}

#[test]
fn take_取走并复位() { /* true 之后第二次 take 得 false */ }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `scripts/in-container.sh cargo test -p rgoc-lex --test test_semicolon`
Expected: 编译失败

- [ ] **Step 3: 实现**

按 `M1-design.md` §5.4 的规则表实现 `set_after`。**逐类别穷举**（不要用"默认 false"兜住 ——
那样新增一个 `TokenKind` 变体时不会有编译错误提示）。

- [ ] **Step 4: 跑测试确认通过**

Run: `scripts/in-container.sh cargo test -p rgoc-lex --test test_semicolon`
Expected: 3 passed

- [ ] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-lex
git commit -m "feat(lex): SemicolonPolicy 状态机（LEX-07 独立可测）"
```

---

### Task T-L15: `Lexer::next` 分派骨架（含 **RF4/RF5**）

**Files:**
- Modify: `rgoc/crates/rgoc-lex/src/lexer.rs`
- Create: `rgoc/crates/rgoc-lex/tests/test_next.rs`

**Interfaces:**
- Consumes: `T-L08`…`T-L14` 全部
- Produces: `rgoc_lex::Lexer::{new(src, file, table, mode), next(&mut self) -> Token}`

- [ ] **Step 1: 写失败测试（`T-L-10`、`T-L-40`…`T-L-48`、**RF4**、**RF5**）**

```rust
#[test]
fn hello_程序的完整词元流() {
    // M1-tests §5.2 的 golden 逐行比对
}

#[test]
fn 分号的三个来源() { }

#[test]
fn 右括号后补分号左括号后不补() { }

#[test]
fn eof_补分号的位置与 oracle 相同() {
    // ★ RF5：末尾经 EndGuard 校验，断言 Semi(Eof) 的 pos 与 oracle 相等
}

#[test]
fn 词元中间遇到_eof_的收尾顺序() {
    // ★ RF4："func f() { \"abc" 之类
    // 断言顺序：先 STRING 未终止诊断 + Literal 词元，【再】Semi(Eof)
}

#[test]
fn 语料_semi1_至_semi7_逐词元与_oracle 相等() { }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `scripts/in-container.sh cargo test -p rgoc-lex --test test_next`
Expected: FAIL

- [ ] **Step 3: 实现**

照 `M1-design.md` §5.2 的骨架：`take()` → `loop`（skip 空白 → 记起点 → EOF 判定 → 首字符分派）
→ `set_after()`。**`'\n'` 只在 `nlsemi` 为假时被跳过**（LEX-07 第一处机关）。

- [ ] **Step 4: 跑测试确认通过**

Run: `scripts/in-container.sh cargo test -p rgoc-lex --test test_next`
Expected: 6 passed

- [ ] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-lex
git commit -m "feat(lex): Lexer::next 分派骨架与分号插入（含 EOF 补分号）"
```

---

### Task T-L16: 错误恢复与诊断排序（`T-L-50`…`T-L-54`）

**Files:**
- Modify: `rgoc/crates/rgoc-lex/src/lexer.rs`
- Create: `rgoc/crates/rgoc-lex/tests/test_recovery.rs`

**Interfaces:**
- Consumes: `T-L15`
- Produces: 无新接口

- [ ] **Step 1: 写失败测试**

`M1-tests.md` §4 的 **N1–N11** 逐个，**每条都断言两件事**：

```rust
#[test]
fn n1_非法_utf8_诊断与后续词元() {
    let r = lex(b"package main\nfunc f() { x := 1\xff2 }\n");
    assert_diag(&r, 4, 8, DiagCode::InvalidUtf8);      // ① 诊断
    assert_tokens_eq_golden(&r, "n1.tokens");           // ② 后续词元与 oracle 相等
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `scripts/in-container.sh cargo test -p rgoc-lex --test test_recovery`
Expected: FAIL

- [ ] **Step 3: 实现**

补齐各处的"报错但继续"路径。**诊断入口只有一个**（`push_diag`），保证不会漏进 `DiagBag`。

- [ ] **Step 4: 跑测试确认通过**

Run: `scripts/in-container.sh cargo test -p rgoc-lex --test test_recovery`
Expected: 11 passed

- [ ] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-lex
git commit -m "feat(lex): 错误恢复的负例全覆盖（诊断 + 后续词元双向断言）"
```

---

### Task T-L17: 资源上限（`T-L-63`）

**Files:**
- Modify: `rgoc/crates/rgoc-lex/src/lexer.rs`
- Create: `rgoc/crates/rgoc-lex/tests/test_limits.rs`

**Interfaces:**
- Consumes: `T-L15`
- Produces: `rgoc_lex::MAX_FILE_BYTES: usize`（= 64 MiB）+ `Lexer::new` 的提前检查

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn 超过上限报错且只返回_eof() {
    // 构造一个 > MAX_FILE_BYTES 的输入（用 metadata 路径测，见下）
    // 断言：恰好一条 LEX-EXCEEDS-FILE-LIMIT，词元流只含 Eof
}

#[test]
fn 检查在读入之前发生() {
    // 断言 Lexer::from_path 先 fs::metadata 再 read
    // ★ 这条防的是「读完 10 GB 再检查」—— 那时已经 OOM
}

#[test]
fn 上限边界值本身可以通过() {
    // 恰等于 MAX_FILE_BYTES 不报错
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `scripts/in-container.sh cargo test -p rgoc-lex --test test_limits`
Expected: FAIL

- [ ] **Step 3: 实现**

新增 `Lexer::from_path(path, table, mode) -> Self`：**先 `fs::metadata().len()` 检查**，超限直接构造一个
"已耗尽"的实例（只产出 `Eof` + 一条诊断），**不读文件**。

- [ ] **Step 4: 跑测试确认通过**

Run: `scripts/in-container.sh cargo test -p rgoc-lex --test test_limits`
Expected: 3 passed

- [ ] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-lex
git commit -m "feat(lex): 单文件 64 MiB 上限（读入前检查，登记为实现限制）"
```

---

## 阶段 E：门禁与交付

### Task T-L18: `rgoc-driver lex` 子命令

**Files:**
- Modify: `rgoc/crates/rgoc-driver/src/cli.rs`
- Create: `rgoc/crates/rgoc-driver/src/lex_cmd.rs`
- Modify: `rgoc/crates/rgoc-driver/Cargo.toml`（加 `rgoc-lex` 依赖）
- Modify: `rgoc/crates/rgoc-driver/tests/test_driver.rs`

**Interfaces:**
- Consumes: `T-L15`/`T-L17` 的 `Lexer`；`T-L13` 的 `take_messages`
- Produces: `rgoc-driver lex <file> [--mode comments|directives]`，输出 `M1-tests.md` §5.2 的格式

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn lex_子命令输出两节() { /* stdout 含 "== tokens ==" 与 "== diags ==" */ }

#[test]
fn lex_子命令的输出是确定性的() { /* 跑两次逐字节相同 */ }

#[test]
fn lex_子命令缺少文件参数时报错退出() { /* 非 0 退出码 */ }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `scripts/in-container.sh cargo test -p rgoc-driver`
Expected: FAIL

- [ ] **Step 3: 实现**

在现有手写参数解析里加 `lex` 分支（**不引 clap**，沿用既有风格）。
输出格式**必须与 adapter 逐字节可 diff** —— 这是腿乙能工作的前提。

- [ ] **Step 4: 跑测试确认通过**

Run: `scripts/in-container.sh cargo test -p rgoc-driver`
Expected: 26 passed（23 + 3）

- [ ] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-driver
git commit -m "feat(driver): lex 子命令（输出与 oracle adapter 同格式）"
```

---

### Task T-L19: 腿乙 —— golden 全量与门禁脚本

**Files:**
- Create: `scripts/check-lex-golden.sh`
- Create: `rgoc/tests/smoke/lex/`（golden 与语料清单）

**Interfaces:**
- Consumes: `T-L18` 的 `lex` 子命令；`T-L04` 的 adapter
- Produces: 一条命令：生成/校验全部 golden

- [ ] **Step 1: 写「测试」—— 脚本本身就是判据**

```sh
# scripts/check-lex-golden.sh 的判据（写进脚本头注释）
#   --record : 用 adapter 生成 golden 并写入 rgoc/tests/smoke/lex/
#   默认     : 用 rgoc-driver lex 重算并逐行 diff
#   判据     : 所有文件 diff 为空 -> exit 0；否则打印差异并 exit 1
```

- [ ] **Step 2: 跑一遍确认失败**

Run: `scripts/check-lex-golden.sh`
Expected: 失败 —— golden 尚未生成

- [ ] **Step 3: 实现**

1. golden 清单（语料相对路径 + 自有 fixture）。
2. `--record` 走 `scripts/in-container.sh go run ./tools/token-oracle`。
3. 校验时**逐行 diff 并打印差异**（满足 `03` §M1 的「token 与位置差异可定位」）。
4. 每个 golden 文件头**必须**有 `# source: oracle | hand-written`；
   唯一的 `hand-written` 是超限用例。
5. 加一条守卫：**adapter 输出与 driver 输出的格式必须逐字节一致**（否则腿乙在比两种格式）。

- [ ] **Step 4: 跑一遍确认通过**

Run: `scripts/check-lex-golden.sh`
Expected: 全部 `PASS`，exit 0

- [ ] **Step 5: 提交**

```bash
git add scripts/check-lex-golden.sh rgoc/tests/smoke/lex
git commit -m "test(lex): 腿乙 golden 全量与门禁脚本（SM-M1-LEX-GOLDEN）"
```

---

### Task T-L20: C1 契约升级（留位 → 完整）

**Files:**
- Modify: `docs/contracts/C1-source-map.md`
- Modify: `docs/README.md`、`AGENTS.md`（按 `AGENTS.md` §5 同步）

**Interfaces:**
- Consumes: `T-L01`/`T-L02` 的实现
- Produces: 无代码接口

- [ ] **Step 1: 写「测试」—— 一致性自检**

Run: `python3 scripts/check-m0-consistency.py`
Expected: 通过（C1 的「留位」声明被改后，`[5g]` 那两条断言会失败 ⇒ 需要同步改断言或改契约表述）

- [ ] **Step 2: 确认失败**

Expected: `[5g]` 上报 C1 级别不符（这正是"断言锚定了会过期的状态"的正当复发）

- [ ] **Step 3: 实现**

1. `C1` 头部：级别由「留位」改为「**完整**」；删除"本文件不是位置跟踪证据"的留位声明
   （M1 之后它确实是位置跟踪的证据了），改为记录 `FileId`/`FileTable` 的落地位置。
2. §2 更新为 `Pos { file: FileId, line, col }` + `FileTable` 的语义。
3. §3/§7 更新 `DiagCode` 与验证方式（测试落点由 `rgoc-hir` 改为 `rgoc-lex`）。
4. 加变更记录行。
5. 同步 `scripts/check-m0-consistency.py` 的 `[5g]` 断言（**改完要做一次反向验证**）。

- [ ] **Step 4: 跑确认通过**

Run: `python3 scripts/check-m0-consistency.py`
Expected: 228 条全过、exit 0

- [ ] **Step 5: 提交**

```bash
git add docs/contracts/C1-source-map.md docs/README.md AGENTS.md scripts/check-m0-consistency.py
git commit -m "docs(contracts): C1 由留位升为完整（FileId/FileTable/DiagCode）"
```

---

### Task T-L21: 阶段收口 —— GAP-01、门禁复核、文档同步

**Files:**
- Modify: `docs/milestones/M1-manifest.json`（新建）
- Modify: `docs/03-roadmap.md`、`docs/README.md`、`AGENTS.md`
- Modify: `docs/milestones/M1-design.md`、`M1-tests.md`（状态改「已冻结 / 已完成」）

**Interfaces:**
- Consumes: 全部
- Produces: `M1-manifest.json`

- [ ] **Step 1: 判定清单（逐条跑并记录输出）**

```sh
scripts/in-container.sh cargo fmt --check
scripts/in-container.sh cargo check --workspace --all-targets
scripts/in-container.sh cargo clippy --workspace --all-targets -- -D warnings
scripts/in-container.sh cargo test --workspace
scripts/in-container.sh cargo test -p rgoc-lex          # 腿甲
scripts/check-lex-golden.sh                              # 腿乙
scripts/in-container.sh cargo test -p rgoc-spikes        # SM-M0-NATIVE-HELLO 的回归载体
python3 scripts/check-m0-consistency.py
```

- [ ] **Step 2: 实现**

1. `M1-manifest.json`：`gate`（LEX 13 项 / golden / 负例 / native 回归 / 四条命令）+ `test_ids` + `unsupported` + `budget`。
2. **GAP-01**：填 **LEX 13 项**的四维归属（**不填完 210 个** —— `M0-report.md` §7.2 明确警告）。
3. **解除 `U12` 的 lex 一半**：在 `M0-manifest.json` 与 `03` 里如实改写（`parse` 仍留 M2）。
4. 同步 `AGENTS.md` §0/§1/§2.1/§3/§7 与 `docs/README.md`。
5. **裁决 R1**（LEX-06 的 47/48）并同步 `01` / `02`。

- [ ] **Step 3: 反向验证断言**

对每条新增断言，**故意制造它能抓的 bug**，确认变红。特别是 `M1-manifest.json` 的 gate 断言 ——
M0 的教训是「写死的键列表会变成盲区」，所以 **gate 的键必须从 manifest 动态枚举**。

- [ ] **Step 4: 全绿确认**

Expected: 上述 8 条命令全部通过

- [ ] **Step 5: 提交**

```bash
git add docs scripts
git commit -m "chore(m1): 阶段收口 —— manifest / GAP-01 / U12 解除 / 文档同步"
```

---

## 自检记录

| 检查 | 结果 |
|---|---|
| **Spec coverage** | `M1-design.md` §1.2 的 7 条不做项都有归属；LEX-01..13 在 `M1-tests.md` §2 逐项映射到测试 ID；D-M1-1~10 每条都有任务落地 |
| **Step scan** | 每个 Step 只要求一件事；无「处理边界情况」这类空话 |
| **Type consistency** | `TokenKind`/`OpForm`/`Pos`/`DiagCode` 的签名在 T-L01/T-L02/T-L03 定义，后续任务只引用不重定义 |
| **Review Focus** | 五条 RF 各自钉在 T-L09 / T-L13 / T-L10 / T-L15 / T-L15 |
| **Proportion** | 本计划 21 个任务；代码块以签名与测试断言为主，不含完整函数体 |
