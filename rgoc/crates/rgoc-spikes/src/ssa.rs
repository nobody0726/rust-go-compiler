//! S2 —— SSA spike 的数据形状：**Block / Value 图**（`M0-plan.md` T43 / `M0-tests.md` §5.2）。
//!
//! # 与 S1 的关系（本文件存在的主要理由）
//!
//! `T-S2-01` 要求「构造出 Block / Value 图；**求值结果与 `T-S1-01` 一致**」。
//! 「一致」必须是**逐字节**的同一份结果，而不是「看起来一样」—— 所以 S2 的求值器
//! **返回同一个 [`RunOutput`] 类型**（来自 `crate::interp`），而不是另造一个。
//! 另造类型的话，「一致」就退化成两个结构体之间的比较，而比较本身可以随便放宽。
//!
//! # ⚠️ 图里必须真的有 `1 + 2` 的**结构**，不能是求值后的 `3`
//!
//! HIR 的 `Stmt::Print` 存的是**已求值的 `Val`**（内建调用边界画在 HIR 里，见 `crate::interp`）。
//! 若 S2 直接把这些 `Val` 搬进图，那图里就只有两条 `const 3` ——
//! 「构造出 Block/Value 图」就退化成「搬运 HIR 的求值结果」，**S2 什么也没验证**。
//!
//! 所以 S2 的路径必须是**独立的**：
//!
//! ```text
//! S1:  Expr(1+2) ──eval_const()──► Val(3) ──► print ──► "3\n"
//! S2:  Expr(1+2) ──lower()──► Block/Value 图 ──eval()──► Val(3) ──► print ──► "3\n"
//! ```
//!
//! 两条路都从**同一份 `Expr`** 出发（`fixtures::sum_expr()`），这才是交叉验证的意义：
//! 若它们结果不同，说明「常量折叠」与「图求值」对同一形状的理解有分歧。
//!
//! # 值表示复用，不另起一套
//!
//! [`Val`] 与 `BigInt` 直接来自 `rgoc-hir`。理由与 S1 相同：三个 spike 共享一份值表示
//! （**D-M0-11**），若 S2 改用 `i128` 之类更窄的类型，S1/S2 的结果一致性就失去意义。

use std::collections::BTreeMap;
use std::fmt;

use crate::interp::RunOutput;
use rgoc_hir::value::BigInt;
use rgoc_hir::{BinOp, Const, Expr, Program, Stmt, Stream, Val};

/// 一个 SSA 值。
///
/// **「唯一定义点」是 SSA 的定义性质**，所以值用 ID 引用而不是嵌套树 ——
/// 若用嵌套树，SSA 与普通表达式树就没有区别了，`T-S2-01` 也就没验证到东西。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ValueId(usize);

impl fmt::Display for ValueId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // 形如 `v3` —— 与 Go 编译器内部命名一致，便于在记录里对照。
        write!(f, "v{}", self.0)
    }
}

/// 一条指令：定义一个值。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Instr {
    /// 常量。
    Const(Val),
    /// 二元运算。
    Bin {
        /// 运算符。
        op: BinOp,
        /// 左操作数的定义点。
        lhs: ValueId,
        /// 右操作数的定义点。
        rhs: ValueId,
    },
}

impl Instr {
    /// 本指令定义的值所**引用**的其他值 ID（用于支配关系的初步检查）。
    pub fn operands(&self) -> Vec<ValueId> {
        match self {
            Self::Const(_) => Vec::new(),
            Self::Bin { lhs, rhs, .. } => vec![*lhs, *rhs],
        }
    }
}

/// 一个基本块。**子集内函数体是直线，故只有一块**；但形状先立住。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// 块名（入口块固定 `entry`）。
    pub name: String,
    /// 指令序列。
    pub instrs: Vec<(ValueId, Instr)>,
    /// 后继块。直线程序 ⇒ 空。
    pub succs: Vec<String>,
    /// 块参数（phi 的容身处）。直线程序 ⇒ 空。
    ///
    /// 保留它是为了让 `T-S2-02` 的「phi 需要什么」有一个**具体的对照物**，
    /// 而不是一句「将来要加 phi」。
    pub params: Vec<ValueId>,
}

