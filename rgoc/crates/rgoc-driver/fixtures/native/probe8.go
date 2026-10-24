package main

var g [10]float32

func main() {
	g[0] = 1
	b := new([100]int)
	for i := 0; i < 100; i = i + 1 {
		b[i] = i
	}
	s := 0
	for i := 0; i < 100; i = i + 1 {
		s = s + b[i]
	}
	println(s)
	println(b[3])
	println(g[0])
}
