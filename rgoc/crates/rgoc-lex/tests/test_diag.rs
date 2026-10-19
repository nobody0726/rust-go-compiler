//! T-L02 的验收测试 —— 诊断与排序（C1 §3 / §4）。
//!
//! 判据来源：`M1-design.md` §7.2 的诊断代号表、`C1` §4 的排序规则。
//! 四条断言分别钉住：排序键顺序、同位置唯一性、代号前缀、**声明顺序与字典序一致**。

use rgoc_lex::{Diag, DiagBag, DiagCode, FileId, Pos};

/// 测试用的位置构造器（`FileId` 的合法来源只有 `intern`，
/// 这里用 `from_index` 是为了不引入文件表）。
fn p(file: u32, line: u32, col: u32) -> Pos {
    Pos {
        base: None, // 无 `//line` 基准
        file: FileId::from_index(file),
        line,
        col,
    }
}

fn d(file: u32, line: u32, col: u32, code: DiagCode) -> Diag {
    Diag {
        pos: p(file, line, col),
        code,
        message: format!("{} 的说明", code.as_str()),
    }
}

#[test]
fn 代号表恰为冻结总数且无重复() {
    // 48 = 23 个 M1 词法（`LEX-`）+ 12 个 M2 语法（`SYN-`）+ 13 个 M3 类型（`TYP-`）。
    //
    // `SYN-` 段的 12 个 = `T-AP06`/`T-AP15` 落的 3 个（`SynExpectedSemicolon` /
    // `SynInvalidLineDirective` / `SynUnexpectedToken`）+ `T-AP16` 从官方
    // `branches.go` 的 12 处 `errf` 枚举后收敛成的 8 个 + `T-AP20` 的
    // `SynNestingTooDeep`（H 判据，官方无此行为）。
    //
    // `TYP-` 段的 7 个 = M3/Task 7 的移位代号（`TypInvalidShiftCount` /
    // `TypInvalidShiftOperand`）+ Task 8 的常量代号（`TypInvalidConstVal` /
    // `TypInvalidConversion` / `TypTruncatedFloat`）+ Task 10 的
    // `TypNumericOverflow`，**全部刻意与官方 `go/types` 的
    // `InvalidShiftCount` / `InvalidShiftOperand` / `InvalidConstVal` /
    // `InvalidConversion` / `TruncatedFloat` / `NumericOverflow` 一一对应**
    // （不拆细，否则「按代号比对」时与 oracle 的类别对不上）。
    //
    // 曾有的是 `TypUnsupportedConstraintIdentity`（M3/Task 17）**刻意相反**：
    // 官方判得了约束接口的同一性（它有完整类型集），M3 只表示不判定 ——
    // M4/Task 10 升级 `identical` 后**已移除**（`U-M3-05` 解除）；现存的
    // TYP- 能力边界码是 `TypUnsupportedAssignability` / `TypUnsupportedImportForm`。
    // Task 22 再 +2：`TypInvalidRecursiveType`（官方 `InvalidRecursiveType`，
    // `go/types/validtype.go`）与 `TypNestingTooDeep`（**H 判据**，与
    // `SynNestingTooDeep` 同理由：官方有可增长栈，我们落在固定宿主栈上 ⇒ 自设上限）。
    // Task 21 再 +3（版本门控）：`TypFeatureRequiresGoVersion`（官方 `UnsupportedFeature`）、
    // `TypFileRequiresNewerGo` / `TypPackageRequiresNewerGo`（官方同一个 `TooNew`，
    // 这里**拆两条** —— 位置与消息都不同，合成一条会让「哪一层太新」丢失）。
    // Task 23 再 +1（包模型）：`TypMismatchedPkgName`（官方 `MismatchedPkgName`，
    // `go/types/check.go:307`）。
    // Task 24 再 +4（声明收集）：`TypInvalidConstInit`（官方 `InvalidConstInit`，
    // `go/types/assignments.go:130`）、`TypInvalidDeclCycle`（官方 `InvalidDeclCycle`，
    // `go/types/decl.go:286-290`；与 `TypInvalidRecursiveType` 分工见枚举注释）、
    // `TypInvalidInitCycle`（官方 `InvalidInitCycle`，`go/types/initorder.go:178`）
    // 与 `TypWrongAssignCount`（官方 `WrongAssignCount`，`go/types/resolver.go:55`）。
    // Task 25 再 +3（作用域）：`TypDuplicateDecl`（官方 `DuplicateDecl`，
    // `go/types/decl.go:22`）、`TypUndeclaredName`（官方 `UndeclaredName`，
    // `go/types/typexpr.go:30`）与 `TypRepeatedDecl`（官方 `RepeatedDecl`，
    // `go/types/assignments.go:552`）—— ⚠️ 计划稿写的 `RedeclaredInThisBlock` /
    // `RedeclaredInThisPackage` / `UndefinedIdent` 在官方**不存在**（见枚举注释），
    // 故按「代号照官方」合并/改名 ⇒ 共 **56**。
    // Task 26 再 +7（导出标识符 / 标签 / 空标识符）：`TypBlankPkgName`、
    // `TypDuplicateLabel`、`TypInvalidBlank`、`TypJumpIntoBlock`、`TypMisplacedLabel`、
    // `TypUndeclaredLabel`、`TypUnusedLabel`（**全部**照官方名，见枚举注释）
    // ⇒ 共 **63**。
    // Task 27 再 +3（源码 import）：`TypBrokenImport`（**官方名**，替代计划稿点名的
    // `ImportNotFound`）、`TypImportCycle` 与 `TypUnsupportedImportForm`
    // （后两条官方无代号 —— 环检测在官方 importer 一侧且只有纯文本；见枚举注释）
    // ⇒ 共 **66**。
    // M4/Task 2 再 +1（`ChkUnimplemented` —— `CHK-` 前缀首码，`rgoc-check` 骨架期的
    // 占位诊断；官方无对应物，**不**进任何 Go 侧镜像表）⇒ 共 **67**。
    // M4/Task 6 再 +1（`TypUnsupportedAssignability` —— 可赋值性窄面的稳定拒绝码；
    // 官方无对应物）⇒ 共 **68**；M4/Task 10 再 **-1**（移除
    // `TypUnsupportedConstraintIdentity` —— `identical` 升级、`U-M3-05` 解除）
    // ⇒ 共 **67**；M4/Task 12 再 +2（`TypInvalidTypeArg` / `TypWrongTypeArgCount`
    // —— 实例化约束/数量拒绝，官方 `InvalidTypeArg` / `WrongTypeArgCount`）
    // ⇒ 共 **69**；M4/Task 14 再 +1（`TypCannotInferTypeArgs` —— 推断失败，
    // 官方 `CannotInferTypeArgs`）⇒ 共 **70**；M4/Task 16 再 +1（`TypInvalidInstanceCycle` —— 实例化链
    // 无界增长，官方 `InvalidInstanceCycle`）⇒ 共 **71**；M4/Task 16 再 +1
    // （`TypUnsupportedPendingInstanceArg` —— 进行中实例作受约束实参，我方边界码）
    // ⇒ 共 **72**；M4/Task 20 再 **-1**（移除 `ChkUnimplemented` —— 检查会话
    // 真身落地、骨架占位退出；五件套纪律「没有触发者的码不许进枚举」）
    // ⇒ 共 **71**；M4/Task 21 再 +1（`ChkInvalidOperation` —— 表达式运算符
    // 静态规则的合并码，`CHK-` 前缀真身首码）⇒ 共 **72**；M4/Task 23 再 +2
    // （`ChkInvalidIndex` / `ChkInvalidSlice` —— 索引 / 切片表达式面的合并码）
    // ⇒ 共 **74**；M4/Task 24 再 +1（`ChkInvalidSelector` —— 选择器面的合并码）
    // ⇒ 共 **75**。
    //
    // ⚠️ 这个数字是「漏代号」的对照面；`LEX-` / `SYN-` 子表另有与 Go 镜像表
    // 逐字一致的断言（`diag_code_的码表与_go_侧映射表逐字一致`），改子表时两边同改。
    assert_eq!(DiagCode::ALL.len(), 75);
    let mut s = DiagCode::ALL;
    s.sort();
    for w in s.windows(2) {
        assert_ne!(w[0], w[1], "代号不得重复：{}", w[0].as_str());
    }
}