/// SSA 函数：块的集合 + 入口块名 + 入口参数（print 的实参在图里的定义点）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsaFunc {
    /// 函数名。
    pub name: String,
    /// 块表。用 `BTreeMap` 而非 `HashMap` —— **顺序必须确定**，
    /// 否则 E6「重复三次结果一致」会随机失败（同 `rgoc_hir::DiagBag` 的理由）。
    pub blocks: BTreeMap<String, Block>,
    /// 入口块名。
    pub entry: String,
    /// 传给内建 `print` 的值 ID 列表（按参数顺序）。
    pub print_args: Vec<ValueId>,
    /// `true` = `println`（空格分隔 + 末尾换行），`false` = `print`。
    pub print_newline: bool,
    /// 输出流。与 `print_newline` 一样是**必填**：S1 用 stderr、S3 用 stdout，
    /// 不给默认值是为了避免「忘了填就默认写 stdout」这类静默错误。
    pub print_stream: Stream,
}

impl SsaFunc {
    /// 入口块。
    ///
    /// 返回 `Result` 而不是 `unwrap`/`panic`：入口块缺失是**子集外形状**，
    /// 而「基建失败」与「语义不匹配」必须可分（`M0-tests.md` §1.2 的判定纪律）。
    pub fn entry_block(&self) -> Result<&Block, String> {
        self.blocks
            .get(&self.entry)
            .ok_or_else(|| format!("入口块 {:?} 不存在", self.entry))
    }

    /// 图的规模摘要（供记录与断言用）。
    pub fn summary(&self) -> SsaSummary {
        let n_instrs: usize = self.blocks.values().map(|b| b.instrs.len()).sum();
        SsaSummary {
            blocks: self.blocks.len(),
            instrs: n_instrs,
            succs: self.blocks.values().map(|b| b.succs.len()).sum(),
            params: self.blocks.values().map(|b| b.params.len()).sum(),
        }
    }

    /// 渲染成可读文本（供 T45 落盘、供人工复核图的结构）。
    pub fn render(&self) -> String {
        let mut s = format!("func {}\n", self.name);
        for b in self.blocks.values() {
            s.push_str(&format!("{}:\n", b.name));
            for (id, instr) in &b.instrs {
                let desc = match instr {
                    Instr::Const(v) => format!("const {v}"),
                    Instr::Bin { op, lhs, rhs } => {
                        format!("{} = {lhs} {} {rhs}", id, op_char(*op))
                    }
                };
                s.push_str(&format!("\t{desc}\n"));
            }
            if !b.params.is_empty() {
                s.push_str(&format!("\t# params: {:?}\n", b.params));
            }
            if !b.succs.is_empty() {
                s.push_str(&format!("\t# succs: {:?}\n", b.succs));
            }
        }
        s.push_str(&format!("# print args: {:?}\n", self.print_args));
        s
    }
}

fn op_char(op: BinOp) -> char {
    match op {
        BinOp::Add => '+',
        BinOp::Shl => '\u{2033}',
        BinOp::Mul => '*',
    }
}

/// 图规模摘要。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SsaSummary {
    /// 块数。
    pub blocks: usize,
    /// 指令数。
    pub instrs: usize,
    /// 后继边数。
    pub succs: usize,
    /// 块参数（phi）数。
    pub params: usize,
}

/// lowering 失败 —— 与「求值结果不符」是**两件事**，故单独一类。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LowerError {
    /// 固定输入里没有 `main`。
    NoMain,
    /// 表达式形状不在子集内。
    UnsupportedExpr(String),
    /// 引用了未定义的值 ID。
    UndefinedValue(ValueId),
    /// 内部不变式被破坏：lowering 结束时仍有未解析的表达式。
    DanglingExpr,
}

impl fmt::Display for LowerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoMain => f.write_str("固定输入必须含 main"),
            Self::UnsupportedExpr(s) => write!(f, "S2 子集不支持该表达式：{s}"),
            Self::UndefinedValue(v) => write!(f, "引用了未定义的值 {v}"),
            Self::DanglingExpr => f.write_str("lowering 结束时有悬空表达式（内部不变式被破坏）"),
        }
    }
}

