// 中立 dump 器 —— rgoc/crates/rgoc-ast/src/dump.rs 的 Go 侧镜像。
//
// 两侧输出必须**逐字节一致**（C6 / D-M2-5）：本文件是官方 AST → 中立格式的
// 遍历器，按 rgoc-ast::dump.rs 的分支逐条直译；改这边（或那边）任何一行
// 输出形状，都要同时改另一侧（见 README §「对齐纪律」）。
//
// 五条格式规则（M2-tests.md §5.2）：
//  1. 节点名用中立名（ForStmt / Operation / ArrayType…）
//  2. 列表显式标 [n entries]（空 = [0 entries]）
//  3. 可选字段用 - 占位
//  4. 位置默认不 dump（另用 @ 腿测）
//  5. 字面量只 dump 原文
//
// 三条规范化规则（§5.3）：
//   - Operation 统一输出（一元时 Y: null）
//   - ListExpr 扁平化（不出现 ListExpr 行）
//   - AssignStmt 统一输出：Rhs==nil 标 IncDec（++/--），op==0 是 =，op==Def 是 :=
package main

import (
	"fmt"
	"strings"

	"astoracle/gosyntax"
)

// rustDebugQuote 用 Rust `{:?}`（str 的 Debug）的语义给字符串加引号。
//
// 为什么不用 %q：两侧要逐字节一致，而 Rust 侧 dump.rs 用的是
// `format!("{:?}", s)` —— 对控制字符是 \0 / \n / \u{7f} 这套写法，
// Go 的 %q 是 \x00 / \n / \x7f 那套写法，非 ASCII 可打印字符两侧都
// 原样保留。这里只实现 Rust 语义（\\ \" \n \r \t \0，其余 <0x20 或
// 0x7f 用 \u{hex} 小写无补零；可打印 Unicode 原样）。
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

func nameStr(n *syntax.Name) string {
	if n == nil {
		return "-"
	}
	return "Name " + rustDebugQuote(n.Value)
}

func litKindStr(k syntax.LitKind) string {
	// 对齐 dump.rs 的 lit_str：小写四字缩写（官方 LitKind.String() 是
	// "IntLit" 等驼峰形，两侧约定的都不是官方串）。
	switch k {
	case syntax.IntLit:
		return "int"
	case syntax.FloatLit:
		return "float"
	case syntax.ImagLit:
		return "imag"
	case syntax.RuneLit:
		return "rune"
	case syntax.StringLit:
		return "string"
	}
	return fmt.Sprintf("LitKind(%d)", k) // 不可达；防御性
}

func litStr(l *syntax.BasicLit) string {
	if l == nil {
		return "-"
	}
	return fmt.Sprintf("BasicLit(%s) %s", litKindStr(l.Kind), rustDebugQuote(l.Value))
}

func optLitStr(l *syntax.BasicLit) string {
	if l == nil {
		return "-"
	}
	return litStr(l)
}

// chanDirStr 对齐 dump.rs 的 chan_dir_str：0 → "-"、SendOnly → "send"、
// RecvOnly → "recv"。
func chanDirStr(d syntax.ChanDir) string {
	switch d {
	case 0:
		return "-"
	case syntax.SendOnly:
		return "send"
	case syntax.RecvOnly:
		return "recv"
	}
	return fmt.Sprintf("ChanDir(%d)", d) // 不可达；防御性
}

