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
//! Task 21 = [`version`]（版本门控），Task 22 = [`validtype`]（非法递归 / 嵌套深度），
//! **Task 23 = [`scope`] 改竞技场**（`ScopeId` / `ScopeTable` / `ScopeRef`，
//! 见该模块文档的 Ruling 23-1）。
//!
//! M4 逐任务落地：Task 6 = [`assignable`]（可赋值性；窄面与拒绝码见该模块文档），
//! Task 7 = [`methodset`]（方法集；生产者 = `rgoc-load` 的接收者挂载），
//! Task 8 = [`typeset`]（语义类型集四态）/ [`comparable`]（可比性）/
//! [`implements`]（接口实现与约束满足），
//! Task 12 = [`instantiate`]（显式实例化）/ [`subst`]（类型参数替换），
//! Task 13 = [`unify`]（统一：`Unifier` 句柄机制 + 官方 `nify` 结构 +
//! `commonUnder` 核心类型；三模式见该模块文档）。
//!
//! [`typ`]: crate::typ
//! [`table`]: crate::table
//! [`universe`]: crate::universe
//! [`object`]: crate::object
//! [`scope`]: crate::scope
//! [`dump`]: crate::dump
//! [`identical`]: crate::identical
//! [`assignable`]: crate::assignable
//! [`methodset`]: crate::methodset
//! [`unify`]: crate::unify
//! [`validtype`]: crate::validtype
//! [`version`]: crate::version

pub use rgoc_const::{BasicId, ConstId, ConstTable, Sizes};

pub mod assignable;
pub mod comparable;
pub mod convertible;
pub mod dump;
pub mod identical;
pub mod implements;
pub mod infer;
pub mod instantiate;
pub mod layout;
pub mod methodset;
pub mod object;
pub mod scope;
pub mod subst;
pub mod table;
pub mod typ;
pub mod typeset;
pub mod unify;
pub mod universe;
pub mod validtype;
pub mod version;

pub use assignable::assignable;
pub use comparable::{comparable, strictly_comparable};
pub use convertible::convertible;
pub use dump::{DumpError, dump_types, try_dump_types};
pub use identical::{identical, identical_ignore_tags, unalias, underlying};
pub use implements::implements;
pub use infer::{
    GenericArgSite, infer_for_call, infer_from_args, infer_from_assignment, rename_tparams,
};
pub use instantiate::{MAX_INSTANTIATION_DEPTH, instantiate};
pub use layout::{Layout, field_offset, field_offsets, layout, ptr_mask};
pub use methodset::{MSetEntry, methodset};
pub use object::{ObjKind, Object};
pub use scope::{Scope, ScopeId, ScopeKind, ScopeRef, ScopeTable};
pub use subst::{SubstMap, substitute};
pub use table::{
    AliasDef, Field, InterfaceDef, NamedDef, SignatureDef, StructDef, TupleDef, TypeParamDef,
    TypeTable, UnionDef,
};
pub use typ::{
    AliasId, ChanDir, InterfaceId, NamedId, SignatureId, StructId, TYPE_VARIANT_COUNT, TupleId,
    Type, TypeId, TypeParamId, UnionId,
};
pub use typeset::IfaceTSet;
pub use unify::{Unifier, UnifyMode, unify};
pub use universe::{declare_universe, declared_names, universe};
pub use validtype::{MAX_TYPE_NEST_DEPTH, valid_type};
pub use version::{GO_CURRENT, GO1_21, GoVersion, Versions, pkg_versions};
