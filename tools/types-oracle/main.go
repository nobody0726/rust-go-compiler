// types oracle —— Go 侧「标准答案的提供者」（M3 阶段，D-M3-10）。
//
// 用**标准库** go/parser（带注释）+ go/types 的 Config.Check 检查 fixture，
// 按 C7 契约（docs/contracts/C7-type-dump.md）输出**中性类型 dump**。
// 与 rgoc 的 rgoc-types/src/dump.rs 逐字节可 diff —— 这是 M3 oracle 对撞
// （T-TY-91）的另一半。
//
// 为什么用标准库 go/types 而不是 vendor 官方 internal/types2：
//
//	types2 是编译器内部包，import 它要把整个 cmd/compile 内部树搬进来
//	（ast-oracle 做了这件事，代价是 ~200 个文件的原样副本）。M3 判的是
//	**语言层面**的类型同一性与可表示性，go/types 与 types2 在这些语义上
//	一致，而「标准库」让本工具不需要 vendor（D-M3-10）。
//
// 用法：
//
//	go run . <file.go>... [--lang go1.N]
//
// 退出码：0 = 检查无错；1 = 有类型错误（dump 照常输出）；2 = 用法/致命错误。
//
// 注意（M1/M2 实测的坑）：必须 cd 进本目录再 go run .（自带 go.mod）；
// 捕获输出要 RGOC_TTY=0，否则伪终端把 \n 翻成 \r\n；
// 本目录下不要放样本 .go（同目录的 .go 会被一起编译 —— fixture 放 testdata/）。
package main

import (
	"fmt"
	"go/ast"
	"go/parser"
	"go/token"
	"go/types"
	"os"
	"sort"
	"strconv"
	"strings"
)

func main() {
	lang := ""
	var files []string
	args := os.Args[1:]
	for i := 0; i < len(args); i++ {
		switch {
		case args[i] == "--lang":
			if i+1 >= len(args) {
				usage(os.Stderr)
			}
			i++
			lang = args[i]
		case strings.HasPrefix(args[i], "--"):
			fmt.Fprintf(os.Stderr, "unknown flag: %s\n", args[i])
			os.Exit(2)
		default:
			files = append(files, args[i])
		}
	}
	if len(files) == 0 {
		usage(os.Stderr)
	}

	fset := token.NewFileSet()
	var astFiles []*ast.File
	for _, f := range files {
		af, err := parser.ParseFile(fset, f, nil, parser.ParseComments)
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		astFiles = append(astFiles, af)
	}

	info := &types.Info{Types: map[ast.Expr]types.TypeAndValue{}, Defs: map[*ast.Ident]types.Object{}}
	conf := types.Config{}
	if lang != "" {
		conf.GoVersion = lang
	}
	var nerr int
	conf.Error = func(err error) {
		nerr++
		fmt.Fprintln(os.Stderr, err)
	}
	_, _ = conf.Check(astFiles[0].Name.Name, fset, astFiles, info)

	os.Stdout.WriteString(dumpAll(astFiles, fset, info))
	if nerr > 0 {
		os.Exit(1)
	}
}

func usage(w *os.File) {
	fmt.Fprintln(w, "usage: go run . <file.go>... [--lang go1.N]")
	os.Exit(2)
}

// ═══ C7 §3：编号 ═══════════════════════════════════════════════════════════

type dumper struct {
	idx   map[types.Type]int
	order []types.Type
	info  *types.Info
}

func dumpAll(files []*ast.File, fset *token.FileSet, info *types.Info) string {
	d := &dumper{idx: map[types.Type]int{}, info: info}
	for _, f := range files {
		for _, root := range d.roots(f) {
			d.number(root)
		}
	}
	return d.render()
}

// roots 按 C7 §3 列出「根」类型（顶层声明的类型表达式，源码序）。
//
// `type D <T>` 的根是 **D 本身**（定义），它的本体由 `number` 顺带下钻
// —— 因此 D 的号一定排在本体之前，且递归类型（`type N struct{ p *N }`）会终止。
func (d *dumper) roots(f *ast.File) []types.Type {
	var out []types.Type
	for _, decl := range f.Decls {
		switch x := decl.(type) {
		case *ast.GenDecl:
			switch x.Tok {
			case token.TYPE:
				for _, s := range x.Specs {
					if ts, ok := s.(*ast.TypeSpec); ok {
						out = append(out, d.defType(ts.Name))
					}
				}
			case token.VAR, token.CONST:
				for _, s := range x.Specs {
					if vs, ok := s.(*ast.ValueSpec); ok && vs.Type != nil {
						out = append(out, d.typOf(vs.Type))
					}
				}
			}
		case *ast.FuncDecl:
			// ⚠️ 顶层 func 的**签名表达式不在 `info.Types` 里**（它只记表达式；
			// 函数签名记在 `Defs[名字]` 上）—— 用 `defType` 取，别用 `typOf`。
			out = append(out, d.defType(x.Name))
		}
	}
	return out
}

