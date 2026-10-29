// Package reflect 实现运行时反射（M12 b 线）。
//
// # 数据从哪来（C13 §5.1/§5.2/§5.4）
//
// 编译器在 `.rodata` 里为每个用到的类型发一块 **TypeDesc**（40 字节本体，
// `C5` §10；+14 的 `meta_idx` 指向 `ReflectMeta`），具名类型与 struct 后面
// 还跟着一块 **ReflectMeta**（名字 / 包路径 / 字段表）。`any` 的运行期表示
// 是两个字：`{动态类型的 *TypeDesc, 数据指针}` —— 所以 `TypeOf` 只需要
// 把第一字抽出来包成 `rtype`，不需要运行时做任何类型计算。
//
// 本文件里的 `descFull` / `reflectMeta` / `fieldMeta` 是那三块数据的
// **Go 侧镜像**：字段序与偏移必须与 `rgoc-meta::desc` 和 rt.c 的 C 结构
// 逐字段一致（两侧都有 const assert 钉着字节；这里靠 T-REF-02 对账）。
//
// # 面是刻意收窄的
//
// 导出符号集合由 `t_ref_01_白名单对账` **冻结**（相等，不是包含）：
// `MakeFunc` 不做（U-M12-02，需要反向 trampoline）。后续 Task 每加一个
// 导出符号都要同时改那张表 —— 露头就红。
package reflect

import "unsafe"

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

// descFull 是 TypeDesc 的 Go 侧镜像（C13 §5.1 的 40 字节本体 + §5.4 的
// 扩展 quad）。**偏移是契约**：
//
//	+0 size / +4 ptrdata / +8 hash / +12 kind / +13 flags / +14 meta_idx
//	+16 gcdata / +24 elem / +32 methods
//	+40 ext0 / +48 ext1 / +56 ext2   ← §5.4 扩展区（按 kind 判读）
//
// 扩展区的形状（发射侧 `rgoc-arm64::module::emit_descs`）：
//
//	map   → key, elem, [meta]        （named 时多一个 meta）
//	chan  → elem, [meta]
//	array → len, [elem, meta]        （匿名 array 的 elem 在本体 +24）
//	named ptr/slice → elem, meta
//	struct / named 其它 → meta
//
// `ext*` 一律声明成 `unsafe.Pointer`：array 的 len 读 `uintptr(ext0)`
// 就行（`uintptr(p)` 方向的转换是安全的），反过来 `unsafe.Pointer(uintptr)`
// 方向今天有 bug（解引用丢失，实测 2026-10-28）—— 绕开，不依赖。
type descFull struct {
	size    uint32         // +0
	ptrdata uint32         // +4
	hash    uint32         // +8
	kind    uint8          // +12
	flags   uint8          // +13
	metaIdx uint16         // +14
	gcdata  unsafe.Pointer // +16
	elem    unsafe.Pointer // +24
	methods unsafe.Pointer // +32
	ext0    unsafe.Pointer // +40
	ext1    unsafe.Pointer // +48
	ext2    unsafe.Pointer // +56
}

// reflectMeta 是 ReflectMeta 的 Go 侧镜像（C13 §5.2，72 字节）。
type reflectMeta struct {
	name     unsafe.Pointer // +0  cstring
	pkgpath  unsafe.Pointer // +8  cstring
	fields   unsafe.Pointer // +16 *fieldMeta
	nfields  uint32         // +24
	nmethods uint32         // +28
	methods  unsafe.Pointer // +32
	params   unsafe.Pointer // +40 *paramMeta
	nparams  uint32         // +48
	nresults uint32         // +52
	results  unsafe.Pointer // +56 *paramMeta
	tramp    unsafe.Pointer // +64 Call 的 ABI 桥入口（非签名类型为 nil）
}

// paramMeta 是 ParamMeta 的 Go 侧镜像（C13 §5.2，16 字节）。
//
// `cls` 是编译期算好的 ABI 类（0 = `x` / 1 = `v` / 2 = 栈）：`Call` 的
// 重放在 `__reflecttramp_*` 里（发射侧生成的直线代码），Go 侧**不读**它 ——
// 留在这里是为了让「两侧同形」这条契约有第二个读者（对账用）。
type paramMeta struct {
	typ unsafe.Pointer // +0 *TypeDesc
	cls uint8          // +8 ABI 类
	// +9..16 填充
}

