//! M12 b 线（有限反射）的判据。
//!
//! # T-REF-00：三个子命令都收 `-I`
//!
//! `types` 从 M3 起就有 `-I <root>`（import 解析根，`Ruling 27-1`），但
//! `check` / `run` / `build` 一直传 `&[]` —— b 线要在 `stdlib/reflect`
//! 上做文章，而反射包自己要被 import，没有 `-I` 就什么都摆不开。
//! 判据：`-I <tmp>/root` 里放一个 `fmt/fmt.go` 桩，三个子命令都必须认
//! 这个旗标并把根透传进 `load_with_imports`。
//!
//! # 为什么 run/build 的 main 只 import 不使用
//!
//! 检查面能解析跨包引用（① 用 `fmt.Println`），但 **HIR lowering 还在
//! M5 的单包面**（实测：`跨包引用 fmt.Println 不在 M5 面`）—— 跨包调用
//! 进 HIR 是另一笔账，不在这个 Task 的范围里。run/build 这里要钉的只有
//! 「旗标被接受 + 根透传 + import 不再被拒」这一件事，所以用
//! 「import 但不使用」的 main（未使用 import 今天不报诊断，与官方的
//! 「imported and not used」的差距属于检查面覆盖，不在本判据管）。
//!
//! 这条判据**不是**反射语义的判据 —— 它只是 b 线的地基：没有 `-I`，
//! `import "reflect"` 连第一步都走不进去。

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("仓库根")
        .to_path_buf()
}

fn driver() -> &'static str {
    env!("CARGO_BIN_EXE_rgoc-driver")
}

