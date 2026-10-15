//! 词元的数据模型（`M1-design.md` §3.2 / §3.3）。
//!
//! # 一句话
//!
//! 一个词元 = **位置**（[`Pos`]）+ **类别**（[`TokenKind`]）；
//! 类别里的**载荷随分支而变**（标识符带名字、字面量带原文+子类+错误标记、运算符带形式+语义+优先级）。
//!
//! # 这个设计要解决的唯一问题：`lit` 的有效域
//!
//! 官方 `gc/syntax` 把词元摊成一个**扁平结构体 + 注释说明字段何时有效**
//! （`lit string // valid if tok is _Name, _Literal, or _Semi`）。
//! 后果我们**实测**过（F5）：读一个运算符词元的 `lit` 会拿到**上一个词元的残留值** ——
//! 字段有值、类型合法，只有语义是错的。那是本仓反复踩过的「静默失败」形状。
//!
//! Rust 的和类型让"这个字段此刻是否存在"成为**编译期事实**：
//! 它在哪个分支里，就只能从那个分支里取。**非法状态不可表示**。
//!
//! # 与官方字段的对应（T-L18 的 `lex` 输出与 golden 要用这张表）
//!
//! | 官方字段 | 我们的位置 | 备注 |
//! |---|---|---|
//! | `tok` | `TokenKind` 的**判别式** | 一个 `tok` 拆成多个分支（`_Operator` → `Op`） |
//! | `lit` | **各分支的载荷** | 有效域从注释变成了类型 |
//! | `kind` | `Literal.lit_kind` | 官方数值见 [`LitKind::as_official_code`] |
//! | `op` / `prec` | `Op.op` / `Op.prec` | **只在 `Op` 分支存在** |
//! | —（官方靠不同 `tok` 区分） | `Op.form` | **D-M1-10**：官方用 `_Operator`/`_AssignOp`/`_IncOp` 三个 `tok`，我们用 `form` 字段 |
//! | `bad` | `Literal.bad` | 只在 `Literal` 分支存在 |
//! | `line` / `col` | [`Pos`] | 独立值对象（C1 契约） |
//! | `blank` | **不实现** | 官方只用于判定 `//go:build` 是否独占一行（`parser.go:85`），M1 不需要 |
//! | `nlsemi` | **不在这里** | 它是**扫描器**的状态，见 `semicolon::SemicolonPolicy` |
//!
//! # 已知缺口（留给 T-L18）
//!
//! 官方对 `=` / `:=` / `<-` **不设置 `op`**（它们是 `_Assign` / `_Define` / `_Arrow`，
//! 而 `op` 只在 `_Operator` / `_Star` / `_AssignOp` / `_IncOp` 上有效，见 `scanner.go:42`）。
//! 我们**故意比官方多给一个语义值**（[`Operator::Assign`] / [`Operator::Define`] / [`Operator::Arrow`]），
//! 但代价是：**生成 golden 时这三者的 `op` 不能参与比对**。
//! 渲染口径属于 T-L18，届时按"只录有效域内字段"的规矩处理。

use crate::pos::Pos;

/// 一个词元：位置 + 类别。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Token {
    /// 词元**起点**的位置（只存起点；宽度可由载荷推出，M1 用不上就不存）
    pub pos: Pos,
    /// 类别与载荷
    pub kind: TokenKind,
}

