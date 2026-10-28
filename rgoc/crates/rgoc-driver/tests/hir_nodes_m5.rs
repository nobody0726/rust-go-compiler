//! HIR 节点**穷尽性**与 lowering 的**诊断一致性**（`EX-1`；`M5-tests.md` §3.1）。
//!
//! # 这条测试盖住哪两个 ID
//!
//! | ID | 判据 | 本文件怎么钉 |
//! |---|---|---|
//! | `T-EX-01` | `Expr` / `Stmt` / `Place` / `RangeKind` 的**每个变体**至少被一个 fixture 产出 | [`t_ex_01_every_hir_node_is_reachable`] |
//! | `T-EX-03` | check 接受 ⇒ lowering **不报诊断**（`D-M5-2` 的静默会话） | [`t_ex_03_lowering_is_silent_when_check_passes`] |
//!
//! # `T-EX-01` 为什么不是「列一张表然后勾掉」
//!
//! 手写一张「14 个 Stmt + 21 个 ExprKind + 5 个 Place + 5 个 RangeKind」的清单，
//! 只能证明**今天**对得上。明天有人往 `types.rs` 加一个 `Stmt::Yield`，
//! 这张表不会自己变长 —— 新变体从此没有任何 fixture，而测试仍然绿。
//!
//! 所以变体集合是**从 `types.rs` 现读的**：
//!
//! 1. 解析 `pub enum Stmt { … }` 等四段，抽出变体名；
//! 2. 断言「映射表」的键集 == 解析出来的变体集 ⇒ 加变体就必须来改表；
//! 3. 把 fixture 的 dump 打散成 token，断言表里**每个**变体名都能在 dump 里找到对应 token
//!    ⇒ 改表就必须补 fixture。
//!
//! 三步合起来才是「穷尽」：缺任何一步，表与代码就可以各自漂移而测试不响。
//!
//! # 为什么用 dump 而不是「直接遍历 HIR」
//!
//! Rust 不能在运行时枚举 enum 变体（没有反射）。`dump` 是**唯一**同时满足两件事的
//! 出口：它是稳定的外部形态（`T-EX-04` 钉了逐字节确定性），而且每个变体在 dump 里
//! 有一个**专属 token**。用它当「变体被产出过」的证据，比在库里塞一堆
//! `#[cfg(test)] const VARIANTS` 更不容易腐坏 —— 后者会被忘记更新，而 dump 不会
//! （dump 少一个变体就编不过）。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rgoc_driver::cli::RunCommand;
use rgoc_driver::run_cmd::run_run;

/// 仓库根：`rgoc/crates/rgoc-driver` 往上三层。
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("仓库根不可达（cargo 的 CARGO_MANIFEST_DIR 变了？）")
}

// ───────────────────── 变体 → dump token 的映射表 ─────────────────────

/// `Stmt` 的 14 个变体 → dump 的**第二列**（kind 列）。
///
/// `Stmt::Expr` 的 label 是 `ExprStmt`（不是 `Expr`）—— 因为第三列已经被表达式节点
/// 占用，两者混在一起就无法区分「语句」与「表达式」。
const STMT_TOKENS: &[(&str, &str)] = &[
    ("Assign", "Assign"),
    ("ShortDecl", "ShortDecl"),
    ("Expr", "ExprStmt"),
    ("If", "If"),
    ("For", "For"),
    ("Range", "Range"),
    ("Switch", "Switch"),
    ("TypeSwitch", "TypeSwitch"),
    ("Return", "Return"),
    ("Branch", "Branch"),
    ("Labeled", "Labeled"),
    ("Block", "Block"),
    ("Defer", "Defer"),
    // M8（`D-M8-10`）：这三个在 Task 7–13 才被 lowering 产出，先用 `M8_PENDING` 登记。
    ("Go", "Go"),
    ("Send", "Send"),
    ("Select", "Select"),
    ("Empty", "Empty"),
];

