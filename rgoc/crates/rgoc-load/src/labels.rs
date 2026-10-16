//! 标签（`Label`）的声明、引用与控制流校验（M3/Task 26：`SCP-05`、`T-TY-57`）。
//!
//! # 这一段为什么**不在** [`crate::scope`] 里
//!
//! 官方的标签判定是**独立的一趟**（`go/types/labels.go`），而且**独立于值作用域**：
//!
//! * 标签不与变量同名冲突（`L: for { L := 1; _ = L }` 合法）；
//! * 标签的可见性是「**整个函数体**」——不是「从声明处开始」：官方用**一个**
//!   `all` 作用域装下体里所有标签，所以 `goto L` 可以**前向**跳到后面才声明的 `L`；
//! * 因此顺序相反的两条规则同时成立：**值**必须「先声明后使用」（`scope` 那一趟），
//!   **标签**可以「先使用后声明」（本趟的前向跳转匹配）。
//!
//! 把两者塞进同一趟会同时错掉这三条，所以这里照官方一分为二：
//! [`crate::scope`] 走值作用域、本文件走标签。
//!
//! # 与官方的对应
//!
//! | 官方（`labels.go`） | 本文件 |
//! |---|---|
//! | `labels(body)`（`stmt.go:45`，**每个函数体**一次） | [`check_labels`] |
//! | `all`（整个体的标签集） | [`Pass::all`] |
//! | `block{parent, labels, lstmt}` | [`Block`] + [`Pass::blocks`]（竞技场 + 下标） |
//! | `blockBranches` / `stmtBranches` | [`Pass::block_branches`] / [`Pass::stmt_branches`] |
//! | `fwdJumps` | `fwd` 形参（**进出都传递**，与官方那个闭包一致） |
//! | `all.Insert` 失败 ⇒ `DuplicateLabel` | [`Pass::declare`] |
//! | `b.gotoTarget` / `b.enclosingTarget` | [`Pass::goto_target`] / [`Pass::enclosing_target`] |
//!
//! # 刻意不做（进 `M3-manifest.json` 的 `open_gaps`）
//!
//! * **`JumpOverDecl`**（`goto L` 跳过变量声明，官方 `softErrorf`）—— 它的消息要
//!   把**位置翻成行号**（“jumps over variable declaration at line %d”），而本仓的
//!   `Diag` 只带位置、不带行号解析器；为一条 `soft` 错引进「位置 → 行号」的
//!   依赖不划算，且它不影响标签**命名空间**的正确性（前向跳转匹配照做，只是
//!   匹配上之后不再额外报那一条）；
//! * **`InvalidSyntaxTree`**（官方 `default:` 支）—— 只有 AST 畸形才走得到，
//!   本仓的 parser 到不了那里；
//! * 官方的 `soft` 语义（软错误不抑制后续诊断）—— M3 的 `Diag` 没有严重度字段，
//!   而**两条标签诊断的位置与代号都与官方相同**，所以本趟照报（见 `M3-design.md` 的
//!   `open_gaps`）。
//!
//! # ⚠️ 与 `rgoc-parse` 的 `SYN-*` 标签诊断**各报一层**
//!
//! 官方有**两套**标签检查：语法器的 `checkBranches`（`cmd/compile/internal/syntax`，
//! M2 已镜像，措辞是 “label L **defined** and not used” / “label L not **defined**”）
//! 与类型检查器的 `labels.go`（措辞是 “label L **declared** and not used” /
//! “label L not **declared**”）。我们的流水线两层都跑，于是同一条问题**可能**出两条
//! （一条 `SYN-LABEL-*` + 一条 `TYP-*`），而官方 `gc` 在语法层报完就**不再类型检查**
//! （`cmd/compile` 只在无语法错时进 types2）。这是**层次镜像**的必然结果，
//! 不是重复实现：两条的代号、位置、措辞都各自与对应官方层一致。
//! ⇒ 本文件自己的用例只断言 `TYP-*` 那一侧（见 `tests/exported_label.rs`）。

use std::collections::BTreeMap;

use rgoc_ast::{BranchTok, LabeledStmt, Stmt};
use rgoc_lex::{Diag, DiagBag, DiagCode, Pos};

