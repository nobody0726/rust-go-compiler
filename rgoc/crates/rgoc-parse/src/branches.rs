//! `checkBranches` —— 标签与分支语句的**语义**检查（`T-AP16`，`T-A-80`/`81`/`82`）。
//!
//! 官方 `branches.go` 全文 339 行，本文件是它的直译。四条检查：
//!
//! 1. **错位的 `break` / `continue` / `fallthrough`**（`branches.go:210-247`）
//! 2. **带标签的 `break` / `continue`**（`:249-292`）
//! 3. **非法 / 未使用 / 重复 / 缺失的标签**（`:28-44` / `declare` / `gotoTarget`）
//! 4. **`goto` 跳过变量声明 / 跳进块**（`:28-36` / `:180-205`）
//!
//! # 与解析的边界：只在**这个函数没有语法错**时才检查
//!
//! 官方 `funcBody`（`parser.go:845-859`）：
//!
//! ```text
//! errcnt := p.errcnt
//! body := p.blockStmt("")
//! p.fnest--
//! if p.mode&CheckBranches != 0 && errcnt == p.errcnt {
//!     checkBranches(body, p.errh)
//! }
//! ```
//!
//! 官方注释写明理由：语法错会让树**不完整**，此时跑分支检查会
//! **报出虚假错误**（官方举例 `test/switch2.go`）甚至**崩在残缺树上**。
//!
//! ⚠️ `errcnt == p.errcnt` 这个条件**不是**「整个文件没报错」而是
//! 「**这个函数体内**没报错」—— 它比的是**进 `funcBody` 之前**的计数。
//!
//! # 为什么用**身份下标**而不是 `Rc`（官方是指针）
//!
//! 官方的 `label.parent` / `label.lstmt` / `block.lstmt` 都是**指针**，
//! `gotoTarget` / `enclosingTarget` 靠**指针相等**判「同一个块 / 同一个
//! 带标签语句」。Rust 侧我们给每个 `block` 与每个 `LabeledStmt` 各分配一个
//! `u32`，比较时比下标。
//!
//! 为什么不用 `Rc`：`Stmt` 需要 `Clone + PartialEq`（`test_model.rs` 逐字段断言、
//! `dump` 要能克隆），而 `Rc<T>` 的 `PartialEq` 是**浅比较**（比指针地址）——
//! 一棵有两条路径引用同一节点的树会被判为「不相等」，`assert_eq!` 直接红。
//!
//! # 刻意**不**建模的：官方的 `BranchStmt.Target`
//!
//! 官方 `nodes.go:407` 有 `Target Stmt`，由 `checkBranches` 回填。
//! 我们**不建模**它（`rgoc-ast/src/stmt.rs` 的 `BranchStmt` 文档已注明）。
//! 理由：① 它是**回指**（指向树里别处的节点），建模成 `Rc` 会破坏 `PartialEq`
//! 的深比较语义；② 它**不参与** dump（`C6` 规则 4：位置与派生字段不进 dump）；
//! ③ 它只被后续的**类型检查阶段**读，M2 用不到。
//! ⇒ 本文件的 `gotoTarget` / `enclosingTarget` 只返回「是否命中」，
//! **不回填**任何字段。

use std::collections::HashMap;

use rgoc_ast::{
    AssignOp, AssignStmt, BlockStmt, BranchStmt, BranchTok, Decl, DeclStmt, Expr, ForStmt,
    LabeledStmt, Stmt, SwitchStmt,
};
use rgoc_lex::diag::DiagCode;
use rgoc_lex::pos::Pos;

use crate::parser::Parser;
use crate::text::expr_text;

/// 官方 `label`（`branches.go:52-56`）。
#[derive(Clone)]
struct Label {
    /// 包含这条标签声明的块。
    parent: BlockId,
    /// 声明这条标签的语句。
    lstmt: LabeledId,
    /// 是否被用到。
    used: bool,
}

