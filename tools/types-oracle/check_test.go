// check_test.go —— M4/Task 3：`types-oracle --check` 的验收测试（C8 契约可执行副本）。
//
// 覆盖（M4-plan Task 3 Step 1）：
//   - parseFlags 向量：lang 提取 / -fakeImportC 与 -goexperiment 响亮失败 / 无指令
//   - shouldTest 向量（官方 check_test.go 同形）
//   - commentMap / marker 计数向量（前缀正则、ERRORx、位置=前一 token、行首注释 ⇒ 0,0）
//   - 适配表向量（真实普查消息）+ C8 §3 类别词表对账
//   - 渲染：排序（行,列,类别）+ 重复保留 + 头行
//   - cheap-trick 行为锁（首错 + 后续 invalid operand/type 类跟随错被抑制）
//   - min fixture（testdata/check-diags-min.go ↔ docs/contracts/testdata/check-diags-min.want）
//   - 响亮失败：shouldTest=false / 不支持的首行旗标 / 读不到文件
package main

import (
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
	"testing"
)

// c8Categories：C8 §3 冻结的 26 类（含逃逸口 other），字典序。
// TestC8CategoryVocabulary 会与 docs/contracts/C8-check-diags.md 的枚举块对账。
var c8Categories = []string{
	"assign", "builtin", "comma-ok", "convert", "cycle", "decl", "implements", "import",
	"index", "not-type", "op-invalid", "other", "overflow", "parse", "range", "ret",
	"selector", "shift", "slice", "stmt", "terminate", "type-arg", "undef", "unused",
	"unsafe", "version",
}

func TestParseCheckFlags(t *testing.T) {
	tests := []struct {
		name         string
		src          string
		lang         string
		goexperiment string
		fakeImportC  bool
		wantErr      string // 非空 ⇒ 期望 error 且包含该子串
	}{
		{name: "无指令", src: "package p\n", lang: "", goexperiment: "", fakeImportC: false},
		{name: "非注释首行", src: "/* x */ package p\n", lang: ""},
		{name: "注释不以 - 开头", src: "// hello -lang=go1.12\npackage p\n", lang: ""},
		{name: "lang 提取", src: "// -lang=go1.12\npackage p\n", lang: "go1.12"},
		{name: "lang 后跟其他字段", src: "// -lang=go1.18 -fakeImportC\npackage p\n", lang: "go1.18", fakeImportC: true},
		{name: "fakeImportC=false 允许", src: "// -fakeImportC=false\npackage p\n", fakeImportC: false},
		{name: "goexperiment 提取", src: "// -lang=go1.21 -goexperiment=rangefunc\npackage p\n", lang: "go1.21", goexperiment: "rangefunc"},
		{name: "首行过长", src: "// -lang=go1.12 " + strings.Repeat("x", 300) + "\npackage p\n", wantErr: "too long"},
		{name: "未知旗标", src: "// -unknown=1\npackage p\n", wantErr: "unsupported flag"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got, err := parseCheckFlags([]byte(tt.src))
			if tt.wantErr != "" {
				if err == nil || !strings.Contains(err.Error(), tt.wantErr) {
					t.Fatalf("parseCheckFlags(%q) err = %v, want contains %q", tt.src, err, tt.wantErr)
				}
				return
			}
			if err != nil {
				t.Fatalf("parseCheckFlags(%q) err = %v, want nil", tt.src, err)
			}
			if got.Lang != tt.lang || got.GoExperiment != tt.goexperiment || got.FakeImportC != tt.fakeImportC {
				t.Fatalf("parseCheckFlags(%q) = %+v, want {Lang:%q GoExperiment:%q FakeImportC:%v}",
					tt.src, got, tt.lang, tt.goexperiment, tt.fakeImportC)
			}
		})
	}
}

func TestShouldTest(t *testing.T) {
	tests := []struct {
		name string
		src  string
		want bool
	}{
		{name: "无约束", src: "package p\n", want: true},
		{name: "go:build ignore 排除", src: "//go:build ignore\n\npackage p\n", want: false},
		{name: "go:build !ignore 保留", src: "//go:build !ignore\n\npackage p\n", want: true},
		{name: "老式 +build 排除", src: "// +build ignore\n\npackage p\n", want: false},
		{name: "package 行之后不看", src: "package p\n\n//go:build ignore\n", want: true},
		{name: "已知 ReleaseTag", src: "//go:build go1.12\n\npackage p\n", want: true},
		{name: "未知 ReleaseTag", src: "//go:build go1.99\n\npackage p\n", want: false},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := shouldTest([]byte(tt.src)); got != tt.want {
				t.Fatalf("shouldTest(%q) = %v, want %v", tt.src, got, tt.want)
			}
		})
	}
}