// fieldMeta 是 FieldMeta 的 Go 侧镜像（C13 §5.2，32 字节）。
type fieldMeta struct {
	name   unsafe.Pointer // +0  cstring
	tag    unsafe.Pointer // +8  cstring
	typ    unsafe.Pointer // +16 *TypeDesc
	offset uint32         // +24
	flags  uint8          // +28（bit0 anonymous / bit1 exported）
}

// methodMeta 是 MethodMeta 的 Go 侧镜像（C13 §5.2，24 字节）。
//
// `sig` 指向**签名描述块**（kind=Func 的 40 字节 desc，发射面按
// `sig_hash` 编码发、跨方法去重）——`Implements` 比签名就比它的
// `hash` 字段（desc +8；两侧同编码，`iface_sig_hash` / `sig_hash`）。
type methodMeta struct {
	name unsafe.Pointer // +0  cstring
	sig  unsafe.Pointer // +8  *TypeDesc（签名 desc）
	fn   unsafe.Pointer // +16 方法入口（接口方法是 0）
}

// cstr 把 NUL 结尾的 C 字符串读成 Go 字符串。
//
// `(*[256]byte)(p)` 只建视图不碰内存（越界页只在索引时才触碰），循环
// 在 NUL 处停 —— 没有运行时 strlen 可用。
func cstr(p unsafe.Pointer) string {
	if p == nil {
		return ""
	}
	b := (*[256]byte)(p)
	s := ""
	for i := 0; i < 256; i++ {
		if b[i] == 0 {
			break
		}
		s = s + string(b[i])
	}
	return s
}

// kindNames 是预声明类型的名字（官方 `Type.Name()` 语义：**预声明类型**
// 的名字就是 kind 名 —— `int` / `string` / `bool`；复合形状没有名字）。
func kindName(k Kind) string {
	switch k {
	case Bool:
		return "bool"
	case Int:
		return "int"
	case Int8:
		return "int8"
	case Int16:
		return "int16"
	case Int32:
		return "int32"
	case Int64:
		return "int64"
	case Uint:
		return "uint"
	case Uint8:
		return "uint8"
	case Uint16:
		return "uint16"
	case Uint32:
		return "uint32"
	case Uint64:
		return "uint64"
	case Uintptr:
		return "uintptr"
	case Float32:
		return "float32"
	case Float64:
		return "float64"
	case Complex64:
		return "complex64"
	case Complex128:
		return "complex128"
	case String:
		return "string"
	case UnsafePointer:
		return "unsafe.Pointer"
	}
	return ""
}

// Type 是类型元数据的**只读**视图。
type Type interface {
	// Kind 返回本类型的种类。
	Kind() Kind
	// Size 返回本类型的值的大小（字节）。
	Size() uintptr
	// Align 返回本类型的值在内存里的对齐（字节）。
	Align() int
	// Name 返回具名类型的名字（无名类型返回空串；预声明类型是 kind 名）。
	Name() string
	// PkgPath 返回具名类型的定义包路径（内建与无名类型返回空串）。
	PkgPath() string
	// String 返回类型的可读描述（官方 `reflect.Type.String` 的形态，
	// 如 `main.Box[int]`）。完整口径在 Task 32 对账（T-REF-05）。
	String() string
	// NumField 返回 struct 的字段数（非 struct panic，文本对齐官方）。
	NumField() int
	// Field 返回第 i 个字段（越界 / 非 struct panic，文本对齐官方）。
	Field(i int) StructField
	// FieldByName 按名字找字段；找不到返回零值与 false（官方同）。
	FieldByName(name string) (StructField, bool)
	// Len 返回数组的长度（非 array panic，文本对齐官方）。
	Len() int
	// Key 返回 map 的键类型（非 map panic，文本对齐官方）。
	Key() Type
	// Elem 返回元素类型（ptr/slice/array/chan/map；其余 panic）。
	Elem() Type
	// NumMethod 返回**导出**方法的个数（官方同口径：非导出不计）。
	NumMethod() int
	// Method 返回第 i 个导出方法（字母序；越界 panic，文本对齐官方）。
	Method(i int) Method
	// Implements 报告本类型是否实现接口 u（u 非接口恒 false，官方同）。
	Implements(u Type) bool
	// AssignableTo 报告本类型的值是否可赋给 u（同一类型或 u 是已实现的接口）。
	AssignableTo(u Type) bool
	// ConvertibleTo 报告本类型的值是否可转换成 u（可赋值或两侧都是数值类）。
	ConvertibleTo(u Type) bool
}

