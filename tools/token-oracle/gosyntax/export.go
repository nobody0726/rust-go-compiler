// 本文件是【我们自己写的】—— 不属于 vendored 的官方五文件。
//
// 为什么需要它：官方 scanner 的**全部**标识符都是未导出的
// （scanner、next、tok、lit、kind、op、prec、line、col、bad、comments、directives …），
// 外部包在语言层面就访问不到。而 "原样复制官方文件、零改动" 是本目录的硬纪律，
// 所以唯一的出路是**在同一个 package 里加一层纯转发**。
//
// 本文件**不含任何词法逻辑**：它只做三件事 ——
//   ① 把未导出的字段摊成一个导出的记录结构；
//   ② 把"哪些字段此刻有效"（官方写在结构体注释里的那套规则）变成**布尔字段**，
//      让调用方无法"读一个无效字段还不知道"；
//   ③ 转发两个未导出的 mode 位。
//
// 语言：
//   lit  // valid if tok is _Name, _Literal, or _Semi                  （官方 scanner.go:41）
//   bad  // valid if tok is _Literal                                   （官方 scanner.go:42）
//   kind // valid if tok is _Literal                                   （官方 scanner.go:43）
//   op   // valid if tok is _Operator, _Star, _AssignOp, or _IncOp      （官方 scanner.go:44）
//   prec // valid if tok is _Operator, _Star, _AssignOp, or _IncOp      （官方 scanner.go:45）

package syntax

import "strings"

// Tok 是一条词元记录。
//
// 只有**契约允许的字段**才会被填；无效域由 `LitValid` / `IsLiteral` / `HasOp` 三个
// 布尔量显式标出 —— 而不是像官方那样"字段里有值，但注释说它无效"。
// 实测踩过：把 `a >>= 1` 里那个 `>>=` 的 `lit` 当有效值录进 golden，
// 录到的是上一个词元残留的 `"a"` —— 字段有值、类型合法，只有语义是错的。
type Tok struct {
	// 位置（1 起；列按**字节**）
	Line, Col uint

	// 官方 token.String()：EOF / name / literal / op / op= / := / <- / * / ( / ; / package …
	Name string

	// 原始字节切片。**仅当 LitValid 为真时有意义**。
	// 注意它可能是**畸形**的（含被跳过的非法字节），见 Bad。
	Lit string
	// Lit 是否落在有效域内（tok ∈ {_Name, _Literal, _Semi}）
	LitValid bool

	// 是否 _Literal（kind / bad 只在此时有效）
	IsLiteral bool
	// 字面量子类（官方数值：0=Int 1=Float 2=Imag 3=Rune 4=String）
	LitKind int
	// 原文里有语法错误（畸形字面量**照常产出词元**，只是 bad=true）
	Bad bool

	// 是否运算符类（_Operator / _Star / _AssignOp / _IncOp）
	HasOp bool
	// 官方 Operator.String()：Add / Shr / Not / Recv / Def …
	Op string
	// 优先级（precOrOr=1 … precMul=5；不参与二元的为 0）
	Prec int
}

// 官方 scanner.go:26-27 的两个 mode 位（未导出，这里转发）。
const (
	// ModeComments：所有注释都经 errh 上报
	ModeComments = comments
	// ModeDirectives：只有 //go: 与 //line 指令经 errh 上报
	ModeDirectives = directives
)

// Scan 扫完整段源码，返回**按源码顺序**的全部词元（含最后那个 EOF）。
//
// errh 收到的消息有两类，靠**首字符是否为 '/'** 区分（官方 parser.go 用的也是这个约定）：
//   - 以 '/' 开头 ⇒ 注释或指令的**文本**（不是错误）
//   - 否则       ⇒ 一条**诊断**
//
// mode 传 0 时注释与指令一律不上报，只有真正的诊断会走 errh。
func Scan(src string, mode uint, errh func(line, col uint, msg string)) []Tok {
	var s scanner
	s.init(strings.NewReader(src), errh, mode)

	var out []Tok
	for s.tok != _EOF {
		s.next()

		t := Tok{
			Line: s.line,
			Col:  s.col,
			Name: s.tok.String(),
		}

		// 有效域判定：照官方结构体注释逐字翻译。
		switch s.tok {
		case _Name, _Literal, _Semi:
			t.LitValid = true
			t.Lit = s.lit
		}
		if s.tok == _Literal {
			t.IsLiteral = true
			t.LitKind = int(s.kind)
			t.Bad = s.bad
		}
		switch s.tok {
		case _Operator, _Star, _AssignOp, _IncOp:
			t.HasOp = true
			t.Op = s.op.String()
			t.Prec = s.prec
		}

		out = append(out, t)
	}
	return out
}
