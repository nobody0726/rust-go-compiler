//! `CgCode` 的形状判据（`T-NAT-52` 的枚举面）。
//!
//! 这里钉的是**码表本身**：变体数、字典序、命名前缀、文本唯一。
//! 「某个码真的在某个输入上出现」由 `rgoc-driver/tests/native_reject.rs`（Task 37/58）负责 ——
//! 两条判据不能合并：码表对了不代表拒绝路径通了，反之亦然。

use rgoc_arm64::diag::CgCode;

#[test]
fn 全部码恰好两条且顺序是字典序() {
    // 写死条数：新增变体而不更新 `ALL` ⇒ 这里红（与 `SsaCode` 的计数测试同款）。
    //
    // M9/Phase 1 起是 **2** 条：`CG-UNSUPPORTED-SLICE` 下线了（切片真的被选择）。
    // 那条码连同它的判据一起删 —— 留一条永远不产出的码比删掉更糟。
    assert_eq!(CgCode::ALL.len(), 2);
    let mut sorted = CgCode::ALL;
    sorted.sort();
    assert_eq!(sorted, CgCode::ALL, "ALL 必须是字典序");
}

#[test]
fn 码文本统一前缀且互不相同() {
    let mut seen = std::collections::BTreeSet::new();
    for c in CgCode::ALL {
        let s = c.as_str();
        assert!(
            s.starts_with("CG-UNSUPPORTED-"),
            "{s} 不以 CG-UNSUPPORTED- 开头"
        );
        assert!(seen.insert(s), "{s} 重复");
    }
}

#[test]
fn display_与_as_str_一致() {
    for c in CgCode::ALL {
        assert_eq!(c.to_string(), c.as_str());
    }
}

#[test]
fn 码不进_diagcode_的命名空间() {
    // `DiagCode` 保持 103（`D-M7-13`）。这条断言防的是「有人图省事把 CG-* 塞进 DiagCode」：
    // 那时 `C8` 的 26 类投影会多出一个 `other` 成员，`T-CK-16` 立刻红。
    // 这里只钉住「SSA/M7 的码不在 DiagCode 里」这一事实的另一半 —— 前缀不同族。
    for c in CgCode::ALL {
        assert!(
            !c.as_str().starts_with("TYP-"),
            "{} 撞了检查器的命名空间",
            c.as_str()
        );
        assert!(
            !c.as_str().starts_with("SSA-"),
            "{} 撞了 SSA 的命名空间",
            c.as_str()
        );
    }
}
