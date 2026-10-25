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
//! `(函数, 当前块, 块内下标, 值表, 根存储)`；主循环取「当前块的下一条指令」执行，
//! `Term` 决定下一块。`Call` 只是**压一帧**然后回到主循环 —— 被调方返回时
//! 把值写进调用方的 `ret_to` 位置。这样 M8 的 goroutine 直接存/取帧栈即可。
//!
//! # 内存：运行期按**真实地址**访存，`root` 只用于静态定序
//!
//! IR 的 `Load/Store` 带显式的 `root`（`D-M6-5`），但那是**静态**信息：
//! 它让 verifier 不跑别名分析就能查「同根保序」。运行期不能按 `RootId` 分区 ——
//! `RootId` 是**函数内**的下标，而 `&a` 传给 `inc(p *int)` 之后，被调方拿到的
//! 指针必须指回**调用方**的槽。所以运行期一律走 [`Value::Ptr`] 里的真实 `Cell`，
//! `root` 字段不参与取址。
//!
//! 三个根的**存储**分别来自：
//!
//! | 根 | 存储 |
//! |---|---|
//! | `Root::Global(g)` | 模块级 `globals[g]`（跨帧共享，`init` 写的 `main` 看得见） |
//! | `Root::Slot(s)` | 本帧 `roots[i]`（进帧时按 `slot_types[s]` 建零值） |
//! | `Root::Heap` | 本帧 `roots[i]`（**占位**：真正的堆对象由 `Make`/`Composite` 造，地址走 `Value::Ptr`） |
//!
//! 「进帧时按类型建零值」不是可选的优化：`var a [3]int` 的声明**不产生任何指令**
//! （零值由 `AddrOf` 的底座隐式给出），若那块存储是 `nil`，`a[1] = 7` 的
//! `Index` 当场失败。
//!
//! # 当前进度
//!
//! | Task | 内容 | 状态 |
//! |---|---|---|
//! | 32 | 帧 + 主循环 + `Return` | ✅ |
//! | 33 | 纯值：`Const`/`Copy`/`Bin`/`Un`/`Convert`/`Composite`/`Make`/`Slice` + `print`/`println` | ✅ |
//! | 34 | `Goto`/`If`（块参数并行赋值）/ `Panic` | ✅ |
//! | 35 | `Load`/`Store`/`Field`/`Index`/`AddrOf` + 调用与其余内建 | ✅ |
//!
//! 没实现的指令一律 `todo!()`，**不静默跳过**（跳过一条指令等于算错，而算错不会报错）。

use rgoc_hir::{BinOp, BuiltinKind, FuncId};
use rgoc_ssa::ir::{BlockId, CalleeRef, Instr, Module, Root, RootId, Term, ValueId};
use rgoc_types::TypeId;
use rgoc_types::TypeTable;

use crate::interp::{
    Options, Outcome, append_value, apply_unary, as_complex, bounds, cap_of, cmp_values,
    const_value, convert, copy_into, expand_spread, len_of, slice_elem, slice_value, to_f64,
};
use crate::ops::{self, RtError};
use crate::print;
use crate::value::{Cell, NilKind, Value, cell, zero_of};
/// 是不是接口类型（剥具名 / 别名）。
fn is_iface_ty(tt: &TypeTable, t: TypeId) -> bool {
    use rgoc_types::typ::Type;
    let mut cur = t;
    for _ in 0..64 {
        match tt.type_of(cur) {
            Type::Named(id) => cur = tt.named_underlying(*id),
            Type::Alias(id) => cur = tt.alias_rhs(*id),
            Type::Interface(_) => return true,
            _ => return false,
        }
    }
    false
}

/// 某个 SSA 值的静态类型（断言失败的文案要它）。
fn value_type_of(_tt: &TypeTable, x: ValueId, m: &Module, fi: usize) -> TypeId {
    m.funcs[fi]
        .value_types
        .get(x.index())
        .copied()
        .unwrap_or_else(|| TypeId::from_index(0))
}

/// 一个 SSA 帧。
///
/// 字段顺序按「定位 → 状态 → 交接」排：三个下标（函数 / 块 / 指令）、值表、
/// 根存储，最后是「返回时把结果写到调用方的哪些值」。
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
    /// 本帧的根存储（下标即 `RootId`）。
    ///
    /// `Root::Global` 的项**不在这里**用：它指向模块级的 `globals`（共享），
    /// 这里放的是同一块存储的克隆（`Cell` 是 `Rc`，克隆即共享）。
    roots: Vec<Cell>,
    /// 本函数返回时，把结果写回调用方的哪些值（空 ⇒ 没人接结果）。
    ret_to: Vec<ValueId>,
}

