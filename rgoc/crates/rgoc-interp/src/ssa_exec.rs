//! SSA 求值器（`M6-design.md` §3.8；`D-M6-7`；`M6-plan.md` Task 32 起）。
//!
//! # 为什么它住在 `rgoc-interp`
//!
//! `D-M6-7`：**值模型只有一个 crate**。SSA 求值器要复用 M5 的 [`Value`] / [`Cell`] /
//! `ops` / `print`，所以它必须与解释器同 crate —— 放在 `rgoc-ssa` 会让「纯数据」
//! 的 crate 拖上运行期，放第三个 crate 会造出第二套 `Value`。依赖图上就不允许
//! 第三套 runtime 存在。
//!
//! # 执行形态：显式帧 + 按块直线走
//!
//! 与 M5 的解释器同一条纪律（`D-M5-1`）：**不递归**。每个调用一帧，帧里记
//! `(函数, 当前块, 块内下标, 值表)`；主循环取「当前块的下一条指令」执行，
//! `Term` 决定下一块。这样 M8 的 goroutine 直接存/取帧栈即可。
//!
//! # 内存：运行期按**真实地址**访存，`root` 只用于静态定序
//!
//! IR 的 `Load/Store` 带显式的 `root`（`D-M6-5`），但那是**静态**信息：
//! 它让 verifier 不跑别名分析就能查「同根保序」。运行期不能按 `RootId` 分区 ——
//! `RootId` 是**函数内**的下标，而 `&a` 传给 `inc(p *int)` 之后，被调方拿到的
//! 指针必须指回**调用方**的槽。所以运行期一律走 [`Value::Ptr`] 里的真实 `Cell`，
//! `root` 字段不参与取址。
//!
//! # 当前进度
//!
//! | Task | 内容 | 状态 |
//! |---|---|---|
//! | 32 | 帧 + 主循环 + `Return` | ✅ |
//! | 33 | 纯值：`Const`/`Copy`/`Bin`/`Un`/`Convert`/`Composite`/`Make`/`Slice` + `print`/`println` | ✅ |
//! | 34 | `Goto`/`If`（块参数并行赋值）/ `Panic` | 待做 |
//! | 35 | `Load`/`Store`/`Field`/`Index`/`AddrOf` + 调用与其余内建 | 待做 |
//!
//! 没实现的指令一律 `todo!()`，**不静默跳过**（跳过一条指令等于算错，而算错不会报错）。

use std::rc::Rc;

use rgoc_hir::{BinOp, BuiltinKind, FuncId};
use rgoc_ssa::ir::{BlockId, CalleeRef, Instr, Module, Term, ValueId};
use rgoc_types::TypeId;
use rgoc_types::TypeTable;

use crate::interp::{Options, Outcome, apply_unary, const_value, convert, nil_of, slice_value};
use crate::ops::{self, RtError};
use crate::print;
use crate::value::{Cell, SliceVal, Value, cell};

/// 一个 SSA 帧。
///
/// 字段顺序按「定位 → 状态」排：先三个下标（函数 / 块 / 指令），再值表。
struct SFrame {
    /// `Module::funcs` 的下标（不是 `FuncId` —— 被拒的函数不进 `funcs`）。
    func: usize,
    /// 当前块。
    block: BlockId,
    /// 块内**下一条**待执行指令的下标（`== instrs.len()` ⇒ 该走终止符了）。
    idx: usize,
    /// 值表（下标即 `ValueId`；块参数写在这里）。
    ///
    /// `Option` 而不是裸 `Value`：`V1`/`V2` 保证「每个值都有定义点且使用处被它支配」，
    /// 运行期读到 `None` 就说明那两条不变式有漏 —— 那种错必须响亮，
    /// 不能靠预填的零值混过去（混过去的结果是「静默算错」）。
    values: Vec<Option<Value>>,
}

/// 执行状态。
struct Exec<'a> {
    m: &'a Module,
    tt: &'a TypeTable,
    frames: Vec<SFrame>,
    /// 合并输出流（与 M5 的解释器同一个语义：`print`/`println` 都进这里）。
    output: String,
    steps: u64,
    max_steps: u64,
}

