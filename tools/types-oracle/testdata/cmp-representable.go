// cmp fixture —— `representable` 的**边界**样本（`T-TY-41/43/44/48`）。
//
// 目标一律是**预声明基本类型**（或本体是基本类型的具名类型）：`M3-tests.md`
// §5.2 的口径 —— M3 只对基本类型给可表示性判定，其余目标不是「拒绝」而是
// **样本违规**（两边的 CLI 都要响亮报错）。
package p

// MyInt 本体是 `int` ⇒ 目标是它时按 `int` 判（官方同：判据取 Underlying 的 Basic）。
type MyInt int

// ── 整型边界（`T-TY-41/43`）───────────────────────────────────────────────
//rgoc:cmp representable 127 => int8
//rgoc:cmp representable 128 => int8
//rgoc:cmp representable -128 => int8
//rgoc:cmp representable -129 => int8
//rgoc:cmp representable 255 => uint8
//rgoc:cmp representable 256 => uint8
//rgoc:cmp representable 65535 => uint16
//rgoc:cmp representable 65536 => uint16
//rgoc:cmp representable 'a' => int8
//rgoc:cmp representable 'a' => uint8
//rgoc:cmp representable 1<<63-1 => int64
//rgoc:cmp representable 1<<63 => int64
//rgoc:cmp representable 1<<64-1 => uint64
//rgoc:cmp representable 1<<64 => uint64
//rgoc:cmp representable -1 => uint32
//rgoc:cmp representable 0 => MyInt

// ── 浮点舍入与溢出（`T-TY-44/45/48`）─────────────────────────────────────
//rgoc:cmp representable 0.1 => float32
//rgoc:cmp representable 1/3 => float64
//rgoc:cmp representable 1e400 => float32
//rgoc:cmp representable 1e-1000 => float32
//rgoc:cmp representable 0x1p-149 => float32
//rgoc:cmp representable 0x1p-150 => float32
//rgoc:cmp representable 0x1p128 => float32
//rgoc:cmp representable 1.5 => int
//rgoc:cmp representable 2.0 => int
// ⚠️ 浮点目标的判据**不是**「精确表示」，而是「可舍入且**不溢出**」
// （官方 `representableConst`）：`0.1`、`2^24+1` 都丢精度，但都算**可表示**。
// ⇒ 浮点目标下 `false` 只有一种成因：**溢出**。这一对里只有 3.5e38 是 false。
// 尾数中点（`float32` 尾数 24 位）
//rgoc:cmp representable 16777216 => float32
//rgoc:cmp representable 16777217 => float32
//rgoc:cmp representable -16777217 => float32
// 溢出分界（float32 最大约 3.4028235e38）
//rgoc:cmp representable 3.4e38 => float32
//rgoc:cmp representable 3.5e38 => float32
// float64 的同一对（尾数 53 位）
//rgoc:cmp representable 9007199254740992 => float64
//rgoc:cmp representable 9007199254740993 => float64

// ── 复常量（`T-TY-34` 邻域）──────────────────────────────────────────────
//rgoc:cmp representable 1i => complex64
//rgoc:cmp representable 1i => float64
//rgoc:cmp representable 1+2i => complex64
