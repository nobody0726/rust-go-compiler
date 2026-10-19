// cmp.go —— `//rgoc:cmp` 指令的读取与判定（M3 的**第二类比对面**，`T-TY-92`/`T-TY-93`
// 的 oracle 侧；第一类是 C7 的 dump，见 main.go）。
//
// # 指令语法（两侧读**同一份** fixture，`D-M3-17`）
//
//	//rgoc:cmp identical <A> => <B>         A / B 是**类型表达式**
//	//rgoc:cmp representable <C> => <T>     C 是**常量表达式**，T 是**类型表达式**
//	//rgoc:cmp assignable <V> => <T>        V / T 是**类型表达式**（M4/Task 15）
//	//rgoc:cmp implements <V> => <I>        I 必须是接口
//	//rgoc:cmp satisfies  <V> => <C>        C 必须是接口（约束）
//	//rgoc:cmp methodset  <T>               **一元**：`methodset ` 之后是整段原文
//	//rgoc:cmp instance   <F> => <a1, a2>   F 必须是泛型具名类型；右侧是实参列表
//
// 一元 `methodset` 与二元 kind 的判据不混：指令里出现 `=>` 而 kind 是
// `methodset` ⇒ 格式错（类型表达式里不会出现 `=>`，所以这是写错了）。
// `instance` 右侧按**顶层逗号**切（括号/方括号/花括号/引号内的逗号不切）——
// `Pair => map[string]int, func(int, string) bool` 是两条实参。
//
// 分隔符是 `=>` 而**不是空白**：类型表达式里本来就有空格（`chan int`、
// `struct{ x int }`、`interface{ String() string }`），按空白切会二义。
// `=>` 在 Go 的类型表达式与常量表达式里都不会出现 ⇒ 唯一确定。
// 指令**必须恰有一个** `=>`；格式错就响亮地炸 —— 静默跳过会让样本
// 悄悄消失（M1/M2 都踩过这一类）。
//
// # 行号
//
// 报告的是**物理**行号（`fset.PositionFor(pos, false)`）。`//line` 重映射两侧
// 都有（M2 的 `T-AP15`），但那是给编译器看的；对撞的坐标必须是文件里肉眼可见
// 的行号。rgoc 侧同口径（`lex::Pos::line` 就是绝对行号，`rel_line()` 才套基准）。
//
// # 判定方式：把操作数**回填成声明**，再走各自的检查器
//
// 两侧都**不在注释里就地求值**：把操作数原文回填成 `package <同一包名>` 的
// 声明，交给各自的检查器。好处是「解析」这一步两侧完全同构（都是 Go 语法），
// 判的确实是同一段代码。oracle 侧合成（**每条声明各占一行**，行号即错误归属）：
//
//	package <fixture 的包名>
//	type  __rgoc_cmp_<行>_a = <A>        // identical
//	type  __rgoc_cmp_<行>_b = <B>
//	const __rgoc_cmp_<行>_c = <C>        // representable：三条
//	type  __rgoc_cmp_<行>_t = <T>
//	var   __rgoc_cmp_<行>_v __rgoc_cmp_<行>_t = __rgoc_cmp_<行>_c
//
// 真值：
//
//   - `identical` ⇒ `types.Identical`（显式 `types.Unalias` 两侧）。别名算不算
//     一层？**不算** —— 判的是**指代的类型**，与 rgoc 侧 `rgoc_types::identical`
//     同口径（那边先 `unalias`）。
//   - `assignable` / `implements` / `satisfies` ⇒ 直接调 `types.AssignableTo` /
//     `types.Implements` / `types.Satisfies`（操作数经 `types.Unalias`，与
//     rgoc 侧先 `unalias` 同口径）。`implements` / `satisfies` 的目标必须是
//     接口（`Underlying().(*types.Interface)`），否则**样本违规** ⇒ 响亮报错。
//   - `methodset` ⇒ `types.NewMethodSet`，名字按字节序排序、`,` 连接；空集 `-`。
//   - `instance` ⇒ `types.Instantiate(…, validate=true)`；成功 ⇒ C7 单根 dump，
//     **每一行前加一个 tab**（续行属于上一条 `instance` 条目）；失败 ⇒
//     `reject:<category>`（计数错 ⇒ `type-arg`；`*types.ArgumentError` ⇒
//     `implements` —— 与 `rgoc-check::diag` 的投影同口径）。
//   - `representable` ⇒ 第 3 行**报不报错**。`var v T = C` 正是规范里
//     「x is an untyped constant representable by a value of type T」那一句
//     （Assignability 的可表示性分支），go/types 内部落到 `representableConst`
//     —— 那个函数**不导出**（`go/types/const.go:78`），所以走「合成声明 +
//     看报错落在哪一行」这条路。前两行的错**不是**判定结果，而是**样本违规**
//     （`<C>` 不是常量表达式 / `<T>` 不是类型表达式）⇒ 响亮报错，不静默算 false。
//
// # oracle **不拒绝**
//
// 语言层的「显式拒绝」（`U-M3-05` 已由 M4/Task 10 解除；现存面如可赋值性的
// 类型参数边界）是 **rgoc 侧**的能力边界：官方 `go/types` 判得出，所以这边
// 一律 true/false。rgoc 输出的 `reject` 条目在对撞里**按白名单排除**
// （`M3-tests.md` §6），排除集本身也要被断言。
package main

