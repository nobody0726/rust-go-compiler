//! 表达式检查（M4/Task 21–24）—— 一元 / 二元 / 比较 / 移位 / 常量面 +
//! 转换 + 索引 / 切片 + 选择器。
//!
//! 对照官方 `go/types/expr.go`（`unary` / `binary` / `shift` / `comparison` /
//! `matchTypes`）、`const.go`（`overflow` / `representable`）、
//! `conversions.go`（`conversion` / `convertibleTo`）与 `index.go`
//! （`indexExpr` / `sliceExpr` / `index` / `isValidIndex`）。本文件是
//! **Task 21–23 的窄面**：包级 `var` 声明的初始化式（`Decl::Var.values`）。
//! 常量声明的求值在装载器（M3），函数体是 Task 25，赋值面是 Task 24+。
//!
//! # 转换面（Task 22；`T-CK-33`）
//!
//! 函数位是**类型表达式** ⇒ 转换（官方 `exprOrType(x, e.Fun, true)` 的
//! `typexpr` 分支）：常量走 `constConvertibleTo`（可表示性 + 整数→字符串
//! 码点替换），非恒量走 `convertibleTo`（M4/Task 9 的静态类型对）。
//! 只接**单实参**且函数位是名字（或括号包名字）的形态 —— 类型族
//! （`[]byte(x)` / `*T(x)` / `map[K]V(x)`）与多实参归调用面 Task 23+/25，
//! 此处 `invalid` 且不报（接受夹具钉住「不假报」）。
//!
//! 常量转换的两种拒绝（oracle 实测 9 行）：
//! - **整数 → 整数**且不可表示 ⇒ `constant V overflows T`（类别 `overflow`，
//!   码 `TYP-NUMERIC-OVERFLOW`）；
//! - 其余不可转换 ⇒ `cannot convert V to type T`（类别 `convert`，码
//!   `TYP-INVALID-CONVERSION`）；整数常量 → 字符串**恒可**（无效码点 ⇒
//!   U+FFFD，`string(-1)` / `string(1<<100)` 都合法）。
//!
//! # 索引 / 切片面（Task 23；`T-CK-34`）
//!
//! `x[i]` 接数组 / 切片 / 字符串 / map / 指向数组的指针；字符串索引**恒**产出
//! `byte` 值（即时字符串与索引都是常量）。下标经 `convertUntyped(int)`
//! （`a[1.5]` / `a[1e100]` ⇒ `truncated to int`、`sl["x"]` ⇒ `cannot convert
//! ... to type int` —— 类别 `convert`）→ 整数性 / 非负 / 可表示（类型化常量超
//! `int` 范围 ⇒ `overflows int`，类别 **`overflow`**）→ 常量越界
//! （类别 `index`）。`x[a:b:c]`：三下标（字符串禁用）+ 常量次序 + 边界
//! （`max = len + 1`；类别 `slice`）。新码 `CHK-INVALID-INDEX` /
//! `CHK-INVALID-SLICE`（oracle `task23-reject.go` 19 行实证）。map 键面
//! （`cannot use ... in map index`，类别 `assign`）归 Task 25 的赋值面。
//!
//! # 选择器面（Task 24；`T-CK-34`）
//!
//! `x.f` / `x.M` / `T.M`：查找在 `lookup.rs`（官方 `lookup.go` 的逐深度 BFS、
//! `seen` 遮蔽、同级碰撞、具名指针类型特例），方法值 / 方法表达式签名在
//! `selection.rs`。诊断四式合一码 `CHK-INVALID-SELECTOR`（类别 `selector`）：
//! `ambiguous selector` / `... undefined (type ...)` / `cannot call pointer
//! method`（不可寻址遇指针接收者）/ `invalid method expression`（typexpr 面）
//! / `operand for field selector`（报在 `X` 位）。`*T` 的 typexpr 臂已接
//! （`(*T).M` 可达）。`pkg.Name` 与泛型接收者面 ⇒ 不报。
//!
//! # 复合字面量面（Task 27；`T-CK-37`）
//!
//! 官方 `literals.go` 的 `compositeLit` / `indexedElts` 逐支对齐：结构体
//! （键值 / 值两形态 + 提升字段 1.27 门控）、数组 / 切片（下标 + 重复 +
//! 位置越界）、map（键赋值 + 重复键）、其余类型（`invalid composite literal
//! type`）。嵌套字面量的类型提示走 [`ExprChecker::expr_hint`]（官方
//! `genericExpr(x, e, hint)`）；`[...]T` 在元素检查后回填长度。
//! 新码 4 个（`CHK-INVALID-LIT` / `CHK-DUPLICATE-LIT-FIELD` /
//! `CHK-DUPLICATE-LIT-INDEX` / `CHK-INVALID-LIT-FIELD`；`T-CK-37` 22 行实证）。
//!
//! # 类型断言面（Task 28；`T-CK-38`）
//!
//! `v.(T)`：操作数必须是接口（否则 `… is not an interface`，`op-invalid`）；
//! 目标是接口 ⇒ 不做静态判定；否则要求**目标实现源接口**（`implements(T, I)`），
//! 失败报 `impossible type assertion`（`comma-ok`，码 `CHK-IMPOSSIBLE-ASSERT`）。
//! 结果模式 `commaok`（官方同形）。
//!
//! # 位置规则（oracle 探针 1–7 与官方源双证；`task21-reject.go` 53 行）
//!
//! | 情形 | 错误位 |
//! |---|---|
//! | 一元谓词拒绝（`-b` / `!a` / `^1.5`）、`*` / `&` / `<-` 失败 | **操作数**位 |
//! | 一元折叠溢出（`-cm`） | **操作符**位（= 一元表达式的 `Pos()`） |
//! | 二元 mismatch / 谓词拒绝 / 无类型操作数转换失败 | **左操作数起点** |
//! | 比较 mismatch | **右操作数**位（官方 `errOp = y`） |
//! | 比较「运算符未定义」（有序面 / `nil` 面） | 点名侧操作数位（多为一元 左=`x`） |
//! | 不可比（map / slice / func） | 点名侧操作数位 |
//! | 除零 | **除数**位 |
//! | 移位计数类 | **计数**位；左操作数类 → **左操作数起点** |
//! | 常量增长（`1 << 512`）/ 有类型常量溢出（`ci8 << 7` / `cm - 1`） | 前者**操作符**位；后者**左操作数起点** |
//!
//! 「起点」对 `Paren` 取 `(` 位（`Expr::pos()` 的官方 `Pos()` 语义）。
//!
//! # 码映射（`C8` 类别面；oracle `--check` 对 53 行逐字节对账）
//!
//! 唯一新增码 `CHK-INVALID-OPERATION`（`op-invalid`）；其余复用 M3 码：
//! `TYP-DIV-BY-ZERO`（op-invalid）、`TYP-INVALID-SHIFT-COUNT` /
//! `TYP-INVALID-SHIFT-OPERAND`（shift）、`TYP-NUMERIC-OVERFLOW`（overflow）、
//! `TYP-INVALID-CONVERSION`（convert）。**截断**在检查面（转换语境）照
//! `TypTruncatedFloat` 文档的既有分工走 `TYP-INVALID-CONVERSION`。
//!
//! # 已知窄面（台账登记；负向缺口由 `U-M4` 面承担）
//!
//! - 非恒移位的 `signed shift count` 版本门（官方 `verifyVersionf(y, go1_13)`）
//!   暂缓（夹具 ≥go1.21；Task 26/29/37 再收）；
//! - 复合除法下溢（`go.dev/issue/20227`）暂缓；
//! - `TypeExpr` 操作数（`*T` / `&T`）返回 invalid 且**不报**；
//! - `assignable` 的 `Err`（类型参数窄面）在比较面按**宽容**处理（不据此报
//!   mismatch；假阳性优先级高于负向缺口）；
//! - 折叠产出 `Unknown` 时不报「not representable」（官方该分支在谓词过滤后
//!   实际不可达；报出来反而会造出 oracle 没有的「other 类」行）。

use rgoc_ast::{
    AssertExpr, BasicLit, CallExpr, CompositeLit, Decl, Expr, IndexExpr, LitKind, Name, Op,
    Operation, SelectorExpr, SliceExpr,
};
use rgoc_const::{
    BasicId, BigInt, Const, Sizes, const_from_literal, default_type_of, fold_binary, fold_compare,
    fold_unary_typed, representable,
};
use rgoc_lex::{Diag, DiagCode, FileId, Pos};
use rgoc_types::{
    ChanDir, ConstId, ObjKind, Type, TypeId, TypeTable, assignable, comparable, convertible,
    identical, implements, unalias, underlying,
};

use crate::check::Session;
use crate::lookup::{Found, lookup_field_or_method};
use crate::operand::{Mode, Operand};
use crate::selection::{SelKind, method_expr_sig, method_value_sig};

/// 表达式入口：包级 `Decl::Var` 的初始化式逐个检查（Task 21 的窄面）。
pub(crate) fn check_package_decls(s: &mut Session<'_>) {
    // 先把包引用拷出来：迭代需要它，而 `cx` 独占 `&mut Session`。
    let pkg = s.pkg;
    let mut cx = ExprChecker { s, env: None };
    for (i, ast) in pkg.asts.iter().enumerate() {
        let file = pkg.files[i];
        for decl in &ast.decls {
            let Decl::Var(v) = decl else { continue };
            for value in &v.values {
                let _ = cx.expr(file, value);
            }
        }
    }
}

/// 带位置的求值结果（操作数 + 它自己的源码位置）。
///
/// 官方的 `operand` 结构体自带 `expr ast.Expr`，位置从它取（`x.Pos()`）；本仓的
/// [`Operand`] 只有 `pos: Option<Pos>` 且并非所有产地都填，所以二元 / 移位的内部
/// 路径显式配一个位置 —— 报点全靠它，不能靠猜。
#[derive(Clone)]
pub(crate) struct At {
    pub op: Operand,
    pub pos: Pos,
}

/// 二元运算的**来源形态** —— 官方 `binary(x, e, lhs, rhs, op, opPos)` 里那个
/// 可以为 `nil` 的 `e`（`expr.go:780` 注释原文：「it may be nil for non-constant
/// expressions (when invoked for an assignment operation where the binary
/// expression is implicit)」）。
///
/// 两态的分歧只在**报错消息**（`expr.go:819` vs `:821`）：
/// 真二元表达式用整条表达式文本，复合赋值用 `lhs op= rhs` 形态。
/// 位置在两种形态下都落在**左操作数**（官方 `posn = x` 与 `posn = e` 的区别只
/// 影响列号：`e` 的起点与 `x` 的起点在二元表达式里相同，所以这里统一用 `xpos`）。
pub(crate) enum BinSrc {
    /// `x op y`：真二元表达式（官方 `e != nil`）。
    ///
    /// 官方消息带整条表达式的**源码文本**（`%s` 打 `e`）；本仓没有源码文本
    /// 设施，消息退化成 `mismatched types A and B` —— 消息只进人读面，
    /// `(行,列,类别)` 才是对账面（见 [`operand_text`] 的注释）。
    Expr,
    /// `x op= y`：复合赋值的隐式二元运算（官方 `e == nil`）。
    Assign {
        lhs: String,
        rhs: String,
        /// 基础运算符的源码形态（`+`、`<<` 等；消息里要拼成 `lhs sym= rhs`）。
        sym: &'static str,
    },
}

/// 表达式检查器：持会话（诊断流 + 类型表 + 被检查包）。
///
/// `env` 是**语句面**（Task 25）的局部作用域栈：包级初始化式检查时为 `None`，
/// 函数体里由 [`crate::stmt`] 传入 —— 否则 `y := 1` 之后的 `y` 在
/// [`Package::lookup_in_file`] 里查不到（那里只有文件/包/universe 三层）。
pub(crate) struct ExprChecker<'s, 'p> {
    pub(crate) s: &'s mut Session<'p>,
    pub(crate) env: Option<&'s crate::env::Env>,
}

// ─────────────────────────────── 入口分派 ───────────────────────────────

impl<'s, 'p> ExprChecker<'s, 'p> {
    fn tt(&self) -> &TypeTable {
        self.s.tt
    }

    /// 类型表只读入口（`builtins.rs` 用）。
    pub(crate) fn type_table(&self) -> &TypeTable {
        self.s.tt
    }

    /// 类型表可写入口（`builtins.rs` 挂新类型用）。
    pub(crate) fn tt_mut(&mut self) -> &mut TypeTable {
        self.s.tt
    }

    pub(crate) fn report(&mut self, pos: Pos, code: DiagCode, message: String) {
        self.s.report_hard(Diag { pos, code, message });
    }
    pub(crate) fn type_text(&self, t: TypeId) -> String {
        type_str(self.tt(), t)
    }

    /// 官方 `exprInternal` 的**已接入子集**。未接入的形态（类型断言 /
    /// 值函数位的调用 / 复合字面量 / 类型族）⇒ `invalid` 且**不报** ——
    /// 避免用假诊断抢占 Task 25/30 的落点。转换（类型函数位）在
    /// [`Self::call`] 里接（Task 22），索引 / 切片在 [`Self::index_expr`] /
    /// [`Self::slice_expr`]（Task 23），选择器在 [`Self::selector`]（Task 24）。
    pub(crate) fn expr(&mut self, file: FileId, e: &Expr) -> Operand {
        self.expr_hint(file, e, None)
    }

    /// 带**元素类型提示**的表达式求值（官方 `genericExpr(x, e, hint)`）。
    ///
    /// 只有复合字面量吃 `hint`（嵌套字面量省略类型时，元素类型由外层给）；
    /// 其余形态一律退回 [`Self::expr`]。`hint` 为 `None` 且字面量也没有类型
    /// ⇒ 官方 `UntypedLit`。
    pub(crate) fn expr_hint(&mut self, file: FileId, e: &Expr, hint: Option<TypeId>) -> Operand {
        match e {
            Expr::Name(n) => self.ident(file, n),
            Expr::BasicLit(l) => self.literal(l),
            Expr::Paren(p) => self.expr_hint(file, &p.x, hint),
            Expr::Unary(o) => self.unary(file, o),
            Expr::Binary(o) => self.binary(file, o),
            Expr::Call(c) => self.call(file, c),
            Expr::Index(ix) => self.index_expr(file, ix),
            Expr::Slice(sx) => self.slice_expr(file, sx),
            Expr::Selector(se) => self.selector(file, se),
            Expr::CompositeLit(cl) => self.composite_lit(file, cl, hint),
            Expr::TypeAssert(a) => self.type_assert(file, a),
            // 类型表达式族（M4/Task 37 补）：`struct{…}` / `interface{…}` /
            // `func(…)` 与已有的数组/切片/map/chan/指针同路 —— 它们出现在
            // **值位置**时是类型操作数（官方 `exprOrType` 的 typexpr 分支）。
            Expr::StructType(_)
            | Expr::InterfaceType(_)
            | Expr::FuncType(_)
            | Expr::ArrayType(_)
            | Expr::SliceType(_)
            | Expr::MapType(_)
            | Expr::ChanType(_)
            | Expr::DotsType(_) => match self.type_expr(file, e) {
                Some(t) => Operand::type_expr(t),
                None => Operand::invalid(),
            },
            _ => Operand::invalid(),
        }
    }

