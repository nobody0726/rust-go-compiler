package p

// 具名 / 别名 / 递归具名 / 常量块（常量的**声明类型**是 dump 根）。

const N = 4
const TypedN int = N

type MyInt int

type Alias = int

type AliasOfNamed = MyInt

// 递归具名类型：先占号、后下钻（C7 §3 的编号规则）。
type Node struct {
	next  *Node
	value int
}

type List struct {
	head *Node
	n    int
}

var arr [N]int
var typed [TypedN]MyInt
var mi MyInt
var ai Alias
var an AliasOfNamed
var node Node
var list *List