/// 跑一个 SSA 模块（`Module::init` → `main`）。
#[must_use]
pub fn run_ssa(m: &Module, tt: &TypeTable, opts: Options) -> Outcome {
    let mut ex = Exec {
        m,
        tt,
        frames: Vec::new(),
        output: String::new(),
        steps: 0,
        max_steps: opts.max_steps,
    };

    // ① 包初始化（`Task 32b` 合成的那个函数）
    if let Some(i) = m.init
        && let Err(e) = ex.call(i, &[])
    {
        return ex.finish(Err(e));
    }
    // ② main
    let Some(main) = m.main else {
        return ex.finish(Err(Halt::NoMain));
    };
    let Some(i) = index_of(m, main) else {
        return ex.finish(Err(Halt::NoMain));
    };
    let r = ex.call(i, &[]);
    ex.finish(r)
}

/// 停机原因（运行期的「非正常结束」）。
enum Halt {
    /// 模块没有 `main`（或被拒了，所以 `funcs` 里找不到它）。
    NoMain,
    /// 步数超限。
    Budget,
    /// 跑到 `Unreachable`（构建 bug：可达的块不该以它终止）。
    Unreachable,
    /// 运行期错误（除零 / 越界 / nil 解引用 …）—— 文本是 Go 的 panic 文本。
    Runtime(String),
    /// `panic(v)`（`Term::Panic`）：值交给 `print::panic_text` 渲染。
    Panic(Value),
}

