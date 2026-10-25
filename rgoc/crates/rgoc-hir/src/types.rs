//! HIR 的数据形状（`M5-design.md` §3.2）。
//!
//! # 三条**偏离**设计文档的地方（都在这里逐条交代理由）
//!
//! 1. **`Expr` 是 `{ kind, typ }` 结构体，不是裸 enum。**
//!    设计文档写的是 `enum Expr`，但解释器每个节点都要问「你的静态类型是
//!    什么」（整数宽度、`print` 的格式、比较的分支、转换的源类型）。把这些
//!    分散成每个变体自己的字段，等于让 20 个变体各自维护一份同样的信息，
//!    且**漏填不会被编译器发现**（`typ` 只会退化成 `Invalid`）。
//!    包一层结构体后「每个表达式都有静态类型」变成**类型系统保证**。
//!    `kind` 的名字与设计文档的变体名一一对应，dump 也按 `kind` 走。
//! 2. **`Program::init_order` 是 `Vec<InitGroup>`。**
//!    官方 `InitOrder` 的元素是「**一个初始化器**」，而 `var a, b = f()` 是
//!    **一个**初始化器带两个变量（`rgoc_check::initorder::Initializer.names`）。
//!    写成 `Vec<GlobalId>` 会把这一组拆成两次求值 —— `f()` 被调两次，输出就错了。
//! 3. **多两个变体**：`Stmt::Empty`（`;` 与空 `case` 体）与 `Expr::Nil`
//!    （有类型 `nil`：`var p *T = nil` 的右侧必须有节点可指）。
//!
//! # 求值顺序在 HIR 里是显式的
//!
//! 这是「可执行」与「AST 换皮」的分界（`M5-design.md` §3.2 的设计原则）：
//!
//! | 构造 | HIR 里的顺序 |
//! |---|---|
//! | `a, b = x, y` | `Assign { rhs: [x, y] }` —— 解释器先全求值再逐个写 |
//! | `defer f(x)` | `Stmt::Defer { call }`，实参**已**是临时 slot 的读 |
//! | 复合字面量的 keyed 元素 | `CompositeItem::Field(下标, 值)` —— 求值期不查名 |
//! | 方法调用 | `Callee::Method { recv, func }` —— 静态派发在 lowering 期定死 |

use std::collections::BTreeMap;

use rgoc_types::TypeId;

/// 帧槽下标（形参 / 命名返回值 / 局部 / 临时）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct SlotId(pub u32);