#[test]
fn diag_bag_into_sorted_按_文件_行_列_code() {
    let mut bag = DiagBag::new();
    // 故意乱序插入
    bag.push(d(1, 1, 1, DiagCode::BadChar));
    bag.push(d(0, 2, 1, DiagCode::InvalidNul));
    bag.push(d(0, 1, 9, DiagCode::BadSep));
    bag.push(d(0, 1, 2, DiagCode::LitNoDigits));

    let sorted = bag.into_sorted();

    // C1 §4 的排序键：file → line → col → code
    assert_eq!(sorted[0].pos, p(0, 1, 2));
    assert_eq!(sorted[1].pos, p(0, 1, 9));
    assert_eq!(sorted[2].pos, p(0, 2, 1));
    assert_eq!(sorted[3].pos, p(1, 1, 1));
}

#[test]
fn 同位置的诊断按_code_排序_保证输出唯一() {
    // 同一位置两条诊断 —— 顺序若取决于插入顺序，输出就不确定（C1 §4.2）
    let mut a = DiagBag::new();
    a.push(d(0, 1, 1, DiagCode::LitNoDigits));
    a.push(d(0, 1, 1, DiagCode::BadChar));

    let mut b = DiagBag::new();
    b.push(d(0, 1, 1, DiagCode::BadChar));
    b.push(d(0, 1, 1, DiagCode::LitNoDigits));

    let ca: Vec<DiagCode> = a.into_sorted().into_iter().map(|x| x.code).collect();
    let cb: Vec<DiagCode> = b.into_sorted().into_iter().map(|x| x.code).collect();

    assert_eq!(ca, cb, "插入顺序不得影响输出顺序");
    assert_eq!(ca, [DiagCode::BadChar, DiagCode::LitNoDigits]);
}

