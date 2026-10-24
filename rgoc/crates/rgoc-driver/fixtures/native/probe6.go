package main

func main() {
	r := ""
	a := 3
	for i := 0; i < 10; i = i + 1 {
		switch i {
		case 5:
			r += "five"
		case a, 7:
			r += "a"
		default:
			r += string(i + '0')
		}
		r += "out" + string(i+'0')
		println(r)
	}
	println(r)
}