// intern 登记一个条目并返回它的号（**不**下钻）。
func (d *dumper) intern(t types.Type) int {
	if i, ok := d.idx[t]; ok {
		return i
	}
	i := len(d.order)
	d.idx[t] = i
	d.order = append(d.order, t)
	return i
}

// number 按 C7 §3 给一个类型编号。
//
// 两条规则：
//  1. **去重**：首次出现定号，再次遇到返回同一个号；
//  2. **后序**：结构型先子后父；**定义型（`Named`/`Alias`）先占号再下钻本体**
//     —— 占号在前是**递归类型终止**的唯一手段，同时让「引用一个未在本 fixture
//     里声明的定义」（如 universe 的 `error`）也能把它的本体拉进来编号。
func (d *dumper) number(t types.Type) int {
	if t == nil {
		return -1
	}
	if i, ok := d.idx[t]; ok {
		return i
	}
	// 定义型：先占号，再下钻本体（顺序与「后序」相反，这是刻意的）
	switch x := t.(type) {
	case *types.Named:
		i := d.intern(t)
		d.number(x.Underlying())
		return i
	case *types.Alias:
		i := d.intern(t)
		d.number(x.Rhs())
		return i
	}
	// 其余：先子后父
	switch x := t.(type) {
	case *types.Pointer:
		d.number(x.Elem())
	case *types.Slice:
		d.number(x.Elem())
	case *types.Array:
		d.number(x.Elem())
	case *types.Map:
		d.number(x.Key())
		d.number(x.Elem())
	case *types.Chan:
		d.number(x.Elem())
	case *types.Struct:
		for i := 0; i < x.NumFields(); i++ {
			d.number(x.Field(i).Type())
		}
	case *types.Tuple:
		for i := 0; i < x.Len(); i++ {
			d.number(x.At(i).Type())
		}
	case *types.Signature:
		if r := x.Recv(); r != nil {
			d.number(r.Type())
		}
		d.number(x.Params())
		d.number(x.Results())
	case *types.Interface:
		for i := 0; i < x.NumMethods(); i++ {
			d.number(x.Method(i).Type())
		}
		if u := unionOf(x); u != nil {
			d.number(u)
		}
	case *types.TypeParam:
		d.number(x.Constraint())
	case *types.Union:
		for i := 0; i < x.Len(); i++ {
			d.number(x.Term(i).Type())
		}
	}
	return d.intern(t)
}

// ref 渲染一次类型引用（C7 §2.2）。
//
// ⚠️ 调用方保证被引用者**已经编过号**。没编号说明 `number` 漏了一条下钻路径，
// 那属于本工具自身的 bug，必须响亮地炸 —— 默默补一个号会改变后续所有编号，
// 两侧从此对不上且极难查。
func (d *dumper) ref(t types.Type) string {
	i, ok := d.idx[t]
	if !ok {
		panic(fmt.Sprintf("C7 §3 违反：类型 %T %v 未编号就被引用", t, t))
	}
	if isDefinition(t) {
		return "#" + strconv.Itoa(i)
	}
	return strconv.Itoa(i)
}

// isDefinition：C7 §2.2 里要写 `#<序号>` 的两类。
func isDefinition(t types.Type) bool {
	switch t.(type) {
	case *types.Named, *types.Alias:
		return true
	}
	return false
}

// typOf 取一个类型表达式**所指代的类型**。
func (d *dumper) typOf(e ast.Expr) types.Type {
	if tv, ok := d.info.Types[e]; ok && tv.Type != nil {
		return tv.Type
	}
	return types.Typ[types.Invalid]
}

func (d *dumper) defType(name *ast.Ident) types.Type {
	if o := d.info.Defs[name]; o != nil && o.Type() != nil {
		return o.Type()
	}
	return types.Typ[types.Invalid]
}

