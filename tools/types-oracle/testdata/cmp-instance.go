// cmp fixture —— 显式实例化（`T-CK-85`；M4/Task 15）。
//
// 语法扩展（`R-32-2`）：`instance <F> => <arg1, arg2, …>` —— 右侧是**实参
// 列表**（顶层逗号分隔；括号/方括号/花括号与引号内的逗号不切）。
// 输出：
//
//	<行号>\tinstance\tok
//	\t<C7 单根 dump 的每一行（原样，前面加一个 tab）>
//
// 失败：`<行号>\tinstance\treject:<category>`（类别 = M4 的 26 类枚举；
// 两侧共用同一套类别名 —— 计数错 ⇒ `type-arg`，约束不满足 ⇒ `implements`）。
package p

type MyInt int

type Box[T any] struct {
	v T
}

type Pair[A, B any] struct {
	a A
	b B
}

type Constrained[T ~int | ~string] struct {
	v T
}

//rgoc:cmp instance Box => int
//rgoc:cmp instance Box => string
//rgoc:cmp instance Box => MyInt
//rgoc:cmp instance Pair => int, string
//rgoc:cmp instance Constrained => int
//rgoc:cmp instance Constrained => string
//rgoc:cmp instance Constrained => bool
//rgoc:cmp instance Box => int, string
//rgoc:cmp instance Pair => int
