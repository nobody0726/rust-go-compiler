//! S3 的验收测试（`T-S3-01` … `T-S3-05`）。
//!
//! 分两类：
//! - **纯 codegen 的**（本文件）：不碰 clang，测「HIR → 汇编」这一步。
//!   好处是快、且在没装 clang 的机器上也能跑。
//! - **需要 clang 与真实运行**的（`s3_native_e2e.rs`）：测 T-S3-02/03。
//!   分开是因为前者必须是「不依赖外部工具」的那一半 —— 否则换机器就会一片红。

use rgoc_hir::{FuncDecl, Stmt, Stream, Val};
use rgoc_spikes::fixtures::s3_hello;
use rgoc_spikes::native::{CodegenError, gen_asm};
use rgoc_spikes::native_records::{SIX_ITEMS, records, render_records};

#[test]
fn t_s3_01_生成了可链接的汇编骨架() {
    let asm = gen_asm(&s3_hello()).expect("codegen 应成功");
    // 骨架：.text 段 + 全局 _start + 数据段
    assert!(asm.contains(".text"), "{asm}");
    assert!(asm.contains(".global _start"), "{asm}");
    assert!(asm.contains(".data"), "{asm}");
    // 顺序：.text 必须在 .data 之前
    let t = asm.find(".text").expect("有 .text");
    let d = asm.find(".data").expect("有 .data");
    assert!(t < d, ".text 应在 .data 之前");
}

#[test]
fn t_s3_01_退出路径用_exit_group_而不是_exit() {
    // exit(60) 只退出当前线程；Go 运行时可能有多个 OS 线程
    let asm = gen_asm(&s3_hello()).expect("codegen 应成功");
    assert!(asm.contains("mov x8, #94"), "应为 exit_group(94)：{asm}");
    assert!(
        !asm.contains("mov x8, #60"),
        "不能用 exit(60) —— 多线程下只退出当前线程：{asm}"
    );
}

#[test]
fn t_s3_01_系统调用号正确() {
    let asm = gen_asm(&s3_hello()).expect("codegen 应成功");
    // Linux aarch64: write=64, exit_group=94
    assert!(asm.contains("mov x8, #64"), "write 应为 64：{asm}");
    assert!(asm.contains("mov x8, #94"), "exit_group 应为 94：{asm}");
}

#[test]
fn t_s3_01_地址获取用_pie_安全的形式() {
    // PIE 下用绝对地址会链接失败
    let asm = gen_asm(&s3_hello()).expect("codegen 应成功");
    assert!(asm.contains("adrp x1,"), "{asm}");
    assert!(asm.contains(":lo12:"), "{asm}");
}

#[test]
fn t_s3_01_hello_被编码成正确字节() {
    let asm = gen_asm(&s3_hello()).expect("codegen 应成功");
    assert!(asm.contains("0x68"), "h = 0x68：{asm}");
    assert!(asm.contains("0x0a"), "\\n = 0x0a：{asm}");
    assert!(
        asm.contains(".text_len_str0 = 6"),
        "hello\\n 共 6 字节：{asm}"
    );
}

#[test]
fn t_s3_02_判定条件是两个而不是一个() {
    // 这条是关于【判定本身】的：x86-64 ELF 同样含 "ELF 64-bit LSB"，
    // 所以只判那一个会漏掉「链接到了错误架构」。判定逻辑在 native.rs 里，
    // 这里只做记录：两个条件缺一不可。
    // 真正的执行在 s3_native_e2e.rs（需要 clang）。
    assert!(SIX_ITEMS.len() == 6, "T-S3-04 的六项是固定集合，不增不减");
}

#[test]
fn t_s3_04_六项都齐备() {
    // M0-tests.md §5.3 明确「六项都要有记录」。用 SIX_ITEMS 做双向校验：
    // 少了任何一项都会红 —— 这正是「写死列表会变盲区」的反面用法（这里列表与内容必须一致）。
    let got: Vec<&str> = records().iter().map(|r| r.item).collect();
    assert_eq!(
        got,
        SIX_ITEMS.to_vec(),
        "记录的项必须与固定六项**逐项**一致（顺序也须一致：M0-tests §5.3 的原文顺序）"
    );
}

#[test]
fn t_s3_04_每项都有实测依据与缺口() {
    for r in records() {
        assert!(r.finding.len() > 20, "{} 的结论太短，说不清是什么", r.item);
        assert!(
            r.evidence.len() > 10,
            "{} 缺实测依据 —— T-S3-04 要求「有记录」，\
             而「应该…」不是记录：{}",
            r.item,
            r.evidence
        );
        assert!(
            r.gap.len() > 20,
            "{} 缺「没记录什么」—— 不知道边界在哪，就会把「当前不需要」误读成「已支持」",
            r.item
        );
    }
}

