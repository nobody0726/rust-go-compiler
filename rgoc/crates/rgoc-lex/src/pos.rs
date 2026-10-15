//! 位置表示 —— `C1` 契约（SourceMap / 位置表示 / 诊断排序）的正式实现。
//!
//! 来源：`docs/contracts/C1-source-map.md`。三条硬规定：
//!
//! 1. `line` 与 `col` 都是 **1 起** —— 用 0 起会让「工具报的行号」与 oracle 差 1，
//!    而这种差 1 极难排查。
//! 2. `col` 是**字节列**，不是 rune 列 —— 否则含中文/emoji 的行会与 oracle 错位。
//!    代价是一个 UTF-8 字符横跨多个列号，这是**有意的取舍**。
//! 3. `file` 是**文件标识**，不是路径字符串本身。
//!
//! # `T-AP15` 的契约变更（用户 2026-10-15 拍板）
//!
//! ① [`Pos`] 加了 [`Pos::base`]（官方 `Pos.base`）—— 没有它，`//line` 重映射无处安放。
//! ② 新增 [`PosBase`] / [`PosBases`] / [`BaseId`]（官方 `PosBase` + 指针链 → 表）。
//!
//! **这三条硬规定一条都没动**，而且是**有意**让它们在 `base == None` 时保持原样：
//! 没有 `//line` 指令的文件，`rel_*` 全部退回绝对位置 ⇒ M1 冻结的行为**逐条不变**
//! ⇒ 现有测试零改动。详见 [`Pos`] 的文档。
//!
//! # 为什么 `Pos` 不实现 `Display`
//!
//! `Pos` 只持有 [`FileId`] 与一个 base 下标，**解析不出文件名**。若给它一个 `Display`，
//! 打出来会是 `0:3:7` 这种「看起来像位置、其实缺文件名」的字符串，
//! 与 `C1` §2.2「必须与 oracle 的 `文件:行号` 前缀兼容」直接冲突。
//! 所以格式化一律经 [`FileTable::format_pos`] —— **让漏掉文件名这件事无法发生**。
//!
//! # 排序仍用**绝对**坐标（已知偏离官方，登记在 `progress.md` 的 T-AP15 残留表）
//!
//! [`Pos`] 的 `derive(Ord)` 按**声明顺序**比：`base → file → line → col`。
//! ⚠️ 官方 `ErrorList.Less` 用的是 `RelFilename()/RelLine()/RelCol()`。
//! 两者只在「**一条文件里有多个 `//line` 基准、且后一条的行号比前一条小**」时
//! 才会给出不同顺序（例如 `//line a:1000` 之后又 `//line b:1`）。
//! 那样排出来的顺序错位**要到 oracle 对撞（T-AP19）才会暴露**，届时按残留表回填。
//! 现在**不**改成按 rel 排：改它要动 `DiagBag::into_sorted` 的签名（C1 的排序入口），
//! 而那条路径上还没有任何用例真的需要 rel 排序。

use std::num::NonZeroU32;

/// 文件标识。
///
/// 内部整数不公开：要拿索引请用 [`FileId::index`]，且不要把它当作稳定值。
///
/// # 一个已知局限：`FileId` **不是全局唯一**的
///
/// `FileId` 底层只是一个下标，所以**不同表产出的 `FileId(0)` 是相等的**。
/// 若把 A 表的 Id 拿去查 B 表，下标恰好落在 B 表范围内时**不会报错**，会取到 B 表里
/// 无关的文件名 —— 这是**检测不出来**的。
///
/// M1 的约束是**每次编译只有一张表**（`Lexer` 持有它），所以这个局限不会触发。
/// 等到 M3 引入多包多文件时，如果那时出现跨表混用，需要给 `FileId` 加上表标识
/// （或改成 `(TableId, Index)` 二元组）—— 那是一次契约变更，不是修 bug。
///
/// 当前防住的是**越界**（下标超出表长），见 [`FileTable::name`] 的 `Panics` 一节。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct FileId(u32);