/// 执行状态。
struct Exec<'a> {
    m: &'a Module,
    tt: &'a TypeTable,
    /// 包级变量的存储（跨帧共享；`init` 写的，`main` 读得到）。
    globals: Vec<Cell>,
    frames: Vec<SFrame>,
    /// 合并输出流（与 M5 的解释器同一个语义：`print`/`println` 都进这里）。
    output: String,
    steps: u64,
    max_steps: u64,
    max_frames: usize,
}

/// 跑一个 SSA 模块（`Module::init` → `main`）。
#[must_use]
pub fn run_ssa(m: &Module, tt: &TypeTable, opts: Options) -> Outcome {
    let globals: Vec<Cell> = (0..m.globals.len())
        .map(|i| {
            cell(
                m.global_types
                    .get(i)
                    .map_or(Value::Nil(NilKind::Ptr), |t| zero_of(tt, *t)),
            )
        })
        .collect();
    let mut ex = Exec {
        m,
        tt,
        globals,
        frames: Vec::new(),
        output: String::new(),
        steps: 0,
        max_steps: opts.max_steps,
        max_frames: opts.max_frames,
    };

    // ① 包初始化（`Task 32b` 合成的那个函数）
    if let Some(i) = m.init
        && let Err(e) = ex.run_func(i)
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
    let r = ex.run_func(i);
    ex.finish(r)
}

/// 停机原因（运行期的「非正常结束」）。
enum Halt {
    /// 模块没有 `main`（或被拒了，所以 `funcs` 里找不到它）。
    NoMain,
    /// 步数超限。
    Budget,
    /// 帧深超限（与 M5 的 `max_frames` 同口径）。
    StackOverflow,
    /// 跑到 `Unreachable`（构建 bug：可达的块不该以它终止）。
    Unreachable,
    /// 运行期错误（除零 / 越界 / nil 解引用 …）—— 文本是 Go 的 panic 文本。
    Runtime(String),
    /// `panic(v)`（`Term::Panic`）：值交给 `print::panic_text` 渲染。
    Panic(Value),
}