    /// 官方 `check.ident` 的**值面**：`lookup_in_file` 命中的对象 → 操作数。
    ///
    /// 未命中说明名字未声明 —— 装载器已报 `TYP-UNDECLARED-NAME`（`T-CK-95`
    /// 钉住），这里**不得**重复上报。
    fn ident(&mut self, file: FileId, n: &Name) -> Operand {
        // ① 局部作用域（Task 25）：`:=` / 参数 / 命名结果 —— 装载表查不到它们
        //    （`Package::lookup_in_file` 只管文件 → 包 → universe 三层）。
        if let Some(env) = self.env
            && let Some(op) = env.lookup(&n.value)
        {
            return op.clone();
        }
        match self.s.pkg.lookup_in_file(file, &n.value) {
            Some(obj) => {
                let mut op = Operand::from_object(obj).unwrap_or_else(Operand::invalid);
                // 装载面对「需类型信息」的 `const` 初始化式**不给值**
                // （`D-M3-19`）⇒ 对象是「常量但无值」。检查面在 Task 30 里
                // 自己折了一遍（`constfold`），命中就用折叠值顶替。
                if op.mode == Mode::Constant
                    && op.val.is_none()
                    && let Some((t, v)) = self.s.consts.get(&n.value)
                {
                    op.typ = Some(*t);
                    op.val = Some(v.clone());
                }
                // 装载面的 M3 缺口：`var x = expr` **不推类型**（只记声明了类型
                // 的 var）。检查面就地补窄面推断 —— 否则 `var s = "a"` 之后的
                // `s` 在索引 / 键位一律「类型未知」⇒ 静默漏报（Task 27 实测）。
                if op.mode == Mode::Variable
                    && op.typ.is_none()
                    && let Some(t) = self.init_type_of(file, &n.value)
                {
                    op.typ = Some(t);
                }
                op
            }
            None => {
                // `--test-hooks` 注入的 `assert` / `trace`（官方
                // `DefPredeclaredTestFuncs`，`universe.go:232`）：它们**只在**
                // 该旗标下存在于 universe，且是内建（不是普通函数）。
                // 顺序：局部 → 包 → 本兜底 ⇒ 用户声明可遮蔽它们。
                if self.s.opts.test_hooks && matches!(n.value.as_str(), "assert" | "trace") {
                    return Operand::builtin();
                }
                Operand::invalid()
            }
        }
    }

    /// `var x = <初始化式>` 的**窄面**类型推断（只在装载面没给类型时用）。
    ///
    /// 覆盖基本字面量（默认型）、括号、显式类型的复合字面量与名字链；
    /// 其余形态 ⇒ `None`（类型未知 ⇒ 各判定按「不判」处理，不造假诊断）。
    fn init_type_of(&mut self, file: FileId, name: &str) -> Option<TypeId> {
        self.init_type_of_at(file, name, 0)
    }

    /// [`Self::init_type_of`] 的带深度版本。
    ///
    /// ⚠️ `depth` **必须**一路带下去：`var s1 = s1` 这种自引用会让
    /// 「名字 → 初始化式 → 名字」无限递归（Task 33 实测：整进程栈溢出）。
    fn init_type_of_at(&mut self, file: FileId, name: &str, depth: u32) -> Option<TypeId> {
        if depth > 8 {
            return None;
        }
        let pkg = self.s.pkg;
        for (i, ast) in pkg.asts.iter().enumerate() {
            for decl in &ast.decls {
                let Decl::Var(v) = decl else { continue };
                let Some(idx) = v.names.iter().position(|n| n.value == name) else {
                    continue;
                };
                let Some(value) = v.values.get(idx) else {
                    continue;
                };
                return self.init_expr_type(pkg.files[i], value, depth + 1);
            }
        }
        let _ = file;
        None
    }

    /// [`Self::init_type_of`] 的表达式面（`depth` 防环）。
    fn init_expr_type(&mut self, file: FileId, e: &Expr, depth: u32) -> Option<TypeId> {
        if depth > 8 {
            return None;
        }
        match e {
            Expr::BasicLit(l) => {
                let kind = match l.kind {
                    LitKind::Int => BasicId::UntypedInt,
                    LitKind::Float => BasicId::UntypedFloat,
                    LitKind::Imag => BasicId::UntypedComplex,
                    LitKind::Rune => BasicId::UntypedRune,
                    LitKind::String => BasicId::UntypedString,
                };
                let d = default_type_of(kind);
                Some(self.s.tt.add_basic(d))
            }
            Expr::Paren(p) => self.init_expr_type(file, &p.x, depth + 1),
            Expr::CompositeLit(cl) => {
                let te = cl.type_.as_ref()?;
                self.type_expr(file, te)
            }
            Expr::Name(n) => {
                let obj = self.s.pkg.lookup_in_file(file, &n.value)?;
                if let Some(t) = obj.typ {
                    return Some(t);
                }
                if obj.kind == ObjKind::Var {
                    return self.init_type_of_at(file, &n.value, depth + 1);
                }
                None
            }
            _ => None,
        }
    }

    /// 基本字面量（官方 `check.basicLit`）。
    ///
    /// 值解析走 M3 的 `const_from_literal`；词法/语法层已报的坏字面量
    /// （`bad`）不在这里重复。
    fn literal(&mut self, l: &BasicLit) -> Operand {
        let kind = match l.kind {
            LitKind::Int => BasicId::UntypedInt,
            LitKind::Float => BasicId::UntypedFloat,
            LitKind::Imag => BasicId::UntypedComplex,
            LitKind::Rune => BasicId::UntypedRune,
            LitKind::String => BasicId::UntypedString,
        };
        let val = const_from_literal(l);
        if val == Const::Unknown && l.kind != LitKind::String {
            // 坏字面量（如 `0x_`）：词法层已有诊断，本层不重复。
            return Operand::invalid();
        }
        let t = self.s.tt.add_basic(kind);
        Operand::constant(t, val)
    }

    // ────────────────────────────── 转换 ──────────────────────────────

    /// 官方 `Checker.call`：类型函数位 ⇒ 转换；值函数位 ⇒ **真调用**（Task 25）；
    /// 内建的**结果面**（Task 25 的窄表；签名面归 Task 29）。
    ///
    /// 内建 / 真调用的**实参检查**（个数、可赋值性、单值语境）也在这一层 ——
    /// 所谓「调用面」在官方就是 `call.go` 的一处逻辑，拆开会让位置口径分家。
    fn call(&mut self, file: FileId, c: &CallExpr) -> Operand {
        // `unsafe.X(...)`（M4/Task 31）：`unsafe` 是编译器伪包，装载面装不出
        // 它的对象表，所以调用面在**选择器求值之前**直接分派。
        if let Expr::Selector(s) = &*c.fun
            && let Expr::Name(n) = &*s.x
            && n.value == crate::unsafe_pkg::PKG_NAME
            && crate::unsafe_pkg::imports_unsafe(self.s.pkg, file)
        {
            let sel = s.sel.value.clone();
            let sel_pos = s.sel.pos;
            return crate::unsafe_pkg::call(self, file, c, &sel, sel_pos);
        }
        let f = self.expr(file, &c.fun);
        if f.mode == Mode::TypeExpr {
            let Some(target) = f.typ else {
                return Operand::invalid();
            };
            return self.conversion(file, c, target);
        }
        match f.mode {
            // `Variable` = 函数类型的**变量**（形参、局部、全局、字段）。
            //
            // M5/Task 2 修正：原先只认 `Value`（= `ObjKind::Func` 的**声明**函数），
            // 于是 `f()`（`f func(int, byte) (byte, int)`）掉进下面那条 `_` 分支
            // 变成 `invalid` —— 单结果调用会**静默通过**（假阴性），多结果调用
            // 则被报成 `assignment mismatch: 2 variables but f returns 1 value`
            // （`method5.go:103` 的实测形态）。官方 `call` 对**任何**非类型表达式
            // 的操作数都按其类型走 `arguments`，不区分「声明函数」与「函数值」。
            Mode::Value | Mode::Variable => self.call_value(file, c, f.typ),
            Mode::Builtin => self.call_builtin(file, c, &c.fun),
            // 无效 / 类型族函数位 / 其他：不报（转换之外的类型族归 Task 23+）。
            _ => Operand::invalid(),
        }
    }

    /// 官方 `Checker.conversion`（`conversions.go:20`）的窄面。
    pub(crate) fn conversion(&mut self, file: FileId, c: &CallExpr, target: TypeId) -> Operand {
        // 实参数量 / `...` 归调用面（Task 23+/25）；窄面只接单实参。
        if c.args.len() != 1 || c.has_dots {
            return Operand::invalid();
        }
        let arg = &c.args[0];
        let x = self.expr(file, arg);
        if x.mode == Mode::Invalid {
            return Operand::invalid();
        }
        let arg_pos = arg.pos();
        let Some(xt) = x.typ else {
            return Operand::invalid();
        };

        if x.mode == Mode::Constant && is_const_type(self.tt(), target) {
            let val = x.val.clone().unwrap_or(Const::Unknown);
            let b = basic_kind(self.tt(), target).expect("is_const_type 已保证目标底层是 basic");
            return match representable(&val, b, &Sizes::HOST_ARM64) {
                Ok(rep) => Operand::constant(target, rep.value),
                Err(_) => {
                    // 整数常量 → 字符串：**恒可**（无效码点 ⇒ U+FFFD，
                    // `string(-1)` / `string(1<<100)` 都合法）。
                    if all_integer(self.tt(), xt) && b.is_string() {
                        return Operand::constant(target, Const::Str(int_const_to_string(&val)));
                    }
                    let to = self.type_text(target);
                    if all_integer(self.tt(), xt) && b.is_integer() {
                        self.report(
                            arg_pos,
                            DiagCode::TypNumericOverflow,
                            format!("constant {} overflows {to}", const_text(&val)),
                        );
                    } else {
                        self.report(
                            arg_pos,
                            DiagCode::TypInvalidConversion,
                            format!("cannot convert {} to type {to}", const_text(&val)),
                        );
                    }
                    Operand::invalid()
                }
            };
        }

        // 非恒量面（含 `nil`）：静态类型对可转换性（M4/Task 9）。
        match convertible(self.tt(), xt, target) {
            Ok(true) => Operand::value(Some(target)),
            Ok(false) => {
                let from = self.type_text(xt);
                let to = self.type_text(target);
                self.report(
                    arg_pos,
                    DiagCode::TypInvalidConversion,
                    format!("cannot convert value of type {from} to type {to}"),
                );
                Operand::invalid()
            }
            // 类型参数窄面等能力边界：不报（台账登记；语料撞面在 Task 37）。
            Err(_) => Operand::invalid(),
        }
    }

    // ────────────────────────────── 调用（Task 25）──────────────────────────────

    /// 取一个类型的签名条目（非签名 ⇒ `None`）。
    fn signature_id(&self, t: TypeId) -> Option<rgoc_types::SignatureId> {
        let tt = self.tt();
        match tt.type_of(underlying(tt, t)) {
            Type::Signature(s) => Some(*s),
            _ => None,
        }
    }

    /// 值函数位的**真调用**（官方 `call.go` 的 `arguments` 面）。
    ///
    /// 窄面（台账登记）：
    /// * **变参**只接「实参个数够」的形态，`...` 与变参逐参类型不判；
    /// * 函数值类型参数（泛型函数）已由 Task 14 的推断面覆盖，这里只按
    ///   已实例化的签名判；
    /// * 返回值个数为 1 时给类型、>1 时给**元组类型**（官方同形：多值以
    ///   `*Tuple` 出现在 `operand.typ` 里），0 个 ⇒ `novalue`。
    fn call_value(&mut self, file: FileId, c: &CallExpr, ty: Option<TypeId>) -> Operand {
        let Some(t) = ty else {
            return Operand::invalid();
        };
        let Some(sid) = self.signature_id(t) else {
            return Operand::invalid();
        };
        let sig = self.tt().signature_def(sid).clone();
        let params = self.tuple_vars(sig.params);
        let results = self.tuple_vars(sig.results);

        // ① 实参逐个求值（顺序照官方：先全部求值，再判长度）
        let mut args: Vec<Operand> = Vec::with_capacity(c.args.len());
        for a in &c.args {
            let x = self.expr(file, a);
            args.push(x);
        }
        // 实参位（展开后与 `args` 等长；未展开时就是源实参的起点）
        let mut arg_pos: Vec<Pos> = c.args.iter().map(|a| a.pos()).collect();

        // ①.5 单实参是**多值调用** ⇒ 展开成逐个实参（官方 `call.go` 的
        //      `arguments`：`f(g())` 当 `g` 返回 n 个值时，各返回值按位置
        //      成为 `f` 的实参）。
        //
        //      M5/Task 2 补：原先没有这一支，`swap(swap(a, b))` 会先被算成
        //      1 个实参、再报 `not enough arguments in call to swap`
        //      （`simassign.go:75` 的实测形态）。
        if args.len() == 1
            && let Some(t) = args[0].typ
        {
            let elems = self.tuple_vars(t);
            if !elems.is_empty() {
                let at = c.args[0].pos();
                args = elems
                    .into_iter()
                    .map(|(_, t)| Operand::value(Some(t)))
                    .collect();
                arg_pos = vec![at; args.len()];
            }
        }

        // ② 个数（官方 `call.go:531-555`）。
        let npars = params.len();
        let variadic_ok = sig.variadic && args.len() + 1 >= npars;
        if !variadic_ok && args.len() != npars {
            let (at, qualifier) = if args.len() > npars {
                // 多的第一个实参位。
                (*arg_pos.get(npars).unwrap_or(&c.rparen), "too many")
            } else {
                // 官方 `atPos(call.Rparen)`：`)` 位。
                (c.rparen, "not enough")
            };
            let msg = format!(
                "{qualifier} arguments in call to {}\n\thave {}\n\twant {}",
                expr_label(&c.fun).unwrap_or_else(|| "function".to_owned()),
                tuple_text(self.tt(), &args),
                params_text(self.tt(), &params, sig.variadic),
            );
            self.report(at, DiagCode::ChkInvalidAssign, msg);
            return Operand::invalid();
        }

        // ③ 逐参可赋值性（变参末尾不展开 —— 窄面）
        if !sig.variadic {
            for (i, p) in params.iter().enumerate() {
                let Some(at) = arg_pos.get(i).copied() else {
                    break;
                };
                let mut x = args[i].clone();
                self.assignment(
                    &mut x,
                    at,
                    Some(p.1),
                    &format!(
                        "argument to {}",
                        expr_label(&c.fun).unwrap_or_else(|| "function".to_owned())
                    ),
                );
            }
        }

        // ④ 结果面
        match results.len() {
            0 => Operand::no_value(),
            1 => Operand::value(Some(results[0].1)),
            _ => {
                let tt = &mut *self.s.tt;
                let tid = tt.add_tuple(results.clone());
                let tup = tt.tuple_def(tid).self_ty;
                Operand::value(Some(tup))
            }
        }
    }

