//! 常量表达式求值（M3/Task 12，`T-TY-83`，`D-M3-19`）。
//!
//! # 这一层**只看值，不看类型**
//!
//! 它拿 `rgoc_ast::Expr` 的语法树算出 [`Const`]。**不建类型表、不查作用域** ——
//! 那是 `rgoc-types` 的事（Task 13+）。所以本文件能自己判的错只有
//! 「值与值之间就能判定的」那几种：
//!
//! | 判据 | 代号 | 为什么本层就能判 |
//! |---|---|---|
//! | 除 / 取模的除数为 0 | `TYP-DIV-BY-ZERO` | 两个操作数的**值**够了 |
//! | 移位计数非法 | `TYP-INVALID-SHIFT-COUNT` | 同上（负数 / 非整数 / 超上界） |
//! | 移位左操作数非整数 | `TYP-INVALID-SHIFT-OPERAND` | 同上 |
//! | 转换不合法 / 溢出 | `TYP-INVALID-CONVERSION` 等 | 目标名是**预声明基本类型**时够了 |
//!
//! ⚠️ **`invalid operation: …` 那类不走本层**（`"a" - "b"`、`-"a"`、`1 + "a"`）：
//! 那些要在**类型**上才说得清（`1 + x` 里 `x` 是无类型常量还是 `int64`，
//! 结论不同），而本层没有类型。折叠算子在这种情形返回 [`Const::Unknown`]，
//! 本层**不报错**、把判断权留给类型层（Task 16+）。这条边界是**有意**的：
//! 在值层猜类型会造出「有时误报、有时漏报」的诊断。
//!
//! ## 唯一一处例外：`^`（Task 30 的语料冒烟发现）
//!
//! `^x` 的**值**依赖 `x` 的类型（无符号有类型 ⇒ 按宽度取补，结果非负；
//! 有符号 / 无类型 ⇒ 无限精度补码 `-x-1`，两者差一个宽度）。
//! 本层**不建类型表**，但有一种形状**语法自己就说得出类型**：
//! 「转换到预声明基本类型」——`^uintptr(0)` 的目标写在源码里。
//! 所以 [`operand_basic_ty`] 只认这种形状（+ 括号穿透），
//! [`fold_unary_typed`] 据此取补；**给不出类型时按无类型算，不猜**。
//!
//! 这不是把类型表偷偷搬进来：它读的是**操作数自己写的那个类型名**，
//! 与「查到某名字的类型」是两回事（后者仍然留给类型层）。
//! 为什么必须补：`const ptrSize = 4 << (^uintptr(0) >> 63)`（语料
//! `unicode/utf8/utf8.go:459`）在合法代码上会被判成 `TYP-INVALID-SHIFT-COUNT`
//! —— **误报**比漏报更贵（它把能力边界伪装成用户的错）。
//!
//! # 三种返回值，别混
//!
//! ```text
//! EvalOutcome::Const(v)        有值（`v` 也可能是 Unknown）
//! EvalOutcome::Const(Unknown)  **形状上是**常量表达式，但**没有值**
//!                              —— 错已经报过（本层的 DiagBag 或上游），
//!                              照官方的话说是 avoid follow-up errors
//! EvalOutcome::NotAConstant    **不是**常量表达式（要类型信息 / 要运行期值）
//! ```
//!
//! 这个三分法是 `T-TY-83` 的要害：它的**正向**要求
//! 「需类型信息的 `const` 初始化式 ⇒ 稳定拒绝码」，**反向**要求
//! 「常量子集初始化式 ⇒ 不报错」。两件事实现在一对返回值上，
//! 而**拒绝码由调用方发**（`TYP-INVALID-CONST-INIT`，官方 `InvalidConstInit`）
//! —— 因为「这是不是常量表达式」的判据在调用方那里还有别的来源
//! （比如引用了一个非常量声明）。
//!
//! # 支持面（常量子集）
//!
//! * 字面量（`BasicLit`，含坏字面量 ⇒ `Unknown`，错由上游报）
//! * 引用到**常量**的标识符（`ConstEnv`）；`true`/`false` 直接认
//! * 一元 `+ - ^ !`；二元 `+ - * / % << >> & | ^ &^`、比较、`&& ||`
//! * **转换到预声明基本类型**（`int8(x)`、`float64(x)`、`string(65)` …）
//! * 括号
//!
//! **不在**支持面（⇒ `NotAConstant`，`open_gaps` 有记录）：
//! 转换到**具名**类型（要类型表）、内建函数（`len`/`cap`/`min`/`max`/`real`/`imag`/`complex`
//! —— 官方的确会把它们折成常量，实测 `min(1,2) == 1`、`len("ab") == 2`）、
//! 索引 / 切片 / 选择器 / 复合字面量 / 函数调用 / 类型断言。