import (
	"errors"
	"fmt"
	"go/ast"
	"go/parser"
	"go/scanner"
	"go/token"
	"go/types"
	"sort"
	"strconv"
	"strings"
)

// synFilename 是合成文件在对撞坐标里的名字。它**只出现在错误归属**里，
// 不进输出（输出只有 `#` 头 + 规范行）。
const synFilename = "<rgoc:cmp>"

// cmpEntry 一条 `//rgoc:cmp` 指令。
type cmpEntry struct {
	line int      // 指令所在**物理**行号
	kind string   // 见 cmpKindArity
	a, b string   // 操作数原文（trime 过）；一元 kind 的 b 为空
	args []string // instance：右侧实参列表的拆分结果（其他 kind 为 nil）
}

// cmpKindArity：kind 的操作数个数（`R-32-2` 冻结的 kind 集合）。
// 集合**封闭**：未知 kind ⇒ 响亮报错（静默跳过 = 样本悄悄消失）。
var cmpKindArity = map[string]int{
	"identical":     2,
	"representable": 2,
	"assignable":    2,
	"implements":    2,
	"satisfies":     2,
	"methodset":     1,
	"instance":      2,
}

// cmpKindsText 是报错里列出的支持面（顺序固定，便于人读与测试）。
const cmpKindsText = "identical / representable / assignable / implements / satisfies / methodset / instance"

// parseCmp 从注释里读指令（源码序；多文件时按文件顺序拼接后再按行号排）。
func parseCmp(fset *token.FileSet, files []*ast.File) ([]cmpEntry, error) {
	var out []cmpEntry
	for _, f := range files {
		for _, g := range f.Comments {
			for _, c := range g.List {
				if !strings.HasPrefix(c.Text, "//rgoc:cmp") {
					continue
				}
				line := fset.PositionFor(c.Slash, false).Line
				rest, ok := strings.CutPrefix(c.Text, "//rgoc:cmp ")
				if !ok {
					return nil, fmt.Errorf("%d 行：`//rgoc:cmp` 后面要跟一个空格", line)
				}
				kind, operands, ok := strings.Cut(rest, " ")
				if !ok {
					return nil, fmt.Errorf("%d 行：cmp 指令缺少操作数", line)
				}
				var a, b string
				var args []string
				switch kind {
				case "methodset":
					// 一元：`methodset ` 之后整段原文都是类型表达式。
					a = strings.TrimSpace(operands)
					if a == "" {
						return nil, fmt.Errorf("%d 行：cmp 指令缺少操作数", line)
					}
					if strings.Contains(a, "=>") {
						return nil, fmt.Errorf("%d 行：methodset 是一元 kind，不接受 `=>`", line)
					}
				case "instance":
					var err error
					a, b, err = splitOperands(operands)
					if err != nil {
						return nil, fmt.Errorf("%d 行：%v", line, err)
					}
					args, err = splitArgList(b)
					if err != nil {
						return nil, fmt.Errorf("%d 行：%v", line, err)
					}
				case "identical", "representable", "assignable", "implements", "satisfies":
					var err error
					a, b, err = splitOperands(operands)
					if err != nil {
						return nil, fmt.Errorf("%d 行：%v", line, err)
					}
				default:
					return nil, fmt.Errorf("%d 行：未知的 cmp kind %q（只认 %s）", line, kind, cmpKindsText)
				}
				out = append(out, cmpEntry{line: line, kind: kind, a: a, b: b, args: args})
			}
		}
	}
	sort.SliceStable(out, func(i, j int) bool { return out[i].line < out[j].line })
	return out, nil
}

