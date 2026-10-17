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
//	go run . <file.go>... [--lang go1.N]     ← C7 dump（stdout）
//
// 退出码：0 = 检查无错；1 = 有类型错误（dump 照常输出）；2 = 用法/致命错误。
//
// 输出以 `#` 开头的注释行开头（`# source: oracle` / `# files:` / `# package:`）：
// C7 §2 规则 1 说两侧都忽略 `#` 行，所以它们**不进对撞**，只为失败时能一眼看出
// 是哪一侧、哪个文件；`M3-tests.md` §5.3 要求来源必须如实标注。
//
// # 三个曾经崩过的形状（都有回归钉）
//
// Task 31 修掉的两处根因（探针实测，见 dump_test.go 的
// TestConstraintShapesDoNotPanic 与 git 历史）：
//
//   - `unionOf` 必须按接口指针**记忆**：`types.NewUnion` 每次调用都是**新指针**，
//     不记忆会让「编号的 Union」与「渲染的 Union」不是同一个对象 ⇒
//     报 `C7 §3 违反：类型 *types.Union 未编号就被引用`；
//   - 接口方法的签名在 go/types 里**带接收者**（`Signature.Recv()` = 接口自身），
//     下钻它会造出真环（匿名约束接口的接收者就是接口自己）⇒ 必须跳过并渲染 `-`
//     （C7 §4.2）。记忆表 `ifaceSigs` 就是为此。
//
// 另一处是**表示差异**而不是崩：`byte` / `uint8`（`rune` / `int32`）在 go/types 里
// 是**两个不同的 `*types.Basic` 对象**（Kind 相同）—— `canon` 把它们折到同一个
// 条目，否则两侧编号从那一行起整体错位。
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

	dump, errs, fatal := checkFiles(lang, files)
	if fatal != nil {
		fmt.Fprintln(os.Stderr, fatal)
		os.Exit(1)
	}
	for _, e := range errs {
		fmt.Fprintln(os.Stderr, e)
	}
	os.Stdout.WriteString(dump)
	if len(errs) > 0 {
		os.Exit(1)
	}
}

// checkFiles 解析 + 检查 + 渲染。`main` 与 `dump_test.go` 共用**同一条**管线 ——
// 测试另写一份的话，测的就不是这条了。
//
// 返回：要写 stdout 的文本、检查器报的错（`conf.Error` 逐条收）、致命错
// （解析失败 / 读不到文件 ⇒ 调用方退出 1）。
func checkFiles(lang string, files []string) (string, []error, error) {
	fset := token.NewFileSet()
	var astFiles []*ast.File
	for _, f := range files {
		af, err := parser.ParseFile(fset, f, nil, parser.ParseComments)
		if err != nil {
			return "", nil, err
		}
		astFiles = append(astFiles, af)
	}

	info := &types.Info{Types: map[ast.Expr]types.TypeAndValue{}, Defs: map[*ast.Ident]types.Object{}}
	conf := types.Config{}
	if lang != "" {
		conf.GoVersion = lang
	}
	var errs []error
	conf.Error = func(err error) { errs = append(errs, err) }
	_, _ = conf.Check(astFiles[0].Name.Name, fset, astFiles, info)

	// C7 §2 规则 1：`#` 开头的行是注释，两侧都忽略（不进对撞）。
	// 但它们要**如实标注来源**（M3-tests.md §5.3）—— 失败时一眼看出是哪一侧。
	var b strings.Builder
	b.WriteString("# source: oracle\n")
	b.WriteString("# files: " + strings.Join(files, " ") + "\n")
	b.WriteString("# package: " + astFiles[0].Name.Name + "\n")
	b.WriteString(dumpAll(astFiles, fset, info))
	return b.String(), errs, nil
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
	// descending 是**环守卫**（见 number）。
	descending map[types.Type]bool
	// unions 是 `unionOf` 的**记忆表**（按接口指针）。
	//
	// 必须记忆：合成的 `*types.Union` 每次 `types.NewUnion` 都是**新指针**
	// （实测），不记忆就会出现「编号时那个 Union」与「渲染时那个 Union」
	// 不是同一个对象 ⇒ `ref` 报 `C7 §3 违反：类型 *types.Union 未编号就被引用`。
	// 实测触发：`type C interface{ comparable }`（裸 `comparable` 是 `*types.Named`，
	// 不在 `interface{ ~T | T }` 那种直接给 `*types.Union` 的形状里）。
	unions map[*types.Interface]*types.Union
	// ifaceSigs 是「**接口方法**的签名」集合。go/types 给接口方法也设了接收者
	// （实测 `I.Method(0).Type().(*types.Signature).Recv()` = `var p.I`），
	// 而 C7 §4.2 规定接口方法的 `recv` 位写 `-`（rgoc 侧的模型里接口方法
	// **根本没有**接收者）。两个后果，必须一起处理：
	//
	//	① 渲染：这些签名写 `-`（见 line）；
	//	② 走访：**匿名**约束接口的接收者就是那个接口自己 ⇒
	//	   「接口 → 方法 → 接收者 → 接口」是真环。实测 `[T interface{ M() }]`
	//	   会在环守卫上炸（`type I interface{ M() }` 不炸，因为具名接口
	//	   先占号，环在 idx 那一行被挡掉）。
	ifaceSigs map[*types.Signature]bool
}