/// `ExprKind` 的 21 个变体 → dump 第三列的**首 token**。
///
/// 带下标的形态（`slot#3` / `field#1`）取 `#` 之前的部分。
const EXPR_TOKENS: &[(&str, &str)] = &[
    ("Const", "const"),
    ("Slot", "slot"),
    ("Global", "global"),
    ("Field", "field"),
    ("Index", "index"),
    ("Slice", "slice"),
    ("Deref", "deref"),
    ("Addr", "addr"),
    ("AddrOf", "addr-of"),
    ("Unary", "unary"),
    ("Binary", "binary"),
    ("Call", "call"),
    ("Convert", "convert"),
    ("Composite", "composite"),
    ("FuncLit", "funclit"),
    ("Bound", "bound"),
    ("TypeAssert", "assert"),
    ("Make", "make"),
    ("New", "new"),
    ("Builtin", "builtin"),
    ("Nil", "nil"),
    ("Recv", "recv"),
];

/// `Place` 的 5 个变体 → dump 第三列的首 token。
const PLACE_TOKENS: &[(&str, &str)] = &[
    ("Slot", "slot"),
    ("Global", "global"),
    ("Field", "field"),
    ("Index", "index"),
    ("Deref", "deref"),
];

/// `RangeKind` 的 5 个变体 → dump 第三列的 `kind=` 值。
const RANGE_TOKENS: &[(&str, &str)] = &[
    ("Array", "Array"),
    ("Slice", "Slice"),
    ("Str", "Str"),
    ("Map", "Map"),
    ("Int", "Int"),
    ("Chan", "Chan"),
    ("Func", "Func"),
];

// ───────────────────── fixture ─────────────────────

/// 一个程序，**同时**产出全部 45 个节点形态（14 + 21 + 5 + 5）。
///
/// 它是「可执行的规格」：每一行都为了点亮某个变体而存在，注释写的是**为什么**
/// 需要那一行。改动它之前先问「删掉这行会熄灭哪个 token」。
///
/// 注意 `defer func() {}()` 而不是 `defer add(1, 2)`：后者被
/// `CHK-INVALID-GO-DEFER` 拒（defer 不许丢弃返回值），程序就降不到 HIR 了。
const NODE_FIXTURE: &str = r#"package main

type T struct{ A int }

type I interface{ M() int }

func (t T) M() int { return t.A }

var g int = 1

func add(a, b int) int { return a + b }

func main() {
	var i I = T{2}
	s := T{3}
	p := &s
	p.A = 4
	q := &T{5}
	_ = *p
	*p = T{9}
	g = 5
	arr0 := [3]int{1, 2, 3}
	arr0[0] = 7
	_ = arr0
	_ = q
	arr := [3]int{1, 2, 3}
	sl := arr[0:2]
	_ = sl
	m := map[string]int{"a": 1}
	_ = m["a"]
	v, ok := m["a"]
	_, _ = v, ok
	x := -g
	y := x + 1
	z := add(x, y)
	_ = int64(z)
	f := func(n int) int { return n }
	_ = f
	mv := s.M
	_ = mv
	n := i.(T)
	_ = n
	nb, ok2 := i.(T)
	_, _ = nb, ok2
	np := new(int)
	_ = np
	ms := make([]int, 2)
	_ = ms
	ln := len(ms)
	_ = ln
	var nilp *int
	if nilp == nil {
	}
	for k := 0; k < 2; k++ {
	}
	for range arr {
	}
	for range sl {
	}
	for range m {
	}
	for range "ab" {
	}
	for range 3 {
	}
	switch y {
	case 1:
	}
	switch {
	default:
	}
	switch i.(type) {
	case T:
	}
L:
	for {
		break L
	}
	for {
		continue
	}
	goto end
end:
	{
	}
	defer func() {}()
	add(3, 4)
}
"#;

/// 写 fixture 到 `target/tmp`（不进仓库）并返回路径。
fn write_fixture(name: &str, src: &str) -> PathBuf {
    let dir = repo_root().join("rgoc/target/tmp/hir-nodes");
    std::fs::create_dir_all(&dir).expect("建不了 tmp 目录");
    let p = dir.join(name);
    std::fs::write(&p, src).expect("写不了 fixture");
    p
}

