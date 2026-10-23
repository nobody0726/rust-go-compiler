//! 值模型（`C9` 的核心；`M5-design.md` §3.3）。
//!
//! # 四条语义的表示（逐条对应一个可测判据）
//!
//! | 语义 | 表示 | 判据 |
//! |---|---|---|
//! | 数组/结构体**拷贝** | `Rc<Vec<Cell>>` + [`Value::deep_copy`] | `a := arr; a[0]=1` ⇒ `arr[0]` 不变 |
//! | slice **共享底层** | `{arr: Rc, off, len, cap}` | `s2 := s1; s2[0]=1` ⇒ `s1[0]==1` |
//! | string **不可变** | `Rc<str>` | 赋值只共享 |
//! | map **引用** | `Rc<RefCell<MapObj>>` | `m2 := m1; m2[k]=v` ⇒ `m1[k]==v` |
//!
//! # `Int(i128)` 与宽度
//!
//! 槽里只存 `i128`，**宽度由静态类型决定**（`ops::normalize` 在每次运算后按
//! 宽度掩码）。`uint64` 的最大值 `18446744073709551615` 在 `i128` 里装得下
//! （`F10` 实测打印正确），所以不需要 `u64` 分支。
//!
//! # `MapObj` 为什么是 `Vec` 而不是 `HashMap`
//!
//! ① 迭代序 = **插入序**（`U-M5-04`：官方故意随机，我们固定一个可复现的序）；
//! ② Go 的键相等不是 `Hash + Eq`（`NaN != NaN`、接口按 `(动态类型, 值)`）——
//!    外层自己实现键相等时，`Vec` 的线性查比「给 `f64` 造一个不自反的 `Hash`」
//!    更不容易写错。M5 的 map 规模是个位数，线性查够用。

use std::cell::RefCell;
use std::rc::Rc;

use rgoc_const::{BasicId, Const};
use rgoc_hir::FuncId;
use rgoc_types::{Type, TypeId, TypeTable, unalias, underlying};

/// 一切可寻址位置的载体。
pub type Cell = Rc<RefCell<Value>>;

/// 造一个单元。
#[must_use]
pub fn cell(v: Value) -> Cell {
    Rc::new(RefCell::new(v))
}

/// 一个值。
#[derive(Clone, Debug)]
pub enum Value {
    /// 布尔。
    Bool(bool),
    /// 整数（宽度由静态类型决定）。
    Int(i128),
    /// 浮点（`float32` 每次运算后按 `f32` 舍入）。
    Float(f64),
    /// 复数。
    Complex(f64, f64),
    /// 字符串（不可变）。
    Str(Rc<str>),
    /// 数组（**值语义**）。
    Array(Rc<Vec<Cell>>),
    /// 结构体（**值语义**；字段下标由 lowering 定死）。
    Struct(Rc<Vec<Cell>>),
    /// 切片（**共享底层数组**）。
    Slice(SliceVal),
    /// map（**引用语义**）。
    Map(Rc<RefCell<MapObj>>),
    /// 指针。
    Ptr(Cell),
    /// 函数值（闭包：函数体 + 捕获的 `Cell`）。
    Func(Rc<Closure>),
    /// 接口值；`typ == None` ⇒ nil 接口。
    Iface {
        /// 动态类型。
        typ: Option<TypeId>,
        /// 动态值。
        data: Box<Value>,
    },
    /// 有类型的 `nil`。
    Nil(NilKind),
}

/// `nil` 的种类（`print` 与 `==` 要区分「哪种 nil」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NilKind {
    /// `*T`
    Ptr,
    /// `map[K]V`
    Map,
    /// `[]T`
    Slice,
    /// `func(...)`
    Func,
    /// `chan T`
    Chan,
    /// 接口（无动态类型）
    Iface,
}

/// 切片的三元组。
#[derive(Clone, Debug)]
pub struct SliceVal {
    /// 底层数组。
    pub arr: Rc<Vec<Cell>>,
    /// 起点。
    pub off: usize,
    /// 长度。
    pub len: usize,
    /// 容量。
    pub cap: usize,
}

/// 一个闭包（或一个绑定了接收者的方法值）。
#[derive(Debug)]
pub struct Closure {
    /// 函数体。
    pub func: FuncId,
    /// 捕获的单元（与外层**共享**）。
    pub captured: Vec<Cell>,
    /// 绑定的接收者（方法值 `x.M` ⇒ `Some(x)`；普通函数字面量 ⇒ `None`）。
    ///
    /// 为什么在这里而不是另开一个 `Value::Method`：方法值调用时的实参序列
    /// 与普通函数完全一样（接收者占 `params[0]`），只是**多一个已经求过值的
    /// 接收者**。一个 `Option` 就够，多一个变体会让 `Value` 的 12 个分支里
    /// 每一处 `Func` 判断都要多写一遍。
    pub recv: Option<Value>,
}

/// map 的存储（插入序）。
#[derive(Debug, Default)]
pub struct MapObj {
    /// 键值对（插入序）。
    pub entries: Vec<(Value, Value)>,
}

