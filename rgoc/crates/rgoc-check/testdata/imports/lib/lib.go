// Package lib 是 `T-CK-74/75` 的**被导入包**（M4/Task 35）。
//
// 它故意同时含导出与非导出名字：`pkg.X` 的可见性判定要有两侧样本。
// 与 `.superpowers/sdd/M4-plan/probe/t35/gopath/src/lib/lib.go` **逐字节一致**
// —— 探针（带源码 importer 的 go/types）就是拿它量出官方口径的。
package lib

// Exported 是导出常量。
const Exported = 1

// hidden 是非导出常量。
const hidden = 2

// T 是导出类型。
type T struct {
	F int
	f int
}

// M 是导出类型 T 的导出方法。
func (t T) M() int { return t.F }

// m 是 T 的非导出方法。
func (t T) m() int { return t.f }

// V 是导出变量。
var V int

// F 是导出函数。
func F(x int) int { return x }

// 非导出函数。
func g() {}