/// 每个代号的前缀必须落在**阶段性前缀白名单**内。
///
/// 前缀 = 「哪个子系统产出它」的标签，也决定它由**哪个 oracle adapter** 镜像：
///
/// | 前缀 | 产出者 | 镜像表 |
/// |---|---|---|
/// | `LEX-` | `rgoc-lex` | `tools/token-oracle/diagmap.go` |
/// | `SYN-` | `rgoc-parse` | `tools/ast-oracle/diagmap.go`（T-AP16 建立） |
/// | `TYP-` | `rgoc-types` | `tools/types-oracle/diagmap.go`（M3/Task 31 建立） |
/// | `CHK-` | `rgoc-check` | `tools/types-oracle` 的 **C8 类别适配表**（M4/Task 3 建立；类别面，非逐码消息面）。Task 21 起由 `ChkInvalidOperation` 占用 —— Task 20 移除骨架占位码后，本前缀随首个真身码加回白名单 |
///
/// 后半段的反向断言比前半段更关键：只查「前缀合法」的话，一个前缀写错的新代号会
/// 静默落进「没人管」的区 —— 那正是 `AGENTS.md` 记的「漏了不会报错」形状。
/// 但白名单本身也不能恒真：留了个没人用的前缀，说明它对应的那一半镜像表**不存在**。
#[test]
fn diag_code_的_as_str_前缀落在阶段白名单内() {
    // `CHK-` 于 Task 21 加回：`ChkInvalidOperation` 是真身首码（五件套纪律——
    // 反向断言「每个白名单前缀必须有代号在用」不允许留空前缀）。
    const PHASES: [&str; 4] = ["LEX-", "SYN-", "TYP-", "CHK-"];

    for c in DiagCode::ALL {
        let s = c.as_str();
        assert!(
            PHASES.iter().any(|p| s.starts_with(p)),
            "前缀必须属于 {PHASES:?} 之一：{s}"
        );
        assert_eq!(
            s,
            s.to_uppercase(),
            "代号必须全大写（可排序、无大小写混排陷阱）：{s}"
        );
    }

    for p in PHASES {
        assert!(
            DiagCode::ALL.iter().any(|c| c.as_str().starts_with(p)),
            "白名单里的前缀 {p} 没有任何代号在用 —— 它对应的镜像表是空的"
        );
    }
}

#[test]
fn diag_code_的声明顺序与_as_str_字典序一致() {
    // `Ord` 是声明顺序，而 golden 与 C1 §4.2 按【字符串】排。
    // 两者不一致时，「同位置按 code 排序」在 Rust 侧与 golden 侧会给出不同顺序 ——
    // 那种不一致只在两条诊断同位置时才暴露，极难定位。
    let mut by_ord = DiagCode::ALL;
    by_ord.sort();

    let mut by_str = DiagCode::ALL;
    by_str.sort_by_key(|c| c.as_str());

    let ord_names: Vec<&str> = by_ord.iter().map(|c| c.as_str()).collect();
    let str_names: Vec<&str> = by_str.iter().map(|c| c.as_str()).collect();
    assert_eq!(ord_names, str_names);
}

