package main

func Sum[T any](a, b T) T { return a }

// 对照组：一个真叫 `Sum_int` 的用户符号 —— 实例名 `Sum·int` 不许与它冲突。
func Sum_int(a, b int) int { return b }

func main() {
	println(Sum[int](1, 2))
	println(Sum[string]("x", "y"))
	println(Sum_int(3, 4))
}