/// 跑 `--dump-hir`，返回 dump 文本（诊断走 stderr，另行断言）。
fn dump_of(path: &Path) -> String {
    let o = run_run(&RunCommand {
        roots: Vec::new(),
        files: vec![path.to_path_buf()],
        dump_hir: true,
        max_steps: None,
        lang: None,
        via: rgoc_driver::cli::Via::Hir,
        opt: rgoc_ssa::opt::Config::all(),
        sched: rgoc_driver::cli::Sched::default(),
    })
    .unwrap_or_else(|e| panic!("{} 内部错：{e}", path.display()));
    assert_eq!(
        o.exit,
        0,
        "{} 没降下来：{}",
        path.display(),
        o.stderr.trim()
    );
    assert!(
        o.stderr.is_empty(),
        "{} 有诊断（fixture 本身就该是干净的）：{}",
        path.display(),
        o.stderr.trim()
    );
    o.stdout
}

// ───────────────────── 解析 types.rs ─────────────────────

/// 从 `types.rs` 里读出 `pub enum <name> { … }` 的变体名集合。
///
/// 只认 rustfmt 之后的标准形态：变体行 = 4 个空格 + 大写开头的标识符 +
/// `{` / `(` / `,`。这样能跳过文档注释（`///`）、字段行（8 空格）与
/// 属性行（`#[…]`）。
fn enum_variants(src: &str, name: &str) -> BTreeSet<String> {
    let marker = format!("pub enum {name} {{");
    let at = src
        .find(&marker)
        .unwrap_or_else(|| panic!("types.rs 里找不到 `{marker}`（枚举改名了？）"));
    let rest = &src[at + marker.len()..];
    let end = rest
        .find("\n}\n")
        .unwrap_or_else(|| panic!("`{marker}` 没有在列 0 的 `}}` 收尾"));
    let body = &rest[..end];

    let mut out = BTreeSet::new();
    for line in body.lines() {
        let Some(t) = line.strip_prefix("    ") else {
            continue;
        };
        // 8 空格的是字段 / 嵌套内容，不是变体。
        if t.starts_with(' ') {
            continue;
        }
        let name_end = t
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .unwrap_or(t.len());
        let v = &t[..name_end];
        if !v.starts_with(|c: char| c.is_ascii_uppercase()) {
            continue;
        }
        let after = t[name_end..].trim_start();
        if after.starts_with('{') || after.starts_with('(') || after.starts_with(',') {
            out.insert(v.to_string());
        }
    }
    assert!(
        !out.is_empty(),
        "`{marker}` 解析出 0 个变体 —— 解析器瞎了（格式变了？）"
    );
    out
}

/// **还没被 lowering 产出**的变体（M8 执行期的临时登记）。
///
/// # 为什么要有这张表
///
/// `T-EX-01` 要求「每个变体都有 fixture 产出」。M8 的六个节点是**分两步**落地的：
/// Task 2 只加类型（让 `match` 的穷尽性先把所有消费方逼出来），Task 7–48 才让
/// lowering 真的产出它们。中间这段窗口里，「有类型但没人产出」是**预期的**。
///
/// # 为什么它不会腐化成「永久豁免」
///
/// 三字段是 `(enum 名, 变体名, dump token)`。测试末尾有一条反向断言：
/// **登记项必须还没被点亮**。某个 Task 一旦把它点亮，这条就红，逼你把登记摘掉。
/// 没有这条，登记表三周后就是一份写着「不用管这些」的墓碑。
const M8_PENDING: &[(&str, &str, &str)] = &[
    ("Stmt", "Go", "Go"),          // Task 11 点亮
    ("Stmt", "Send", "Send"),      // Task 9 点亮
    ("Stmt", "Select", "Select"),  // Task 12 点亮
    ("ExprKind", "Recv", "recv"),  // Task 10 点亮
    ("RangeKind", "Chan", "Chan"), // Task 13 点亮
    ("RangeKind", "Func", "Func"), // Task 45 点亮
];

/// 某个 token 是否登记在 [`M8_PENDING`] 里。
fn m8_pending(enum_name: &str, token: &str) -> bool {
    M8_PENDING
        .iter()
        .any(|(e, _, t)| *e == enum_name && *t == token)
}

// ───────────────────── T-EX-01 ─────────────────────