use rgoc_ast::{BasicLit, CallExpr, Expr, Op};
use rgoc_lex::{Diag, DiagBag, DiagCode, Pos};

use crate::basic::BasicId;
use crate::convert::convert_const;
use crate::fold::{fold_binary, fold_compare, fold_unary_typed, shift_ok, shift_operand_ok};
use crate::representable::representable;
use crate::sizes::Sizes;
use crate::value::{Const, const_from_literal};

/// 常量求值的结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum EvalOutcome {
    /// 有值 —— 值本身可能是 [`Const::Unknown`]（见模块文档的三种返回值）。
    Const(Const),
    /// **不是**常量表达式：需要类型信息或运行期值。
    NotAConstant,
}

impl EvalOutcome {
    /// 取到值（`NotAConstant` 或 `Unknown` ⇒ `None`）。
    #[must_use]
    pub fn value(&self) -> Option<&Const> {
        match self {
            EvalOutcome::Const(v) if *v != Const::Unknown => Some(v),
            _ => None,
        }
    }

    /// 是否有值（`Unknown` 算「没有」）。
    #[must_use]
    pub fn has_value(&self) -> bool {
        self.value().is_some()
    }
}

/// 常量求值要看的环境。
///
/// M3 的检查器（Task 19）用作用域实现它：包级常量表 + 当前文件的 `iota`。
/// 本层只问一个问题 —— 「这个名字是常量吗，值是多少」。
pub trait ConstEnv {
    /// 名字 → 常量值。`None` ⇒ 不知道（可能是变量、类型、函数…）。
    fn lookup_const(&self, ident: &str) -> Option<&Const>;
}

/// 求常量表达式的值。
///
/// 诊断（若有）与结果**一起**返回：调用方不该丢掉其中一半 ——
/// `Const(Unknown)` 的场合，值必须与「为什么没有值」的同一条诊断配对解释，
/// 分成两次调用会让两者可能失配。
pub fn eval_const_expr(
    expr: &Expr,
    env: &(impl ConstEnv + ?Sized),
    sizes: &Sizes,
) -> (EvalOutcome, DiagBag) {
    let mut diags = DiagBag::new();
    let out = eval(expr, env, sizes, &mut diags);
    (out, diags)
}

/// 递归求值。诊断**带位置**地累加到 `diags`（位置取自触发它的那个子表达式）。
fn eval(
    expr: &Expr,
    env: &(impl ConstEnv + ?Sized),
    sizes: &Sizes,
    diags: &mut DiagBag,
) -> EvalOutcome {
    match expr {
        Expr::BasicLit(lit) => EvalOutcome::Const(eval_lit(lit)),

        Expr::Name(n) => match n.value.as_str() {
            // 预声明布尔常量：它们**没有**类型依赖，本层直接认
            "true" => EvalOutcome::Const(Const::Bool(true)),
            "false" => EvalOutcome::Const(Const::Bool(false)),
            // `nil` 是预声明常量，但 `Const` 表达不出它 ⇒ 本层判「非常量」
            // （不是「未知值」：nil 有确定的值，只是不在常量子集里）
            "nil" => EvalOutcome::NotAConstant,
            ident => match env.lookup_const(ident) {
                Some(c) => EvalOutcome::Const(c.clone()),
                // 名字不在常量环境里 ⇒ 可能是变量/类型/函数 ⇒ 要类型信息
                None => EvalOutcome::NotAConstant,
            },
        },

        Expr::Paren(p) => eval(&p.x, env, sizes, diags),
        Expr::Unary(op) => {
            let x = eval(&op.x, env, sizes, diags);
            match x {
                EvalOutcome::NotAConstant => EvalOutcome::NotAConstant,
                // `^` 的值**依赖操作数的类型**（无符号有类型 ⇒ 按宽度取补，非负；
                // 其余 ⇒ 无限精度补码）。类型只在本层**语法上就能确定**时才给得出
                // （见 `operand_basic_ty`）；给不出就按无类型算。
                EvalOutcome::Const(v) => {
                    EvalOutcome::Const(fold_unary_typed(op.op, &v, operand_basic_ty(&op.x), sizes))
                }
            }
        }
        Expr::Binary(op) => {
            let x = eval(&op.x, env, sizes, diags);
            if x == EvalOutcome::NotAConstant {
                return EvalOutcome::NotAConstant;
            }
            // `Expr::Binary` 的右操作数理应是 `Some`（形状由 parser 保证）；
            // 缺了就当成「非常量」而不是崩 —— 崩在这里会把用户输入变成 ICE
            let Some(rhs) = &op.y else {
                return EvalOutcome::NotAConstant;
            };
            let y = eval(rhs, env, sizes, diags);
            let (EvalOutcome::Const(a), EvalOutcome::Const(b)) = (x, y) else {
                return EvalOutcome::NotAConstant;
            };
            // `Unknown` 传染：**不**把「没有值」当成 0 / ""，也不再报错
            if a == Const::Unknown || b == Const::Unknown {
                return EvalOutcome::Const(Const::Unknown);
            }
            eval_binary(op.op, &a, &b, rhs.pos(), diags)
        }

        Expr::Call(c) => eval_call(c, env, sizes, diags),

        // 其余（选择器/索引/切片/复合字面量/函数字面量/类型断言/类型字面量/坏节点…）
        // 都需要类型信息或运行期值 ⇒ 常量子集之外
        _ => EvalOutcome::NotAConstant,
    }
}