    /// 内建调用（Task 29 起由 [`crate::builtins`] 接管签名与静态约束）。
    fn call_builtin(&mut self, file: FileId, c: &CallExpr, fun: &Expr) -> Operand {
        let Some(name) = expr_label(fun) else {
            return Operand::invalid();
        };
        crate::builtins::call_builtin(self, file, c, &name)
    }

    /// 元组条目（`Type::Invalid` / 非元组 ⇒ 空表 —— 签名残缺时不报假错）。
    fn tuple_vars(&self, t: TypeId) -> Vec<(Option<String>, TypeId)> {
        let tt = self.tt();
        match tt.type_of(unalias(tt, t)) {
            Type::Tuple(id) => tt.tuple_def(*id).vars.clone(),
            _ => Vec::new(),
        }
    }

    /// 官方 `Checker.assignment`（`assignments.go:22`）的窄面。
    ///
    /// `target == None` ⇒ 空标识符位（不做类型判定）。类型未知（`None` /
    /// `invalid`）⇒ **不判**（假阴性优于假阳性；台账登记）。
    /// 失败时把操作数打成 `invalid`（官方 `x.invalidate()`），调用方据此
    /// **不再**级联报错。
    pub(crate) fn assignment(
        &mut self,
        x: &mut Operand,
        xpos: Pos,
        target: Option<TypeId>,
        context: &str,
    ) -> bool {
        if x.mode == Mode::Invalid {
            return false;
        }
        let Some(t) = target else {
            return true;
        };
        let Some(xt) = x.typ else {
            return true;
        };
        {
            let tt = self.tt();
            if matches!(tt.type_of(unalias(tt, t)), Type::Invalid)
                || matches!(tt.type_of(unalias(tt, xt)), Type::Invalid)
            {
                return true;
            }
        }
        match assignable(self.tt(), xt, t) {
            Ok(true) => true,
            Ok(false) => {
                let desc = type_str(self.tt(), t);
                let what = operand_text(self.tt(), x);
                self.report(
                    xpos,
                    DiagCode::ChkInvalidAssign,
                    format!("cannot use {what} as {desc} value in {context}"),
                );
                x.mode = Mode::Invalid;
                false
            }
            // 能力边界（类型参数面）：不报。
            Err(_) => true,
        }
    }

    // ────────────────────────────── 索引 / 切片 ──────────────────────────────

    /// 官方 `indexExpr`（`index.go:19`）的窄面。
    ///
    /// 泛型面（`X[A]` 类型实例化作值语境 / 函数实例化 / 类型参数接口索引）
    /// 归 Task 16 的 API 与 Task 25 之后的语境，此处 `invalid` 且**不报**。
    fn index_expr(&mut self, file: FileId, e: &IndexExpr) -> Operand {
        let mut x = self.expr(file, &e.x);
        if x.mode == Mode::Invalid || x.mode == Mode::TypeExpr {
            return Operand::invalid();
        }
        let Some(xt) = x.typ else {
            return Operand::invalid();
        };
        // 官方 `x.typ().Underlying()` 分类（窄面无类型参数）。
        enum Shape {
            Str,
            Arr { len: i64, elem: TypeId },
            PtrArr { len: i64, elem: TypeId },
            Slice(TypeId),
            Map(TypeId),
            Other,
        }
        let shape = {
            let tt = self.tt();
            match tt.type_of(underlying(tt, xt)) {
                Type::Basic(b) if b.is_string() => Shape::Str,
                Type::Array { len, elem } => Shape::Arr {
                    len: arr_len(tt, *len),
                    elem: *elem,
                },
                Type::Pointer(base) => match tt.type_of(underlying(tt, *base)) {
                    Type::Array { len, elem } => Shape::PtrArr {
                        len: arr_len(tt, *len),
                        elem: *elem,
                    },
                    _ => Shape::Other,
                },
                Type::Slice(elem) => Shape::Slice(*elem),
                Type::Map { elem, .. } => Shape::Map(*elem),
                _ => Shape::Other,
            }
        };
        let mut length: i64 = -1;
        match shape {
            Shape::Str => {
                length = if x.mode == Mode::Constant {
                    string_len(&x.val)
                } else {
                    -1
                };
                // 官方：字符串索引**恒**产出 byte 值（非常量 —— 即时字符串与
                // 索引都是常量）。
                x.mode = Mode::Value;
                x.typ = Some(self.s.tt.add_basic(BasicId::Uint8));
                x.val = None;
            }
            Shape::Arr { len, elem } => {
                length = len;
                if x.mode != Mode::Variable {
                    x.mode = Mode::Value;
                }
                x.typ = Some(elem);
                x.val = None;
            }
            Shape::PtrArr { len, elem } => {
                // 指向数组的指针：隐式解引用 ⇒ 变量。
                length = len;
                x.mode = Mode::Variable;
                x.typ = Some(elem);
                x.val = None;
            }
            Shape::Slice(elem) => {
                x.mode = Mode::Variable;
                x.typ = Some(elem);
                x.val = None;
            }
            Shape::Map(elem) => {
                // 键面（`cannot use ... in map index`）归 Task 25 的赋值面；
                // 窄面只求索引表达式本身的诊断，结果照官方置 `mapindex`。
                let _ = self.expr(file, &e.index);
                x.mode = Mode::MapIndex;
                x.typ = Some(elem);
                x.val = None;
                return x;
            }
            Shape::Other => {
                let _ = self.expr(file, &e.index);
                let what = operand_text(self.tt(), &x);
                self.report(
                    e.x.pos(),
                    DiagCode::ChkInvalidIndex,
                    format!("cannot index {what}"),
                );
                return Operand::invalid();
            }
        }
        // 索引操作数：官方 `check.index`（`convertUntyped` + 整数性 + 非负 +
        // 溢出 int + 越界 —— 后两项在常量时被判）。
        let mut idx = self.expr(file, &e.index);
        if !self.valid_index(&mut idx, e.index.pos()) {
            return Operand::invalid();
        }
        if idx.mode == Mode::Constant {
            let val = idx.val.clone().unwrap_or(Const::Unknown);
            if val != Const::Unknown
                && let Some(v) = const_as_i64(&val)
                && length >= 0
                && v >= length
            {
                self.report(
                    e.index.pos(),
                    DiagCode::ChkInvalidIndex,
                    format!(
                        "invalid argument: index {} out of bounds [0:{length}]",
                        const_text(&val)
                    ),
                );
                return Operand::invalid();
            }
        }
        x
    }

