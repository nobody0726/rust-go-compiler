package p

// 签名与元组：无名 / 有名 / 变参 / 多结果 / 函数类型参数 / 方法（带接收者）。
// 方法**是** dump 根（C7 §3：顶层 `func` 声明，含方法 —— 它的签名带 `recv`）。

func f0() {}

func f1(x int) int { return x }

func f2(x, y int) (int, error) { return x, nil }

func f3(xs ...int) {}

func f4(f func(int) int) {}

func f5() (n int, err error) { return 0, nil }

type S struct{}

func (s S) M(x int) {}

func (s *S) P() {}