// Method 是类型单个方法的元数据（官方同名结构的收窄版：
// `Func`（方法值）需要反向 trampoline（U-M12-02 同族），`Index` 由调用方
// 自己数 —— 都不进收窄面）。
type Method struct {
	// Name 是方法名（不含类型前缀）。
	Name string
	// Type 是方法的签名类型（kind 是 Func；**不含接收者**，官方同）。
	Type Type
}

// StructTag 是字段 tag（字段声明里反引号里的原文）。
type StructTag string

// StructField 是 struct 单个字段的元数据（官方同名结构的收窄版）。
type StructField struct {
	// Name 是字段名（匿名字段是类型名）。
	Name string
	// Tag 是字段 tag（没有是空串）。
	Tag StructTag
	// Offset 是字段在 struct 里的字节偏移。
	Offset uintptr
	// Type 是字段的类型。
	Type Type
}

// rtype 是 Type 的实现者：包着动态类型的 TypeDesc 指针。
type rtype struct{ desc unsafe.Pointer }

// d 返回镜像视图（nil 安全）。
func (t rtype) d() *descFull {
	if t.desc == nil {
		return nil
	}
	return (*descFull)(t.desc)
}

// meta 返回本类型的 ReflectMeta（没有则 nil）。
//
// 扩展区的 meta 位置按 kind 走（见 descFull 的注释）：struct / named 基本类型
// 在 ext0；named ptr/slice/chan 在 ext1；named map/array 在 ext2。发射侧
// `needs_reflect_meta` 保证 Named|Struct 必有 meta。
func (t rtype) meta() *reflectMeta {
	d := t.d()
	if d == nil || d.metaIdx == 0 {
		return nil
	}
	k := Kind(d.kind)
	switch k {
	case Struct:
		return (*reflectMeta)(d.ext0)
	case Map, Array:
		return (*reflectMeta)(d.ext2)
	case Pointer, Slice, Chan:
		return (*reflectMeta)(d.ext1)
	}
	return (*reflectMeta)(d.ext0)
}

// Kind 返回本类型的种类。
func (t rtype) Kind() Kind {
	d := t.d()
	if d == nil {
		return Invalid
	}
	return Kind(d.kind)
}

// Size 返回本类型的值的大小（字节）。
func (t rtype) Size() uintptr {
	d := t.d()
	if d == nil {
		return 0
	}
	return uintptr(d.size)
}

// Align 返回本类型的值在内存里的对齐（字节）。
//
// TypeDesc 里没有对齐字段（40 字节冻结）—— 按 kind 推：基本类型对齐就是
// size（≤8；string/complex128 这类 16 字节的按 8）；指针/字符串/接口/函数/
// map/chan/slice 都是 8；数组是元素的对齐；struct 是字段对齐的最大值
// （字段表递归）。已知缺口（G-M12-04）：named array 的元素对齐要从扩展区
// 的 elem desc 递归 —— 今天的语料面里没出现偏差形态，先按数组处理。
func (t rtype) Align() int {
	d := t.d()
	if d == nil {
		return 0
	}
	k := Kind(d.kind)
	switch k {
	case Bool, Int8, Uint8:
		return 1
	case Int16, Uint16:
		return 2
	case Int32, Uint32, Float32:
		return 4
	case Array:
		e := t.elemDesc()
		if e == nil {
			return 8
		}
		return (rtype{desc: e}).Align()
	case Struct:
		m := t.meta()
		if m == nil || m.nfields == 0 {
			return 1
		}
		fm := (*[64]fieldMeta)(m.fields)
		a := 1
		for i := 0; i < int(m.nfields); i++ {
			if fa := (rtype{desc: fm[i].typ}).Align(); fa > a {
				a = fa
			}
		}
		return a
	}
	return 8
}