func TestCommentMapMarkers(t *testing.T) {
	// 行内注释：marker 位置 = **前一 token**（官方 commentMap 同款语义）。
	m := commentMap([]byte("package p\n\nvar x = 1 // ERROR \"boom\"\n"))
	cs := m[3]
	if len(cs) != 1 {
		t.Fatalf("commentMap: line 3 has %d comments, want 1 (map=%v)", len(cs), m)
	}
	if cs[0].Line != 3 || cs[0].Col != 9 {
		t.Fatalf("marker pos = %d:%d, want 3:9 (前一 token)", cs[0].Line, cs[0].Col)
	}
	if got := countMarkers([]byte("package p\n\nvar x = 1 // ERROR \"boom\"\n")); got != 1 {
		t.Fatalf("countMarkers = %d, want 1", got)
	}

	// 行首注释：没有任何前一 token ⇒ (0,0)（官方对 NoPos 的行为）。
	m = commentMap([]byte("// ERROR \"x\"\npackage p\n"))
	cs = m[0]
	if len(cs) != 1 || cs[0].Line != 0 || cs[0].Col != 0 {
		t.Fatalf("行首注释 marker = %v, want [(0,0)]", cs)
	}

	// 前缀正则：`^ ERRORx? `（前导空格必需；ERRORx 变体；/* */ 等价）。
	vectors := []struct {
		name string
		src  string
		want int
	}{
		{"标准 ERROR", "package p\n\nvar _ = x // ERROR \"undefined\"\n", 1},
		{"ERRORx 变体", "package p\n\nvar _ = x /* ERRORx \"undef.*\" */\n", 1},
		{"无前导空格不计数", "package p\n\nvar _ = x //ERROR \"undefined\"\n", 0},
		{"双空格不计数", "package p\n\nvar _ = x //  ERROR \"undefined\"\n", 0},
		{"非前缀不计数", "package p\n\nvar _ = x // hello ERROR \"undefined\"\n", 0},
		{"块注释行首", "/* ERROR \"x\" */\npackage p\n", 1},
	}
	for _, v := range vectors {
		t.Run(v.name, func(t *testing.T) {
			if got := countMarkers([]byte(v.src)); got != v.want {
				t.Fatalf("countMarkers(%q) = %d, want %d", v.src, got, v.want)
			}
		})
	}
}

