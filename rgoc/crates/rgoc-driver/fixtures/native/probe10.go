package main

func main() {
	var a [10]float32
	for i := int16(5); i < 10; i = i + 1 {
		a[i] = float32(i)
	}
	s1 := float32(0)
	for i := 5; i < 10; i = i + 1 {
		s1 = s1 + a[i]
	}
	println(s1)
}
