//! 手写 IR 的构造工具（**只给测试用**；`D-M6-11` 的地基）。
//!
//! # 它故意**不做任何检查**
//!
//! verifier 的反例必须手工造非法图。工具一旦顺手「纠正」输入
//! （去重 `ValueId`、补齐块参数、拒绝不支配的使用），反例就造不出来，
//! `verify_negative.rs` 会退化成「用合法图证明 verifier 会拒」——
//! 永远绿，且没人发现。
//!
//! 所以这里只做三件事：**分配 id、按我给的字面内容落盘、把 `SsaFunc` 交出去**。
//! 值个数与 `value_types` 的长度由 `value()` 的调用次数决定（不猜、不补）。
//!
//! # 为什么放在 crate 内而不是 `tests/` 目录下
//!
//! 集成测试（`tests/*.rs`）是两个 crate，共享辅助代码只能靠 `pub` 模块或
//! `include!`。前者可读性更好，且这份工具本身就需要被测（见 `tests/testsupport.rs`）。
//! 代价：它进了公开 API —— 文件头的「**只给测试用**」是纪律，不是编译器保证。

use std::collections::BTreeMap;

use rgoc_types::{TypeId, TypeTable};

use crate::ir::{Block, BlockId, Instr, Module, Root, RootId, Sig, SsaFunc, Term, ValueId};

/// 一个最小的 `SsaFunc` 构造器。
#[derive(Debug)]
pub struct FnBuilder {
    name: String,
    blocks: Vec<Block>,
    roots: Vec<Root>,
    value_types: Vec<TypeId>,
    /// 类型表（`V3` 要按 `TypeId` 取真实类型；`universe()` 自带 22 个基本类型）。
    tt: TypeTable,
    /// 基本类型名的缓存 —— `add_basic` **不去重**，每次调用会新开一个槽。
    basics: BTreeMap<String, TypeId>,
    /// 被调方签名（`V5` 的比对面）。
    call_sigs: BTreeMap<rgoc_hir::FuncId, Sig>,
    /// 取过址的槽 → 根（`V9` 的比对面）。
    slot_root: BTreeMap<rgoc_hir::SlotId, RootId>,
    /// 取过址的槽 → 类型（求值器进帧时按它建零值存储）。
    slot_types: BTreeMap<rgoc_hir::SlotId, TypeId>,
    /// `elem → *elem`（`ptr_to` 的缓存，避免同一元素建出多个指针类型）。
    ptrs: BTreeMap<TypeId, TypeId>,
    /// `(len, elem) → [len]elem`（`array_of` 的缓存）。
    arrays: BTreeMap<(i64, TypeId), TypeId>,
}

