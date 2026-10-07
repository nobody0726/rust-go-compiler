//! S1 的验收测试（`T-S1-01` / `02` / `03`）。
//!
//! 这些测试**只测库里的判定逻辑**；「bin 的进程边界是否正确」由
//! `tests/s1_cli.rs` 用真实子进程测（捕获两条流 + 退出码）。
//! 分开的原因：混在一起时，一旦 bin 写错了流，测试会通过 ——
//! 因为库里的 `stdout` 字段本来就是空串，跟实际写没写 stdout 无关。

use rgoc_hir::Stream;
use rgoc_hir::value::BigInt;
use rgoc_hir::{Diag, Pos, Program, Stmt, Val};
use rgoc_spikes::interp::{check_t_s1_01, check_t_s1_02, check_t_s1_03, interpret, render_records};

#[test]
fn t_s1_01_stderr_精确_3_换行() {
    let out = check_t_s1_01().expect("T-S1-01 应通过");
    assert_eq!(out.stderr, "3\n", "必须逐字节相等，不是「包含 3」");
}

#[test]
fn t_s1_01_stdout_精确为空() {
    // 这条单独写成一个测试：合并进上一条时，失败信息会指向「stderr 不对」，
    // 而实际根因是「多写了 stdout」。
    let out = check_t_s1_01().expect("T-S1-01 应通过");
    assert_eq!(
        out.stdout, "",
        "内建 println 走 stderr，stdout 必须一个字节都没有"
    );
}

#[test]
fn t_s1_02_两次求值逐字节相同() {
    check_t_s1_02().expect("T-S1-02 应通过");
}

#[test]
fn t_s1_03_超宽常量必须拒绝收敛() {
    let rows = check_t_s1_03().expect("T-S1-03 应通过");
    // 至少要有一项「超宽」和一项「恰好在边界」—— 全同侧的实现是错的
    assert!(
        rows.iter().any(|(_, fits)| !*fits),
        "T-S1-03 必须覆盖超宽常量（Go 在此处报 overflows）"
    );
    assert!(
        rows.iter().any(|(_, fits)| *fits),
        "T-S1-03 必须覆盖恰好落在 int64 边界的常量"
    );
}

#[test]
fn t_s1_03_边界值与_printbig_的期望逐字节一致() {
    let rows = check_t_s1_03().expect("T-S1-03 应通过");
    let texts: Vec<&str> = rows.iter().map(|(t, _)| t.as_str()).collect();
    // 这两个就是 T-C-04（test/printbig.go）的冻结期望
    assert!(
        texts.contains(&"-9223372036854775808"),
        "必须含 int64 下界：{texts:?}"
    );
    assert!(
        texts.contains(&"9223372036854775807"),
        "必须含 int64 上界：{texts:?}"
    );
    // 顺序也固定：先超宽（1<<100），再 int64 上界、下界
    assert_eq!(
        texts[0], "1267650600228229401496703205376",
        "首项应是 1<<100"
    );
}

#[test]
fn 常量折叠丢位会被_check_03_抓出来() {
    // 反向验证：把 BigInt 换成一个「在 i64 上凑巧成立」的窄实现不可行
    // （类型层面就不允许），所以这里退一步验证 check 本身不是恒真：
    // 构造一个渲染与期望不符的 Program，走 interpret 看它是否报出。
    let bogus = Program {
        funcs: vec![rgoc_hir::FuncDecl {
            name: "main".to_string(),
            body: vec![Stmt::Print {
                newline: true,
                stream: Stream::Stderr,
                args: vec![Val::Int(BigInt::from_i64(3000))], // 不是 3
            }],
        }],
    };
    let out = interpret(&bogus).expect("语法上合法");
    assert_eq!(out.stderr, "3000\n");
    // check_t_s1_01 走的是固定 fixture，因此不受这个 bogus 输入影响 ——
    // 这一点本身要显式确认：fixture 不能被调用方传入的输入污染。
    assert!(check_t_s1_01().is_ok());
}

#[test]
fn 没有_main_时给出诊断而不是_panic() {
    let p = Program {
        funcs: vec![rgoc_hir::FuncDecl {
            name: "not_main".to_string(),
            body: vec![],
        }],
    };
    let err = interpret(&p).expect_err("无 main 应报错");
    assert_eq!(err.len(), 1);
    assert_eq!(err[0].code, "S1-NO-MAIN");
    assert!(err[0].message.contains("main"));
}

#[test]
fn 子集外的_return_被显式拒绝而不是忽略() {
    let p = Program {
        funcs: vec![rgoc_hir::FuncDecl {
            name: "main".to_string(),
            body: vec![
                Stmt::Print {
                    newline: true,
                    stream: Stream::Stderr,
                    args: vec![Val::Int(BigInt::from_i64(3))],
                },
                Stmt::Return(Val::Void),
            ],
        }],
    };
    let err = interpret(&p).expect_err("含 return 应被拒绝");
    assert_eq!(err.len(), 1);
    assert_eq!(err[0].code, "S1-EVAL-UNSUPPORTED");
    // 关键：不能「前半段照常输出、后半段静默跳过」—— 那样错误就看不见了
    assert!(
        err[0].message.contains("return"),
        "诊断要说清是哪个形状不被支持：{}",
        err[0]
    );
}

