package main

func main() {
	var x float32 = 35
	if x != 35 {
		println("bad")
	}
	var y float64 = 1.5
	if y < 2 {
		println("lt")
	}
	if y == 1.5 {
		println("eq")
	}
}