// elemDesc 返回元素类型的 desc 指针（meta_idx 区分本体 elem 与扩展区 elem）。
func (t rtype) elemDesc() unsafe.Pointer {
	d := t.d()
	if d == nil {
		return nil
	}
	switch Kind(d.kind) {
	case Pointer, Slice:
		if d.metaIdx != 0 {
			return d.ext0
		}
		return d.elem
	case Array:
		if d.metaIdx != 0 {
			return d.ext1
		}
		return d.elem
	case Chan:
		return d.ext0
	case Map:
		return d.ext1
	}
	return nil
}

// Name 返回具名类型的名字（无名类型空串；预声明类型是 kind 名）。
func (t rtype) Name() string {
	if m := t.meta(); m != nil {
		return cstr(m.name)
	}
	return kindName(Kind(t.d().kind))
}

// PkgPath 返回具名类型的定义包路径（内建与无名类型空串）。
//
// 匿名 struct 也有 meta（`Field` 要用），但 `name` 为空 —— 官方对
// 未命名类型一律空串（`reflect.TypeOf(struct{}{}).PkgPath() == ""`），
// 不能把发射侧填的包路径漏出去（实测打出 "main"）。
func (t rtype) PkgPath() string {
	if m := t.meta(); m != nil && cstr(m.name) != "" {
		return cstr(m.pkgpath)
	}
	return ""
}

// String 返回类型的可读描述（官方 `reflect.Type.String` 形态）。
//
// 具名类型 `pkgpath.name`（实例的 name 字段本身就是 `Box[int]` 形，
// 发射面渲染）；复合形状沿 elem 链递归：`*T` / `[]T` / `[N]T` /
// `map[K]V` / `chan T`。其余（func / iface / struct 的完整官方拼写）
// 不在收窄面（`U-M12` 族），退 kind 名。
func (t rtype) String() string {
	d := t.d()
	if d == nil {
		return "<nil>"
	}
	switch Kind(d.kind) {
	case Pointer:
		return "*" + (rtype{desc: t.elemDesc()}).String()
	case Slice:
		return "[]" + (rtype{desc: t.elemDesc()}).String()
	case Array:
		return "[" + itoa(t.Len()) + "]" + (rtype{desc: t.elemDesc()}).String()
	case Map:
		return "map[" + (rtype{desc: d.ext0}).String() + "]" + (rtype{desc: d.ext1}).String()
	case Chan:
		return "chan " + (rtype{desc: d.ext0}).String()
	}
	if m := t.meta(); m != nil {
		n := cstr(m.name)
		p := cstr(m.pkgpath)
		if p != "" {
			return p + "." + n
		}
		return n
	}
	return kindName(Kind(d.kind))
}

// itoa 是十进制无符号渲染（`Array` 的 len；runtime 没有 strconv）。
func itoa(n int) string {
	if n == 0 {
		return "0"
	}
	digits := ""
	for n > 0 {
		digits = string(rune('0'+n%10)) + digits
		n /= 10
	}
	return digits
}

// methodsOf 返回方法表视图（没有 meta / 没有方法返回 nil）。
func (t rtype) methodsOf() *[64]methodMeta {
	m := t.meta()
	if m == nil || m.methods == nil || m.nmethods == 0 {
		return nil
	}
	return (*[64]methodMeta)(m.methods)
}

// exportedMethods 返回**导出**方法的下标表（官方 `NumMethod`/`Method`
// 的口径：非导出方法不进反射的方法列表 —— 但 `Implements` 看全集）。
func (t rtype) exportedMethods() []int {
	mm := t.methodsOf()
	if mm == nil {
		return nil
	}
	var out []int
	for i := 0; i < int(t.meta().nmethods); i++ {
		if c := cstr(mm[i].name); c != "" && c[0] >= 'A' && c[0] <= 'Z' {
			out = append(out, i)
		}
	}
	return out
}

// NumMethod 返回导出方法的个数。
func (t rtype) NumMethod() int {
	return len(t.exportedMethods())
}