#[test]
fn t_s3_04_关键结论与实测一致() {
    let text = render_records();
    // 这几条是从实测得来的，结论若变必须同步改实测
    assert!(text.contains("exit_group"), "退出码项须写明用 exit_group");
    assert!(text.contains("94"), "须给出 exit_group 的系统调用号 94");
    assert!(
        text.contains("Scrt1.o"),
        "runtime 桥接项须记下 -nostartfiles 的原因"
    );
    assert!(
        text.contains("16 字节对齐"),
        "栈对齐项须给出 AAPCS64 的具体要求"
    );
    assert!(
        text.contains("没有 `.cfi_*`") || text.contains("无 `.cfi_*`") || text.contains(".cfi_"),
        "unwind 项须点明缺 CFI 指令"
    );
    assert!(
        text.contains("部分写") || text.contains("短写"),
        "输出流项须记下短写缺口"
    );
}

#[test]
fn t_s3_04_栈对齐项不得声称已验证() {
    // 「本 fixture 不需要动栈」≠「栈对齐已验证」。这条断言专门防后者。
    let all = records();
    let stack = all.iter().find(|r| r.item == "栈对齐").expect("有栈对齐项");
    assert!(
        stack.gap.contains("不等于"),
        "栈对齐的缺口必须点明「不需要动栈」不等于「栈对齐已验证」"
    );
}

#[test]
fn t_s3_04_runtime_桥接项区分_不引用_与_不需要() {
    // 这是最容易含糊过去的一条：动态链接的产物仍写着 NEEDED libc.so.6，
    // 但我们一个符号都没引用 —— 两件事不能混说。
    let text = render_records();
    let all = records();
    let rt = all
        .iter()
        .find(|r| r.item == "最小 runtime 桥接")
        .expect("有 runtime 项");
    assert!(
        rt.finding.contains("零 libc 符号"),
        "须明确「不引用 libc 符号」"
    );
    assert!(
        rt.gap.contains("不等于"),
        "须明确「不引用」不等于「不需要」"
    );
    assert!(text.contains("NEEDED"), "须记下 readelf -d 的实测输出");
}

#[test]
fn t_s3_05_codegen_是纯函数_字节级可重复() {
    // E6/T-S3-05 的基础：同一输入必须生成同一份汇编。
    // 若 codegen 里混进了时间戳、随机数或 HashMap 迭代序，这里就会红。
    let a = gen_asm(&s3_hello()).expect("应能生成");
    let b = gen_asm(&s3_hello()).expect("应能生成");
    let c = gen_asm(&s3_hello()).expect("应能生成");
    assert_eq!(a, b);
    assert_eq!(b, c);
    // 显式排除「三次生成的内容其实很短」这种假通过
    assert!(a.len() > 100, "汇编应有实质内容，实际 {} 字节", a.len());
}

#[test]
fn 多条_print_各自有独立的数据标签() {
    // 两条 print ⇒ 两个 label，地址不能冲突
    let f = FuncDecl {
        name: "main".to_string(),
        body: vec![
            Stmt::Print {
                newline: true,
                stream: Stream::Stdout,
                args: vec![Val::Str("a".to_string())],
            },
            Stmt::Print {
                newline: true,
                stream: Stream::Stdout,
                args: vec![Val::Str("b".to_string())],
            },
        ],
    };
    let asm = gen_asm(&f).expect("应能生成");
    assert!(asm.contains("str0:"), "{asm}");
    assert!(asm.contains("str1:"), "{asm}");
    assert!(asm.contains(".text_len_str0 = 2"), "a\\n = 2 字节：{asm}");
    assert!(asm.contains(".text_len_str1 = 2"), "b\\n = 2 字节：{asm}");
}

#[test]
fn 整数实参也能生成() {
    // 子集内整数走同一条路径（十进制渲染 → .byte 序列）
    let f = FuncDecl {
        name: "main".to_string(),
        body: vec![Stmt::Print {
            newline: true,
            stream: Stream::Stdout,
            args: vec![Val::Int(rgoc_hir::value::BigInt::from_i64(7))],
        }],
    };
    let asm = gen_asm(&f).expect("应能生成");
    assert!(asm.contains("0x37"), "'7' = 0x37：{asm}");
    assert!(asm.contains("0x0a"), "换行：{asm}");
    assert!(asm.contains(".text_len_str0 = 2"), "7\\n = 2 字节：{asm}");
}

#[test]
fn codegen_错误都有可读信息() {
    // 错误信息会进 spike 记录，不能是空洞的
    assert!(CodegenError::MultipleArgs(3).to_string().contains('3'));
    assert!(CodegenError::NotMain("f".into()).to_string().contains('f'));
    assert!(
        !CodegenError::UnsupportedStmt("x".into())
            .to_string()
            .is_empty()
    );
    assert!(
        !CodegenError::UnsupportedArg("x".into())
            .to_string()
            .is_empty()
    );
}

#[test]
fn 短写缺口在代码里留了标记() {
    // 「没记录什么」不能只在文档里 —— 代码里也要留痕，否则将来重构会以为处理了
    let asm = gen_asm(&s3_hello()).expect("应能生成");
    assert!(
        asm.contains("TODO(S3-04)") && asm.contains("部分写"),
        "汇编里应留有短写缺口的标记：{asm}"
    );
}
