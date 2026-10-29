//! 内建函数的签名与静态约束（M4/Task 29；`T-CK-41/42`）。
//!
//! # 判据来源
//!
//! 官方 `go/types/builtins.go`（`builtin` 的主分派 + 18 个 `case`）与
//! `universe.go:180` 的 `predeclaredFuncs`（元数表）。本模块按「一个内建一个
//! 函数」逐支对齐；类别 / 位置由 `probe/task29-builtins.go` 的 oracle 实测钉死。
//!
//! # 元数表（官方 `predeclaredFuncs`）
//!
//! | 内建 | 元数 | 变参 | 结果 |
//! |---|---|---|---|
//! | `append` | 1 | ✔ | 首参类型 |
//! | `cap` / `len` | 1 | | `int`（常量面见下） |
//! | `clear` / `close` / `panic` | 1 | | 无值 |
//! | `complex` | 2 | | `complex64/128` / `untyped complex` |
//! | `copy` | 2 | | `int` |
//! | `delete` | 2 | | 无值 |
//! | `imag` / `real` | 1 | | `float32/64` / `untyped float` |
//! | `make` | 1 | ✔ | 首参类型 |
//! | `max` / `min` | 1 | ✔ | 公共类型 |
//! | `new` | 1 | | `*T` |
//! | `print` / `println` | 0 | ✔ | 无值 |
//! | `recover` | 0 | | `any`（`interface{}`；官方签名 `func() any`） |
//! | `assert` | 1 | | 无值（**仅** `--test-hooks`） |
//! | `trace` | 0 | ✔ | 无值（**仅** `--test-hooks`） |
//!
//! # 码与类别（`C8` 适配表）
//!
//! * `invalid argument: …` / `invalid append:` / `invalid copy:` ⇒
//!   `CHK-INVALID-BUILTIN-ARG`（类别 `builtin`）；
//! * `invalid operation:` 前缀族（`cannot close …`、`complex(…) (mismatched types …)`）
//!   ⇒ `CHK-INVALID-OPERATION`（`op-invalid`）；
//! * `cannot use … as … value in argument to append` / `in delete`
//!   ⇒ `CHK-INVALID-ASSIGN`（`assign`，走 [`ExprChecker::assignment`]）；
//! * `make` 的尺寸面（`must be integer` / `must not be negative` / `overflows int`）
//!   ⇒ `CHK-INVALID-INDEX`（`index`，与官方 `check.index` 同一条判据）。
//!
//! # 窄面（台账登记）
//!
//! * 类型参数操作数（`underIs` 的约束面）：`D-M3-2` —— 不判；
//! * `append` 的 `b = append(b, s...)` 特例、`copy` 的 `[]byte` ← `string`
//!   特例：**只接**非 `...` 形态；`...` 形态跳过元素判定（不假报）；
//! * `unsafe` 的 `Add/Sizeof/…` 与 `assert/trace` 的注入在 Task 31 / 本任务
//!   的 `ident` 分支。

use rgoc_ast::CallExpr;
use rgoc_const::{BasicId, Const};
use rgoc_lex::{DiagCode, FileId, Pos};
use rgoc_types::{Type, TypeId, identical, unalias, underlying};

use crate::expr::{
    ExprChecker, all_ordered, basic_kind, common_map_key, common_slice_elem, common_under,
    expr_label, is_untyped, operand_text, type_str, under_is,
};
use crate::operand::{Mode, Operand};