// Method 返回第 i 个导出方法（字母序 —— 发射面按名字升序发）。
func (t rtype) Method(i int) Method {
	exp := t.exportedMethods()
	if i < 0 || i >= len(exp) {
		panic("reflect: Method index out of range")
	}
	mm := t.methodsOf()[exp[i]]
	return Method{Name: cstr(mm.name), Type: rtype{desc: mm.sig}}
}

// Implements 报告本类型是否实现接口 u。
//
// 判据：u 的**每个**方法都能在本类型的方法表里找到同名且**签名 desc 的
// hash 相同**的条目（两侧同走 `sig_hash` 编码 —— desc 块的 +8 就是它）。
// u 非接口、或 u 是没有方法表的接口形状 ⇒ false（官方同）。
func (t rtype) Implements(u Type) bool {
	ut, ok := u.(rtype)
	if !ok {
		return false
	}
	ud := ut.d()
	if ud == nil || Kind(ud.kind) != Interface {
		return false
	}
	um := ut.methodsOf()
	if um == nil {
		// 没有方法的接口（`interface{}`）—— 谁都实现。
		return true
	}
	mm := t.methodsOf()
	if mm == nil {
		return false
	}
	n := int(ut.meta().nmethods)
	for i := 0; i < n; i++ {
		want := cstr(um[i].name)
		wantSig := (rtype{desc: um[i].sig}).d()
		found := false
		for j := 0; j < int(t.meta().nmethods); j++ {
			if cstr(mm[j].name) != want {
				continue
			}
			got := (rtype{desc: mm[j].sig}).d()
			if got != nil && wantSig != nil && got.hash == wantSig.hash {
				found = true
				break
			}
		}
		if !found {
			return false
		}
	}
	return true
}

// AssignableTo 报告本类型的值是否可赋给 u：同一类型（desc 同一块），
// 或 u 是已实现的接口（官方口径的收窄面 —— 常量可表示性、双向 chan
// 等不在语料里，不判）。
func (t rtype) AssignableTo(u Type) bool {
	ut, ok := u.(rtype)
	if !ok {
		return false
	}
	ud := ut.d()
	if ud == nil || t.d() == nil {
		return false
	}
	if Kind(ud.kind) == Interface {
		return t.Implements(u)
	}
	return t.d().hash == ud.hash
}

// ConvertibleTo 报告本类型的值是否可转换成 u：可赋值，或两侧都是
// 数值类（bool/string 之外的标量 kind 区间，官方 §转换 的收窄面 ——
// string⇄[]byte、struct 同形等不在 t_ref_05 的对账范围里，不判）。
func (t rtype) ConvertibleTo(u Type) bool {
	if t.AssignableTo(u) {
		return true
	}
	ut, ok := u.(rtype)
	if !ok {
		return false
	}
	ud := ut.d()
	d := t.d()
	if ud == nil || d == nil {
		return false
	}
	return numericKind(Kind(d.kind)) && numericKind(Kind(ud.kind))
}

// numericKind 判「数值类 kind」（整数 / 浮点 / 复数；bool/string/
// unsafe.Pointer 不算 —— string(i) 的码点转换官方也不是「数值转换」）。
func numericKind(k Kind) bool {
	return k >= Int && k <= Complex128
}

// NumField 返回 struct 的字段数（非 struct panic，文本对齐官方）。
func (t rtype) NumField() int {
	if Kind(t.d().kind) != Struct {
		panic("reflect: NumField of non-struct type")
	}
	m := t.meta()
	if m == nil {
		return 0
	}
	return int(m.nfields)
}

// fieldsOf 返回字段表视图（非 struct 返回 nil）。
func (t rtype) fieldsOf() *[64]fieldMeta {
	m := t.meta()
	if m == nil || m.fields == nil {
		return nil
	}
	return (*[64]fieldMeta)(m.fields)
}

// field 返回第 i 个字段的 StructField（越界 / 非 struct 由调用方 panic）。
func (t rtype) field(i int) StructField {
	fm := t.fieldsOf()[i]
	return StructField{
		Name:   cstr(fm.name),
		Tag:    StructTag(cstr(fm.tag)),
		Offset: uintptr(fm.offset),
		Type:   rtype{desc: fm.typ},
	}
}

