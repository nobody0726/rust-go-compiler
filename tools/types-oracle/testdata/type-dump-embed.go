package p

// 嵌入面（M4/Task 8 语法拍平修复的对撞目标 —— M3 的 fixture 与语料
// 都没有「嵌入具名接口」形态，会让 splice/exact 两种口径同绿）。
//
// oracle `synthUnion` 口径（探针实测 + 本 fixture 对撞）：
//   * 嵌入**具名方法接口** ⇒ 方法**拍平** + 一条 **exact 项 as-written**；
//   * 嵌入**具名约束接口**（`~` 并集）⇒ exact 项，项面求交发生在语义层；
//   * 嵌入**别名** ⇒ exact 项给**别名条目本身**（不是它的目标）；
//   * 嵌入预声明的 `comparable` ⇒ exact 项；
//   * 嵌入**别名指向的非接口类型**（`A2 = int`）⇒ exact 项；
//   * 嵌套（`interface{ D }`，D 自己嵌 J）⇒ 方法**透传**拍平，项仍给 D。

type J interface {
	M()
}

type D interface {
	J
}

type C interface {
	~int | ~string
}

type E interface {
	C
}

type F interface {
	comparable
}

type A = J

type DA interface {
	A
}

type A2 = int

type DA2 interface {
	A2
}

type G interface {
	J
	Extra2()
}

type N interface {
	D
}

type H interface {
	J
	DA
}