/// 内建调用入口（`ExprChecker::call` 的 `Mode::Builtin` 分支）。
pub(crate) fn call_builtin(
    cx: &mut ExprChecker<'_, '_>,
    file: FileId,
    c: &CallExpr,
    name: &str,
) -> Operand {
    // `...` 只允许 `append`（官方 `InvalidDotDotDot`）。
    if c.has_dots && name != "append" {
        cx.report(
            call_pos(c),
            DiagCode::ChkInvalidOperation,
            format!("invalid operation: invalid use of ... with built-in {name}"),
        );
        return Operand::invalid();
    }

    let Some((min, variadic)) = arity(name) else {
        return Operand::invalid();
    };
    let n = c.args.len();
    if n < min || (!variadic && n > min) {
        // 官方 `builtin` 的元数面：报点一律是**调用右括号**
        // （`task29-arity.go` 实测 `7:13` / `8:17` / `9:15` / `11:18` / `15:18`
        // 全部落在 `)`），措辞是 `invalid operation: {not enough|too many}
        // arguments for <调用原文> (expected N, found M)`。
        let word = if n < min { "not enough" } else { "too many" };
        cx.report(
            c.rparen,
            DiagCode::ChkInvalidBuiltinArg,
            format!(
                "invalid operation: {word} arguments for {}({}) (expected {min}, found {n})",
                name,
                call_args_text(c)
            ),
        );
        return Operand::invalid();
    }

    match name {
        "len" | "cap" => len_cap(cx, file, c, name),
        "append" => append(cx, file, c),
        "copy" => copy(cx, file, c),
        "close" => close(cx, file, c),
        "delete" => delete(cx, file, c),
        "clear" => clear(cx, file, c),
        "complex" => complex(cx, file, c),
        "real" | "imag" => real_imag(cx, file, c, name),
        "min" | "max" => min_max(cx, file, c, name),
        "make" => make(cx, file, c),
        "new" => new(cx, file, c),
        "panic" => {
            let _ = cx.expr(file, &c.args[0]);
            Operand::no_value()
        }
        "print" | "println" | "trace" => {
            for a in &c.args {
                let _ = cx.expr(file, a);
            }
            Operand::no_value()
        }
        "recover" => {
            // `recover()` 的类型是 `any`（官方 `universe.go`：`recover` 的
            // 签名是 `func() any`）。原先给 `Operand::no_value()` 是错的：
            // `err := recover(); err == nil` 里 `err` 因此拿到「无值」，
            // 判等报 `operator == not defined on untyped_nil`
            // （`closure4.go` / `convert4.go` / `defernil.go` / `method.go` 实测）。
            //
            // 这里直接造 `interface{}` 本体：`any` 是它的别名，
            // `identical` 走 unalias 后完全相同。
            let id =
                cx.s.tt
                    .add_interface(Vec::new(), None, rgoc_types::IfaceTSet::All);
            let t = cx.s.tt.interface_type(id);
            Operand::value(Some(t))
        }
        "assert" => {
            let _ = cx.expr(file, &c.args[0]);
            Operand::no_value()
        }
        // `rt_reflect_call(funcval, tramp, args, results)`：`reflect.Value.Call`
        // 的 ABI 桥入口（`C13` §5.5）。四个实参都必须是**一个字**的量
        // （指针 / uintptr）—— 它们直接进 `x0..x3`，宽度不对就是错位。
        "rt_reflect_call" => {
            for a in &c.args {
                let x = cx.expr(file, a);
                if x.mode == Mode::Invalid {
                    return Operand::invalid();
                }
                let ok = x
                    .typ
                    .and_then(|t| rgoc_types::layout::layout(cx.type_table(), t))
                    .is_some_and(|l| l.size == 8);
                if !ok {
                    cx.report(
                        a.start_pos(),
                        DiagCode::ChkInvalidBuiltinArg,
                        "invalid argument: rt_reflect_call 的实参必须是 8 字节（指针 / uintptr）"
                            .to_string(),
                    );
                    return Operand::invalid();
                }
            }
            Operand::no_value()
        }
        // `rt_reflect_alloc(op, out, a, b, c)`：写面的分配入口（`C13` §5.6）。
        // 五个实参同样必须各占**一个字**（`op` 与 `c` 是整数，其余是指针）。
        "rt_reflect_alloc" => {
            for a in &c.args {
                let x = cx.expr(file, a);
                if x.mode == Mode::Invalid {
                    return Operand::invalid();
                }
                let ok = x
                    .typ
                    .and_then(|t| rgoc_types::layout::layout(cx.type_table(), t))
                    .is_some_and(|l| l.size == 8);
                if !ok {
                    cx.report(
                        a.start_pos(),
                        DiagCode::ChkInvalidBuiltinArg,
                        "invalid argument: rt_reflect_alloc 的实参必须是 8 字节（指针 / 整数）"
                            .to_string(),
                    );
                    return Operand::invalid();
                }
            }
            Operand::no_value()
        }
        _ => Operand::invalid(),
    }
}

/// 元数表（`min` 与是否变参）。
fn arity(name: &str) -> Option<(usize, bool)> {
    Some(match name {
        "append" => (1, true),
        "cap" | "len" | "clear" | "close" | "panic" | "imag" | "real" => (1, false),
        "complex" | "copy" | "delete" => (2, false),
        "make" | "max" | "min" => (1, true),
        "new" | "assert" => (1, false),
        // `rt_reflect_call(funcval, tramp, args, results)`：定元数 4 个。
        "rt_reflect_call" => (4, false),
        // `rt_reflect_alloc(op, out, a, b, c)`：定元数 5 个。
        "rt_reflect_alloc" => (5, false),
        // ⚠️ `recover` **不是**变参（`predeclaredFuncs` 的 `{0, false}`）——
        // `recover(1)` 是 `too many arguments`（`task29-arity.go` 实测 `15:18`）。
        "recover" => (0, false),
        "print" | "println" | "trace" => (0, true),
        _ => return None,
    })
}

/// 调用起点（官方 `CallExpr.Pos()` = `Fun.Pos()`）。
fn call_pos(c: &CallExpr) -> Pos {
    c.fun.start_pos()
}