/// `root/fmt/fmt.go` 桩 + 一个 `import "fmt"` 的 main，返回 `(root, main)`。
///
/// `use_it` = true ⇒ main 里真的调 `fmt.Println`（给检查面用）；
/// false ⇒ 只 import（给 run/build 用，见模块注释）。
fn setup(root_tag: &str, use_it: bool) -> (PathBuf, PathBuf) {
    let tmp = std::env::temp_dir().join(format!(
        "rgoc-m12-reflect-{root_tag}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(tmp.join("root/fmt")).expect("建 root/fmt 失败");
    fs::write(
        tmp.join("root/fmt/fmt.go"),
        "package fmt\n\nfunc Println(s string) {}\n",
    )
    .expect("写 fmt 桩失败");
    let body = if use_it {
        "func main() { fmt.Println(\"x\") }\n"
    } else {
        "func main() {}\n"
    };
    let main = tmp.join("main.go");
    fs::write(&main, format!("package main\n\nimport \"fmt\"\n\n{body}")).expect("写 main 失败");
    (tmp.join("root"), main)
}

/// 旗标写成 `-I root`（分参数，与 `types` 同款）。
#[test]
fn t_ref_00_import_root() {
    // ── ① check：跨包引用在检查面必须解析到 ─────────────────────────
    let (root, main) = setup("check", true);
    let out = Command::new(driver())
        .args(["check", "-I"])
        .arg(&root)
        .arg(&main)
        .output()
        .expect("起不了 check");
    let _ = fs::remove_dir_all(root.parent().unwrap());
    assert_eq!(
        out.status.code(),
        Some(0),
        "check -I 应当认旗标并解析到 fmt 桩：\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    // ── ② run：旗标被接受 + import 不再被拒 ─────────────────────────
    let (root, main) = setup("run", false);
    let out = Command::new(driver())
        .args(["run", "-I"])
        .arg(&root)
        .arg(&main)
        .output()
        .expect("起不了 run");
    let _ = fs::remove_dir_all(root.parent().unwrap());
    assert_eq!(
        out.status.code(),
        Some(0),
        "run -I 应当认旗标并解析到 fmt 桩：\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    // ── ③ build：同上 ───────────────────────────────────────────────
    let (root, main) = setup("build", false);
    let out = Command::new(driver())
        .args(["build", "-I"])
        .arg(&root)
        .arg(&main)
        .output()
        .expect("起不了 build");
    let _ = fs::remove_dir_all(root.parent().unwrap());
    assert_eq!(
        out.status.code(),
        Some(0),
        "build -I 应当认旗标并解析到 fmt 桩：\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    // ── ④ 负例：没有 -I 时 import 必须照旧被拒（旗标不是万能开关）────
    let (root, main) = setup("negative", false);
    let out = Command::new(driver())
        .args(["check"])
        .arg(&main)
        .output()
        .expect("起不了 check（负例）");
    let _ = fs::remove_dir_all(root.parent().unwrap());
    assert_eq!(
        out.status.code(),
        Some(1),
        "没有 -I 时 import \"fmt\" 必须照旧被拒：\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("TYP-UNSUPPORTED-IMPORT-FORM"),
        "拒绝理由应当还是 import 形式不支持：\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// ════════════════════════════════════════════════════════════════════
// T-REF-01 白名单对账（Task 29）
// ════════════════════════════════════════════════════════════════════

/// `stdlib/reflect/reflect.go` 的导出 `func`/`type` 集合必须与这里登记的
/// 集合**相等**（两边都从源码读：源码侧正则提取，判据侧下表登记）。
///
/// # 为什么是「相等」不是「包含」
///
/// 反射的面是**刻意收窄**的（`U-M12-02`：不做 `MakeFunc`）。用「包含」
/// 的话，将来有人顺手加一个导出符号，白名单纹丝不动 —— 收窄面就漏了。
/// 相等意味着：**多一个、少一个都得来改这张表**（§11.3 的冻结纪律）。
///
/// # 为什么 `MakeFunc` 单独点名
///
/// 它是官方反射里最常用的高阶入口，也最容易被「顺手补上」—— 但它需要
/// 反向 trampoline（Go 调栈 → C 调栈的 ABI 桥），是独立的一块工作。
/// 断言它**不在**集合里：想加它必须同时改这条判据（露头就红）。
///
/// # 注册集合 = Task 29 骨架的实际交付
///
/// 后续 Task（31 读面 / 32 方法表 / 33 Call / 34 写面）每加一个导出
/// `func`/`type`，这张表跟着长一行 —— 那是有意识的动作，不是静默扩散。
#[test]
fn t_ref_01_白名单对账() {
    let src = fs::read_to_string(repo_root().join("stdlib/reflect/reflect.go"))
        .expect("stdlib/reflect/reflect.go 不存在（b 线骨架还没落？）");

    // 源码侧：顶层导出 func / type（行首无缩进）。
    let mut got: BTreeSet<String> = BTreeSet::new();
    for line in src.lines() {
        if let Some(rest) = line.strip_prefix("func ") {
            let name = rest
                .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .next()
                .unwrap_or("");
            if name.starts_with(|c: char| c.is_ascii_uppercase()) {
                got.insert(format!("func {name}"));
            }
        }
        if let Some(rest) = line.strip_prefix("type ") {
            let name = rest
                .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .next()
                .unwrap_or("");
            if name.starts_with(|c: char| c.is_ascii_uppercase()) {
                got.insert(format!("type {name}"));
            }
        }
    }

    // 判据侧：登记集合（Task 31 读面：StructTag / StructField 随
    // Field(i) 的读面一起进来；Task 32 方法表：Method 随 NumMethod/Method
    // 一起进来 —— 官方 `reflect.Method` 的收窄版；Task 34 写面：四个
    // 包级构造函数进来 —— 方法（`Value.SetInt` 之类）不计入这张表，
    // 它只看**行首无缩进**的 `func` / `type`）。
    let want: BTreeSet<String> = [
        "type Kind",
        "type Type",
        "type StructTag",
        "type StructField",
        "type Method",
        "type Value",
        "func TypeOf",
        "func ValueOf",
        "func New",
        "func MakeSlice",
        "func MakeMap",
        "func MakeChan",
    ]
    .into_iter()
    .map(String::from)
    .collect();

    let extra: Vec<_> = got.difference(&want).collect();
    let missing: Vec<_> = want.difference(&got).collect();
    assert!(
        extra.is_empty() && missing.is_empty(),
        "导出面与登记表不一致：\n多出的（要改表先说明理由）：{extra:?}\n少了的：{missing:?}"
    );

    assert!(
        !got.iter().any(|s| s.contains("MakeFunc")),
        "MakeFunc 不该出现（U-M12-02：需要反向 trampoline，独立工作）"
    );

    // 骨架必须过检查面（-I stdlib 指向仓库的 stdlib/ 根）。
    let main = std::env::temp_dir().join(format!("rgoc-ref01-main-{}.go", std::process::id()));
    fs::write(
        &main,
        "package main\n\nimport \"reflect\"\n\nfunc main() { _ = reflect.TypeOf(0); _ = reflect.ValueOf(0) }\n",
    )
    .expect("写 main 失败");
    let out = Command::new(driver())
        .args(["check", "-I"])
        .arg(repo_root().join("stdlib"))
        .arg(&main)
        .output()
        .expect("起不了 check");
    let _ = fs::remove_file(&main);
    assert_eq!(
        out.status.code(),
        Some(0),
        "reflect 骨架自己必须过检查面：\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

// ════════════════════════════════════════════════════════════════════
// T-REF-02 元数据逐字段（Task 30 发射面 / Task 31 读面 API 共用）
// ════════════════════════════════════════════════════════════════════

/// `t_ref_02` 的 fixture：20 个类型的读面探测，builtin `println` 逐行输出。
///
/// 行序是判据的一部分：每类型先 `dump`（kind/size/align/name/pkgpath 五行），
/// struct 追加 numfield + 每 field 四行（name/tag/offset/typekind），
/// 容器类追加 elem/key kind 与 array 的 len。
const T_REF_02_SRC: &str = r#"package main

import "reflect"

type Point struct {
	X int
	Y string
}

type Box[T any] struct {
	v T
}

func dump(t reflect.Type) {
	println(t.Kind())
	println(t.Size())
	println(t.Align())
	println(t.Name())
	println(t.PkgPath())
}

func fields(t reflect.Type) {
	println(t.NumField())
	for i := 0; i < t.NumField(); i++ {
		f := t.Field(i)
		println(f.Name)
		println(f.Tag)
		println(f.Offset)
		println(f.Type.Kind())
	}
}

func main() {
	dump(reflect.TypeOf(int(1)))      // 1
	dump(reflect.TypeOf(int8(1)))     // 2
	dump(reflect.TypeOf(int64(1)))    // 3
	dump(reflect.TypeOf(uint8(1)))    // 4
	dump(reflect.TypeOf(float32(1)))  // 5
	dump(reflect.TypeOf(float64(1)))  // 6
	dump(reflect.TypeOf(true))        // 7
	dump(reflect.TypeOf("s"))         // 8
	dump(reflect.TypeOf(complex128(1i))) // 9
	dump(reflect.TypeOf(new(int)))    // 10 *int
	t := reflect.TypeOf(new(int))
	println(t.Elem().Kind())
	dump(reflect.TypeOf([]string{}))  // 11
	t = reflect.TypeOf([]string{})
	println(t.Elem().Kind())
	dump(reflect.TypeOf(map[string]int{})) // 12
	t = reflect.TypeOf(map[string]int{})
	println(t.Key().Kind())
	println(t.Elem().Kind())
	dump(reflect.TypeOf(make(chan int))) // 13
	t = reflect.TypeOf(make(chan int))
	println(t.Elem().Kind())
	dump(reflect.TypeOf(func(int) string { return "" })) // 14
	dump(reflect.TypeOf(Point{}))                        // 15
	fields(reflect.TypeOf(Point{}))
	dump(reflect.TypeOf(struct {
		X int `json:"x"`
	}{})) // 16
	fields(reflect.TypeOf(struct {
		X int `json:"x"`
	}{}))
	dump(reflect.TypeOf([3]int{})) // 17
	t = reflect.TypeOf([3]int{})
	println(t.Len())
	println(t.Elem().Kind())
	dump(reflect.TypeOf(Box[int]{})) // 18
	fields(reflect.TypeOf(Box[int]{}))
	// **nil 接口上的方法调用**必须按官方 panic（文本逐字对齐）——
	// `TypeOf(nil 接口)` 返回 nil `Type`，`t.Kind()` 是 nil itab 派发。
	// 放在**最后**：panic 之后的块不会执行（官方行为，oracle 实测）。
	dump(reflect.TypeOf(interface{}(nil)))
}
"#;

/// 期望输出：**官方 Go 的语义常量**（`reflect.Kind` 的枚举序照官方
/// `runtime/type.go`；size/align 照官方 ABI 事实），不是从 rgoc 实现里
/// 抄的 —— 与 `desc_emit.rs` 的「期望值来自冻结事实」同一条纪律。
///
/// 每类型 5 行（kind/size/align/name/pkgpath）+ 追加行，与 fixture 的
/// 打印序逐行对应。name/pkgpath 空则输出空行。
#[test]
fn t_ref_02_元数据逐字段() {
    let tmp = std::env::temp_dir().join(format!("rgoc-ref02-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).expect("建 scratch 失败");
    let main = tmp.join("main.go");
    fs::write(&main, T_REF_02_SRC).expect("写 fixture 失败");

    // **native 通路**（Ruling 31-1）：反射是 native 特性 —— `any` 的两字
    // 表示 `{*TypeDesc, 数据指针}` 在解释器的 `Value` 模型里不可表达
    // （`(*[2]unsafe.Pointer)(unsafe.Pointer(&i))` 的重解释，实测
    // 2026-10-28「取字段的底座不是结构体」）。解释器缺口登记
    // `G-M12-05`，本判据走 build + 执行产物。
    let bin = tmp.join("ref02bin");
    let build = Command::new(driver())
        .args(["build", "-I"])
        .arg(repo_root().join("stdlib"))
        .arg("-o")
        .arg(&bin)
        .arg(&main)
        .output()
        .expect("起不了 build");
    assert_eq!(
        build.status.code(),
        Some(0),
        "反射读面 fixture 应当能编译：\n{}{}",
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr)
    );

    let out = Command::new(&bin).output().expect("起不了产物");
    let got = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let want = want_ref02_output();
    assert_eq!(
        out.status.code(),
        Some(2),
        "nil 接口的方法调用必须按官方 panic（rc=2）：\n{got}"
    );
    assert_eq!(got, want, "18 个类型的读面必须与官方逐字段相同");
}

/// 期望序列（与 `T_REF_02_SRC` 的打印序一一对应）。
///
/// `Kind` 枚举值照官方 `runtime/type.go`：Bool=1 Int=2 Int8=3 … Uint=7
/// Uint8=8 … Float32=13 Float64=14 … Complex128=16 Array=17 Chan=18
/// Func=19 Interface=20 Map=21 Ptr=22 Slice=23 String=24 Struct=25。
/// size/align 照官方 ABI（string 16、interface 16、func/chan/map/ptr 8、
/// complex128 16；struct 按字段序无填充时 24/8）。
///
/// **结尾是 nil 接口 panic**（官方行为：`TypeOf(nil 接口)` ⇒ nil `Type`
/// ⇒ 方法调用 nil 解引用；oracle 实测 rc=2、goroutine 转储不算内容契约）。
fn want_ref02_output() -> String {
    let mut want: Vec<String> = Vec::new();
    // (kind, size, align, name, pkgpath)
    let scalars: &[(&str, &str, &str, &str)] = &[
        ("2", "8", "8", "int"),          // 1  int
        ("3", "1", "1", "int8"),         // 2  int8
        ("6", "8", "8", "int64"),        // 3  int64
        ("8", "1", "1", "uint8"),        // 4  uint8
        ("13", "4", "4", "float32"),     // 5  float32
        ("14", "8", "8", "float64"),     // 6  float64
        ("1", "1", "1", "bool"),         // 7  bool
        ("24", "16", "8", "string"),     // 8  string
        ("16", "16", "8", "complex128"), // 9 complex128
    ];
    for (k, s, a, n) in scalars {
        want.extend([
            k.to_string(),
            s.to_string(),
            a.to_string(),
            n.to_string(),
            String::new(),
        ]);
    }
    // 10 *int + elem
    want.extend(["22", "8", "8", "", "", "2"].map(String::from));
    // 11 []string + elem
    want.extend(["23", "24", "8", "", "", "24"].map(String::from));
    // 12 map[string]int + key/elem
    want.extend(["21", "8", "8", "", "", "24", "2"].map(String::from));
    // 13 chan int + elem
    want.extend(["18", "8", "8", "", "", "2"].map(String::from));
    // 14 func(int) string
    want.extend(["19", "8", "8", "", ""].map(String::from));
    // 15 Point + fields（X@0 int、Y@8 string）
    want.extend(["25", "24", "8", "Point", "main"].map(String::from));
    want.extend(
        [
            "2", // NumField
            "X", "", "0", "2", "Y", "", "8", "24",
        ]
        .map(String::from),
    );
    // 16 匿名 struct{ X int `json:"x"` } + fields
    want.extend(["25", "8", "8", "", ""].map(String::from));
    want.extend(
        [
            "1", // NumField
            "X",
            "json:\"x\"",
            "0",
            "2",
        ]
        .map(String::from),
    );
    // 17 [3]int + len/elem
    want.extend(["17", "24", "8", "", ""].map(String::from));
    want.extend(["3", "2"].map(String::from));
    // 18 Box[int] + fields（v@0 int）—— 官方 Name() 是**实例化后的拼写**
    // `Box[int]`（Task 30 手写 want 的 "Box" 是错的，oracle 实测）
    want.extend(["25", "8", "8", "Box[int]", "main"].map(String::from));
    want.extend(
        [
            "1", // NumField
            "v", "", "0", "2",
        ]
        .map(String::from),
    );
    // nil 接口的方法调用 ⇒ 官方 panic 文本（一行；转储不算内容契约）。
    want.push(
        "panic: runtime error: invalid memory address or nil pointer dereference".to_string(),
    );

    let mut s = want.join("\n");
    s.push('\n');
    s
}

// ════════════════════════════════════════════════════════════════════
// T-REF-05 方法表 + 类型身份（Task 32）
// ════════════════════════════════════════════════════════════════════

/// `t_ref_05` 的 fixture：实例的 `String()` / 方法表 / 实现与赋值转换判定。
///
/// * `Box[int]`（泛型实例，值接收者 `Get` + 指针接收者 `Set`）：
///   `String()` 是 `main.Box[int]`（实例化拼写），`NumMethod()` 官方口径
///   只数**值接收者**的（=1），`Method(0).Name == "Get"`（字母序）。
/// * `Num`（具名基本类型，方法 `Two`）实现 `Twoer` 接口。
/// * `Implements/AssignableTo/ConvertibleTo` 各取官方判真与判假各一。
const T_REF_05_SRC: &str = r#"package main

import "reflect"

type Box[T any] struct {
	v T
}

func (b Box[T]) Get() T   { return b.v }
func (b *Box[T]) Set(x T) { b.v = x }

type Num int

func (n Num) Two() int { return 2 }

type Twoer interface {
	Two() int
}

func main() {
	bi := reflect.TypeOf(Box[int]{})
	bs := reflect.TypeOf(Box[string]{})
	println(bi.String())
	println(bs.String())
	gt := reflect.TypeOf((*Twoer)(nil)).Elem()
	println(bi.NumMethod())
	println(bi.Method(0).Name)
	n := reflect.TypeOf(Num(0))
	println(n.NumMethod())
	println(n.Method(0).Name)
	println(n.Implements(gt))
	println(reflect.TypeOf(7).Implements(gt))
	println(n.AssignableTo(gt))
	println(n.AssignableTo(reflect.TypeOf(Num(0))))
	println(n.ConvertibleTo(reflect.TypeOf(0)))
	println(reflect.TypeOf(3.5).ConvertibleTo(n))
}
"#;

/// 期望输出 —— **官方 `go run` 的逐行实测**（oracle，2026-10-27；
/// 12 行：String×2 / NumMethod+Method 名×2 组 / Implements 判真假 /
/// AssignableTo 判真假 / ConvertibleTo 判真假）。
#[test]
fn t_ref_05_方法表与类型身份() {
    let tmp = std::env::temp_dir().join(format!("rgoc-ref05-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).expect("建 scratch 失败");
    let main = tmp.join("main.go");
    fs::write(&main, T_REF_05_SRC).expect("写 fixture 失败");

    let bin = tmp.join("ref05bin");
    let build = Command::new(driver())
        .args(["build", "-I"])
        .arg(repo_root().join("stdlib"))
        .arg("-o")
        .arg(&bin)
        .arg(&main)
        .output()
        .expect("起不了 build");
    assert_eq!(
        build.status.code(),
        Some(0),
        "反射方法表 fixture 应当能编译：\n{}{}",
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr)
    );

    let out = Command::new(&bin).output().expect("起不了产物");
    let got = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let want = [
        "main.Box[int]",
        "main.Box[string]",
        "1",
        "Get",
        "1",
        "Two",
        "true",
        "false",
        "true",
        "true",
        "true",
        "true",
    ]
    .join("\n");
    // println 逐行输出 ⇒ 末尾有换行。
    let want = want + "\n";
    assert_eq!(out.status.code(), Some(0), "fixture 必须正常退出：\n{got}");
    assert_eq!(got, want, "方法表与类型身份必须与官方逐行相同");
}

// ════════════════════════════════════════════════════════════════════
// T-REF-03：`Call` 的 ABI 分档（Task 33）
// ════════════════════════════════════════════════════════════════════

/// `t_ref_03` 的 fixture：九档各一条，每条都打印**可观察的数值**
/// （`Value` 的读访问器是这条判据的观察面，见 `reflect.go` 的注释）。
///
/// | 档 | 形态 | 期望 |
/// |---|---|---|
/// | 0 参数 | `func() int` | 42 |
/// | 1..8 整型 | `func(int,int,int) int` | 123 |
/// | 1..8 浮点 | `func(float64,float64) float64` | 5.25 |
/// | 整浮混合 | `func(int,float64,int) float64` | 5.5 |
/// | 超 8 个（走栈）| `func(10×int) int` | 55 |
/// | 聚合体按值 | `func(S) int`（16 字节 struct）| 34 |
/// | 多返回值 | `func(int) (int,int)` | 21 42 |
/// | 零大小参数 | `func(struct{}) int` | 7 |
/// | `nil` 参数 | `func(*int) int` | −1 |
///
/// 浮点结果乘 100 取整再打：`println` 的浮点格式是**另一条契约**
/// （`C9`），不该让 ABI 判据去背它。`0.5×2 + 2.25 = 5.25`、`2+0.5+3 = 5.5`
/// 在二进制里都是精确值，乘 100 不会有舍入噪声。
const T_REF_03_SRC: &str = r#"package main

import "reflect"

type S struct {
	A int
	B int
}

func f0() int { return 42 }

func f3(a, b, c int) int { return a*100 + b*10 + c }

func f2f(a, b float64) float64 { return a*2 + b }

func fmix(a int, b float64, c int) float64 { return float64(a) + b + float64(c) }

func f10(a1, a2, a3, a4, a5, a6, a7, a8, a9, a10 int) int {
	return a1 + a2 + a3 + a4 + a5 + a6 + a7 + a8 + a9 + a10
}

func fs(s S) int { return s.A*10 + s.B }

func f2r(a int) (int, int) { return a, a * 2 }

func fz(z struct{}) int { return 7 }

func fp(p *int) int {
	if p == nil {
		return -1
	}
	return *p
}

func main() {
	println("0-param", reflect.ValueOf(f0).Call(nil)[0].Int())
	println("3-int", reflect.ValueOf(f3).Call([]reflect.Value{reflect.ValueOf(1), reflect.ValueOf(2), reflect.ValueOf(3)})[0].Int())
	println("2-float", int(reflect.ValueOf(f2f).Call([]reflect.Value{reflect.ValueOf(1.5), reflect.ValueOf(2.25)})[0].Float()*100))
	println("mixed", int(reflect.ValueOf(fmix).Call([]reflect.Value{reflect.ValueOf(2), reflect.ValueOf(0.5), reflect.ValueOf(3)})[0].Float()*100))
	println("10-arg", reflect.ValueOf(f10).Call([]reflect.Value{
		reflect.ValueOf(1), reflect.ValueOf(2), reflect.ValueOf(3), reflect.ValueOf(4), reflect.ValueOf(5),
		reflect.ValueOf(6), reflect.ValueOf(7), reflect.ValueOf(8), reflect.ValueOf(9), reflect.ValueOf(10),
	})[0].Int())
	println("agg", reflect.ValueOf(fs).Call([]reflect.Value{reflect.ValueOf(S{3, 4})})[0].Int())
	r := reflect.ValueOf(f2r).Call([]reflect.Value{reflect.ValueOf(21)})
	println("multi", r[0].Int(), r[1].Int())
	println("zero", reflect.ValueOf(fz).Call([]reflect.Value{reflect.ValueOf(struct{}{})})[0].Int())
	println("nil", reflect.ValueOf(fp).Call([]reflect.Value{reflect.ValueOf((*int)(nil))})[0].Int())
	println("func-kind", reflect.ValueOf(f0).Kind())
}
"#;

/// 期望输出 —— 官方 `go run` 的逐行实测（oracle，2026-10-29）。
///
/// 最后一行是 `Kind()` 的**数值**：官方 `reflect.Kind` 的枚举序里
/// `Func = 19`（`go run` 打的是 `Kind().String()` 的 `func`）——
/// 这里要的是「档位没串」，所以钉数值。
#[test]
fn t_ref_03_call的abi分档() {
    let tmp = std::env::temp_dir().join(format!("rgoc-ref03-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).expect("建 scratch 失败");
    let main = tmp.join("main.go");
    fs::write(&main, T_REF_03_SRC).expect("写 fixture 失败");

    let bin = tmp.join("ref03bin");
    let build = Command::new(driver())
        .args(["build", "-I"])
        .arg(repo_root().join("stdlib"))
        .arg("-o")
        .arg(&bin)
        .arg(&main)
        .output()
        .expect("起不了 build");
    assert_eq!(
        build.status.code(),
        Some(0),
        "Call 的九档 fixture 应当能编译：\n{}{}",
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr)
    );

    let out = Command::new(&bin).output().expect("起不了产物");
    let _ = fs::remove_dir_all(&tmp);
    let got = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let want = [
        "0-param 42",
        "3-int 123",
        "2-float 525",
        "mixed 550",
        "10-arg 55",
        "agg 34",
        "multi 21 42",
        "zero 7",
        "nil -1",
        "func-kind 19",
    ]
    .join("\n");
    let want = want + "\n";
    assert_eq!(out.status.code(), Some(0), "fixture 必须正常退出：\n{got}");
    assert_eq!(got, want, "九档 ABI 必须与官方逐行相同");
}

// ════════════════════════════════════════════════════════════════════
// T-REF-04 写面（Task 34）
// ════════════════════════════════════════════════════════════════════

/// 编译 + 运行一个 fixture，返回（合并流, 退出码）。
fn build_run(tag: &str, src: &str) -> (String, Option<i32>) {
    let tmp = std::env::temp_dir().join(format!("rgoc-ref04-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).expect("建 scratch 失败");
    let main = tmp.join("main.go");
    fs::write(&main, src).expect("写 fixture 失败");
    let bin = tmp.join("bin");
    let build = Command::new(driver())
        .args(["build", "-I"])
        .arg(repo_root().join("stdlib"))
        .arg("-o")
        .arg(&bin)
        .arg(&main)
        .output()
        .expect("起不了 build");
    assert_eq!(
        build.status.code(),
        Some(0),
        "fixture 应当能编译：\n{}{}",
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr)
    );
    let out = Command::new(&bin).output().expect("起不了产物");
    let _ = fs::remove_dir_all(&tmp);
    let got = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (got, out.status.code())
}

/// `t_ref_04` 的 fixture：**每条 API 一个断言**，十三条。
///
/// 三条造可写值的路：`New(t).Elem()` / `Index(i)` / 上一层的延续；
/// 四个改写器：`SetInt` / `SetFloat` / `SetBool` / `SetString`（`Set`
/// 单独一条）；三个容器构造器：`MakeSlice` / `MakeMap` / `MakeChan`；
/// `CanSet` 真假各一条。
///
/// 浮点用 `int(...)` 取整再打：`println` 的浮点格式是另一条契约（`C9`），
/// 不该让写面的判据去背它。
const T_REF_04_SRC: &str = r#"package main

import "reflect"

func main() {
	p := reflect.New(reflect.TypeOf(0))
	v := p.Elem()
	println("canset-true", v.CanSet())
	v.SetInt(41)
	println("after-setint", v.Int())

	v.Set(reflect.ValueOf(7))
	println("after-set", v.Int())

	println("new-kind", int(p.Kind()))

	q := reflect.New(reflect.TypeOf(0))
	q.Elem().SetInt(99)
	println("new-elem", q.Elem().Int())

	f := reflect.New(reflect.TypeOf(0.0)).Elem()
	f.SetFloat(25)
	println("setfloat", int(f.Float()))

	b := reflect.New(reflect.TypeOf(false)).Elem()
	b.SetBool(true)
	println("setbool", b.Bool())

	s := reflect.New(reflect.TypeOf("")).Elem()
	s.SetString("hello")
	println("setstring", s.String())

	sl := reflect.MakeSlice(reflect.TypeOf([]int{}), 3, 5)
	println("makeslice", sl.Len(), sl.Cap())

	sl.Index(0).SetInt(11)
	println("slice0", sl.Index(0).Int())

	m := reflect.MakeMap(reflect.TypeOf(map[string]int{}))
	println("map-len", m.Len())

	c := reflect.MakeChan(reflect.TypeOf(make(chan int)), 2)
	println("chan-cap", c.Cap())

	println("canset-false", reflect.ValueOf(3).CanSet())
}
"#;

/// `CanSet` 为假时 `SetInt` 必须 panic（`T-REF-04` Step 2）。
const T_REF_04_PANIC_UNADDR: &str = r#"package main

import "reflect"

func main() {
	reflect.ValueOf(3).SetInt(4)
	println("unreachable")
}
"#;

/// 零 `Value` 的写面 panic（官方是 `ValueError`，不是「不可寻址」）。
const T_REF_04_PANIC_ZERO: &str = r#"package main

import "reflect"

func main() {
	var v reflect.Value
	v.SetInt(1)
	println("unreachable")
}
"#;

#[test]
fn t_ref_04_写面逐条() {
    let (got, code) = build_run("main", T_REF_04_SRC);
    let want = [
        "canset-true true",
        "after-setint 41",
        "after-set 7",
        "new-kind 22",
        "new-elem 99",
        "setfloat 25",
        "setbool true",
        "setstring hello",
        "makeslice 3 5",
        "slice0 11",
        "map-len 0",
        "chan-cap 2",
        "canset-false false",
    ]
    .join("\n");
    let want = want + "\n";
    assert_eq!(code, Some(0), "fixture 必须正常退出：\n{got}");
    assert_eq!(got, want, "写面十三条必须与官方 `go run` 逐行相同");
}

/// `CanSet` 为假 / 零 `Value` 的 panic：**文本逐字对齐官方**，退出 2。
///
/// 官方两条消息来自 `flag.mustBeAssignableSlow`：
///   - 有类型但没地址 ⇒ `<method> using unaddressable value`
///   - `flag == 0`      ⇒ `ValueError{method, Invalid}` ⇒ `call of <method> on zero Value`
///
/// 顺序也是判据的一部分：**先零值、再不可寻址**。反过来写的话，
/// 零 `Value` 会打出「unaddressable」——那是错的。
#[test]
fn t_ref_04_panic文本对齐官方() {
    for (tag, src, want) in [
        (
            "unaddr",
            T_REF_04_PANIC_UNADDR,
            "panic: reflect: reflect.Value.SetInt using unaddressable value\n",
        ),
        (
            "zero",
            T_REF_04_PANIC_ZERO,
            "panic: reflect: call of reflect.Value.SetInt on zero Value\n",
        ),
    ] {
        let (got, code) = build_run(tag, src);
        assert_eq!(code, Some(2), "{tag}：panic 必须退出 2：\n{got}");
        assert_eq!(got, want, "{tag}：panic 文本必须与官方逐字相同");
    }
}