/// 官方 `block`（`branches.go:58-62`）。
struct Block {
    /// 紧邻的外层块（顶层为 `None`）。
    parent: Option<BlockId>,
    /// 块起点（官方 `start`）。
    start: Pos,
    /// 与本块关联的带标签语句（无则 `None`）。
    lstmt: Option<LabeledId>,
}

/// 块的**身份**（官方是 `*block`）。
type BlockId = u32;
/// 带标签语句的**身份**（官方是 `*LabeledStmt`）。
type LabeledId = u32;

/// 官方 `targets.breaks`（`branches.go:122`）—— `*ForStmt` / `*SwitchStmt` /
/// `*SelectStmt` / `nil` 的和类型版本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TargetKind {
    For,
    Switch,
    Select,
}

/// 官方 `targets`（`branches.go:119-125`）。
#[derive(Clone, Copy, Debug)]
struct Targets {
    /// `break` 的目标（`None` ⇒ 不在任何可 `break` 的语句里）。
    breaks: Option<TargetKind>,
    /// `continue` 的目标（官方只有 `*ForStmt`，但字段类型一样是 `Stmt`）。
    continues: Option<TargetKind>,
    /// 紧邻外层 `switch` 里的 `case` 下标；`< 0` 表示不在 `case` 体里。
    case_index: i32,
    /// 外层 `switch` 的 `case` **总**数（官方靠 `len(t.Body)` 取，我们没回指 `switch` 节点）。
    case_total: usize,
    /// 外层 `switch` 是不是**类型** switch（官方判 `t.Tag.(*TypeSwitchGuard)`）。
    is_type_switch: bool,
}

impl Targets {
    /// 官方 `targets{ctxt.breaks, ctxt.continues, -1}`（`:300` / `:304` / `:321`）。
    fn passthrough(breaks: Option<TargetKind>, continues: Option<TargetKind>) -> Self {
        Self {
            breaks,
            continues,
            case_index: -1,
            case_total: 0,
            is_type_switch: false,
        }
    }
}

/// 官方顶层的 `targets{}`（`:23`）—— 什么都不合法。
const TOP: Targets = Targets {
    breaks: None,
    continues: None,
    case_index: -1,
    case_total: 0,
    is_type_switch: false,
};

/// 一条待发的分支检查诊断。
///
/// 官方 `ls.errf` 直接调 `errh`；我们让 [`LabelScope::block_branches`] **收集**
/// 成 `Vec` 再一次性发出，理由见 [`LabelScope`] 的 `deferred` 字段文档。
struct BranchErr {
    pos: Pos,
    code: DiagCode,
    msg: String,
}

/// 一条带标签语句的存档：名字 + 声明位置 + 它标注的语句是**哪一类**。
///
/// # 为什么需要 `kind`
///
/// 官方 `break` / `continue` 的标签检查（`:256-262` / `:270-276`）要读
/// `t.Stmt` 的**具体类型**：只有 `*SwitchStmt` / `*SelectStmt` / `*ForStmt`
/// 才是合法的 `break` 目标，只有 `*ForStmt` 才是合法的 `continue` 目标。
/// 别的（`*BlockStmt` / `*IfStmt` / …）报 `invalid break label`。
///
/// 我们不回指 AST 节点（那要 `Rc`，见模块文档）⇒ 在**登记标签时**
/// 顺手记下形状。
#[derive(Clone, Copy, PartialEq, Eq)]
enum LabeledKind {
    /// 标注的是 `for`（`break`/`continue` 都合法）。
    For,
    /// 标注的是 `switch`（`break` 合法、`continue` 不合法）。
    Switch,
    /// 标注的是 `select`（`break` 合法、`continue` 不合法）。
    Select,
    /// 标注的是别的（块 / `if` / 表达式 …）—— 两条标签都非法。
    Other,
}

