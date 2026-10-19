// cmp fixture —— 约束满足（`T-CK-83`；M4/Task 15）。
//
// 语法：`satisfies <V> => <C>`（二元）；`<C>` 必须是接口类型（约束也是接口）。
// 与 `implements` 的差别：**类型集**参与判定（`~int | ~float64`、`comparable`）。
package p

type MyInt int

type S struct{ v int }

func (S) String() string { return "" }

type Num interface {
	~int | ~float64
}

type IntOnly interface {
	~int
}

type Stringer interface {
	String() string
}

//rgoc:cmp satisfies int => Num
//rgoc:cmp satisfies MyInt => Num
//rgoc:cmp satisfies float64 => Num
//rgoc:cmp satisfies string => Num
//rgoc:cmp satisfies int => IntOnly
//rgoc:cmp satisfies MyInt => IntOnly
//rgoc:cmp satisfies int => comparable
//rgoc:cmp satisfies []int => comparable
//rgoc:cmp satisfies int => any
//rgoc:cmp satisfies S => Stringer