// assignOpStr 对齐 dump.rs 的 assign_op_str。官方 AssignStmt.Op 的取值域
// （parser.go simpleStmt）：
//   - `=`  → Operator(0)
//   - `:=` → Def
//   - `+=` 等复合赋值 → _AssignOp 词元的 op（基底运算符）
//   - `++` / `--` → Add / Sub，**且 Rhs == nil**（nodes.go 注释）
//
// 所以复合赋值在这里（且只在这里）要映射成 "op=" 的拼写；二元表达式那边的
// Operation 用的是 opStr（裸 "+"）。
func assignOpStr(op syntax.Operator, rhs syntax.Expr) string {
	if rhs == nil {
		// Rhs == nil 只能来自 _IncOp：Lhs++（Add）或 Lhs--（Sub）。
		switch op {
		case syntax.Add:
			return "++"
		case syntax.Sub:
			return "--"
		}
		return fmt.Sprintf("IncOp(%d)", op) // 不可达；防御性
	}
	switch op {
	case 0:
		return "="
	case syntax.Def:
		return ":="
	case syntax.Add:
		return "+="
	case syntax.Sub:
		return "-="
	case syntax.Mul:
		return "*="
	case syntax.Div:
		return "/="
	case syntax.Rem:
		return "%="
	case syntax.And:
		return "&="
	case syntax.Or:
		return "|="
	case syntax.Xor:
		return "^="
	case syntax.Shl:
		return "<<="
	case syntax.Shr:
		return ">>="
	case syntax.AndNot:
		return "&^="
	}
	return op.String() // 不可达；防御性
}

// opStr 对齐 dump.rs 的 op_str：二元/一元运算符的裸拼写。
// 官方 operator_string.go 的 String()（stringer -linecomment）正是这套。
func opStr(op syntax.Operator) string {
	return op.String()
}

type dumper struct {
	out    strings.Builder
	indent int
}

func dump(f *syntax.File) string {
	d := &dumper{}
	d.file(f)
	return d.out.String()
}

func (d *dumper) line(s string) {
	for i := 0; i < d.indent; i++ {
		d.out.WriteString("  ")
	}
	d.out.WriteString(s)
	d.out.WriteByte('\n')
}

func (d *dumper) printf(format string, args ...any) {
	d.line(fmt.Sprintf(format, args...))
}

// optField 对齐 dump.rs 的 opt_dump：Some → 标签行 + 缩进递归；None → "标签: -"。
func (d *dumper) optField(label string, present bool, f func()) {
	if present {
		d.line(label + ":")
		d.indent++
		f()
		d.indent--
	} else {
		d.printf("%s: -", label)
	}
}

func (d *dumper) list(label string, n int, f func(i int)) {
	d.printf("%s: [%d entries]", label, n)
	d.indent++
	for i := 0; i < n; i++ {
		f(i)
	}
	d.indent--
}

// exprListFrom 把官方「单个 Expr（可能是 ListExpr / nil）」归一成列表
// （§5.3 扁平化）：nil → [0 entries]；ListExpr → [n entries] + 展开；
// 其余 → [1 entries] + 单元素。
func (d *dumper) exprListFrom(label string, e syntax.Expr) {
	switch x := e.(type) {
	case nil:
		d.list(label, 0, func(int) {})
	case *syntax.ListExpr:
		d.list(label, len(x.ElemList), func(i int) { d.expr(x.ElemList[i]) })
	default:
		d.list(label, 1, func(int) { d.expr(e) })
	}
}

// exprField 对齐 dump.rs 的 expr_field：简单节点（Bad/Name/BasicLit）内联，
// 复合节点换行缩进。
func (d *dumper) exprField(label string, e syntax.Expr) {
	if e == nil {
		d.printf("%s: -", label)
		return
	}
	if inline := inlineExpr(e); inline != "" {
		d.printf("%s: %s", label, inline)
	} else {
		d.line(label + ":")
		d.indent++
		d.expr(e)
		d.indent--
	}
}

func (d *dumper) file(f *syntax.File) {
	d.line("File")
	d.indent++
	d.printf("PkgName: %s", nameStr(f.PkgName))
	gv := f.GoVersion
	if gv == "" {
		gv = "-"
	}
	d.printf("GoVersion: %s", gv)
	d.list("Decls", len(f.DeclList), func(i int) { d.decl(f.DeclList[i]) })
	d.indent--
}