    /// 官方 `sliceExpr`（`index.go:233`）的窄面。
    fn slice_expr(&mut self, file: FileId, e: &SliceExpr) -> Operand {
        let mut x = self.expr(file, &e.x);
        if x.mode == Mode::Invalid {
            return Operand::invalid();
        }
        let Some(xt) = x.typ else {
            return Operand::invalid();
        };
        enum Cu {
            Str,
            Arr { len: i64, elem: TypeId },
            PtrArr { len: i64, elem: TypeId },
            Slice,
            Other,
        }
        let cu = {
            let tt = self.tt();
            match tt.type_of(underlying(tt, xt)) {
                Type::Basic(b) if b.is_string() => Cu::Str,
                Type::Array { len, elem } => Cu::Arr {
                    len: arr_len(tt, *len),
                    elem: *elem,
                },
                Type::Pointer(base) => match tt.type_of(underlying(tt, *base)) {
                    Type::Array { len, elem } => Cu::PtrArr {
                        len: arr_len(tt, *len),
                        elem: *elem,
                    },
                    _ => Cu::Other,
                },
                Type::Slice(_) => Cu::Slice,
                _ => Cu::Other,
            }
        };
        let mut length: i64 = -1;
        match cu {
            Cu::Str => {
                if e.full {
                    // 官方：`at := e.Max; if at == nil { at = e }`。
                    let at = e.max.as_deref().map_or(e.pos, Expr::pos);
                    self.report(
                        at,
                        DiagCode::ChkInvalidSlice,
                        "invalid operation: 3-index slice of string".to_string(),
                    );
                    return Operand::invalid();
                }
                length = if x.mode == Mode::Constant {
                    string_len(&x.val)
                } else {
                    -1
                };
                // 无类型字符串切片 ⇒ 非常量 string（有名字符串保持自身）。
                if is_untyped(self.tt(), xt) {
                    let st = self.s.tt.add_basic(BasicId::String);
                    x.typ = Some(st);
                }
            }
            Cu::Arr { len, elem } => {
                length = len;
                if x.mode != Mode::Variable {
                    let what = operand_text(self.tt(), &x);
                    self.report(
                        e.x.pos(),
                        DiagCode::ChkInvalidSlice,
                        format!("cannot slice unaddressable value {what}"),
                    );
                    return Operand::invalid();
                }
                let st = self.s.tt.add_slice(elem);
                x.typ = Some(st);
            }
            Cu::PtrArr { len, elem } => {
                length = len;
                let st = self.s.tt.add_slice(elem);
                x.typ = Some(st);
            }
            Cu::Slice => {
                // 官方：`x.typ` 不变（命名切片保持命名）。
            }
            Cu::Other => {
                let what = operand_text(self.tt(), &x);
                self.report(
                    e.x.pos(),
                    DiagCode::ChkInvalidSlice,
                    format!("cannot slice {what}"),
                );
                return Operand::invalid();
            }
        }
        x.mode = Mode::Value;
        x.val = None;

        // 下标：官方 `check.index` 逐个（`max = length + 1`）。
        let exprs: [Option<&Expr>; 3] = [e.low.as_deref(), e.high.as_deref(), e.max.as_deref()];
        let mut ind = [-1i64; 3];
        for (i, expr) in exprs.iter().enumerate() {
            let mut v: i64 = -1;
            match expr {
                Some(ex) => {
                    let max = if length >= 0 { length + 1 } else { -1 };
                    v = self.index_of(file, ex, max);
                }
                None => {
                    // 官方：首位缺省 0；其余在长度已知时缺省 length。
                    if i == 0 {
                        v = 0;
                    } else if length >= 0 {
                        v = length;
                    }
                }
            }
            ind[i] = v;
        }
        // 常量下标的次序约束（官方 `L:` 双循环：**只报一条**）。
        'outer: for i in 0..ind.len() - 1 {
            let xv = ind[i];
            if xv > 0 {
                for j in 0..ind.len() - i - 1 {
                    let yv = ind[i + 1 + j];
                    if yv >= 0 && yv < xv {
                        let at = exprs[i + 1 + j].map_or(e.pos, Expr::pos);
                        self.report(
                            at,
                            DiagCode::ChkInvalidSlice,
                            format!("invalid slice indices: {yv} < {xv}"),
                        );
                        break 'outer;
                    }
                }
            }
        }
        x
    }

    /// 官方 `index`（`index.go:420`）：索引合法性 + 常量越界；
    /// 返回常量值（`-1` = 非常量 / 无效 / 已报错）。
    fn index_of(&mut self, file: FileId, ex: &Expr, max: i64) -> i64 {
        let mut idx = self.expr(file, ex);
        if !self.valid_index(&mut idx, ex.pos()) {
            return -1;
        }
        if idx.mode != Mode::Constant {
            return -1;
        }
        let val = idx.val.clone().unwrap_or(Const::Unknown);
        if val == Const::Unknown {
            return -1;
        }
        let Some(v) = const_as_i64(&val) else {
            return -1;
        };
        if max >= 0 && v >= max {
            self.report(
                ex.pos(),
                DiagCode::ChkInvalidIndex,
                format!(
                    "invalid argument: index {} out of bounds [0:{max}]",
                    const_text(&val)
                ),
            );
            return -1;
        }
        v
    }

    /// 官方 `isValidIndex`（`index.go:449`）：无类型常量 ⇒ `int`；
    /// 整数性 / 非负 / 可表示（超 `int` 范围 ⇒ `overflows int`）。
    pub(crate) fn valid_index(&mut self, x: &mut Operand, pos: Pos) -> bool {
        if x.mode == Mode::Invalid {
            return false;
        }
        let int_t = self.s.tt.add_basic(BasicId::Int);
        self.convert_untyped(x, int_t, pos);
        if x.mode == Mode::Invalid {
            return false;
        }
        let Some(xt) = x.typ else {
            return false;
        };
        if !all_integer(self.tt(), xt) {
            let what = operand_text(self.tt(), x);
            self.report(
                pos,
                DiagCode::ChkInvalidIndex,
                format!("invalid argument: index {what} must be integer"),
            );
            return false;
        }
        if x.mode == Mode::Constant {
            let val = x.val.clone().unwrap_or(Const::Unknown);
            if const_negative(&val) {
                self.report(
                    pos,
                    DiagCode::ChkInvalidIndex,
                    format!(
                        "invalid argument: index {} must not be negative",
                        const_text(&val)
                    ),
                );
                return false;
            }
            if val != Const::Unknown
                && representable(&val, BasicId::Int, &Sizes::HOST_ARM64).is_err()
            {
                self.report(
                    pos,
                    DiagCode::TypNumericOverflow,
                    format!("invalid argument: index {} overflows int", const_text(&val)),
                );
                return false;
            }
        }
        true
    }

    // ────────────────────────────── 复合字面量 ──────────────────────────────

    /// 类型表达式 → `TypeId`（官方 `check.typ` 的**窄面**）。
    ///
    /// 覆盖复合字面量类型位实际会出现的形态：名字、括号、数组 / 切片 /
    /// map / chan / 指针。匿名 `struct{…}` / `interface{…}` / `func(…)` 暂不接
    /// （`None` ⇒ 调用方 `invalid` 且**不报**，负向缺口由 `U-M4` 承担）。
    pub(crate) fn type_expr(&mut self, file: FileId, e: &Expr) -> Option<TypeId> {
        match e {
            Expr::Name(n) => {
                let op = self.ident(file, n);
                if op.mode == Mode::TypeExpr {
                    op.typ
                } else {
                    None
                }
            }
            Expr::Paren(p) => self.type_expr(file, &p.x),
            Expr::ArrayType(a) => {
                let elem = self.type_expr(file, &a.elem)?;
                let len = match &a.len {
                    None => None,
                    Some(le) => {
                        let op = self.expr(file, le);
                        if op.mode != Mode::Constant {
                            return None;
                        }
                        let v = op.val.clone().unwrap_or(Const::Unknown);
                        let n = const_as_i64(&v)?;
                        Some(self.s.tt.consts_mut().add(Const::Int(BigInt::from_i64(n))))
                    }
                };
                Some(self.s.tt.add_array(len, elem))
            }
            Expr::SliceType(s) => {
                let elem = self.type_expr(file, &s.elem)?;
                Some(self.s.tt.add_slice(elem))
            }
            Expr::MapType(m) => {
                let key = self.type_expr(file, &m.key)?;
                let val = self.type_expr(file, &m.value)?;
                Some(self.s.tt.add_map(key, val))
            }
            Expr::ChanType(c) => {
                let elem = self.type_expr(file, &c.elem)?;
                let dir = match c.dir {
                    None => ChanDir::SendRecv,
                    Some(rgoc_ast::ChanDir::SendOnly) => ChanDir::SendOnly,
                    Some(rgoc_ast::ChanDir::RecvOnly) => ChanDir::RecvOnly,
                };
                Some(self.s.tt.add_chan(dir, elem))
            }
            // `*T`（一元 `*` 的 typexpr 臂）。
            Expr::Unary(u) if u.op == Op::Mul => {
                let t = self.type_expr(file, &u.x)?;
                Some(self.s.tt.add_pointer(t))
            }
            // `struct{…}`（M4/Task 37）：字段名 / 标签 / 嵌入三件套，字段类型递归。
            Expr::StructType(st) => {
                let mut fields = Vec::with_capacity(st.fields.len());
                for f in &st.fields {
                    let typ = self.type_expr(file, &f.type_)?;
                    fields.push(rgoc_types::Field {
                        name: f.name.as_ref().map(|n| n.value.clone()),
                        typ,
                        tag: f.tag.as_ref().map(|t| t.value.clone()),
                        embedded: f.name.is_none(),
                    });
                }
                let id = self.s.tt.add_struct(fields);
                Some(self.s.tt.struct_type(id))
            }
            // `interface{…}`（M4/Task 37）：只接**方法集**形态；嵌入接口
            // （无名字段）需要合并类型集 ⇒ 本阶段 `None`（`U-M4` 面）。
            Expr::InterfaceType(it) => {
                let mut methods = Vec::with_capacity(it.methods.len());
                for f in &it.methods {
                    let n = f.name.as_ref()?;
                    let sig = self.type_expr(file, &f.type_)?;
                    methods.push((n.value.clone(), sig));
                }
                let id = self
                    .s
                    .tt
                    .add_interface(methods, None, rgoc_types::IfaceTSet::All);
                Some(self.s.tt.interface_type(id))
            }
            // `func(…) …`（M4/Task 37）：变参 `...T` ⇒ 末参类型是 `[]T`。
            Expr::FuncType(ft) => self.func_type_expr(file, ft),
            // `...T` 单独出现只在参数表里，落到这里说明上下文不对。
            Expr::DotsType(d) => self.type_expr(file, &d.elem),
            _ => None,
        }
    }

    /// 函数类型 → 签名 `TypeId`（官方 `check.typ` 的 `*FuncType` 分支窄面）。
    fn func_type_expr(&mut self, file: FileId, ft: &rgoc_ast::FuncType) -> Option<TypeId> {
        let mut variadic = false;
        let mut params = Vec::with_capacity(ft.params.len());
        for (i, p) in ft.params.iter().enumerate() {
            let last = i + 1 == ft.params.len();
            let (t, dots) = match &p.type_ {
                Expr::DotsType(d) => (self.type_expr(file, &d.elem)?, true),
                other => (self.type_expr(file, other)?, false),
            };
            if dots {
                if !last {
                    return None;
                }
                variadic = true;
                let sl = self.s.tt.add_slice(t);
                params.push((p.name.as_ref().map(|n| n.value.clone()), sl));
            } else {
                params.push((p.name.as_ref().map(|n| n.value.clone()), t));
            }
        }
        let mut results = Vec::with_capacity(ft.results.len());
        for p in &ft.results {
            let t = self.type_expr(file, &p.type_)?;
            results.push((p.name.as_ref().map(|n| n.value.clone()), t));
        }
        let pt = self.s.tt.add_tuple(params);
        let rt = self.s.tt.add_tuple(results);
        let sig = self.s.tt.add_signature(None, pt, rt, variadic);
        Some(self.s.tt.signature_type(sig))
    }

    /// 官方 `compositeLit`（`literals.go:111`）的窄面（Task 27）。
    ///
    /// `hint` = 外层复合字面量给的**元素类型**（嵌套字面量省略类型时用）。
    /// `*T` 的提示 ⇒ `base` 取 `T`（`&T{}` 的隐式取址），但结果类型仍是 `*T`。
    fn composite_lit(&mut self, file: FileId, e: &CompositeLit, hint: Option<TypeId>) -> Operand {
        // ① 类型：显式 > 提示 > `UntypedLit`。
        //    `[...]T` 单独一路（先占 `len = None` 的数组，元素检查后回填）。
        let mut dots_elem: Option<TypeId> = None;
        let (typ, base) = match &e.type_ {
            Some(te) => {
                if let Expr::ArrayType(a) = &**te
                    && a.len.is_none()
                {
                    let Some(elem) = self.type_expr(file, &a.elem) else {
                        return Operand::invalid();
                    };
                    dots_elem = Some(elem);
                    let t = self.s.tt.add_array(None, elem);
                    (t, t)
                } else {
                    let Some(t) = self.type_expr(file, te) else {
                        // 类型表达式面未接（匿名 struct 等）：静默 invalid。
                        return Operand::invalid();
                    };
                    (t, t)
                }
            }
            None => match hint {
                Some(h) => {
                    let base = {
                        let tt = self.tt();
                        match tt.type_of(underlying(tt, h)) {
                            Type::Pointer(inner) => *inner,
                            _ => h,
                        }
                    };
                    (h, base)
                }
                None => {
                    self.report(
                        e.pos,
                        DiagCode::ChkInvalidLit,
                        "missing type in composite literal".to_string(),
                    );
                    return Operand::invalid();
                }
            },
        };

        // ② 按 base 的底层分类。
        enum Shape {
            Struct(Vec<rgoc_types::Field>),
            Array { len: i64, elem: TypeId },
            Slice(TypeId),
            Map { key: TypeId, elem: TypeId },
            Other,
        }
        let shape = {
            let tt = self.tt();
            match tt.type_of(underlying(tt, base)) {
                Type::Struct(id) => Shape::Struct(tt.struct_def(*id).fields.clone()),
                Type::Array { len, elem } => Shape::Array {
                    len: arr_len(tt, *len),
                    elem: *elem,
                },
                Type::Slice(elem) => Shape::Slice(*elem),
                Type::Map { key, elem } => Shape::Map {
                    key: *key,
                    elem: *elem,
                },
                _ => Shape::Other,
            }
        };

        match shape {
            Shape::Struct(fields) => {
                if !e.elems.is_empty() {
                    if matches!(e.elems[0], Expr::KeyValue(_)) {
                        self.struct_lit_keyed(file, e, base, &fields);
                    } else {
                        self.struct_lit_values(file, e, base, &fields);
                    }
                }
            }
            Shape::Array { len, elem } => {
                let n = self.indexed_elts(file, &e.elems, elem, len);
                if dots_elem.is_some() {
                    // `[...]T`：回填长度（官方 `utyp.len = n`）。
                    let cid = self.s.tt.consts_mut().add(Const::Int(BigInt::from_i64(n)));
                    let t = self.s.tt.add_array(Some(cid), elem);
                    return Operand::value(Some(t));
                }
            }
            Shape::Slice(elem) => {
                self.indexed_elts(file, &e.elems, elem, -1);
            }
            Shape::Map { key, elem } => self.map_lit(file, e, key, elem),
            Shape::Other => {
                // 官方 default 支：先「用掉」元素，再报无效类型。
                for el in &e.elems {
                    let eval = match el {
                        Expr::KeyValue(kv) => &kv.value,
                        _ => el,
                    };
                    let _ = self.expr(file, eval);
                }
                let t = self.type_text(typ);
                // 官方报在 `e`（`ast.CompositeLit.Pos()` = **类型起点**；本仓
                // `CompositeLit.pos` 存的是 `{`，见 `parser.go` 的 `pos` 口径）。
                let at = e.type_.as_ref().map_or(e.pos, |te| te.start_pos());
                self.report(
                    at,
                    DiagCode::ChkInvalidLit,
                    format!("invalid composite literal type {t}"),
                );
                return Operand::invalid();
            }
        }

        Operand::value(Some(typ))
    }

    /// 结构体字面量的**键值形态**（官方 `literals.go:157-207`）。
    fn struct_lit_keyed(
        &mut self,
        file: FileId,
        e: &CompositeLit,
        base: TypeId,
        fields: &[rgoc_types::Field],
    ) {
        // `visited`：已用过的字段路径 + 字段名（官方的 `trie[*Var]`）。
        let mut visited: Vec<(Vec<usize>, String)> = Vec::new();
        for el in &e.elems {
            let Expr::KeyValue(kv) = el else {
                // 键值形态里混进值元素：报在**该元素**上。
                self.report(
                    el.start_pos(),
                    DiagCode::ChkInvalidLit,
                    "mixture of field:value and value elements in struct literal".to_string(),
                );
                continue;
            };
            // 官方先求值 value（`check.genericExpr(x, kv.Value, nil)`）。
            let mut x = self.expr_hint(file, &kv.value, None);

            let Expr::Name(key) = &*kv.key else {
                self.report(
                    kv.key.start_pos(),
                    DiagCode::ChkInvalidLitField,
                    format!(
                        "invalid field name {} in struct literal",
                        expr_label(&kv.key).unwrap_or_else(|| "?".to_string())
                    ),
                );
                continue;
            };
            let name = key.value.as_str();

            // 查找（官方 `lookupFieldOrMethod(utyp, false, pkg, name, false)`）。
            let (found, index, indirect) = {
                let tt = self.tt();
                let r = lookup_field_or_method(tt, base, false, name);
                (r.obj, r.index, r.indirect)
            };
            let Some(found) = found else {
                let msg = if index.is_some() {
                    format!("ambiguous selector {name}")
                } else {
                    let t = self.type_text(base);
                    format!("unknown field {name} in struct literal of type {t}")
                };
                self.report(kv.key.start_pos(), DiagCode::ChkInvalidLitField, msg);
                continue;
            };
            let Found::Field { typ: ftyp } = found else {
                self.report(
                    kv.key.start_pos(),
                    DiagCode::ChkInvalidLitField,
                    format!("{name} is not a field"),
                );
                continue;
            };
            let Some(path) = index else {
                self.report(
                    kv.key.start_pos(),
                    DiagCode::ChkInvalidLitField,
                    format!("unknown field {name} in struct literal"),
                );
                continue;
            };
            // 提升字段键的 1.27 门控（`literals.go:194`）。
            if path.len() > 1 {
                let subject = format!(
                    "{} in struct literal of type {}",
                    self.field_path_text(fields, &path),
                    self.type_text(base)
                );
                if let Some(raw) = crate::version::verify_version_soft(
                    &self.s.pkg.versions,
                    file,
                    kv.key.start_pos(),
                    crate::version::Feat::PromotedFieldKey,
                    &subject,
                ) {
                    self.s.report(raw);
                    // 官方 `continue`：门控拒绝后不再走后面的判定。
                    continue;
                }
            }
            if indirect {
                self.report(
                    kv.key.start_pos(),
                    DiagCode::ChkInvalidLitField,
                    format!("invalid implicit pointer indirection to reach {name}"),
                );
                continue;
            }
            // 赋值面（官方 `check.assignment(x, etyp, "struct literal")`）。
            self.assignment(&mut x, kv.value.start_pos(), Some(ftyp), "struct literal");
            // 重复面（官方 `visited.insert`）。
            let dup = visited
                .iter()
                .find(|(p, _)| p == &path)
                .map(|(_, n)| n.clone());
            if let Some(prev) = dup {
                self.report(
                    kv.key.start_pos(),
                    DiagCode::ChkDuplicateLitField,
                    format!("duplicate field name {prev} in struct literal"),
                );
            } else if let Some((_, enclosing)) = visited
                .iter()
                .find(|(p, _)| p.len() < path.len() && path.starts_with(p))
            {
                self.report(
                    kv.key.start_pos(),
                    DiagCode::ChkInvalidLitField,
                    format!(
                        "cannot specify promoted field {name} and enclosing embedded field {enclosing}"
                    ),
                );
            } else if let Some((_, enclosed)) = visited
                .iter()
                .find(|(p, _)| path.len() < p.len() && p.starts_with(&path))
            {
                self.report(
                    kv.key.start_pos(),
                    DiagCode::ChkInvalidLitField,
                    format!("cannot specify embedded field {name} and enclosed promoted field {enclosed}"),
                );
            }
            visited.push((path, name.to_string()));
        }
    }

    /// 结构体字面量的**值形态**（官方 `literals.go:209-240`）。
    fn struct_lit_values(
        &mut self,
        file: FileId,
        e: &CompositeLit,
        base: TypeId,
        fields: &[rgoc_types::Field],
    ) {
        for (i, el) in e.elems.iter().enumerate() {
            if let Expr::KeyValue(kv) = el {
                self.report(
                    kv.key.start_pos(),
                    DiagCode::ChkInvalidLit,
                    "mixture of field:value and value elements in struct literal".to_string(),
                );
                continue;
            }
            let mut x = self.expr_hint(file, el, None);
            if i >= fields.len() {
                let t = self.type_text(base);
                self.report(
                    el.start_pos(),
                    DiagCode::ChkStructLitCount,
                    format!("too many values in struct literal of type {t}"),
                );
                break;
            }
            let ftyp = fields[i].typ;
            self.assignment(&mut x, el.start_pos(), Some(ftyp), "struct literal");
        }
        if e.elems.len() < fields.len() {
            let t = self.type_text(base);
            self.report(
                e.rbrace,
                DiagCode::ChkStructLitCount,
                format!("too few values in struct literal of type {t}"),
            );
        }
    }

    /// 官方 `indexedElts`（`literals.go:349`）：数组 / 切片字面量的下标与元素。
    ///
    /// 返回字面量长度（`max index + 1`），供 `[...]T` 回填。
    fn indexed_elts(&mut self, file: FileId, elts: &[Expr], elem: TypeId, length: i64) -> i64 {
        let mut visited: Vec<i64> = Vec::new();
        let mut index: i64 = 0;
        let mut max: i64 = 0;
        for el in elts {
            let mut valid = false;
            let mut eval = el;
            if let Expr::KeyValue(kv) = el {
                let (ok, i) = self.lit_index(file, &kv.key, length);
                if ok {
                    if i >= 0 {
                        index = i;
                        valid = true;
                    } else {
                        let k = expr_label(&kv.key).unwrap_or_else(|| "?".to_string());
                        self.report(
                            kv.key.start_pos(),
                            DiagCode::ChkInvalidLitIndex,
                            format!("index {k} must be integer constant"),
                        );
                    }
                }
                eval = &kv.value;
            } else if length >= 0 && index >= length {
                // 官方 `OversizeArrayLit`（位置元素越界）—— 本仓 `CHK-INVALID-LIT-INDEX`。
                self.report(
                    el.start_pos(),
                    DiagCode::ChkInvalidLitIndex,
                    format!("index {index} is out of bounds (>= {length})"),
                );
            } else {
                valid = true;
            }
            if valid {
                if visited.contains(&index) {
                    self.report(
                        el.start_pos(),
                        DiagCode::ChkDuplicateLitIndex,
                        format!("duplicate index {index} in array or slice literal"),
                    );
                }
                visited.push(index);
            }
            index += 1;
            if index > max {
                max = index;
            }
            let mut x = self.expr_hint(file, eval, Some(elem));
            self.assignment(
                &mut x,
                eval.start_pos(),
                Some(elem),
                "array or slice literal",
            );
        }
        max
    }

    /// 官方 `check.index`（`index.go:420`）的字面量键面：
    /// 返回 `(类型有效, 下标值)`；`-1` = 非常量 / 未知（调用方报
    /// `must be integer constant`）。越界与非法下标在内部报（类别 `index`）。
    fn lit_index(&mut self, file: FileId, key: &Expr, max: i64) -> (bool, i64) {
        let mut x = self.expr(file, key);
        if !self.valid_index(&mut x, key.start_pos()) {
            return (false, -1);
        }
        if x.mode != Mode::Constant {
            return (true, -1);
        }
        let val = x.val.clone().unwrap_or(Const::Unknown);
        if val == Const::Unknown {
            return (true, -1);
        }
        let Some(v) = const_as_i64(&val) else {
            return (true, -1);
        };
        if max >= 0 && v >= max {
            self.report(
                key.start_pos(),
                DiagCode::ChkInvalidIndex,
                format!(
                    "invalid argument: index {} out of bounds [0:{max}]",
                    const_text(&val)
                ),
            );
            return (false, -1);
        }
        (true, v)
    }

    /// map 字面量（官方 `literals.go:287-330`）。
    fn map_lit(&mut self, file: FileId, e: &CompositeLit, key_typ: TypeId, elem: TypeId) {
        let mut visited: Vec<Const> = Vec::new();
        for el in &e.elems {
            let Expr::KeyValue(kv) = el else {
                self.report(
                    el.start_pos(),
                    DiagCode::ChkInvalidLit,
                    "missing key in map literal".to_string(),
                );
                continue;
            };
            let mut k = self.expr_hint(file, &kv.key, Some(key_typ));
            self.assignment(&mut k, kv.key.start_pos(), Some(key_typ), "map literal");
            if k.mode == Mode::Invalid {
                continue;
            }
            if k.mode == Mode::Constant {
                let val = k.val.clone().unwrap_or(Const::Unknown);
                if val != Const::Unknown {
                    if visited.contains(&val) {
                        self.report(
                            kv.key.start_pos(),
                            DiagCode::ChkDuplicateLitIndex,
                            format!("duplicate key {} in map literal", const_text(&val)),
                        );
                        continue;
                    }
                    visited.push(val);
                }
            }
            let mut v = self.expr_hint(file, &kv.value, Some(elem));
            self.assignment(&mut v, kv.value.start_pos(), Some(elem), "map literal");
        }
    }

    /// 官方 `fieldPath(utyp, index)`：`S.a` 形态（只进人读消息；`C8` 不比文本）。
    fn field_path_text(&self, fields: &[rgoc_types::Field], path: &[usize]) -> String {
        let tt = self.tt();
        let mut names: Vec<String> = Vec::new();
        let mut cur: &[rgoc_types::Field] = fields;
        for &i in path {
            let Some(f) = cur.get(i) else { break };
            let base = match tt.type_of(unalias(tt, f.typ)) {
                Type::Pointer(p) => *p,
                _ => f.typ,
            };
            let name = f
                .name
                .clone()
                .unwrap_or_else(|| match tt.type_of(unalias(tt, base)) {
                    Type::Named(id) => tt.named_def(*id).name.clone(),
                    _ => "?".to_string(),
                });
            names.push(name);
            cur = match tt.type_of(underlying(tt, base)) {
                Type::Struct(id) => &tt.struct_def(*id).fields,
                _ => &[],
            };
        }
        names.join(".")
    }

    // ────────────────────────────── 类型断言 ──────────────────────────────

    /// 官方 `rawExpr` 的 `*ast.TypeAssertExpr` 支（`expr.go:1091`）。
    ///
    /// 三步：操作数必须是接口（否则 `invalid operation: … is not an interface`）
    /// → 目标类型可解析 → `assertableTo`（目标是接口时**不做**静态判定；
    /// 否则要求目标类型实现源接口）。结果模式 `commaok`（官方同形）。
    ///
    /// 窄面（台账登记）：类型参数操作数、`x.(type)` 在 type switch 之外、
    /// 类型表达式面未接的形态 —— 一律 `invalid` 且**不报**。
    fn type_assert(&mut self, file: FileId, e: &AssertExpr) -> Operand {
        let x = self.expr(file, &e.x);
        if x.mode == Mode::Invalid {
            return Operand::invalid();
        }
        let Some(te) = &e.type_ else {
            // `x.(type)` 只允许出现在 type switch 守卫里（守卫面归 Task 25）。
            return Operand::invalid();
        };
        let Some(xt) = x.typ else {
            return Operand::invalid();
        };
        // 类型参数操作数：官方 `cannot use type assertion on type parameter value`
        // —— `D-M3-2` 窄面（表示有位置、判定不做），不报。
        {
            let tt = self.tt();
            if matches!(tt.type_of(unalias(tt, xt)), Type::TypeParam(_)) {
                return Operand::invalid();
            }
        }
        let is_iface = {
            let tt = self.tt();
            matches!(tt.type_of(underlying(tt, xt)), Type::Interface(_))
        };
        if !is_iface {
            let what = operand_text(self.tt(), &x);
            self.report(
                e.x.start_pos(),
                DiagCode::ChkInvalidOperation,
                format!("invalid operation: {what} is not an interface"),
            );
            return Operand::invalid();
        }
        let Some(target) = self.type_expr(file, te) else {
            return Operand::invalid();
        };
        let target_is_iface = {
            let tt = self.tt();
            matches!(tt.type_of(underlying(tt, target)), Type::Interface(_))
        };
        if !target_is_iface {
            let ok = {
                let tt = self.tt();
                implements(tt, target, xt, false).unwrap_or(true)
            };
            if !ok {
                let (t_txt, v_txt) = {
                    let tt = self.tt();
                    (type_str(tt, target), type_str(tt, xt))
                };
                self.report(
                    e.x.start_pos(),
                    DiagCode::ChkImpossibleAssert,
                    format!("impossible type assertion: {t_txt} does not implement {v_txt}"),
                );
                return Operand::invalid();
            }
        }
        Operand::comma_ok(target)
    }

    // ────────────────────────────── 选择器 ──────────────────────────────
    /// 官方 `selector`（`call.go:686`）的窄面。
    ///
    /// 查找在 [`lookup_field_or_method`]（官方 `lookup.go`），方法值 /
    /// 方法表达式的签名构造在 [`crate::selection`]。`pkg.Name`（`PkgName`
    /// 操作数建不出来 ⇒ `invalid`）与泛型接收者面 ⇒ 不报。
    fn selector(&mut self, file: FileId, e: &SelectorExpr) -> Operand {
        // `unsafe.Pointer` 这种**类型名**用法（M4/Task 31）。
        if let Expr::Name(n) = &*e.x
            && n.value == crate::unsafe_pkg::PKG_NAME
            && crate::unsafe_pkg::imports_unsafe(self.s.pkg, file)
        {
            return crate::unsafe_pkg::select(self, e);
        }
        // `pkg.X`（M4/Task 35）：包名不是操作数模式，只能在这里被消费
        // （官方 `call.go:698-700` 同一形状）。
        if let Some(op) = crate::imports::select(self, file, e) {
            return op;
        }
        let mut x = self.expr(file, &e.x);
        let sel = e.sel.value.as_str();
        if x.mode == Mode::Invalid {
            return Operand::invalid();
        }
        if x.mode == Mode::Builtin {
            // 官方 `UncalledBuiltin`（内建不能进选择器）；报在 `.` 后的名字位。
            self.report(
                e.sel.pos,
                DiagCode::ChkInvalidSelector,
                "invalid use of builtin in selector expression".to_string(),
            );
            return Operand::invalid();
        }
        let Some(xt) = x.typ else {
            return Operand::invalid();
        };
        let addressable = x.mode == Mode::Variable;
        let r = lookup_field_or_method(self.tt(), xt, addressable, sel);
        let Some(found) = r.obj else {
            if r.index.is_some() {
                let label = expr_label(&e.x).unwrap_or_else(|| operand_text(self.tt(), &x));
                self.report(
                    e.sel.pos,
                    DiagCode::ChkInvalidSelector,
                    format!("ambiguous selector {label}.{sel}"),
                );
                return Operand::invalid();
            }
            if r.indirect {
                let t = self.type_text(xt);
                let msg = if x.mode == Mode::TypeExpr {
                    format!(
                        "invalid method expression {t}.{sel} (needs pointer receiver (*{t}).{sel})"
                    )
                } else {
                    format!("cannot call pointer method {sel} on {t}")
                };
                self.report(e.sel.pos, DiagCode::ChkInvalidSelector, msg);
                return Operand::invalid();
            }
            let label = expr_label(&e.x).unwrap_or_else(|| operand_text(self.tt(), &x));
            let t = self.type_text(xt);
            self.report(
                e.sel.pos,
                DiagCode::ChkInvalidSelector,
                format!("{label}.{sel} undefined (type {t} has no field or method {sel})"),
            );
            return Operand::invalid();
        };
        // 类别按官方两处分支裁决：typexpr 面的方法 ⇒ 方法表达式。
        let kind = match &found {
            Found::Field { .. } => SelKind::FieldVal,
            Found::Method { .. } if x.mode == Mode::TypeExpr => SelKind::MethodExpr,
            Found::Method { .. } => SelKind::MethodVal,
        };
        match (kind, found) {
            (SelKind::FieldVal, Found::Field { typ }) => {
                if x.mode == Mode::TypeExpr {
                    // 官方 `NonFieldSelector`：报在 `X` 位（不是 `.` 后的名字位）。
                    let t = self.type_text(xt);
                    self.report(
                        e.x.pos(),
                        DiagCode::ChkInvalidSelector,
                        format!("operand for field selector {sel} must be value of type {t}"),
                    );
                    return Operand::invalid();
                }
                // 字段值：可寻址变量或路径带指针 ⇒ variable，否则 value。
                x.mode = if x.mode == Mode::Variable || r.indirect {
                    Mode::Variable
                } else {
                    Mode::Value
                };
                x.typ = Some(typ);
                x.val = None;
                x
            }
            (SelKind::MethodVal | SelKind::MethodExpr, Found::Method { sig }) => {
                let new_sig = if kind == SelKind::MethodExpr {
                    method_expr_sig(self.s.tt, xt, sig)
                } else {
                    method_value_sig(self.s.tt, sig)
                };
                Operand::value(Some(new_sig))
            }
            _ => unreachable!("kind 由 found 裁决（构造保证同源）"),
        }
    }

    // ────────────────────────────── 一元 ──────────────────────────────

    /// 官方 `unary`（`expr.go:129`）。
    fn unary(&mut self, file: FileId, e: &Operation) -> Operand {
        let x = self.expr(file, &e.x);
        if !is_valid_op(&x) {
            return Operand::invalid();
        }
        let Some(t) = x.typ else {
            return Operand::invalid();
        };
        let xpos = e.x.pos();
        match e.op {
            Op::Add | Op::Sub | Op::Not | Op::Xor => {
                let ok = {
                    let tt = self.tt();
                    match e.op {
                        Op::Add | Op::Sub => all_numeric(tt, t),
                        Op::Not => all_boolean(tt, t),
                        Op::Xor => all_integer(tt, t),
                        _ => unreachable!("外层已过滤"),
                    }
                };
                if !ok {
                    return self.op_not_defined(e.op, xpos, t);
                }
                self.finish_unary(e, x)
            }
            Op::Mul => self.deref(x, xpos),
            Op::And => self.addr(x, xpos, is_composite_lit(&e.x)),
            Op::Arrow => self.recv(x, xpos),
            _ => Operand::invalid(),
        }
    }

    /// 一元运算的常量尾（官方 `unary` 的折叠段 + `overflow`）。
    fn finish_unary(&mut self, e: &Operation, x: Operand) -> Operand {
        let Some(t) = x.typ else {
            return Operand::invalid();
        };
        if x.mode != Mode::Constant {
            // 官方：`x.mode_ = value`，类型不变。
            return Operand::value(Some(t));
        }
        let val_in = x.val.unwrap_or(Const::Unknown);
        if val_in == Const::Unknown {
            // 官方 `overflow` 首行：输入没有值 ⇒ 直接返回（不报）。
            return Operand::constant(t, Const::Unknown);
        }
        let typed = {
            let tt = self.tt();
            if is_untyped(tt, t) {
                None
            } else {
                basic_kind(tt, t)
            }
        };
        let val = fold_unary_typed(e.op, &val_in, typed, &Sizes::HOST_ARM64);
        let mut out = Operand::constant(t, val);
        // 一元：`x.expr = e` ⇒ 溢出报告的 err 位 = 一元表达式的 `Pos()`（操作符位）。
        self.overflow_check(&mut out, Some(e.op), true, e.pos, e.pos);
        out
    }

    // ──────────────────────── `*` / `&` / `<-` ────────────────────────

    /// 官方 StarExpr 的**求值**支（`expr.go:1126`）：`*p` ⇒ variable。
    fn deref(&mut self, x: Operand, xpos: Pos) -> Operand {
        let Some(t) = x.typ else {
            return Operand::invalid();
        };
        // `*T` 类型面（TypeExpr）：官方 `*ast.StarExpr` 的 typexpr 臂 ——
        // 模式保持 `typexpr`，类型换成**指针类型**（`(*T).M` 方法表达式由此可达）。
        if matches!(x.mode, Mode::TypeExpr) {
            let p = self.s.tt.add_pointer(t);
            return Operand::type_expr(p);
        }
        let base = {
            let tt = self.tt();
            match tt.type_of(underlying(tt, t)) {
                Type::Pointer(e) => Some(*e),
                _ => None,
            }
        };
        match base {
            Some(e) => Operand::variable(Some(e)),
            None => {
                let desc = self.type_text(t);
                self.report(
                    xpos,
                    DiagCode::ChkInvalidOperation,
                    format!("invalid operation: cannot indirect {desc}"),
                );
                Operand::invalid()
            }
        }
    }

    /// 官方 `&`（`expr.go:137`）：`variable`，**外加复合字面量**。
    ///
    /// 规范原文（`go_spec.html` §Address operators）：「As an exception to the
    /// addressability requirement, `x` may also be a (possibly parenthesized)
    /// composite literal.」——所以 `&T{…}` 合法，`&f()` 不合法。
    ///
    /// M5/Task 4 补：原先只放行 `Mode::Variable`，`&T{1}` 会被报成
    /// `cannot take address of T`（`compos.go:16`、`ddd.go:189/222`、
    /// `method7.go:62` 的形态）。
    fn addr(&mut self, x: Operand, xpos: Pos, composite: bool) -> Operand {
        let Some(t) = x.typ else {
            return Operand::invalid();
        };
        if x.mode != Mode::Variable && !composite {
            let desc = self.type_text(t);
            self.report(
                xpos,
                DiagCode::ChkInvalidOperation,
                format!("invalid operation: cannot take address of {desc}"),
            );
            return Operand::invalid();
        }
        let p = self.s.tt.add_pointer(t);
        Operand::value(Some(p))
    }

    /// 官方 `chanElem` 的接收支：`<-ch` ⇒ 元素类型值。
    fn recv(&mut self, x: Operand, xpos: Pos) -> Operand {
        let Some(t) = x.typ else {
            return Operand::invalid();
        };
        let elem = {
            let tt = self.tt();
            match tt.type_of(underlying(tt, t)) {
                Type::Chan { dir, elem } if *dir != ChanDir::SendOnly => Some(*elem),
                _ => None,
            }
        };
        match elem {
            Some(e) => Operand::value(Some(e)),
            None => {
                let (kind, desc) = {
                    let tt = self.tt();
                    let is_chan = matches!(tt.type_of(underlying(tt, t)), Type::Chan { .. });
                    let kind = if is_chan {
                        "send-only channel"
                    } else {
                        "non-channel"
                    };
                    (kind, type_str(tt, t))
                };
                self.report(
                    xpos,
                    DiagCode::ChkInvalidOperation,
                    format!("invalid operation: cannot receive from {kind} {desc}"),
                );
                Operand::invalid()
            }
        }
    }

    // ────────────────────────────── 二元 ──────────────────────────────

    /// 官方 `binary`（`expr.go:780`）。
    fn binary(&mut self, file: FileId, e: &Operation) -> Operand {
        let x = self.expr(file, &e.x);
        let Some(rhs) = e.y.as_deref() else {
            return Operand::invalid();
        };
        let y = self.expr(file, rhs);
        let (x, y) = (
            At {
                op: x,
                pos: e.x.pos(),
            },
            At {
                op: y,
                pos: rhs.pos(),
            },
        );
        self.binary_core(x, y, e.op, BinSrc::Expr, e.pos)
    }

    /// 官方 `binary` 的**核心**：两侧操作数已求值。
    ///
    /// `src` 区分两种调用形态（官方靠 `e == nil` 区分）：
    ///
    /// * `BinSrc::Expr` —— 真二元表达式 `x op y`；
    /// * `BinSrc::Assign` —— 复合赋值 `x op= y` 的**隐式**二元运算
    ///   （官方 `stmt.go:524` 的 `check.binary(&x, nil, lhs, rhs, op, s.TokPos)`）。
    ///   此时不匹配消息走 `expr.go:821` 的 `lhs op= rhs` 形态。
    ///
    /// 这条路径是 M5/Task 5 引入的：`stmt.rs` 原先**自己**实现了一份「两侧类型
    /// 匹配 + 谓词」的近似，把 `i <<= c`（`c` 为 `uint`）判成
    /// `mismatched types int and uint` —— 而官方对移位**只要求计数是整数**，
    /// 不要求与左操作数同类型（`F3-D`）。近似实现与官方分歧的第二处是
    /// `i += 1.5`：官方报 `convert` @ 右操作数（`convertUntyped` 截断），
    /// 近似报 `op-invalid` @ 左操作数。改走 `binary` 后两处自动对齐。
    pub(crate) fn binary_core(&mut self, x: At, y: At, op: Op, src: BinSrc, epos: Pos) -> Operand {
        let (xpos, ypos) = (x.pos, y.pos);
        let (x, y) = (x.op, y.op);
        // 官方：x 无效 ⇒ 原样返回；y 无效 ⇒ x 置 invalid。
        if !is_valid_op(&x) || !is_valid_op(&y) {
            return Operand::invalid();
        }
        if matches!(op, Op::Shl | Op::Shr) {
            return self.shift(At { op: x, pos: xpos }, At { op: y, pos: ypos }, op, epos);
        }
        let (mut x, mut y) = (x, y);
        self.match_types(&mut x, &mut y, xpos, ypos);
        if !is_valid_op(&x) {
            return Operand::invalid();
        }
        if is_comparison(op) {
            return self.comparison(&x, &y, op, xpos, ypos);
        }
        let (Some(tx), Some(ty)) = (x.typ, y.typ) else {
            return Operand::invalid();
        };
        let same = {
            let tt = self.tt();
            identical(tt, tx, ty).unwrap_or(false)
        };
        if !same {
            let (a, b) = {
                let tt = self.tt();
                (type_str(tt, tx), type_str(tt, ty))
            };
            let msg = match &src {
                BinSrc::Expr => format!("invalid operation: mismatched types {a} and {b}"),
                BinSrc::Assign { lhs, rhs, sym } => {
                    format!("invalid operation: {lhs} {sym}= {rhs} (mismatched types {a} and {b})")
                }
            };
            self.report(xpos, DiagCode::ChkInvalidOperation, msg);
            return Operand::invalid();
        }
        // 官方 `binaryOpPredicates`（`expr.go:96`）。
        let ok = {
            let tt = self.tt();
            match op {
                Op::Add => all_numeric(tt, tx) || all_string(tt, tx),
                Op::Sub | Op::Mul | Op::Quo => all_numeric(tt, tx),
                // 官方 `binaryOpPredicates`：`REM` 只对整数定义（`f % 2` 是 op-invalid）。
                Op::Rem => all_integer(tt, tx),
                Op::And | Op::Or | Op::Xor | Op::AndNot => all_integer(tt, tx),
                Op::LAnd | Op::LOr => all_boolean(tt, tx),
                _ => false,
            }
        };
        if !ok {
            return self.op_not_defined(op, xpos, tx);
        }
        // 除零（官方 `binary` 的 QUO/REM 守卫；条件含 `allInteger(x)`。
        // 复合除法下溢面暂缓 —— 台账登记）。
        if matches!(op, Op::Quo | Op::Rem) && self.divisor_zero(&x, &y) {
            self.report(
                ypos,
                DiagCode::TypDivByZero,
                "invalid operation: division by zero".to_string(),
            );
            return Operand::invalid();
        }
        // 常量折叠 + 溢出（官方 `overflow`）。
        if x.mode == Mode::Constant && y.mode == Mode::Constant {
            let xv = x.val.unwrap_or(Const::Unknown);
            let yv = y.val.unwrap_or(Const::Unknown);
            if xv == Const::Unknown || yv == Const::Unknown {
                return Operand::constant(tx, Const::Unknown);
            }
            let val = fold_binary(op, &xv, &yv);
            let mut out = Operand::constant(tx, val);
            self.overflow_check(&mut out, Some(op), false, epos, xpos);
            return out;
        }
        // 官方：`x.mode_ = value`，类型不变。
        Operand::value(Some(tx))
    }

    /// 除零判据（官方 `(x.mode == constant_ || allInteger(x.typ)) && y.mode == constant_ && Sign(y.val) == 0`）。
    fn divisor_zero(&self, x: &Operand, y: &Operand) -> bool {
        if y.mode != Mode::Constant {
            return false;
        }
        let Some(yv) = y.val.as_ref() else {
            return false;
        };
        if !const_sign_zero(yv) {
            return false;
        }
        if x.mode == Mode::Constant {
            return true;
        }
        let Some(tx) = x.typ else { return false };
        all_integer(self.tt(), tx)
    }

    // ────────────────────────────── 移位 ──────────────────────────────

    /// 官方 `shift`（`expr.go:618`）。
    ///
    /// 官方签名带 `e ast.Expr` / `op token.Token`，但函数体只用 `e` 的**位置**
    /// （`x`/`y` 的报点 + 折叠溢出的报点）与 `op`（折叠）。这里直接收
    /// `xpos` / `ypos` / `epos`（`epos` = **运算符**位置，溢出报在它上面），
    /// 于是同一条路径同时服务真二元表达式与复合赋值（`i <<= c`，官方 `e == nil`）。
    fn shift(&mut self, x: At, y: At, op: Op, epos: Pos) -> Operand {
        let (xpos, ypos) = (x.pos, y.pos);
        let (x, y) = (x.op, y.op);
        let Some(xt) = x.typ else {
            return Operand::invalid();
        };
        let Some(yt) = y.typ else {
            return Operand::invalid();
        };

        // ① 左操作数：整数类型，或无类型且值为整数（官方 `allInteger || ToInt(val).Kind()==Int`）。
        let x_ok = {
            let tt = self.tt();
            if all_integer(tt, xt) {
                true
            } else if is_untyped(tt, xt) {
                x.mode == Mode::Constant && x.val.as_ref().is_some_and(|v| to_int(v).is_some())
            } else {
                false
            }
        };
        if !x_ok {
            let desc = self.type_text(xt);
            self.report(
                xpos,
                DiagCode::TypInvalidShiftOperand,
                format!("invalid operation: shifted operand {desc} must be integer"),
            );
            return Operand::invalid();
        }

        // ② 计数检查（官方对 const 计数的三段：负 / 无类型可表示性；非 const：整数）。
        let mut yval: Option<Const> = None;
        if y.mode == Mode::Constant {
            let yv = y.val.clone().unwrap_or(Const::Unknown);
            yval = to_int(&yv);
            if let Some(Const::Int(n)) = &yval
                && n.is_neg()
            {
                self.report(
                    ypos,
                    DiagCode::TypInvalidShiftCount,
                    format!(
                        "invalid operation: negative shift count {}",
                        const_text(&yv)
                    ),
                );
                return Operand::invalid();
            }
            let y_untyped = is_untyped(self.tt(), yt);
            if y_untyped && !self.convert_count_to_uint(&yv, ypos) {
                return Operand::invalid();
            }
        } else {
            let y_int = {
                let tt = self.tt();
                all_integer(tt, yt)
            };
            if !y_int {
                // 无符号面（untyped / 常量）在上；这里只剩已声明类型的非常量计数。
                // 官方对非整数计数（`1 << f`）：InvalidShiftCount @计数位。
                let desc = self.type_text(yt);
                self.report(
                    ypos,
                    DiagCode::TypInvalidShiftCount,
                    format!("invalid operation: shift count {desc} must be integer"),
                );
                return Operand::invalid();
            }
            // 已声明**有符号**整数计数的 go1.13 版本门（官方 `verifyVersionf`）
            // 暂缓 —— 台账登记（Task 26/29/37）。
        }

        // ③ 双侧恒定：边界 + 折叠 + 溢出。
        if x.mode == Mode::Constant && y.mode == Mode::Constant {
            let xv = x.val.unwrap_or(Const::Unknown);
            let yv = y.val.clone().unwrap_or(Const::Unknown);
            if xv == Const::Unknown || yv == Const::Unknown {
                let t = self.integer_result_type(xt);
                return Operand::constant(t, Const::Unknown);
            }
            // 官方 `Uint64Val(yval)` 失败或 `s > shiftBound` ⇒ InvalidShiftCount @计数位。
            let s = match &yval {
                Some(Const::Int(n)) => n.to_i64().filter(|s| (0..=1074).contains(s)),
                _ => None,
            };
            let Some(_s) = s else {
                self.report(
                    ypos,
                    DiagCode::TypInvalidShiftCount,
                    format!("invalid operation: invalid shift count {}", const_text(&yv)),
                );
                return Operand::invalid();
            };
            // 官方：非常量整值左操作数（`2.0`）的结果类型修正为 `untyped int`。
            let t = self.integer_result_type(xt);
            let val = fold_binary(op, &xv, &yv);
            let mut out = Operand::constant(t, val);
            self.overflow_check(&mut out, Some(op), false, epos, xpos);
            return out;
        }

        // ④ 非双侧恒定（含 `a << 1075`：只有两侧都恒时才查 1074 上界）。
        let t = if x.mode == Mode::Constant {
            // 常量 lhs + 非常量计数：官方「延迟检查」保留 lhs 类型。
            self.integer_result_type(xt)
        } else {
            xt
        };
        Operand::value(Some(t))
    }

    /// 官方：`!isInteger(x.typ)` ⇒ 结果类型 `untyped int`（如 `2.0 << 1`）。
    fn integer_result_type(&mut self, t: TypeId) -> TypeId {
        let is_int = {
            let tt = self.tt();
            all_integer(tt, t)
        };
        if is_int {
            t
        } else {
            self.s.tt.add_basic(BasicId::UntypedInt)
        }
    }

    /// 无类型计数向 `uint` 的可表示性（官方 `representable(y, Typ[Uint])`）。
    ///
    /// 失败消息/码照 `invalidConversion`：截断 → `TYP-INVALID-CONVERSION`
    /// （「truncated to uint」，夹具 `1 << 1.5` = `61:14` convert）。
    fn convert_count_to_uint(&mut self, yv: &Const, ypos: Pos) -> bool {
        match representable(yv, BasicId::Uint, &Sizes::HOST_ARM64) {
            Ok(_) => true,
            Err(code) => {
                let msg = conversion_failure_message(code, &const_text(yv), "uint");
                self.report(ypos, msg.0, msg.1);
                false
            }
        }
    }

    // ────────────────────────────── 比较 ──────────────────────────────

    /// 官方 `comparison`（`expr.go:513`）。
    fn comparison(&mut self, x: &Operand, y: &Operand, op: Op, xpos: Pos, ypos: Pos) -> Operand {
        let (Some(tx), Some(ty)) = (x.typ, y.typ) else {
            return Operand::invalid();
        };
        // ① 双向可赋值（官方 spec 引文）。`Err`（类型参数窄面）按**宽容**：
        //    不据此报 mismatch（假阳性优先于负向缺口；模块头已登记）。
        let ok = {
            let tt = self.tt();
            assignable(tt, tx, ty).unwrap_or(true) || assignable(tt, ty, tx).unwrap_or(true)
        };
        if !ok {
            let (a, b) = {
                let tt = self.tt();
                (type_str(tt, tx), type_str(tt, ty))
            };
            self.report(
                ypos,
                DiagCode::ChkInvalidOperation,
                format!("invalid operation: mismatched types {a} and {b}"),
            );
            return Operand::invalid();
        }
        // ② 运算符定义面。
        match op {
            Op::Eql | Op::Neq => {
                if x.mode == Mode::NilValue || y.mode == Mode::NilValue {
                    // 只有 `nil == nil` 会走到这（另一侧可 nil 时已通过转换接线）。
                    let other = if x.mode == Mode::NilValue { ty } else { tx };
                    let has = {
                        let tt = self.tt();
                        has_nil(tt, other)
                    };
                    if !has {
                        let what = self.what_or_type(ty);
                        self.report(
                            ypos,
                            DiagCode::ChkInvalidOperation,
                            format!(
                                "invalid operation: operator {} not defined on {what}",
                                op_str(op)
                            ),
                        );
                        return Operand::invalid();
                    }
                } else {
                    for (t, pos) in [(tx, xpos), (ty, ypos)] {
                        let c = {
                            let tt = self.tt();
                            comparable(tt, t).unwrap_or(true)
                        };
                        if !c {
                            let cause = {
                                let tt = self.tt();
                                incomparable_cause(tt, t)
                            };
                            self.report(
                                pos,
                                DiagCode::ChkInvalidOperation,
                                format!("invalid operation: {cause}"),
                            );
                            return Operand::invalid();
                        }
                    }
                }
            }
            Op::Lss | Op::Leq | Op::Gtr | Op::Geq => {
                for (t, pos) in [(tx, xpos), (ty, ypos)] {
                    let ordered = {
                        let tt = self.tt();
                        all_ordered(tt, t)
                    };
                    if !ordered {
                        let what = self.what_or_type(t);
                        self.report(
                            pos,
                            DiagCode::ChkInvalidOperation,
                            format!(
                                "invalid operation: operator {} not defined on {what}",
                                op_str(op)
                            ),
                        );
                        return Operand::invalid();
                    }
                }
            }
            _ => return Operand::invalid(),
        }
        // ③ 结果：`untyped bool`；双恒折叠（Unknown 保持 Unknown，不报）。
        let bt = self.s.tt.add_basic(BasicId::UntypedBool);
        if x.mode == Mode::Constant && y.mode == Mode::Constant {
            let xv = x.val.clone().unwrap_or(Const::Unknown);
            let yv = y.val.clone().unwrap_or(Const::Unknown);
            let val = if xv == Const::Unknown || yv == Const::Unknown {
                Const::Unknown
            } else {
                fold_compare(op, &xv, &yv)
            };
            return Operand::constant(bt, val);
        }
        Operand::value(Some(bt))
    }

    fn what_or_type(&self, t: TypeId) -> String {
        what_or_type(self.tt(), t)
    }

    // ────────────────────── 类型匹配与隐式转换 ──────────────────────

    /// 官方 `matchTypes`（`expr.go:876`）：`mayConvert` 门槛 + 双向无类型转换。
    fn match_types(&mut self, x: &mut Operand, y: &mut Operand, xpos: Pos, ypos: Pos) {
        let (Some(tx), Some(ty)) = (x.typ, y.typ) else {
            return;
        };
        let may = {
            let tt = self.tt();
            may_convert(tt, x, y, tx, ty)
        };
        if !may {
            return;
        }
        // 官方顺序：先 x（目标 = **当前** y.typ），后 y（目标 = **新** x.typ）。
        self.convert_untyped(x, ty, xpos);
        if !is_valid_op(x) {
            return;
        }
        let Some(tx2) = x.typ else { return };
        self.convert_untyped(y, tx2, ypos);
        if !is_valid_op(y) {
            // 官方：y 转换失败 ⇒ x.invalidate()。
            *x = Operand::invalid();
        }
    }

    /// 官方 `convertUntyped`（`const.go:301`）：无类型操作数就地隐式转换。
    ///
    /// 失败 ⇒ 上报（错误位 = **操作数自身**）并置 invalid。
    pub(crate) fn convert_untyped(&mut self, x: &mut Operand, target: TypeId, pos: Pos) {
        let Some(t) = x.typ else { return };
        {
            let tt = self.tt();
            // 非无类型 ⇒ noop（官方 `implicitTypeAndValue` 首行）。
            if !is_untyped(tt, t) {
                return;
            }
            // 无效目标 ⇒ noop。
            if matches!(tt.type_of(unalias(tt, target)), Type::Invalid) {
                return;
            }
        }
        let kind_x = basic_kind(self.tt(), t).expect("untyped 操作数底层必为 Basic");
        let is_nil = x.mode == Mode::NilValue;

        // ── 决策（纯读；借用在块内结束）──
        enum Plan {
            /// 无变化（nil 进可 nil 目标等）。
            Keep,
            /// 只改种类（无类型 ↔ 无类型提升；空接口 ⇒ 默认型）。
            Kind(BasicId),
            /// 设类型（+ 可能换值表示）。
            Set(TypeId, Option<Const>),
            /// 失败（按码渲染）。
            Fail(DiagCode),
        }
        let plan = {
            let tt = self.tt();
            let target_kind = basic_kind(tt, target);
            if target_kind.is_some_and(BasicId::is_untyped) {
                // ① 双无类型：官方 `maxType`。
                match max_type(kind_x, target_kind.expect("已判 Some")) {
                    Some(m) => Plan::Kind(m),
                    None => Plan::Fail(DiagCode::TypInvalidConversion),
                }
            } else {
                let u = underlying(tt, target);
                match tt.type_of(u) {
                    Type::Basic(b) => {
                        if x.mode == Mode::Constant {
                            let val = x.val.clone().unwrap_or(Const::Unknown);
                            match representable(&val, *b, &Sizes::HOST_ARM64) {
                                Ok(rep) => Plan::Set(target, Some(rep.value)),
                                Err(code) => Plan::Fail(code),
                            }
                        } else {
                            // 非恒无类型（比较结果 / nil 等）：只查种类兼容。
                            let ok = match kind_x {
                                BasicId::UntypedBool => b.is_bool(),
                                BasicId::UntypedInt
                                | BasicId::UntypedRune
                                | BasicId::UntypedFloat
                                | BasicId::UntypedComplex => b.is_numeric(),
                                BasicId::UntypedString => b.is_string(),
                                BasicId::UntypedNil => *b == BasicId::UnsafePointer,
                                _ => false,
                            };
                            if ok {
                                Plan::Set(target, None)
                            } else {
                                Plan::Fail(DiagCode::TypInvalidConversion)
                            }
                        }
                    }
                    Type::Interface(i) => {
                        if is_nil {
                            // nil 进任何接口都真（官方 `x.isNil()` 面）。
                            Plan::Keep
                        } else if tt.interface_def(*i).methods.is_empty()
                            && tt.interface_def(*i).tset.is_all()
                        {
                            // 非 nil 无类型值只能进**空**接口 ⇒ 默认型（官方 `Default`）。
                            Plan::Kind(default_type_of(kind_x))
                        } else {
                            Plan::Fail(DiagCode::TypInvalidConversion)
                        }
                    }
                    Type::Pointer(_)
                    | Type::Signature(_)
                    | Type::Slice(_)
                    | Type::Map { .. }
                    | Type::Chan { .. }
                        if is_nil =>
                    {
                        Plan::Keep
                    }
                    _ => Plan::Fail(DiagCode::TypInvalidConversion),
                }
            }
        };

        // ── 应用（可能上报）──
        match plan {
            Plan::Keep => {}
            Plan::Kind(m) => {
                let id = self.s.tt.add_basic(m);
                x.typ = Some(id);
            }
            Plan::Set(t2, val) => {
                x.typ = Some(t2);
                if let Some(v) = val {
                    x.val = Some(v);
                }
            }
            Plan::Fail(code) => {
                let v = x
                    .val
                    .as_ref()
                    .map_or_else(|| "value".to_string(), const_text);
                let to = {
                    let tt = self.tt();
                    type_str(tt, underlying(tt, target))
                };
                let msg = conversion_failure_message(code, &v, &to);
                self.report(pos, msg.0, msg.1);
                *x = Operand::invalid();
            }
        }
    }

    // ────────────────────────── 常量溢出守卫 ──────────────────────────

    /// 官方 `overflow`（`const.go:44`）：
    ///
    /// 1. 值 `Unknown` ⇒ 返回（**不报**：模块头登记的理由）；
    /// 2. 有类型常量 ⇒ `representable`：失败报在 `err_pos`（操作数位 / 表达式起点）
    ///    并把操作数置 invalid；
    /// 3. 无类型整数增长（`BitLen > 512`）⇒ 报在 `op_pos`（**操作符位**），
    ///    值改为 Unknown，**模式仍为 constant**（官方语义，继续传播）。
    #[allow(clippy::too_many_arguments)]
    fn overflow_check(
        &mut self,
        x: &mut Operand,
        op: Option<Op>,
        unary: bool,
        op_pos: Pos,
        err_pos: Pos,
    ) {
        let Some(t) = x.typ else { return };
        let val = x.val.clone().unwrap_or(Const::Unknown);
        if val == Const::Unknown {
            return;
        }
        let untyped = {
            let tt = self.tt();
            is_untyped(tt, t)
        };
        if untyped {
            if let Const::Int(n) = &val
                && n.bit_len() > 512
            {
                let name = overflow_op_name(op, unary);
                let message = if name.is_empty() {
                    "constant overflow".to_string()
                } else {
                    format!("constant {name} overflow")
                };
                // 移位语境的增长归 shift 类（夹具 `57:11`），其余归 overflow 类。
                let code = if op.is_some_and(|o| matches!(o, Op::Shl | Op::Shr)) {
                    DiagCode::TypInvalidShiftCount
                } else {
                    DiagCode::TypInvalidConstVal
                };
                self.report(op_pos, code, message);
                x.val = Some(Const::Unknown);
            }
            return;
        }
        let Some(b) = basic_kind(self.tt(), t) else {
            return;
        };
        match representable(&val, b, &Sizes::HOST_ARM64) {
            Ok(rep) => x.val = Some(rep.value),
            Err(code) => {
                let to = {
                    let tt = self.tt();
                    type_str(tt, t)
                };
                let msg = conversion_failure_message(code, &const_text(&val), &to);
                self.report(err_pos, msg.0, msg.1);
                *x = Operand::invalid();
            }
        }
    }

    /// 官方 `invalidConversion` 的消息 compose（官方 `Code` → 本仓码 + 消息）。
    ///
    /// - `TruncatedFloat` → 「truncated to」（转换语境错 `TYP-INVALID-CONVERSION`）；
    /// - `NumericOverflow` → 「overflows」（错 `TYP-NUMERIC-OVERFLOW`）；
    /// - 其余 → 「cannot convert」（错 `TYP-INVALID-CONVERSION`）。
    fn op_not_defined(&mut self, op: Op, pos: Pos, t: TypeId) -> Operand {
        let what = self.what_or_type(t);
        self.report(
            pos,
            DiagCode::ChkInvalidOperation,
            format!(
                "invalid operation: operator {} not defined on {what}",
                op_str(op)
            ),
        );
        Operand::invalid()
    }
}