/// 二元运算：先查**值与值就能判定**的错误，再交给折叠算子。
fn eval_binary(op: Op, a: &Const, b: &Const, rhs_pos: Pos, diags: &mut DiagBag) -> EvalOutcome {
    // ① 除 / 取模的除数为零（官方四条全报 `DivByZero`，消息带 `invalid operation:` 前缀）
    if is_zero(b) && matches!(op, Op::Quo | Op::Rem) {
        diags.push(Diag {
            pos: rhs_pos,
            code: DiagCode::TypDivByZero,
            message: "invalid operation: division by zero".to_string(),
        });
        return EvalOutcome::Const(Const::Unknown);
    }
    // ② 移位：左操作数必须是整数、计数必须是合法整数
    if matches!(op, Op::Shl | Op::Shr) {
        if !shift_operand_ok(a) {
            diags.push(Diag {
                pos: rhs_pos,
                code: DiagCode::TypInvalidShiftOperand,
                message: "invalid operation: shift of non-integer operand".to_string(),
            });
            return EvalOutcome::Const(Const::Unknown);
        }
        if let Err(code) = shift_ok(b) {
            diags.push(Diag {
                pos: rhs_pos,
                code,
                message: "invalid operation: invalid shift count".to_string(),
            });
            return EvalOutcome::Const(Const::Unknown);
        }
    }
    // ③ 比较走 `fold_compare`，其余走 `fold_binary`
    let v = match op {
        Op::Eql | Op::Neq | Op::Lss | Op::Leq | Op::Gtr | Op::Geq => fold_compare(op, a, b),
        _ => fold_binary(op, a, b),
    };
    // 折叠给不出值（`-"a"`、`1 + "a"`、`true + true` …）⇒ 那是**类型**层面的问题，
    // 本层没有类型，判断权留给 Task 16+（见模块文档的边界说明）。
    EvalOutcome::Const(v)
}

/// 调用：**只**支持「转换到预声明基本类型」。
fn eval_call(
    c: &CallExpr,
    env: &(impl ConstEnv + ?Sized),
    sizes: &Sizes,
    diags: &mut DiagBag,
) -> EvalOutcome {
    // `f(x, y)` / `f(x...)` / 泛型实例化都不是常量转换
    if c.has_dots || c.args.len() != 1 {
        return EvalOutcome::NotAConstant;
    }
    let Expr::Name(fun) = &*c.fun else {
        return EvalOutcome::NotAConstant;
    };
    // 目标不是预声明基本类型名（内建函数名、具名类型、变量、`unsafe.Pointer`…）
    // ⇒ 要类型信息 ⇒ `T-TY-83` 的「需类型信息」那一侧
    let Some(to) = basic_from_name(&fun.value) else {
        return EvalOutcome::NotAConstant;
    };

    let arg = eval(&c.args[0], env, sizes, diags);
    let EvalOutcome::Const(v) = arg else {
        return EvalOutcome::NotAConstant;
    };
    if v == Const::Unknown {
        return EvalOutcome::Const(Const::Unknown);
    }

    // 两段走（Task 8/10 的分工）：先「换表示」（种类），再「判可表示」（范围 + 舍入）。
    // **转换语境**的代号映射（实测官方）：
    //   `cannot convert … to type T` ⇒ `TYP-INVALID-CONVERSION`
    //     （覆盖种类的 `InvalidConversion` **和** `TruncatedFloat`：
    //      实测 `int(1.5)` 与 `float64(1i)` 都是这句）
    //   `constant … overflows T` ⇒ `TYP-NUMERIC-OVERFLOW`
    let converted = match convert_const(&v, to) {
        Ok(cv) => cv,
        Err(_) => return invalid_conversion(fun.pos, diags),
    };
    match representable(&converted, to, sizes) {
        Ok(rep) => EvalOutcome::Const(rep.value),
        Err(DiagCode::TypTruncatedFloat | DiagCode::TypInvalidConstVal) => {
            invalid_conversion(fun.pos, diags)
        }
        // `NumericOverflow`（以及将来可能新增的范围类代号）**保持原码**
        Err(code) => {
            diags.push(Diag {
                pos: fun.pos,
                code,
                message: format!("constant overflows {}", to.as_str()),
            });
            EvalOutcome::Const(Const::Unknown)
        }
    }
}

