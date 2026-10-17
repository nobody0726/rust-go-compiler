package p

// 泛型：类型参数 + 约束的几种形状（含**曾经让 oracle 崩**的三种：
// `[T int]`、`[T interface{ M() }]`、`[T Named]`）。
// `named` 行**不渲染类型参数**（C7 §4.2）—— 泛型具名类型与非泛型同形。

type Num interface {
	~int | ~float64
}

type Stringer interface {
	String() string
}

type Box[T any] struct {
	v T
}

func fAny[T any](x T) {}

func fComparable[T comparable](x T) {}

func fInt[T int](x T) {}

func fTildeSlice[T ~[]int](x T) {}

func fUnion[T int | ~string](x T) {}

func fNamed[T Num](x T) {}

func fNamedIface[T Stringer](x T) {}

func fIfaceMethod[T interface{ String() string }](x T) {}

func fTwo[A any, B comparable](a A, b B) {}