impl FnBuilder {
    /// 建一个只有入口块的空函数（入口块终止符默认 `Unreachable`，可覆盖）。
    #[must_use]
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            blocks: vec![Block::default()],
            roots: Vec::new(),
            value_types: Vec::new(),
            tt: rgoc_types::universe::universe().2,
            basics: BTreeMap::new(),
            call_sigs: BTreeMap::new(),
            slot_root: BTreeMap::new(),
            slot_types: BTreeMap::new(),
            ptrs: BTreeMap::new(),
            arrays: BTreeMap::new(),
        }
    }

    /// 建一个**空类型表**的构建器（`t0` 从 0 开始）。
    ///
    /// 只给「要逐字节对 `C10` 示例」的 dump 测试用：`new()` 走 `universe()`，
    /// 表里已经有 22 个基本类型，`bool` 会是 `t22` 而不是 `t0`。
    /// 别的测试别用它 —— 类型编号不可读会让人对着 dump 数错。
    #[must_use]
    pub fn bare(name: &str) -> Self {
        Self {
            name: name.to_string(),
            blocks: vec![Block::default()],
            roots: Vec::new(),
            value_types: Vec::new(),
            tt: TypeTable::new(),
            basics: BTreeMap::new(),
            call_sigs: BTreeMap::new(),
            slot_root: BTreeMap::new(),
            slot_types: BTreeMap::new(),
            ptrs: BTreeMap::new(),
            arrays: BTreeMap::new(),
        }
    }

    /// 入口块。
    #[must_use]
    pub fn entry(&self) -> BlockId {
        BlockId(0)
    }

    /// 追加一个块并返回它的 id（**总是**新建，不复用）。
    pub fn block(&mut self) -> BlockId {
        self.blocks.push(Block::default());
        BlockId((self.blocks.len() - 1) as u32)
    }

    /// 分配一个值（类型进 `value_types`，编号按调用顺序）。
    ///
    /// **只查一件事**：`typ` 必须在本构建器的类型表里（否则 `V3` 会拿到越界 id
    /// 而不是「类型写错了」）。IR 的合法性仍然一概不查。
    pub fn value(&mut self, typ: TypeId) -> ValueId {
        assert!(
            typ.index() < self.tt.len(),
            "TypeId({}) 不在本构建器的类型表里（表里只有 {} 项）",
            typ.index(),
            self.tt.len()
        );
        self.value_types.push(typ);
        ValueId((self.value_types.len() - 1) as u32)
    }

    /// 取基本类型（按名字，**同一次构建里复用同一个 `TypeId`**）。
    ///
    /// # Panics
    ///
    /// 名字不是基本类型名（`int` / `string` / `bool` / …）时 panic —— 拼错类型名
    /// 的测试应该立刻炸，而不是拿到一个 `None` 再推理。
    pub fn basic(&mut self, name: &str) -> TypeId {
        if let Some(&id) = self.basics.get(name) {
            return id;
        }
        let b = rgoc_types::universe::basic_name_to_id(name)
            .unwrap_or_else(|| panic!("`{name}` 不是基本类型名"));
        let id = self.tt.add_basic(b);
        self.basics.insert(name.to_string(), id);
        id
    }

    /// 取一个指针类型 `*elem`（同一个 `elem` 复用同一个 `TypeId`）。
    ///
    /// 活跃性的 `ptrs` 只收指针类型（`live.rs` 模块头），所以测试必须造得出
    /// 指针 —— `basic()` 只给基本类型。
    pub fn ptr_to(&mut self, elem: TypeId) -> TypeId {
        if let Some(&id) = self.ptrs.get(&elem) {
            return id;
        }
        let id = self.tt.add_pointer(elem);
        self.ptrs.insert(elem, id);
        id
    }

    /// 取一个数组类型 `[len]elem`（同一个 `(len, elem)` 复用同一个 `TypeId`）。
    ///
    /// 活跃性的 `ptrs` / 派生指针判据要真实类型（`Index` 的底座是
    /// `*[N]T`，元素地址是 `*T`），`basic()` 与 `ptr_to()` 造不出来。
    pub fn array_of(&mut self, len: i64, elem: TypeId) -> TypeId {
        if let Some(&id) = self.arrays.get(&(len, elem)) {
            return id;
        }
        let c = self
            .tt
            .consts_mut()
            .add(rgoc_const::Const::Int(rgoc_const::BigInt::from_i64(len)));
        let id = self.tt.add_array(Some(c), elem);
        self.arrays.insert((len, elem), id);
        id
    }

    /// 取一个结构体类型（字段名 + 类型；同一次构建里**总是新建**）。
    pub fn struct_of(&mut self, fields: &[(&str, TypeId)]) -> TypeId {
        let fs: Vec<rgoc_types::table::Field> = fields
            .iter()
            .map(|(n, t)| rgoc_types::table::Field {
                name: Some((*n).to_string()),
                typ: *t,
                tag: None,
                embedded: false,
            })
            .collect();
        let sid = self.tt.add_struct(fs);
        self.tt.struct_type(sid)
    }

    /// 登记「这个槽取过址、它的根是 `root`」（`V9` 要查）。
    pub fn slot_root(&mut self, slot: rgoc_hir::SlotId, root: RootId) {
        self.slot_root.insert(slot, root);
    }

    /// 登记「这个槽的类型」（求值器要按它建根存储的零值）。
    ///
    /// 不登记 ⇒ 求值器把根存储建成 `nil`：`Store` 整块覆盖仍然对，
    /// 但 `Index`/`Field` 的**取址**会当场失败（那才是该失败的时候）。
    pub fn slot_typ(&mut self, slot: rgoc_hir::SlotId, typ: TypeId) {
        self.slot_types.insert(slot, typ);
    }

    /// 登记一个被调方的签名（`V5` 要查；不登记就等于「查不了」）。
    pub fn call_sig(&mut self, f: rgoc_hir::FuncId, params: usize, results: usize, variadic: bool) {
        self.call_sigs.insert(
            f,
            Sig {
                params,
                results,
                variadic,
            },
        );
    }

    /// 本构建器的类型表（`verify` 的第二个参数）。
    #[must_use]
    pub fn tt(&self) -> &TypeTable {
        &self.tt
    }

    /// 登记一个内存根。
    pub fn root(&mut self, r: Root) -> RootId {
        self.roots.push(r);
        RootId((self.roots.len() - 1) as u32)
    }

    /// 设某块的块参数（**原样**落盘，不校验与任何边是否匹配）。
    pub fn set_params(&mut self, b: BlockId, params: &[ValueId]) {
        self.blocks[b.index()].params = params.to_vec();
    }

    /// 往某块追加一条指令（**不**检查 `dst` 是否已被定义过）。
    pub fn instr(&mut self, b: BlockId, i: Instr) {
        self.blocks[b.index()].instrs.push(i);
    }

    /// 设某块的终止符（后写覆盖先写）。
    pub fn term(&mut self, b: BlockId, t: Term) {
        self.blocks[b.index()].term = t;
    }

    /// 取出函数与其类型表（需要 `verify` 时用这个）。
    #[must_use]
    pub fn into_parts(self) -> (SsaFunc, TypeTable) {
        let (f, tt) = self.finish_inner();
        (f, tt)
    }

    /// 取出函数（丢掉类型表 —— 只做结构检查的测试用它）。
    #[must_use]
    pub fn finish(self) -> SsaFunc {
        self.finish_inner().0
    }

    fn finish_inner(self) -> (SsaFunc, TypeTable) {
        let tt = self.tt;
        let f = SsaFunc {
            captured: Vec::new(),
            ctx_abi: false,
            src: rgoc_hir::FuncId(0),
            name: self.name,
            params: Vec::new(),
            blocks: self.blocks,
            entry: BlockId(0),
            roots: self.roots,
            slot_root: self.slot_root,
            slot_types: self.slot_types,
            value_types: self.value_types,
            call_sigs: self.call_sigs,
        };
        (f, tt)
    }
}

/// 把一批函数包成一个模块（多函数场景的便捷入口）。
#[must_use]
pub fn module_of(funcs: Vec<SsaFunc>) -> Module {
    Module {
        methods: Vec::new(),
        itabs: Vec::new(),
        funcs,
        globals: Vec::new(),
        global_types: Vec::new(),
        main: None,
        init: None,
    }
}