func (d *dumper) decl(x syntax.Decl) {
	switch dd := x.(type) {
	case *syntax.ImportDecl:
		d.line("ImportDecl")
		d.indent++
		d.printf("Name: %s", nameStr(dd.LocalPkgName))
		d.printf("Path: %s", optLitStr(dd.Path))
		d.indent--
	case *syntax.ConstDecl:
		d.line("ConstDecl")
		d.indent++
		d.list("Names", len(dd.NameList), func(i int) { d.line(nameStr(dd.NameList[i])) })
		d.exprField("Type", dd.Type)
		d.exprListFrom("Values", dd.Values)
		d.indent--
	case *syntax.TypeDecl:
		d.line("TypeDecl")
		d.indent++
		d.printf("Name: %s", nameStr(dd.Name))
		d.printf("Alias: %t", dd.Alias)
		d.list("TParams", len(dd.TParamList), func(i int) { d.field(dd.TParamList[i], nil) })
		d.exprField("Type", dd.Type)
		d.indent--
	case *syntax.VarDecl:
		d.line("VarDecl")
		d.indent++
		d.list("Names", len(dd.NameList), func(i int) { d.line(nameStr(dd.NameList[i])) })
		d.exprField("Type", dd.Type)
		d.exprListFrom("Values", dd.Values)
		d.indent--
	case *syntax.FuncDecl:
		d.line("FuncDecl")
		d.indent++
		d.optField("Recv", dd.Recv != nil, func() { d.field(dd.Recv, nil) })
		d.printf("Name: %s", nameStr(dd.Name))
		d.list("TParams", len(dd.TParamList), func(i int) { d.field(dd.TParamList[i], nil) })
		d.exprField("Type", dd.Type)
		d.optField("Body", dd.Body != nil, func() { d.block(dd.Body) })
		d.indent--
	}
}

// field 对齐 dump.rs 的 dump_field。tag 只在 StructType 上有值（TagList
// 平行数组按位 zip，C6 归一）；其余上下文恒 "-"。
func (d *dumper) field(f *syntax.Field, tag *syntax.BasicLit) {
	d.line("Field")
	d.indent++
	d.printf("Name: %s", nameStr(f.Name))
	d.exprField("Type", f.Type)
	d.printf("Tag: %s", optLitStr(tag))
	d.indent--
}

func (d *dumper) block(b *syntax.BlockStmt) {
	d.line("BlockStmt")
	d.indent++
	d.list("Stmts", len(b.List), func(i int) { d.stmt(b.List[i]) })
	d.indent--
}

func (d *dumper) stmt(s syntax.Stmt) {
	switch st := s.(type) {
	case *syntax.EmptyStmt:
		d.line("EmptyStmt")
	case *syntax.LabeledStmt:
		d.line("LabeledStmt")
		d.indent++
		d.printf("Label: %s", nameStr(st.Label))
		d.line("Stmt:")
		d.indent++
		d.stmt(st.Stmt)
		d.indent--
		d.indent--
	case *syntax.BlockStmt:
		d.block(st)
	case *syntax.ExprStmt:
		d.line("ExprStmt")
		d.indent++
		d.exprField("X", st.X)
		d.indent--
	case *syntax.SendStmt:
		d.line("SendStmt")
		d.indent++
		d.exprField("Chan", st.Chan)
		d.exprField("Value", st.Value)
		d.indent--
	case *syntax.DeclStmt:
		d.line("DeclStmt")
		d.indent++
		d.list("Decls", len(st.DeclList), func(i int) { d.decl(st.DeclList[i]) })
		d.indent--
	case *syntax.AssignStmt:
		d.line("AssignStmt")
		d.indent++
		d.printf("Op: %s", assignOpStr(st.Op, st.Rhs))
		d.exprListFrom("Lhs", st.Lhs)
		d.exprListFrom("Rhs", st.Rhs)
		d.indent--
	case *syntax.BranchStmt:
		d.line("BranchStmt")
		d.indent++
		d.printf("Tok: %s", st.Tok.String())
		d.printf("Label: %s", nameStr(st.Label))
		d.indent--
	case *syntax.CallStmt:
		d.line("CallStmt")
		d.indent++
		d.printf("Tok: %s", st.Tok.String())
		d.exprField("Call", st.Call)
		d.indent--
	case *syntax.ReturnStmt:
		d.line("ReturnStmt")
		d.indent++
		d.exprListFrom("Results", st.Results)
		d.indent--
	case *syntax.IfStmt:
		d.line("IfStmt")
		d.indent++
		d.optSimple("Init", st.Init)
		d.exprField("Cond", st.Cond)
		d.line("Then:")
		d.indent++
		d.block(st.Then)
		d.indent--
		d.optField("Else", st.Else != nil, func() { d.stmt(st.Else) })
		d.indent--
	case *syntax.ForStmt:
		d.line("ForStmt")
		d.indent++
		d.optSimple("Init", st.Init)
		d.exprField("Cond", st.Cond)
		d.optSimple("Post", st.Post)
		d.line("Body:")
		d.indent++
		d.block(st.Body)
		d.indent--
		d.indent--
	case *syntax.SwitchStmt:
		d.line("SwitchStmt")
		d.indent++
		d.optSimple("Init", st.Init)
		d.exprField("Tag", st.Tag)
		d.list("Cases", len(st.Body), func(i int) {
			c := st.Body[i]
			d.line("CaseClause")
			d.indent++
			d.exprField("Cases", c.Cases)
			d.list("Body", len(c.Body), func(j int) { d.stmt(c.Body[j]) })
			d.indent--
		})
		d.indent--
	case *syntax.SelectStmt:
		d.line("SelectStmt")
		d.indent++
		d.list("Clauses", len(st.Body), func(i int) {
			c := st.Body[i]
			d.line("CommClause")
			d.indent++
			d.optSimple("Comm", c.Comm)
			d.list("Body", len(c.Body), func(j int) { d.stmt(c.Body[j]) })
			d.indent--
		})
		d.indent--
	}
}

