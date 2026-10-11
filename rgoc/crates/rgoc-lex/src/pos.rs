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
//! # 为什么 `Pos` 不实现 `Display`
//!
//! `Pos` 只持有 [`FileId`]，**解析不出文件名**。若给它一个 `Display`，
//! 打出来会是 `0:3:7` 这种「看起来像位置、其实缺文件名」的字符串，
//! 与 `C1` §2.2「必须与 oracle 的 `文件:行号` 前缀兼容」直接冲突。
//! 所以格式化一律经 [`FileTable::format_pos`] —— **让漏掉文件名这件事无法发生**。

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

/// 位置三元组。
///
/// `line`/`col` 都是 **1 起**；`col` 的单位是**字节**。
///
/// `derive` 的字段顺序**就是** [`Ord`] 的排序键顺序 —— `C1` §4 要求
/// `file → line → col`（第 4 键 `code` 只有 [`Diag`] 才有，不在本类型上）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Pos {
    /// 所属文件
    pub file: FileId,
    /// 行号，**1 起**
    pub line: u32,
    /// 列号，**1 起**，单位是**字节**
    pub col: u32,
}
