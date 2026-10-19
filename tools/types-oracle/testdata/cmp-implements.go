// cmp fixture —— 接口实现（`T-CK-82`；M4/Task 15）。
//
// 语法：`implements <V> => <I>`（二元）；`<I>` 必须是接口类型（否则是**样本
// 违规** ⇒ 两侧都响亮报错，不是 `false`）。判据 = 方法集包含（`types.Implements`，
// 不含类型集面 —— 那是 `satisfies` 的判据）。
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

//rgoc:cmp implements S => Stringer
//rgoc:cmp implements *S => Stringer
//rgoc:cmp implements *S => Two
//rgoc:cmp implements S => Two
//rgoc:cmp implements MyInt => Stringer
//rgoc:cmp implements S => any
//rgoc:cmp implements MyInt => any
//rgoc:cmp implements int => any
//rgoc:cmp implements Stringer => Stringer
//rgoc:cmp implements Stringer => any
