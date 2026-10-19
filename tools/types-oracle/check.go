// check.go —— `types-oracle --check`：官方 go/types/check_test.go 语义复刻 + C8 渲染。
//
// 语义清单（M4-plan Task 3；判据出处 M4-design F2 / C8-check-diags.md）：
//
//	① parseFlags 复刻：只读**第一个文件（srcs[0]）的首行** `// -flag...`；
//	   只认 `-lang=`（⇒ Config.GoVersion）；`-goexperiment=` 与 `-fakeImportC`
//	   本侧不支持 ⇒ **响亮失败**（调用方 exit 2；U-M4-04/05），绝不静默忽略。
//	② parser.AllErrors|parser.SkipObjectResolution；解析错误**参与**诊断流（类别 parse）。
//	③ importer 只认 `unsafe`，其余一律失败（错误文本含 `could not import <path>`）——
//	   把「无法解析」静默当作「无错误」是假绿之源（map0.go 是钉子）。
//	④ conf.Error 回调剔除次级消息（`: \t` 前缀是澄清行；官方同款过滤）。
//	⑤ shouldTest 构建约束复刻（官方同形）——**false ⇒ 响亮失败**（与官方的静默 skip
//	   不同：清单里混入不可测文件必须可见，不许假绿）。
//	⑥ marker 判定 = 官方 commentMap：注释剥 `//`、`/*`、`*/` 后匹配 `^ ERRORx? `
//	   （前导空格必需）；marker 位置取**前一 token**（行首注释 ⇒ (0,0)）。
//	   渲染只输出计数 `# markers: N`。
//	⑦ `--test-hooks` ⇒ types.DefPredeclaredTestFuncs()（官方 TestCheck 注入、
//	   TestSpec/TestExamples 不注入 —— 由调用方对 check/ 前缀文件传旗标）。
//	⑧ 适配表（消息 → C8 类别）：**本表是 26 类的语义字典**（rgoc 侧的码投影应对齐
//	   同一语义）；未命中 ⇒ `other` + stderr 告警行（正式守卫 = Task 4 的 T-CK-94）。
//	⑨ 渲染：`# source: oracle` / `# file: <调用方给的原样路径>` / `# markers: N` 头，
//	   数据行 `<行号>:<列>\t<类别>`；按（行号,列,类别）**稳定排序**；重复行保留。
//
// 退出码（调用方 main）：0 = 无诊断；1 = 有诊断；2 = 用法或致命错误。
package main

import (
	"bytes"
	"fmt"
	"go/ast"
	"go/build"
	"go/build/constraint"
	"go/parser"
	"go/scanner"
	"go/token"
	"go/types"
	"os"
	"regexp"
	"runtime"
	"sort"
	"strings"
)

// ═══ ① parseFlags 复刻（官方 check_test.go::parseFlags，只取 lang 面）════════

type checkFlags struct {
	Lang         string // -lang=go1.N
	GoExperiment string // -goexperiment=...（不支持 ⇒ 响亮失败）
	FakeImportC  bool   // -fakeImportC（不支持 ⇒ 响亮失败）
}

// parseCheckFlags 复刻官方 parseFlags 的**首行指令**语义：首行是 `//` 注释
// 且第一个 `-` 之前没有非空白字符时，按空白切分解析旗标。
func parseCheckFlags(src []byte) (checkFlags, error) {
	const prefix = "//"
	var flags checkFlags
	if !bytes.HasPrefix(src, []byte(prefix)) {
		return flags, nil // 首行不是行注释
	}
	s := src[len(prefix):]
	if i := bytes.Index(s, []byte("-")); i < 0 || len(bytes.TrimSpace(s[:i])) != 0 {
		return flags, nil // 注释不以 "-" 开头
	}
	end := bytes.IndexByte(s, '\n')
	const maxLen = 256
	if end < 0 || end > maxLen {
		return flags, fmt.Errorf("flags comment line too long")
	}
	for _, f := range strings.Fields(string(s[:end])) {
		if !strings.HasPrefix(f, "-") {
			break // 官方 flag.Parse：遇首个非旗标字段即停止
		}
		switch {
		case strings.HasPrefix(f, "-lang="):
			flags.Lang = strings.TrimPrefix(f, "-lang=")
		case strings.HasPrefix(f, "-goexperiment="):
			flags.GoExperiment = strings.TrimPrefix(f, "-goexperiment=")
		case f == "-fakeImportC" || f == "-fakeImportC=true":
			flags.FakeImportC = true
		case f == "-fakeImportC=false":
			// 官方 BoolVar 允许显式 false；等价于未设。
		default:
			return flags, fmt.Errorf("unsupported flag %q in flags comment", f)
		}
	}
	return flags, nil
}