// splitOperands 把 kind 之后的原文切成两个操作数。
//
// 两种拼写都认（**同一条判据，两侧必须一致**）：
//
//	//rgoc:cmp identical <A> => <B>     ← 操作数里带空格时**只能**用这条
//	//rgoc:cmp identical <A> <B>        ← `M3-tests.md` §5.2 冻结的示意形态
//
// 判据：**先看有没有 `=>`**。有就按它切（`=>` 不是任何合法的 Go 类型表达式
// 或常量表达式的子串，所以不会与操作数混淆）；没有就按空白切成**恰好两段**。
// 两种拼写不会互相干扰 ⇒ 「两种拼写」不等于「两种语义」。
func splitOperands(s string) (string, string, error) {
	if strings.Contains(s, "=>") {
		a, b, ok := strings.Cut(s, "=>")
		if !ok || strings.Contains(b, "=>") {
			return "", "", fmt.Errorf("cmp 指令只能有一个 `=>`")
		}
		a, b = strings.TrimSpace(a), strings.TrimSpace(b)
		if a == "" || b == "" {
			return "", "", fmt.Errorf("cmp 指令有空的操作数")
		}
		return a, b, nil
	}
	fields := strings.Fields(s)
	if len(fields) != 2 {
		return "", "", fmt.Errorf(
			"cmp 指令要有一个 `=>`，或**恰好两个**空白分隔的操作数（实际 %d 个）", len(fields))
	}
	return fields[0], fields[1], nil
}

// splitArgList 把 `instance` 右侧的**实参列表**按顶层逗号切开。
//
// 只按括号/方括号/花括号的**嵌套深度**与引号状态切 —— 类型表达式里本来就有
// 逗号（`map[string]int, func(int, string) bool` 是两条实参，不是四条）。
// 空实参（`int,,string`、结尾逗号）⇒ 格式错：静默丢掉会让样本悄悄变形。
func splitArgList(s string) ([]string, error) {
	var out []string
	depth, start := 0, 0
	var quote byte
	for i := 0; i < len(s); i++ {
		c := s[i]
		if quote != 0 {
			if quote == '"' && c == '\\' {
				i++ // 跳过转义的下一个字节（类型表达式里只有 struct tag 可能出现）
				continue
			}
			if c == quote {
				quote = 0
			}
			continue
		}
		switch c {
		case '"', '`':
			quote = c
		case '(', '[', '{':
			depth++
		case ')', ']', '}':
			depth--
			if depth < 0 {
				return nil, fmt.Errorf("实参列表的括号不配对：%q", s)
			}
		case ',':
			if depth == 0 {
				out = append(out, strings.TrimSpace(s[start:i]))
				start = i + 1
			}
		}
	}
	if quote != 0 || depth != 0 {
		return nil, fmt.Errorf("实参列表的括号/引号不配对：%q", s)
	}
	out = append(out, strings.TrimSpace(s[start:]))
	for _, a := range out {
		if a == "" {
			return nil, fmt.Errorf("实参列表里有空实参：%q", s)
		}
	}
	return out, nil
}

// synRole 合成文件里的一行是**哪条指令的哪个操作数**。
//
// 用于：解析错要能指名道姓（`1+1` 作为类型在 go/parser 里就是解析错，
// 只有 `<rgoc:cmp>:2:23` 这种坐标对写 fixture 的人毫无意义）。
type synRole struct {
	entryLine int    // `//rgoc:cmp` 指令所在行
	text      string // 该操作数的原文
	noun      string // 该操作数该被说成什么（"类型表达式" / "常量表达式"）
}

// cmpDeclLines 一条指令在合成文件里占的行（从 1 起，含 `package` 那行之后）。
type cmpDeclLines struct {
	a, b int   // 二元 kind：两个 `type ... = <A|B>`（instance 不用 a）
	c, t int   // representable：`const` / `type`
	v    int   // representable：`var`（**判定**挂在这一行）
	args []int // instance：实参声明的行号（按实参顺序）
}