/// 词元类别 —— 官方 `scanner.tok` 的和类型版本。
///
/// 46 个真实类别（F1）在这里被归成 7 个变体：官方把"是什么"和"长什么样"混在一个扁平结构里，
/// 我们把**载荷挂到各自的分支上**。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TokenKind {
    /// 标识符（官方 `_Name`）。**关键字不在这里** —— 它们有独立变体。
    ///
    /// # 为什么是 `Vec<u8>` 而不是 `String`
    ///
    /// 实测（2026-10-11，oracle）：`x\xff y` ⇒ 标识符的原文是 **`x\xff`** ——
    /// 那个 `0xFF` 是**非法 UTF-8**，而 Rust 的 `String` 在**类型上**就装不了它。
    /// 用 `String` 会让"照官方行为产出词元"这件事**做不到**，
    /// 而失败方式是**编码期**的（要么编译不过，要么被逼着做有损转换 —— 后者静默丢信息）。
    ///
    /// 合法的非 ASCII 不受影响：NUL / BOM / 私有区字符都是合法 UTF-8，实测能正常装进 `String`；
    /// **只有真正非法的字节序列**需要字节容器。
    ///
    /// 取 `&str` 请用 [`std::str::from_utf8`]：关键字全是 ASCII，
    /// 所以非法 UTF-8 的标识符**必然不是关键字**（转换失败 ⇒ 直接 `None`）。
    Ident(Vec<u8>),
    /// 字面量（官方 `_Literal`）。5 个子类共用这一个类别，靠 [`LitKind`] 分辨。
    Literal {
        /// **原文切片** —— 含引号/前缀/分隔符，转义**未解码**。
        ///
        /// 例：`"hello"` 的 `text` 是 `"\"hello\""`（含两端的双引号），
        /// `\n` 等转义此时**还不存在**。算值是 M3 常量模块的事。
        ///
        /// 类型同 [`TokenKind::Ident`]：**必须是字节**。
        /// 实测 `1\xff2` ⇒ `text` 是 `1\xff2`（非法字节留在原文里），
        /// 且此时 `bad` 可能**仍是 false** —— 见 [`TokenKind::Literal::bad`]。
        text: Vec<u8>,
        /// 字面量子类
        lit_kind: LitKind,
        /// 原文里有**语法**错误（官方 `bad`）。
        ///
        /// **它不影响词元的产出** —— 实测：`0x` / `1__2` / `'ab'` / `"unterm`
        /// 都照常产出 `Literal`（`bad=true`）并附带一条诊断。恢复纪律是
        /// 「报错但继续，把畸形的东西也交出去」。
        ///
        /// ⚠️ **`bad == false` 不代表 `text` 是好的**：实测 `1\xff2` 是 `kind=0 bad=false` ——
        /// 非法 UTF-8 由字符层（`nextch`）报出，不经数字语法，所以 `bad` 不变真。
        /// `bad` 只说明"字面量的**语法**没问题"，**不等于**"原文可信"。
        bad: bool,
    },
    /// 保留关键字（25 个，LEX-05）。
    ///
    /// 独立成变体而不是 `Ident` 的特例，是因为官方就是独立 `tok`；
    /// 而且实测 F7 表明 `true`/`nil`/`println` 这些**预声明标识符不是关键字**，
    /// 混进 `Ident` 反而更容易被误判成关键字。
    Keyword(Keyword),
    /// 运算符（官方 `_Operator` / `_Star` / `_AssignOp` / `_IncOp` / `_Assign` / `_Define` / `_Arrow`）。
    ///
    /// **`form` 是 D-M1-10 加的**：官方靠**不同的 `tok`** 区分 `+`/`+=`/`++`，
    /// 若只存 `op`，三者都成 `Add` ⇒ M2 无法区分二元表达式与赋值语句（静默丢信息）。
    Op {
        /// 书写形式（`+` / `+=` / `++`）
        form: OpForm,
        /// 运算符语义（`+` 与 `+=` 的 `op` 都是 [`Operator::Add`]）
        op: Operator,
        /// 优先级（`spec:L268-282` 的五级；无优先级的为 0）
        prec: u8,
    },
    /// 分隔符（官方 `_Lparen` … `_DotDotDot`）。**`;` 不在其中** —— 它归 [`TokenKind::Semi`]。
    Delim(Delim),
    /// 分号（官方 `_Semi`）。**来源可辨**是它必须带上载荷的理由：
    /// 源码里真写的、换行处自动补的、文件尾补的，三者类别相同、语义不同。
    Semi(SemiOrigin),
    /// 文件结束标记。**它本身也是一个词元**（实测：流里恰好一个，且在最后）。
    Eof,
}

/// 运算符的**书写形式** —— 与 [`Operator`]（语义）**正交**。见 D-M1-10。
///
/// | 拼写 | 建模 |
/// |---|---|
/// | `+` | `(Plain, Add)` |
/// | `+=` | `(Assign, Add)` |
/// | `++` | `(IncDec, Add)` |
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum OpForm {
    /// 普通形式：`+` `-` `*` `/` `==` `&&` …（含 `=` `:=` `<-` `~`）
    Plain,
    /// 复合赋值：`+=` `-=` `*=` … `&^=`（11 个）
    Assign,
    /// 自增自减：`++` `--`（2 个）
    IncDec,
}

/// 字面量子类（官方 `LitKind`；实测数值见 [`LitKind::as_official_code`]，来自 F6）。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum LitKind {
    /// 整数（`1` `0x1f` `0123`）
    Int,
    /// 浮点（`1.5` `1e9` `0x1p-2`）
    Float,
    /// 虚数（后缀 `i`）
    Imag,
    /// 字符（`'x'`）
    Rune,
    /// 字符串（解释型与原始型同属这一类）
    String,
}