/// 实参列表的**近似原文**（`len(s, s)` 里的 `s, s` 段）。
///
/// 官方消息里是源码切片（`call.Args` 的原文）；本仓检查器**不持有源码**，
/// 所以按 `expr_label` 逐参还原，取不到标签的落 `_`。这只影响**消息文本**，
/// 不影响位置与类别 —— `C8` 只比 `(行, 类别)`，消息不进出对撞面。
fn call_args_text(c: &CallExpr) -> String {
    c.args
        .iter()
        .map(|a| expr_label(a).unwrap_or_else(|| "_".to_string()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// 参数操作数文本（`x (variable of type T)` / `"abc" (untyped string constant)`）。
fn arg_text(cx: &ExprChecker<'_, '_>, x: &Operand) -> String {
    let tt = cx.type_table();
    let mode = x.mode.as_str();
    match x.typ {
        Some(t) => format!(
            "{} ({mode} of type {})",
            operand_text(tt, x),
            type_str(tt, t)
        ),
        None => operand_text(tt, x),
    }
}

// ────────────────────────────── len / cap ──────────────────────────────

fn len_cap(cx: &mut ExprChecker<'_, '_>, file: FileId, c: &CallExpr, name: &str) -> Operand {
    let arg = &c.args[0];
    let x = cx.expr(file, arg);
    if x.mode == Mode::Invalid {
        return Operand::invalid();
    }
    let Some(t) = x.typ else {
        return Operand::invalid();
    };
    // ── 类型参数面（`M12/Task 9`）：官方 `underIs`（`builtins.go:186`）────────
    //
    // 官方对类型参数只判「形状认得出」，结果恒 `int` 且**不是常量**
    // （`mode = value`，不走 `constant_` 分支）—— 所以这里不需要具体形状，
    // 只要「类型集里每个 term 的底层都是 len/cap 支持的形状」。
    // `*[N]T` 也算（官方 `arrayPtrDeref`）。
    let tp_verdict = {
        let tt = cx.type_table();
        if matches!(tt.type_of(unalias(tt, t)), Type::TypeParam(_)) {
            Some(under_is(tt, t, |u| match u {
                Type::Basic(b) => b.is_string() && name == "len",
                Type::Array { .. } | Type::Slice(_) | Type::Chan { .. } => true,
                Type::Pointer(p) => {
                    matches!(tt.type_of(underlying(tt, *p)), Type::Array { .. })
                }
                Type::Map { .. } => name == "len",
                _ => false,
            }))
        } else {
            None
        }
    };
    if let Some(ok) = tp_verdict {
        if ok {
            let int_t = cx.tt_mut().add_basic(BasicId::Int);
            return Operand::value(Some(int_t));
        }
        let what = arg_text(cx, &x);
        cx.report(
            arg.start_pos(),
            DiagCode::ChkInvalidBuiltinArg,
            format!("invalid argument: {what} for built-in {name}"),
        );
        return Operand::invalid();
    }
    enum K {
        Str,
        Arr(i64),
        PtrArr(i64),
        Slice,
        Map,
        Chan,
        Bad,
    }
    let k = {
        let tt = cx.type_table();
        match tt.type_of(underlying(tt, t)) {
            Type::Basic(b) if b.is_string() => K::Str,
            Type::Array { len, .. } => K::Arr(arr_len(tt, *len)),
            Type::Pointer(p) => match tt.type_of(underlying(tt, *p)) {
                Type::Array { len, .. } => K::PtrArr(arr_len(tt, *len)),
                _ => K::Bad,
            },
            Type::Slice(_) => K::Slice,
            Type::Map { .. } => K::Map,
            Type::Chan { .. } => K::Chan,
            _ => K::Bad,
        }
    };
    let int_t = cx.tt_mut().add_basic(BasicId::Int);
    match k {
        K::Str if name == "len" => {
            if x.mode == Mode::Constant {
                let n = match &x.val {
                    Some(Const::Str(b)) => i64::try_from(b.len()).unwrap_or(i64::MAX),
                    Some(Const::Unknown) => 0,
                    _ => -1,
                };
                if n >= 0 {
                    return Operand::constant(int_t, int_const(n));
                }
            }
            Operand::value(Some(int_t))
        }
        K::Arr(n) | K::PtrArr(n) => {
            if n >= 0 {
                Operand::constant(int_t, int_const(n))
            } else {
                Operand::value(Some(int_t))
            }
        }
        K::Slice | K::Map | K::Chan => Operand::value(Some(int_t)),
        _ => {
            let what = arg_text(cx, &x);
            cx.report(
                arg.start_pos(),
                DiagCode::ChkInvalidBuiltinArg,
                format!("invalid argument: {what} for built-in {name}"),
            );
            Operand::invalid()
        }
    }
}

// ────────────────────────────── append ──────────────────────────────

fn append(cx: &mut ExprChecker<'_, '_>, file: FileId, c: &CallExpr) -> Operand {
    let first = &c.args[0];
    let s = cx.expr(file, first);
    if s.mode == Mode::Invalid {
        return Operand::invalid();
    }
    let Some(st) = s.typ else {
        return Operand::invalid();
    };
    // 元素类型：官方 `sliceElem`（`builtins.go:992`）—— 「所有 term 的底层都是
    // 同一个 `[]E`」⇒ `E`。类型参数面（`M12/Task 9`）。
    let elem = {
        let tt = cx.type_table();
        common_slice_elem(tt, st)
    };
    let Some(elem) = elem else {
        let what = arg_text(cx, &s);
        cx.report(
            first.start_pos(),
            DiagCode::ChkInvalidBuiltinArg,
            format!("invalid append: argument must be a slice; have {what}"),
        );
        return Operand::invalid();
    };
    // `append(s, xs...)` 的展开形态：元素类型由编译器核（窄面跳过）。
    if !c.has_dots {
        for a in &c.args[1..] {
            let mut v = cx.expr_hint(file, a, Some(elem));
            cx.assignment(&mut v, a.start_pos(), Some(elem), "argument to append");
        }
    } else {
        for a in &c.args[1..] {
            let _ = cx.expr(file, a);
        }
    }
    Operand::value(Some(st))
}

// ────────────────────────────── copy ──────────────────────────────

fn copy(cx: &mut ExprChecker<'_, '_>, file: FileId, c: &CallExpr) -> Operand {
    let d = cx.expr(file, &c.args[0]);
    if d.mode == Mode::Invalid {
        return Operand::invalid();
    }
    let Some(dt) = d.typ else {
        return Operand::invalid();
    };
    let dst_elem = {
        let tt = cx.type_table();
        match tt.type_of(underlying(tt, dt)) {
            Type::Slice(e) => Some(*e),
            _ => None,
        }
    };
    let Some(dst_elem) = dst_elem else {
        let what = arg_text(cx, &d);
        cx.report(
            c.args[0].start_pos(),
            DiagCode::ChkInvalidBuiltinArg,
            format!("invalid copy: argument must be a slice; have {what}"),
        );
        return Operand::invalid();
    };
    let src = cx.expr(file, &c.args[1]);
    if src.mode == Mode::Invalid {
        return Operand::invalid();
    }
    let Some(st) = src.typ else {
        return Operand::invalid();
    };
    let (src_elem, is_str) = {
        let tt = cx.type_table();
        match tt.type_of(underlying(tt, st)) {
            Type::Slice(e) => (Some(*e), false),
            Type::Basic(b) if b.is_string() => (None, true),
            _ => (None, false),
        }
    };
    // `copy([]byte, string)` 特例（官方 `special` 分支）。
    if is_str {
        let byte_ok = {
            let tt = cx.type_table();
            matches!(tt.type_of(underlying(tt, dst_elem)), Type::Basic(b) if *b == BasicId::Uint8)
        };
        if !byte_ok {
            let what = arg_text(cx, &src);
            cx.report(
                c.args[1].start_pos(),
                DiagCode::ChkInvalidBuiltinArg,
                format!("invalid copy: argument must be a slice; have {what}"),
            );
            return Operand::invalid();
        }
    } else {
        let Some(se) = src_elem else {
            let what = arg_text(cx, &src);
            cx.report(
                c.args[1].start_pos(),
                DiagCode::ChkInvalidBuiltinArg,
                format!("invalid copy: argument must be a slice; have {what}"),
            );
            return Operand::invalid();
        };
        let same = {
            let tt = cx.type_table();
            identical(tt, dst_elem, se).unwrap_or(true)
        };
        if !same {
            let (a, b) = {
                let tt = cx.type_table();
                (type_str(tt, dst_elem), type_str(tt, se))
            };
            cx.report(
                c.args[0].start_pos(),
                DiagCode::ChkInvalidBuiltinArg,
                format!("invalid copy: arguments have different element types {a} and {b}"),
            );
            return Operand::invalid();
        }
    }
    let int_t = cx.tt_mut().add_basic(BasicId::Int);
    Operand::value(Some(int_t))
}

// ────────────────────────────── close ──────────────────────────────

fn close(cx: &mut ExprChecker<'_, '_>, file: FileId, c: &CallExpr) -> Operand {
    let arg = &c.args[0];
    let x = cx.expr(file, arg);
    if x.mode == Mode::Invalid {
        return Operand::invalid();
    }
    let Some(t) = x.typ else {
        return Operand::invalid();
    };
    // ── 类型参数面（`M12/Task 9`）：官方 `underIs`（`builtins.go:254`）───────
    //
    // 官方把两种否决写在同一个谓词里并**分别报消息**，`underIs` 只回 bool
    // ⇒ 这里分两趟：先「都是 chan」，再「都不是 `RecvOnly`」。
    let tp = {
        let tt = cx.type_table();
        matches!(tt.type_of(unalias(tt, t)), Type::TypeParam(_))
    };
    if tp {
        let all_chan = {
            let tt = cx.type_table();
            under_is(tt, t, |u| matches!(u, Type::Chan { .. }))
        };
        let all_sendable = all_chan && {
            let tt = cx.type_table();
            under_is(
                tt,
                t,
                |u| matches!(u, Type::Chan { dir, .. } if *dir != rgoc_types::ChanDir::RecvOnly),
            )
        };
        if all_sendable {
            return Operand::no_value();
        }
        let what = arg_text(cx, &x);
        let msg = if all_chan {
            format!("invalid operation: cannot close receive-only channel {what}")
        } else {
            format!("invalid operation: cannot close non-channel {what}")
        };
        cx.report(arg.start_pos(), DiagCode::ChkInvalidOperation, msg);
        return Operand::invalid();
    }
    enum K {
        Ok,
        RecvOnly,
        NotChan,
    }
    let k = {
        let tt = cx.type_table();
        match tt.type_of(underlying(tt, t)) {
            Type::Chan { dir, .. } => {
                if *dir == rgoc_types::ChanDir::RecvOnly {
                    K::RecvOnly
                } else {
                    K::Ok
                }
            }
            _ => K::NotChan,
        }
    };
    match k {
        K::Ok => Operand::no_value(),
        K::RecvOnly => {
            let what = arg_text(cx, &x);
            cx.report(
                arg.start_pos(),
                DiagCode::ChkInvalidOperation,
                format!("invalid operation: cannot close receive-only channel {what}"),
            );
            Operand::invalid()
        }
        K::NotChan => {
            let what = arg_text(cx, &x);
            cx.report(
                arg.start_pos(),
                DiagCode::ChkInvalidOperation,
                format!("invalid operation: cannot close non-channel {what}"),
            );
            Operand::invalid()
        }
    }
}

// ────────────────────────────── delete ──────────────────────────────

fn delete(cx: &mut ExprChecker<'_, '_>, file: FileId, c: &CallExpr) -> Operand {
    let m = cx.expr(file, &c.args[0]);
    if m.mode == Mode::Invalid {
        return Operand::invalid();
    }
    let Some(mt) = m.typ else {
        return Operand::invalid();
    };
    // 类型参数面（`M12/Task 9`）：官方 `underIs`（`builtins.go:428`）——
    // 每个 term 的底层都必须是 map，且**所有 term 的 key 必须 identical**。
    let key = {
        let tt = cx.type_table();
        common_map_key(tt, mt)
    };
    let Some(key) = key else {
        let what = arg_text(cx, &m);
        cx.report(
            c.args[0].start_pos(),
            DiagCode::ChkInvalidBuiltinArg,
            format!("invalid argument: {what} is not a map"),
        );
        return Operand::invalid();
    };
    let mut k = cx.expr(file, &c.args[1]);
    cx.assignment(&mut k, c.args[1].start_pos(), Some(key), "delete");
    Operand::no_value()
}

// ────────────────────────────── clear ──────────────────────────────

fn clear(cx: &mut ExprChecker<'_, '_>, file: FileId, c: &CallExpr) -> Operand {
    let arg = &c.args[0];
    let x = cx.expr(file, arg);
    if x.mode == Mode::Invalid {
        return Operand::invalid();
    }
    let Some(t) = x.typ else {
        return Operand::invalid();
    };
    let ok = {
        let tt = cx.type_table();
        // 类型参数面（`M12/Task 9`）：官方 `underIs` —— 每个 term 的底层都得是
        // map 或 slice。
        under_is(tt, t, |u| matches!(u, Type::Map { .. } | Type::Slice(_)))
    };
    if ok {
        return Operand::no_value();
    }
    let what = arg_text(cx, &x);
    cx.report(
        arg.start_pos(),
        DiagCode::ChkInvalidBuiltinArg,
        format!(
            "invalid argument: cannot clear {what}: argument must be (or constrained by) map or slice"
        ),
    );
    Operand::invalid()
}

// ────────────────────────────── complex ──────────────────────────────

fn complex(cx: &mut ExprChecker<'_, '_>, file: FileId, c: &CallExpr) -> Operand {
    let mut x = cx.expr(file, &c.args[0]);
    let mut y = cx.expr(file, &c.args[1]);
    if x.mode == Mode::Invalid || y.mode == Mode::Invalid {
        return Operand::invalid();
    }
    let (Some(tx), Some(ty)) = (x.typ, y.typ) else {
        return Operand::invalid();
    };
    // 无类型面（官方三段 `d` 分支）：单侧无类型 ⇒ 转到另一侧；双侧 ⇒ 都抬到
    // `untyped float`（数值且虚部为 0 的那些）。
    let (x_untyped, y_untyped) = {
        let tt = cx.type_table();
        (is_untyped(tt, tx), is_untyped(tt, ty))
    };
    match (x_untyped, y_untyped) {
        (true, false) => cx.convert_untyped(&mut x, ty, c.args[0].start_pos()),
        (false, true) => cx.convert_untyped(&mut y, tx, c.args[1].start_pos()),
        (true, true) => {
            let uf = cx.tt_mut().add_basic(BasicId::UntypedFloat);
            if x.mode == Mode::Constant && y.mode == Mode::Constant {
                for op in [&mut x, &mut y] {
                    if is_numeric_val(op) {
                        op.typ = Some(uf);
                    }
                }
            } else {
                cx.convert_untyped(&mut x, uf, c.args[0].start_pos());
                cx.convert_untyped(&mut y, uf, c.args[1].start_pos());
            }
        }
        (false, false) => {}
    }
    if x.mode == Mode::Invalid || y.mode == Mode::Invalid {
        return Operand::invalid();
    }
    let (Some(tx), Some(ty)) = (x.typ, y.typ) else {
        return Operand::invalid();
    };
    let same = {
        let tt = cx.type_table();
        identical(tt, tx, ty).unwrap_or(true)
    };
    if !same {
        let (a, b) = {
            let tt = cx.type_table();
            (type_str(tt, tx), type_str(tt, ty))
        };
        let label = expr_label(&c.fun).unwrap_or_else(|| "complex".to_string());
        cx.report(
            c.args[0].start_pos(),
            DiagCode::ChkInvalidOperation,
            format!("invalid operation: {label}(...) (mismatched types {a} and {b})"),
        );
        return Operand::invalid();
    }
    // 结果：float32 ⇒ complex64；float64 ⇒ complex128；untyped float ⇒ untyped complex。
    let res = {
        let tt = cx.type_table();
        match basic_kind(tt, tx) {
            Some(BasicId::Float32) => Some(BasicId::Complex64),
            Some(BasicId::Float64) => Some(BasicId::Complex128),
            Some(BasicId::UntypedFloat) => Some(BasicId::UntypedComplex),
            _ => None,
        }
    };
    let Some(res) = res else {
        let a = {
            let tt = cx.type_table();
            type_str(tt, tx)
        };
        cx.report(
            c.args[0].start_pos(),
            DiagCode::ChkInvalidBuiltinArg,
            format!("invalid argument: arguments have type {a}, expected floating-point"),
        );
        return Operand::invalid();
    };
    let res_t = cx.tt_mut().add_basic(res);
    if x.mode == Mode::Constant && y.mode == Mode::Constant {
        // 常量折叠：实部 + 虚部（`Const::Complex`）。
        let re = x.val.clone().unwrap_or(Const::Unknown);
        let im = y.val.clone().unwrap_or(Const::Unknown);
        return match (to_float(&re), to_float(&im)) {
            (Some(re), Some(im)) => Operand::constant(res_t, Const::Complex { re, im }),
            _ => Operand::constant(res_t, Const::Unknown),
        };
    }
    Operand::value(Some(res_t))
}

// ────────────────────────────── real / imag ──────────────────────────────

fn real_imag(cx: &mut ExprChecker<'_, '_>, file: FileId, c: &CallExpr, name: &str) -> Operand {
    let arg = &c.args[0];
    let x = cx.expr(file, arg);
    if x.mode == Mode::Invalid {
        return Operand::invalid();
    }
    let Some(t) = x.typ else {
        return Operand::invalid();
    };
    let res = {
        let tt = cx.type_table();
        match basic_kind(tt, t) {
            Some(BasicId::Complex64) => Some(BasicId::Float32),
            Some(BasicId::Complex128) => Some(BasicId::Float64),
            Some(BasicId::UntypedComplex) => Some(BasicId::UntypedFloat),
            // 无类型数值（int / rune / float）：`real(1)` 是 `untyped float`。
            Some(BasicId::UntypedInt)
            | Some(BasicId::UntypedRune)
            | Some(BasicId::UntypedFloat) => Some(BasicId::UntypedFloat),
            _ => None,
        }
    };
    let Some(res) = res else {
        let a = {
            let tt = cx.type_table();
            type_str(tt, t)
        };
        cx.report(
            arg.start_pos(),
            DiagCode::ChkInvalidBuiltinArg,
            format!("invalid argument: argument has type {a}, expected complex type"),
        );
        return Operand::invalid();
    };
    let res_t = cx.tt_mut().add_basic(res);
    if x.mode == Mode::Constant {
        let v = x.val.clone().unwrap_or(Const::Unknown);
        let out = match (&v, name) {
            (Const::Complex { re, im: _ }, "real") => Const::Float(re.clone()),
            (Const::Complex { re: _, im }, _) => Const::Float(im.clone()),
            (Const::Int(n), _) => {
                Const::Float(rgoc_const::BigRat::from_i64(n.to_i64().unwrap_or_default()))
            }
            (Const::Float(r), _) => Const::Float(r.clone()),
            _ => Const::Unknown,
        };
        return Operand::constant(res_t, out);
    }
    Operand::value(Some(res_t))
}

// ────────────────────────────── min / max ──────────────────────────────

fn min_max(cx: &mut ExprChecker<'_, '_>, file: FileId, c: &CallExpr, name: &str) -> Operand {
    let mut ops: Vec<Operand> = Vec::with_capacity(c.args.len());
    for a in &c.args {
        let x = cx.expr(file, a);
        if x.mode == Mode::Invalid {
            return Operand::invalid();
        }
        ops.push(x);
    }
    let mut common: Option<TypeId> = ops[0].typ;
    for i in 1..ops.len() {
        let (Some(tp), Some(tc)) = (common, ops[i].typ) else {
            return Operand::invalid();
        };
        let (pu, cu) = {
            let tt = cx.type_table();
            (is_untyped(tt, tc), is_untyped(tt, tp))
        };
        if pu && cu {
            // 双无类型 ⇒ `maxType`；失败即 mismatch。
            let (a, b) = {
                let tt = cx.type_table();
                (
                    basic_kind(tt, tc).unwrap_or(BasicId::UntypedInt),
                    basic_kind(tt, tp).unwrap_or(BasicId::UntypedInt),
                )
            };
            match crate::expr::max_type(a, b) {
                Some(m) => {
                    let t = cx.tt_mut().add_basic(m);
                    common = Some(t);
                    continue;
                }
                None => {
                    let (ta, tb) = {
                        let tt = cx.type_table();
                        (type_str(tt, tp), type_str(tt, tc))
                    };
                    cx.report(
                        c.args[i].start_pos(),
                        DiagCode::ChkInvalidBuiltinArg,
                        format!(
                            "invalid argument: mismatched types {ta} (previous argument) and {tb} (type of {})",
                            operand_text(cx.type_table(), &ops[i])
                        ),
                    );
                    return Operand::invalid();
                }
            }
        }
        if pu {
            cx.convert_untyped(&mut ops[i], tp, c.args[i].start_pos());
            if ops[i].mode == Mode::Invalid {
                return Operand::invalid();
            }
        } else if cu {
            cx.convert_untyped(&mut ops[0], tc, c.args[0].start_pos());
            if ops[0].mode == Mode::Invalid {
                return Operand::invalid();
            }
            common = Some(tc);
        } else {
            let same = {
                let tt = cx.type_table();
                identical(tt, tp, tc).unwrap_or(true)
            };
            if !same {
                let (ta, tb) = {
                    let tt = cx.type_table();
                    (type_str(tt, tp), type_str(tt, tc))
                };
                cx.report(
                    c.args[i].start_pos(),
                    DiagCode::ChkInvalidBuiltinArg,
                    format!("invalid argument: mismatched types {ta} and {tb}"),
                );
                return Operand::invalid();
            }
        }
    }
    let Some(t) = common else {
        return Operand::invalid();
    };
    let ordered = {
        let tt = cx.type_table();
        all_ordered(tt, t)
    };
    if !ordered {
        let a = {
            let tt = cx.type_table();
            type_str(tt, t)
        };
        cx.report(
            c.args[0].start_pos(),
            DiagCode::ChkInvalidBuiltinArg,
            format!("invalid argument: {a} is not an ordered type"),
        );
        return Operand::invalid();
    }
    // 结果：全常量 ⇒ 折叠；否则值。
    if ops.iter().all(|o| o.mode == Mode::Constant) {
        let mut best = ops[0].val.clone().unwrap_or(Const::Unknown);
        for o in &ops[1..] {
            let v = o.val.clone().unwrap_or(Const::Unknown);
            if best == Const::Unknown || v == Const::Unknown {
                return Operand::constant(t, Const::Unknown);
            }
            let take = if name == "min" {
                fold_lt(&v, &best)
            } else {
                fold_lt(&best, &v)
            };
            if take == Some(true) {
                best = v;
            }
        }
        return Operand::constant(t, best);
    }
    let _ = name;
    Operand::value(Some(t))
}

// ────────────────────────────── make / new ──────────────────────────────

fn make(cx: &mut ExprChecker<'_, '_>, file: FileId, c: &CallExpr) -> Operand {
    let Some(t) = cx.type_expr(file, &c.args[0]) else {
        return Operand::invalid();
    };
    // ── 形状面（`M12/Task 9`）：官方 `commonUnder`（`under.go:79`）────────────
    //
    // 这里**不能**用 `underIs`：`make` 要靠具体形状定参数个数（切片 2..3、
    // map/chan 1..2），所以必须拿到「所有 term 的**共同**底层」。
    // `map[string]int | map[string]float64` 有共同底层吗？没有（两个 map 不同）
    // ⇒ 官方 `commonUnder` 报 `different underlying types`，这里落同一个码。
    let cu = {
        let tt = cx.tt_mut();
        common_under(tt, t)
    };
    let shape = {
        let tt = cx.type_table();
        cu.is_some_and(|u| {
            matches!(
                tt.type_of(u),
                Type::Slice(_) | Type::Map { .. } | Type::Chan { .. }
            )
        })
    };
    if !shape {
        let a = {
            let tt = cx.type_table();
            type_str(tt, t)
        };
        cx.report(
            c.args[0].start_pos(),
            DiagCode::ChkInvalidBuiltinArg,
            format!("invalid argument: cannot make {a}: type must be slice, map, or channel"),
        );
        return Operand::invalid();
    }
    // 形状相关的**上界**（官方 `make` 分支的 `arg` 计数面）：
    // 切片 2..=3（`make([]T, len, cap)`）、map / chan 1..=2（`make(M[, n])`）。
    // 报点是**调用起点**（`make` 这个标识符），措辞 `expects A or B arguments`。
    // `task29-make.go` 实测：`5:9` / `6:9` / `7:9`。
    let (lo, hi) = {
        let tt = cx.type_table();
        match cu.map(|u| tt.type_of(u)) {
            Some(Type::Slice(_)) => (2, 3),
            _ => (1, 2),
        }
    };
    let nargs = c.args.len();
    if nargs < lo || nargs > hi {
        cx.report(
            call_pos(c),
            DiagCode::ChkInvalidBuiltinArg,
            format!(
                "invalid operation: make({}) expects {lo} or {hi} arguments; found {nargs}",
                call_args_text(c)
            ),
        );
        return Operand::invalid();
    }
    // 尺寸参数：官方 `check.index` 同一判据（整数 / 非负 / 不溢出 int）。
    for a in &c.args[1..] {
        let mut s = cx.expr(file, a);
        if !cx.valid_index(&mut s, a.start_pos()) {
            return Operand::invalid();
        }
    }
    Operand::value(Some(t))
}

fn new(cx: &mut ExprChecker<'_, '_>, file: FileId, c: &CallExpr) -> Operand {
    if let Some(t) = cx.type_expr(file, &c.args[0]) {
        let p = cx.tt_mut().add_pointer(t);
        return Operand::value(Some(p));
    }
    // `new(expr)`（Go 1.26 起的形态，`newexpr.go` 实测）：实参不是类型，而是
    // 一个**值**；结果是 `*T`，`T` 是该值的类型（无类型常量取默认型）。
    //
    // 旧实现只认类型，`new(123)` 直接 `invalid`，于是 `*p` 报
    // `cannot indirect invalid type`，整个文件连静态面都过不去。
    let op = cx.expr(file, &c.args[0]);
    let Some(t) = op.typ else {
        return Operand::invalid();
    };
    // 无类型常量取**默认型**（`new(123)` ⇒ `*int`，官方 `defaultType`）。
    let t = {
        let under = rgoc_types::underlying(cx.s.tt, rgoc_types::unalias(cx.s.tt, t));
        match cx.s.tt.type_of(under) {
            rgoc_types::Type::Basic(b) if b.is_untyped() => {
                let d = rgoc_const::default_type_of(*b);
                cx.tt_mut().add_basic(d)
            }
            _ => t,
        }
    };
    let p = cx.tt_mut().add_pointer(t);
    Operand::value(Some(p))
}

// ────────────────────────────── 小工具 ──────────────────────────────

/// 整数常量（`len` / `cap` 的结果）。
fn int_const(n: i64) -> Const {
    Const::Int(rgoc_const::BigInt::from_i64(n))
}

/// 数组长度（`[...]` / 非常量 ⇒ `-1`）。
fn arr_len(tt: &rgoc_types::TypeTable, len: Option<rgoc_types::ConstId>) -> i64 {
    match len.map(|c| tt.consts().get(c)) {
        Some(Const::Int(v)) => v.to_i64().unwrap_or(-1),
        _ => -1,
    }
}

/// 无类型常量 → `BigRat`（`complex` / `real` 的折叠面）。
fn to_float(c: &Const) -> Option<rgoc_const::BigRat> {
    match c {
        Const::Int(n) => n.to_i64().map(rgoc_const::BigRat::from_i64),
        Const::Float(r) => Some(r.clone()),
        _ => None,
    }
}

/// 常量数值（`complex` 的双无类型分支：只抬「数值且虚部为 0」的那些）。
fn is_numeric_val(x: &Operand) -> bool {
    matches!(
        x.val,
        Some(Const::Int(_)) | Some(Const::Float(_)) | Some(Const::Complex { .. })
    )
}

/// 常量 `<`（`min` / `max` 折叠；不可比 ⇒ `None`）。
///
/// 混合数值面（`max(1.0, 2)` —— `untyped float` 与 `untyped int`）按官方
/// `matchTypes` 抬到公共 `untyped float` 后再比，否则会给出错误的极值。
fn fold_lt(a: &Const, b: &Const) -> Option<bool> {
    match (a, b) {
        (Const::Int(x), Const::Int(y)) => Some(x < y),
        (Const::Str(x), Const::Str(y)) => Some(x < y),
        (Const::Int(_) | Const::Float(_), Const::Int(_) | Const::Float(_)) => {
            Some(to_float(a)? < to_float(b)?)
        }
        _ => None,
    }
}