impl SlotId {
    /// 取下标。
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// 函数下标（`Program::funcs` 的索引）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct FuncId(pub u32);

impl FuncId {
    /// 取下标。
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// 包级变量下标（`Program::globals` 的索引）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct GlobalId(pub u32);

impl GlobalId {
    /// 取下标。
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// 一个可执行的程序（单包）。
#[derive(Clone, Debug)]
pub struct Program {
    /// 全部函数（索引即 [`FuncId`]）。
    pub funcs: Vec<Func>,
    /// 包级变量。
    pub globals: Vec<Global>,
    /// 初始化顺序（每组一个 `var` 初始化式）。
    pub init_order: Vec<InitGroup>,
    /// 各文件的 `init()`（按文件序）。
    pub inits: Vec<FuncId>,
    /// `main`（可执行程序必有；库形态为 `None`）。
    pub main: Option<FuncId>,
}

/// 一个函数。
#[derive(Clone, Debug)]
pub struct Func {
    /// 显示名（`main` / `f` / `T.M` / `init.0` / `func@12:3`）。
    pub name: String,
    /// 形参槽（含变参的最后一个 slice 槽）。
    pub params: Vec<SlotId>,
    /// 形参的静态类型（按位置；接口形参要装箱，转换靠它）。
    pub param_types: Vec<TypeId>,
    /// 结果槽（命名结果与匿名结果都有槽 —— 统一写回路径）。
    pub results: Vec<SlotId>,
    /// 结果的静态类型（按位置）。
    pub result_types: Vec<TypeId>,
    /// 帧槽总数（形参 + 结果 + 局部 + 临时）。
    pub nslots: usize,
    /// 每个槽的静态类型（长度 == `nslots`；`Invalid` ⇒ 无类型临时槽）。
    ///
    /// 为什么必须有：`var x int` 不产生赋值语句，帧建立时就得把槽初始化成
    /// **该类型的零值**（`0` / `""` / `nil` / 全零数组）。没有类型就只能填
    /// 「某种 nil」，读到未赋值的 `int` 会得到 nil 而不是 0。
    pub slot_types: Vec<TypeId>,
    /// 函数体。
    pub body: Block,
    /// 闭包捕获（按变量：闭包帧的 `slot` 与外层帧的 `outer_slot` 是**同一个** `Cell`）。
    pub captured: Vec<Capture>,
    /// 是不是变参（`...T`）。
    pub variadic: bool,
    /// 有没有**命名**结果（裸 `return` 与 defer 改写返回值靠它）。
    pub named_results: bool,
    /// 方法的接收者基类型名（`func (t T) M()` ⇒ `Some("T")`；普通函数 ⇒ `None`）。
    ///
    /// 解释器的**接口动态派发**靠它：拿到动态类型名 + 方法名就能找到 `FuncId`。
    pub method_of: Option<String>,
    /// 函数体顶层块的标签 → 语句下标（`goto` 的目标；`M5-design.md` §3.2 的窄面）。
    pub labels: BTreeMap<String, usize>,
}

impl Func {
    /// 空函数（lowering 的第一遍占位）。
    #[must_use]
    pub fn placeholder(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            params: Vec::new(),
            param_types: Vec::new(),
            results: Vec::new(),
            result_types: Vec::new(),
            nslots: 0,
            slot_types: Vec::new(),
            body: Block::default(),
            captured: Vec::new(),
            variadic: false,
            named_results: false,
            method_of: None,
            labels: BTreeMap::new(),
        }
    }
}

/// 一条闭包捕获。
#[derive(Clone, Copy, Debug)]
pub struct Capture {
    /// 外层帧的槽。
    pub outer_slot: SlotId,
    /// 本帧的槽（调用时与 `outer_slot` 指向同一个 `Cell`）。
    pub slot: SlotId,
}

/// 一个包级变量。
#[derive(Clone, Debug)]
pub struct Global {
    /// 名字。
    pub name: String,
    /// 静态类型。
    pub typ: TypeId,
}

/// 一个初始化组（`var a, b = f()` 是一组）。
#[derive(Clone, Debug)]
pub struct InitGroup {
    /// 组内的全局变量（按声明序）。
    pub globals: Vec<GlobalId>,
    /// 右侧表达式（个数与 `globals` 的元数关系同赋值语句）。
    pub rhs: Vec<Expr>,
}

/// 语句块。
#[derive(Clone, Debug, Default)]
pub struct Block {
    /// 语句。
    pub stmts: Vec<Stmt>,
}

/// 语句。
#[derive(Clone, Debug)]
pub enum Stmt {
    /// 赋值（`=` / 复合赋值）。求值顺序：rhs 先全求值，再逐个写 lhs。
    Assign {
        /// 左值（可寻址位置）。
        lhs: Vec<Place>,
        /// 复合赋值的运算符（`+=` ⇒ `Some(Add)`）。
        op: Option<BinOp>,
        /// 右侧表达式。
        rhs: Vec<Expr>,
    },
    /// 短变量声明 `a, b := ...`（新变量已分配好槽）。
    ShortDecl {
        /// 新变量的槽（`_` 用 `SlotId(u32::MAX)` 占位）。
        slots: Vec<SlotId>,
        /// 右侧表达式。
        rhs: Vec<Expr>,
    },
    /// 表达式语句（调用语句；结果丢弃）。
    Expr(Expr),
    /// `if`。
    If {
        /// `if` 的初始化语句。
        init: Option<Box<Stmt>>,
        /// 条件。
        cond: Expr,
        /// then 块。
        then: Block,
        /// else 分支（`else if` 是嵌套的 `Stmt::If`）。
        els: Option<Box<Stmt>>,
    },
    /// 三句式 `for`。
    For {
        /// 初始化语句。
        init: Option<Box<Stmt>>,
        /// 条件（`None` ⇒ 恒真）。
        cond: Option<Expr>,
        /// 后置语句。
        post: Option<Box<Stmt>>,
        /// 循环体。
        body: Block,
        /// 标签（`L: for ...`）。
        label: Option<String>,
        /// 由 `init` 的 `:=` **声明**出来的循环变量（Go 1.22 的 per-iteration 语义）。
        ///
        /// 每轮进入后置语句之前，解释器给这些槽换一个**新 `Cell`**（初值取上一轮
        /// 的值）—— 否则循环体里创建的闭包共享同一个 `Cell`，退出时全部看到末值
        /// （`for i := 0; i < 3; i++ { fs = append(fs, func(){print(i)}) }` 会打
        /// `333` 而不是 `012`）。`init` 是**赋值**（`i = 0`）时为空：那种写法
        /// 没有声明新变量，Go 1.22 也不给它 per-iteration 语义。
        loopvars: Vec<SlotId>,
    },
    /// `range` 循环。
    Range {
        /// 键/下标目标。
        key: Option<Place>,
        /// 值目标。
        value: Option<Place>,
        /// 被遍历的表达式。
        x: Expr,
        /// 循环体。
        body: Block,
        /// 遍历形态（lowering 期按静态类型定死）。
        kind: RangeKind,
        /// `:=` 形态（每轮**新建**迭代变量 `Cell`，Go 1.22 语义）。
        define: bool,
        /// 标签。
        label: Option<String>,
    },
    /// `switch`（表达式 switch）。
    Switch {
        /// 初始化语句。
        init: Option<Box<Stmt>>,
        /// tag（`None` ⇒ `true`）。
        tag: Option<Expr>,
        /// 子句。
        cases: Vec<Case>,
        /// 标签。
        label: Option<String>,
    },
    /// 类型 switch。
    TypeSwitch {
        /// 初始化语句。
        init: Option<Box<Stmt>>,
        /// 守卫表达式（接口值）。
        x: Expr,
        /// 子句。
        cases: Vec<TypeCase>,
        /// 标签。
        label: Option<String>,
    },
    /// `return`。
    Return(Vec<Expr>),
    /// 分支语句。
    Branch {
        /// 种类。
        kind: BranchKind,
        /// 标签。
        label: Option<String>,
    },
    /// 带标签的语句。
    Labeled {
        /// 标签名。
        label: String,
        /// 被标记的语句。
        stmt: Box<Stmt>,
    },
    /// 块。
    Block(Block),
    /// `defer`（实参已展开为临时槽）。
    Defer {
        /// 被登记的调用。
        call: Expr,
    },
    /// `go f(args)`（M8，`D-M8-10`）。
    ///
    /// **求值顺序**（`STM-14`，写进 `C3`）：`call` 里的被调者与**全部实参**在
    /// **当前** goroutine 求值完，才创建新 goroutine。所以 `go f(g())` 的 `g()`
    /// 在旧 goroutine 跑 —— 这条不是实现细节，是判据（`T-CON-02`）。
    Go {
        /// 被发起的调用（形状同 [`Stmt::Expr`] 里的调用表达式）。
        call: Expr,
    },
    /// `ch <- v`（M8，`D-M8-10`）。
    ///
    /// 求值顺序：`chan` 先、`value` 后（Go 规范：操作数从左到右）。
    Send {
        /// 通道表达式。
        chan: Expr,
        /// 要发送的值。
        value: Expr,
    },
    /// `select { ... }`（M8，`D-M8-10`）。
    Select {
        /// 子句，**按源码顺序**（多就绪时的默认选择依赖这个顺序，`D-M8-3`）。
        clauses: Vec<SelectClause>,
    },
    /// 空语句。
    Empty,
}

/// `select` 的一个子句（M8，`D-M8-10`）。
#[derive(Clone, Debug)]
pub struct SelectClause {
    /// 子句种类。
    pub kind: SelectKind,
    /// 通道表达式（`Recv`/`Send` 必有；`Default` 必无）。
    pub chan: Option<Expr>,
    /// 接收目标，长度决定形态：
    ///
    /// | 长度 | 源码形态 |
    /// |---|---|
    /// | 0 | `case <-c:`（丢弃） |
    /// | 1 | `case v := <-c:` |
    /// | 2 | `case v, ok := <-c:` |
    ///
    /// 用 `Vec` 而不是 `Option<Place>`：`R2` 实测——两个目标那个形态装不下，
    /// 而 `T-CON-34` 就是判它。
    pub recv_targets: Vec<Place>,
    /// 发送的右值（仅 `Send` 有）。
    pub send_value: Option<Expr>,
    /// 子句体。
    pub body: Block,
}

/// `select` 子句的种类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SelectKind {
    /// `case ch <- v:`
    Send,
    /// `case v[, ok] := <-ch:`
    Recv,
    /// `default:`
    Default,
}

/// `range` 的形态（静态定死，运行时不再看类型）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RangeKind {
    /// 数组 / 数组指针。
    Array,
    /// slice。
    Slice,
    /// 字符串（按 rune）。
    Str,
    /// map。
    Map,
    /// 整数（Go 1.22 `range n`）。
    Int,
    /// 通道（M8）：反复接收直到 `ok == false`。
    Chan,
    /// 迭代器函数（Go 1.23，M8 的**窄面**，`D-M8-11`）。
    Func,
}