impl Exec<'_> {
    /// 建一帧并把它跑到底。
    fn call(&mut self, func: usize, args: &[Value]) -> Result<(), Halt> {
        let f = &self.m.funcs[func];
        let mut values: Vec<Option<Value>> = vec![None; f.value_types.len()];
        // 入口块的块参数就是形参（`C10` 规则 2）
        for (p, v) in f.params.iter().zip(args.iter()) {
            values[p.index()] = Some(v.clone());
        }
        self.frames.push(SFrame {
            func,
            block: f.entry,
            idx: 0,
            values,
        });
        self.drive()
    }

    /// 主循环：一直执行到帧栈空。
    fn drive(&mut self) -> Result<(), Halt> {
        while let Some(frame) = self.frames.last() {
            if self.steps >= self.max_steps {
                return Err(Halt::Budget);
            }
            self.steps += 1;
            let f = &self.m.funcs[frame.func];
            let block = &f.blocks[frame.block.index()];
            if frame.idx < block.instrs.len() {
                if let Some(text) = self.step_instr()? {
                    self.output.push_str(&text);
                }
                continue;
            }
            // 指令走完了 ⇒ 终止符
            match &block.term {
                // 单层（`main` / `init`）直接弹帧；被调方的返回值交接在 Task 35
                Term::Return(_) => {
                    self.frames.pop();
                }
                Term::Goto { to, args } => {
                    let (to, args) = (*to, args.clone());
                    self.jump(to, &args)?;
                }
                Term::If { cond, then_, else_ } => {
                    let (cond, then_, else_) = (*cond, then_.clone(), else_.clone());
                    let c = self.read_cur(cond)?.as_bool().unwrap_or(false);
                    let (to, args) = if c { then_ } else { else_ };
                    self.jump(to, &args)?;
                }
                Term::Panic(v) => {
                    let v = *v;
                    let val = self.read_cur(v)?;
                    return Err(Halt::Panic(val));
                }
                Term::Unreachable => return Err(Halt::Unreachable),
            }
        }
        Ok(())
    }

    /// 读当前帧里的一个值（终止符的操作数用它）。
    fn read_cur(&self, v: ValueId) -> Result<Value, Halt> {
        let frame = self.frames.last().expect("帧栈非空");
        get(&frame.values, v)
    }

    /// 跳到 `to`，并把边实参写进目标块的块参数。
    ///
    /// # 必须**先全部求值、再全部写入**（并行语义）
    ///
    /// `a, b = b, a` 编译成「跳到汇合块，实参是 `(b, a)`」。如果边算边写
    /// （先写 `a ← b`，再读 `a` 给 `b`），交换就变成「两个都是 b」。
    /// 这不是假想：`T-SSA-28` 的判据就是它（`M6-tests.md` 的 `T-SSA-52` 变异）。
    fn jump(&mut self, to: BlockId, args: &[ValueId]) -> Result<(), Halt> {
        let m = self.m;
        let frame = self.frames.last_mut().expect("帧栈非空");
        let mut vals: Vec<Value> = Vec::with_capacity(args.len());
        for a in args {
            vals.push(get(&frame.values, *a)?);
        }
        frame.block = to;
        frame.idx = 0;
        let params = m.funcs[frame.func].blocks[to.index()].params.clone();
        for (p, v) in params.iter().zip(vals) {
            frame.values[p.index()] = Some(v);
        }
        Ok(())
    }

    /// 执行当前块的当前指令，返回「要追加到输出流」的文本（`print` 家族用）。
    ///
    /// # 为什么把输出**返回**而不是直接写 `self.output`
    ///
    /// 指令执行要可变借用 `self.frames`（写值表），而 `self.output` 也是 `self` 的
    /// 字段 —— 同一函数里分字段借用可以，但跨方法调用就不行。返回文本让 `drive`
    /// 追加，等于把「写输出」挪到借用结束之后。
    fn step_instr(&mut self) -> Result<Option<String>, Halt> {
        let m = self.m;
        let tt = self.tt;
        let fr = self.frames.last().expect("帧栈非空");
        let (fi, bi, ii) = (fr.func, fr.block, fr.idx);
        let f = &m.funcs[fi];
        let ins = &f.blocks[bi.index()].instrs[ii];
        self.frames.last_mut().expect("帧栈非空").idx += 1;
        let vals = &mut self.frames.last_mut().expect("帧栈非空").values;

        match ins {
            Instr::Const { dst, c, typ } => {
                put(vals, *dst, const_ssa(c, *typ, tt)?);
                Ok(None)
            }
            Instr::Copy { dst, src } => {
                // `Copy` 是**重命名**（块参数交接 / 结果直连的落点），不是赋值：
                // 深拷贝会把「同一个值流到汇合点」变成「两份副本」。
                // 真正的值语义赋值在构建期由 `Store`/`coerce_to` 表达。
                let v = get(vals, *src)?;
                put(vals, *dst, v);
                Ok(None)
            }
            Instr::Bin {
                dst,
                op,
                lhs,
                rhs,
                typ,
            } => {
                let x = get(vals, *lhs)?;
                let y = get(vals, *rhs)?;
                let v = eval_bin(*op, &x, &y, *typ, tt)?;
                put(vals, *dst, v);
                Ok(None)
            }
            Instr::Un { dst, op, x, typ } => {
                let v = get(vals, *x)?;
                put(vals, *dst, apply_unary(*op, v, *typ, tt));
                Ok(None)
            }
            Instr::Convert { dst, x, typ } => {
                let v = get(vals, *x)?;
                let from = f.value_types[x.index()];
                put(vals, *dst, convert(tt, v, *typ, from));
                Ok(None)
            }
            Instr::Composite { dst, typ, items } => {
                let mut vs = Vec::with_capacity(items.len());
                for it in items {
                    vs.push(get(vals, *it)?);
                }
                put(vals, *dst, eval_composite(tt, *typ, vs)?);
                Ok(None)
            }
            Instr::Make { dst, typ, len, cap } => {
                let l = match len {
                    Some(x) => Some(get(vals, *x)?),
                    None => None,
                };
                let c = match cap {
                    Some(x) => Some(get(vals, *x)?),
                    None => None,
                };
                put(vals, *dst, eval_make(tt, *typ, l, c)?);
                Ok(None)
            }
            Instr::Slice {
                dst,
                base,
                lo,
                hi,
                max,
            } => {
                let b = get(vals, *base)?;
                let l = match lo {
                    Some(x) => Some(get(vals, *x)?),
                    None => None,
                };
                let h = match hi {
                    Some(x) => Some(get(vals, *x)?),
                    None => None,
                };
                let mx = match max {
                    Some(x) => Some(get(vals, *x)?),
                    None => None,
                };
                let v = slice_value(&b, l, h, mx, TypeId::from_index(0)).map_err(rt)?;
                put(vals, *dst, v);
                Ok(None)
            }
            Instr::Call {
                dsts, callee, args, ..
            } => {
                // `panic(v)` 正常应由构建期落成 `Term::Panic`；这里兜住「漏网的」
                // （防御：真出现时语义仍与 Go 一致，而不是 panic 到宿主）
                if let CalleeRef::Builtin(BuiltinKind::Panic) = callee {
                    let v = match args.first() {
                        Some(a) => get(vals, *a)?,
                        None => Value::Nil(crate::value::NilKind::Iface),
                    };
                    return Err(Halt::Panic(v));
                }
                if let CalleeRef::Builtin(k @ (BuiltinKind::Print | BuiltinKind::Println)) = callee
                {
                    let mut out = String::new();
                    for (i, a) in args.iter().enumerate() {
                        let v = get(vals, *a)?;
                        if *k == BuiltinKind::Println && i > 0 {
                            out.push(' ');
                        }
                        out.push_str(&print::format_operand(&v, tt, f.value_types[a.index()]));
                    }
                    if *k == BuiltinKind::Println {
                        out.push('\n');
                    }
                    debug_assert!(dsts.is_empty(), "print 家族没有结果");
                    return Ok(Some(out));
                }
                todo!("M6/Task 35：调用与其余内建还没进来")
            }
            other => todo!(
                "M6/Task 35：指令 `{}` 的执行还没进来",
                rgoc_ssa::ir::kind_of_instr(other)
            ),
        }
    }

    /// 收口成 [`Outcome`]。
    fn finish(&mut self, r: Result<(), Halt>) -> Outcome {
        let output = std::mem::take(&mut self.output);
        let steps = self.steps;
        match r {
            Ok(()) => Outcome {
                output,
                exit: 0,
                diag: None,
                steps,
            },
            // `panic(v)`：与 M5 同一个渲染（`panic: <文本>` 进合并流、退出码 2）
            Err(Halt::Panic(v)) => Outcome {
                output: format!("{output}panic: {}\n", print::panic_text(&v)),
                exit: 2,
                diag: None,
                steps,
            },
            // 运行期错误与 M5 的 panic 同一条通道：文本进合并流、退出码 2
            Err(Halt::Runtime(msg)) => Outcome {
                output: format!("{output}panic: {msg}\n"),
                exit: 2,
                diag: None,
                steps,
            },
            Err(Halt::Budget) => Outcome {
                output,
                exit: 2,
                diag: Some(format!("步数超限（--max-steps {}）", self.max_steps)),
                steps,
            },
            Err(Halt::NoMain) => Outcome {
                output,
                exit: 2,
                diag: Some("模块里没有 main（或被拒绝，没进 funcs）".to_string()),
                steps,
            },
            Err(Halt::Unreachable) => Outcome {
                output,
                exit: 2,
                diag: Some("执行到了 Unreachable 块（构建 bug：可达块不该以它终止）".to_string()),
                steps,
            },
        }
    }
}