/// 分号的**来源**。
///
/// 官方靠 `lit` 的取值（`"semicolon"` / `"newline"` / `"EOF"`）区分同一类别的三种来源 ——
/// 那是 `lit` 必须有效的**真实理由**（F5 里 `lit` 唯一"有用的例外"）。
/// 我们用类型把它挑明。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum SemiOrigin {
    /// 源码里真的写了 `;`
    Explicit,
    /// 换行处自动补出（LEX-07）
    Newline,
    /// 文件尾补出（收尾时 `nlsemi` 仍为真）
    Eof,
}

/// 分隔符（10 个）。
///
/// 不含 `;`（归 [`TokenKind::Semi`]）与 `:=`（归 [`Operator::Define`]）。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Delim {
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `[`
    LBrack,
    /// `]`
    RBrack,
    /// `{`
    LBrace,
    /// `}`
    RBrace,
    /// `,`
    Comma,
    /// `:` —— 注意 `:=` 是 [`Operator::Define`]，两者必须可区分
    Colon,
    /// `.`
    Dot,
    /// `...`（官方 `_DotDotDot`）
    Ellipsis,
}

/// 运算符的**语义**（官方 `Operator`）。
///
/// 除官方的 21 个"真运算符"外，另含三个**官方不设 `op` 值**的语义（见模块文档的"已知缺口"）：
/// [`Operator::Assign`]（`=`）、[`Operator::Define`]（`:=`）、[`Operator::Arrow`]（`<-`）。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Operator {
    /// `!`
    Not,
    /// `~`
    Tilde,
    /// `=` —— 官方是 `_Assign`，**无 `op` 值**
    Assign,
    /// `:=` —— 官方是 `_Define`，**无 `op` 值**
    Define,
    /// `<-` —— 官方是 `_Arrow`，**无 `op` 值**
    Arrow,

    /// `||`
    OrOr,
    /// `&&`
    AndAnd,
    /// `==`
    Eql,
    /// `!=`
    Neq,
    /// `<`
    Lss,
    /// `<=`
    Leq,
    /// `>`
    Gtr,
    /// `>=`
    Geq,
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `|`
    Or,
    /// `^`
    Xor,
    /// `*`
    Mul,
    /// `/`
    Div,
    /// `%`
    Rem,
    /// `&`
    And,
    /// `&^`
    AndNot,
    /// `<<`
    Shl,
    /// `>>`
    Shr,
}

impl Operator {
    /// 全部变体，**按声明顺序**（分组的顺序：无优先级 → prec1 → … → prec5）。
    ///
    /// 供遍历式断言使用 —— 让「漏了一个变体」有地方可查。
    pub const ALL: [Operator; 24] = [
        Operator::Not,
        Operator::Tilde,
        Operator::Assign,
        Operator::Define,
        Operator::Arrow,
        Operator::OrOr,
        Operator::AndAnd,
        Operator::Eql,
        Operator::Neq,
        Operator::Lss,
        Operator::Leq,
        Operator::Gtr,
        Operator::Geq,
        Operator::Add,
        Operator::Sub,
        Operator::Or,
        Operator::Xor,
        Operator::Mul,
        Operator::Div,
        Operator::Rem,
        Operator::And,
        Operator::AndNot,
        Operator::Shl,
        Operator::Shr,
    ];

    /// 优先级（`spec:L268-282`）。
    ///
    /// 五级：`||`=1 / `&&`=2 / 比较=3 / `+ - | ^`=4 / `* / % & &^ << >>`=5。
    /// **`0` 表示"不参与二元优先级"**（`!` `~` `=` `:=` `<-`）——
    /// 与官方 `precOrOr`(=1) … `precMul`(=5) 的编号一致（`tokens.go` 的常量块）。
    pub fn prec(self) -> u8 {
        match self {
            Operator::Not
            | Operator::Tilde
            | Operator::Assign
            | Operator::Define
            | Operator::Arrow => 0,
            Operator::OrOr => 1,
            Operator::AndAnd => 2,
            Operator::Eql
            | Operator::Neq
            | Operator::Lss
            | Operator::Leq
            | Operator::Gtr
            | Operator::Geq => 3,
            Operator::Add | Operator::Sub | Operator::Or | Operator::Xor => 4,
            Operator::Mul
            | Operator::Div
            | Operator::Rem
            | Operator::And
            | Operator::AndNot
            | Operator::Shl
            | Operator::Shr => 5,
        }
    }

