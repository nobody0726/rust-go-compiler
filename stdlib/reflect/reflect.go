// Package reflect 实现运行时反射（M12 b 线骨架，Task 29）。
//
// # 骨架期（Ruling 29-1）
//
// TypeOf / ValueOf 的函数体**暂时返回零值** —— 计划原文是「内部调
// rt_reflect_*（先 todo）」，但 Go 源码调 rt.c 函数的接线（runtime 侧
// 读 ReflectMeta 表）在 Task 30 才落位，骨架期没有可调的东西。
// 零值占位与 panic 占位的差别：前者让「import reflect + 摆调用形状」的
// 程序能过检查面，判据（T-REF-01）就能从 Task 29 起一直跑着；语义由
// T-REF-02..06 逐条接管。
//
// # 面是刻意收窄的
//
// 导出符号集合由 `t_ref_01_白名单对账` **冻结**（相等，不是包含）：
// `MakeFunc` 不做（U-M12-02，需要反向 trampoline）。后续 Task 每加一个
// 导出符号都要同时改那张表 —— 露头就红。
package reflect

// Kind 是类型的种类。**数值与官方 reflect.Kind 一致**（iota 序 ==
// 官方的枚举序）—— T-REF-02 的元数据对账要拿它和官方 `go run` 逐字段比。
type Kind uint8

// Kind 的全集（官方 reflect 包的枚举序，勿改顺序 —— iota 就是数值）。
const (
	Invalid Kind = iota // 0
	Bool                // 1
	Int                 // 2
	Int8                // 3
	Int16               // 4
	Int32               // 5
	Int64               // 6
	Uint                // 7
	Uint8               // 8
	Uint16              // 9
	Uint32              // 10
	Uint64              // 11
	Uintptr             // 12
	Float32             // 13
	Float64             // 14
	Complex64           // 15
	Complex128          // 16
	Array               // 17
	Chan                // 18
	Func                // 19
	Interface           // 20
	Map                 // 21
	Pointer             // 22
	Slice               // 23
	String              // 24
	Struct              // 25
	UnsafePointer       // 26
)

// Type 是类型元数据的**只读**视图。
//
// 骨架期只立形状；方法体在 Task 31（读面）/ Task 32（方法表与类型身份）
// 由运行时的 ReflectMeta 表接线（Task 30）。
type Type interface {
	// Kind 返回本类型的种类。
	Kind() Kind
	// Size 返回本类型的值的大小（字节）。
	Size() uintptr
	// Align 返回本类型的值在内存里的对齐（字节）。
	Align() int
	// Name 返回具名类型的名字（无名类型返回空串）。
	Name() string
	// PkgPath 返回具名类型的定义包路径（内建与无名类型返回空串）。
	PkgPath() string
	// String 返回类型的可读描述（官方 `reflect.Type.String` 的形态，
	// 如 `main.Box[int]`）。
	String() string
}

// Value 是值的视图（读面 + 后续的写面）。
//
// 字段是骨架占位：Task 30 起换成运行时 TypeDesc / ReflectMeta 的指针
// （字段语义随 Task 30 的 `meta_idx` 一起定，不在这里预填）。
type Value struct {
	typ Type
	ptr uintptr
}

// TypeOf 返回 i 的动态类型（i 为 nil 接口时返回 nil）。
//
// 骨架期恒返回 nil（见 Ruling 29-1）。
func TypeOf(i any) Type {
	var t Type
	_ = i
	return t
}

// ValueOf 返回 i 的 Value 视图。
//
// 骨架期恒返回零值（见 Ruling 29-1）。
func ValueOf(i any) Value {
	var v Value
	_ = i
	return v
}
