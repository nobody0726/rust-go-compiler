//! emitter 的**确定性**判据（`T-NAT-48` 的骨架版）。
//!
//! 三条性质，缺一条 `T-NAT-48` 就立不住：
//!
//! 1. **同输入同输出**：两次 `render` 逐字节相同。
//! 2. **标签按调用顺序分配**：不按内容去重 —— 否则「同一段字节第二次出现」会拿到旧标签，
//!    整个文件的编号随内容重复情况而变（= 依赖内容的不确定源）。
//! 3. **段的顺序固定**：先写常量后写代码，与反过来写，产物相同。

use rgoc_arm64::emit::Asm;

/// 造一份「有代码、有字符串、有浮点、有分支标签」的样例。
fn sample() -> Asm {
    let mut a = Asm::new();
    a.global("_start");
    a.label("_start");
    let s = a.rodata_bytes(b"hello, world\n");
    let f = a.rodata_f64(0x3ff0_0000_0000_0000); // 1.0
    let l0 = a.fresh_label();
    a.ins(&format!("adrp x1, {s}"));
    a.ins(&format!("add x1, x1, :lo12:{s}"));
    a.ins(&format!("ldr d0, [x2, :lo12:{f}]"));
    a.ins("cbnz x0, .Lend{l0}");
    a.label(&format!(".Lend{l0}"));
    a.ins("ret");
    a
}

#[test]
fn t_nat_48_同一输入两次渲染逐字节相同() {
    let first = sample().render();
    let second = sample().render();
    assert_eq!(first, second, "两次 render 必须逐字节相同");
    // 再来一次，防「只有前两次碰巧相同」
    assert_eq!(sample().render(), first);
}

#[test]
fn 标签按调用顺序递增() {
    let mut a = Asm::new();
    assert_eq!(a.rodata_bytes(b"a"), ".Lstr0");
    assert_eq!(a.rodata_bytes(b"b"), ".Lstr1");
    assert_eq!(a.rodata_f64(0), ".Lf64_0");
    assert_eq!(a.fresh_label(), 0);
    assert_eq!(a.fresh_label(), 1);
}

#[test]
fn 相同内容不去重_第二次拿到新标签() {
    // 这条是「不许按内容做标签名」的**可执行版本**：
    // 若有人把 `rodata_bytes` 改成「内容 → 标签」的 HashMap 缓存，
    // 第二次调用会返回 `.Lstr0`，这里立刻红。
    let mut a = Asm::new();
    let l1 = a.rodata_bytes(b"same");
    let l2 = a.rodata_bytes(b"same");
    assert_ne!(l1, l2, "同一段字节两次申请必须是两个标签（不按内容去重）");
    assert_eq!((l1.as_str(), l2.as_str()), (".Lstr0", ".Lstr1"));
}

#[test]
fn 空串也占一个字节的存储() {
    // 空串必须有**可取的地址**：Go 允许 `len(s)==0` 时 `&s[0]` 合法。
    // 若这里什么都不写，标签后面紧跟的可能是下一个标签 ⇒ `adrp/add` 指到别处。
    let mut a = Asm::new();
    let l = a.rodata_bytes(b"");
    let text = a.render();
    assert!(
        text.contains(&format!("{l}:\n  .byte 0")),
        "空串要写一个 0 字节"
    );
}

#[test]
fn 段的顺序与书写顺序无关() {
    let mut a = Asm::new();
    a.label("f");
    a.ins("ret");
    a.rodata_bytes(b"x");

    let mut b = Asm::new();
    b.rodata_bytes(b"x");
    b.label("f");
    b.ins("ret");

    assert_eq!(a.render(), b.render(), "先常量后代码 == 先代码后常量");
}

#[test]
fn 产物不含地址与时间() {
    let text = sample().render();
    // 十六进制字面量只允许出现在 `.quad 0x…`（位模式常量）里。
    // 其余任何 `0x` 都意味着「把某个运行期地址写进了汇编」—— 那正是不可复现的根。
    for line in text.lines() {
        if line.contains("0x") {
            assert!(
                line.trim_start().starts_with(".quad 0x"),
                "可疑的十六进制常量（可能是地址）：{line}"
            );
        }
    }
    for bad in ["/Users/", "/work/", "/Volumes/", "2026-", "T00:"] {
        assert!(!text.contains(bad), "产物里不该出现 {bad}");
    }
}

#[test]
fn 数据段与只读段共用标签计数器() {
    // 两个段里的标签同处一个命名空间：分开计数就会出现 `.Lstr3` 同时在两段里。
    let mut a = Asm::new();
    let r = a.rodata_bytes(b"r");
    let d = a.data_bytes(b"d");
    assert_ne!(r, d, "只读段与数据段的标签不许撞");
}