func TestClassifyCheckMessage(t *testing.T) {
	// 向量取自 m4census 的真实消息（.superpowers/sdd/M4-plan/probe/m4census）。
	tests := []struct {
		msg  string
		want string
	}{
		// version 优先（可叠加在别的句式上）
		{"type instantiation requires go1.18 or later", "version"},
		{"cannot range over 10 (untyped int constant): requires go1.22 or later", "version"},
		{"cannot convert s (variable of slice type Slice) to type Array: conversion of slice to array requires go1.20 or later", "version"},
		// unused / undef
		{"0 (untyped int constant) is not used", "unused"},
		{"undefined: C", "undef"},
		{"label _ not declared", "undef"},
		// selector
		{"x.m undefined (type P has no field or method m)", "selector"},
		{"ambiguous selector x.m", "selector"},
		// shift
		{"invalid operation: shifted operand 1 (type float64) must be integer", "shift"},
		{"constant shift overflow", "shift"},
		// type-arg（须压过 assign 泛规则）
		{"in call to g, cannot infer P", "type-arg"},
		{"cannot use a type parameter as constraint", "type-arg"},
		{"invalid operation: myInt[(ast: *ast.BadExpr)] (myInt is not a generic type)", "type-arg"},
		// comma-ok
		{"multiple-value f() (value of type (a int, b int)) in single-value context", "comma-ok"},
		{"impossible type assertion: x.(T1)", "comma-ok"},
		{"use of .(type) outside type switch", "comma-ok"},
		// implements
		{"T does not satisfy comparable", "implements"},
		{"cannot use type I outside a type constraint: interface contains type constraints", "implements"},
		// decl 特例：is not constant（须压过 unsafe 泛规则）
		{"unsafe.Alignof(t) (value of type uintptr) is not constant", "decl"},
		// assign（须压过 convert/overflow 句式后缀；untyped nil 归 assign）
		{"cannot use t (variable of type *T) as *S value in assignment", "assign"},
		{"cannot use min(1.2, 3) (untyped float constant 1.2) as int value in constant declaration (truncated)", "assign"},
		{"cannot use maxInt + 1 (untyped int constant 9223372036854775808) as int value in assignment (overflows)", "assign"},
		{"use of untyped nil in argument to unsafe.Sizeof", "assign"},
		// convert（含 unsafe.Pointer 转换句式：归 convert 不归 unsafe）
		{"cannot convert \"foo\" (untyped string constant) to type int", "convert"},
		{"cannot convert x (variable of type X constrained by unsafe.Pointer) to type int64: cannot convert unsafe.Pointer (in X) to type int64", "convert"},
		// unsafe
		{"embedded field type cannot be unsafe.Pointer", "unsafe"},
		{"invalid operation: too many arguments for unsafe.String(1, 2, 3) (expected 2, found 3)", "unsafe"},
		{"invalid argument: nil is not a pointer", "unsafe"},
		// builtin
		{"len (built-in) must be called", "builtin"},
		{"invalid argument: c (variable of type complex128) cannot be ordered", "builtin"},
		// ret / terminate
		{"missing return", "terminate"},
		{"too many return values", "ret"},
		// range / index / slice
		{"range over 10 (untyped int constant) permits only one iteration variable", "range"},
		{"invalid argument: index -1 (constant of type int) must not be negative", "index"},
		{"invalid slice indices: 0 < 10", "slice"},
		// cycle / import
		{"initialization cycle: A refers to itself", "cycle"},
		{"instantiation cycle:\n\tT instantiated as A[T]", "cycle"},
		{"could not import math (could not import math (unsafe-only importer))", "import"},
		// decl / stmt
		{"x redeclared in this block", "decl"},
		{"goto L jumps into block", "stmt"},
		// not-type
		{"f0() (no value) used as value", "not-type"},
		{"panic(0) (no value) used as value or type", "not-type"},
		{"cannot use _ as value or type", "not-type"},
		// parse 特例（语法树无效的 types 错误）
		{"invalid syntax tree: invalid use of ...", "parse"},
		// overflow / op-invalid
		{"256 (untyped int constant) overflows byte", "overflow"},
		{"invalid operation: division by zero", "op-invalid"},
		// 逃逸口
		{"something entirely unknown", "other"},
	}
	for _, tt := range tests {
		if got := classifyCheckMessage(tt.msg); got != tt.want {
			t.Errorf("classifyCheckMessage(%q) = %q, want %q", tt.msg, got, tt.want)
		}
	}
}

// TestC8CategoryVocabulary 把适配表与 C8 §3 的冻结枚举对账：
//  1. 文档枚举块 == c8Categories（测试内冻结副本）；
//  2. 适配表产出的类别 ⊆ 文档枚举；
//  3. 适配表**不得**使用 "other"（它是逃逸口，只能由兜底产生）。
func TestC8CategoryVocabulary(t *testing.T) {
	docPath := filepath.Join("..", "..", "docs", "contracts", "C8-check-diags.md")
	data, err := os.ReadFile(docPath)
	if err != nil {
		t.Fatalf("read %s: %v", docPath, err)
	}
	doc := string(data)
	start := strings.Index(doc, "## 3. 类别枚举")
	end := strings.Index(doc[start:], "## 4.")
	if start < 0 || end < 0 {
		t.Fatalf("C8 文档缺少 §3/§4 标题")
	}
	sec := doc[start : start+end]
	m := regexp.MustCompile("(?s)```text\n(.*?)```").FindStringSubmatch(sec)
	if m == nil {
		t.Fatalf("C8 §3 缺少 ```text 枚举块")
	}
	var docCats []string
	for _, f := range strings.Fields(m[1]) {
		docCats = append(docCats, f)
	}
	sort.Strings(docCats)
	want := append([]string(nil), c8Categories...)
	sort.Strings(want)
	if strings.Join(docCats, " ") != strings.Join(want, " ") {
		t.Fatalf("C8 §3 枚举与冻结副本不一致:\n doc  = %v\n test = %v", docCats, want)
	}

	frozen := map[string]bool{}
	for _, c := range c8Categories {
		frozen[c] = true
	}
	used := map[string]bool{}
	for _, r := range c8Rules {
		if r.Cat == "other" {
			t.Errorf("适配表不得显式使用 other（逃逸口只由兜底产生）：%s", r.RX)
		}
		if !frozen[r.Cat] {
			t.Errorf("适配表类别 %q 不在 C8 §3 枚举内", r.Cat)
		}
		used[r.Cat] = true
	}
	// 25 个真实类别都应至少有一条规则（parse 亦在表内有 "invalid syntax tree" 规则）。
	for _, c := range c8Categories {
		if c == "other" {
			continue
		}
		if !used[c] {
			t.Errorf("适配表没有任何规则使用类别 %q", c)
		}
	}
}