impl MapObj {
    /// 查一个键。
    #[must_use]
    pub fn get(&self, k: &Value) -> Option<&Value> {
        self.entries
            .iter()
            .find(|(kk, _)| go_eq(kk, k))
            .map(|(_, v)| v)
    }

    /// 写入（存在则改值，否则追加到末尾）。
    pub fn set(&mut self, k: Value, v: Value) {
        if let Some(slot) = self.entries.iter_mut().find(|(kk, _)| go_eq(kk, &k)) {
            slot.1 = v;
        } else {
            self.entries.push((k, v));
        }
    }

    /// 删除。
    pub fn remove(&mut self, k: &Value) {
        if let Some(i) = self.entries.iter().position(|(kk, _)| go_eq(kk, k)) {
            self.entries.remove(i);
        }
    }

    /// 长度。
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是不是空的（与 [`MapObj::len`] 成对；clippy 的 `len_without_is_empty`）。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Value {
    /// 值语义的深拷贝：数组 / 结构体 / 接口里的复合值**递归复制**。
    ///
    /// slice / map / 指针 / 函数是引用类型 ⇒ 只复制句柄（这是 Go 的语义）。
    #[must_use]
    pub fn deep_copy(&self) -> Value {
        match self {
            Value::Array(cells) => Value::Array(Rc::new(
                cells.iter().map(|c| cell(c.borrow().deep_copy())).collect(),
            )),
            Value::Struct(cells) => Value::Struct(Rc::new(
                cells.iter().map(|c| cell(c.borrow().deep_copy())).collect(),
            )),
            Value::Iface { typ, data } => Value::Iface {
                typ: *typ,
                data: Box::new(data.deep_copy()),
            },
            other => other.clone(),
        }
    }

    /// 是不是 `nil`（含 nil 接口）。
    #[must_use]
    pub fn is_nil(&self) -> bool {
        matches!(self, Value::Nil(_)) || matches!(self, Value::Iface { typ: None, .. })
    }

    /// 取整数值（非整数 ⇒ `None`）。
    #[must_use]
    pub fn as_int(&self) -> Option<i128> {
        match self {
            Value::Int(v) => Some(*v),
            _ => None,
        }
    }

    /// 取布尔（非布尔 ⇒ `None`）。
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }
}

/// Go 的 `==`（数值按值、字符串按字节、指针按身份、接口按 `(动态类型, 值)`）。
///
/// `NaN != NaN` 是**刻意**的：`f64` 的 `==` 已经这样，这里不额外修。
#[must_use]
pub fn go_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Float(x), Value::Float(y)) => x == y,
        (Value::Complex(xr, xi), Value::Complex(yr, yi)) => xr == yr && xi == yi,
        (Value::Str(x), Value::Str(y)) => x == y,
        // 指针相等 = **对象身份**。零尺寸对象是唯一的例外：Go 规范说
        // 「指向不同零尺寸变量的指针**可能**相等也可能不等」，gc 一律让它们
        // 相等（都指向 `runtime.zerobase`）。我们必须跟 gc 一致，否则
        // `zerosize.go` 的 `p != q`（`p, q = new([0]int), new([0]int)`）
        // 会走错分支。
        (Value::Ptr(x), Value::Ptr(y)) => {
            Rc::ptr_eq(x, y) || (is_empty_aggregate(&x.borrow()) && is_empty_aggregate(&y.borrow()))
        }
        (Value::Nil(_), Value::Nil(_)) => true,
        (Value::Iface { typ: tx, data: dx }, Value::Iface { typ: ty, data: dy }) => {
            match (tx, ty) {
                (None, None) => true,
                (Some(x), Some(y)) => x == y && go_eq(dx, dy),
                _ => false,
            }
        }
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len()
                && x.iter()
                    .zip(y.iter())
                    .all(|(p, q)| go_eq(&p.borrow(), &q.borrow()))
        }
        (Value::Struct(x), Value::Struct(y)) => {
            x.len() == y.len()
                && x.iter()
                    .zip(y.iter())
                    .all(|(p, q)| go_eq(&p.borrow(), &q.borrow()))
        }
        (Value::Func(x), Value::Func(y)) => Rc::ptr_eq(x, y),
        // 不同类型的 nil 与 nil 比：`var p *T; var m map[..]; p == m` 是编译错误，
        // 能走到这里只可能是「同一种 nil」的两种表示，按相等处理。
        (Value::Nil(_), Value::Iface { typ: None, .. })
        | (Value::Iface { typ: None, .. }, Value::Nil(_)) => true,
        _ => false,
    }
}

/// 是不是**零尺寸聚合值**（`[0]T` / `struct{}`）。
///
/// 只认「一个单元都没有」的聚合：`struct{ a, b struct{} }` 有 2 个字段单元，
/// 即使整体尺寸为 0 也不在判定内 —— 那些字段是**可寻址**的，把它们并到
/// 同一个身份上会破坏字段别名。
fn is_empty_aggregate(v: &Value) -> bool {
    match v {
        Value::Array(cells) | Value::Struct(cells) => cells.is_empty(),
        _ => false,
    }
}

