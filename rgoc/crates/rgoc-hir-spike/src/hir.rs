//! HIR 形状 —— 三个 spike 共享的**固定**输入。
//!
//! # 「固定」是硬要求，不是偷懒
//!
//! `M0-plan.md` T41 明确：三个 spike 的**输入必须写死**，不接受命令行传入可变输入 ——
//! 否则 E6「重复执行 3 次结果一致」无从判定（传不同输入当然不一致，那不叫可复现）。
//! 所以本模块只提供**构造器**，不提供 `from_source(&str)` 之类的解析入口。
//!
//! 支持范围仅限三个 fixture：`println(1 + 2)`、常量折叠边界（`1 << 100` 等）、
//! 输出 `hello` 的 native 函数。**遇到子集外的形状由 [`Expr::eval_const`] 返回 `None`**
//! 表示「不适用」，而不是给一个猜出来的值 —— 那样 S1 与 S2 的交叉验证就失去了区分能力。

use crate::value::{BigInt, Val};

/// 常量节点。
///
/// 关键区分：**`Int`（已收敛到具体类型）vs `Big`（仍是任意精度的 untyped constant）**。
/// 两者混为一谈就是 T-S1-03 要抓的错误 —— 一旦常量在 HIR 里就变成 `i64`，
/// `1 << 100` 会在**折叠那一步**静默丢位，而 `1 + 2` 仍然正确，测试照样绿。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Const {
    /// 无类型常量：大整数高精度值，**在被赋给具体类型时才收敛**。
    Big(BigInt),
    /// 布尔字面量。
    Bool(bool),
    /// 字符串字面量（内容已解转义；本子集里没有字符串字面量的 HIR 构造入口）。
    Str(String),
}

impl Const {
    /// 收敛到 `i64`（溢出即 `None`，对应 Go 的 `overflows` 诊断）。
    pub fn as_int64(&self) -> Option<i64> {
        match self {
            Self::Big(b) => b.to_int64(),
            Self::Bool(_) | Self::Str(_) => None,
        }
    }

    /// 变成运行时值。**不做类型检查** —— 调用方负责在收敛失败时分类。
    pub fn to_val(&self) -> Val {
        match self {
            Self::Big(b) => Val::Int(b.clone()),
            Self::Bool(b) => Val::Str(b.to_string()),
            Self::Str(s) => Val::Str(s.clone()),
        }
    }
}

/// 二元运算。**只列三个 fixture 用到的**，不做完整算术。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    /// `+`
    Add,
    /// `<<`
    Shl,
    /// `*`
    Mul,
}

/// 表达式。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    /// 常量。
    Const(Const),
    /// 二元运算。
    Bin {
        /// 运算符。
        op: BinOp,
        /// 左操作数。
        lhs: Box<Expr>,
        /// 右操作数。
        rhs: Box<Expr>,
    },
}

/// 语句。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    /// 内建 `print` / `println` 调用（`M0-plan.md` T42 要求记录「内建调用边界」）。
    ///
    /// `args` 是**已求值的值**而不是 `Expr`：内建调用边界就画在这里 ——
    /// 参数在进入内建之前完成求值，内建本身只负责按 print 家族语义渲染。
    Print {
        /// `true` = `println`（操作数间加空格、末尾加 `\n`）；`false` = `print`。
        newline: bool,
        /// **输出流**。这不是可省字段 —— 见 [`Stream`] 的文档。
        stream: Stream,
        /// 已求值的实参。
        args: Vec<Val>,
    },
    /// `return`（S3 的 native 函数需要一个显式返回 0 的出口）。
    Return(Val),
}

/// 输出流。
///
/// # 为什么必须是类型层面的区分，而不是注释里的一句话
///
/// Go 的内建 `print` / `println` 写**标准错误**（实测 go1.27.1，`od -c` 逐字节：
/// stdout 长度为 0）。而 S3 的 native fixture 要往 **标准输出**写 `hello\n`
/// （`T-S3-03` 的冻结期望，对应真实 Go 程序的 `os.Stdout`）。
///
/// 这两件事在 S1 与 S3 里**恰好都是「打印一个值」**，若只用注释区分，
/// 迟早有人把它们「统一」掉 —— 而那会同时打破 `T-S1-01`（stdout 必须精确为空）
/// 与 `T-S3-03`（stdout 必须精确 `hello\n`）中的**一个**。
/// 做成枚举后，编译器层面就不允许混淆：S1 必须写 [`Stream::Stderr`]，
/// S3 必须写 [`Stream::Stdout`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stream {
    /// 标准错误。**Go 内建 `print` / `println` 的实际去向**（实测依据见上）。
    Stderr,
    /// 标准输出。对应 Go 程序的 `os.Stdout` / `fmt.Println` 的效果。
    Stdout,
}