impl LabeledKind {
    /// 官方 `:258` 的 `case *SwitchStmt, *SelectStmt, *ForStmt`。
    fn is_break_target(self) -> bool {
        matches!(
            self,
            LabeledKind::For | LabeledKind::Switch | LabeledKind::Select
        )
    }

    /// 官方 `:272` 的 `case *ForStmt`。
    fn is_continue_target(self) -> bool {
        matches!(self, LabeledKind::For)
    }
}

/// 官方 `labelScope`（`branches.go:47-50`）。
struct LabelScope {
    /// 全部标签声明（官方 `map[string]*label`，**懒分配**）。
    labels: HashMap<String, Label>,
    /// 块的**身份表**（官方是 `block` 的树形链；Rust 侧按分配顺序平铺 + `parent` 下标）。
    blocks: Vec<Block>,
    /// 带标签语句的**身份表**。
    labeled: Vec<LabeledSlot>,
    /// 待发的诊断。
    ///
    /// # 为什么**收集**而不是立刻发
    ///
    /// 官方 `ls.errf` 直接调 `p.errh`，而我们的 `blockBranches` 同时要
    /// `&mut self`（改 `labels[].used` / `blocks`）与 `&mut Parser`（发诊断）。
    /// 两个可变借用不能同时持有 ⇒ 收集起来，在 `check_branches` 末尾统一发。
    /// 顺序与官方一致（都是遍历顺序）。
    deferred: Vec<BranchErr>,
}

/// 带标签语句的存档。
struct LabeledSlot {
    name: String,
    pos: Pos,
    kind: LabeledKind,
}

impl LabelScope {
    fn new() -> Self {
        Self {
            labels: HashMap::new(),
            blocks: Vec::new(),
            labeled: Vec::new(),
            deferred: Vec::new(),
        }
    }

    fn errf(&mut self, pos: Pos, code: DiagCode, msg: String) {
        self.deferred.push(BranchErr { pos, code, msg });
    }

    /// 官方 `declare`（`branches.go:71-84`）。
    ///
    /// 标签已定义时报一条 `label %s already defined at %s` 并**不覆盖**已有条目
    /// （官方 `return alt`）—— 于是后面 `l.used = true` 标的是**第一条**声明。
    fn declare(&mut self, p: &Parser, b: BlockId, s: LabeledId) {
        let slot = &self.labeled[s as usize];
        let (name, pos) = (slot.name.clone(), slot.pos);
        if let Some(alt) = self.labels.get(&name).cloned() {
            let alt_pos = self.labeled[alt.lstmt as usize].pos;
            let alt_str = p.pos_str(alt_pos);
            self.errf(
                pos,
                DiagCode::SynLabelDuplicate,
                format!("label {name} already defined at {alt_str}"),
            );
            return; // 官方 `return alt` ⇒ 不覆盖已有条目
        }
        self.labels.insert(
            name,
            Label {
                parent: b,
                lstmt: s,
                used: false,
            },
        );
    }

    /// 官方 `gotoTarget`（`branches.go:89-99`）：标签声明在 `b` 或其**外层**块里。
    fn goto_target(&mut self, b: Option<BlockId>, name: &str) -> bool {
        let Some(l) = self.labels.get(name).cloned() else {
            return false;
        };
        // 官方 `l.used = true // even if it's not a valid target`
        if let Some(slot) = self.labels.get_mut(name) {
            slot.used = true;
        }
        let mut cur = b;
        while let Some(id) = cur {
            if id == l.parent {
                return true;
            }
            cur = self.blocks[id as usize].parent;
        }
        false
    }