/// `switch` 的一个子句。
#[derive(Clone, Debug)]
pub struct Case {
    /// case 表达式（`default` ⇒ 空）。
    pub exprs: Vec<Expr>,
    /// 子句体。
    pub body: Block,
    /// 是不是 `default`。
    pub is_default: bool,
}

/// 类型 switch 的一个子句。
#[derive(Clone, Debug)]
pub struct TypeCase {
    /// case 的类型列表（`nil` case 记在 [`Self::is_nil`]）。
    pub types: Vec<TypeId>,
    /// 子句体。
    pub body: Block,
    /// 是不是 `default`。
    pub is_default: bool,
    /// 是不是 `case nil`。
    pub is_nil: bool,
    /// 绑定变量槽（每个子句一份）。
    pub bind: Option<SlotId>,
    /// 绑定变量的静态类型。
    pub bind_typ: TypeId,
}

/// 分支语句的种类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BranchKind {
    /// `break`
    Break,
    /// `continue`
    Continue,
    /// `goto`
    Goto,
    /// `fallthrough`
    Fallthrough,
}

/// 表达式（`kind` + 静态类型）。
#[derive(Clone, Debug)]
pub struct Expr {
    /// 节点。
    pub kind: ExprKind,
    /// 静态类型（来自检查会话的 `Operand.typ`）。
    pub typ: TypeId,
}