/// 读一个值（未定义 ⇒ 内部错，见 `SFrame::values` 的说明）。
fn get(vals: &[Option<Value>], v: ValueId) -> Result<Value, Halt> {
    vals.get(v.index())
        .and_then(|x| x.as_ref())
        .cloned()
        .ok_or_else(|| Halt::Runtime(format!("读未定义的值 v{}（V1/V2 有漏）", v.0)))
}

fn put(vals: &mut [Option<Value>], v: ValueId, val: Value) {
    vals[v.index()] = Some(val);
}

fn rt(e: RtError) -> Halt {
    Halt::Runtime(e.message())
}

/// `ConstVal` → 值，按 SSA 的「`Int(0)` = 该类型的零值」约定（`R-20-1`）。
fn const_ssa(c: &rgoc_hir::types::ConstVal, typ: TypeId, tt: &TypeTable) -> Result<Value, Halt> {
    if let rgoc_hir::types::ConstVal::Int(0) = c
        && !is_numeric(tt, typ)
    {
        return Ok(zero_of(tt, typ));
    }
    Ok(const_value(c, typ, tt))
}

fn is_numeric(tt: &TypeTable, t: TypeId) -> bool {
    match ops::basic_of(tt, t) {
        Some(b) => b.is_integer() || b.is_float() || b.is_complex(),
        None => false,
    }
}