func TestRenderCheckDiags(t *testing.T) {
	diags := []checkDiag{
		{Line: 5, Col: 3, Cat: "undef"},
		{Line: 2, Col: 9, Cat: "parse"},
		{Line: 2, Col: 9, Cat: "assign"},
		{Line: 2, Col: 9, Cat: "assign"},
	}
	got := renderCheckDiags("testdata/x.go", 2, diags)
	want := "# source: oracle\n" +
		"# file: testdata/x.go\n" +
		"# markers: 2\n" +
		"2:9\tassign\n" +
		"2:9\tassign\n" +
		"2:9\tparse\n" +
		"5:3\tundef\n"
	if got != want {
		t.Fatalf("renderCheckDiags:\n got %q\nwant %q", got, want)
	}
}

// runCheckOnString 把一段源码写进临时 .go 文件并跑整条 `--check` 管线。
func runCheckOnString(t *testing.T, src string, testHooks bool) (string, bool, error) {
	t.Helper()
	path := filepath.Join(t.TempDir(), "p.go")
	if err := os.WriteFile(path, []byte(src), 0o644); err != nil {
		t.Fatal(err)
	}
	return runCheckMode([]string{path}, testHooks)
}

// stripHashLines 剥掉 `#` 头行（C8：两侧比较忽略 `#` 行）。
func stripHashLines(s string) string {
	var out []string
	for _, line := range strings.Split(s, "\n") {
		if strings.HasPrefix(line, "#") {
			continue
		}
		out = append(out, line)
	}
	return strings.Join(out, "\n")
}

// TestCheckModeCheapTrick 锁 go/types 的 cheap-trick 行为
// （errors.go: "invalid operand"/"invalid type" 跟随错在首个错误之后被抑制）：
// 探针对 = 同一构造「无前置错」与「有前置错」两形态，前者给出真实的 1 条主错，
// 后者证明跟随错不出现、且独立错误不被误杀。
func TestCheckModeCheapTrick(t *testing.T) {
	tests := []struct {
		name string
		src  string
		rows string // 期望的非 `#` 行（含换行）
	}{
		{
			name: "移位操作数未定义：仅主错，跟随错被抑制",
			src:  "package p\n\nvar _ = 1 << y\n",
			rows: "3:14\tundef\n",
		},
		{
			name: "二元操作数未定义：仅主错",
			src:  "package p\n\nvar _ = x + 1\n",
			rows: "3:9\tundef\n",
		},
		{
			name: "无效类型链：仅首个解析错误",
			src:  "package p\n\ntype T Undef\nvar xx T\nvar _ = 1 << xx\n",
			rows: "3:8\tundef\n",
		},
		{
			name: "两条独立错误都报（不被过度抑制）",
			src:  "package p\n\nvar _ = 1 << y\nvar _ = 1 << z\n",
			rows: "3:14\tundef\n4:14\tundef\n",
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			out, hasDiags, err := runCheckOnString(t, tt.src, false)
			if err != nil {
				t.Fatalf("runCheckMode fatal: %v", err)
			}
			if !hasDiags {
				t.Fatalf("hasDiags = false, want true\n%s", out)
			}
			if got := stripHashLines(out); got != tt.rows {
				t.Fatalf("rows:\n got %q\nwant %q\nfull output:\n%s", got, tt.rows, out)
			}
		})
	}
}

