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

// ── 补记（Task 38 收口审计）：`T-TY-3a` 字符串拼接 / 复常量 **128 位** ────────
//
// 这一节是**事后补的**。收口审计发现：`M3-tests.md` §3 把 `T-TY-3a`（字符串常量
// 拼接）与 `T-TY-34`（复数常量，**含 128 位**）标成 `O`（oracle 对撞），但夹具
// 此前只有 64 位复数、**没有**字符串目标 —— 声明有、证据没有，正是 `AGENTS.md`
// §6.1 点名的「已检查的错觉」。补法沿用既有形态，且**追加在文件末尾** ⇒ 既有
// `.want` 的行号一律不变（只多出末尾这几行）。
//
// `Bool` 目标也一并钉住：官方 `representableConst` 判的**不止数值**种类
// （`Bool`/`String` 也在内），而此前没有任何一条夹具走过这两支。
type MyStr string

//rgoc:cmp representable "a"+"b" => string
//rgoc:cmp representable "a"+"b" => MyStr
//rgoc:cmp representable "a"+"c" => MyStr
//rgoc:cmp representable true => bool
//rgoc:cmp representable 1 => bool
//rgoc:cmp representable true => int
//rgoc:cmp representable 1i => complex128
//rgoc:cmp representable 1+2i => complex128
//rgoc:cmp representable 1e400i => complex128