// runCmp 执行 identity / representable 判定，返回规范行。
//
// 返回的 `errs` 是 **fixture 自己**的检查错误（合成文件上的不算 —— 那些是
// 判定数据或样本违规）。fixture 必须合法（`M3-tests.md` §5.1 同一条纪律）：
// fixture 有错 ⇒ 调用方非零退出，对撞就会看到「oracle 侧没有产出」。
func runCmp(lang string, fset *token.FileSet, files []string, astFiles []*ast.File) (string, []error, error) {
	entries, err := parseCmp(fset, astFiles)
	if err != nil {
		return "", nil, err
	}
	if len(entries) == 0 {
		return "", nil, fmt.Errorf("fixture 里没有 `//rgoc:cmp` 指令")
	}

	pkgName := astFiles[0].Name.Name
	var src strings.Builder
	src.WriteString("package " + pkgName + "\n")
	line := 1
	loc := make(map[int]cmpDeclLines, len(entries))
	// synRole 记住合成文件每一行是**哪条指令的哪个操作数**：解析错（`1+1`
	// 这类「不是类型」在 go/parser 里就是解析错）要能指名道姓地报回指令行，
	// 否则只有 `<rgoc:cmp>:2:23` 这种谁也看不懂的坐标。
	roles := make(map[int]synRole)
	emit := func(e cmpEntry, noun, text, format string, args ...any) int {
		line++
		fmt.Fprintf(&src, format+"\n", args...)
		roles[line] = synRole{entryLine: e.line, text: text, noun: noun}
		return line
	}
	for _, e := range entries {
		n := fmt.Sprintf("__rgoc_cmp_%d", e.line)
		const typ = "类型表达式"
		switch e.kind {
		case "identical":
			a := emit(e, typ, e.a, "type %s_a = %s", n, e.a)
			b := emit(e, typ, e.b, "type %s_b = %s", n, e.b)
			loc[e.line] = cmpDeclLines{a: a, b: b}
		case "representable":
			c := emit(e, "常量表达式", e.a, "const %s_c = %s", n, e.a)
			t := emit(e, typ, e.b, "type %s_t = %s", n, e.b)
			v := emit(e, "合法的声明", e.b, "var %s_v %s_t = %s_c", n, n, n)
			loc[e.line] = cmpDeclLines{c: c, t: t, v: v}
		case "assignable", "implements", "satisfies":
			a := emit(e, typ, e.a, "type %s_a = %s", n, e.a)
			b := emit(e, typ, e.b, "type %s_b = %s", n, e.b)
			loc[e.line] = cmpDeclLines{a: a, b: b}
		case "methodset":
			a := emit(e, typ, e.a, "type %s_a = %s", n, e.a)
			loc[e.line] = cmpDeclLines{a: a}
		case "instance":
			// F 是**本包声明的泛型具名类型的名字**：不能合成 `type __x_f = F`
			// （`type A = Box` 在 Go 里非法 —— 泛型类型必须实例化）。
			// 于是按包作用域解析（与 rgoc 侧 `pkg.lookup` 同口径）。
			var argLines []int
			for k, arg := range e.args {
				argLines = append(argLines, emit(e, typ, arg, "type %s_a%d = %s", n, k, arg))
			}
			loc[e.line] = cmpDeclLines{args: argLines}
		}
	}

	synFile, perr := parser.ParseFile(fset, synFilename, src.String(), parser.ParseComments)
	if perr != nil {
		return "", nil, fmt.Errorf("%s\n%w", synthParseHint(roles, perr), perr)
	}

	info := &types.Info{
		Defs:  map[*ast.Ident]types.Object{},
		Types: map[ast.Expr]types.TypeAndValue{},
	}
	conf := types.Config{}
	if lang != "" {
		conf.GoVersion = lang
	}
	var all []error
	conf.Error = func(e error) { all = append(all, e) }
	check := append(append([]*ast.File{}, astFiles...), synFile)
	chkPkg, _ := conf.Check(pkgName, fset, check, info)

	// 分类：合成文件上的按行归位；其余是 fixture 自己的错。
	byLine := map[int][]error{}
	var fixtureErrs []error
	for _, e := range all {
		if p, ok := errPos(fset, e); ok && p.Filename == synFilename {
			byLine[p.Line] = append(byLine[p.Line], e)
			continue
		}
		fixtureErrs = append(fixtureErrs, e)
	}

	var out strings.Builder
	out.WriteString("# source: oracle\n")
	out.WriteString("# files: " + strings.Join(files, " ") + "\n")
	out.WriteString("# package: " + pkgName + "\n")
	out.WriteString("# cmp\n")
	for _, e := range entries {
		n := fmt.Sprintf("__rgoc_cmp_%d", e.line)
		l := loc[e.line]
		switch e.kind {
		case "identical":
			if errs := byLine[l.a]; len(errs) > 0 {
				return "", nil, fmt.Errorf("%d 行：`%s` 不是类型表达式：%v", e.line, e.a, errs[0])
			}
			if errs := byLine[l.b]; len(errs) > 0 {
				return "", nil, fmt.Errorf("%d 行：`%s` 不是类型表达式：%v", e.line, e.b, errs[0])
			}
			ta, err := defTypeOf(info, n+"_a")
			if err != nil {
				return "", nil, err
			}
			tb, err := defTypeOf(info, n+"_b")
			if err != nil {
				return "", nil, err
			}
			eq := types.Identical(types.Unalias(ta), types.Unalias(tb))
			out.WriteString(strconv.Itoa(e.line) + "\tidentical\t" + strconv.FormatBool(eq) + "\n")
		case "representable":
			if errs := byLine[l.c]; len(errs) > 0 {
				return "", nil, fmt.Errorf("%d 行：`%s` 不是常量表达式：%v", e.line, e.a, errs[0])
			}
			if errs := byLine[l.t]; len(errs) > 0 {
				return "", nil, fmt.Errorf("%d 行：`%s` 不是类型表达式：%v", e.line, e.b, errs[0])
			}
			ok := len(byLine[l.v]) == 0
			out.WriteString(strconv.Itoa(e.line) + "\trepresentable\t" + strconv.FormatBool(ok) + "\n")
		case "assignable", "implements", "satisfies":
			if errs := byLine[l.a]; len(errs) > 0 {
				return "", nil, fmt.Errorf("%d 行：`%s` 不是类型表达式：%v", e.line, e.a, errs[0])
			}
			if errs := byLine[l.b]; len(errs) > 0 {
				return "", nil, fmt.Errorf("%d 行：`%s` 不是类型表达式：%v", e.line, e.b, errs[0])
			}
			tv, err := defTypeOf(info, n+"_a")
			if err != nil {
				return "", nil, err
			}
			tt, err := defTypeOf(info, n+"_b")
			if err != nil {
				return "", nil, err
			}
			var res bool
			switch e.kind {
			case "assignable":
				res = types.AssignableTo(types.Unalias(tv), types.Unalias(tt))
			default:
				iface := asInterface(tt)
				if iface == nil {
					return "", nil, fmt.Errorf(
						"%d 行：`%s` 不是接口类型（%s 的目标必须是接口）", e.line, e.b, e.kind)
				}
				if e.kind == "implements" {
					res = types.Implements(types.Unalias(tv), iface)
				} else {
					res = types.Satisfies(types.Unalias(tv), iface)
				}
			}
			out.WriteString(strconv.Itoa(e.line) + "\t" + e.kind + "\t" + strconv.FormatBool(res) + "\n")
		case "methodset":
			if errs := byLine[l.a]; len(errs) > 0 {
				return "", nil, fmt.Errorf("%d 行：`%s` 不是类型表达式：%v", e.line, e.a, errs[0])
			}
			tv, err := defTypeOf(info, n+"_a")
			if err != nil {
				return "", nil, err
			}
			ms := types.NewMethodSet(types.Unalias(tv))
			names := make([]string, 0, ms.Len())
			for i := 0; i < ms.Len(); i++ {
				names = append(names, ms.At(i).Obj().Name())
			}
			sort.Strings(names)
			res := "-"
			if len(names) > 0 {
				res = strings.Join(names, ",")
			}
			out.WriteString(strconv.Itoa(e.line) + "\tmethodset\t" + res + "\n")
		case "instance":
			obj := chkPkg.Scope().Lookup(e.a)
			if obj == nil {
				return "", nil, fmt.Errorf(
					"%d 行：`%s` 不是本包声明的类型名 —— instance 的 F 必须是泛型具名类型的名字",
					e.line, e.a)
			}
			named, ok := types.Unalias(obj.Type()).(*types.Named)
			if !ok {
				return "", nil, fmt.Errorf(
					"%d 行：`%s` 不是具名类型 —— instance 的 F 必须是泛型具名类型", e.line, e.a)
			}
			if named.TypeParams().Len() == 0 {
				return "", nil, fmt.Errorf("%d 行：`%s` 不是泛型类型（没有类型形参）", e.line, e.a)
			}
			targs := make([]types.Type, len(e.args))
			for k, ln := range l.args {
				if errs := byLine[ln]; len(errs) > 0 {
					return "", nil, fmt.Errorf(
						"%d 行：`%s` 不是类型表达式：%v", e.line, e.args[k], errs[0])
				}
				ta, err := defTypeOf(info, fmt.Sprintf("%s_a%d", n, k))
				if err != nil {
					return "", nil, err
				}
				targs[k] = types.Unalias(ta)
			}
			inst, ierr := types.Instantiate(types.NewContext(), named, targs, true)
			if ierr != nil {
				// 计数错 ⇒ `type-arg`；约束不满足（`*types.ArgumentError`）
				// ⇒ `implements` —— 与 `rgoc-check::diag::category_of` 同口径。
				cat := "type-arg"
				var ae *types.ArgumentError
				if errors.As(ierr, &ae) {
					cat = "implements"
				}
				out.WriteString(strconv.Itoa(e.line) + "\tinstance\treject:" + cat + "\n")
				continue
			}
			out.WriteString(strconv.Itoa(e.line) + "\tinstance\tok\n")
			block := strings.TrimSuffix(dumpRoot(inst), "\n")
			for _, ln := range strings.Split(block, "\n") {
				out.WriteString("\t" + ln + "\n")
			}
		}
	}
	return out.String(), fixtureErrs, nil
}