// Field 返回第 i 个字段（非 struct / 越界 panic，文本对齐官方）。
func (t rtype) Field(i int) StructField {
	if Kind(t.d().kind) != Struct {
		panic("reflect: Field of non-struct type")
	}
	if i < 0 || i >= t.NumField() {
		panic("reflect: Field index out of range")
	}
	return t.field(i)
}

// FieldByName 按名字找字段；找不到返回零值与 false（官方同）。
func (t rtype) FieldByName(name string) (StructField, bool) {
	if Kind(t.d().kind) != Struct {
		var z StructField
		return z, false
	}
	n := t.NumField()
	for i := 0; i < n; i++ {
		f := t.field(i)
		if f.Name == name {
			return f, true
		}
	}
	var z StructField
	return z, false
}

// Len 返回数组的长度（非 array panic，文本对齐官方）。
func (t rtype) Len() int {
	d := t.d()
	if Kind(d.kind) != Array {
		panic("reflect: Len of non-array type")
	}
	return int(uintptr(d.ext0))
}

// Key 返回 map 的键类型（非 map panic，文本对齐官方）。
func (t rtype) Key() Type {
	d := t.d()
	if Kind(d.kind) != Map {
		panic("reflect: Key of non-map type")
	}
	return rtype{desc: d.ext0}
}

// Elem 返回元素类型（ptr/slice/array/chan/map；其余 panic）。
func (t rtype) Elem() Type {
	d := t.d()
	switch Kind(d.kind) {
	case Pointer, Slice, Array, Chan, Map:
		p := t.elemDesc()
		if p == nil {
			panic("reflect: Elem of invalid type")
		}
		return rtype{desc: p}
	}
	panic("reflect: Elem of invalid type")
}

// TypeOf 返回 i 的动态类型（i 为 nil 接口时返回 nil）。
//
// `any` 的两个字是 `{*TypeDesc, 数据指针}`（isel 的装箱口径）—— 第一字
// 抽出来就是答案，没有运行时计算。
func TypeOf(i any) Type {
	w := (*[2]unsafe.Pointer)(unsafe.Pointer(&i))
	if w[0] == nil {
		var z Type
		return z
	}
	return rtype{desc: w[0]}
}

// ValueOf 返回 i 的 Value 视图。
//
// 读面（Task 31）只需要 typ；ptr（数据指针）被写面（Task 34）与 `Call`
// （Task 33）用。
func ValueOf(i any) Value {
	w := (*[2]unsafe.Pointer)(unsafe.Pointer(&i))
	return Value{typ: rtype{desc: w[0]}, ptr: w[1]}
}

// Value 是值的视图（读面 + `Call` + 后续的写面）。
type Value struct {
	// typ 是值的动态类型。
	typ Type
	// ptr 是值的数据指针（装箱值的存储地址）。
	//
	// ⚠️ 用 `unsafe.Pointer` 而不是 `uintptr`：`Call` 要把它交给
	// `copyBytes` / 记进结果的 `Value`，而 `uintptr → unsafe.Pointer`
	// 这个方向在本仓今天的代码生成上有缺口（解引用丢失，实测
	// 2026-10-28）。存指针就没有这个转换。
	ptr unsafe.Pointer
}

// ────────────────────────────── Call ──────────────────────────────

// callArena 是 `Call` 的参数 / 结果缓冲（`C13` §5.5）。
//
// 为什么是**包级数组**：`Value` 只存裸指针，而返回给调用方的结果值指向
// 缓冲里的位置 —— 局部切片一回收，那些指针就是悬空的。包级数组天然是
// GC 根，也不依赖分配器（解释器缺口 `G-M12-05` 因此不在这条路上）。
var callArena [65536]byte

// callUsed 是本次顶层 `Call` 已用掉的字节数。
var callUsed int

// callDepth 是嵌套深度：顶层调用从 0 开始复用，嵌套调用往后长 ——
// 否则「被调函数里再 `reflect.Value.Call` 一次」会把外层正在用的缓冲踩掉。
var callDepth int

