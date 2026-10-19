// cmp fixture —— 方法集（`T-CK-84`；M4/Task 15）。
//
// 语法扩展（`R-32-2`）：**一元** kind —— `//rgoc:cmp methodset <T>`。操作数是
// 指令行 `methodset ` 之后的**整段原文**（类型表达式里本来就有空格）。
// 输出：`<行号>\tmethodset\t<名字序列>`，名字按字节序升序、`,` 分隔；
// 空集写 `-`（与 C7 的 `-` 同一条可读性纪律）。
package p

type MyInt int

type S struct{ v int }

func (S) String() string { return "" }

func (*S) Ptr() {}

type Stringer interface {
	String() string
}

type Two interface {
	String() string
	Ptr()
}

//rgoc:cmp methodset S
//rgoc:cmp methodset *S
//rgoc:cmp methodset MyInt
//rgoc:cmp methodset Stringer
//rgoc:cmp methodset Two
//rgoc:cmp methodset int
//rgoc:cmp methodset struct{ x int }