// ───────────────────────────── 纯函数面 ─────────────────────────────

/// 官方 `isValid`（模式面）。
fn is_valid_op(x: &Operand) -> bool {
    !matches!(x.mode, Mode::Invalid | Mode::NoValue)
}

/// 官方 `mayConvert`（`expr.go` `matchTypes` 内联；规则顺序逐条对齐）。
pub(crate) fn may_convert(
    tt: &TypeTable,
    x: &Operand,
    y: &Operand,
    tx: TypeId,
    ty: TypeId,
) -> bool {
    let x_typed = !is_untyped(tt, tx);
    let y_typed = !is_untyped(tt, ty);
    if x_typed && y_typed {
        return false;
    }
    if all_numeric(tt, tx) != all_numeric(tt, ty) {
        return false;
    }
    if is_non_typeparam_interface(tt, tx) || is_non_typeparam_interface(tt, ty) {
        return true;
    }
    if all_boolean(tt, tx) != all_boolean(tt, ty) {
        return false;
    }
    if all_string(tt, tx) != all_string(tt, ty) {
        return false;
    }
    if x.mode == Mode::NilValue {
        return has_nil(tt, ty);
    }
    if y.mode == Mode::NilValue {
        return has_nil(tt, tx);
    }
    if is_pointer(tt, tx) || is_pointer(tt, ty) {
        return false;
    }
    true
}

