// cmp fixture —— 可赋值性（`T-CK-81`；M4/Task 15）。
//
// 语法扩展（`M3-tests.md` §5.2 的修订 `R-32-2`）：二元 kind，`<V> => <T>`。
// 输出沿用 `<行号>\tassignable\ttrue|false|reject`；`reject` 是 rgoc 侧的
// 能力边界（官方永远给 true/false）⇒ 排除集机制照旧（`// rgoc:expect-reject`）。
//
// 样本面：
//   - 具名 ↔ 未具名（同一本体 + 至少一侧未具名 ⇒ 可赋值）；
//   - 方向性 channel（双向 ⇒ 单向可以，反向不行）；
//   - 空接口 / 方法接口（T 接口 ⇒ 走「实现」分支）。
package p

type MyInt int

type S struct{ v int }

type Stringer interface {
	String() string
}

func (S) String() string { return "" }

//rgoc:cmp assignable int => int
//rgoc:cmp assignable int => MyInt
//rgoc:cmp assignable MyInt => int
//rgoc:cmp assignable S => struct{ v int }
//rgoc:cmp assignable struct{ v int } => S
//rgoc:cmp assignable int => any
//rgoc:cmp assignable S => Stringer
//rgoc:cmp assignable chan int => <-chan int
//rgoc:cmp assignable <-chan int => chan int
//rgoc:cmp assignable []int => any
//rgoc:cmp assignable int => Stringer
//rgoc:cmp assignable func(int) string => func(int) string