    /// 官方 `enclosingTarget`（`branches.go:106-117`）：标签**标注的**语句是否在 `b` 或其外层。
    ///
    /// 四态对应官方的「`nil` / `*LabeledStmt` / `invalid`」加上「找到了但形状不对」：
    ///
    /// - `None` —— 标签未定义（官方 `return nil`）
    /// - `Some(k)` —— 找到了，附带那个语句的形状（官方 `return l.lstmt`）
    /// - `Some(LabeledKind::Other)` —— 标签已定义、标注的语句在链上，
    ///   但**不是**合法的跳转目标（官方那个 `invalid` 单例）
    fn enclosing_target(&mut self, b: Option<BlockId>, name: &str) -> Option<LabeledKind> {
        let l = self.labels.get(name).cloned()?;
        // 官方 `l.used = true // even if it's not a valid target (see e.g. bug136.go)`
        if let Some(slot) = self.labels.get_mut(name) {
            slot.used = true;
        }
        let mut cur = b;
        while let Some(id) = cur {
            if self.blocks[id as usize].lstmt == Some(l.lstmt) {
                return Some(self.labeled[l.lstmt as usize].kind);
            }
            cur = self.blocks[id as usize].parent;
        }
        Some(LabeledKind::Other)
    }

    /// 官方 `checkBranches`（`branches.go:16-45`）。
    fn check(&mut self, p: &Parser, body: &BlockStmt) {
        let fwd_gotos = self.block_branches(p, None, TOP, None, body.pos, &body.stmts);

        // 官方 `:28-36`：剩下的前向 `goto` 都找不到标签 ——
        // 要么标签从未定义，要么它在**块内部**、跳不进去。
        for id in fwd_gotos {
            let slot = &self.labeled[id as usize];
            let (name, pos) = (slot.name.clone(), slot.pos);
            match self.labels.get(&name).cloned() {
                Some(l) => {
                    // 避免多报一条「defined and not used」（官方 `l.used = true`）
                    if let Some(slot) = self.labels.get_mut(&name) {
                        slot.used = true;
                    }
                    let start = self.blocks[l.parent as usize].start;
                    let start_str = p.pos_str(start);
                    self.errf(
                        pos,
                        DiagCode::SynGotoIntoBlock,
                        format!("goto {name} jumps into block starting at {start_str}"),
                    );
                }
                None => {
                    self.errf(
                        pos,
                        DiagCode::SynLabelUndefined,
                        format!("label {name} not defined"),
                    );
                }
            }
        }

        // 官方 `:38-44`：spec「It is illegal to define a label that is never used.」
        //
        // ⚠️ 遍历顺序：官方遍历 Go `map`（**随机序**）⇒ 诊断顺序不确定。
        // 我们按**声明顺序**（`labeled` 表下标）遍历 ⇒ 顺序确定，
        // 这让 `C1` §4.3 的「同位置多条诊断的顺序确定」与 `E6` 的可重复执行成立。
        // 对撞时按**消息**匹配（`F11`），不按序 ⇒ 不受影响。
        //
        // ⚠️ `l.lstmt == id` 这个条件照应官方 `labels` 表的**值**而不是键：
        // 重复声明时 `declare` 不覆盖 ⇒ 第二次声明的 `id` 不在 `labels` 里
        // ⇒ 不再报一次「defined and not used」（那一条由「already defined」负责）。
        for id in 0..self.labeled.len() as LabeledId {
            let slot = &self.labeled[id as usize];
            let (name, pos) = (slot.name.clone(), slot.pos);
            let Some(l) = self.labels.get(&name).cloned() else {
                continue; // 这个名字登记的条目被别的同名标签占了（重复声明）
            };
            if l.lstmt == id && !l.used {
                self.errf(
                    pos,
                    DiagCode::SynLabelUnused,
                    format!("label {name} defined and not used"),
                );
            }
        }
    }