impl Exec<'_> {
    /// 无参跑一个函数（`init` / `main`）。
    fn run_func(&mut self, func: usize) -> Result<(), Halt> {
        self.push_frame(func, &[], Vec::new())?;
        self.drive()
    }

    /// 建一帧并压栈（**不驱动**：`drive` 的下一轮就会执行它）。
    ///
    /// # 为什么不能「压栈 + 驱动到底」
    ///
    /// 那样 `Call` 就变成 Rust 栈上的递归调用 —— 与 `D-M5-1` 的「显式帧栈」
    /// 直接冲突，而且 `max_frames` 根本挡不住宿主栈溢出。所以 `Call` 只压帧，
    /// 控制权回到主循环；被调方 `Return` 时把值写进调用方的 `ret_to`。
    fn push_frame(
        &mut self,
        func: usize,
        args: &[Value],
        ret_to: Vec<ValueId>,
    ) -> Result<(), Halt> {
        if self.frames.len() >= self.max_frames {
            return Err(Halt::StackOverflow);
        }
        let f = &self.m.funcs[func];
        let mut values: Vec<Option<Value>> = vec![None; f.value_types.len()];
        // 入口块的块参数就是形参（`C10` 规则 2）
        for (p, v) in f.params.iter().zip(args.iter()) {
            values[p.index()] = Some(v.clone());
        }
        let roots = frame_roots(self.m, self.tt, func, &self.globals);
        let entry = f.entry;
        self.frames.push(SFrame {
            func,
            block: entry,
            idx: 0,
            values,
            roots,
            ret_to,
        });
        Ok(())
    }

    /// 主循环：一直执行到帧栈空。
    fn drive(&mut self) -> Result<(), Halt> {
        loop {
            let Some(frame) = self.frames.last() else {
                return Ok(());
            };
            if self.steps >= self.max_steps {
                return Err(Halt::Budget);
            }
            self.steps += 1;
            let (fi, bi, ii) = (frame.func, frame.block, frame.idx);
            let f = &self.m.funcs[fi];
            let block = &f.blocks[bi.index()];
            if ii < block.instrs.len() {
                if let Some(text) = self.step_instr()? {
                    self.output.push_str(&text);
                }
                continue;
            }
            match &block.term {
                Term::Return(vals) => {
                    let vals = vals.clone();
                    let mut ret: Vec<Value> = Vec::with_capacity(vals.len());
                    for v in &vals {
                        ret.push(self.read(*v)?);
                    }
                    let ret_to = self
                        .frames
                        .last()
                        .map_or_else(Vec::new, |f| f.ret_to.clone());
                    debug_assert!(
                        ret_to.len() == ret.len(),
                        "结果个数不符：{} 个位置 vs {} 个值（V5 有漏）",
                        ret_to.len(),
                        ret.len()
                    );
                    self.frames.pop();
                    if let Some(caller) = self.frames.last_mut() {
                        for (d, v) in ret_to.iter().zip(ret) {
                            caller.values[d.index()] = Some(v);
                        }
                    }
                }
                Term::Goto { to, args } => {
                    let (to, args) = (*to, args.clone());
                    self.jump(to, &args)?;
                }
                Term::If { cond, then_, else_ } => {
                    let (cond, then_, else_) = (*cond, then_.clone(), else_.clone());
                    let c = self.read(cond)?.as_bool().unwrap_or(false);
                    let (to, args) = if c { then_ } else { else_ };
                    self.jump(to, &args)?;
                }
                Term::Panic(v) => {
                    let v = *v;
                    let val = self.read(v)?;
                    return Err(Halt::Panic(val));
                }
                Term::Unreachable => return Err(Halt::Unreachable),
            }
        }
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
        let mut vals: Vec<Value> = Vec::with_capacity(args.len());
        for a in args {
            vals.push(self.read(*a)?);
        }
        let frame = self.frames.last_mut().expect("帧栈非空");
        frame.block = to;
        frame.idx = 0;
        let params = m.funcs[frame.func].blocks[to.index()].params.clone();
        for (p, v) in params.iter().zip(vals) {
            frame.values[p.index()] = Some(v);
        }
        Ok(())
    }

    /// 读当前帧的一个值。
    fn read(&self, v: ValueId) -> Result<Value, Halt> {
        let frame = self.frames.last().expect("帧栈非空");
        get(&frame.values, v)
    }

    /// 写当前帧的一个值。
    fn write(&mut self, v: ValueId, val: Value) {
        let frame = self.frames.last_mut().expect("帧栈非空");
        put(&mut frame.values, v, val);
    }

    /// 某个根的存储（`Root::Global` 走模块级表）。
    fn root_cell(&self, root: RootId) -> Result<Cell, Halt> {
        self.frames
            .last()
            .and_then(|f| f.roots.get(root.index()))
            .cloned()
            .ok_or_else(|| {
                Halt::Runtime(format!("根 r{} 不在本帧的根表里（构建 bug）", root.index()))
            })
    }

    /// 把「地址值」变成 `Cell`。
    fn as_ptr(&self, v: &Value) -> Result<Cell, Halt> {
        match v {
            Value::Ptr(c) => Ok(c.clone()),
            Value::Nil(_) => Err(Halt::Runtime(RtError::NilDeref.message())),
            other => Err(Halt::Runtime(format!(
                "地址不是指针：{}",
                print::format_operand(other, self.tt, TypeId::from_index(0))
            ))),
        }
    }

    /// 执行当前块的当前指令，返回「要追加到输出流」的文本（`print` 家族用）。
    fn step_instr(&mut self) -> Result<Option<String>, Halt> {
        let m = self.m;
        let tt = self.tt;
        let (fi, bi, ii) = {
            let fr = self.frames.last().expect("帧栈非空");
            (fr.func, fr.block, fr.idx)
        };
        self.frames.last_mut().expect("帧栈非空").idx += 1;
        let ins = &m.funcs[fi].blocks[bi.index()].instrs[ii];

        match ins {
            Instr::Const { dst, c, typ } => {
                let v = const_ssa(c, *typ, tt)?;
                self.write(*dst, v);
                Ok(None)
            }
            Instr::Copy { dst, src } => {
                // `Copy` 是**重命名**（块参数交接 / 结果直连的落点），不是赋值：
                // 深拷贝会把「同一个值流到汇合点」变成「两份副本」。
                // 真正的值语义赋值在构建期由 `Store`/`coerce_to` 表达。
                let v = self.read(*src)?;
                self.write(*dst, v);
                Ok(None)
            }
            Instr::Bin {
                dst,
                op,
                lhs,
                rhs,
                typ,
            } => {
                let x = self.read(*lhs)?;
                let y = self.read(*rhs)?;
                let v = eval_bin(*op, &x, &y, *typ, tt)?;
                self.write(*dst, v);
                Ok(None)
            }
            Instr::Un { dst, op, x, typ } => {
                let v = self.read(*x)?;
                self.write(*dst, apply_unary(*op, v, *typ, tt));
                Ok(None)
            }
            // 类型断言（`M9/Task 33`）：与 M5 的 `type_assert` 同一套判据
            // （`implements` / `identical`），所以 `--via=ssa` 与 `--via=hir`
            // 在断言上不会分叉。
            Instr::TypeAssert { dst, ok, x, typ } => {
                let v = self.read(*x)?;
                let (dt, data) = match v {
                    Value::Iface { typ, data } => (typ, data),
                    // 静态类型不是接口（检查器该已拒）—— 按「断言恒成功」处理，
                    // 与 M5 的 `type_assert` 兜底一致
                    other => (None, Box::new(other)),
                };
                let hit = match dt {
                    None => false,
                    Some(dt) => {
                        if is_iface_ty(tt, *typ) {
                            rgoc_types::implements(tt, dt, *typ, false).unwrap_or(false)
                        } else {
                            rgoc_types::identical(tt, dt, *typ).unwrap_or(false)
                        }
                    }
                };
                if hit {
                    let out = if is_iface_ty(tt, *typ) {
                        Value::Iface {
                            typ: dt,
                            data: data.clone(),
                        }
                    } else {
                        (*data).clone()
                    };
                    self.write(*dst, out);
                    if let Some(o) = ok {
                        self.write(*o, Value::Bool(true));
                    }
                } else if let Some(o) = ok {
                    self.write(*dst, zero_of(tt, *typ));
                    self.write(*o, Value::Bool(false));
                } else {
                    let got = match dt {
                        Some(t) => crate::interp::type_name(tt, t),
                        None => "nil".to_string(),
                    };
                    return Err(Halt::Runtime(format!(
                        "panic: interface conversion: {} is {}, not {}",
                        crate::interp::type_name(tt, value_type_of(tt, *x, m, fi)),
                        got,
                        crate::interp::type_name(tt, *typ)
                    )));
                }
                Ok(None)
            }
            Instr::Convert { dst, x, typ } => {
                let v = self.read(*x)?;
                let from = m.funcs[fi].value_types[x.index()];
                self.write(*dst, convert(tt, v, *typ, from));
                Ok(None)
            }
            Instr::AddrOf { dst, root } => {
                let c = self.root_cell(*root)?;
                self.write(*dst, Value::Ptr(c));
                Ok(None)
            }
            Instr::Field { dst, base, index } => {
                let p = self.read(*base)?;
                let c = self.as_ptr(&p)?;
                let f = field_cell(&c, *index)?;
                self.write(*dst, Value::Ptr(f));
                Ok(None)
            }
            Instr::Index { dst, base, i } => {
                let b = self.read(*base)?;
                let idx = self.read(*i)?;
                let c = index_cell(&b, idx.as_int().unwrap_or(0), tt)?;
                self.write(*dst, Value::Ptr(c));
                Ok(None)
            }
            Instr::Composite { dst, typ, items } => {
                let mut vs = Vec::with_capacity(items.len());
                for it in items {
                    vs.push(self.read(*it)?);
                }
                let v = eval_composite(tt, *typ, vs)?;
                self.write(*dst, v);
                Ok(None)
            }
            Instr::Make { dst, typ, len, cap } => {
                let l = match len {
                    Some(x) => Some(self.read(*x)?),
                    None => None,
                };
                let c = match cap {
                    Some(x) => Some(self.read(*x)?),
                    None => None,
                };
                let v = eval_make(tt, *typ, l, c)?;
                self.write(*dst, v);
                Ok(None)
            }
            Instr::Slice {
                dst,
                base,
                lo,
                hi,
                max,
            } => {
                let b = self.read(*base)?;
                let take = |x: &Option<ValueId>| -> Result<Option<Value>, Halt> {
                    match x {
                        Some(x) => Ok(Some(self.read(*x)?)),
                        None => Ok(None),
                    }
                };
                let l = take(lo)?;
                let h = take(hi)?;
                let mx = take(max)?;
                let v = slice_value(&b, l, h, mx, TypeId::from_index(0)).map_err(rt)?;
                self.write(*dst, v);
                Ok(None)
            }
            Instr::Load { dst, addr, .. } => {
                let p = self.read(*addr)?;
                let c = self.as_ptr(&p)?;
                // 读出来的是**拷贝**：Go 的赋值/取值都是值语义，
                // 共享只发生在指针/切片/map 这些引用型值上（它们 clone 后仍共享）。
                let v = c.borrow().deep_copy();
                self.write(*dst, v);
                Ok(None)
            }
            Instr::Store {
                dst,
                mem,
                addr,
                val,
                ..
            } => {
                let p = self.read(*addr)?;
                let v = self.read(*val)?;
                let c = self.as_ptr(&p)?;
                *c.borrow_mut() = v.deep_copy();
                // `Store` 产出的「新 mem」只是**静态**定序用的令牌，运行期原样传递
                let tok = self.read(*mem)?;
                self.write(*dst, tok);
                Ok(None)
            }
            Instr::Call {
                dsts,
                callee,
                args,
                dots,
                ..
            } => {
                let dsts = dsts.clone();
                let callee = callee.clone();
                let args = args.clone();
                let dots = *dots;
                self.exec_call(fi, dsts, callee, args, dots)
            }
        }
    }

    /// 一条 `Call`。
    ///
    /// 内建当场算完写进 `dsts`；用户函数只**压帧**（`ret_to = dsts`），
    /// 控制权回主循环 —— 这样深递归不吃宿主栈（`D-M5-1`）。
    fn exec_call(
        &mut self,
        fi: usize,
        dsts: Vec<ValueId>,
        callee: CalleeRef,
        args: Vec<ValueId>,
        dots: bool,
    ) -> Result<Option<String>, Halt> {
        let mut avals: Vec<Value> = Vec::with_capacity(args.len());
        let mut atypes: Vec<TypeId> = Vec::with_capacity(args.len());
        for a in &args {
            avals.push(self.read(*a)?);
            atypes.push(self.m.funcs[fi].value_types[a.index()]);
        }
        match callee {
            // 运行期助手（M9/Phase 2 的 map）。
            //
            // # 为什么解释器也要实现
            //
            // `rgoc build` 走同一条 SSA 建图：map 现在**能建成图**了，所以
            // `--via=ssa` 也一定会看到这些 `Call`。不实现的话，SSA 那侧要么
            // 拒绝（那就是假的不支持）、要么 panic（更糟）。
            //
            // # 语义对齐
            //
            // `MapObj` 是 M5 就在用的那个（`Vec<(Value, Value)>` + `go_eq`），
            // 所以键相等、`struct{}` 全等这些细节与 HIR 通路**同一份实现**。
            CalleeRef::Runtime(f) => {
                use rgoc_ssa::ir::RuntimeFn as R;
                match f {
                    R::MapMake => {
                        let d = dsts[0];
                        let m = Value::Map(std::rc::Rc::new(std::cell::RefCell::new(
                            crate::value::MapObj {
                                entries: Vec::new(),
                            },
                        )));
                        self.write(d, m);
                    }
                    R::MapGet => {
                        let vd = dsts[0];
                        let vt = self.m.funcs[fi].value_types[vd.index()];
                        let hit = match &avals[0] {
                            Value::Map(mp) => mp.borrow().get(&avals[1]).cloned(),
                            _ => None,
                        };
                        match &hit {
                            Some(v) => self.write(vd, v.clone()),
                            None => {
                                let z = zero_of(self.tt, vt);
                                self.write(vd, z);
                            }
                        }
                        if let Some(ok) = dsts.get(1).copied() {
                            self.write(ok, Value::Bool(hit.is_some()));
                        }
                    }
                    R::MapSet => match &avals[0] {
                        Value::Map(mp) => {
                            mp.borrow_mut().set(avals[1].clone(), avals[2].clone());
                        }
                        // 规范：写 nil map 是 panic（文本与 native 一致）
                        _ => {
                            return Err(Halt::Panic(Value::Str(std::rc::Rc::from(
                                "assignment to entry in nil map",
                            ))));
                        }
                    },
                    R::MapDel => {
                        if let Value::Map(mp) = &avals[0] {
                            mp.borrow_mut().remove(&avals[1]);
                        }
                    }
                    R::MapIter => {
                        let (kd, vd, nd) = (dsts[0], dsts[1], dsts[2]);
                        let idx = match &avals[1] {
                            Value::Int(i) => usize::try_from(*i).unwrap_or(0),
                            _ => 0,
                        };
                        let pair = match &avals[0] {
                            Value::Map(mp) => {
                                let b = mp.borrow();
                                if idx < b.entries.len() {
                                    Some(b.entries[idx].clone())
                                } else {
                                    None
                                }
                            }
                            _ => None,
                        };
                        match pair {
                            Some((k, v)) => {
                                self.write(kd, k);
                                self.write(vd, v);
                                self.write(
                                    nd,
                                    Value::Int(i128::try_from(idx + 1).unwrap_or(i128::MAX)),
                                );
                            }
                            None => self.write(nd, Value::Int(0)),
                        }
                    }
                }
                Ok(None)
            }
            // `panic(v)` 正常应由构建期落成 `Term::Panic`；这里兜住「漏网的」
            // （防御：真出现时语义仍与 Go 一致，而不是 panic 到宿主）
            CalleeRef::Builtin(BuiltinKind::Panic) => {
                let v = avals
                    .into_iter()
                    .next()
                    .unwrap_or(Value::Nil(NilKind::Iface));
                Err(Halt::Panic(v))
            }
            CalleeRef::Builtin(k @ (BuiltinKind::Print | BuiltinKind::Println)) => {
                let text = render_print(k, &avals, &atypes, dots, self.tt);
                debug_assert!(dsts.is_empty(), "print 家族没有结果");
                Ok(Some(text))
            }
            CalleeRef::Builtin(k) => {
                let out = self.eval_builtin(k, &avals, &atypes, dots)?;
                for (d, v) in dsts.iter().zip(out) {
                    self.write(*d, v);
                }
                Ok(None)
            }
            CalleeRef::Func(fid) => {
                let idx = index_of(self.m, fid)
                    .ok_or_else(|| Halt::Runtime(format!("调用不存在的函数 func#{}", fid.0)))?;
                self.push_frame(idx, &avals, dsts)?;
                Ok(None)
            }
            CalleeRef::Method { recv, func } => {
                // HIR 的 `Func.params[0]` 就是接收者（`build.rs` 的 `sig_of` 减一
                // 只是为了 `V5` 的实参计数），所以这里要把它补回实参表最前面。
                let r = self.read(recv)?;
                let idx = index_of(self.m, func)
                    .ok_or_else(|| Halt::Runtime(format!("调用不存在的方法 func#{}", func.0)))?;
                let mut all = Vec::with_capacity(avals.len() + 1);
                all.push(r);
                all.extend(avals);
                self.push_frame(idx, &all, dsts)?;
                Ok(None)
            }
            // 接口方法派发（`M9/Task 32`）：槽号 → 方法名 → 沿嵌入链找 FuncId。
            //
            // 三处必须同源（`R-9-10`）：静态 ITab 的填序、`build` 的取槽序、
            // 这里的查序 —— 都走 `rgoc_ssa::ir::iface_methods` + `resolve_method`。
            CalleeRef::Iface { recv, slot } => {
                let r = self.read(recv)?;
                let it = self.m.funcs[fi].value_types[recv.index()];
                let names = rgoc_ssa::ir::iface_methods(self.tt, it);
                let name = names
                    .get(slot as usize)
                    .cloned()
                    .ok_or_else(|| Halt::Runtime(format!("接口方法槽 {slot} 越界（构建 bug）")))?;
                let (dt, data) = match r {
                    Value::Iface { typ, data } => (typ, *data),
                    other => {
                        return Err(Halt::Runtime(format!(
                            "接口方法调用：接收者不是接口值（{}）",
                            print::format_operand(&other, self.tt, it)
                        )));
                    }
                };
                // nil 接口上调用 ⇒ 与 Go 同文本
                let Some(dt) = dt else {
                    return Err(Halt::Runtime(RtError::NilDeref.message()));
                };
                let (fid, hops) = rgoc_ssa::ir::resolve_method(self.tt, &self.m.methods, dt, &name)
                    .ok_or_else(|| {
                        Halt::Runtime(format!("类型 t{} 没有方法 {name}（构建 bug）", dt.index()))
                    })?;
                // 沿嵌入链把接收者取到位（与 `resolve_method` 的 hops 同源）
                let mut rv = data;
                for h in &hops {
                    rv = crate::interp::field_of(&rv, h.index).ok_or_else(|| {
                        Halt::Runtime(format!("嵌入链下降失败（字段 {}）", h.index))
                    })?;
                }
                let idx = index_of(self.m, fid)
                    .ok_or_else(|| Halt::Runtime(format!("调用不存在的方法 func#{}", fid.0)))?;
                let mut all = Vec::with_capacity(avals.len() + 1);
                all.push(rv);
                all.extend(avals);
                self.push_frame(idx, &all, dsts)?;
                Ok(None)
            }
            CalleeRef::Indirect(_) => Err(Halt::Runtime(
                "间接调用：函数值不在 M6 的求值面内（`SSA-UNSUPPORTED-FUNC-VALUE`）".to_string(),
            )),
        }
    }

    /// 内建（**与 M5 的 `do_builtin` 同一套语义**，共用 `interp` 的纯函数）。
    fn eval_builtin(
        &self,
        k: BuiltinKind,
        args: &[Value],
        atypes: &[TypeId],
        dots: bool,
    ) -> Result<Vec<Value>, Halt> {
        let tt = self.tt;
        let t0 = atypes.first().copied().unwrap_or(TypeId::from_index(0));
        let arg = |i: usize, dflt: Value| args.get(i).cloned().unwrap_or(dflt);
        Ok(match k {
            BuiltinKind::Len => vec![Value::Int(len_of(&arg(0, Value::Int(0))) as i128)],
            BuiltinKind::Cap => vec![Value::Int(cap_of(&arg(0, Value::Int(0))) as i128)],
            BuiltinKind::Append => {
                let base = arg(0, Value::Nil(NilKind::Slice));
                // `...` 是语义：展开成多个元素（`R-35-1`）
                let extra: Vec<Value> = if dots {
                    expand_spread(args.get(1).cloned())
                } else {
                    args.iter().skip(1).cloned().collect()
                };
                vec![append_value(tt, base, extra, t0).map_err(rt)?]
            }
            BuiltinKind::Copy => {
                let dst = arg(0, Value::Nil(NilKind::Slice));
                let src = arg(1, Value::Nil(NilKind::Slice));
                vec![Value::Int(copy_into(dst, &src).map_err(rt)? as i128)]
            }
            BuiltinKind::Min | BuiltinKind::Max => {
                let want_max = k == BuiltinKind::Max;
                let mut it = args.iter().cloned();
                let mut best = it.next().unwrap_or(Value::Int(0));
                for v in it {
                    let ord = cmp_values(&best, &v);
                    if (want_max && ord == std::cmp::Ordering::Less)
                        || (!want_max && ord == std::cmp::Ordering::Greater)
                    {
                        best = v;
                    }
                }
                vec![best]
            }
            BuiltinKind::Complex => vec![Value::Complex(
                to_f64(&arg(0, Value::Int(0))),
                to_f64(&arg(1, Value::Int(0))),
            )],
            BuiltinKind::Real => {
                let (re, _) = as_complex(&arg(0, Value::Int(0)));
                vec![Value::Float(re)]
            }
            BuiltinKind::Imag => {
                let (_, im) = as_complex(&arg(0, Value::Int(0)));
                vec![Value::Float(im)]
            }
            // `new(T)` 在 IR 里走 `Make`（`len/cap` 都空且类型是指针）；
            // 这条分支只在「内建直接出现」时兜底（`BuiltinKind::New` 的语义是
            // 「分配并返回地址」，参数是被分配的值）。
            BuiltinKind::New => vec![Value::Ptr(cell(arg(0, Value::Int(0))))],
            BuiltinKind::Clear => {
                clear_value(&arg(0, Value::Nil(NilKind::Slice)));
                Vec::new()
            }
            // 下面这些在构建期就被拒了（白名单 9 条），走到这里说明预扫描有漏：
            // 响亮地报，不静默当无事发生。
            BuiltinKind::Delete => {
                return Err(Halt::Runtime(
                    "`delete` 需要 map（`SSA-UNSUPPORTED-MAP`）".to_string(),
                ));
            }
            BuiltinKind::Close => {
                return Err(Halt::Runtime(
                    "`close` 需要 chan（`SSA-UNSUPPORTED-CHAN`）".to_string(),
                ));
            }
            BuiltinKind::Recover => {
                return Err(Halt::Runtime(
                    "`recover` 需要 defer（`SSA-UNSUPPORTED-DEFER`）".to_string(),
                ));
            }
            BuiltinKind::Panic => {
                return Err(Halt::Runtime("`panic` 应落成 Term::Panic".to_string()));
            }
            BuiltinKind::Print | BuiltinKind::Println => {
                return Err(Halt::Runtime(
                    "print 家族由 `render_print` 处理（不该走到这里）".to_string(),
                ));
            }
        })
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
            Err(Halt::StackOverflow) => Outcome {
                output,
                exit: 2,
                diag: Some(format!("栈溢出（帧深上限 {}）", self.max_frames)),
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

/// 一帧的根存储。
fn frame_roots(m: &Module, tt: &TypeTable, func: usize, globals: &[Cell]) -> Vec<Cell> {
    let f = &m.funcs[func];
    f.roots
        .iter()
        .map(|r| match r {
            Root::Slot(s) => cell(
                f.slot_types
                    .get(s)
                    .map_or(Value::Nil(NilKind::Ptr), |t| zero_of(tt, *t)),
            ),
            // 全局的存储是**模块级**的：这里放的是同一块 `Cell` 的克隆
            Root::Global(g) => globals
                .get(g.index())
                .cloned()
                .unwrap_or_else(|| cell(Value::Nil(NilKind::Ptr))),
            // 堆根的「存储」在运行期没有意义（真对象由 `Make`/`Composite` 造，
            // 地址走 `Value::Ptr`）；留一块占位，只为 `AddrOf` 不 panic。
            Root::Heap => cell(Value::Nil(NilKind::Ptr)),
        })
        .collect()
}

/// 结构体字段的地址：`Ptr(结构体) → 第 index 个字段的 Cell`。
fn field_cell(c: &Cell, index: usize) -> Result<Cell, Halt> {
    let v = c.borrow();
    match &*v {
        Value::Struct(cells) => cells.get(index).cloned().ok_or_else(|| {
            Halt::Runtime(format!(
                "字段下标 {index} 越界（结构体只有 {} 个字段）",
                cells.len()
            ))
        }),
        other => Err(Halt::Runtime(format!("取字段的底座不是结构体：{other:?}"))),
    }
}

/// 元素地址：数组 / 切片 / 指向数组的指针 / 字符串。
///
/// # 字符串为什么也在这里
///
/// `s[i]` 在 Go 里取的是**字节值**，不是地址（所以 `&s[i]` 非法）。而 IR 的
/// `Index` 语义是「算元素地址」，`Load` 再去读它。字符串没有可写的存储，
/// 所以这里造一个**临时 `Cell`** 装着那个字节 —— `Load` 读它一次就够，
/// 也没人能写它（构建期不可能为 `s[i]` 生成 `Store`）。
fn index_cell(base: &Value, i: i128, tt: &TypeTable) -> Result<Cell, Halt> {
    match base {
        // 指向数组的指针（`p[i]`，`p *[N]T`）：底座是那个指针
        Value::Ptr(c) => {
            let inner = c.borrow().clone();
            match &inner {
                // 指针指向指针：Go 不允许 `**T` 取下标，这里直接报
                Value::Ptr(_) => Err(Halt::Runtime(
                    "取下标：底座是「指向指针」的指针（Go 不允许）".to_string(),
                )),
                _ => index_cell(&inner, i, tt),
            }
        }
        Value::Array(cells) => {
            let k = bounds(i, cells.len()).map_err(rt)?;
            Ok(cells[k].clone())
        }
        Value::Slice(sv) => {
            let k = bounds(i, sv.len).map_err(rt)?;
            Ok(sv.arr[sv.off + k].clone())
        }
        Value::Str(s) => {
            let bytes = s.as_bytes();
            let k = bounds(i, bytes.len()).map_err(rt)?;
            Ok(cell(Value::Int(i128::from(bytes[k]))))
        }
        Value::Nil(_) => Err(Halt::Runtime(RtError::NilDeref.message())),
        other => Err(Halt::Runtime(format!(
            "取下标的底座不可索引：{}",
            print::format_operand(other, tt, TypeId::from_index(0))
        ))),
    }
}

/// `clear(x)`：切片清零（map 在构建期就被拒了）。
fn clear_value(v: &Value) {
    match v {
        Value::Map(obj) => obj.borrow_mut().entries.clear(),
        Value::Slice(sv) => {
            for i in 0..sv.len {
                *sv.arr[sv.off + i].borrow_mut() = Value::Int(0);
            }
        }
        _ => {}
    }
}

/// `print` / `println` 的文本（`...` 时展开）。
fn render_print(
    k: BuiltinKind,
    vals: &[Value],
    types: &[TypeId],
    dots: bool,
    tt: &TypeTable,
) -> String {
    let mut out = String::new();
    // `println(xs...)` 要把切片摊成多个操作数；类型按元素类型给
    let mut items: Vec<(Value, TypeId)> = Vec::new();
    if dots && vals.len() == 1 {
        let elem = slice_elem(tt, types.first().copied().unwrap_or(TypeId::from_index(0)));
        for v in expand_spread(vals.first().cloned()) {
            items.push((v, elem));
        }
    } else {
        for (v, t) in vals.iter().zip(types.iter()) {
            items.push((v.clone(), *t));
        }
    }
    for (i, (v, t)) in items.iter().enumerate() {
        if k == BuiltinKind::Println && i > 0 {
            out.push(' ');
        }
        out.push_str(&print::format_operand(v, tt, *t));
    }
    if k == BuiltinKind::Println {
        out.push('\n');
    }
    out
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
        Type::Struct(_) => Ok(Value::Struct(std::rc::Rc::new(cells))),
        Type::Array { .. } => Ok(Value::Array(std::rc::Rc::new(cells))),
        Type::Slice(_) => {
            let n = cells.len();
            Ok(Value::Slice(crate::value::SliceVal {
                arr: std::rc::Rc::new(cells),
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
            Ok(Value::Slice(crate::value::SliceVal {
                arr: std::rc::Rc::new(cells),
                off: 0,
                len: l,
                cap: c,
            }))
        }
        // `new(T)`：分配一个零值并返回它的地址
        Type::Pointer(inner) => Ok(Value::Ptr(cell(zero_of(tt, *inner)))),
        // `make(map[K]V)`（M9/Task 19）：建图的是 `Instr::Make`（不是 runtime 调用），
        // 所以解释器这条也要认。空 map —— 容量提示不影响语义。
        Type::Map { .. } => Ok(Value::Map(std::rc::Rc::new(std::cell::RefCell::new(
            crate::value::MapObj {
                entries: Vec::new(),
            },
        )))),
        other => Err(Halt::Runtime(format!(
            "`make` 的类型既不是 slice 也不是指针：{other:?}"
        ))),
    }
}

/// `FuncId` → `funcs` 下标（被拒的函数不在 `funcs` 里，所以可能找不到）。
fn index_of(m: &Module, fid: FuncId) -> Option<usize> {
    m.funcs.iter().position(|f| f.src == fid)
}
