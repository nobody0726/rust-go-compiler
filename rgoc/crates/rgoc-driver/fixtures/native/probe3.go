package main

func main() {
	i := 5
	s := string(i + '0')
	println(s)
	println(len(s))
	r := "x" + s
	println(r)
}