// ═══ ⑤ shouldTest 复刻（官方 check_test.go::shouldTest）══════════════════════

// shouldTest 扫到 `package ` 行为止，首个可被 constraint.Parse 解析的行按
// GOOS/GOARCH/go 版本标签求值（其余标签一律不匹配）。
func shouldTest(src []byte) bool {
	match := func(tag string) bool {
		for _, rt := range build.Default.ReleaseTags {
			if rt == tag {
				return true
			}
		}
		return tag == runtime.GOOS || tag == runtime.GOARCH
	}
	for line := range strings.SplitSeq(string(src), "\n") {
		if strings.HasPrefix(line, "package ") {
			break
		}
		if expr, err := constraint.Parse(line); err == nil {
			return expr.Eval(match)
		}
	}
	return true
}

// ═══ ⑥ marker 计数（官方 commentMap 同形）════════════════════════════════════

// markerRX 官方 comment_test.go 的匹配：注释去标记后以 ` ERROR ` / ` ERRORx ` 开头。
var markerRX = regexp.MustCompile(`^ ERRORx? `)

type checkComment struct {
	Line, Col int
	Text      string
}

// commentMap 复刻官方 commentMap：marker 位置 = **前一 token**
// （非注释、非自动分号；注释在任何 token 之前 ⇒ (0,0)）。
func commentMap(src []byte) map[int][]checkComment {
	fset := token.NewFileSet()
	file := fset.AddFile("", -1, len(src))

	var s scanner.Scanner
	s.Init(file, src, nil, scanner.ScanComments)
	var prev token.Pos
	var res map[int][]checkComment

	for {
		pos, tok, lit := s.Scan()
		switch tok {
		case token.EOF:
			return res
		case token.COMMENT:
			if len(lit) >= 2 && lit[1] == '*' {
				lit = lit[:len(lit)-2] // 去尾部 */
			}
			lit = lit[2:] // 去 // 或 /*
			if markerRX.MatchString(lit) {
				p := fset.Position(prev)
				if res == nil {
					res = make(map[int][]checkComment)
				}
				res[p.Line] = append(res[p.Line], checkComment{Line: p.Line, Col: p.Column, Text: lit})
			}
		case token.SEMICOLON:
			// 忽略自动插入的分号
			if lit == "\n" {
				continue
			}
			fallthrough
		default:
			prev = pos
		}
	}
}

func countMarkers(src []byte) int {
	n := 0
	for _, cs := range commentMap(src) {
		n += len(cs)
	}
	return n
}

// ═══ ⑧ 适配表：消息 → C8 类别（顺序 = 先到先得；Python 侧已用全线普查验证）════

type c8Rule struct {
	Cat string
	RX  *regexp.Regexp
}

