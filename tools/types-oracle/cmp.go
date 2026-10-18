// cmp.go —— `//rgoc:cmp` 指令的读取与判定（M3 的**第二类比对面**，`T-TY-92`/`T-TY-93`
// 的 oracle 侧；第一类是 C7 的 dump，见 main.go）。
//
// # 指令语法（两侧读**同一份** fixture，`D-M3-17`）
//
//	//rgoc:cmp identical <A> => <B>         A / B 是**类型表达式**
//	//rgoc:cmp representable <C> => <T>     C 是**常量表达式**，T 是**类型表达式**
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
//（`M3-tests.md` §6），排除集本身也要被断言。
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
	line int    // 指令所在**物理**行号
	kind string // "identical" | "representable"
	a, b string // `=>` 两侧的原文（trime 过；各自解析）
}

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
				if kind != "identical" && kind != "representable" {
					return nil, fmt.Errorf("%d 行：未知的 cmp kind %q（只认 identical / representable）", line, kind)
				}
				a, b, err := splitOperands(operands)
				if err != nil {
					return nil, fmt.Errorf("%d 行：%v", line, err)
				}
				out = append(out, cmpEntry{line: line, kind: kind, a: a, b: b})
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

// synRole 合成文件里的一行是**哪条指令的哪个操作数**。
//
// 用于：解析错要能指名道姓（`1+1` 作为类型在 go/parser 里就是解析错，
// 只有 `<rgoc:cmp>:2:23` 这种坐标对写 fixture 的人毫无意义）。
type synRole struct {
	entryLine int    // `//rgoc:cmp` 指令所在行
	text      string // 该操作数的原文
	kind      string // "A" / "B" / "C" / "T" / "V"
}

// cmpDeclLines 一条指令在合成文件里占的行（从 1 起，含 `package` 那行之后）。
type cmpDeclLines struct {
	a, b int // identical：两个 `type ... = <A|B>`
	c, t int // representable：`const` / `type`
	v    int // representable：`var`（**判定**挂在这一行）
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
	emit := func(kind string, e cmpEntry, format string, args ...any) int {
		line++
		fmt.Fprintf(&src, format+"\n", args...)
		text := e.a
		if kind == "B" || kind == "T" {
			text = e.b
		}
		roles[line] = synRole{entryLine: e.line, text: text, kind: kind}
		return line
	}
	for _, e := range entries {
		n := fmt.Sprintf("__rgoc_cmp_%d", e.line)
		switch e.kind {
		case "identical":
			a := emit("A", e, "type %s_a = %s", n, e.a)
			b := emit("B", e, "type %s_b = %s", n, e.b)
			loc[e.line] = cmpDeclLines{a: a, b: b}
		case "representable":
			c := emit("C", e, "const %s_c = %s", n, e.a)
			t := emit("T", e, "type %s_t = %s", n, e.b)
			v := emit("V", e, "var %s_v %s_t = %s_c", n, n, n)
			loc[e.line] = cmpDeclLines{c: c, t: t, v: v}
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
	_, _ = conf.Check(pkgName, fset, check, info)

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
		return fmt.Sprintf("第 %d 行：`%s` 不是%s", r.entryLine, r.text, operandNoun(r.kind))
	}
	return fmt.Sprintf("合成文件解析失败（合成文件第 %d 行）", p.Line)
}

// operandNoun 一个操作数该被说成什么（错误消息用词与检查路径一致）。
func operandNoun(kind string) string {
	if kind == "C" {
		return "常量表达式"
	}
	if kind == "V" {
		return "合法的声明" // 内部生成，正常不可达
	}
	return "类型表达式"
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
