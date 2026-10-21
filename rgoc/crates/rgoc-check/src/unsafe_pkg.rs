//! `unsafe` 白名单与尺寸（M4/Task 31；`T-CK-44`，`U-M4-06` 守卫）。
//!
//! # 为什么要有这一层
//!
//! `unsafe` 是**编译器伪包**：标准库里没有 `src/unsafe` 目录，所以
//! `rgoc-load` 的 import 解析器只能报 `TypUnsupportedImportForm`
//! （`D-M3-5`：模块解析与二进制 export data 都不在能力面内）。但官方语料的
//! 8 个 `--check` 文件（`cycles0/2/3/4/6`、`decls0`、`typeinstcycles`、
//! `spec/conversions`）都 `import "unsafe"`，且只用到 `Sizeof`（26 处）与
//! `Pointer`（13 处）。
//!
//! 所以本模块做两件事：
//!
//! 1. **白名单**（`D-M4-8`）：只接 `Pointer` / `Sizeof` / `Alignof` /
//!    `Offsetof`；其余成员（`Add` / `Slice` / `SliceData` / `String` /
//!    `StringData`）**显式拒绝** —— `U-M4-06` 的纪律是「白名单项不得记为
//!    pass，每项要有正向断言证明它确实被显式拒绝」，静默接受是**更糟**的
//!    通过。
//! 2. **装载诊断豁免**：`import "unsafe"` 的那条
//!    `TypUnsupportedImportForm` 在检查面**不再转述**（位置精确匹配，见
//!    [`unsafe_import_positions`]）—— 装载面本身不改（`rgoc types` 照旧）。
//!
//! # 尺寸模型（`StdSizes{8,8}`）
//!
//! 首发平台是 Linux/arm64（`AGENTS.md` §0），所以 `StdSizes{WordSize: 8,
//! MaxAlign: 8}`。[`size_of`] 按官方的 `stdSizes.Sizeof` 逐变体算：
//! 基本类型走 [`Sizes::width_of`]（位 → 字节），指针 / map / chan / 函数
//! 都是 8，`string` 16，切片 24，接口 16，数组 = 长度 × 元素，结构体按
//! 「逐字段对齐 + 尾部补齐」。
//!
//! # 窄面（`Ruling 31-1`，台账登记）
//!
//! * `unsafe.Pointer` 的**转换**（`(*T)(unsafe.Pointer(p))`）走通用
//!   [`crate::expr::ExprChecker::conversion`] 面：`UnsafePointer` 与 `*T`
//!   之间官方恒可转，我方按 `convertible` 的既有判定（`U-M4-06` 的
//!   「`Pointer` 转换」项留 M6+）；
//! * `uintptr` 往返（`unsafe.Pointer(uintptr(x))`）不建模；
//! * `Offsetof` 的字段偏移**只支持单层字段**（`unsafe.Offsetof(s.f)`）；
//!   多级选择器与提升字段 ⇒ `Const::Unknown`（不给错值）。

use rgoc_ast::{Decl, Expr, SelectorExpr};
use rgoc_const::{BasicId, BigInt, Const, Sizes};
use rgoc_lex::{DiagCode, FileId, Pos};
use rgoc_load::Package;
use rgoc_types::{Type, TypeId, TypeTable, underlying};

use crate::expr::ExprChecker;
use crate::operand::Operand;

/// 伪包名。
pub(crate) const PKG_NAME: &str = "unsafe";

/// `StdSizes{WordSize: 8, MaxAlign: 8}`（首发平台 Linux/arm64）。
const WORD: u64 = 8;
const MAX_ALIGN: u64 = 8;

/// 白名单成员（`D-M4-8`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Member {
    /// `unsafe.Pointer`（类型名）。
    Pointer,
    /// `unsafe.Sizeof`。
    Sizeof,
    /// `unsafe.Alignof`。
    Alignof,
    /// `unsafe.Offsetof`。
    Offsetof,
}

/// 名字 → 白名单成员（不在白名单 ⇒ `None`，调用方**显式拒绝**）。
#[must_use]
pub(crate) fn member(name: &str) -> Option<Member> {
    Some(match name {
        "Pointer" => Member::Pointer,
        "Sizeof" => Member::Sizeof,
        "Alignof" => Member::Alignof,
        "Offsetof" => Member::Offsetof,
        _ => return None,
    })
}

/// 某文件里 `import "unsafe"` 的位置（没有 ⇒ 空）。
///
/// 位置取法与 `rgoc-load` 的 `import_pos` 同源（有别名取别名位，否则取
/// spec 位）—— 装载诊断报的就是这个位置，两边必须逐字一致才能精确豁免。
#[must_use]
pub(crate) fn unsafe_import_positions(pkg: &Package, file: FileId) -> Vec<Pos> {
    let mut out = Vec::new();
    for (i, ast) in pkg.asts.iter().enumerate() {
        if pkg.files[i] != file {
            continue;
        }
        for decl in &ast.decls {
            let Decl::Import(imp) = decl else { continue };
            let Some(lit) = &imp.path else { continue };
            if lit.value.trim_matches(|c| c == '"' || c == '`') == PKG_NAME {
                out.push(imp.name.as_ref().map_or(imp.pos, |n| n.pos));
            }
        }
    }
    out
}