// c8Rules 是 26 类（C8 §3）的**语义字典**。规则顺序即优先级；改动本表
// 必须全语料重跑（T-CK-94 守卫），并同步 check_test.go 的向量与 C8 对账。
var c8Rules = []c8Rule{
	// version 最高优先（可叠加在别的句式上：signed shift count / cannot range / cannot convert ...）
	{"version", regexp.MustCompile(`requires go1\.`)},
	// unused
	{"unused", regexp.MustCompile(`not used`)},
	// undef
	{"undef", regexp.MustCompile(`^undefined: `)},
	{"undef", regexp.MustCompile(`\bnot declared\b`)},
	// selector 族
	{"selector", regexp.MustCompile(`undefined \(type `)},
	{"selector", regexp.MustCompile(`^ambiguous selector `)},
	{"selector", regexp.MustCompile(`^unknown field `)},
	{"selector", regexp.MustCompile(`^operand for field selector `)},
	{"selector", regexp.MustCompile(`^cannot specify (promoted|embedded) field `)},
	{"selector", regexp.MustCompile(`^invalid method expression `)},
	{"selector", regexp.MustCompile(`^cannot call pointer method `)},
	{"selector", regexp.MustCompile(`^invalid field name `)},
	{"selector", regexp.MustCompile(`^invalid implicit pointer indirection `)},
	// shift（version 已在上方截获 "signed shift count ..."）
	{"shift", regexp.MustCompile(`shift count|shifted operand|constant shift overflow`)},
	// type-arg 族（须在 assign 泛规则前：as constraint / without instantiation 等）
	{"type-arg", regexp.MustCompile(`cannot infer|inferred type`)},
	{"type-arg", regexp.MustCompile(`is not a generic type`)},
	{"type-arg", regexp.MustCompile(`without instantiation`)},
	{"type-arg", regexp.MustCompile(`type arguments? (for|but)|got .* type arguments`)},
	{"type-arg", regexp.MustCompile(`type parameter value`)},
	{"type-arg", regexp.MustCompile(`as constraint`)},
	{"type-arg", regexp.MustCompile(`cannot be a type parameter`)},
	{"type-arg", regexp.MustCompile(`as RHS in type declaration`)},
	// comma-ok 族
	{"comma-ok", regexp.MustCompile(`single-value context`)},
	{"comma-ok", regexp.MustCompile(`^impossible type (assertion|switch)`)},
	{"comma-ok", regexp.MustCompile(`\d+-valued`)},
	{"comma-ok", regexp.MustCompile(`outside type switch`)},
	{"comma-ok", regexp.MustCompile(`non-variadic`)},
	// implements 族
	{"implements", regexp.MustCompile(`does not satisfy|^cannot satisfy `)},
	{"implements", regexp.MustCompile(`outside a type constraint`)},
	{"implements", regexp.MustCompile(`^invalid use of ~`)},
	{"implements", regexp.MustCompile(`overlapping terms|in union|contains methods`)},
	{"implements", regexp.MustCompile(`^cannot use ~ outside`)},
	{"implements", regexp.MustCompile(`^(invalid map key type|.* missing in ~)`)},
	{"implements", regexp.MustCompile(`more than 100 union terms`)},
	// 先判族（否则会被 unsafe / assign / builtin 泛规则误吸）
	{"decl", regexp.MustCompile(`is not constant$`)},
	{"assign", regexp.MustCompile(`^use of untyped nil in `)},
	{"not-type", regexp.MustCompile(`^cannot use _ as value or type`)}, // 须在泛 "cannot use ... as" 之前
	// assign 泛族（在 convert/unsafe 前；"cannot use ... as ... (overflows)/(truncated)" 归本族）
	{"assign", regexp.MustCompile(`^cannot use .+ as .+`)},
	{"assign", regexp.MustCompile(`^cannot assign to `)},
	{"assign", regexp.MustCompile(`^assignment mismatch`)},
	{"assign", regexp.MustCompile(`arguments in call to`)},
	{"assign", regexp.MustCompile(`too many values in struct literal|too few values in struct literal`)},
	// convert（在 unsafe 前：cannot convert ... unsafe.Pointer 属 convert 句式）
	{"convert", regexp.MustCompile(`^cannot convert `)},
	{"convert", regexp.MustCompile(`in conversion to|truncated to`)},
	// unsafe 族
	{"unsafe", regexp.MustCompile(`unsafe\.Pointer|use of package unsafe`)},
	{"unsafe", regexp.MustCompile(`is not a selector expression|is a method value`)},
	{"unsafe", regexp.MustCompile(`embedded via a pointer`)},
	{"unsafe", regexp.MustCompile(`is not a pointer$|is not a slice$`)},
	{"unsafe", regexp.MustCompile(`unsafe\.`)},
	// builtin 族
	{"builtin", regexp.MustCompile(`\(built-in\) must be called`)},
	{"builtin", regexp.MustCompile(`invalid use of \.\.\. with built-in`)},
	{"builtin", regexp.MustCompile(`(not enough|too many) arguments for `)},
	{"builtin", regexp.MustCompile(`expects \d+ or \d+ arguments`)},
	{"builtin", regexp.MustCompile(`for built-in (len|cap)`)},
	{"builtin", regexp.MustCompile(`^invalid (copy|append): `)},
	{"builtin", regexp.MustCompile(`is not a boolean constant`)},
	{"builtin", regexp.MustCompile(`is not a map$|is not a map\b`)},
	{"builtin", regexp.MustCompile(`must have identical key types`)},
	{"builtin", regexp.MustCompile(`cannot make |cannot clear `)},
	{"builtin", regexp.MustCompile(`arguments? have type|argument has type`)},
	{"builtin", regexp.MustCompile(`cannot be ordered`)},
	{"builtin", regexp.MustCompile(`^assert\(false\) failed$`)},
	{"builtin", regexp.MustCompile(`length and capacity swapped|length .* must not be negative`)},
	{"builtin", regexp.MustCompile(`\(previous argument\)`)},
	// ret / terminate
	{"terminate", regexp.MustCompile(`^missing return$`)},
	{"ret", regexp.MustCompile(`return values|not in scope at return`)},
	// range
	{"range", regexp.MustCompile(`^cannot range over |^range over `)},
	{"range", regexp.MustCompile(`iteration variable`)},
	// index
	{"index", regexp.MustCompile(`^cannot index `)},
	{"index", regexp.MustCompile(`(index .* out of bounds|index .* must not be negative|index .* must be integer|^index .* must be integer constant)`)},
	// slice
	{"slice", regexp.MustCompile(`^cannot slice |invalid slice indices|3-index slice`)},
	// cycle
	{"cycle", regexp.MustCompile(`^invalid recursive type|^invalid cycle in |^initialization cycle|^instantiation cycle`)},
	// import
	{"import", regexp.MustCompile(`^could not import `)},
	// decl 泛族
	{"decl", regexp.MustCompile(`redeclared|already declared`)},
	{"decl", regexp.MustCompile(`^duplicate (method|key|index|field name) `)},
	{"decl", regexp.MustCompile(`methods must have a unique|cannot define new methods|^invalid receiver type |receiver type parameter`)},
	{"decl", regexp.MustCompile(`^invalid package name `)},
	{"decl", regexp.MustCompile(`func (main|init) must|cannot declare (main|init) `)},
	{"decl", regexp.MustCompile(`^(missing|extra) init expr`)},
	{"decl", regexp.MustCompile(`no new variables? on left side|repeated on left side`)},
	{"decl", regexp.MustCompile(`^(invalid|undefined) array length|^array length `)},
	{"decl", regexp.MustCompile(`^cannot use iota outside`)},
	{"decl", regexp.MustCompile(`^invalid constant type |is not constant$|^iota outside constant declaration$`)},
	{"decl", regexp.MustCompile(`^invalid composite literal type |missing type in composite literal|mixture of field|missing key in map literal`)},
	{"decl", regexp.MustCompile(`^invalid use of \[\.\.\.\] array`)},
	{"decl", regexp.MustCompile(`embedded field type cannot be `)},
	{"decl", regexp.MustCompile(`generic function is missing function body`)},
	// stmt 族
	{"stmt", regexp.MustCompile(`^goto `)},
	{"stmt", regexp.MustCompile(`^invalid (break|continue) label `)},
	{"stmt", regexp.MustCompile(`fallthrough`)},
	{"stmt", regexp.MustCompile(`^(break|continue) not in `)},
	{"stmt", regexp.MustCompile(`^select case must be `)},
	{"stmt", regexp.MustCompile(`^multiple defaults`)},
	{"stmt", regexp.MustCompile(`^non-boolean condition|^missing condition`)},
	{"stmt", regexp.MustCompile(`^cannot declare in post statement$`)},
	{"stmt", regexp.MustCompile(`^invalid case |^duplicate case `)},
	{"stmt", regexp.MustCompile(`^(go|defer) (requires|discards) `)},
	// not-type 泛族
	{"not-type", regexp.MustCompile(`is not a type|is not an expression`)},
	{"not-type", regexp.MustCompile(`used as value`)},
	// parse 特例（语法树无效的 types 错误；解析错误本身由**来源**判定）
	{"parse", regexp.MustCompile(`^invalid syntax tree`)},
	// overflow
	{"overflow", regexp.MustCompile(`overflows?|overflow`)},
	// op-invalid（最泛，放最后）
	{"op-invalid", regexp.MustCompile(`^invalid operation: `)},
	{"op-invalid", regexp.MustCompile(`is not an interface`)},
}