    /// 官方 `blockBranches`（`branches.go:131-330`），返回仍未解决的**前向** `goto`。
    ///
    /// 参数与官方一一对应：`parent` = 紧邻外层块、`ctxt` = 外层语句提供的
    /// `break`/`continue` 上下文、`lstmt` = 与本块关联的带标签语句、
    /// `start` = 块起点、`body` = 语句表。
    fn block_branches(
        &mut self,
        p: &Parser,
        parent: Option<BlockId>,
        ctxt: Targets,
        lstmt: Option<LabeledId>,
        start: Pos,
        body: &[Stmt],
    ) -> Vec<LabeledId> {
        let b = self.blocks.len() as BlockId;
        self.blocks.push(Block {
            parent,
            start,
            lstmt,
        });

        let mut var_pos: Option<Pos> = None;
        let mut var_name: String = String::new();
        let mut fwd_gotos: Vec<LabeledId> = Vec::new();
        let mut bad_gotos: Vec<LabeledId> = Vec::new();

        // 官方 `recordVarDecl`（`:138-146`）：记住变量声明，
        // 并把**当前所有**前向 `goto` 记成「可能非法」（它们也可能合法跳出去）。
        // 闭包捕获可变状态在 Rust 里要用 `let mut` 局部变量手写，故展开成宏。
        macro_rules! record_var_decl {
            ($pos:expr, $name:expr) => {{
                var_pos = Some($pos);
                var_name = $name;
                bad_gotos.clone_from(&fwd_gotos);
            }};
        }

        // 官方 `trimTrailingEmptyStmts`（`:167`）：`fallthrough` 即使后面跟着
        // 空语句也算列表最后一条 ⇒ 判它合不合法前必须先削掉尾部空语句。
        let stmt_list: Vec<&Stmt> = trim_trailing_empty_stmts(body);

        for (stmt_index, stmt) in stmt_list.iter().copied().enumerate() {
            // 官方 `lstmt = nil`（`:169`）+ `L:`（`:170`）+ `goto L`（`:208`）。
            // Rust 没有 `goto`，用一层 `loop` 重演「处理 LabeledStmt 后把 stmt
            // 换成 `s.Stmt` 再重来一次，且 `lstmt` 更新」。
            let mut cur = stmt;
            let mut lstmt_here = lstmt;
            loop {
                match cur {
                    Stmt::Decl(DeclStmt { decls, .. }) => {
                        for d in decls {
                            if let Decl::Var(v) = d {
                                // 官方 `:174-177`：`v.NameList[0]` —— 只取**第一个**名字，
                                // 且 `break`（第一个 VarDecl 就够）。
                                record_var_decl!(v.pos, v.names[0].value.clone());
                                break;
                            }
                        }
                        break;
                    }
                    Stmt::Labeled(LabeledStmt {
                        label, stmt: inner, ..
                    }) => {
                        let name = label.value.clone();
                        // 官方 `:182`：`name != "_"`（空白标签不算声明）
                        if name != "_" {
                            let id = self.labeled.len() as LabeledId;
                            self.labeled.push(LabeledSlot {
                                name: name.clone(),
                                pos: label.pos,
                                kind: labeled_kind(inner),
                            });
                            self.declare(p, b, id);

                            // 官方 `:184-203`：解决匹配的前向 `goto`
                            //
                            // ⚠️ **必须遍历快照**（官方的 `for _, fwd := range fwdGotos`
                            // 遍历的是**副本**，长度固定为进入循环时的值），
                            // 靠 `else` 分支的 `fwdGotos[i] = fwd; i++` 原地压缩、
                            // 最后 `fwdGotos = fwdGotos[:i]` 截掉已解决的那批。
                            //
                            // 直译成 `while i < fwd_gotos.len()` 会**死循环**：
                            // 匹配时 `i` 不递增，而 `len()` 也没变 ⇒ 条件恒真。
                            // 症状是**测试进程被 SIGKILL（OOM）**而不是报错 ——
                            // 8 条含 `goto` 的用例一起挂，看起来像「`goto` 一律坏」。
                            let snapshot = fwd_gotos.clone();
                            let mut i = 0;
                            for fid in snapshot {
                                let fname = self.labeled[fid as usize].name.clone();
                                if fname == name {
                                    // 官方 `:190-196`：`jumpsOverVarDecl`
                                    //
                                    // ⚠️ 判据的**两个条件顺序**：官方是
                                    // `if varPos.IsKnown() { for _, bad := range badGotos {…} }`
                                    // —— `badGotos` 只在 `varPos` 已知时才有意义。
                                    // 直译成 `if let Some(vp) = var_pos` 同时
                                    // 保住「`var_pos` 为空 ⇒ 一律不报」这个语义。
                                    if let Some(vp) = var_pos
                                        && bad_gotos.contains(&fid)
                                    {
                                        let vp = p.pos_str(vp);
                                        let vn = var_name.clone();
                                        let fpos = self.labeled[fid as usize].pos;
                                        self.errf(
                                            fpos,
                                            DiagCode::SynGotoJumpsOverDecl,
                                            format!(
                                                "goto {name} jumps over declaration of {vn} at {vp}"
                                            ),
                                        );
                                    }
                                    // 官方 `l.used = true`
                                    if let Some(slot) = self.labels.get_mut(&name) {
                                        slot.used = true;
                                    }
                                    // 匹配的那条**不**保留（官方只 `i++` 于 else 分支）
                                } else {
                                    fwd_gotos[i] = fid;
                                    i += 1;
                                }
                            }
                            fwd_gotos.truncate(i);
                            lstmt_here = Some(id);
                        }
                        // 官方 `:207-208`：`stmt = s.Stmt; goto L`
                        cur = inner;
                        continue;
                    }
                    Stmt::Branch(bs) => {
                        fwd_gotos.extend(self.branch_stmt(
                            b,
                            bs,
                            ctxt,
                            stmt_index,
                            stmt_list.len(),
                        ));
                        break;
                    }
                    Stmt::Assign(AssignStmt { op, lhs, pos, .. }) => {
                        // 官方 `:294-297`：`s.Op == Def` ⇒ 记录变量声明
                        if *op == AssignOp::Define {
                            let n = lhs.first().map(expr_text).unwrap_or_default();
                            record_var_decl!(*pos, n);
                        }
                        break;
                    }
                    Stmt::Block(inner) => {
                        // 官方 `:299-301`：`inner := targets{ctxt.breaks, ctxt.continues, -1}`
                        let t = Targets::passthrough(ctxt.breaks, ctxt.continues);
                        fwd_gotos.extend(self.block_branches(
                            p,
                            Some(b),
                            t,
                            lstmt_here,
                            inner.pos,
                            &inner.stmts,
                        ));
                        break;
                    }
                    Stmt::If(x) => {
                        // 官方 `:303-308`
                        let t = Targets::passthrough(ctxt.breaks, ctxt.continues);
                        fwd_gotos.extend(self.block_branches(
                            p,
                            Some(b),
                            t,
                            lstmt_here,
                            x.then.pos,
                            &x.then.stmts,
                        ));
                        if let Some(els) = &x.els {
                            fwd_gotos.extend(self.block_branches(
                                p,
                                Some(b),
                                t,
                                lstmt_here,
                                crate::text::stmt_pos(els),
                                std::slice::from_ref(els.as_ref()),
                            ));
                        }
                        break;
                    }
                    Stmt::For(ForStmt { body: fbody, .. }) => {
                        // 官方 `:310-312`：`inner := targets{s, s, -1}` ——
                        // `break` 与 `continue` 都指向这个 `for`。
                        let t = Targets {
                            breaks: Some(TargetKind::For),
                            continues: Some(TargetKind::For),
                            case_index: -1,
                            case_total: 0,
                            is_type_switch: false,
                        };
                        fwd_gotos.extend(self.block_branches(
                            p,
                            Some(b),
                            t,
                            lstmt_here,
                            fbody.pos,
                            &fbody.stmts,
                        ));
                        break;
                    }
                    Stmt::Switch(SwitchStmt { tag, cases, .. }) => {
                        // 官方 `:314-319`：`inner := targets{s, ctxt.continues, -1}`
                        // ⚠️ `continues` **沿用外层** —— `switch` 里的 `continue`
                        // 指向的是**外层**的 `for`，不是这个 `switch`。
                        let mut t = Targets {
                            breaks: Some(TargetKind::Switch),
                            continues: ctxt.continues,
                            case_index: -1,
                            case_total: cases.len(),
                            is_type_switch: is_type_switch_tag(tag.as_ref()),
                        };
                        for (i, cc) in cases.iter().enumerate() {
                            t.case_index = i as i32;
                            fwd_gotos.extend(self.block_branches(
                                p,
                                Some(b),
                                t,
                                lstmt_here,
                                cc.pos,
                                &cc.body,
                            ));
                        }
                        break;
                    }
                    Stmt::Select(sel) => {
                        // 官方 `:321-325`：`inner := targets{s, ctxt.continues, -1}`
                        let t = Targets {
                            breaks: Some(TargetKind::Select),
                            continues: ctxt.continues,
                            case_index: -1,
                            case_total: sel.clauses.len(),
                            is_type_switch: false,
                        };
                        for cc in &sel.clauses {
                            fwd_gotos.extend(self.block_branches(
                                p,
                                Some(b),
                                t,
                                lstmt_here,
                                cc.pos,
                                &cc.body,
                            ));
                        }
                        break;
                    }
                    _ => break,
                }
            }
        }

        fwd_gotos
    }

