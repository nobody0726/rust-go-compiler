package main

func show(c complex128, b bool) {
	println(c, b)
}

func main() {
	show(5+6i, true)
	c := complex(1.5, 2.5)
	show(c, false)
}