func dumpAll(files []*ast.File, fset *token.FileSet, info *types.Info) string {
	d := &dumper{
		idx:        map[types.Type]int{},
		info:       info,
		descending: map[types.Type]bool{},
		unions:     map[*types.Interface]*types.Union{},
		ifaceSigs:  map[*types.Signature]bool{},
	}
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
// canon 把「同一种类型的另一个名字」折到**唯一表示**上。
//
// 实测（go1.27.1）：`byte` 与 `uint8` 是两个**不同的 `*types.Basic` 对象**
// （Kind 都是 8；`rune`/`int32` 是 Kind 5）—— go/types 保留别名拼写是为了
// 错误消息里能写 `byte`。但它们是**同一个类型**（语言层面 `byte` 就是
// `uint8`）⇒ 编号必须去重（C7 §3 规则 1：同一类型只占一个号），否则两侧
// 从那一行起整体错位（M3/Task 33 实测：`type-dump-basics.go`）。
//
// 只折 `*types.Basic`：它是按 Kind 身份化的；其余种类（`Named`/`Alias`/…）
// 的指针身份就是语义身份，不能动。
func canon(t types.Type) types.Type {
	if b, ok := t.(*types.Basic); ok {
		if c := types.Typ[b.Kind()]; c != nil {
			return c
		}
	}
	return t
}

// intern 分配下标：首次出现定号，再次遇到返回同一个号。
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
	// 别名拼写（`byte` 与 `uint8`、`rune` 与 `int32`）折到唯一表示：它们是
	// **同一个类型**，必须共用一个号（见 canon）。
	t = canon(t)
	if i, ok := d.idx[t]; ok {
		return i
	}
	// 环守卫：**非法程序**的接口类型集可以成环 —— 例如
	// `var h interface{ comparable; M() }`（`comparable` 与方法的组合是非法用法，
	// 但检查器仍会造出一个 term 指回该接口自身的类型集）。后序走访在环上会
	// 无限下钻，最后以 `fatal error: stack overflow` 收场（实测，一次吃掉 1 GB 栈）。
	//
	// **合法程序里匿名类型不可能自引用**（自引用必须经 `Named`/`Alias`，
	// 而那两类是「先占号再下钻」，环在 idx 那一行就被挡掉了）——
	// 所以这个分支**只**在坏样本上命中，不会改变任何合法样本的编号。
	//
	// 命中时响亮地炸（不是静默补号）：对撞样本必须是**合法程序**（C7 §5）。
	if d.descending[t] {
		panic("types-oracle: 类型图成环 —— 对撞样本必须是合法 Go 程序" +
			"（典型触发：interface{ comparable; M() } 这类非法接口用法）")
	}
	d.descending[t] = true
	defer delete(d.descending, t)
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
		// 接口方法的接收者**不下钻**（它是接口自己 ⇒ 匿名接口上是真环），
		// 见 ifaceSigs 的注释；渲染时也写 `-`。
		if r := x.Recv(); r != nil && !d.ifaceSigs[x] {
			d.number(r.Type())
		}
		d.number(x.Params())
		d.number(x.Results())
	case *types.Interface:
		for i := 0; i < x.NumMethods(); i++ {
			m := x.Method(i)
			if sig, ok := m.Type().(*types.Signature); ok {
				// **先标记再下钻** —— 标记要在 number 之前，否则 Recv 已经被走过了
				d.ifaceSigs[sig] = true
			}
			d.number(m.Type())
		}
		if u := d.unionOf(x); u != nil {
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
	// `canon` 与 `number` 用**同一把**钥匙：`byte`/`uint8` 折到同一个条目，
	// 否则字段类型里写的 `byte`（如 `struct{ _ byte }`）会查不到号
	// （实测：`type-dump-compound.go` 曾因此 panic）。
	i, ok := d.idx[canon(t)]
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
// ⚠️ **按接口指针记忆**（`unions`）：合成的 `*types.Union` 每次都是新指针，
// 不记忆会让「编号的」与「渲染的」不是同一个对象（见 `dumper.unions` 的注释）。
// nil 也照样缓存 —— 否则每个接口每次都要重算一遍 `NumEmbeddeds`。
//
// 官方 types2 的 `Interface.tset *termList` 是本函数的对应物：
//   - 单个 `*types.Union` 嵌入 ⇒ 直接用它的项；
//   - 若干个**裸类型**嵌入（`interface{ int }`、`interface{ comparable }`）
//     ⇒ 合成一个 union（每项 exact）；
//   - 没有嵌入 ⇒ 通用集，返回 nil（dump 里就没有 `term` 行）。
func (d *dumper) unionOf(iface *types.Interface) *types.Union {
	if u, ok := d.unions[iface]; ok {
		return u
	}
	u := synthUnion(iface)
	d.unions[iface] = u
	return u
}

func synthUnion(iface *types.Interface) *types.Union {
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
//
// ⚠️ 这里**只出 dump 正文** —— `# source:` 等注释头由 `checkFiles` 加。
// 分成两处是为了让「正文」可被单独比对：头里带文件路径，正文不该带。

func (d *dumper) render() string {
	var b strings.Builder
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
		// 接口方法**一律** `-`（C7 §4.2）：go/types 给它们也设了接收者，
		// 但那不是「方法的外部接收者」口径，rgoc 侧模型里也没有 —— 见 ifaceSigs。
		if r := x.Recv(); r != nil && !d.ifaceSigs[x] {
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
		u := d.unionOf(x)
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