impl FileId {
    /// 内部索引。**不承诺稳定性** —— 只用于调试与断言。
    pub fn index(self) -> u32 {
        self.0
    }

    /// 从索引构造。
    ///
    /// **日常代码不该调用它** —— `FileId` 的唯一合法来源是 [`FileTable::intern`]。
    /// 存在只为两件事：测试越界路径、以及将来的反序列化。
    #[doc(hidden)]
    pub fn from_index(i: u32) -> Self {
        FileId(i)
    }
}

/// 文件表：只登记 `名字 ↔ FileId`。
///
/// **不做** offset ↔ `(line, col)` 的双向换算 —— 那是 SourceMap 的职责，归 M2/M4
/// （`C1` §6 第 2 问；`M1-design.md` D-M1-5 只划定了 `FileTable` 这一半）。
///
/// 用 `Vec` 线性查而不是 `HashMap`：M1 处理的文件数是个位数，且线性查让
/// 「同名字必须得到同一个 Id」这条不依赖哈希行为。
#[derive(Default, Debug)]
pub struct FileTable {
    names: Vec<String>,
}

impl FileTable {
    /// 建一张空表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记一个文件名。同名字重复调用**返回同一个 [`FileId`]**。
    pub fn intern(&mut self, name: &str) -> FileId {
        if let Some(i) = self.names.iter().position(|n| n == name) {
            return FileId(i as u32);
        }
        self.names.push(name.to_string());
        FileId((self.names.len() - 1) as u32)
    }

    /// 取回文件名。
    ///
    /// # Panics
    ///
    /// 传入不属于本表的 [`FileId`] 时 panic。**这是有意的**：那说明调用方把两张表的 Id
    /// 混用了，静默返回 `""` 只会让错误位置变成 `<空>:3:7` 这种看似正常的东西。
    pub fn name(&self, id: FileId) -> &str {
        match self.names.get(id.0 as usize) {
            Some(n) => n.as_str(),
            None => panic!("FileId({}) 不属于本表", id.0),
        }
    }

    /// 按 `C1` §2.2 的格式渲染位置：`{file}:{line}:{col}`。
    ///
    /// 例：`test/helloworld.go:3:7`。该格式必须与 oracle 的「文件:行号」前缀兼容
    /// —— 那是诊断对齐（比较器 R3/R4）的前提。
    pub fn format_pos(&self, pos: Pos) -> String {
        format!("{}:{}:{}", self.name(pos.file), pos.line, pos.col)
    }
}

/// 位置三元组 + 可选的 `//line` 基准（`T-AP15`；用户 2026-10-15 拍板走「与官方同构」）。
///
/// `line`/`col` 都是 **1 起**；`col` 的单位是**字节**。
///
/// # 为什么 `file` 与 `base` **并存**（官方只有 `base`）
///
/// 官方的 `Pos` 是 `{base *PosBase, line, col}`，文件名要从 `base.Filename()` 取；
/// 我们多留一个 `file: FileId` 是因为 **`C1` §2.2 的 `FileTable::format_pos`
/// 与 oracle 的「文件:行号」前缀对齐**这条契约已经在 M1 冻结并有断言，且
/// `FileId` 是**表下标**、比 `String` 便宜得多。
///
/// 两者不是重复：
///
/// - `file` ≡ 官方的 `pos.FileBase()`（`pos.go:40-47`，沿 base 链回溯到**自指的**那个）；
/// - `base` ≡ 官方的 `pos.Base()`（当前生效的那条 `//line`）。
///
/// # `base == None` 的含义
///
/// 「还没有 `//line` 基准」，等价于官方的 `base == nil`。此时
/// [`Pos::rel_line`] / [`Pos::rel_col`] / [`Pos::rel_filename`] 全部**退回绝对位置**
/// —— 官方在 `base == nil` 时也是这么兜的（`pos.go:53-56` 的 `b.Line() == 0` 那支）。
/// 这让 **`base: None` 的文件行为与 M1 完全一致** ⇒ 现有全部测试零改动。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Pos {
    /// 当前生效的 `//line` 基准（`T-AP15`）。`None` ⇒ 没有指令，退回绝对位置。
    ///
    /// ⚠️ **不参与 [`Ord`] 的比较键**：排序键仍然是 `file → line → col`
    /// （`C1` §4），所以本字段声明在 `file` **之后**是**有意**的 ——
    /// 见下面那条「排序为什么仍用绝对坐标」的说明。
    pub base: Option<BaseId>,
    /// 所属文件（**绝对**：等价于官方的 `FileBase()`）
    pub file: FileId,
    /// 行号，**1 起**
    pub line: u32,
    /// 列号，**1 起**，单位是**字节**
    pub col: u32,
}