    /// 官方 `Operator.String()`（`operator_string.go`，`T-AP16` 补）。
    ///
    /// # 为什么这三个也在这里，尽管官方**没有**它们
    ///
    /// 官方的 `Operator` 只有 21 个变体（`=` `:=` `<-` 是**独立的 token**，
    /// 官方不设 `op` 值），而我们把三者并进了同一枚举（见枚举文档的「已知缺口」）。
    /// 消息复刻（`T-A-66`）要用 `op.String()` 拼 `unexpected X`，那三个位置
    /// 同样要一个拼写 —— 所以**在这里补齐**，而不是让 `rgoc-parse` 另写一张表。
    ///
    /// ⚠️ 这张表**逐字**来自官方 `operator_string.go` 的 `_Operator_name`：
    /// `":!<-~||&&==!=<<=>>=+-|^*/%&&^<<>>"` —— 手抄这张表极易出错（`&&` 出现两次、
    /// `&` 的二进制与一元写法不同），所以下面的单测**逐条**断言。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Operator::Not => "!",
            Operator::Tilde => "~",
            Operator::Assign => "=",
            Operator::Define => ":=",
            Operator::Arrow => "<-",
            Operator::OrOr => "||",
            Operator::AndAnd => "&&",
            Operator::Eql => "==",
            Operator::Neq => "!=",
            Operator::Lss => "<",
            Operator::Leq => "<=",
            Operator::Gtr => ">",
            Operator::Geq => ">=",
            Operator::Add => "+",
            Operator::Sub => "-",
            Operator::Or => "|",
            Operator::Xor => "^",
            Operator::Mul => "*",
            Operator::Div => "/",
            Operator::Rem => "%",
            Operator::And => "&",
            Operator::AndNot => "&^",
            Operator::Shl => "<<",
            Operator::Shr => ">>",
        }
    }
}

impl OpForm {
    /// 全部变体，按声明顺序。
    pub const ALL: [OpForm; 3] = [OpForm::Plain, OpForm::Assign, OpForm::IncDec];
}

impl LitKind {
    /// 全部变体，按声明顺序。
    pub const ALL: [LitKind; 5] = [
        LitKind::Int,
        LitKind::Float,
        LitKind::Imag,
        LitKind::Rune,
        LitKind::String,
    ];

    /// 官方的数值编码（`syntax.LitKind`，**实测事实 F6**）。
    ///
    /// 存在的理由：golden 用的是**官方数值**（`kind=4`），
    /// 把这张映射做成可断言的方法，好过让它散落在渲染代码里。
    pub fn as_official_code(self) -> u8 {
        match self {
            LitKind::Int => 0,
            LitKind::Float => 1,
            LitKind::Imag => 2,
            LitKind::Rune => 3,
            LitKind::String => 4,
        }
    }
}

impl SemiOrigin {
    /// 全部变体，按声明顺序。
    pub const ALL: [SemiOrigin; 3] = [SemiOrigin::Explicit, SemiOrigin::Newline, SemiOrigin::Eof];
}

impl Delim {
    /// 全部变体，按声明顺序。**`;` 不在其中**（归 [`TokenKind::Semi`]）。
    pub const ALL: [Delim; 10] = [
        Delim::LParen,
        Delim::RParen,
        Delim::LBrack,
        Delim::RBrack,
        Delim::LBrace,
        Delim::RBrace,
        Delim::Comma,
        Delim::Colon,
        Delim::Dot,
        Delim::Ellipsis,
    ];
}

/// 保留关键字（**25** 个，LEX-05）。
///
/// # 为什么用 `match` 而不是哈希表
///
/// 官方那套 `keywordMap` + `init()` 校验需要**构造期检查**（哈希撞了就 panic 或漏判）。
/// Rust 的 `match` 由编译器生成跳转表，并且**穷尽性由编译器保证** ——
/// 新增一个关键字却忘了更新 `from_str` 会**编译失败**，而不是静默漏判。
///
/// # 不含预声明标识符
///
/// `true` / `nil` / `iota` / `println` / `append` / `int` / `any` …
/// 在官方**全是 `_Name`**（实测事实 F7）。它们是 M3/M4 的语义，不是 M1 的词法。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Keyword {
    /// `break`
    Break,
    /// `case`
    Case,
    /// `chan`
    Chan,
    /// `const`
    Const,
    /// `continue`
    Continue,
    /// `default`
    Default,
    /// `defer`
    Defer,
    /// `else`
    Else,
    /// `fallthrough`
    Fallthrough,
    /// `for`
    For,
    /// `func`
    Func,
    /// `go`
    Go,
    /// `goto`
    Goto,
    /// `if`
    If,
    /// `import`
    Import,
    /// `interface`
    Interface,
    /// `map`
    Map,
    /// `package`
    Package,
    /// `range`
    Range,
    /// `return`
    Return,
    /// `select`
    Select,
    /// `struct`
    Struct,
    /// `switch`
    Switch,
    /// `type`
    Type,
    /// `var`
    Var,
}