    /// 官方 `blockBranches` 里 `*BranchStmt` 那一大段（`:210-292`）。
    ///
    /// 返回本条语句新增的**前向 `goto`**（`goto` 那条臂最多一条），
    /// 由调用者并入本块的 `fwd_gotos` —— 官方的 `fwdGotos` 是
    /// `blockBranches` 的局部变量，Rust 里让方法返回比共享可变状态更直白。
    fn branch_stmt(
        &mut self,
        b: BlockId,
        s: &BranchStmt,
        ctxt: Targets,
        stmt_index: usize,
        stmt_count: usize,
    ) -> Vec<LabeledId> {
        let Some(label) = &s.label else {
            // 官方 `:211-246`：无标签的分支
            match s.tok {
                BranchTok::Break => {
                    if ctxt.breaks.is_none() {
                        self.errf(
                            s.pos,
                            DiagCode::SynBranchBreak,
                            "break is not in a loop, switch, or select".to_string(),
                        );
                    }
                }
                BranchTok::Continue => {
                    if ctxt.continues.is_none() {
                        self.errf(
                            s.pos,
                            DiagCode::SynBranchContinue,
                            "continue is not in a loop".to_string(),
                        );
                    }
                }
                BranchTok::Fallthrough => {
                    // 官方 `:226-240`：**三档**文案
                    let msg = if ctxt.breaks == Some(TargetKind::Switch) {
                        if ctxt.is_type_switch {
                            "cannot fallthrough in type switch".to_string()
                        } else if ctxt.case_index < 0 || stmt_index + 1 < stmt_count {
                            // 官方 `:231-233`：嵌套在块里、或**不是**最后一条
                            // ⇒ 保持默认文案
                            "fallthrough statement out of place".to_string()
                        } else if ctxt.case_index as usize + 1 == ctxt.case_total {
                            // 官方 `:234-236`：最后一个 `case`（其后没有别的 case）
                            "cannot fallthrough final case in switch".to_string()
                        } else {
                            // 官方 `:237`：`break // fallthrough ok` ⇒ 不报
                            return Vec::new();
                        }
                    } else {
                        "fallthrough statement out of place".to_string()
                    };
                    self.errf(s.pos, DiagCode::SynBranchFallthrough, msg);
                }
                BranchTok::Goto => {
                    // 官方 `:241-242` 的 `fallthrough` 落到下面「带标签」那一段。
                    // 官方注释写「should always have a label」—— 无标签的 `goto`
                    // 在官方会 panic（`s.Label.Value` 解 nil 引用）。
                    // 我们**跳过**：解析器保证 `goto` 一定带标签（`T-AP14`），
                    // 这条路不可达。
                }
            }
            return Vec::new();
        };

        // 官方 `:249-292`：带标签的分支
        let name = label.value.clone();
        match s.tok {
            BranchTok::Break => match self.enclosing_target(Some(b), &name) {
                None => self.errf(
                    label.pos,
                    DiagCode::SynLabelUndefined,
                    format!("break label not defined: {name}"),
                ),
                // 官方 `:258-262`：`t.Stmt` 必须是 Switch / Select / For
                Some(k) if !k.is_break_target() => self.errf(
                    label.pos,
                    DiagCode::SynBranchBreak,
                    format!("invalid break label {name}"),
                ),
                Some(_) => {}
            },
            BranchTok::Continue => match self.enclosing_target(Some(b), &name) {
                None => self.errf(
                    label.pos,
                    DiagCode::SynLabelUndefined,
                    format!("continue label not defined: {name}"),
                ),
                // 官方 `:272-276`：`t.Stmt` 必须是 For
                Some(k) if !k.is_continue_target() => self.errf(
                    label.pos,
                    DiagCode::SynBranchContinue,
                    format!("invalid continue label {name}"),
                ),
                Some(_) => {}
            },
            BranchTok::Goto => {
                if !self.goto_target(Some(b), &name) {
                    // 标签可能在**后面**才声明 ⇒ 记成前向 `goto`（官方 `:285`）。
                    // 存档的 `kind` 是 `Other` —— 它只作为「这条 `goto` 还没解决」
                    // 的占位，形状要等标签真被 `declare` 时才有。
                    let id = self.labeled.len() as LabeledId;
                    self.labeled.push(LabeledSlot {
                        name: name.clone(),
                        pos: label.pos,
                        kind: LabeledKind::Other,
                    });
                    return vec![id];
                }
            }
            BranchTok::Fallthrough => {
                // 官方 `:288-289`：`fallthrough` **永远不带标签**
            }
        }
        Vec::new()
    }
}