impl Pos {
    /// 官方 `Pos.Line()` —— **绝对**行号（不是 `RelLine`）。
    #[must_use]
    pub fn line(self) -> u32 {
        self.line
    }

    /// 官方 `Pos.Col()` —— **绝对**列号。
    #[must_use]
    pub fn col(self) -> u32 {
        self.col
    }

    /// 官方 `Pos.IsKnown()`（`pos.go:32`）。
    #[must_use]
    pub fn is_known(self) -> bool {
        self.line > 0
    }

    /// 官方 `Pos.RelFilename()`（`pos.go:49`）：当前基准下的文件名。
    ///
    /// 无基准 ⇒ 就是 `file` 的名字；`//line` 指定了名字 ⇒ 那个名字
    /// （此时**不再** `intern` —— 基准表自带 `String`，见 [`PosBases`]）。
    #[must_use]
    pub fn rel_filename(self, ft: &FileTable, bases: &PosBases) -> String {
        match self.base.and_then(|b| bases.get(b)) {
            Some(b) => b.filename.clone(),
            None => ft.name(self.file).to_string(),
        }
    }

    /// 官方 `Pos.RelLine()`（`pos.go:51-58`）。
    ///
    /// ```text
    /// b.Line() == 0  ⇒ 0（基准未知）
    /// 否则            ⇒ b.line + (pos.line - b.pos.line)
    /// ```
    #[must_use]
    pub fn rel_line(self, bases: &PosBases) -> u32 {
        let Some(b) = self.base.and_then(|b| bases.get(b)) else {
            return self.line;
        };
        if b.line == 0 {
            return 0;
        }
        b.line + (self.line - b.pos.line)
    }

    /// 官方 `Pos.RelCol()`（`pos.go:60-74`）。
    ///
    /// ```text
    /// b.Col() == 0                  ⇒ 0（列未知）
    /// pos.Line() == b.Pos().Line()  ⇒ b.col + (pos.col - b.pos.col)
    /// 否则                          ⇒ pos.col（**绝对列**）
    /// ```
    ///
    /// ⚠️ 第三条是 `return pos.Col()`（`pos.go:73`）——**不是** `b.col`。
    /// 而 `//line foo:123`（**无** `:col`）时 `b.col` 恰好是 **0**，
    /// 所以早就被上面那条「`b.Col() == 0 ⇒ 0`」截走了 ⇒ 「列未知」对**后续所有行**生效
    /// （官方注释 `pos.go:64-66` 明说这符合当前规范：*"this to apply until the
    /// next PosBase/line directive, not just until the new newline"*）。
    ///
    /// ⚠️ 抄错这条的症状**极轻**：`pos.col` 与 `b.col` 在「无 `//line`」时
    /// 只差 `b.pos.col` 的偏移，写成 `b.col` 会让**每行的第一列都变成 1** ——
    /// 而列号只差几列的位置错误**不会报错**，只在对撞时以「某行 diff」暴露。
    #[must_use]
    pub fn rel_col(self, bases: &PosBases) -> u32 {
        let Some(b) = self.base.and_then(|b| bases.get(b)) else {
            return self.col;
        };
        if b.col == 0 {
            return 0;
        }
        if self.line == b.pos.line {
            b.col + (self.col - b.pos.col)
        } else {
            self.col
        }
    }
}