// unionOf 取接口的**项集**（C7 §4.2 的 `term` 行）；没有限制项时返回 nil。
//
// 官方 types2 的 `Interface.tset *termList` 是本函数的对应物：
//   - 单个 `*types.Union` 嵌入 ⇒ 直接用它的项；
//   - 若干个**裸类型**嵌入（`interface{ int }`）⇒ 合成一个 union（每项 exact）；
//   - 没有嵌入 ⇒ 通用集，返回 nil（dump 里就没有 `term` 行）。
func unionOf(iface *types.Interface) *types.Union {
	n := iface.NumEmbeddeds()
	if n == 0 {
		return nil
	}
	if n == 1 {
		if u, ok := iface.EmbeddedType(0).(*types.Union); ok {
			return u
		}
	}
	var terms []*types.Term
	for i := 0; i < n; i++ {
		t := iface.EmbeddedType(i)
		if u, ok := t.(*types.Union); ok {
			for j := 0; j < u.Len(); j++ {
				terms = append(terms, u.Term(j))
			}
			continue
		}
		terms = append(terms, types.NewTerm(false, t))
	}
	return types.NewUnion(terms)
}

// ═══ C7 §4：渲染 ═══════════════════════════════════════════════════════════

func (d *dumper) render() string {
	var b strings.Builder
	b.WriteString("# source: oracle\n")
	for i, t := range d.order {
		fmt.Fprintf(&b, "%d\t%s\n", i, d.line(t))
	}
	return b.String()
}

// line 渲染一个条目的**首行**（嵌套项另起行、2 空格缩进）。
func (d *dumper) line(t types.Type) string {
	switch x := t.(type) {
	case *types.Basic:
		if x.Kind() == types.Invalid {
			return "invalid"
		}
		return "basic " + basicName(x.Kind())

	case *types.Named:
		return "named " + d.ref(x.Underlying())

	case *types.Alias:
		return "alias " + d.ref(x.Rhs())

	case *types.Pointer:
		return "pointer " + d.ref(x.Elem())

	case *types.Slice:
		return "slice " + d.ref(x.Elem())

	case *types.Array:
		return "array " + lenText(x.Len()) + " " + d.ref(x.Elem())

	case *types.Map:
		return "map " + d.ref(x.Key()) + " " + d.ref(x.Elem())

	case *types.Chan:
		return "chan " + chanDir(x.Dir()) + " " + d.ref(x.Elem())

	case *types.Struct:
		var b strings.Builder
		fmt.Fprintf(&b, "struct %d", x.NumFields())
		for i := 0; i < x.NumFields(); i++ {
			f := x.Field(i)
			name := "-"
			emb := "false"
			if f.Embedded() {
				emb = "true"
			} else {
				name = rustDebugQuote(f.Name())
			}
			fmt.Fprintf(&b, "\n  %s %s %s %s", name, d.ref(f.Type()), tagOrDash(x.Tag(i)), emb)
		}
		return b.String()

	case *types.Tuple:
		var b strings.Builder
		fmt.Fprintf(&b, "tuple %d", x.Len())
		for i := 0; i < x.Len(); i++ {
			v := x.At(i)
			fmt.Fprintf(&b, "\n  %s %s", nameOrDash(v.Name()), d.ref(v.Type()))
		}
		return b.String()

	case *types.Signature:
		recv := "-"
		if r := x.Recv(); r != nil {
			recv = d.ref(r.Type())
		}
		variadic := "false"
		if x.Variadic() {
			variadic = "true"
		}
		return "signature " + recv + " " + d.ref(x.Params()) + " " + d.ref(x.Results()) + " " + variadic

	case *types.Interface:
		// 方法**按名字升序**（C7 §4.2），保证两侧输出确定
		ms := make([]*types.Func, 0, x.NumMethods())
		for i := 0; i < x.NumMethods(); i++ {
			ms = append(ms, x.Method(i))
		}
		sort.Slice(ms, func(i, j int) bool { return ms[i].Name() < ms[j].Name() })
		u := unionOf(x)
		hasTerm := "0"
		if u != nil {
			hasTerm = "1"
		}
		var b strings.Builder
		fmt.Fprintf(&b, "interface %d %s", len(ms), hasTerm)
		for _, m := range ms {
			fmt.Fprintf(&b, "\n  method %s %s", rustDebugQuote(m.Name()), d.ref(m.Type()))
		}
		if u != nil {
			fmt.Fprintf(&b, "\n  term %s", d.ref(u))
		}
		return b.String()

	case *types.TypeParam:
		return fmt.Sprintf("typeparam %d %s", x.Index(), d.ref(x.Constraint()))

	case *types.Union:
		var b strings.Builder
		fmt.Fprintf(&b, "union %d", x.Len())
		for i := 0; i < x.Len(); i++ {
			term := x.Term(i)
			kind := "exact"
			if term.Tilde() {
				kind = "tilde"
			}
			fmt.Fprintf(&b, "\n  term %s %s", kind, d.ref(term.Type()))
		}
		return b.String()
	}
	panic(fmt.Sprintf("C7：没有覆盖的类型 %T %v", t, t))
}