func (d *dumper) optSimple(label string, s syntax.SimpleStmt) {
	if s != nil {
		d.line(label + ":")
		d.indent++
		d.simple(s)
		d.indent--
	} else {
		d.printf("%s: -", label)
	}
}

func (d *dumper) simple(s syntax.SimpleStmt) {
	switch st := s.(type) {
	case *syntax.EmptyStmt:
		d.line("EmptyStmt")
	case *syntax.ExprStmt:
		d.line("ExprStmt")
		d.indent++
		d.exprField("X", st.X)
		d.indent--
	case *syntax.SendStmt:
		d.line("SendStmt")
		d.indent++
		d.exprField("Chan", st.Chan)
		d.exprField("Value", st.Value)
		d.indent--
	case *syntax.AssignStmt:
		d.line("AssignStmt")
		d.indent++
		d.printf("Op: %s", assignOpStr(st.Op, st.Rhs))
		d.exprListFrom("Lhs", st.Lhs)
		d.exprListFrom("Rhs", st.Rhs)
		d.indent--
	case *syntax.RangeClause:
		d.line("RangeClause")
		d.indent++
		d.printf("Define: %t", st.Def)
		d.exprListFrom("Lhs", st.Lhs)
		d.exprField("X", st.X)
		d.indent--
	}
}