/// 这个文件是否 `import "unsafe"`。
#[must_use]
pub(crate) fn imports_unsafe(pkg: &Package, file: FileId) -> bool {
    !unsafe_import_positions(pkg, file).is_empty()
}

/// 全包的 `import "unsafe"` 位置（`check_package` 过滤装载诊断用）。
#[must_use]
pub(crate) fn all_unsafe_import_positions(pkg: &Package) -> std::collections::BTreeSet<Pos> {
    let mut out = std::collections::BTreeSet::new();
    for ast in &pkg.asts {
        for decl in &ast.decls {
            let Decl::Import(imp) = decl else { continue };
            let Some(lit) = &imp.path else { continue };
            if lit.value.trim_matches(|c| c == '"' || c == '`') == PKG_NAME {
                out.insert(imp.name.as_ref().map_or(imp.pos, |n| n.pos));
            }
        }
    }
    out
}

// ────────────────────────────── 求值面 ──────────────────────────────

/// `unsafe.X` 的**选择器**面（`unsafe.Pointer` 这种类型名用法）。
///
/// 非白名单成员 ⇒ 报 [`DiagCode::ChkUnsupportedUnsafe`]（类别 `unsafe`），
/// 报在 `.` 后的名字位（官方 `selector` 的落点）。
pub(crate) fn select(cx: &mut ExprChecker<'_, '_>, e: &SelectorExpr) -> Operand {
    let sel = e.sel.value.as_str();
    match member(sel) {
        Some(Member::Pointer) => {
            let t = cx.tt_mut().add_basic(BasicId::UnsafePointer);
            Operand::type_expr(t)
        }
        // 函数位：`unsafe.Sizeof` 不调用是官方 `UncalledBuiltin` 面；
        // 我方不给假诊断（调用面在 `call` 里接）。
        Some(Member::Sizeof | Member::Alignof | Member::Offsetof) => Operand::invalid(),
        None => {
            reject(cx, e, sel);
            Operand::invalid()
        }
    }
}

/// `unsafe.X(...)` 的**调用**面。
pub(crate) fn call(
    cx: &mut ExprChecker<'_, '_>,
    file: FileId,
    c: &rgoc_ast::CallExpr,
    sel: &str,
    sel_pos: Pos,
) -> Operand {
    match member(sel) {
        // `unsafe.Pointer(p)` 是**转换**（官方把它当类型用）。
        Some(Member::Pointer) => {
            let t = cx.tt_mut().add_basic(BasicId::UnsafePointer);
            cx.conversion(file, c, t)
        }
        Some(Member::Sizeof) | Some(Member::Alignof) => {
            if c.args.len() != 1 || c.has_dots {
                cx.report(
                    c.rparen,
                    DiagCode::ChkUnsupportedUnsafe,
                    format!(
                        "invalid operation: {sel} expects 1 argument, found {}",
                        c.args.len()
                    ),
                );
                return Operand::invalid();
            }
            let arg = cx.expr(file, &c.args[0]);
            if arg.mode == crate::operand::Mode::Invalid {
                return Operand::invalid();
            }
            let Some(t) = arg.typ else {
                return Operand::invalid();
            };
            let v = if member(sel) == Some(Member::Sizeof) {
                size_of(cx.type_table(), t)
            } else {
                align_of(cx.type_table(), t)
            };
            let uintptr = cx.tt_mut().add_basic(BasicId::Uintptr);
            Operand::constant(
                uintptr,
                v.map_or(Const::Unknown, |n| Const::Int(BigInt::from_i64(n as i64))),
            )
        }
        Some(Member::Offsetof) => {
            if c.args.len() != 1 || c.has_dots {
                cx.report(
                    c.rparen,
                    DiagCode::ChkUnsupportedUnsafe,
                    format!(
                        "invalid operation: Offsetof expects 1 argument, found {}",
                        c.args.len()
                    ),
                );
                return Operand::invalid();
            }
            let v = offset_of(cx, file, &c.args[0]);
            let uintptr = cx.tt_mut().add_basic(BasicId::Uintptr);
            Operand::constant(
                uintptr,
                v.map_or(Const::Unknown, |n| Const::Int(BigInt::from_i64(n as i64))),
            )
        }
        None => {
            cx.report(
                sel_pos,
                DiagCode::ChkUnsupportedUnsafe,
                format!(
                    "unsafe.{sel} is outside the M4 whitelist (only Pointer/Sizeof/Alignof/Offsetof are supported; U-M4-06)"
                ),
            );
            Operand::invalid()
        }
    }
}