func chanDir(d types.ChanDir) string {
	switch d {
	case types.SendRecv:
		return "sendrecv"
	case types.SendOnly:
		return "sendonly"
	case types.RecvOnly:
		return "recvonly"
	}
	return "sendrecv"
}

func nameOrDash(s string) string {
	if s == "" {
		return "-"
	}
	return rustDebugQuote(s)
}

func tagOrDash(s string) string {
	if s == "" {
		return "-"
	}
	return rustDebugQuote(s)
}

// lenText：C7 §4.3 —— 十进制、无前导 0、无 `+`、无下划线；长度未定（`[...]T`）写 `-`。
//
// `go/types` 把数组长度直接给成 `int64`（未定的 `[...]T` 是 `-1`），
// 所以这里不需要经 `constant.Value`。
func lenText(n int64) string {
	if n < 0 {
		return "-"
	}
	return strconv.FormatInt(n, 10)
}

// basicName：C7 §4.1 的 25 个 basic token（`Invalid` 走 `invalid` 标签，不在这里）。
//
// ⚠️ **`byte` 与 `rune` 不在这里**：`go/types` 里 `Byte = Uint8`、`Rune = Int32`
// 是**同一个 `*Basic`**（同一指针），所以 `byte` dump 成 `basic uint8`、
// `rune` dump 成 `basic int32` —— 这是 `C7` §4.1 早期版本写错、由本 oracle
// 实测订正的一处（M3/Task 3）。
func basicName(k types.BasicKind) string {
	switch k {
	case types.Bool:
		return "bool"
	case types.Int:
		return "int"
	case types.Int8:
		return "int8"
	case types.Int16:
		return "int16"
	case types.Int32:
		return "int32"
	case types.Int64:
		return "int64"
	case types.Uint:
		return "uint"
	case types.Uint8:
		return "uint8"
	case types.Uint16:
		return "uint16"
	case types.Uint32:
		return "uint32"
	case types.Uint64:
		return "uint64"
	case types.Uintptr:
		return "uintptr"
	case types.Float32:
		return "float32"
	case types.Float64:
		return "float64"
	case types.Complex64:
		return "complex64"
	case types.Complex128:
		return "complex128"
	case types.String:
		return "string"
	case types.UnsafePointer:
		return "unsafe_pointer"
	case types.UntypedBool:
		return "untyped_bool"
	case types.UntypedInt:
		return "untyped_int"
	case types.UntypedRune:
		return "untyped_rune"
	case types.UntypedFloat:
		return "untyped_float"
	case types.UntypedComplex:
		return "untyped_complex"
	case types.UntypedString:
		return "untyped_string"
	case types.UntypedNil:
		return "untyped_nil"
	}
	panic(fmt.Sprintf("C7 §4.1：未映射的 BasicKind %d（%v）—— 契约与 go/types 的口径不一致", int(k), k))
}

// rustDebugQuote 用 Rust `{:?}`（str 的 Debug）的语义给字符串加引号。
//
// 与 tools/ast-oracle/dump.go 的同名函数**同形**（C6 §2.4 的转义语义被 C7 §2.3 沿用）。
func rustDebugQuote(s string) string {
	var b strings.Builder
	b.WriteByte('"')
	for _, r := range s {
		switch r {
		case '"':
			b.WriteString("\\\"")
		case '\\':
			b.WriteString("\\\\")
		case '\n':
			b.WriteString("\\n")
		case '\r':
			b.WriteString("\\r")
		case '\t':
			b.WriteString("\\t")
		case 0:
			b.WriteString("\\0")
		default:
			if r < 0x20 || r == 0x7f {
				fmt.Fprintf(&b, "\\u{%x}", r)
			} else {
				b.WriteRune(r)
			}
		}
	}
	b.WriteByte('"')
	return b.String()
}