impl Expr {
    /// 造一个表达式。
    #[must_use]
    pub fn new(kind: ExprKind, typ: TypeId) -> Self {
        Self { kind, typ }
    }
}

/// 表达式的节点。
#[derive(Clone, Debug)]
pub enum ExprKind {
    /// 常量（已在 lowering 期落到宿主表示）。
    Const(ConstVal),
    /// 读槽（值语义：数组/结构体读出来就拷贝 —— 由解释器的 `deep_copy` 负责）。
    Slot(SlotId),
    /// 读全局。
    Global(GlobalId),
    /// 结构体字段（下标在 lowering 期定死）。
    Field {
        /// 底座。
        base: Box<Expr>,
        /// 字段下标。
        index: usize,
    },
    /// 索引（数组 / slice / string / map）。
    Index {
        /// 底座。
        base: Box<Expr>,
        /// 下标 / 键。
        index: Box<Expr>,
        /// 逗号-ok 双值形式（`v, ok := m[k]` / `v, ok = m[k]`）。
        ///
        /// HIR 的表达式是单值的，而 `v, ok := m[k]` 在 Go 里是**一个表达式
        /// 产生两个值**。这里把「双值」标在产生它的节点上，解释器据此多压
        /// 一个 `bool`（map 上有这个键 ⇒ `true`）。
        ///
        /// 只有「右侧唯一一项、左侧恰好两个」这一种位置会置 `true`
        /// （见 `lower::mark_comma_ok`）；其余位置（`f(m[k])`、`x = m[k]`）
        /// 必须保持 `false`，否则值栈会多出一个值。
        comma_ok: bool,
    },
    /// 切片。
    Slice {
        /// 底座。
        base: Box<Expr>,
        /// `lo`。
        lo: Option<Box<Expr>>,
        /// `hi`。
        hi: Option<Box<Expr>>,
        /// `max`（三下标）。
        max: Option<Box<Expr>>,
    },
    /// `*p`。
    Deref(Box<Expr>),
    /// `&x`（**唯一**产生指针的构造）。
    Addr(Place),
    /// `&T{...}` —— 对**复合字面量**取址。
    ///
    /// 复合字面量是 Go 规范里唯一「不是位置却可以取址」的表达式
    /// （Address operators：「a composite literal」）。它没有 `Place` 可指，
    /// 语义等价于「新建一个 `T`、把字面量填进去、返回它的地址」，
    /// 所以单独一支：求值出值，再分配一个 `Cell` 装它。
    ///
    /// 少了这一支，`&List{}` / `&[3]int{1,2,3}` 会落进 `lower_place` 并被拒
    /// `不是可寻址位置`（`compos.go` / `newexpr.go` 实测）。
    AddrOf(Box<Expr>),
    /// 一元运算。
    Unary {
        /// 运算符。
        op: UnOp,
        /// 操作数。
        x: Box<Expr>,
    },
    /// 二元运算。
    Binary {
        /// 运算符。
        op: BinOp,
        /// 左。
        x: Box<Expr>,
        /// 右。
        y: Box<Expr>,
    },
    /// 调用。
    Call {
        /// 被调方。
        callee: Callee,
        /// 实参。
        args: Vec<Expr>,
        /// 末参带 `...`。
        variadic: bool,
        /// 唯一实参是**多值调用**时展开出的实参个数（`0` ⇒ 不展开）。
        ///
        /// Go 允许 `f(g())` 把 `g` 的多个结果直接当作 `f` 的实参列表
        /// （`simassign.go` 的 `swap(swap(a, b))`）。HIR 的 `Expr` 是单值的，
        /// 没有「一个表达式占多个实参位」的表示，所以这里只记**个数**：
        /// 求值栈上确实会多出 `spread` 个值，解释器按它来切分实参，
        /// 而不是按 `args.len()`。
        spread: u8,
    },
    /// 类型转换（含隐式的接口装箱 / 数值拓宽）。
    Convert {
        /// 目标类型。
        to: TypeId,
        /// 源表达式。
        x: Box<Expr>,
    },
    /// 复合字面量。
    Composite(Composite),
    /// 函数字面量（已提升为独立 `Func`）。
    FuncLit(FuncId),
    /// 方法值（`x.M` 不调用）。
    Bound {
        /// 接收者。
        recv: Box<Expr>,
        /// 方法。
        method: MethodRef,
    },
    /// 类型断言。
    TypeAssert {
        /// 接口值。
        x: Box<Expr>,
        /// 目标类型。
        to: TypeId,
        /// 双值形态（`v, ok := i.(T)`）。
        comma_ok: bool,
    },
    /// `make`。
    Make {
        /// 形态。
        kind: MakeKind,
        /// 实参（长度 / 容量）。
        ///
        /// | `kind` | `args` |
        /// |---|---|
        /// | `Slice` | `[len]` 或 `[len, cap]` |
        /// | `Map` | `[hint]` 或空 |
        /// | `Chan` | `[cap]` 或空（空 = 无缓冲，`C11` §3.1） |
        args: Vec<Expr>,
    },
    /// `new(T)` —— 分配一个零值 `Cell`，结果是 `*T`。
    New(TypeId),
    /// 内建调用（非 `make` / 非 `print` 家族也在这里）。
    Builtin {
        /// 哪一个。
        kind: BuiltinKind,
        /// 实参。
        args: Vec<Expr>,
        /// 末参带 `...`（只有 `append(dst, src...)` 会用到）。
        ///
        /// 为什么内建也要这一位：`append(s, t...)` 与 `append(s, t)` 的语义
        /// **不同**（前者展开 `t` 的元素，后者追加一个 `t` 值）。缺了这一位，
        /// 解释器只能猜，而猜错不会报错、只会静默算错 —— 正是 `D-M5-2`
        /// 想禁止的那类失败。
        variadic: bool,
    },
    /// 接收 `<-ch`（M8，`D-M8-10`）。
    Recv {
        /// 通道表达式。
        ch: Box<Expr>,
        /// 双值形态（`v, ok := <-c`）。关闭后 `ok == false`（`T-CON-34`）。
        comma_ok: bool,
    },
    /// 有类型的 `nil`。
    Nil,
}

