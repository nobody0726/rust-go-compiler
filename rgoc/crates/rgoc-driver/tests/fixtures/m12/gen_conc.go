package main

// 泛型 + goroutine：显式实例化的 `go` 调用 + range chan 汇合。
// 刻意避开 U-M11-09（select 的 comma-ok）与 U-M11-10（go 的聚合浮点
// 实参）——实参全是单字（chan / slice），实例化显式。

func produce[T any](ch chan T, xs []T) {
	for _, x := range xs {
		ch <- x
	}
	close(ch)
}

func sum[T int | float64](ch chan T) T {
	var s T
	for v := range ch {
		s += v
	}
	return s
}

func main() {
	ch := make(chan int, 8)
	go produce[int](ch, []int{1, 2, 3, 4})
	total := sum[int](ch)
	println(total)
}