/// `T-EX-01`：HIR 的每个变体都至少被一个 fixture 产出。
///
/// 三步（缺一步这条测试就退化成摆设）：
///
/// 1. **表 == 代码**：映射表的键集必须等于 `types.rs` 里解析出的变体集；
/// 2. **fixture 点亮每个变体**：表里每个 token 都要出现在 dump 里；
/// 3. **数量写死**：51 个（17 + 22 + 5 + 7）—— 数字变化说明 `types.rs` 动了，
///    那时应当同时看到第 1 步报出「只在代码 / 只在表」的差额。
#[test]
fn t_ex_01_every_hir_node_is_reachable() {
    let types_src = std::fs::read_to_string(repo_root().join("rgoc/crates/rgoc-hir/src/types.rs"))
        .expect("读不到 rgoc-hir/src/types.rs");

    // ① 表 == 代码
    for (enum_name, table) in [
        ("Stmt", STMT_TOKENS),
        ("ExprKind", EXPR_TOKENS),
        ("Place", PLACE_TOKENS),
        ("RangeKind", RANGE_TOKENS),
    ] {
        let code = enum_variants(&types_src, enum_name);
        let tab: BTreeSet<String> = table.iter().map(|(v, _)| (*v).to_string()).collect();
        if code != tab {
            let only_code: Vec<&String> = code.difference(&tab).collect();
            let only_table: Vec<&String> = tab.difference(&code).collect();
            panic!(
                "{enum_name}：映射表与 types.rs 不同步 —— \
                 只在 types.rs {only_code:?} / 只在表 {only_table:?}。\
                 新增变体时必须同时补 (a) 这张表和 (b) 一个产出它的 fixture。"
            );
        }
    }
    assert_eq!(
        STMT_TOKENS.len() + EXPR_TOKENS.len() + PLACE_TOKENS.len() + RANGE_TOKENS.len(),
        51,
        "四张表合计应为 51 个变体（Stmt 17 + ExprKind 22 + Place 5 + RangeKind 7）"
    );

    // ② 与 ③：fixture 点亮每个 token
    let path = write_fixture("t_ex_01_nodes.go", NODE_FIXTURE);
    let dump = dump_of(&path);

    let mut stmt_seen: BTreeSet<String> = BTreeSet::new();
    let mut expr_seen: BTreeSet<String> = BTreeSet::new();
    let mut place_seen: BTreeSet<String> = BTreeSet::new();
    let mut range_seen: BTreeSet<String> = BTreeSet::new();

    for line in dump.lines() {
        if line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 3 {
            continue;
        }
        let kind = cols[1];
        let detail = cols[2];
        let head = detail
            .split_whitespace()
            .next()
            .unwrap_or("")
            .split('#')
            .next()
            .unwrap_or("");
        match kind {
            "Expr" => {
                expr_seen.insert(head.to_string());
            }
            "Place" => {
                place_seen.insert(head.to_string());
            }
            other => {
                // `Stmt::Range` 的 label 就是 `Range`，与 `RangeKind` 共用同一行 ——
                // 所以先记 label，再从 detail 里取 `kind=` 那一项，两者不冲突。
                stmt_seen.insert(other.to_string());
                if other == "Range"
                    && let Some(rest) = detail.strip_prefix("kind=")
                {
                    let v = rest.split_whitespace().next().unwrap_or("");
                    range_seen.insert(v.to_string());
                }
            }
        }
    }

    let mut missing: Vec<String> = Vec::new();
    for (variant, token) in STMT_TOKENS {
        if !stmt_seen.contains(*token) && !m8_pending("Stmt", token) {
            missing.push(format!("Stmt::{variant}（dump token `{token}`）"));
        }
    }
    for (variant, token) in EXPR_TOKENS {
        if !expr_seen.contains(*token) && !m8_pending("ExprKind", token) {
            missing.push(format!("ExprKind::{variant}（dump token `{token}`）"));
        }
    }
    for (variant, token) in PLACE_TOKENS {
        if !place_seen.contains(*token) && !m8_pending("Place", token) {
            missing.push(format!("Place::{variant}（dump token `{token}`）"));
        }
    }
    for (variant, token) in RANGE_TOKENS {
        if !range_seen.contains(*token) && !m8_pending("RangeKind", token) {
            missing.push(format!("RangeKind::{variant}（dump token `kind={token}`）"));
        }
    }
    assert!(
        missing.is_empty(),
        "有 {} 个 HIR 变体没有任何 fixture 产出它们（补 NODE_FIXTURE，别改判据）：\n{}",
        missing.len(),
        missing.join("\n")
    );

    // ③ 登记项必须**还没**被点亮 —— 点亮了就说明该摘登记了，
    //    否则 `M8_PENDING` 会悄悄变成「永久豁免」。
    let mut stale: Vec<String> = Vec::new();
    for (enum_name, variant, token) in M8_PENDING {
        let lit = match *enum_name {
            "Stmt" => stmt_seen.contains(*token),
            "ExprKind" => expr_seen.contains(*token),
            "Place" => place_seen.contains(*token),
            "RangeKind" => range_seen.contains(*token),
            other => panic!("M8_PENDING 里的枚举名不认识：{other}"),
        };
        if lit {
            stale.push(format!("{enum_name}::{variant}"));
        }
    }
    assert!(
        stale.is_empty(),
        "这些变体已经被 fixture 点亮了，请从 M8_PENDING 摘掉：{stale:?}"
    );
}