/// 常量值（宿主表示 —— 精度损失已在 lowering 期按静态类型收敛）。
#[derive(Clone, Debug, PartialEq)]
pub enum ConstVal {
    /// 整数（`i128` 装得下 `uint64` 的全域）。
    Int(i128),
    /// 浮点。
    Float(f64),
    /// 复数。
    Complex(f64, f64),
    /// 字符串。
    Str(String),
    /// 布尔。
    Bool(bool),
}

/// 一元运算符。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UnOp {
    /// `-x`
    Neg,
    /// `+x`
    Pos,
    /// `!x`
    Not,
    /// `^x`（按位取反）
    BitNot,
    /// `<-ch`（M5 不出）
    Recv,
}

/// 二元运算符。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BinOp {
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `*`
    Mul,
    /// `/`
    Div,
    /// `%`
    Rem,
    /// `&`
    And,
    /// `|`
    Or,
    /// `^`
    Xor,
    /// `&^`
    AndNot,
    /// `<<`
    Shl,
    /// `>>`
    Shr,
    /// `==`
    Eq,
    /// `!=`
    Ne,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
    /// `&&`
    LAnd,
    /// `||`
    LOr,
}

/// 可寻址位置（`C3` §2.2 的答案）。
#[derive(Clone, Debug)]
pub enum Place {
    /// 局部 / 形参 / 命名返回值。
    Slot(SlotId),
    /// 包级变量。
    Global(GlobalId),
    /// 结构体字段。
    Field {
        /// 底座。
        base: Box<Place>,
        /// 字段下标。
        index: usize,
    },
    /// 数组 / slice 元素（**map 元素不可寻址**）。
    Index {
        /// 底座（表达式 —— 数组值 / slice 值 / 指针）。
        base: Box<Expr>,
        /// 下标。
        index: Box<Expr>,
    },
    /// `*p`。
    Deref(Box<Expr>),
}