impl Expr {
    /// 常量折叠。**只支持本子集**，遇到别的形状返回 `None`。
    ///
    /// `None` 的含义是「本 spike 不覆盖这个形状」，调用方必须显式分类 ——
    /// 这是 `M0-tests.md` §1.2 判定分类纪律在 spike 内的同形要求：
    /// 「能力不足」与「算错了」必须能分开。
    pub fn eval_const(&self) -> Option<Val> {
        match self {
            Self::Const(c) => Some(c.to_val()),
            Self::Bin { op, lhs, rhs } => {
                let l = as_int(&lhs.eval_const()?)?;
                let r = as_int(&rhs.eval_const()?)?;
                match op {
                    BinOp::Add => Some(Val::Int(l.add(&r))),
                    BinOp::Shl => {
                        // 移位量必须是非负且在预算内，否则 `None`（不静默取模）
                        if r.is_negative() {
                            return None;
                        }
                        let bits = u32::try_from(r.to_int64()?).ok()?;
                        l.shl(bits).map(Val::Int)
                    }
                    BinOp::Mul => {
                        // 子集内没做乘法（fixture 不需要）；显式不支持
                        let _ = (l, r);
                        None
                    }
                }
            }
        }
    }
}

/// 从 `Val` 取出整数；非整数返回 `None`（类型不匹配，不是 0）。
fn as_int(v: &Val) -> Option<BigInt> {
    match v {
        Val::Int(b) => Some(b.clone()),
        Val::Str(_) | Val::Void => None,
    }
}

/// 函数声明（S3 native spike 的输入）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuncDecl {
    /// 函数名（导出符号名）。
    pub name: String,
    /// 函数体（线性语句序列；本子集不需要控制流）。
    pub body: Vec<Stmt>,
}

/// 程序：S1 与 S2 的输入。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    /// 函数表。`main` 必须存在。
    pub funcs: Vec<FuncDecl>,
}

impl Program {
    /// 按名查找函数。
    pub fn func(&self, name: &str) -> Option<&FuncDecl> {
        self.funcs.iter().find(|f| f.name == name)
    }

    /// `main` 的便捷访问器。
    ///
    /// 返回 `Option` 而不是 `&FuncDecl`：三个 fixture 之外**可能没有 `main`**，
    /// 而「没有 main」在 spike 里应被显式报出，不是 panic。
    pub fn main(&self) -> Option<&FuncDecl> {
        self.func("main")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn int(v: i64) -> Expr {
        Expr::Const(Const::Big(BigInt::from_i64(v)))
    }

    #[test]
    fn 常量折叠加法() {
        // S1 的固定 fixture：println(1 + 2)
        let e = Expr::Bin {
            op: BinOp::Add,
            lhs: Box::new(int(1)),
            rhs: Box::new(int(2)),
        };
        assert_eq!(e.eval_const(), Some(Val::Int(BigInt::from_i64(3))));
    }

    #[test]
    fn 常量折叠移位产生超_i64_的常量() {
        // T-S1-03：1 << 100 必须在 HIR 层以任意精度存在
        let e = Expr::Bin {
            op: BinOp::Shl,
            lhs: Box::new(int(1)),
            rhs: Box::new(int(100)),
        };
        let v = e.eval_const().expect("应可折叠");
        let Val::Int(b) = v else {
            panic!("应是整数");
        };
        assert_eq!(b.to_string(), "1267650600228229401496703205376");
        // 这是与 T-S1-03 的接口：装得下 BigInt，装不下 i64
        assert_eq!(b.to_int64(), None);
    }

    #[test]
    fn 子集外形状返回_none_而不是猜一个值() {
        // 移位量过大 → 明确不支持
        let e = Expr::Bin {
            op: BinOp::Shl,
            lhs: Box::new(int(1)),
            rhs: Box::new(int(1000)),
        };
        assert_eq!(e.eval_const(), None, "超预算移位必须返回 None");

        // 负移位量 → 明确不支持
        let e = Expr::Bin {
            op: BinOp::Shl,
            lhs: Box::new(int(1)),
            rhs: Box::new(int(-1)),
        };
        assert_eq!(e.eval_const(), None, "负移位量必须返回 None");

        // 乘法在子集外
        let e = Expr::Bin {
            op: BinOp::Mul,
            lhs: Box::new(int(2)),
            rhs: Box::new(int(3)),
        };
        assert_eq!(e.eval_const(), None, "子集外的乘法必须返回 None");

        // 类型不匹配：字符串参与算术
        let e = Expr::Bin {
            op: BinOp::Add,
            lhs: Box::new(Expr::Const(Const::Str("x".to_string()))),
            rhs: Box::new(int(1)),
        };
        assert_eq!(e.eval_const(), None, "字符串参与算术必须返回 None");
    }

    #[test]
    fn 常量收敛溢出时给出_none() {
        let big = BigInt::from_i64(1).shl(100).expect("ok");
        assert_eq!(Const::Big(big).as_int64(), None);
        let ok = Const::Big(BigInt::from_i64(42));
        assert_eq!(ok.as_int64(), Some(42));
        // 布尔不是整数
        assert_eq!(Const::Bool(true).as_int64(), None);
    }

    #[test]
    fn program_按名查找函数() {
        let p = Program {
            funcs: vec![FuncDecl {
                name: "main".to_string(),
                body: vec![Stmt::Print {
                    newline: true,
                    stream: Stream::Stderr,
                    args: vec![Val::Int(BigInt::from_i64(3))],
                }],
            }],
        };
        assert!(p.main().is_some());
        assert!(p.func("nope").is_none());
    }

    #[test]
    fn 没有_main_时返回_none_而不是_panic() {
        let p = Program {
            funcs: vec![FuncDecl {
                name: "helper".to_string(),
                body: vec![],
            }],
        };
        assert_eq!(p.main(), None, "无 main 必须可被显式判出");
    }
}