/// 基准表的下标 —— 与 [`FileId`] 同构（`pub(crate)` 的内部整数 + 两个访问器）。
///
/// # 为什么是 [`NonZeroU32`] 而不是 `u32`
///
/// 有了 `NonZeroU32`，`Option<BaseId>` 靠**空位优化**只占 **4 字节**（`u32` 的
/// 全 0 位模式是 `None`）。这不只是省内存 —— [`Pos`] 因此只比 M1 的版本大 4 字节，
/// 而 `rgoc_ast::Decl` 这个大 enum 的「最大变体 vs 次大变体」的**大小差**仍在
/// `clippy::large_enum_variant` 的阈值之下（写成 `Option<u32>` 会占 8 字节、
/// 顶破阈值）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct BaseId(NonZeroU32);

impl BaseId {
    /// 内部索引。**不承诺稳定性**。
    #[must_use]
    pub fn index(self) -> u32 {
        self.0.get() - 1
    }

    /// 从索引构造。**日常代码不该调用它** —— 唯一合法来源是 [`PosBases`]。
    ///
    /// `# Panics`：索引为 `u32::MAX`（加一后溢出）。
    #[doc(hidden)]
    pub fn from_index(i: u32) -> Self {
        BaseId(NonZeroU32::new(i + 1).expect("基准表下标溢出 u32 —— 表不可能这么大"))
    }
}

/// 一条 `//line` 基准（官方 `PosBase`，`pos.go:26-32`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PosBase {
    /// 挂这条基准的**位置**（官方 `PosBase.pos`）—— 即「紧跟那条指令之后」的 `Pos`。
    pub pos: Pos,
    /// 文件名（官方 `PosBase.filename`）。
    pub filename: String,
    /// 目标行号；**0 = 未知**（官方用 0 表示 `//line foo` 无行号的情形）。
    pub line: u32,
    /// 目标列号；**0 = 未知** ⇒ [`Pos::rel_col`] 对后续所有位置返回 0。
    pub col: u32,
    /// 官方 `PosBase.trimmed` —— `//line :10:20` 的空文件名会把上一条的 `trimmed` 一并沿用。
    pub trimmed: bool,
}

/// `//line` 基准的链（官方是 `*PosBase` 的指针链；这里是**表**）。
///
/// # 为什么是表而不是 `Rc<PosBase>`
///
/// 官方的 `Pos` 持 `*PosBase` 指针。我们若照抄，base 会变成**生命周期问题**
/// （`Pos` 是 `Copy` 且跨文件传递，指向谁？），而 **`Pos` 必须继续 `Copy`**
/// ——它是全仓最常按值传的��型，且 `Ord`/`Hash` 都靠它。
///
/// 表 + 下标的代价是「算 rel 要带上表」，收益是 `Pos` 的 `Copy`/`Ord`/`Hash`/`Eq`
/// **全部原样保留** ⇒ M1 冻结的行为零改动。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct PosBases {
    bases: Vec<PosBase>,
    /// 当前生效的那条（官方 `p.base`）。`None` ⇒ 还没有 `//line`。
    current: Option<BaseId>,
}

