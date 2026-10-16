//! `rgoc-types` —— 类型表示、同一性与按文件的版本门控（M3）。
//!
//! # 位置与依赖方向
//!
//! ```text
//! rgoc-lex ◄── rgoc-ast ◄── rgoc-const ◄── rgoc-types ◄── rgoc-load
//! ```
//!
//! 单向，无环。本 crate 吃 [`rgoc_const`] 的常量与 [`rgoc_lex`] 的诊断/位置。
//!
//! # `BasicId` / `Sizes` 为什么是**转导出**
//!
//! 它们的**定义**在 [`rgoc_const`]（那里是 `convert_const` / `representable`
//! 的家乡，解环见该 crate 的模块文档）。这里转导出是为了让下游（`rgoc-load`、
//! `rgoc-driver`）能用 `rgoc_types::BasicId` 这个更顺手的路径 ——
//! **不是**第二份定义。
//!
//! # 状态
//!
//! M3 逐任务落地：Task 13 = [`typ`]（`Type` 的 15 个变体），
//! Task 14 = [`table`]（arena），Task 15 = [`universe`] / [`object`] / [`scope`]，
//! Task 16 = [`dump`]（C7），Task 17/18 = [`identical`]（同一性判定），
//! Task 21 = [`version`]（版本门控），Task 22 = [`validtype`]（非法递归 / 嵌套深度）。
//!
//! [`typ`]: crate::typ
//! [`table`]: crate::table
//! [`universe`]: crate::universe
//! [`object`]: crate::object
//! [`scope`]: crate::scope
//! [`dump`]: crate::dump
//! [`identical`]: crate::identical
//! [`validtype`]: crate::validtype
//! [`version`]: crate::version

pub use rgoc_const::{BasicId, ConstId, ConstTable, Sizes};

pub mod dump;
pub mod identical;
pub mod object;
pub mod scope;
pub mod table;
pub mod typ;
pub mod universe;
pub mod validtype;
pub mod version;

pub use dump::dump_types;
pub use identical::{identical, unalias};
pub use object::{ObjKind, Object};
pub use scope::Scope;
pub use table::{
    AliasDef, Field, InterfaceDef, NamedDef, SignatureDef, StructDef, TupleDef, TypeParamDef,
    TypeTable, UnionDef,
};
pub use typ::{
    AliasId, ChanDir, InterfaceId, NamedId, SignatureId, StructId, TYPE_VARIANT_COUNT, TupleId,
    Type, TypeId, TypeParamId, UnionId,
};
pub use universe::{declare_universe, declared_names, universe};
pub use validtype::{MAX_TYPE_NEST_DEPTH, valid_type};
pub use version::{GO_CURRENT, GO1_21, GoVersion, Versions, pkg_versions};
