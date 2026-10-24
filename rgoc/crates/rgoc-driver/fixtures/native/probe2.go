package main

var n int

func mk(s string) string {
	n++
	return s
}

func main() {
	a := mk("ab")
	b := mk("ab")
	println(a == b)
	println(a != b)
	println(a < b)
	println(mk("a")+"b")
	println(a == "ab")
}
