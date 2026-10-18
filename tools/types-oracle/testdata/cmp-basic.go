// cmp fixture —— `//rgoc:cmp` 指令的**基准样本**（两侧读**同一份**，`D-M3-17`）。
//
// 语法（`M3-plan.md` Task 32 的落地形式；`M3-tests.md` §5.2 的修订 R-32-1）：
//
//	//rgoc:cmp identical <A> => <B>         A / B 是**类型表达式**
//	//rgoc:cmp representable <C> => <T>     C 是**常量表达式**，T 是**类型表达式**
//	//rgoc:cmp identical <A> <B>            ← §5.2 的示意形态（操作数无空格时可用）
//
// 判据是「先看有没有 `=>`」：类型表达式里本来就有空格（`chan int`、
// `struct{ x int }`、`interface{ String() string }`），**带空格时只能**用 `=>`；
// 没有 `=>` 就按空白切成**恰好两段**，段数不对就响亮报错。两种拼写不会互相干扰。
//
// 两侧输出 `<行号>\t<kind>\t<result>`（`result ∈ true|false|reject`）：
// 行号是**物理**行号（不套 `//line` 重映射）—— 两侧必须用同一套坐标。
package p

type MyInt int

type S struct{ v int }

func (S) String() string { return "" }

// Num 是**约束接口**（含项集）—— 请求它的同一性要**显式拒绝**（`U-M3-05`）。
type Num interface {
	~int | ~float64
}

type Stringer interface {
	String() string
}

// ── identical：类型同一性（`T-TY-92`）─────────────────────────────────────
//rgoc:cmp identical int => int
//rgoc:cmp identical int => MyInt
//rgoc:cmp identical MyInt => MyInt
//rgoc:cmp identical *int => *MyInt
//rgoc:cmp identical []int => []MyInt
//rgoc:cmp identical [4]int => [4]int
//rgoc:cmp identical [4]int => [5]int
//rgoc:cmp identical map[string]int => map[string]int
//rgoc:cmp identical map[string]int => map[int]string
//rgoc:cmp identical chan int => chan int
//rgoc:cmp identical chan int => <-chan int
//rgoc:cmp identical func(int) string => func(int) string
//rgoc:cmp identical func(int) string => func(int) error
//rgoc:cmp identical func(x int) string => func(y int) string
//rgoc:cmp identical struct{ x int } => struct{ x int }
//rgoc:cmp identical struct{ x int } => struct{ y int }
//rgoc:cmp identical struct{ x int } => struct{ x int; y int }
//rgoc:cmp identical interface{ String() string } => Stringer
//rgoc:cmp identical S => Stringer
//rgoc:cmp identical any => interface{}
//rgoc:cmp identical any => any
//rgoc:cmp identical error => error
//rgoc:cmp identical comparable => comparable
//rgoc:cmp identical Num => Num
//rgoc:cmp identical Num => Stringer

// ── 匿名约束接口：M4 **按类型集判定**（`U-M3-05` 由 M4/Task 10 解除）────────
// 两侧都是匿名约束接口（含项集 / `comparable`）时按官方 `identical` 判：
// `comparable` 位相同 + 项集（`termlist.equal`，双向 `subsetOf` 的集合语义）
// + 方法集（顺序无关）。具名约束接口（Num）走具名分支（见上）。
//
// 下面三条：同项集 ⇒ true、异项集 ⇒ false、同 `comparable` ⇒ true。
// （M3 曾在这三行挂 `// rgoc:expect-reject` 排除标记；解除后夹具里不再有
// 标记 —— 排除集机制保留在 `collide_cmp.rs`，供后续 kind 的拒面复用，
// 标记语法改为自带**期望代号**。）
//
//rgoc:cmp identical interface{ ~int } => interface{ ~int }
//
//rgoc:cmp identical interface{ ~int } => interface{ ~float64 }
//
//rgoc:cmp identical interface{ comparable } => interface{ comparable }

// ── 空白拼写（§5.2 的示意形态；与 `=>` 同一条判据）────────────────────────
// 操作数里**没有空格**时可用；带空格的表达式必须回到 `=>` 形态。
//rgoc:cmp identical int int
//rgoc:cmp identical int MyInt
//rgoc:cmp representable 1<<100 int8