/// 一个**块**在官方 `labels.go` 里的对应物（`block`）。
struct Block<'a> {
    /// 外层块（`None` = 函数体那一层）。
    parent: Option<usize>,
    /// 本块里声明的标签（官方 `block.labels`）—— **只**服务 `goto`：`goto` 只能
    /// 跳到本块或**外层**块里的标签（跳到内层 = 「跳进块」）。
    labels: BTreeMap<&'a str, &'a LabeledStmt>,
    /// 本块关联的**带标签语句**（官方 `block.lstmt`）—— `break L` / `continue L`
    /// 要求 `L` 是**包住自己**的那个语句的标签。
    ///
    /// ⚠️ `case` / `select` 子句的块是 `lstmt = None`（官方 `blockBranches(nil, …)`），
    /// 但 `parent` 照旧指向外层 ⇒ 从子句里 `break L` 仍能沿链找到外层那个标签。
    lstmt: Option<&'a LabeledStmt>,
}

/// 一个**函数体**的标签空间（官方 `labels()` 里的 `all` 作用域）。
struct Pass<'a, 'b> {
    /// 名字 →（声明处，是否被引用过）。官方是 `*Scope` + `Label.used`。
    all: BTreeMap<&'a str, (&'a LabeledStmt, bool)>,
    /// 块竞技场（`parent` 是下标而不是引用：官方那棵树在 Rust 里借不过去）。
    blocks: Vec<Block<'a>>,
    bag: &'b mut DiagBag,
}

/// 校验一个函数体（或函数字面量体）的标签 —— 官方 `check.labels(body)`。
///
/// 官方只在「体里出现过标签语句」时才调它（`check.hasLabel`）；本仓没有那个标志位，
/// 空体/无标签体走一趟的开销与一次遍历相同，不值得多存一个字段。
pub(crate) fn check_labels(body: &[Stmt], bag: &mut DiagBag) {
    let mut pass = Pass {
        all: BTreeMap::new(),
        blocks: Vec::new(),
        bag,
    };

    // 走完整棵树：块链随递归进出，前向 `goto` 一路往上冒。
    let fwd = pass.block_branches(None, None, body);

    // 收尾：还有前向跳转没匹配上 ⇒ 要么根本没人声明（`UndeclaredLabel`），
    // 要么声明在**内层**块里（`JumpIntoBlock`）—— 判据是「全集里有没有这个名字」。
    let mut jumps: Vec<(Pos, String, bool)> = Vec::new();
    for jmp in fwd {
        // `fwd` 里只可能有带标签的 `goto`（入栈处就筛过了）
        let Some(n) = &jmp.label else { continue };
        let inside = pass.all.contains_key(n.value.as_str());
        jumps.push((n.pos, n.value.clone(), inside));
    }
    for (pos, name, inside) in jumps {
        if inside {
            // 官方同时把该标签标成「已使用」（免得再报一条 `UnusedLabel`）
            pass.mark_used(&name);
            pass.push(
                pos,
                DiagCode::TypJumpIntoBlock,
                format!("goto {name} jumps into block"),
            );
        } else {
            pass.push(
                pos,
                DiagCode::TypUndeclaredLabel,
                format!("label {name} not declared"),
            );
        }
    }

    // 声明了却没人引用 ⇒ 官方 `softErrorf(UnusedLabel)`。
    // 先收集再报（要 `&mut pass` 才能 push，不能边遍历 `all` 边借）。
    let unused: Vec<(Pos, String)> = pass
        .all
        .iter()
        .filter(|(_, (_, used))| !*used)
        .map(|(n, (l, _))| (l.label.pos, (*n).to_owned()))
        .collect();
    for (pos, name) in unused {
        pass.push(
            pos,
            DiagCode::TypUnusedLabel,
            format!("label {name} declared and not used"),
        );
    }
}

impl<'a, 'b> Pass<'a, 'b> {
    fn push(&mut self, pos: Pos, code: DiagCode, message: String) {
        self.bag.push(Diag { pos, code, message });
    }

    /// 记下一个标签**已使用**（官方 `all.Lookup(name).(*Label).used = true`）。
    ///
    /// 官方那里强制断言一定查得到；这里用 `if let` —— 查不到只会少记一次「已用」，
    /// 而多报的那条 `UnusedLabel` 是**保守**方向（真有问题才报），不会掩盖错误。
    fn mark_used(&mut self, name: &str) {
        if let Some(e) = self.all.get_mut(name) {
            e.1 = true;
        }
    }