/// 官方 `maxType` 的**无类型 kind 面**：同 kind ⇒ 自身；数值按
/// `int < rune < float < complex` 提升；其余（bool / string / nil 混合）⇒ `None`。
pub(crate) fn max_type(a: BasicId, b: BasicId) -> Option<BasicId> {
    if a == b {
        return Some(a);
    }
    let rank = |k: BasicId| -> Option<u8> {
        match k {
            BasicId::UntypedInt => Some(0),
            BasicId::UntypedRune => Some(1),
            BasicId::UntypedFloat => Some(2),
            BasicId::UntypedComplex => Some(3),
            _ => None,
        }
    };
    match (rank(a), rank(b)) {
        (Some(x), Some(y)) => Some(if x >= y { a } else { b }),
        _ => None,
    }
}

/// 官方 `invalidConversion` 的成对结果：`(码, 消息)`。
fn conversion_failure_message(code: DiagCode, v: &str, to: &str) -> (DiagCode, String) {
    match code {
        DiagCode::TypNumericOverflow => (code, format!("constant {v} overflows {to}")),
        DiagCode::TypTruncatedFloat => (
            DiagCode::TypInvalidConversion,
            format!("constant {v} truncated to {to}"),
        ),
        _ => (
            DiagCode::TypInvalidConversion,
            format!("cannot convert constant {v} to {to}"),
        ),
    }
}