impl From<LowerError> for String {
    /// 让 `?` 能在返回 `Result<_, String>` 的函数里直接传播 lowering 错误。
    ///
    /// 写成 `impl From` 而不是让每个调用点写 `.map_err(|e| e.to_string())` ——
    /// 后者漏一处就是一个难查的类型错误，而这里**只有一处**需要维护。
    fn from(e: LowerError) -> String {
        e.to_string()
    }
}

/// lowering 的可变状态：指令表 + 下一个 ID。
#[derive(Debug, Default)]
struct Builder {
    instrs: Vec<(ValueId, Instr)>,
    next: usize,
}

impl Builder {
    /// 定义一个新值，返回其 ID。
    fn define(&mut self, instr: Instr) -> ValueId {
        let id = ValueId(self.next);
        self.next += 1;
        self.instrs.push((id, instr));
        id
    }

    /// lowering 一个表达式，返回它的值 ID。
    fn expr(&mut self, e: &Expr) -> Result<ValueId, LowerError> {
        match e {
            Expr::Const(Const::Big(b)) => Ok(self.define(Instr::Const(Val::Int(b.clone())))),
            Expr::Const(Const::Bool(x)) => Ok(self.define(Instr::Const(Val::Str(x.to_string())))),
            Expr::Const(Const::Str(s)) => Ok(self.define(Instr::Const(Val::Str(s.clone())))),
            Expr::Bin { op, lhs, rhs } => {
                // **后序遍历**：先 lower 两侧，再建本条指令。
                // 顺序即定义顺序，SSA 的「唯一定义点」与「使用在定义之后」由此成立 ——
                // 递归里写成先序（先建本条再 lower 两侧）就会出现「使用先于定义」。
                let a = self.expr(lhs)?;
                let b = self.expr(rhs)?;
                Ok(self.define(Instr::Bin {
                    op: *op,
                    lhs: a,
                    rhs: b,
                }))
            }
        }
    }
}

/// 把 HIR 程序 lowering 成 SSA 图。
///
/// 只支持直线语句序列 + 内建 print 调用。遇到子集外形状返回 [`LowerError`]，
/// **不 panic、不猜**。
pub fn lower(p: &Program) -> Result<SsaFunc, LowerError> {
    let main = p.main().ok_or(LowerError::NoMain)?;
    let mut b = Builder::default();
    let mut print_args: Vec<ValueId> = Vec::new();
    let mut print_newline = true;
    let mut print_stream = Stream::Stderr;

    for stmt in &main.body {
        match stmt {
            Stmt::Print {
                newline,
                stream,
                args,
            } => {
                print_newline = *newline;
                print_stream = *stream;
                // 实参在 HIR 里是**已求值的 Val**。图里要装的是**表达式**，
                // 所以这里对每个实参反向构造一个 const 指令（保底），
                // 真正的表达式形状由 `lower_expr_program` 那条路径提供。
                for a in args {
                    let id = match a {
                        Val::Int(v) => b.define(Instr::Const(Val::Int(v.clone()))),
                        Val::Str(s) => b.define(Instr::Const(Val::Str(s.clone()))),
                        Val::Void => {
                            return Err(LowerError::UnsupportedExpr("内建实参是 Void".to_string()));
                        }
                    };
                    print_args.push(id);
                }
            }
            Stmt::Return(_) => {
                return Err(LowerError::UnsupportedExpr(
                    "main 里的 return（子集内不出现）".to_string(),
                ));
            }
        }
    }

    if print_args.is_empty() {
        return Err(LowerError::DanglingExpr);
    }

    let mut blocks = BTreeMap::new();
    blocks.insert(
        "entry".to_string(),
        Block {
            name: "entry".to_string(),
            instrs: b.instrs,
            succs: Vec::new(),
            params: Vec::new(),
        },
    );
    Ok(SsaFunc {
        name: main.name.clone(),
        blocks,
        entry: "entry".to_string(),
        print_args,
        print_newline,
        print_stream,
    })
}