/// 标签标注的语句是**哪一类**（官方读 `t.Stmt` 的动态类型，我们登记时记下）。
fn labeled_kind(s: &Stmt) -> LabeledKind {
    match s {
        Stmt::For(_) => LabeledKind::For,
        Stmt::Switch(_) => LabeledKind::Switch,
        Stmt::Select(_) => LabeledKind::Select,
        _ => LabeledKind::Other,
    }
}

/// `x.(type)` 守卫（官方 `*TypeSwitchGuard`）—— 类型 switch 的判据。
///
/// 官方 `:229` 用 `t.Tag.(*TypeSwitchGuard)`。我们把类型 switch 的 tag
/// 建成同一个变体（见 `T-AP13` 的 `type_switch_guard`），所以判据是
/// 「tag 是 `TypeSwitchGuard`」。
fn is_type_switch_tag(tag: Option<&Expr>) -> bool {
    matches!(tag, Some(Expr::TypeSwitchGuard(_)))
}

/// 官方 `trimTrailingEmptyStmts`（`branches.go:332-339`）。
///
/// 官方是**就地**截短切片（`list[:i]`），我们返回引用版。
fn trim_trailing_empty_stmts(list: &[Stmt]) -> Vec<&Stmt> {
    let mut i = list.len();
    while i > 0 && matches!(list[i - 1], Stmt::Empty(_)) {
        i -= 1;
    }
    list[..i].iter().collect()
}

/// 官方 `checkBranches` 的入口（`branches.go:16`）。
///
/// 由 [`crate::parser::Parser::func_body`] 在**这个函数没有语法错**时调用。
pub(crate) fn check_branches(body: &BlockStmt, p: &mut Parser) {
    let mut ls = LabelScope::new();
    ls.check(p, body);
    // 按收集顺序发出（官方是遍历顺序，同序）
    for e in ls.deferred {
        p.report_branch(e.pos, e.code, e.msg);
    }
}