// ───────────────────── T-EX-03 ─────────────────────

/// `T-EX-03`：check 接受 ⇒ lowering **不报任何诊断**。
///
/// `D-M5-2` 把 lowering 挂到检查器**同一个会话**上，好处是 lowering 拿得到
/// `Operand`（类型只从会话出，不自己猜）。代价是：lowering 一旦需要**报诊断**，
/// 就说明「检查器认为这个程序没问题，而 HIR 认为它有问题」—— 这是**内部矛盾**，
/// 不是用户错误。
///
/// 所以判据是**否定式**的：接受面（入选语料 + 节点 fixture）上，stderr 必须是空的。
/// 反向的那一半（不接受 ⇒ 必须带 `HIR-UNSUPPORTED-*` 且不执行）在 `T-EX-02`
/// （`run_semantics.rs`），两条合起来才封住这个面。
///
/// 分母用**入选语料 69 个**而不是几个自造 fixture：接受面就是这 69 个文件定义的，
/// 拿几个自己挑的程序去证明「接受面是干净的」等于自问自答。
#[test]
fn t_ex_03_lowering_is_silent_when_check_passes() {
    let root = repo_root();
    let list = std::fs::read_to_string(root.join("tools/run-oracle/run-selected.txt"))
        .expect("读不到 run-selected.txt");

    let mut bad: Vec<String> = Vec::new();
    let mut n = 0usize;
    for line in list.lines() {
        let rel = line.trim();
        if rel.is_empty() || rel.starts_with('#') {
            continue;
        }
        n += 1;
        let o = run_run(&RunCommand {
            roots: Vec::new(),
            files: vec![root.join(rel)],
            dump_hir: true,
            max_steps: None,
            lang: None,
            via: rgoc_driver::cli::Via::Hir,
            opt: rgoc_ssa::opt::Config::all(),
            sched: rgoc_driver::cli::Sched::default(),
        })
        .unwrap_or_else(|e| panic!("{rel} 内部错：{e}"));
        if o.exit != 0 {
            bad.push(format!(
                "{rel}: 退出码 {}（check 接受了却降不下来）",
                o.exit
            ));
            continue;
        }
        if !o.stderr.is_empty() {
            bad.push(format!("{rel}: lowering 报了诊断：{}", o.stderr.trim()));
        }
        if !o.stdout.starts_with("# program") {
            bad.push(format!("{rel}: dump 没有 `# program` 头行"));
        }
    }
    assert_eq!(n, 69, "入选清单应为 69 个文件（`R-6-1` 的冻结值）");
    assert!(
        bad.is_empty(),
        "接受面上 lowering 不是静默的（{} 条）—— 这说明检查器与 HIR 对「什么合法」不一致：\n{}",
        bad.len(),
        bad.join("\n")
    );
}