/// 官方 `op2str1` / `op2str2`（`expr.go` 的 `opName` 前置表）：只给会**增长**的运算。
fn overflow_op_name(op: Option<Op>, unary: bool) -> &'static str {
    let Some(op) = op else { return "" };
    if unary {
        match op {
            Op::Xor => "bitwise complement",
            _ => "",
        }
    } else {
        match op {
            Op::Add => "addition",
            Op::Sub => "subtraction",
            Op::Xor => "bitwise XOR",
            Op::Mul => "multiplication",
            Op::Shl => "shift",
            _ => "",
        }
    }
}

/// 官方 `isComparison`。
fn is_comparison(op: Op) -> bool {
    matches!(
        op,
        Op::Eql | Op::Neq | Op::Lss | Op::Leq | Op::Gtr | Op::Geq
    )
}

/// 运算符文本（`Op` 无 `as_str`；这里是**消息**用文本，官方按 `token.Token` 渲染）。
pub(crate) fn op_str(op: Op) -> &'static str {
    match op {
        Op::Add => "+",
        Op::Sub => "-",
        Op::Mul => "*",
        Op::Quo => "/",
        Op::Rem => "%",
        Op::And => "&",
        Op::Or => "|",
        Op::Xor => "^",
        Op::Shl => "<<",
        Op::Shr => ">>",
        Op::AndNot => "&^",
        Op::LAnd => "&&",
        Op::LOr => "||",
        Op::Not => "!",
        Op::Eql => "==",
        Op::Neq => "!=",
        Op::Lss => "<",
        Op::Leq => "<=",
        Op::Gtr => ">",
        Op::Geq => ">=",
        Op::Arrow => "<-",
        Op::Tilde => "~",
    }
}