// callSpace 从 `callArena` 里切 n 字节（16 对齐）。`n` 至少 8。
func callSpace(n int) unsafe.Pointer {
	if n < 8 {
		n = 8
	}
	if callUsed+n > len(callArena) {
		panic("reflect: Call 的缓冲用尽（callArena）")
	}
	p := unsafe.Pointer(&callArena[callUsed])
	callUsed += (n + 15) &^ 15
	return p
}

// slotBytes 返回缓冲里一个形参 / 结果占的字节数。
//
// 与发射侧 `emit_reflect_trampoline` 的 `buf_slot` **同一个公式**：
// `max(8, round8(size))`。两侧一旦分叉，实参就整体错位。
func slotBytes(d *descFull) int {
	n := int(d.size)
	n = (n + 7) &^ 7
	if n < 8 {
		n = 8
	}
	return n
}

// copyBytes 拷 n 个字节（Go 侧没有 memcpy 内建）。
func copyBytes(dst unsafe.Pointer, src unsafe.Pointer, n int) {
	if n <= 0 || dst == nil || src == nil {
		return
	}
	d := (*[1 << 24]byte)(dst)
	s := (*[1 << 24]byte)(src)
	for i := 0; i < n; i++ {
		d[i] = s[i]
	}
}

// Call 用 args 调用函数值 v，返回结果值。
//
// 收窄面（`C13` §5.5 / T-REF-03）：实参**个数**必须与签名一致（官方同）；
// 类型不逐一对账 —— 静态类型已由调用方保证。
//
// 通路：实参按 `slotBytes` 摆进缓冲 → `rt_reflect_call(funcval, tramp,
// args, results)` → 发射侧生成的 `__reflecttramp_<hash>` 按 AAPCS64 装
// 寄存器、`blr`、再把结果搬回缓冲。
func (v Value) Call(args []Value) []Value {
	if v.typ == nil {
		panic("reflect: call of nil Value")
	}
	t := v.typ.(rtype)
	d := t.d()
	if d == nil || Kind(d.kind) != Func {
		panic("reflect: call of non-function Value")
	}
	m := t.meta()
	if m == nil || m.tramp == nil {
		panic("reflect: call of unimplemented function type")
	}
	n := int(m.nparams)
	if len(args) != n {
		panic("reflect: wrong number of arguments in call")
	}
	nr := int(m.nresults)
	pm := (*[64]paramMeta)(m.params)
	rm := (*[64]paramMeta)(m.results)

	// 缓冲：先算总长，再一次性切（顶层调用会把水位归零）。
	argBytes := 0
	for i := 0; i < n; i++ {
		argBytes += slotBytes((*descFull)(pm[i].typ))
	}
	resBytes := 0
	for i := 0; i < nr; i++ {
		resBytes += slotBytes((*descFull)(rm[i].typ))
	}
	if callDepth == 0 {
		callUsed = 0
	}
	callDepth++
	argp := callSpace(argBytes)
	resp := callSpace(resBytes)

	// 实参落缓冲（`d.size` 个字节；零大小的只占位不拷）。
	//
	// ⚠️ **指针类不拷，直接写那个字**：`box_iface` 对指针有 `is_ptr` 特例
	// （不装箱，`any` 的 data 字就是指针本身）⇒ `Value.ptr` 指的是**目标
	// 对象**而不是装着指针的槽。照 `size` 字节去拷会拷成「被指向的内容」，
	// 于是被调方拿 `5` 当地址解引用 —— 实测 SIGSEGV。
	ab := (*[1 << 24]byte)(argp)
	off := 0
	for i := 0; i < n; i++ {
		pd := (*descFull)(pm[i].typ)
		if Kind(pd.kind) == Pointer {
			(*[1]*byte)(unsafe.Pointer(&ab[off]))[0] = (*byte)(args[i].ptr)
		} else if pd.size > 0 {
			copyBytes(unsafe.Pointer(&ab[off]), args[i].ptr, int(pd.size))
		}
		off += slotBytes(pd)
	}

	// 函数值槽里存的是**闭包块**的地址（与 `CalleeRef::Indirect` 的
	// `ldr x16, [x0]` 同口径）。
	//
	// ⚠️ 不写 `*(*unsafe.Pointer)(v.ptr)`：`*unsafe.Pointer` 这种「跨包
	// 类型的指针」还在单包窄面里（lowering 只认本包复合类型）。`*[1]*byte`
	// 的底座是内建类型，走得通。
	fn := unsafe.Pointer((*[1]*byte)(v.ptr)[0])
	if fn == nil {
		callDepth--
		panic("reflect: call of nil function")
	}
	rt_reflect_call(fn, m.tramp, argp, resp)
	callDepth--

	// 结果：逐个包成 Value（指向结果缓冲 —— 缓冲是包级数组，不会回收）。
	// 结果：逐个包成 Value。
	//
	// 指针类**同一个口径**（`box_iface` 的 `is_ptr` 臂）：`Value.ptr` 是
	// 指针本身，不是「装着指针的槽」—— 这样结果可以直接当下一轮的实参。
	out := make([]Value, nr)
	rb := (*[1 << 24]byte)(resp)
	roff := 0
	for i := 0; i < nr; i++ {
		rd := (*descFull)(rm[i].typ)
		if Kind(rd.kind) == Pointer {
			w := (*[1]*byte)(unsafe.Pointer(&rb[roff]))
			out[i] = Value{typ: rtype{desc: rm[i].typ}, ptr: unsafe.Pointer(w[0])}
		} else {
			out[i] = Value{typ: rtype{desc: rm[i].typ}, ptr: unsafe.Pointer(&rb[roff])}
		}
		roff += slotBytes(rd)
	}
	return out
}

