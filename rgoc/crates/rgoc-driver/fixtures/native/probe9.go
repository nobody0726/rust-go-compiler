package main

var b [10]float32

func main() {
	for i := int16(5); i < 10; i = i + 1 {
		b[i] = float32(i)
	}
	s2 := float32(0)
	for i := 5; i < 10; i = i + 1 {
		s2 = s2 + b[i]
	}
	println(s2)

	b := new([100]int)
	for i := 0; i < 100; i = i + 1 {
		b[i] = i
	}
	s3 := 0
	for i := 0; i < 100; i = i + 1 {
		s3 = s3 + b[i]
	}
	println(s3)
}
