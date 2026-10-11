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

// CharEvent 一条**字符级**事件。事件流按**发生顺序**排列。
//
// # 为什么需要字符级（在 token 级之外再加一层）
//
// token 级 golden 只覆盖到"词元边界上的位置"，而位置是**每个字节**推出来的。
// `source.nextch` 是 byte-column 语义的**唯一来源** —— 它的 off-by-one 会**静默地**
// 污染下游所有位置，而且 token 级用例的密度远不足以发现它（手写的几十条里
// 未必有一条恰好落在出错的字节上）。
//
// 所以这里直接把 `nextch()` 暴露出来做**逐事件对撞**。
type CharEvent struct {
	// 诊断（true）还是"读到一个字符"（false）
	IsDiag bool

	// 位置（1 起；列按字节）。诊断与字符都用它。
	Line, Col uint

	// 官方诊断消息原文（仅 IsDiag）。**代号映射在 main 包里做** ——
	// 本文件保持"不含任何逻辑"的纪律，不认识 DiagCode。
	Msg string

	// 字符（仅 !IsDiag）。EOF **不会**出现在事件流里。
	Ch rune
	// Ch 的字节宽度（仅 !IsDiag）。**这是 byte-column 语义的直接证据**：
	// 一个 3 字节的 CJK 字符会让下一列 +3。
	Chw int
}

// DumpChars 逐字符推进整段源码，返回**按发生顺序**的字符级事件流。
//
// # 与 Scan 的分工
//
//   - Scan 走 `next()`，产出**词元**；诊断与词元分两条流各自排序（因为"诊断何时
//     触发"在词元级别属于内部时序，会因实现不同而不同）。
//   - 这里走 `nextch()`，只观察**字符层**的推进：位置、`chw`、以及被跳过的字节
//     报出的诊断。
//
// # 顺序在这里是有意义的，不需要排序
//
// "先报诊断、再让出下一个字符"是 `nextch` 里 `goto redo` 的直接后果 —— 那是**语义**
// （违规字节必须各自占一个列号），不是实现细节。所以事件流可以直接逐行比对。
func DumpChars(src string) []CharEvent {
	var out []CharEvent
	var s scanner
	s.init(strings.NewReader(src), func(line, col uint, msg string) {
		out = append(out, CharEvent{IsDiag: true, Line: line, Col: col, Msg: msg})
	}, 0)

	for {
		s.nextch()
		if s.ch < 0 { // EOF
			break
		}
		// ★ 必须写 s.source.line / s.source.col —— `scanner` 自己的 line/col 是
		//   **词元**位置且会遮蔽嵌入字段 `source` 的同名字段。写 s.line 拿到的是
		//   上一个词元的起点（在这里永远是 0），位置会全错。
		//   位置对外 1 起（官方的 linebase / colbase）。
		out = append(out, CharEvent{
			Line: s.source.line + 1,
			Col:  s.source.col + 1,
			Ch:   s.ch,
			Chw:  s.chw,
		})
	}
	return out
}

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