/// 白名单外成员的统一拒绝（选择器面）。
fn reject(cx: &mut ExprChecker<'_, '_>, e: &SelectorExpr, sel: &str) {
    cx.report(
        e.sel.pos,
        DiagCode::ChkUnsupportedUnsafe,
        format!(
            "unsafe.{sel} is outside the M4 whitelist (only Pointer/Sizeof/Alignof/Offsetof are supported; U-M4-06)"
        ),
    );
}

// ────────────────────────────── 尺寸 ──────────────────────────────

/// 官方 `stdSizes.Sizeof`：字节数；判不了 ⇒ `None`。
#[must_use]
pub(crate) fn size_of(tt: &TypeTable, t: TypeId) -> Option<u64> {
    let t = underlying(tt, t);
    match tt.type_of(t) {
        Type::Basic(b) => basic_size(*b),
        // 指针 / map / chan / 函数值：一个机器字。
        Type::Pointer(_) | Type::Map { .. } | Type::Chan { .. } | Type::Signature(_) => Some(WORD),
        // `string` = 数据指针 + 长度。
        Type::Slice(_) => Some(3 * WORD),
        Type::Interface(_) => Some(2 * WORD),
        Type::Array { len, elem } => {
            let n = arr_len(tt, *len)?;
            Some(n * size_of(tt, *elem)?)
        }
        Type::Struct(sid) => {
            let def = tt.struct_def(*sid);
            let mut off = 0u64;
            let mut max_align = 1u64;
            for f in &def.fields {
                let a = align_of(tt, f.typ)?;
                let s = size_of(tt, f.typ)?;
                max_align = max_align.max(a);
                off = round_up(off, a);
                off += s;
            }
            Some(round_up(off, max_align.min(MAX_ALIGN)))
        }
        Type::Named(_)
        | Type::Alias(_)
        | Type::Tuple(_)
        | Type::TypeParam(_)
        | Type::Union(_)
        | Type::Invalid => None,
    }
}

/// 官方 `stdSizes.Alignof`。
#[must_use]
pub(crate) fn align_of(tt: &TypeTable, t: TypeId) -> Option<u64> {
    let t = underlying(tt, t);
    let a = match tt.type_of(t) {
        Type::Basic(b) => basic_size(*b)?,
        Type::Pointer(_) | Type::Map { .. } | Type::Chan { .. } | Type::Signature(_) => WORD,
        Type::Slice(_) => WORD,
        Type::Interface(_) => WORD,
        Type::Array { elem, .. } => align_of(tt, *elem)?,
        Type::Struct(sid) => {
            let def = tt.struct_def(*sid);
            def.fields
                .iter()
                .map(|f| align_of(tt, f.typ))
                .try_fold(1u64, |acc, a| a.map(|a| acc.max(a)))?
        }
        Type::Named(_)
        | Type::Alias(_)
        | Type::Tuple(_)
        | Type::TypeParam(_)
        | Type::Union(_)
        | Type::Invalid => return None,
    };
    Some(a.min(MAX_ALIGN))
}

/// 基本类型的字节数（`bool` 1、`string` 16、`unsafe.Pointer` 8）。
fn basic_size(b: BasicId) -> Option<u64> {
    match b {
        BasicId::Bool => Some(1),
        BasicId::String => Some(2 * WORD),
        BasicId::UnsafePointer => Some(WORD),
        _ => Sizes::HOST_ARM64
            .width_of(b)
            .map(|bits| u64::from(bits) / 8),
    }
}

/// `unsafe.Offsetof(s.f)` 的偏移（**只支持单层字段**；多级 / 提升 ⇒ `None`）。
///
/// 实参本身也过一遍表达式检查 —— `unsafe.Offsetof(nope)` 的
/// `undefined: nope` 归表达式面，本函数只回答「偏移是多少」。
fn offset_of(cx: &mut ExprChecker<'_, '_>, file: FileId, e: &Expr) -> Option<u64> {
    let Expr::Selector(s) = e else { return None };
    let x = cx.expr(file, &s.x);
    if x.mode == crate::operand::Mode::Invalid {
        return None;
    }
    let t = x.typ?;
    let tt = cx.type_table();
    let Type::Struct(sid) = tt.type_of(underlying(tt, t)) else {
        return None;
    };
    let mut off = 0u64;
    for f in &tt.struct_def(*sid).fields {
        let a = align_of(tt, f.typ)?;
        let sz = size_of(tt, f.typ)?;
        off = round_up(off, a);
        if f.name.as_deref() == Some(s.sel.value.as_str()) {
            return Some(off);
        }
        off += sz;
    }
    None
}

fn round_up(v: u64, align: u64) -> u64 {
    if align <= 1 {
        v
    } else {
        v.div_ceil(align) * align
    }
}

fn arr_len(tt: &TypeTable, len: Option<rgoc_types::ConstId>) -> Option<u64> {
    match len.map(|c| tt.consts().get(c)) {
        Some(Const::Int(v)) => v.to_i64().and_then(|n| u64::try_from(n).ok()),
        _ => None,
    }
}