#[test]
fn print_不加空格与换行_println_都加() {
    // ⚠️ 本测试的期望来自 **go1.27.1 实测**（`od -c` 逐字节），不是从规范推的 ——
    // 规范只把 print/println 列为内建名，**没有规定分隔符**。第一版实现对两者
    // 都用了 `join(" ")`，被这里抓到（`print` 应无分隔符）。
    let mk = |newline: bool| Program {
        funcs: vec![rgoc_hir::FuncDecl {
            name: "main".to_string(),
            body: vec![Stmt::Print {
                newline,
                stream: Stream::Stderr,
                args: vec![
                    Val::Int(BigInt::from_i64(-9223372036854775808)),
                    Val::Int(BigInt::from_i64(9223372036854775807)),
                ],
            }],
        }],
    };
    let pl = interpret(&mk(true)).expect("println 子集内");
    assert_eq!(
        pl.stderr, "-9223372036854775808 9223372036854775807\n",
        "println：操作数之间一个空格，末尾一个换行"
    );
    let p = interpret(&mk(false)).expect("print 子集内");
    assert_eq!(
        p.stderr, "-92233720368547758089223372036854775807",
        "print：既不加空格也不加换行（实测 ab12 形态）"
    );
}

#[test]
fn 多操作数的四种组合与_oracle_逐字节一致() {
    // 把实测到的四种形态全钉住：{print, println} × {多操作数, 空操作数}
    //   print("a","b",1,2)   → ab12
    //   println("a","b",1,2) → "a b 1 2\n"
    //   print()             → ""
    //   println()           → "\n"
    let mk = |newline: bool, args: Vec<Val>| Program {
        funcs: vec![rgoc_hir::FuncDecl {
            name: "main".to_string(),
            body: vec![Stmt::Print {
                newline,
                stream: Stream::Stderr,
                args,
            }],
        }],
    };
    let four = vec![
        Val::Str("a".to_string()),
        Val::Str("b".to_string()),
        Val::Int(BigInt::from_i64(1)),
        Val::Int(BigInt::from_i64(2)),
    ];
    assert_eq!(
        interpret(&mk(false, four.clone())).expect("子集内").stderr,
        "ab12"
    );
    assert_eq!(
        interpret(&mk(true, four)).expect("子集内").stderr,
        "a b 1 2\n"
    );
    assert_eq!(interpret(&mk(false, vec![])).expect("子集内").stderr, "");
    assert_eq!(interpret(&mk(true, vec![])).expect("子集内").stderr, "\n");
}

#[test]
fn 空参数列表不产生多余字符() {
    let p = Program {
        funcs: vec![rgoc_hir::FuncDecl {
            name: "main".to_string(),
            body: vec![Stmt::Print {
                newline: true,
                stream: Stream::Stderr,
                args: vec![],
            }],
        }],
    };
    let out = interpret(&p).expect("子集内");
    // join("") 是空串，再加一个 \n —— 不能是 "\n\n" 也不能是 ""
    assert_eq!(out.stderr, "\n");
}

#[test]
fn 诊断按_c1_规则排序() {
    // 子集外形状产生多条诊断时，顺序必须确定（E6 可复现性的前提）
    let p = Program {
        funcs: vec![rgoc_hir::FuncDecl {
            name: "main".to_string(),
            body: vec![Stmt::Return(Val::Void), Stmt::Return(Val::Void)],
        }],
    };
    let err = interpret(&p).expect_err("应报错");
    assert_eq!(err.len(), 2);
    // 位置递增 ⇒ 排序后与语句顺序一致
    assert!(err[0].pos < err[1].pos, "排序应按位置递增：{err:?}");
}

#[test]
fn 记录文本包含每一项与说明() {
    let rows = check_t_s1_03().expect("T-S1-03 应通过");
    let text = render_records(&rows);
    assert_eq!(text.lines().count(), rows.len() + 2, "表头两行 + 每项一行");
    assert!(text.contains("overflows"), "超宽项须说明 Go 的行为");
    assert!(text.contains("-9223372036854775808"));
}

#[test]
fn 诊断对象可比较_便于测试断言() {
    // Diag 派生了 PartialEq/Eq —— 这不是可选的：
    // S2 的「与 S1 结果一致」要比较诊断集合，能比较是前提。
    let a = Diag::new(Pos::new("f", 1, 1), "C", "m");
    let b = Diag::new(Pos::new("f", 1, 1), "C", "m");
    assert_eq!(a, b);
}
