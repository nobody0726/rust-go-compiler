package p

// 复合形状：指针 / 切片 / 数组 / 映射 / 三种通道 / 结构体（含 tag、嵌入、空字段名 `_`）。

type Point struct {
	X, Y int
}

type Tagged struct {
	A     int `json:"a"`
	b     string
	Point
	_ byte
}

var p *int
var s []int
var nested []map[string]*int
var a [4]int
var m map[string]int
var c1 chan int
var c2 chan<- int
var c3 <-chan int
var st struct{ x int }
var tagged Tagged
var pt Point
