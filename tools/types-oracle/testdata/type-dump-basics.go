package p

// 25 个预声明基本类型各来一个变量 —— C7 §4.1 的 `basic名` 拼写表。
// `byte`/`rune` 是**同一个 `*Basic`**（实测），dump 成 `uint8`/`int32`；
// `error` 是 `Named`，走 `named` 标签。
var vBool bool
var vInt int
var vInt8 int8
var vInt16 int16
var vInt32 int32
var vInt64 int64
var vUint uint
var vUint8 uint8
var vUint16 uint16
var vUint32 uint32
var vUint64 uint64
var vUintptr uintptr
var vFloat32 float32
var vFloat64 float64
var vComplex64 complex64
var vComplex128 complex128
var vString string

var vByte byte
var vRune rune
var vError error