// ────────────────────── Value 的读访问器 ──────────────────────
//
// `Call` 的结果是 `Value`，没有这些访问器就**没法观察**（判据 T-REF-03
// 的九档全都要把结果打出来）。面按官方同名方法收窄：只读、只认对应
// kind（官方对不匹配的 kind 也 panic）。

// descOf 取 v 的类型 desc（零 Value 返回 nil）。
func (v Value) descOf() *descFull {
	if v.typ == nil {
		return nil
	}
	return v.typ.(rtype).d()
}

// Kind 返回 v 的动态类型种类（零 Value 是 `Invalid`）。
func (v Value) Kind() Kind {
	d := v.descOf()
	if d == nil {
		return Invalid
	}
	return Kind(d.kind)
}

// Type 返回 v 的动态类型（零 Value 返回 nil）。
func (v Value) Type() Type {
	return v.typ
}

// Int 返回有符号整数（官方口径：kind 必须是整数类，否则 panic）。
func (v Value) Int() int64 {
	d := v.descOf()
	if d == nil {
		panic("reflect: call of reflect.Value.Int on zero Value")
	}
	switch Kind(d.kind) {
	case Int, Int8, Int16, Int32, Int64:
		return readInt(v.ptr, int(d.size))
	}
	panic("reflect: call of reflect.Value.Int on " + kindName(Kind(d.kind)) + " Value")
}

// readInt 按 `size` 字节小端读一个整数并做符号扩展。
func readInt(p unsafe.Pointer, size int) int64 {
	b := (*[8]byte)(p)
	var u uint64
	for i := 0; i < size && i < 8; i++ {
		u |= uint64(b[i]) << uint(8*i)
	}
	if size > 0 && size < 8 {
		sh := uint(64 - 8*size)
		return int64(u<<sh) >> sh
	}
	return int64(u)
}

// Float 返回浮点值（官方口径：kind 必须是浮点类，否则 panic）。
func (v Value) Float() float64 {
	d := v.descOf()
	if d == nil {
		panic("reflect: call of reflect.Value.Float on zero Value")
	}
	if Kind(d.kind) == Float32 {
		return float64(*(*float32)(v.ptr))
	}
	if Kind(d.kind) == Float64 {
		return *(*float64)(v.ptr)
	}
	panic("reflect: call of reflect.Value.Float on " + kindName(Kind(d.kind)) + " Value")
}

// Bool 返回布尔值（官方口径：kind 必须是 bool，否则 panic）。
func (v Value) Bool() bool {
	d := v.descOf()
	if d == nil {
		panic("reflect: call of reflect.Value.Bool on zero Value")
	}
	if Kind(d.kind) != Bool {
		panic("reflect: call of reflect.Value.Bool on " + kindName(Kind(d.kind)) + " Value")
	}
	return (*[1]byte)(v.ptr)[0] != 0
}