// TestCheckModeMarkersAndParseErrors：marker 计数进 `# markers:` 头；
// 解析错误以类别 parse 参与（规格 R-6-1）。
func TestCheckModeMarkersAndParseErrors(t *testing.T) {
	out, hasDiags, err := runCheckOnString(t, "package p\n\nvar _ = x // ERROR \"undefined: x\"\n", false)
	if err != nil {
		t.Fatalf("fatal: %v", err)
	}
	if !hasDiags {
		t.Fatal("hasDiags = false, want true")
	}
	if !strings.Contains(out, "# markers: 1\n") {
		t.Fatalf("missing/incorrect `# markers:` line:\n%s", out)
	}
	if got := stripHashLines(out); got != "3:9\tundef\n" {
		t.Fatalf("rows = %q, want %q", got, "3:9\tundef\n")
	}

	// 解析错误参与：`var _ = f(` ⇒ 至少一条 parse 行，且不是致命。
	out, _, err = runCheckOnString(t, "package p\n\nvar _ = f(\n", false)
	if err != nil {
		t.Fatalf("parse errors must not be fatal: %v", err)
	}
	if !strings.Contains(out, "\tparse\n") {
		t.Fatalf("parse errors must participate with category parse:\n%s", out)
	}
}

// TestCheckModeLoudFailures：装置前提被破坏时必须**响亮失败**（exit 2 类），
// 而不是静默 skip 或被静默当作"无错"。
func TestCheckModeLoudFailures(t *testing.T) {
	t.Run("shouldTest=false 不静默", func(t *testing.T) {
		_, _, err := runCheckOnString(t, "//go:build ignore\n\npackage p\n", false)
		if err == nil || !strings.Contains(err.Error(), "build constraints") {
			t.Fatalf("err = %v, want build-constraints failure", err)
		}
	})
	t.Run("fakeImportC 不支持", func(t *testing.T) {
		_, _, err := runCheckOnString(t, "// -fakeImportC\n\npackage p\n", false)
		if err == nil || !strings.Contains(err.Error(), "fakeImportC") {
			t.Fatalf("err = %v, want fakeImportC failure", err)
		}
	})
	t.Run("goexperiment 不支持", func(t *testing.T) {
		_, _, err := runCheckOnString(t, "// -goexperiment=rangefunc\n\npackage p\n", false)
		if err == nil || !strings.Contains(err.Error(), "goexperiment") {
			t.Fatalf("err = %v, want goexperiment failure", err)
		}
	})
	t.Run("读不到文件", func(t *testing.T) {
		_, _, err := runCheckMode([]string{filepath.Join(t.TempDir(), "nope.go")}, false)
		if err == nil {
			t.Fatalf("err = nil, want read failure")
		}
	})
	t.Run("首行旗标语法错误", func(t *testing.T) {
		_, _, err := runCheckOnString(t, "// -unknown=1\n\npackage p\n", false)
		if err == nil || !strings.Contains(err.Error(), "unsupported flag") {
			t.Fatalf("err = %v, want unsupported-flag failure", err)
		}
	})
}

// TestCheckModeMinFixture：C8 §8 的最小对撞目标 —— oracle 现跑（剥 `#` 行）
// 必须与冻结的 .want 逐字节一致。
func TestCheckModeMinFixture(t *testing.T) {
	const rel = "testdata/check-diags-min.go"
	wantPath := filepath.Join("..", "..", "docs", "contracts", "testdata", "check-diags-min.want")
	wantRaw, err := os.ReadFile(wantPath)
	if err != nil {
		t.Fatalf("read %s: %v", wantPath, err)
	}
	out, hasDiags, err := runCheckMode([]string{rel}, false)
	if err != nil {
		t.Fatalf("runCheckMode(%s): %v", rel, err)
	}
	if !hasDiags {
		t.Fatalf("hasDiags = false, want true")
	}
	got := stripHashLines(out)
	want := stripHashLines(string(wantRaw))
	if got != want {
		t.Fatalf("min fixture mismatch:\n got %q\nwant %q\nfull output:\n%s", got, want, out)
	}
	if !strings.Contains(out, "# file: "+rel+"\n") {
		t.Fatalf("`# file:` must echo the path as given:\n%s", out)
	}
}
