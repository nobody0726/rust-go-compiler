//! **T-S2-02**：memory / tuple / 调用边界的需求清单（`03` §4 第 5 条）。
//!
//! `03` §4 第 5 条要求 SSA spike **识别**这三项需求 —— 不是实现它们，
//! 而是「把需要什么写清楚」。所以本文件是一份**可被审阅的清单**，
//! 每一条都给出：触发它的 Go 形状、为什么 Block/Value 图本身不够、M0 的处置。
//!
//! # 为什么清单要带「最小可验证例子」
//!
//! 只写「需要 memory 模型」是一句空话 —— 任何人重写 M6 时都能再写一遍。
//! 每条都配一个**当前子集装不下**的具体 Go 例子，于是「装不下」是可验证的事实，
//! 而不是设计者的印象。
//!
//! # 本清单不是 M6 的设计
//!
//! M6 会产正式契约（`docs/contracts/C4`，见 `M0-plan.md` T51）。本清单的定位是
//! **M6 的输入**，所以它只回答「需要什么、为什么、谁来验」，**不回答「怎么实现」**。

/// 一条需求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Need {
    /// 稳定代号（排序与引用用，理由同 `rgoc_hir::Diag` 的 `code`）。
    pub id: &'static str,
    /// 需求名。
    pub title: &'static str,
    /// 触发它的 Go 形状 —— 必须**具体到能写成一个测试**。
    pub trigger: &'static str,
    /// 为什么当前的 Block/Value 图装不下。
    pub why: &'static str,
    /// M0 的处置（这一条决定「为什么现在不做」）。
    pub m0_disposition: &'static str,
    /// 归谁（哪个里程碑）。
    pub owner: &'static str,
}

/// 全部需求：memory / tuple / 调用边界三类。
pub fn needs() -> Vec<Need> {
    vec![
        Need {
            id: "S2-NEED-MEMORY",
            title: "memory（可变内存 / 别名）",
            trigger: "func f() { a := 1; p := &a; *p = 2; return a }",
            why: "Block/Value 图里每个值是**不可变且唯一定义**的。`a` 被 `*p = 2` 改写后，\
                  它的值取决于「谁最后写」，这与 SSA 的「值不变」直接冲突。\
                  必须引入显式的 memory SSA 形式（内存作为**另一个**SSA 值，\
                  load/store 作为带副作用的指令），否则别名分析无处安放。",
            m0_disposition: "**不实现**。fixture 是纯表达式（println(1+2)），没有可变状态。\
                            本条只登记需求 —— M0 若不登记，Phase 4 的 C4 契约就会漏掉它。",
            owner: "M6（`03` §M6：SSA、verifier 与精简优化）",
        },
        Need {
            id: "S2-NEED-TUPLE",
            title: "tuple（多返回值）",
            trigger: "func two() (int, error) { return 1, nil }",
            why: "Go 的多返回值不是「结构体」而是**语言级 tuple**：可被整体赋值给 `a, b :=`、\
                  可被直接传参、还能出现在 range 与赋值左侧。SSA 的值是**单个**的，\
                  所以要么给每个返回值各自一个值并在调用点建一组「结果向量」，\
                  要么引入 bundle 类型。前者会让调用边界与赋值边界都变复杂。",
            m0_disposition: "**不实现**。三个 fixture 都不含多返回值。",
            owner: "M5（M5b：函数、帧与初始化，明确列了「多返回值」）",
        },
        Need {
            id: "S2-NEED-CALL",
            title: "调用边界",
            trigger: "println(1+2) —— 参数在进入内建之前就已是值",
            why: "M0 的 HIR 把内建实参存成**已求值的 `Val`**（`rgoc_hir::Stmt::Print`），\
                  于是调用边界画在 HIR 层，S2 的图里**看不到调用**。\
                  真实编译里调用边界是三件事的重合：① 传参约定（栈/寄存器）、\
                  ② 调用点之后的控制流（被调用方可能不返回）、③ 结果的接收方式。\
                  这三件事都要求调用在图里**显式存在**，否则 SSA 的控制流边无从画起。",
            m0_disposition: "**只记录，不实现**。当前形态让 S1/S2 能专注比较求值结果；\
                            但必须写明这是个**简化**，否则 M6 会误以为调用边界已经解决。",
            owner: "M6（调用与调用边）；M5（Go 侧调用语义）",
        },
        Need {
            id: "S2-NEED-PHI",
            title: "phi（块参数 / 支配）",
            trigger: "func f(c bool) int { if c { return 1 }; return 2 }",
            why: "分支汇合处同一个「逻辑变量」有两个定义点。SSA 用 **phi 指令**在块入口\
                  选值来恢复「唯一定义」。本子集无分支，`Block::params` 刻意留空作为容身处，\
                  这样 M6 接手时能看到「phi 该挂在哪里」，而不是重新发明。",
            m0_disposition: "**不实现**。`Block::params` / `Block::succs` 字段已留位（恒空）。",
            owner: "M6",
        },
        Need {
            id: "S2-NEED-VERIFIER",
            title: "verifier（图不变式检查）",
            trigger: "任何多块的图 —— 本 spike 只有一个块，所以**触发不了**",
            why: "SSA 的正确性依赖一组不变式：唯一定义点、使用先于定义、类型一致、\
                  phi 位于汇合处。这些必须**被检查**而不是被信任。",
            m0_disposition: "**不实现**（`M0-tests.md` §5.2 T-S2-03 明确「不要求 M0 实现 verifier」）。\
                            差距记录见 `crate::verifier_gap`。",
            owner: "M6（verifier 是 M6 的门禁内容之一）",
        },
    ]
}

