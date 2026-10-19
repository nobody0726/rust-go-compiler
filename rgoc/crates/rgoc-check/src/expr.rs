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
    BasicLit, CallExpr, Decl, Expr, IndexExpr, LitKind, Name, Op, Operation, SelectorExpr,
    SliceExpr,
};
use rgoc_const::{
    BasicId, Const, Sizes, const_from_literal, default_type_of, fold_binary, fold_compare,
    fold_unary_typed, representable,
};
use rgoc_lex::{Diag, DiagCode, FileId, Pos};
use rgoc_types::{
    ChanDir, ConstId, Type, TypeId, TypeTable, assignable, comparable, convertible, identical,
    unalias, underlying,
};

use crate::check::Session;
use crate::lookup::{Found, lookup_field_or_method};
use crate::operand::{Mode, Operand};
use crate::selection::{SelKind, method_expr_sig, method_value_sig};

/// 表达式入口：包级 `Decl::Var` 的初始化式逐个检查（Task 21 的窄面）。
pub(crate) fn check_package_decls(s: &mut Session<'_>) {
    // 先把包引用拷出来：迭代需要它，而 `cx` 独占 `&mut Session`。
    let pkg = s.pkg;
    let mut cx = ExprChecker { s };
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

/// 表达式检查器：持会话（诊断流 + 类型表 + 被检查包）。
struct ExprChecker<'s, 'p> {
    s: &'s mut Session<'p>,
}

// ─────────────────────────────── 入口分派 ───────────────────────────────

impl<'s, 'p> ExprChecker<'s, 'p> {
    fn tt(&self) -> &TypeTable {
        self.s.tt
    }

    fn report(&mut self, pos: Pos, code: DiagCode, message: String) {
        self.s.report_hard(Diag { pos, code, message });
    }

    fn type_text(&self, t: TypeId) -> String {
        type_str(self.tt(), t)
    }

    /// 官方 `exprInternal` 的**已接入子集**。未接入的形态（类型断言 /
    /// 值函数位的调用 / 复合字面量 / 类型族）⇒ `invalid` 且**不报** ——
    /// 避免用假诊断抢占 Task 25/30 的落点。转换（类型函数位）在
    /// [`Self::call`] 里接（Task 22），索引 / 切片在 [`Self::index_expr`] /
    /// [`Self::slice_expr`]（Task 23），选择器在 [`Self::selector`]（Task 24）。
    fn expr(&mut self, file: FileId, e: &Expr) -> Operand {
        match e {
            Expr::Name(n) => self.ident(file, n),
            Expr::BasicLit(l) => self.literal(l),
            Expr::Paren(p) => self.expr(file, &p.x),
            Expr::Unary(o) => self.unary(file, o),
            Expr::Binary(o) => self.binary(file, o),
            Expr::Call(c) => self.call(file, c),
            Expr::Index(ix) => self.index_expr(file, ix),
            Expr::Slice(sx) => self.slice_expr(file, sx),
            Expr::Selector(se) => self.selector(file, se),
            _ => Operand::invalid(),
        }
    }

    /// 官方 `check.ident` 的**值面**：`lookup_in_file` 命中的对象 → 操作数。
    ///
    /// 未命中说明名字未声明 —— 装载器已报 `TYP-UNDECLARED-NAME`（`T-CK-95`
    /// 钉住），这里**不得**重复上报。
    fn ident(&mut self, file: FileId, n: &Name) -> Operand {
        match self.s.pkg.lookup_in_file(file, &n.value) {
            Some(obj) => Operand::from_object(obj).unwrap_or_else(Operand::invalid),
            None => Operand::invalid(),
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

    /// 官方 `Checker.call` 的类型函数位分支：`T(x)` ⇒ [`Self::conversion`]。
    ///
    /// 值函数位（真调用 / 内建）⇒ `invalid` 且不报 —— 实参面与签名匹配归
    /// Task 23+/25；类型族函数位（`[]byte(x)` 等）在本任务同样不接。
    fn call(&mut self, file: FileId, c: &CallExpr) -> Operand {
        let f = self.expr(file, &c.fun);
        if f.mode != Mode::TypeExpr {
            return Operand::invalid();
        }
        let Some(target) = f.typ else {
            return Operand::invalid();
        };
        self.conversion(file, c, target)
    }

    /// 官方 `Checker.conversion`（`conversions.go:20`）的窄面。
    fn conversion(&mut self, file: FileId, c: &CallExpr, target: TypeId) -> Operand {
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
    fn valid_index(&mut self, x: &mut Operand, pos: Pos) -> bool {
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

    // ────────────────────────────── 选择器 ──────────────────────────────

    /// 官方 `selector`（`call.go:686`）的窄面。
    ///
    /// 查找在 [`lookup_field_or_method`]（官方 `lookup.go`），方法值 /
    /// 方法表达式的签名构造在 [`crate::selection`]。`pkg.Name`（`PkgName`
    /// 操作数建不出来 ⇒ `invalid`）与泛型接收者面 ⇒ 不报。
    fn selector(&mut self, file: FileId, e: &SelectorExpr) -> Operand {
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
            Op::And => self.addr(x, xpos),
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

    /// 官方 `&`（`expr.go:137`）：只允许 `variable`（复合字面量面未接入）。
    fn addr(&mut self, x: Operand, xpos: Pos) -> Operand {
        let Some(t) = x.typ else {
            return Operand::invalid();
        };
        if x.mode != Mode::Variable {
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
        // 官方：x 无效 ⇒ 原样返回；y 无效 ⇒ x 置 invalid。
        if !is_valid_op(&x) || !is_valid_op(&y) {
            return Operand::invalid();
        }
        if matches!(e.op, Op::Shl | Op::Shr) {
            return self.shift(x, y, e);
        }
        let xpos = e.x.pos();
        let ypos = rhs.pos();
        let (mut x, mut y) = (x, y);
        self.match_types(&mut x, &mut y, xpos, ypos);
        if !is_valid_op(&x) {
            return Operand::invalid();
        }
        if is_comparison(e.op) {
            return self.comparison(&x, &y, e.op, xpos, ypos);
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
            self.report(
                xpos,
                DiagCode::ChkInvalidOperation,
                format!("invalid operation: mismatched types {a} and {b}"),
            );
            return Operand::invalid();
        }
        // 官方 `binaryOpPredicates`（`expr.go:96`）。
        let ok = {
            let tt = self.tt();
            match e.op {
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
            return self.op_not_defined(e.op, xpos, tx);
        }
        // 除零（官方 `binary` 的 QUO/REM 守卫；条件含 `allInteger(x)`。
        // 复合除法下溢面暂缓 —— 台账登记）。
        if matches!(e.op, Op::Quo | Op::Rem) && self.divisor_zero(&x, &y) {
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
            let val = fold_binary(e.op, &xv, &yv);
            let mut out = Operand::constant(tx, val);
            self.overflow_check(&mut out, Some(e.op), false, e.pos, xpos);
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
    fn shift(&mut self, x: Operand, y: Operand, e: &Operation) -> Operand {
        let xpos = e.x.pos();
        let ypos = e.y.as_deref().map_or(e.pos, Expr::pos);
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
            let val = fold_binary(e.op, &xv, &yv);
            let mut out = Operand::constant(t, val);
            self.overflow_check(&mut out, Some(e.op), false, e.pos, xpos);
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
    fn convert_untyped(&mut self, x: &mut Operand, target: TypeId, pos: Pos) {
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
fn may_convert(tt: &TypeTable, x: &Operand, y: &Operand, tx: TypeId, ty: TypeId) -> bool {
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
fn max_type(a: BasicId, b: BasicId) -> Option<BasicId> {
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
fn op_str(op: Op) -> &'static str {
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
fn const_text(c: &Const) -> String {
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
fn type_str(tt: &TypeTable, t: TypeId) -> String {
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

fn basic_kind(tt: &TypeTable, t: TypeId) -> Option<BasicId> {
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
fn expr_label(e: &Expr) -> Option<String> {
    match e {
        Expr::Name(n) => Some(n.value.clone()),
        Expr::Selector(s) => expr_label(&s.x).map(|b| format!("{b}.{}", s.sel.value)),
        Expr::Paren(p) => expr_label(&p.x),
        _ => None,
    }
}

/// 官方 `operand.String` 的**窄面替身**：常量 ⇒ 值文本；
/// 其余 ⇒ `value of type T`。（无源码文本 —— 消息文本只进人读面，
/// `(行,列,类别)` 才是对账面。）
fn operand_text(tt: &TypeTable, x: &Operand) -> String {
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

/// 官方 `isUntyped`（含 `untyped nil`）。
fn is_untyped(tt: &TypeTable, t: TypeId) -> bool {
    basic_kind(tt, t).is_some_and(BasicId::is_untyped)
}

/// 官方 `allNumeric`（含无类型数值）。
fn all_numeric(tt: &TypeTable, t: TypeId) -> bool {
    basic_kind(tt, t).is_some_and(BasicId::is_numeric)
}

/// 官方 `allBoolean`（含无类型 bool）。
fn all_boolean(tt: &TypeTable, t: TypeId) -> bool {
    basic_kind(tt, t).is_some_and(BasicId::is_bool)
}

/// 官方 `allString`（含无类型 string）。
fn all_string(tt: &TypeTable, t: TypeId) -> bool {
    basic_kind(tt, t).is_some_and(BasicId::is_string)
}

/// 官方 `allInteger`（含无类型整数/rune）。
fn all_integer(tt: &TypeTable, t: TypeId) -> bool {
    basic_kind(tt, t).is_some_and(BasicId::is_integer)
}

/// 官方 `allOrdered = allBasic(t, IsOrdered)`（`= 整数 ∪ 浮点 ∪ 字符串`）。
fn all_ordered(tt: &TypeTable, t: TypeId) -> bool {
    basic_kind(tt, t).is_some_and(|b| b.is_integer() || b.is_float() || b.is_string())
}

/// 官方 `isPointer`。
fn is_pointer(tt: &TypeTable, t: TypeId) -> bool {
    matches!(tt.type_of(underlying(tt, t)), Type::Pointer(_))
}

/// 官方 `isNonTypeParamInterface`：非类型参数的接口。
///
/// 我们的 `Type::TypeParam` 是独立变体 ⇒ `underlying` 落在 `Interface` 即「非类型参数」。
fn is_non_typeparam_interface(tt: &TypeTable, t: TypeId) -> bool {
    if matches!(tt.type_of(unalias(tt, t)), Type::TypeParam(_)) {
        return false;
    }
    matches!(tt.type_of(underlying(tt, t)), Type::Interface(_))
}