/// 常量值文本（消息用；`Const` 没有 `Display`）。
pub(crate) fn const_text(c: &Const) -> String {
    match c {
        Const::Int(n) => n.to_string(),
        Const::Float(r) => r.to_string(),
        Const::Complex { re, im } => format!("({re} + {im}i)"),
        Const::Str(bytes) => format!("{:?}", String::from_utf8_lossy(bytes)),
        Const::Bool(v) => v.to_string(),
        Const::Unknown => "unknown".to_string(),
    }
}

/// 官方 `constant.ToInt`：`Int` 原样；整值 `Float` ⇒ `Int`；其余 ⇒ `None`。
fn to_int(c: &Const) -> Option<Const> {
    match c {
        Const::Int(n) => Some(Const::Int(n.clone())),
        Const::Float(r) => r.to_int().map(Const::Int),
        _ => None,
    }
}

/// 官方 `constant.Sign(y.val) == 0`：复数按 `Sign(re) | Sign(im)`。
fn const_sign_zero(c: &Const) -> bool {
    match c {
        Const::Int(n) => n.is_zero(),
        Const::Float(r) => r.is_zero(),
        Const::Complex { re, im } => re.is_zero() && im.is_zero(),
        _ => false,
    }
}

/// 官方 `hasNil`。
fn has_nil(tt: &TypeTable, t: TypeId) -> bool {
    match tt.type_of(underlying(tt, t)) {
        Type::Pointer(_)
        | Type::Slice(_)
        | Type::Map { .. }
        | Type::Chan { .. }
        | Type::Signature(_)
        | Type::Interface(_) => true,
        Type::Basic(b) => *b == BasicId::UnsafePointer,
        _ => false,
    }
}

/// 官方 `incomparableCause` 的切片/函数/map 支（`expr.go:513` 邻域）。
fn incomparable_cause(tt: &TypeTable, t: TypeId) -> String {
    match tt.type_of(underlying(tt, t)) {
        Type::Slice(_) => "slice can only be compared to nil".to_string(),
        Type::Signature(_) => "func can only be compared to nil".to_string(),
        Type::Map { .. } => "map can only be compared to nil".to_string(),
        _ => format!("{t} cannot be compared", t = type_str(tt, t)),
    }
}

/// 官方 `compositeKind`（`what` 的派生；`""` ⇒ 用类型名）。
fn composite_kind(tt: &TypeTable, t: TypeId) -> &'static str {
    match tt.type_of(underlying(tt, t)) {
        Type::Interface(_) => "interface",
        Type::Slice(_) => "slice",
        Type::Map { .. } => "map",
        Type::Chan { .. } => "chan",
        Type::Signature(_) => "func",
        Type::Struct(_) => "struct",
        _ => "",
    }
}

fn what_or_type(tt: &TypeTable, t: TypeId) -> String {
    let k = composite_kind(tt, t);
    if k.is_empty() {
        type_str(tt, t)
    } else {
        k.to_string()
    }
}

/// 类型文本（消息用）。具名类型用其名字；复合按官方构造子渲染；签名/结构体等
/// 只给类名（消息不复刻 —— `C8` 只比位置 + 类别）。
pub(crate) fn type_str(tt: &TypeTable, t: TypeId) -> String {
    let t = unalias(tt, t);
    match tt.type_of(t) {
        Type::Basic(b) => b.as_str().to_string(),
        Type::Named(n) => tt.named_def(*n).name.clone(),
        Type::Alias(_) => "alias".to_string(),
        Type::Pointer(e) => format!("*{}", type_str(tt, *e)),
        Type::Slice(e) => format!("[]{}", type_str(tt, *e)),
        Type::Array { elem, .. } => format!("[...]{}", type_str(tt, *elem)),
        Type::Map { key, elem } => {
            format!("map[{}]{}", type_str(tt, *key), type_str(tt, *elem))
        }
        Type::Chan { dir, elem } => match dir {
            ChanDir::SendRecv => format!("chan {}", type_str(tt, *elem)),
            ChanDir::SendOnly => format!("chan<- {}", type_str(tt, *elem)),
            ChanDir::RecvOnly => format!("<-chan {}", type_str(tt, *elem)),
        },
        Type::Signature(_) => "func".to_string(),
        Type::Struct(_) => "struct".to_string(),
        Type::Interface(_) => "interface".to_string(),
        Type::TypeParam(_) => "type parameter".to_string(),
        Type::Tuple(_) => "tuple".to_string(),
        Type::Union(_) => "union".to_string(),
        Type::Invalid => "invalid type".to_string(),
    }
}

pub(crate) fn basic_kind(tt: &TypeTable, t: TypeId) -> Option<BasicId> {
    match tt.type_of(underlying(tt, t)) {
        Type::Basic(b) => Some(*b),
        _ => None,
    }
}

/// 官方 `isConstType`（`predicates.go:32`）：**不探入类型参数**
/// （`underlying` 对 `TypeParam` 返回约束接口 ⇒ [`basic_kind`] 为 `None`）。
fn is_const_type(tt: &TypeTable, t: TypeId) -> bool {
    basic_kind(tt, t).is_some_and(|b| b.is_bool() || b.is_numeric() || b.is_string())
}

/// 官方 `constConvertibleTo` 的整数→字符串码点面：
/// `Uint64Val` 失败或超 `unicode.MaxRune` ⇒ **U+FFFD**（`string(-1)` 合法）。
fn int_const_to_string(val: &Const) -> Vec<u8> {
    let ch = match to_int(val) {
        Some(Const::Int(n)) => n
            .to_i64()
            .filter(|v| (0..=0x10FFFF).contains(v))
            .and_then(|v| u32::try_from(v).ok())
            .and_then(char::from_u32)
            .unwrap_or('\u{FFFD}'),
        _ => '\u{FFFD}',
    };
    ch.to_string().into_bytes()
}

/// 数组长度（`[n]T` 的 `n`；`[...]T` 或非常量 ⇒ `-1`）。
fn arr_len(tt: &TypeTable, len: Option<ConstId>) -> i64 {
    match len.map(|c| tt.consts().get(c)) {
        Some(Const::Int(v)) => v.to_i64().unwrap_or(-1),
        _ => -1,
    }
}

/// 官方 `constant.StringLen`：常量字符串的**字节**长度；
/// `Unknown` ⇒ `0`（官方 `unknownVal` 分支）；非常量 ⇒ `-1`。
fn string_len(val: &Option<Const>) -> i64 {
    match val {
        Some(Const::Str(bytes)) => i64::try_from(bytes.len()).unwrap_or(i64::MAX),
        Some(Const::Unknown) => 0,
        _ => -1,
    }
}

/// 整值常量 ⇒ `i64`（官方 `constant.Int64Val` 的窄面：经 `ToInt`）。
fn const_as_i64(c: &Const) -> Option<i64> {
    match to_int(c) {
        Some(Const::Int(n)) => n.to_i64(),
        _ => None,
    }
}

/// 官方 `constant.Sign(v) < 0`（索引面只会遇到整数常量）。
fn const_negative(c: &Const) -> bool {
    match c {
        Const::Int(n) => n.is_neg(),
        Const::Float(r) => r.is_neg(),
        _ => false,
    }
}

/// 官方 `operand.String` 的源码近似：名字 / 选择器链 / 括号剥壳。
/// （消息文本只进人读面，`(行,列,类别)` 才是对账面 —— 与 22-3 / 23-4 同口径。）
pub(crate) fn expr_label(e: &Expr) -> Option<String> {
    match e {
        Expr::Name(n) => Some(n.value.clone()),
        Expr::Selector(s) => expr_label(&s.x).map(|b| format!("{b}.{}", s.sel.value)),
        Expr::Paren(p) => expr_label(&p.x),
        // 字面量的源码文本（官方 `ExprString` 同样直接回原文本）。
        // M5/Task 5 补：此前 `s += 1` 的消息退化成 `s += expression`。
        Expr::BasicLit(l) => Some(l.value.clone()),
        _ => None,
    }
}

/// 官方 `operand.String` 的**窄面替身**：常量 ⇒ 值文本；
/// 其余 ⇒ `value of type T`。（无源码文本 —— 消息文本只进人读面，
/// `(行,列,类别)` 才是对账面。）
pub(crate) fn operand_text(tt: &TypeTable, x: &Operand) -> String {
    if x.mode == Mode::Constant
        && let Some(v) = &x.val
    {
        return const_text(v);
    }
    match x.typ {
        Some(t) => format!("value of type {}", type_str(tt, t)),
        None => "value".to_string(),
    }
}

/// 官方 `typesSummary(operandTypes(args), …)` 的窄面：`(int, string)`。
///
/// 只进人读消息（`not enough arguments in call to f` 的 `have` 行）。
pub(crate) fn tuple_text(tt: &TypeTable, args: &[Operand]) -> String {
    let items: Vec<String> = args
        .iter()
        .map(|x| match x.typ {
            Some(t) => type_str(tt, t),
            None => "value".to_owned(),
        })
        .collect();
    format!("({})", items.join(", "))
}

/// 形参表的窄面文本：`(int)` / `(...int)`。
pub(crate) fn params_text(
    tt: &TypeTable,
    params: &[(Option<String>, TypeId)],
    variadic: bool,
) -> String {
    let n = params.len();
    let items: Vec<String> = params
        .iter()
        .enumerate()
        .map(|(i, (_, t))| {
            if variadic && i + 1 == n {
                let inner = match tt.type_of(underlying(tt, *t)) {
                    Type::Slice(e) => type_str(tt, *e),
                    _ => type_str(tt, *t),
                };
                format!("...{inner}")
            } else {
                type_str(tt, *t)
            }
        })
        .collect();
    format!("({})", items.join(", "))
}

/// 官方 `isUntyped`（含 `untyped nil`）。
pub(crate) fn is_untyped(tt: &TypeTable, t: TypeId) -> bool {
    basic_kind(tt, t).is_some_and(BasicId::is_untyped)
}

/// 官方 `allNumeric`（含无类型数值）。
pub(crate) fn all_numeric(tt: &TypeTable, t: TypeId) -> bool {
    basic_kind(tt, t).is_some_and(BasicId::is_numeric)
}

/// 官方 `allBoolean`（含无类型 bool）。
pub(crate) fn all_boolean(tt: &TypeTable, t: TypeId) -> bool {
    basic_kind(tt, t).is_some_and(BasicId::is_bool)
}

/// 官方 `allString`（含无类型 string）。
fn all_string(tt: &TypeTable, t: TypeId) -> bool {
    basic_kind(tt, t).is_some_and(BasicId::is_string)
}

/// 官方 `allInteger`（含无类型整数/rune）。
pub(crate) fn all_integer(tt: &TypeTable, t: TypeId) -> bool {
    basic_kind(tt, t).is_some_and(BasicId::is_integer)
}

/// 官方 `allOrdered = allBasic(t, IsOrdered)`（`= 整数 ∪ 浮点 ∪ 字符串`）。
pub(crate) fn all_ordered(tt: &TypeTable, t: TypeId) -> bool {
    basic_kind(tt, t).is_some_and(|b| b.is_integer() || b.is_float() || b.is_string())
}

/// 官方 `isPointer`。
fn is_pointer(tt: &TypeTable, t: TypeId) -> bool {
    matches!(tt.type_of(underlying(tt, t)), Type::Pointer(_))
}

/// 官方 `isNonTypeParamInterface`：非类型参数的接口。
///
/// 我们的 `Type::TypeParam` 是独立变体 ⇒ `underlying` 落在 `Interface` 即「非类型参数」。
pub(crate) fn is_non_typeparam_interface(tt: &TypeTable, t: TypeId) -> bool {
    if matches!(tt.type_of(unalias(tt, t)), Type::TypeParam(_)) {
        return false;
    }
    matches!(tt.type_of(underlying(tt, t)), Type::Interface(_))
}

/// `x` 是不是（可能带括号的）**复合字面量**。
///
/// 只服务 `&` 的「可寻址性例外」一条（`go_spec` §Address operators）；
/// 不参与其它判定 —— 复合字面量在别处就是普通的 `value`。
fn is_composite_lit(e: &Expr) -> bool {
    match e {
        Expr::Paren(p) => is_composite_lit(&p.x),
        Expr::CompositeLit(_) => true,
        _ => false,
    }
}