/// 渲染成可落盘的清单文本（T45 写进 benchmarks §12）。
pub fn render_needs() -> String {
    let mut s =
        String::from("T-S2-02 SSA 需求清单（memory / tuple / 调用边界，另附 phi 与 verifier）\n");
    for n in needs() {
        s.push_str(&format!("\n## {} {}\n", n.id, n.title));
        s.push_str(&format!("触发形状: {}\n", n.trigger));
        s.push_str(&format!("为什么图不够: {}\n", n.why));
        s.push_str(&format!("M0 处置: {}\n", n.m0_disposition));
        s.push_str(&format!("归属: {}\n", n.owner));
    }
    s
}

/// **T-S2-03**：与 M6 完整 verifier 的差距。
///
/// 结构刻意是「已具备 / 未具备 / 何时才需要」三栏 —— 因为「差距」若只写「缺 verifier」，
/// 就无法判断**缺多少**。
pub fn verifier_gap() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        (
            "唯一定义点",
            "✅ 具备",
            "`ValueId` 是不可变的新类型，`Builder::define` 是唯一的 ID 来源，\
             类型系统保证同一个 ID 不会被定义两次。",
        ),
        (
            "使用先于定义",
            "✅ 具备（直线程序内）",
            "后序遍历 + 顺序求值，使「定义必在使用之前」成为结构性事实而非约定。\
             ⚠️ 多块时**不再成立** —— 那时要靠 phi 与支配关系补上。",
        ),
        (
            "控制流图",
            "❌ 不具备",
            "当前只有一个块，`succs` 恒空。没有分支就没有边，也就没有「汇合」概念。",
        ),
        (
            "phi 正确性",
            "❌ 不具备",
            "没有 phi 指令，也没有块参数。`Block::params` 只是**容身处**。",
        ),
        (
            "支配关系（dominator）",
            "❌ 不具备",
            "单块图里支配关系是平凡的（全可达），因此**从未被验证过**。\
             这是最容易「以为自己已经会了」的一项。",
        ),
        (
            "类型一致性",
            "❌ 不具备",
            "`eval_bin` 遇到非整数实参返回 `Err`（运行时检查），但图**本身**没有类型标注，\
             所以「两个 i32 相加」与「一个 i32 加一个 string」在图里长得一样。",
        ),
        (
            "副作用 / 内存",
            "❌ 不具备",
            "指令全是纯函数（const / bin）。没有 load / store，\
             也就无法检查「先写后读」的顺序。见 `S2-NEED-MEMORY`。",
        ),
        (
            "调用指令",
            "❌ 不具备",
            "图里没有 call 指令 —— print 的实参在 HIR 层已是值。见 `S2-NEED-CALL`。",
        ),
    ]
}

/// 渲染差距记录（T45 落盘）。
pub fn render_gap() -> String {
    let mut s = String::from("T-S2-03 与 M6 完整 verifier 的差距\n");
    s.push_str("（✅ = 当前子集已具备；❌ = 不具备，且**从未被验证过**——两者要分开）\n");
    for (item, status, note) in verifier_gap() {
        s.push_str(&format!("\n## {item}: {status}\n{note}\n"));
    }
    s
}