/// 从**表达式**直接 lowering 出 SSA 图（S2 的主路径）。
///
/// 与 [`lower`] 的区别：这里输入是 `Expr`（`1 + 2` 的结构），图里因此有一条
/// `bin` 指令，而不是只有 const。这是「S2 验证了图求值」与「S2 只是搬运」的分界。
pub fn lower_expr_program(
    name: &str,
    args: &[Expr],
    newline: bool,
    stream: Stream,
) -> Result<SsaFunc, LowerError> {
    let mut b = Builder::default();
    let mut print_args = Vec::new();
    for e in args {
        print_args.push(b.expr(e)?);
    }
    let mut blocks = BTreeMap::new();
    blocks.insert(
        "entry".to_string(),
        Block {
            name: "entry".to_string(),
            instrs: b.instrs,
            succs: Vec::new(),
            params: Vec::new(),
        },
    );
    Ok(SsaFunc {
        name: name.to_string(),
        blocks,
        entry: "entry".to_string(),
        print_args,
        print_newline: newline,
        print_stream: stream,
    })
}

/// SSA 求值：产出**与 S1 同类型**的结果（逐字节可比）。
pub fn eval(f: &SsaFunc) -> Result<RunOutput, String> {
    let block = f.entry_block()?;
    let mut env: BTreeMap<ValueId, Val> = BTreeMap::new();
    let mut stderr = String::new();
    let mut stdout = String::new();

    for (id, instr) in &block.instrs {
        let v = match instr {
            Instr::Const(v) => v.clone(),
            Instr::Bin { op, lhs, rhs } => {
                let a = env
                    .get(lhs)
                    .ok_or(LowerError::UndefinedValue(*lhs))?
                    .clone();
                let b = env
                    .get(rhs)
                    .ok_or(LowerError::UndefinedValue(*rhs))?
                    .clone();
                eval_bin(*op, &a, &b)?
            }
        };
        env.insert(*id, v);
    }

    let args: Vec<Val> = f
        .print_args
        .iter()
        .map(|id| {
            env.get(id)
                .cloned()
                .ok_or_else(|| LowerError::UndefinedValue(*id).to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;

    // print 的分隔规则与 S1 **共用同一份实现语义**（`crate::interp` 里已由 oracle 校准）：
    // println → 空格分隔 + 末尾换行；print → 直接拼接。
    // 这里刻意重写而不是调用 S1 的渲染：S2 必须**独立**得出同一个字节串，
    // 否则「结果一致」就变成「调用了同一个函数当然一致」。
    let rendered: Vec<String> = args.iter().map(Val::render).collect();
    let text = if f.print_newline {
        // println：空格分隔 + 末尾换行
        let mut t = rendered.join(" ");
        t.push('\n');
        t
    } else {
        // print：直接拼接
        rendered.concat()
    };
    // 按 fixture 声明的流写。不写死 stderr —— 理由同 `rgoc_hir::Stream` 的文档。
    match f.print_stream {
        Stream::Stderr => stderr.push_str(&text),
        Stream::Stdout => stdout.push_str(&text),
    }

    Ok(RunOutput { stderr, stdout })
}

/// 二元运算求值（S2 的语义核心）。
fn eval_bin(op: BinOp, a: &Val, b: &Val) -> Result<Val, String> {
    let ai = as_int(a)?;
    let bi = as_int(b)?;
    match op {
        BinOp::Add => Ok(Val::Int(ai.add(&bi))),
        BinOp::Shl => {
            if bi.is_negative() {
                return Err("S2 子集不支持负移位量".to_string());
            }
            let bits = u32::try_from(
                bi.to_int64()
                    .ok_or_else(|| "移位量无法收敛到 i64".to_string())?,
            )
            .map_err(|_| "移位量超出 u32".to_string())?;
            ai.shl(bits)
                .map(Val::Int)
                .ok_or_else(|| "移位量超出 S2 预算".to_string())
        }
        BinOp::Mul => Err("S2 子集不支持乘法".to_string()),
    }
}

fn as_int(v: &Val) -> Result<BigInt, String> {
    match v {
        Val::Int(b) => Ok(b.clone()),
        Val::Str(_) | Val::Void => Err("S2 只支持整数运算".to_string()),
    }
}