// classifyCheckMessage 返回消息的 C8 类别；未命中 ⇒ "other"（逃逸口）。
// 只取首行：多行消息的后续行是位置注解，不参与类别判定。
func classifyCheckMessage(msg string) string {
	if i := strings.IndexByte(msg, '\n'); i >= 0 {
		msg = msg[:i]
	}
	for _, r := range c8Rules {
		if r.RX.MatchString(msg) {
			return r.Cat
		}
	}
	return "other"
}

// ═══ ③ importer：只认 unsafe ══════════════════════════════════════════════════

type unsafeOnlyImporter struct{}

func (unsafeOnlyImporter) Import(path string) (*types.Package, error) {
	if path == "unsafe" {
		return types.Unsafe, nil
	}
	return nil, fmt.Errorf("could not import %s (unsafe-only importer)", path)
}

// ═══ ⑨ 渲染 ═══════════════════════════════════════════════════════════════════

type checkDiag struct {
	Line, Col int
	Cat       string
	Msg       string // 原始消息（人读/告警用；不进渲染）
}

// renderCheckDiags 按（行号,列,类别）稳定排序后渲染；重复行保留。
func renderCheckDiags(file string, markers int, diags []checkDiag) string {
	sorted := make([]checkDiag, len(diags))
	copy(sorted, diags)
	sort.SliceStable(sorted, func(i, j int) bool {
		a, b := sorted[i], sorted[j]
		if a.Line != b.Line {
			return a.Line < b.Line
		}
		if a.Col != b.Col {
			return a.Col < b.Col
		}
		return a.Cat < b.Cat
	})
	var b strings.Builder
	b.WriteString("# source: oracle\n")
	b.WriteString("# file: " + file + "\n")
	fmt.Fprintf(&b, "# markers: %d\n", markers)
	for _, d := range sorted {
		fmt.Fprintf(&b, "%d:%d\t%s\n", d.Line, d.Col, d.Cat)
	}
	return b.String()
}