// errPos 取错误的**物理**位置。`go/types` 用 `types.Error` **值**回调
// （`errors.go:196` 的 `e := Error{...}`），但两种形态都收下更稳。
func errPos(fset *token.FileSet, e error) (token.Position, bool) {
	switch x := e.(type) {
	case types.Error:
		return fset.PositionFor(x.Pos, false), true
	case *types.Error:
		return fset.PositionFor(x.Pos, false), true
	}
	return token.Position{}, false
}

// synthParseHint 把合成文件的解析错翻译成「第 N 行的哪个操作数不合法」。
//
// 为什么需要：`1+1` 作为类型**在 go/parser 里就是解析错**（`expected type,
// found 1`）—— 那是本文件内部的坐标，对写 fixture 的人毫无意义。
func synthParseHint(roles map[int]synRole, perr error) string {
	var list scanner.ErrorList
	if !errors.As(perr, &list) || len(list) == 0 {
		return "合成文件解析失败（操作数不是合法的类型/常量表达式？）"
	}
	p := list[0].Pos
	if r, ok := roles[p.Line]; ok {
		return fmt.Sprintf("第 %d 行：`%s` 不是%s", r.entryLine, r.text, r.noun)
	}
	return fmt.Sprintf("合成文件解析失败（合成文件第 %d 行）", p.Line)
}

// defTypeOf 按名字取合成声明的类型（名字是唯一的，`info.Defs` 小，直接扫）。
func defTypeOf(info *types.Info, name string) (types.Type, error) {
	for id, obj := range info.Defs {
		if id.Name != name {
			continue
		}
		if obj == nil {
			return nil, fmt.Errorf("合成声明 %s 没有对象（检查器没走到它）", name)
		}
		return obj.Type(), nil
	}
	return nil, fmt.Errorf("合成声明 %s 不在 Defs 里", name)
}

// asInterface 取类型表达式所指的**接口**（`implements` / `satisfies` 的目标）。
//
// 目标不是接口 ⇒ 返回 nil，调用方按**样本违规**响亮报错（不是 `false`：
// `implements V => int` 没有语言层面的真值）。
func asInterface(t types.Type) *types.Interface {
	iface, ok := types.Unalias(t).Underlying().(*types.Interface)
	if !ok {
		return nil
	}
	iface.Complete()
	return iface
}