impl Keyword {
    /// 全部关键字，**按官方枚举顺序**（即字母序）。
    ///
    /// 供遍历式断言使用 —— 与 [`Keyword::lookup`] / [`Keyword::as_str`] 的穷尽性
    /// 一起构成三重保险：编译器管"分支齐全"，`ALL` 管"基数与重复"。
    pub const ALL: [Keyword; 25] = [
        Keyword::Break,
        Keyword::Case,
        Keyword::Chan,
        Keyword::Const,
        Keyword::Continue,
        Keyword::Default,
        Keyword::Defer,
        Keyword::Else,
        Keyword::Fallthrough,
        Keyword::For,
        Keyword::Func,
        Keyword::Go,
        Keyword::Goto,
        Keyword::If,
        Keyword::Import,
        Keyword::Interface,
        Keyword::Map,
        Keyword::Package,
        Keyword::Range,
        Keyword::Return,
        Keyword::Select,
        Keyword::Struct,
        Keyword::Switch,
        Keyword::Type,
        Keyword::Var,
    ];

    /// 源码里的拼写。**大小写敏感**，且是 golden 比对用的官方文本。
    pub fn as_str(self) -> &'static str {
        match self {
            Keyword::Break => "break",
            Keyword::Case => "case",
            Keyword::Chan => "chan",
            Keyword::Const => "const",
            Keyword::Continue => "continue",
            Keyword::Default => "default",
            Keyword::Defer => "defer",
            Keyword::Else => "else",
            Keyword::Fallthrough => "fallthrough",
            Keyword::For => "for",
            Keyword::Func => "func",
            Keyword::Go => "go",
            Keyword::Goto => "goto",
            Keyword::If => "if",
            Keyword::Import => "import",
            Keyword::Interface => "interface",
            Keyword::Map => "map",
            Keyword::Package => "package",
            Keyword::Range => "range",
            Keyword::Return => "return",
            Keyword::Select => "select",
            Keyword::Struct => "struct",
            Keyword::Switch => "switch",
            Keyword::Type => "type",
            Keyword::Var => "var",
        }
    }

    /// 拼写 → 关键字。**大小写敏感**（`For` 不是关键字）。
    ///
    /// 返回 `Option` 而不是 `Result`：**"不是关键字"不是错误**，它是绝大多数标识符的
    /// 正常归宿（`forx` 是标识符，这完全合法）。做成 `Result` 会把正常路径写成错误路径。
    ///
    /// 名字是 `lookup` 而非 `from_str`：后者与 [`std::str::FromStr`] 撞名，
    /// 而本方法返回 `Option` 又不符合那个 trait 的约定（它要 `Result`）——
    /// 叫 `from_str` 会让调用方以为可以用 `.parse()`。（`clippy::should_implement_trait`）
    ///
    /// 调用时机：**必须先把标识符整串切完**再查（LEX-05）——
    /// 边扫边匹配会让 `forx` 被误判成 `for` + `x`。实测：`forx` 是标识符，`for` 才是关键字。
    pub fn lookup(s: &str) -> Option<Keyword> {
        let k = match s {
            "break" => Keyword::Break,
            "case" => Keyword::Case,
            "chan" => Keyword::Chan,
            "const" => Keyword::Const,
            "continue" => Keyword::Continue,
            "default" => Keyword::Default,
            "defer" => Keyword::Defer,
            "else" => Keyword::Else,
            "fallthrough" => Keyword::Fallthrough,
            "for" => Keyword::For,
            "func" => Keyword::Func,
            "go" => Keyword::Go,
            "goto" => Keyword::Goto,
            "if" => Keyword::If,
            "import" => Keyword::Import,
            "interface" => Keyword::Interface,
            "map" => Keyword::Map,
            "package" => Keyword::Package,
            "range" => Keyword::Range,
            "return" => Keyword::Return,
            "select" => Keyword::Select,
            "struct" => Keyword::Struct,
            "switch" => Keyword::Switch,
            "type" => Keyword::Type,
            "var" => Keyword::Var,
            _ => return None,
        };
        Some(k)
    }
}