func (d *dumper) expr(e syntax.Expr) {
	switch x := e.(type) {
	case *syntax.BadExpr:
		d.line("BadExpr")
	case *syntax.Name:
		d.line(nameStr(x))
	case *syntax.BasicLit:
		d.line(litStr(x))
	case *syntax.ListExpr:
		// §5.3 扁平化：不出现 ListExpr 行，直接展开元素。
		for _, el := range x.ElemList {
			d.expr(el)
		}
	case *syntax.CompositeLit:
		d.line("CompositeLit")
		d.indent++
		d.exprField("Type", x.Type)
		d.list("Elems", len(x.ElemList), func(i int) { d.expr(x.ElemList[i]) })
		d.indent--
	case *syntax.KeyValueExpr:
		d.line("KeyValueExpr")
		d.indent++
		d.exprField("Key", x.Key)
		d.exprField("Value", x.Value)
		d.indent--
	case *syntax.FuncLit:
		d.line("FuncLit")
		d.indent++
		d.exprField("Type", x.Type)
		d.line("Body:")
		d.indent++
		d.block(x.Body)
		d.indent--
		d.indent--
	case *syntax.ParenExpr:
		d.line("ParenExpr")
		d.indent++
		d.exprField("X", x.X)
		d.indent--
	case *syntax.SelectorExpr:
		d.line("SelectorExpr")
		d.indent++
		d.exprField("X", x.X)
		d.printf("Sel: %s", nameStr(x.Sel))
		d.indent--
	case *syntax.IndexExpr:
		d.line("IndexExpr")
		d.indent++
		d.exprField("X", x.X)
		d.exprField("Index", x.Index)
		d.indent--
	case *syntax.SliceExpr:
		d.line("SliceExpr")
		d.indent++
		d.exprField("X", x.X)
		d.exprField("Low", x.Index[0])
		d.exprField("High", x.Index[1])
		d.exprField("Max", x.Index[2])
		d.indent--
	case *syntax.AssertExpr:
		d.line("AssertExpr")
		d.indent++
		d.exprField("X", x.X)
		d.exprField("Type", x.Type)
		d.indent--
	case *syntax.TypeSwitchGuard:
		d.line("TypeSwitchGuard")
		d.indent++
		d.printf("Lhs: %s", nameStr(x.Lhs))
		d.exprField("X", x.X)
		d.indent--
	case *syntax.Operation:
		// §5.3：一元/二元统一输出 Operation；Y == nil 是一元 → "Y: null"。
		d.line("Operation")
		d.indent++
		d.printf("Op: %s", opStr(x.Op))
		d.exprField("X", x.X)
		if x.Y == nil {
			d.line("Y: null")
		} else {
			d.exprField("Y", x.Y)
		}
		d.indent--
	case *syntax.CallExpr:
		d.line("CallExpr")
		d.indent++
		d.exprField("Fun", x.Fun)
		d.list("Args", len(x.ArgList), func(i int) { d.expr(x.ArgList[i]) })
		d.printf("HasDots: %t", x.HasDots)
		d.indent--
	case *syntax.ArrayType:
		d.line("ArrayType")
		d.indent++
		d.exprField("Len", x.Len)
		d.exprField("Elem", x.Elem)
		d.indent--
	case *syntax.SliceType:
		d.line("SliceType")
		d.indent++
		d.exprField("Elem", x.Elem)
		d.indent--
	case *syntax.DotsType:
		d.line("DotsType")
		d.indent++
		d.exprField("Elem", x.Elem)
		d.indent--
	case *syntax.StructType:
		d.line("StructType")
		d.indent++
		d.list("Fields", len(x.FieldList), func(i int) {
			var tag *syntax.BasicLit
			if i < len(x.TagList) {
				tag = x.TagList[i]
			}
			d.field(x.FieldList[i], tag)
		})
		d.indent--
	case *syntax.InterfaceType:
		d.line("InterfaceType")
		d.indent++
		d.list("Methods", len(x.MethodList), func(i int) { d.field(x.MethodList[i], nil) })
		d.indent--
	case *syntax.FuncType:
		d.line("FuncType")
		d.indent++
		// 注意：没有 TParams —— 类型参数属于 FuncDecl/TypeDecl，不属于
		// FuncType（官方 funcType 把它们返回给调用方）。
		d.list("Params", len(x.ParamList), func(i int) { d.field(x.ParamList[i], nil) })
		d.list("Results", len(x.ResultList), func(i int) { d.field(x.ResultList[i], nil) })
		d.indent--
	case *syntax.MapType:
		d.line("MapType")
		d.indent++
		d.exprField("Key", x.Key)
		d.exprField("Value", x.Value)
		d.indent--
	case *syntax.ChanType:
		d.line("ChanType")
		d.indent++
		d.printf("Dir: %s", chanDirStr(x.Dir))
		d.exprField("Elem", x.Elem)
		d.indent--
	}
}

// inlineExpr 对齐 dump.rs 的 inline_expr：只有 Bad/Name/BasicLit 是单行内联。
func inlineExpr(e syntax.Expr) string {
	switch x := e.(type) {
	case *syntax.BadExpr:
		return "BadExpr"
	case *syntax.Name:
		return nameStr(x)
	case *syntax.BasicLit:
		return litStr(x)
	}
	return ""
}