/// 某个类型的零值。
///
/// # 聚合类型还只有 `nil` 兜底
///
/// `[3]int` / `struct{a int}` 的零值要逐元素递归（数组长度还得查常量表）。
/// Task 33 只做**纯值**，聚合零值到 Task 35（帧建立与 `make` 的真实元素）一起补。
fn zero_of(tt: &TypeTable, t: TypeId) -> Value {
    use rgoc_types::BasicId;
    match ops::basic_of(tt, t) {
        Some(BasicId::String) => Value::Str("".into()),
        Some(BasicId::Bool) => Value::Bool(false),
        Some(b) if b.is_integer() => Value::Int(0),
        Some(b) if b.is_float() => Value::Float(0.0),
        Some(b) if b.is_complex() => Value::Complex(0.0, 0.0),
        _ => nil_of(tt, t),
    }
}

/// 二元运算：比较走 `ops::compare`，算术走 `ops::bin_arith`（**不重写**）。
fn eval_bin(op: BinOp, x: &Value, y: &Value, typ: TypeId, tt: &TypeTable) -> Result<Value, Halt> {
    match op {
        BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
            Ok(Value::Bool(ops::compare(op, x, y).unwrap_or(false)))
        }
        // `&&` / `||` 在构建期已经拆成控制流（`Task 33b`）：出现即构建 bug
        BinOp::LAnd | BinOp::LOr => Err(Halt::Runtime(
            "逻辑运算出现在 `Bin` 里（短路应在构建期拆成控制流）".to_string(),
        )),
        _ => ops::bin_arith(op, x, y, typ, tt).map_err(rt),
    }
}

/// 复合字面量：按**静态类型**决定装成结构体、数组还是 slice。
///
/// `items` 的顺序就是元素/字段顺序（构建期已按 keyed 下标落位，`Task 29`）。
fn eval_composite(tt: &TypeTable, typ: TypeId, items: Vec<Value>) -> Result<Value, Halt> {
    use rgoc_types::typ::Type;
    let cells: Vec<Cell> = items.into_iter().map(cell).collect();
    match tt.type_of(rgoc_types::underlying(tt, typ)) {
        Type::Struct(_) => Ok(Value::Struct(Rc::new(cells))),
        Type::Array { .. } => Ok(Value::Array(Rc::new(cells))),
        Type::Slice(_) => {
            let n = cells.len();
            Ok(Value::Slice(SliceVal {
                arr: Rc::new(cells),
                off: 0,
                len: n,
                cap: n,
            }))
        }
        other => Err(Halt::Runtime(format!(
            "复合字面量的静态类型不是聚合：{other:?}"
        ))),
    }
}

/// `make([]T, len[, cap])` 与 `new(T)`（`len`/`cap` 都为 `None` 且类型是指针）。
fn eval_make(
    tt: &TypeTable,
    typ: TypeId,
    len: Option<Value>,
    cap: Option<Value>,
) -> Result<Value, Halt> {
    use rgoc_types::typ::Type;
    let n = |v: &Option<Value>, dflt: usize| -> usize {
        v.as_ref()
            .and_then(Value::as_int)
            .map_or(dflt, |i| i.max(0) as usize)
    };
    match tt.type_of(rgoc_types::underlying(tt, typ)) {
        Type::Slice(elem) => {
            let l = n(&len, 0);
            let c = n(&cap, l).max(l);
            let zero = zero_of(tt, *elem);
            let cells: Vec<Cell> = (0..c).map(|_| cell(zero.clone())).collect();
            Ok(Value::Slice(SliceVal {
                arr: Rc::new(cells),
                off: 0,
                len: l,
                cap: c,
            }))
        }
        // `new(T)`：分配一个零值并返回它的地址
        Type::Pointer(inner) => Ok(Value::Ptr(cell(zero_of(tt, *inner)))),
        other => Err(Halt::Runtime(format!(
            "`make` 的类型既不是 slice 也不是指针：{other:?}"
        ))),
    }
}

/// `FuncId` → `funcs` 下标（被拒的函数不在 `funcs` 里，所以可能找不到）。
fn index_of(m: &Module, fid: FuncId) -> Option<usize> {
    m.funcs.iter().position(|f| f.src == fid)
}