/// 被调方。
#[derive(Clone, Debug)]
pub enum Callee {
    /// 直接调用一个已知函数。
    Func(FuncId),
    /// 静态方法调用（接收者已按值/指针语义准备好）。
    Method {
        /// 接收者。
        recv: Box<Expr>,
        /// 方法体。
        func: FuncId,
    },
    /// 接口方法调用（动态派发）。
    Iface {
        /// 接口值。
        recv: Box<Expr>,
        /// 方法名。
        name: String,
    },
    /// 调用一个函数值（闭包 / 变量里的函数）。
    Value(Box<Expr>),
    /// 内建。
    Builtin(BuiltinKind),
}

/// 方法引用（方法值用）。
#[derive(Clone, Debug)]
pub enum MethodRef {
    /// 静态方法。
    Func(FuncId),
    /// 接口方法。
    Iface(String),
}

/// 复合字面量。
#[derive(Clone, Debug)]
pub struct Composite {
    /// 目标类型。
    pub typ: TypeId,
    /// 元素。
    pub items: Vec<CompositeItem>,
}

/// 复合字面量的一个元素。
#[derive(Clone, Debug)]
pub enum CompositeItem {
    /// 位置元素（数组 / slice）。
    Value(Expr),
    /// map 键值。
    Key(Expr, Expr),
    /// 结构体字段（下标已定）。
    Field(usize, Expr),
    /// 数组 / slice 的显式下标。
    Index(usize, Expr),
}

/// `make` 的形态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MakeKind {
    /// `make([]T, n[, cap])`
    Slice,
    /// `make(map[K]V[, n])`
    Map,
    /// `make(chan T, n)`（M5 不出）
    Chan,
}

/// 内建函数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BuiltinKind {
    /// `len`
    Len,
    /// `cap`
    Cap,
    /// `append`
    Append,
    /// `copy`
    Copy,
    /// `delete`
    Delete,
    /// `clear`
    Clear,
    /// `new`
    New,
    /// `min`
    Min,
    /// `max`
    Max,
    /// `complex`
    Complex,
    /// `real`
    Real,
    /// `imag`
    Imag,
    /// `print`
    Print,
    /// `println`
    Println,
    /// `panic`
    Panic,
    /// `recover`
    Recover,
    /// `close`（M5 显式拒绝，lowering 不产出）
    Close,
}