impl PosBases {
    /// 新建一条空链。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 官方 `NewFileBase(filename)`（`pos.go:171`）：链的根，`IsFileBase()` 为真。
    ///
    /// 根基准的 `pos` 是 `(1, 1)` 且**自指**（`base` 指回自己的下标）——
    /// 官方 `IsFileBase()` 正是靠 `base.pos.base == base` 判「我是根」，
    /// 而 `pos.go:43` 的 `FileBase()` 回溯也靠这条自指边停下。
    /// 这里同样**自指**：构造时先算出下标再回填，所以 `is_file_base` 能用同一判据
    /// （`pos.base == Some(self)`），不必为根单开一条特例。
    ///
    /// ⚠️ **`line`/`col` 必须是 `1`/`1`（官方的 `linebase`/`colbase`），
    /// 不是 `0`**（官方 `pos.go:162` 的 `NewTrimmedFileBase`）。
    /// 这两个值决定「没有 `//line` 时 rel 是否退回绝对」：
    /// `RelLine() = b.line + (pos.line - b.pos.line) = 1 + (line - 1) = line`。
    /// 写成 0 会命中 `pos.go:53-56` 的「基准行号未知 ⇒ 返回 0」那支
    /// ⇒ **全仓每个 `rel_line` 都变成 0**，而症状只是「位置全是 0」——
    /// 一个看着像「位置没算出来」的错误，真因是基准的默认值抄错了。
    ///
    /// ⚠️ **`file` 用 `FileId::from_index(0)` 而不 `intern`**：官方
    /// `NewFileBase` 只收一个字符串，不碰任何表；我们的 `file_base` 若要
    /// `intern` 就得收 `&mut FileTable`，那会让「建基准」依赖「某张特定的表」——
    /// 而 `FileId` 是**表下标**，跨表混用**检测不出来**（见 [`FileId`] 的已知局限）。
    ///
    /// 正确用法：**先 `ft.intern(filename)` 再调本函数**，让两边都落在下标 0
    /// —— 这正是「本文件就是那个文件」的常规情形，也是 M1 的约束
    /// （「每次编译只有一张表」，见 [`FileId`] 的文档）。
    pub fn file_base(&mut self, filename: &str) -> BaseId {
        let id = BaseId::from_index(self.bases.len() as u32);
        self.bases.push(PosBase {
            pos: Pos {
                base: Some(id), // 自指
                file: FileId::from_index(0),
                line: 1, // linebase
                col: 1,  // colbase
            },
            filename: filename.to_string(),
            line: 1, // linebase —— 见上面那条 ⚠️
            col: 1,  // colbase
            trimmed: false,
        });
        self.current = Some(id);
        id
    }

    /// 官方 `NewLineBase(pos, filename, trimmed, line, col)`（`pos.go:172`）。
    pub fn line_base(
        &mut self,
        pos: Pos,
        filename: &str,
        trimmed: bool,
        line: u32,
        col: u32,
    ) -> BaseId {
        self.bases.push(PosBase {
            pos,
            filename: filename.to_string(),
            line,
            col,
            trimmed,
        });
        let id = BaseId::from_index((self.bases.len() - 1) as u32);
        self.current = Some(id);
        id
    }

    /// 当前生效的那条基准（官方 `p.base`）。
    #[must_use]
    pub fn current(&self) -> Option<BaseId> {
        self.current
    }

    /// 取一条基准。**`# Panics`**：下标越界 —— 那说明有人拿别处的 `BaseId` 来查，
    /// 与 [`FileTable::name`] 的越界 panic 同理（静默返回默认值会让位置变成
    /// 「看起来正常」的错值）。
    #[must_use]
    pub fn get(&self, id: BaseId) -> Option<&PosBase> {
        self.bases.get(id.index() as usize)
    }

    /// 官方 `PosBase.IsFileBase()`（`pos.go:176-181`）：`base.pos.base == base`。
    #[must_use]
    pub fn is_file_base(&self, id: BaseId) -> bool {
        self.get(id).is_some_and(|b| b.pos.base == Some(id))
    }

    /// 官方 `PosBase.Trimmed()`（`pos.go:211`）。
    #[must_use]
    pub fn trimmed(&self, id: BaseId) -> bool {
        self.get(id).is_some_and(|b| b.trimmed)
    }

    /// 基准表长度（M2 的用例用；也便于诊断「base 有没有挂上」）。
    #[must_use]
    pub fn len(&self) -> usize {
        self.bases.len()
    }

    /// 链上有没有基准。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bases.is_empty()
    }
}