#[test]
fn diag_bag_的_len_与_is_empty_跟随_push() {
    let mut bag = DiagBag::new();
    assert!(bag.is_empty());
    assert_eq!(bag.len(), 0);
    bag.push(d(0, 1, 1, DiagCode::BadChar));
    assert!(!bag.is_empty());
    assert_eq!(bag.len(), 1);
}

/// 跨语言对撞：Rust 的 **`LEX-` 子表**必须与 Go 侧 `allCodes` **逐字一致**。
///
/// # 为什么只对撞 `LEX-` 子表
///
/// `tools/token-oracle/diagmap.go` 是**词法** oracle 的映射表，它的自检 ② 要求
/// 「码表里每个代号都至少有一条**官方 `scanner.go` / `source.go` 消息**能产出它」。
/// `SYN-` 代号由 parser 产出，在那张表里**永远不可能被产出** —— 硬塞进去只会给
/// 自检 ② 多添一串需要豁免的死条目，并且让一个词法专用的文件背上语法概念。
///
/// 所以两侧各自镜像**自己能产出的那部分**：
///
/// | 前缀 | 镜像表 | 建立于 |
/// |---|---|---|
/// | `LEX-` | `tools/token-oracle/diagmap.go` | M1 |
/// | `SYN-` | `tools/ast-oracle/diagmap.go` | T-AP16 / T-AP18 |
///
/// 本断言只覆盖左半边；`SYN-` 的半边随 `tools/ast-oracle` 落地时补上。
///
/// # 为什么要跨语言去读一个 Go 文件
///
/// 这张表曾经只有 19 个代号 —— 是**凭印象**从官方报错消息里挑的，而把
/// `scanner.go` / `source.go` 里全部 `errorf`/`errorAtf`/`error` 调用点枚举出来，
/// 实际有 **25 条消息模板**。漏掉的 4 条（`LEX-BAD-DIGIT` / `LEX-BAD-RADIX-POINT` /
/// `LEX-STRING-NEWLINE` / `LEX-ESCAPE-BAD-DIGIT`）在当时**不会引发任何报错** ——
/// 只会在那 4 条消息真的出现时行为未定义。
///
/// 「两侧各维护一份清单」= 必然漂移（这正是 `AGENTS.md` 记的
/// 「写死的键列表会变成盲区」，已三次同形）。所以这里用一条断言把它们钉在一起：
/// 任何一侧改了而另一侧没改，这条测试立刻红。
///
/// 用 `CARGO_MANIFEST_DIR` 拼**绝对路径**，所以不受测试工作目录影响。
#[test]
fn diag_code_的码表与_go_侧映射表逐字一致() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../tools/token-oracle/diagmap.go");
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("读不到 Go 侧映射表 {}：{e}", path.display()));

    const HEAD: &str = "var allCodes = []string{";
    let start = src
        .find(HEAD)
        .unwrap_or_else(|| panic!("{} 里找不到 `{HEAD}` 声明", path.display()));
    let rest = &src[start..];
    let end = rest
        .find('}')
        .unwrap_or_else(|| panic!("{} 的 allCodes 声明未闭合", path.display()));
    let body = &rest[HEAD.len()..end];

    let mut go_codes: Vec<&str> = Vec::new();
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        let lit = line.strip_suffix(',').unwrap_or(line).trim();
        let lit = lit.trim_matches('"');
        if !lit.is_empty() {
            go_codes.push(lit);
        }
    }
    assert!(
        !go_codes.is_empty(),
        "解析 {} 的 allCodes 得到 0 条 —— 解析逻辑坏了（不是数据坏了）",
        path.display()
    );
    go_codes.sort_unstable();

    // 只取 `LEX-` 子表（见本测试文档注释）。前缀写错会得到**空表**，
    // 而空表会让下面那条 `assert_eq!` 以一种难读的方式失败 ——
    // 所以先立刻断言非空，把「过滤掉了一切」和「两侧真的不一致」分开报。
    let mut rs_codes: Vec<&str> = DiagCode::ALL
        .iter()
        .map(|c| c.as_str())
        .filter(|s| s.starts_with("LEX-"))
        .collect();
    assert!(
        !rs_codes.is_empty(),
        "按 `LEX-` 前缀过滤后得到 0 条 —— 前缀写错了（不是数据坏了）"
    );
    rs_codes.sort_unstable();

    assert_eq!(
        rs_codes, go_codes,
        "Rust 的 `LEX-` 子表与 Go 侧 allCodes 不一致 —— 两侧必须同步（见 `M1-design.md` §7.2 的 D-M1-11）"
    );
}
