package main

func main() {
	c := complex(1.5, 2.5)
	println(real(c), imag(c))
	println(c)
	var z complex128
	z = c + complex(0.5, 0.5)
	println(z)
}