    /// 开一个块（官方 `b := &block{parent: parent, lstmt: lstmt}`）。
    fn push_block(&mut self, parent: Option<usize>, lstmt: Option<&'a LabeledStmt>) -> usize {
        self.blocks.push(Block {
            parent,
            labels: BTreeMap::new(),
            lstmt,
        });
        self.blocks.len() - 1
    }

    /// 官方 `block.gotoTarget`：沿**外层**链找名字（**不含**内层块）。
    fn goto_target(&self, b: usize, name: &str) -> Option<&'a LabeledStmt> {
        let mut cur = Some(b);
        while let Some(i) = cur {
            if let Some(t) = self.blocks[i].labels.get(name) {
                return Some(t);
            }
            cur = self.blocks[i].parent;
        }
        None
    }

    /// 官方 `block.enclosingTarget`：沿外层链找「是这个名字的**带标签语句**」。
    fn enclosing_target(&self, b: usize, name: &str) -> Option<&'a LabeledStmt> {
        let mut cur = Some(b);
        while let Some(i) = cur {
            if let Some(t) = self.blocks[i].lstmt
                && t.label.value == name
            {
                return Some(t);
            }
            cur = self.blocks[i].parent;
        }
        None
    }

    /// 官方 `blockBranches`：走一个**语句列表**，返回**向上冒**的前向跳转。
    ///
    /// ⚠️ 内层块冒上来的跳转要**并入**本层的 `fwd`（官方那个闭包的
    /// `fwdJumps = append(fwdJumps, check.blockBranches(...)...)`）——
    /// 少并一次，内层的 `goto L` 就永远匹配不上，会被误报成 `UndeclaredLabel`。
    fn block_branches(
        &mut self,
        parent: Option<usize>,
        lstmt: Option<&'a LabeledStmt>,
        list: &'a [Stmt],
    ) -> Vec<&'a rgoc_ast::BranchStmt> {
        let b = self.push_block(parent, lstmt);
        let mut fwd: Vec<&'a rgoc_ast::BranchStmt> = Vec::new();
        for s in list {
            self.stmt_branches(b, None, s, &mut fwd);
        }
        fwd
    }

    /// 官方 `stmtBranches`：一条语句。`lstmt` 是「当前这条语句链上最近的那个标签」。
    fn stmt_branches(
        &mut self,
        b: usize,
        lstmt0: Option<&'a LabeledStmt>,
        s: &'a Stmt,
        fwd: &mut Vec<&'a rgoc_ast::BranchStmt>,
    ) {
        let mut lstmt = lstmt0;
        match s {
            // 官方在这里只记 `var` 声明的位置（`JumpOverDecl` 用）—— 本文件不做那条
            Stmt::Decl(_) => {}

            Stmt::Labeled(l) => {
                let name: &str = &l.label.value;
                if name != "_" {
                    self.declare(b, l, name);
                    // 匹配**前向**跳转（官方 `fwdJumps` 的过滤）
                    let mut i = 0;
                    while i < fwd.len() {
                        let hit = fwd[i]
                            .label
                            .as_ref()
                            .is_some_and(|n| n.value.as_str() == name);
                        if hit {
                            self.mark_used(name);
                            fwd.remove(i);
                        } else {
                            i += 1;
                        }
                    }
                    // ⚠️ 官方把 `lstmt = s` 放在 `if name != "_"` **里面** ⇒
                    // `_: for{}` 后面的 `break L` 反而能跳到更外层的同名标签
                    lstmt = Some(l);
                }
                self.stmt_branches(b, lstmt, &l.stmt, fwd);
            }

            Stmt::Branch(br) => {
                // 无标签的分支由第一趟（`scope`）管 —— 这里只管带标签的
                let Some(n) = &br.label else { return };
                let name: &str = &n.value;
                match br.tok {
                    BranchTok::Break => {
                        // 规范：有标签时，它必须是**外层**的 for/switch/select
                        let ok = self.enclosing_target(b, name).is_some_and(|t| {
                            matches!(
                                t.stmt.as_ref(),
                                Stmt::Switch(_) | Stmt::Select(_) | Stmt::For(_)
                            )
                        });
                        if !ok {
                            self.push(
                                n.pos,
                                DiagCode::TypMisplacedLabel,
                                format!("invalid break label {name}"),
                            );
                            // 官方此处 `return` ⇒ **不**标记已使用（于是还会报未用）
                            return;
                        }
                    }
                    BranchTok::Continue => {
                        // 规范：有标签时，它必须是**外层**的 for
                        let ok = self
                            .enclosing_target(b, name)
                            .is_some_and(|t| matches!(t.stmt.as_ref(), Stmt::For(_)));
                        if !ok {
                            self.push(
                                n.pos,
                                DiagCode::TypMisplacedLabel,
                                format!("invalid continue label {name}"),
                            );
                            return;
                        }
                    }
                    BranchTok::Goto => {
                        if self.goto_target(b, name).is_none() {
                            // 可能声明在后面 ⇒ 记成前向跳转，收尾再算
                            fwd.push(br);
                            return;
                        }
                    }
                    // `fallthrough` 不带标签（语法层已管位置约束）；真带了也只需记「已用」
                    BranchTok::Fallthrough => {}
                }
                self.mark_used(name);
            }

            Stmt::Block(bl) => {
                let inner = self.block_branches(Some(b), lstmt, &bl.stmts);
                fwd.extend(inner);
            }

            Stmt::If(i) => {
                // 官方：`stmtBranches(lstmt, s.Body)` ⇒ `s.Body` 是块 ⇒ 开一层（`lstmt` 传下去）
                let inner = self.block_branches(Some(b), lstmt, &i.then.stmts);
                fwd.extend(inner);
                if let Some(els) = i.els.as_deref() {
                    // `else if` 是嵌套的 `Stmt::If`（递归），`else {}` 是块
                    self.stmt_branches(b, lstmt, els, fwd);
                }
            }

            Stmt::Switch(sw) => {
                // 官方：switch 体是一层块（`lstmt` 传下去），**每个 case** 再各一层
                // 且 `lstmt = nil`（`blockBranches(nil, s.Body)`）——
                // 这正是「case 里 `break L` 仍能跳到外层 `L: for`」的原因。
                let sbody = self.push_block(Some(b), lstmt);
                for c in &sw.cases {
                    let cb = self.push_block(Some(sbody), None);
                    for s2 in &c.body {
                        self.stmt_branches(cb, None, s2, fwd);
                    }
                }
            }

            Stmt::Select(sel) => {
                let sbody = self.push_block(Some(b), lstmt);
                for c in &sel.clauses {
                    let cb = self.push_block(Some(sbody), None);
                    for s2 in &c.body {
                        self.stmt_branches(cb, None, s2, fwd);
                    }
                }
            }

            // `for`（含 `range` 形式）：官方 `stmtBranches(lstmt, s.Body)` ⇒ 一层块
            Stmt::For(f) => {
                let inner = self.block_branches(Some(b), lstmt, &f.body.stmts);
                fwd.extend(inner);
            }

            // 其余语句里不可能出现 `goto`/标签（表达式内部由第一趟走；`defer`/`go`
            // 的调用、`return` 的表达式都不含语句）
            Stmt::Empty(_)
            | Stmt::Expr(_)
            | Stmt::Send(_)
            | Stmt::Assign(_)
            | Stmt::Call(_)
            | Stmt::Return(_) => {}
        }
    }

    /// 官方 `all.Insert` 那一支：成功则同时进「全集」与「本块」。
    ///
    /// ⚠️ `b` 必须由调用方给出（不能取 `blocks.len() - 1`）：竞技场里
    /// **后面的语句可能已经开了内层块**，最后一个块不一定是当前这一层。
    fn declare(&mut self, b: usize, l: &'a LabeledStmt, name: &'a str) {
        if self.all.contains_key(name) {
            // 官方 `DuplicateLabel`（`soft`）—— 位置取**后**声明处
            self.push(
                l.label.pos,
                DiagCode::TypDuplicateLabel,
                format!("label {name} already declared"),
            );
            return;
        }
        self.all.insert(name, (l, false));
        self.blocks[b].labels.insert(name, l);
    }
}