/// 类型的零值。
///
/// **切片 / map / 通道 / 函数 / 接口的零值是 `nil`，不是「空的同类值」**：
/// `var z []int; z == nil` 在 Go 里是 `true`，而 `[]int{} == nil` 是 `false`。
/// 所以这里必须给 `Nil(NilKind::Slice)`，不能给 `Slice{len:0,cap:0}` ——
/// 后者会和 `[]int{}` 撞成同一个表示，`== nil` 就永远为假。
#[must_use]
pub fn zero_of(tt: &TypeTable, typ: TypeId) -> Value {
    let t = unalias(tt, typ);
    match tt.type_of(underlying(tt, t)) {
        Type::Basic(b) => zero_basic(*b),
        Type::Pointer(_) => Value::Nil(NilKind::Ptr),
        Type::Slice(_) => Value::Nil(NilKind::Slice),
        Type::Map { .. } => Value::Nil(NilKind::Map),
        Type::Chan { .. } => Value::Nil(NilKind::Chan),
        Type::Signature(_) => Value::Nil(NilKind::Func),
        Type::Interface(_) => Value::Iface {
            typ: None,
            data: Box::new(Value::Nil(NilKind::Iface)),
        },
        Type::Array { len, elem } => {
            let n = len
                .map(|id| match tt.consts().get(id) {
                    Const::Int(b) => b.to_dec().parse::<usize>().unwrap_or(0),
                    _ => 0,
                })
                .unwrap_or(0);
            Value::Array(Rc::new((0..n).map(|_| cell(zero_of(tt, *elem))).collect()))
        }
        Type::Struct(sid) => {
            let fields = tt.struct_def(*sid).fields.clone();
            Value::Struct(Rc::new(
                fields.iter().map(|f| cell(zero_of(tt, f.typ))).collect(),
            ))
        }
        _ => Value::Nil(NilKind::Ptr),
    }
}

fn zero_basic(b: BasicId) -> Value {
    match b {
        BasicId::Bool | BasicId::UntypedBool => Value::Bool(false),
        BasicId::String | BasicId::UntypedString => Value::Str(Rc::from("")),
        BasicId::Float32 | BasicId::Float64 | BasicId::UntypedFloat => Value::Float(0.0),
        BasicId::Complex64 | BasicId::Complex128 | BasicId::UntypedComplex => {
            Value::Complex(0.0, 0.0)
        }
        BasicId::UnsafePointer | BasicId::UntypedNil => Value::Nil(NilKind::Ptr),
        _ => Value::Int(0),
    }
}

/// 整数类型的宽度与符号性（`None` ⇒ 不是整数）。
#[must_use]
pub fn int_shape(b: BasicId) -> Option<(u32, bool)> {
    Some(match b {
        BasicId::Int | BasicId::UntypedInt => (64, true),
        BasicId::Int8 => (8, true),
        BasicId::Int16 => (16, true),
        BasicId::Int32 | BasicId::UntypedRune => (32, true),
        BasicId::Int64 => (64, true),
        BasicId::Uint | BasicId::Uintptr => (64, false),
        BasicId::Uint8 => (8, false),
        BasicId::Uint16 => (16, false),
        BasicId::Uint32 => (32, false),
        BasicId::Uint64 => (64, false),
        _ => return None,
    })
}

/// 类型是不是整数（含无类型整数）。
#[must_use]
pub fn is_integer(tt: &TypeTable, typ: TypeId) -> bool {
    match tt.type_of(underlying(tt, unalias(tt, typ))) {
        Type::Basic(b) => int_shape(*b).is_some(),
        _ => false,
    }
}

/// 类型是不是 `float32`（运算后要按 `f32` 舍入）。
#[must_use]
pub fn is_float32(tt: &TypeTable, typ: TypeId) -> bool {
    matches!(
        tt.type_of(underlying(tt, unalias(tt, typ))),
        Type::Basic(BasicId::Float32)
    )
}

/// 类型是不是浮点。
#[must_use]
pub fn is_float(tt: &TypeTable, typ: TypeId) -> bool {
    matches!(
        tt.type_of(underlying(tt, unalias(tt, typ))),
        Type::Basic(BasicId::Float32 | BasicId::Float64 | BasicId::UntypedFloat)
    )
}

/// 类型是不是复数。
#[must_use]
pub fn is_complex(tt: &TypeTable, typ: TypeId) -> bool {
    matches!(
        tt.type_of(underlying(tt, unalias(tt, typ))),
        Type::Basic(BasicId::Complex64 | BasicId::Complex128 | BasicId::UntypedComplex)
    )
}

/// 类型是不是字符串。
#[must_use]
pub fn is_string(tt: &TypeTable, typ: TypeId) -> bool {
    matches!(
        tt.type_of(underlying(tt, unalias(tt, typ))),
        Type::Basic(BasicId::String | BasicId::UntypedString)
    )
}

/// 类型是不是接口。
#[must_use]
pub fn is_iface(tt: &TypeTable, typ: TypeId) -> bool {
    matches!(
        tt.type_of(underlying(tt, unalias(tt, typ))),
        Type::Interface(_)
    )
}