/// 「转换不合法」：报一条 `TYP-INVALID-CONVERSION` 并退化成没有值。
fn invalid_conversion(pos: Pos, diags: &mut DiagBag) -> EvalOutcome {
    diags.push(Diag {
        pos,
        code: DiagCode::TypInvalidConversion,
        message: "cannot convert constant to the target type".to_string(),
    });
    EvalOutcome::Const(Const::Unknown)
}

/// 常量是不是数值零（`/` 与 `%` 的除数判据）。
///
/// 复数要看**实部虚部都是零**：`(1+0i)` 当除数是合法的（`arithmetic` 的判据同此）。
fn is_zero(c: &Const) -> bool {
    match c {
        Const::Int(v) => v.is_zero(),
        Const::Float(r) => r.is_zero(),
        Const::Complex { re, im } => re.is_zero() && im.is_zero(),
        _ => false,
    }
}

/// 操作数**能确定**的预声明基本类型（`^` 需要，见 [`fold_unary_typed`]）。
///
/// 只在**语法自己就说得出类型**的形状上给答案 —— 不需要类型表，也没有作用域：
///
/// * 转换到预声明基本类型：`uintptr(0)` / `uint32(x)` / `byte(x)`
///   （`byte` / `rune` 是别名，折到本体）；
/// * 括号穿透：`^(uintptr(0))` 也要认。
///
/// 其余形状（字面量、具名常量、复合运算…）返回 `None`：本层**不猜**类型
/// （模块文档的边界：在值层猜类型会造出「有时误报、有时漏报」的诊断）。
/// 已知代价见 [`fold_unary_typed`] 的「已知边界」。
fn operand_basic_ty(expr: &Expr) -> Option<BasicId> {
    match expr {
        Expr::Paren(p) => operand_basic_ty(&p.x),
        Expr::Call(c) if !c.has_dots && c.args.len() == 1 => match &*c.fun {
            Expr::Name(n) => basic_from_name(&n.value),
            _ => None,
        },
        _ => None,
    }
}

/// 预声明基本类型名 → [`BasicId`]（转换目标）。
///
/// `byte` / `rune` 是**别名**（`Byte = Uint8`、`Rune = Int32`）⇒ 折到同一变体，
/// 与 `C7` §4.1 的 25 项口径一致（它们**不是**独立变体）。
///
/// ⚠️ `error` **不在这里**：它是 `Named`（`C7` 的实测结论），不是基本类型，
/// 而且它没有常量值可转换。
fn basic_from_name(name: &str) -> Option<BasicId> {
    Some(match name {
        "bool" => BasicId::Bool,
        "int" => BasicId::Int,
        "int8" => BasicId::Int8,
        "int16" => BasicId::Int16,
        "int32" => BasicId::Int32,
        "int64" => BasicId::Int64,
        "uint" => BasicId::Uint,
        "uint8" => BasicId::Uint8,
        "uint16" => BasicId::Uint16,
        "uint32" => BasicId::Uint32,
        "uint64" => BasicId::Uint64,
        "uintptr" => BasicId::Uintptr,
        "float32" => BasicId::Float32,
        "float64" => BasicId::Float64,
        "complex64" => BasicId::Complex64,
        "complex128" => BasicId::Complex128,
        "string" => BasicId::String,
        // 别名 ⇒ 折到本体
        "byte" => BasicId::Uint8,
        "rune" => BasicId::Int32,
        _ => return None,
    })
}

/// 从字面量取常量值。
///
/// **不变式**：`bad == false` ⇒ 一定解析得出值。`bad` 由 parser 在**字面量本身
/// 语法非法**时置位（`0b12`、未闭合的引号…），此时 parser 已经报过错，
/// 本层按官方 `avoid follow-up errors` 给「没有值」而**不**再报一条。
///
/// 反过来说：**非 `bad` 却解析不出值**就是「parser 的 `bad`」与
/// 「`const_from_literal` 的可解析性」两处判据不一致 —— 那是内部缺陷，
/// 在调试构建里当场炸掉（`T-TY-96` 的语料冒烟会真实地走到这条断言上）。
fn eval_lit(lit: &BasicLit) -> Const {
    if lit.bad {
        return Const::Unknown;
    }
    let v = const_from_literal(lit);
    debug_assert!(
        v != Const::Unknown,
        "内部不一致：非 bad 字面量解析不出值（{:?} {:?}）",
        lit.kind,
        lit.value
    );
    v
}
