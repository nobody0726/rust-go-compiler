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

use rgoc_types::TypeId;

use crate::ir::{Block, BlockId, Instr, Module, Root, RootId, SsaFunc, Term, ValueId};

/// 一个最小的 `SsaFunc` 构造器。
#[derive(Debug)]
pub struct FnBuilder {
    name: String,
    blocks: Vec<Block>,
    roots: Vec<Root>,
    value_types: Vec<TypeId>,
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
    pub fn value(&mut self, typ: TypeId) -> ValueId {
        self.value_types.push(typ);
        ValueId((self.value_types.len() - 1) as u32)
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

    /// 取出函数。
    #[must_use]
    pub fn finish(self) -> SsaFunc {
        SsaFunc {
            src: rgoc_hir::FuncId(0),
            name: self.name,
            params: Vec::new(),
            blocks: self.blocks,
            entry: BlockId(0),
            roots: self.roots,
            slot_root: std::collections::BTreeMap::new(),
            value_types: self.value_types,
        }
    }
}

/// 把一批函数包成一个模块（多函数场景的便捷入口）。
#[must_use]
pub fn module_of(funcs: Vec<SsaFunc>) -> Module {
    Module {
        funcs,
        globals: Vec::new(),
        main: None,
    }
}
