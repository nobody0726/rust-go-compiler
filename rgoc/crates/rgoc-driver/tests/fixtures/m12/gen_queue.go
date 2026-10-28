package main

// 泛型 + chan：泛型类型的 chan 字段 + 泛型方法 + select(default)
// 非阻塞排空。不用 comma-ok（U-M11-09 范围外）。

type Queue[T any] struct {
	ch chan T
}

func NewQueue[T any]() Queue[T] {
	return Queue[T]{ch: make(chan T, 4)}
}

func (q Queue[T]) Push(v T) {
	q.ch <- v
}

func (q Queue[T]) Drain() []T {
	var out []T
	for {
		select {
		case v := <-q.ch:
			out = append(out, v)
		default:
			return out
		}
	}
}

func main() {
	q := NewQueue[int]()
	q.Push(1)
	q.Push(2)
	q.Push(3)
	for _, v := range q.Drain() {
		println(v)
	}
}