// ═══ 端到端：`--check` 主入口 ═════════════════════════════════════════════════

// runCheckMode 把 files 当作同一包检查并渲染。
// 返回：渲染文本 / 是否有诊断 / 致命错（调用方 exit 2）。
func runCheckMode(files []string, testHooks bool) (string, bool, error) {
	srcs := make([][]byte, len(files))
	for i, f := range files {
		src, err := os.ReadFile(f)
		if err != nil {
			return "", false, err
		}
		srcs[i] = src
	}

	// ① 官方只读**第一个文件**的首行指令。
	flags, err := parseCheckFlags(srcs[0])
	if err != nil {
		return "", false, fmt.Errorf("%s: %v", files[0], err)
	}
	if flags.GoExperiment != "" {
		return "", false, fmt.Errorf("%s: flags comment requests -goexperiment=%q; unsupported by this oracle (U-M4-04)", files[0], flags.GoExperiment)
	}
	if flags.FakeImportC {
		return "", false, fmt.Errorf("%s: flags comment requests -fakeImportC; unsupported by this oracle (U-M4-05)", files[0])
	}

	// ⑦ 官方 TestCheck 才注入 assert/trace（调用方对 check/ 前缀文件传 --test-hooks）。
	if testHooks {
		types.DefPredeclaredTestFuncs()
	}

	fset := token.NewFileSet()
	astFiles := make([]*ast.File, 0, len(files))
	var errs []error
	for i, f := range files {
		// ⑤ 清单里混入不可测文件 ⇒ 响亮失败，不静默 skip。
		if !shouldTest(srcs[i]) {
			return "", false, fmt.Errorf("%s: build constraints exclude this file (shouldTest=false); refusing to check silently (exit 2)", f)
		}
		// ② AllErrors|SkipObjectResolution；解析错误参与。
		af, perr := parser.ParseFile(fset, f, srcs[i], parser.ParseComments|parser.AllErrors|parser.SkipObjectResolution)
		if af == nil {
			return "", false, fmt.Errorf("%s: %v", f, perr)
		}
		astFiles = append(astFiles, af)
		if perr != nil {
			if list, ok := perr.(scanner.ErrorList); ok && len(list) > 0 {
				for _, e := range list {
					errs = append(errs, e)
				}
			} else {
				errs = append(errs, perr)
			}
		}
	}

	conf := types.Config{Importer: unsafeOnlyImporter{}} // ③
	conf.GoVersion = flags.Lang
	conf.Error = func(e error) {
		// ④ 次级消息（": \t" 澄清行）剔除——官方同款过滤。
		if strings.Contains(e.Error(), ": \t") {
			return
		}
		errs = append(errs, e)
	}
	// go/types 对畸形输入可能 panic（设计期探针见过）：不许把该死法当"无错"。
	panicErr := func() (perr error) {
		defer func() {
			if r := recover(); r != nil {
				perr = fmt.Errorf("%s: go/types panic: %v", files[0], r)
			}
		}()
		conf.Check(astFiles[0].Name.Name, fset, astFiles, nil)
		return nil
	}()
	if panicErr != nil {
		return "", false, panicErr
	}

	markers := 0
	for _, src := range srcs {
		markers += countMarkers(src)
	}

	diags := make([]checkDiag, 0, len(errs))
	for _, e := range errs {
		switch x := e.(type) {
		case *scanner.Error: // 解析错误（ErrorList 的元素是指针）
			diags = append(diags, checkDiag{Line: x.Pos.Line, Col: x.Pos.Column, Cat: "parse", Msg: x.Msg})
		case scanner.Error:
			diags = append(diags, checkDiag{Line: x.Pos.Line, Col: x.Pos.Column, Cat: "parse", Msg: x.Msg})
		case types.Error:
			p := fset.Position(x.Pos)
			cat := classifyCheckMessage(x.Msg)
			if cat == "other" {
				msg := x.Msg
				if i := strings.IndexByte(msg, '\n'); i >= 0 {
					msg = msg[:i]
				}
				fmt.Fprintf(os.Stderr, "warning: uncategorized check message -> other: %s\n", msg)
			}
			diags = append(diags, checkDiag{Line: p.Line, Col: p.Column, Cat: cat, Msg: x.Msg})
		default:
			return "", false, fmt.Errorf("unexpected error type %T: %v", e, e)
		}
	}

	out := renderCheckDiags(strings.Join(files, " "), markers, diags)
	return out, len(diags) > 0, nil
}
