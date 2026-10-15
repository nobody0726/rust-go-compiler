//! 错误恢复 —— `advance` / `stopset` / `errh` 双档（对齐 `F2`–`F5`）。
//!
//! 设计依据：`M2-design.md` §5.3。核心只有一个函数 —— [`Parser::advance`]。
//!
//! # 为什么这四件事单独放一个文件
//!
//! 官方 `parser.go` 里 `advance` / `stopset` / `errorAt` / `syntaxErrorAt` 是四个
//! 互不相关的小函数，散落在 parser 主体中间。但它们合起来是**一个机制**：
//! 出错时「报什么、要不要停、跳到哪」。拆开散进 `parser.rs`，改其中一块很容易漏掉另一块 ——
//! 而漏掉的症状是「恢复位置与 oracle 分叉」，极难定位。

use std::sync::OnceLock;

use rgoc_lex::diag::{Diag, DiagCode};
use rgoc_lex::pos::Pos;
use rgoc_lex::token::{Delim, Keyword, OpForm, Operator, SemiOrigin, TokenKind};

use crate::parser::{ErrMode, Parser, lit_text};

/// 官方 `tokstring`（`parser.go:304-316`）—— 把一个**类别**词元拼成人话。
///
/// 官方只有三个特例，其余一律 `tok.String()`：
///
/// ```text
/// _Comma            → "comma"
/// _Semi             → "semicolon or newline"
/// break..var(关键字)  → "keyword " + tok.String()
/// ```
///
/// # 为什么签名收 [`Follow`] 而不是 [`TokenKind`]
///
/// 官方的 `tokstring(tok token)` 收的是**类别**，而调用点全是 `p.want(tok)` /
/// `p.syntaxError("expected " + tokstring(follow))` —— `tok` 在那里就是一个
/// followset 成员。我们的对应物是 [`Follow`]（本文件的 `Follow` 文档解释了
/// 「为什么不能直接用 `TokenKind`」）。
///
/// # 为什么返回 [`String`] 而不是 `&'static str`
///
/// 关键字那一支要**拼接**（`"keyword " + k.as_str()`）⇒ 静态字符串做不到。
/// 另两个特例与默认分支本来都是 `&'static str`，统一成 `String` 免得调用方
/// 按分支处理（两处调用点，都是拼进消息，不差这点分配）。
///
/// # 三个特例各自的理由
///
/// - `comma` 而不是 `,` —— 消息要读成 `expected comma`，不是 `expected ,`。
/// - `semicolon or newline` 而不是 `;` —— 官方的 `;` 有**三种来源**
///   （显式 `;` / 换行自动补 / 文件尾补），只说 `;` 会误导。
/// - `keyword ` 前缀 —— 避免与标识符混淆：`expected chan` 读起来像「期望叫
///   `chan` 的变量」，加上 `keyword` 才说得清是关键字。
///
/// ⚠️ 这三条**每一个都容易被「按直觉」写错**（写成 `","` / `";"` / `"chan"`），
/// 而写错时语法完全正常 —— 症状只是消息里几个字符不同，要到 `T-AP18` 对撞
/// 才暴露。`tests/test_diag.rs` 为每条准备了**端到端**用例 + 一条反向用例。
pub(crate) fn tokstring(f: &Follow) -> String {
    match f {
        Follow::Kind(TokenKind::Delim(Delim::Comma)) => "comma".to_string(),
        Follow::Kind(TokenKind::Semi(_)) | Follow::Semi => "semicolon or newline".to_string(),
        Follow::Kind(TokenKind::Keyword(k)) => {
            // 官方 `:312-313`：
            // ```text
            // if _Break <= tok && tok <= _Var { return "keyword " + s }
            // ```
            // 判据是**区间的上下界**（25 个关键字的连续编号），而我们把关键字
            // 建成**独立变体** ⇒ 「落在区间内」这件事落在**类型**上：
            // 能走进这个臂，就已经是关键字了。
            //
            // ⚠️ 与官方**结构不同但语义等价**：官方的 `tokenCount` 上界在
            // Rust 侧不可表达，也不需要 —— 加一个非关键字的 `TokenKind`
            // 会走不到这个臂，而不是被 `keyword ` 前缀错标。
            format!("keyword {}", k.as_str())
        }
        Follow::Name => "name".to_string(),
        Follow::Plain(op) => op.as_str().to_string(),
        Follow::Operator => "op".to_string(),
        Follow::Kind(k) => token_kind_str(k).to_string(),
    }
}

/// 官方 `token.String()`（`token_string.go`）在我们数据模型上的直译。
///
/// 只在 [`Follow::Kind`] 装**非关键字、非 `,`、非 `;`** 时才会走到这里
/// （`tokstring` 的三个特例已在前面接住）。所以本函数只需覆盖
/// 分隔符与 `EOF` —— 官方表里其余 41 项分别对应 `_Name`（走 `Follow::Name`）、
/// 运算符（走 [`Operator::as_str`]）、`_Literal`（无 `Follow` 类别）。
fn token_kind_str(k: &TokenKind) -> &'static str {
    match k {
        TokenKind::Delim(d) => match d {
            Delim::LParen => "(",
            Delim::RParen => ")",
            Delim::LBrack => "[",
            Delim::RBrack => "]",
            Delim::LBrace => "{",
            Delim::RBrace => "}",
            Delim::Colon => ":",
            Delim::Dot => ".",
            Delim::Ellipsis => "...",
            // `tokstring` 的 `comma` 特例已在外层接住；这里到不了的 `Comma`
            // 仍给出官方 `token.String()` 的值，避免「漏一个变体」变成编译错误。
            Delim::Comma => ",",
        },
        TokenKind::Eof => "EOF",
        // 下面三个在 `tokstring` 里不会走到（`Keyword` 被外层接住、
        // `Semi` 被第一个特例接住、`Ident`/`Literal` 没有 `Follow` 类别）。
        // 仍按官方 `token.String()` 给出值，让本函数**穷尽** `TokenKind` ——
        // 加变体时编译器会指到这里，而不是让「漏了一个」悄悄变成别处的行为。
        TokenKind::Keyword(_) => "keyword",
        TokenKind::Semi(_) => ";",
        TokenKind::Ident(_) => "name",
        TokenKind::Literal { .. } => "literal",
        TokenKind::Op { form, op, .. } => match form {
            OpForm::Plain => op.as_str(),
            OpForm::Assign => concat_op(*op),
            OpForm::IncDec => op.as_str(),
        },
    }
}

/// 官方 `_AssignOp` 的拼法：`op.String() + "="`（`parser.go:287`）。
///
/// `"+="` 这类复合赋值**没有对应的 `Operator` 变体**（我们的 `Operator` 是
/// **语义**侧，`+=` 的语义仍是 `Add`）⇒ 这里把后缀补回去。
///
/// ⚠️ 不能把它做成 `Operator` 的方法：`op` 是语义（`Add`），
/// `+=` 的「哪部分」信息在 `OpForm` 里，不在 `op` 里。
fn concat_op(op: Operator) -> &'static str {
    match op {
        Operator::Add => "+=",
        Operator::Sub => "-=",
        Operator::Mul => "*=",
        Operator::Div => "/=",
        Operator::Rem => "%=",
        Operator::And => "&=",
        Operator::AndNot => "&^=",
        Operator::Or => "|=",
        Operator::Xor => "^=",
        Operator::Shl => "<<=",
        Operator::Shr => ">>=",
        // `op` 必须是**复合赋值的 11 个左操作数**之一（词法层保证）。
        // 兜底给出官方 `_AssignOp` 的类别名 `op=` 而不是 `unreachable!()`：
        // 一条诊断路径 panic 会把「消息写错」升级成「解析崩了」。
        _ => "op=",
    }
}

/// `stopset`：能**起头一个语句**的关键字（`F3`，官方 `parser.go:326-339`）。
///
/// 它们是错误恢复时的**同步点** —— 撞上就停下，通常不该跳过。
/// **只有在函数体内（`fnest > 0`）才并入 followset**：顶层的 `func` 也在这个集合里，
/// 若无条件并入，顶层恢复会一撞到 `func` 就停（那正是官方用 `fnest` 挡掉的）。
///
/// # 为什么类型写成 `[Keyword; 14]` 而不是 `&[Keyword]`
///
/// 「14」这个数曾经在文档里写错过（`M2-design.md` §14 勘误：初稿写 13，而列了 14 个）。
/// 写成固定长度数组，**「13」在类型上就不可表达** —— 数字再也错不了。
/// 成员的**集合**则由 `t_a_61_stopset_恰好十四个且与官方逐条一致` 钉住。
pub const STOPSET: [Keyword; 14] = [
    Keyword::Break,
    Keyword::Const,
    Keyword::Continue,
    Keyword::Defer,
    Keyword::Fallthrough,
    Keyword::For,
    Keyword::Go,
    Keyword::Goto,
    Keyword::If,
    Keyword::Return,
    Keyword::Select,
    Keyword::Switch,
    Keyword::Type,
    Keyword::Var,
];

/// followset 的一个成员。
///
/// # 为什么不能直接用 `TokenKind`
///
/// 官方 `advance(followlist ...token)` 里的 token 是**整数**，而它的词法把
/// 「所有标识符」折叠成一个 `_Name`、「所有分号」折叠成一个 `_Semi`（`tokens.go`）。
/// Rust 侧 [`TokenKind`] 把**载荷留着**（`Ident(Vec<u8>)` / `Semi(SemiOrigin)` /
/// `Op{form,op,prec}`），所以**没有任何一个 `TokenKind` 值能表示"任意标识符"** ——
/// 按值比较只能表达"具体词元"，表达不了"词元类别"。
///
/// # 类别变体为什么是**四个**（`T-AP10` 加了 `Plain`）
///
/// 回源枚举 `parser.go` 里**全部 26 处** `p.advance(...)` 调用，出现在 followlist 里的
/// 类别词元**恰好三个**：`_Name`（`:2310` / `:2485`）、`_Semi`（`:212`…`:2808` 共十处）、
/// `_Operator`（`:1897`）。其余全是关键字与分隔符（无载荷，用 [`Follow::Kind`] 即可）。
/// **官方 `_Literal` 从未出现在任何 followlist 里**，所以不加 —— 按证据加，不按对称感加。
///
/// 第四个 [`Follow::Plain`] **不是从 `advance` 里来的**，而是从「`got(_Assign)`」这类
/// **单点比较**来的（`gotAssign`，`parser.go:218`）：它要按 `(form, op)` 比，
/// 不能用 `Kind`（后者被迫多填一个由词法层决定的 `prec`）。判据见该变体的文档。
///
/// # 为什么住在 `rgoc-parse` 而不是 `rgoc-lex`
///
/// 「哪些 token 是同一个类别」是 `rgoc-lex` 的知识，但**这个枚举只为一个用途存在**：
/// 表达 parser 的 followset。放进 `rgoc-lex` 会把一个纯 parser 概念塞进 M1 的冻结 crate。
///
/// # 为什么不实现 `Copy`
///
/// [`Follow::Kind`] 装的是 [`TokenKind`]，而它带着 `Vec<u8>` 载荷 ⇒ 类型上就不是 `Copy`。
/// 需要传值时用 `Clone`；**const 成员不受影响**（`SEMI` 这类常量每次使用都是一个新值）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum Follow {
    /// 一个**具体**词元（关键字 / 分隔符）—— 无载荷，按值比较。
    Kind(TokenKind),
    /// 官方 `_Name`：**任意**标识符。
    Name,
    /// 官方 `_Semi`：**任意来源**的分号（显式 / 换行自动补 / 文件尾补）。
    Semi,
    /// 官方 `_Assign`(`=`) / `_Define`(`:=`) / `_Arrow`(`<-`) / `_Star`(`*`) ——
    /// 「书写形式是 [`OpForm::Plain`]、语义是某个 [`Operator`]」的那一类。
    ///
    /// # 为什么不能写成 `Follow::Kind(TokenKind::Op { form: Plain, op, prec })`
    ///
    /// 因为它要多填一个 **`prec`**，而 `Follow::Kind` 是**按值**比较 ⇒ 填错就永远不中，
    /// 且**不会报错**（症状是「语句层面认不出 `=`」这种很晚才暴露的错）。
    /// 而 `prec` 是**词法层**的知识（`Operator::prec()`），在 parser 里重写一遍就是
    /// 第二份真相。这里按 `(form, op)` 匹配，把 `prec` 从判据里**去掉**。
    ///
    /// 这四个恰好就是官方 `tokens.go` 里**不叫 `_Operator` 的四个运算符类 token**。
    Plain(Operator),
    /// 官方 `_Operator`。
    ///
    /// ⚠️ **比 [`TokenKind::Op`] 更窄**：官方把 `_Star`(`*`) / `_Assign`(`=`) /
    /// `_Define`(`:=`) / `_Arrow`(`<-`) / `_AssignOp`(`+=`) / `_IncOp`(`++`)
    /// 都算**独立** token，而 Rust 侧它们全都共用 `TokenKind::Op`。
    /// 按官方的折叠边界，`_Operator` 只含 `form == Plain` 且
    /// `op ∉ {Assign, Define, Arrow, Mul}` 的那些。
    ///
    /// 消费者是 [`EMBEDDED_TERM_FOLLOW`]（`T-AP12` 起）。
    Operator,
}

impl Follow {
    /// 词元 `k` 是否落在本成员所表示的集合里。
    ///
    /// `pub(crate)` 是给 [`crate::parser::Parser::got`] / `want` 用的 ——
    /// 它们是「按 followset 成员匹配」的通用入口，不该各自重写一遍匹配规则
    /// （两份匹配规则 ⇒ 迟早分叉，且分叉不报错）。
    pub(crate) fn matches(&self, k: &TokenKind) -> bool {
        match self {
            Follow::Kind(want) => k == want,
            Follow::Name => matches!(k, TokenKind::Ident(_)),
            Follow::Semi => matches!(k, TokenKind::Semi(_)),
            Follow::Plain(want) => matches!(
                k,
                TokenKind::Op {
                    form: OpForm::Plain,
                    op,
                    ..
                } if op == want
            ),
            Follow::Operator => matches!(
                k,
                TokenKind::Op { form: OpForm::Plain, op, .. }
                    if !matches!(
                        op,
                        Operator::Assign | Operator::Define | Operator::Arrow | Operator::Mul
                    )
            ),
        }
    }

    /// 把**具体**词元包成「描述它的类别成员」（`T-AP16`，[`tokstring`] 的调用方用）。
    ///
    /// # 为什么需要它
    ///
    /// [`tokstring`] 收的是**类别**（官方 `tokstring(tok token)`），
    /// 而 [`Parser::tok_desc`] 要描述的是**当前那个具体词元**。
    /// 官方的 `token` 本身就是「类别 + 一个 `op`/`lit` 载荷」的扁平结构，
    /// 所以 `tokstring(p.tok)` 能直接传；我们是 `TokenKind` 枚举，载荷挂在变体上 ⇒
    /// 需要一次「具体 → 类别」的投影。
    ///
    /// # 投影规则（与官方 `token.String()` 的表逐条对齐）
    ///
    /// | 当前词元 | 投影成 |
    /// |---|---|
    /// | `Ident(_)` | [`Follow::Name`] |
    /// | `Semi(_)` | [`Follow::Semi`] |
    /// | `Literal{..}` | `Kind(Literal{..})` —— 官方 `_Literal` 没有 `Follow` 类别，
    ///   但 [`tok_desc`] 的六档里它有自己的分支，**不会**走到 `tokstring` |
    /// | `Keyword(k)` / `Delim(_)` / `Eof` | `Kind(克隆出来的同一类别)` |
    /// | `Op{form: Plain, op}` | [`Follow::Plain(op)]` |
    /// | `Op{form: Assign, op}` | `Kind(Op{..})` —— 官方 `_AssignOp` **不是**
    ///   `Follow` 类别（`Follow::Operator` 明确排除它），同理 `IncDec` |
    fn from_kind(k: &TokenKind) -> Self {
        match k {
            TokenKind::Ident(_) => Follow::Name,
            TokenKind::Semi(_) => Follow::Semi,
            other => Follow::Kind(other.clone()),
        }
    }
}

/// 便捷常量：分号（写 followlist 时少一层 `Follow::Semi` 的噪声）。
pub(crate) const SEMI: Follow = Follow::Semi;
/// 便捷常量：`(`。分组声明 / 参数表 / 调用实参都用它。
pub(crate) const LPAREN: Follow = Follow::Kind(TokenKind::Delim(Delim::LParen));
/// 便捷常量：`)`。
pub(crate) const RPAREN: Follow = Follow::Kind(TokenKind::Delim(Delim::RParen));
/// 便捷常量：`,`
pub(crate) const COMMA: Follow = Follow::Kind(TokenKind::Delim(Delim::Comma));
/// 便捷常量：`package`。
pub(crate) const PACKAGE: Follow = Follow::Kind(TokenKind::Keyword(Keyword::Package));
/// 便捷常量：`type`。唯一用途是区分 `x.(T)`（类型断言）与 `x.(type)`（类型 switch 守卫）。
pub(crate) const TYPE: Follow = Follow::Kind(TokenKind::Keyword(Keyword::Type));
/// 便捷常量：`{`。
pub(crate) const LBRACE: Follow = Follow::Kind(TokenKind::Delim(Delim::LBrace));
/// 便捷常量：`}`。
pub(crate) const RBRACE: Follow = Follow::Kind(TokenKind::Delim(Delim::RBrace));
/// 便捷常量：`]`。
pub(crate) const RBRACK: Follow = Follow::Kind(TokenKind::Delim(Delim::RBrack));
/// 便捷常量：`:`（`:=` 是 [`Operator::Define`]，两者必须可区分）。
pub(crate) const COLON: Follow = Follow::Kind(TokenKind::Delim(Delim::Colon));
/// 便捷常量：`.`。
pub(crate) const DOT: Follow = Follow::Kind(TokenKind::Delim(Delim::Dot));
/// 便捷常量：`=`（官方 `_Assign`）。
pub(crate) const ASSIGN: Follow = Follow::Plain(Operator::Assign);
/// 便捷常量：`:=`（官方 `_Define`）。
pub(crate) const DEFINE: Follow = Follow::Plain(Operator::Define);

/// 恢复点 ①（**顶层声明之间**）—— 官方
/// `advance(_Import, _Const, _Type, _Var, _Func)`（`parser.go:469` / `479`）。
pub(crate) const TOP_DECL_FOLLOW: &[Follow] = &[
    Follow::Kind(TokenKind::Keyword(Keyword::Import)),
    Follow::Kind(TokenKind::Keyword(Keyword::Const)),
    Follow::Kind(TokenKind::Keyword(Keyword::Type)),
    Follow::Kind(TokenKind::Keyword(Keyword::Var)),
    Follow::Kind(TokenKind::Keyword(Keyword::Func)),
];

/// 恢复点 ②（**语句列表**）—— 官方
/// `advance(_Semi, _Rbrace, _Case, _Default)`（`parser.go:2728`）。
pub(crate) const STMT_LIST_FOLLOW: &[Follow] = &[
    SEMI,
    RBRACE,
    Follow::Kind(TokenKind::Keyword(Keyword::Case)),
    Follow::Kind(TokenKind::Keyword(Keyword::Default)),
];

/// `blockStmt` 缺 `{` 时的同步点 —— 官方 `advance(_Name, _Rbrace)`（`parser.go:2310`）。
pub(crate) const BLOCK_STMT_FOLLOW: &[Follow] = &[Follow::Name, RBRACE];

/// **三个闭括号**这个集合 —— 官方在**两处**写了同一份字面量：
/// `list` 缺分隔符（`:518`）与 `operand` 遇不认识的开头（`:1076`）。
///
/// 三个闭括号共享一个集合是有原因的：收哪种括号的**判定在调用点**，而错误路径
/// 不必（也无法）知道 —— 撞上任意一个都该停下。
///
/// # 为什么只留一份常量
///
/// 两处内容逐字相同。写两个同内容的常量 ⇒ 将来只改一处就会分叉，而分叉**不报错**
/// （只让恢复位置与 oracle 不一致，要到对撞才发现）。名字按**内容**取，不按用途取。
pub(crate) const CLOSE_RECOVER_FOLLOW: &[Follow] = &[RPAREN, RBRACK, RBRACE];

/// `advance(_Semi, _Rparen)` —— 官方 `:573`（`import` 缺路径）/ `:1147`（`.` 后不是名字）。
///
/// 同 [`CLOSE_RECOVER_FOLLOW`]：两处同内容，只留一份。
pub(crate) const SEMI_OR_RPAREN_FOLLOW: &[Follow] = &[SEMI, RPAREN];

/// 切片缺 `:` —— 官方 `advance(_Comma, _Colon, _Rbrack)`（`parser.go:1179`）。
pub(crate) const SLICE_RECOVER_FOLLOW: &[Follow] = &[COMMA, COLON, RBRACK];

/// `qualifiedName` 缺名字 —— 官方 `advance(_Dot, _Semi, _Rbrace)`（`parser.go:2808`）。
pub(crate) const QUALIFIED_NAME_FOLLOW: &[Follow] = &[DOT, SEMI, RBRACE];

/// `type_` 缺类型 —— 官方
/// `advance(_Comma, _Colon, _Semi, _Rparen, _Rbrack, _Rbrace)`（`parser.go:1331`）。
pub(crate) const TYPE_RECOVER_FOLLOW: &[Follow] = &[COMMA, COLON, SEMI, RPAREN, RBRACK, RBRACE];

// ── 类型族（`T-AP12`）────────────────────────────────────────────────────

/// 便捷常量：`[`。类型族的 `want(_Lbrack)` / `got(_Lbrack)` 用。
pub(crate) const LBRACK: Follow = Follow::Kind(TokenKind::Delim(Delim::LBrack));
/// 便捷常量：`...`（官方 `_DotDotDot`）。`[...]T` 与参数表里的 `...T` 用。
pub(crate) const ELLIPSIS: Follow = Follow::Kind(TokenKind::Delim(Delim::Ellipsis));
/// 便捷常量：`<-`（官方 `_Arrow`）。`chan<- T` 的方向判定用。
pub(crate) const ARROW: Follow = Follow::Plain(Operator::Arrow);
/// 便捷常量：`chan`。
pub(crate) const CHAN: Follow = Follow::Kind(TokenKind::Keyword(Keyword::Chan));
/// 便捷常量：`struct`。
pub(crate) const STRUCT: Follow = Follow::Kind(TokenKind::Keyword(Keyword::Struct));
/// 便捷常量：`interface`。
pub(crate) const INTERFACE: Follow = Follow::Kind(TokenKind::Keyword(Keyword::Interface));

/// `embeddedTerm` 缺项 —— 官方
/// `advance(_Operator, _Semi, _Rparen, _Rbrack, _Rbrace)`（`parser.go:1897`）。
///
/// ⚠️ 这是 [`Follow::Operator`] 的**唯一**消费者：`_Operator` 在这里当同步点用，
/// 因为类型约束的下一项（`|` 后面那一项）**也可能是 `~` 开头的**。
pub(crate) const EMBEDDED_TERM_FOLLOW: &[Follow] =
    &[Follow::Operator, SEMI, RPAREN, RBRACK, RBRACE];

// ── 控制流（`T-AP13`）────────────────────────────────────────────────────

/// 便捷常量：`case`。`caseClause`/`commClause` 的分派与 `switchStmt`/`selectStmt`
/// 的恢复点都用它。
pub(crate) const CASE: Follow = Follow::Kind(TokenKind::Keyword(Keyword::Case));
/// 便捷常量：`default`。
pub(crate) const DEFAULT: Follow = Follow::Kind(TokenKind::Keyword(Keyword::Default));
/// 便捷常量：`if`。`header` 的 `want` 与 `ifStmt` 的 `else if` 都用它。
pub(crate) const IF: Follow = Follow::Kind(TokenKind::Keyword(Keyword::If));
/// 便捷常量：`for`。`header` 的 `want(_For)` 用。
pub(crate) const FOR: Follow = Follow::Kind(TokenKind::Keyword(Keyword::For));
/// 便捷常量：`switch`。`header` 的 `want(_Switch)` 用。
pub(crate) const SWITCH: Follow = Follow::Kind(TokenKind::Keyword(Keyword::Switch));
/// 便捷常量：`select`。`selectStmt` 的 `want(_Select)` 用。
pub(crate) const SELECT: Follow = Follow::Kind(TokenKind::Keyword(Keyword::Select));
/// 便捷常量：`else`。`ifStmt` 的 `got(_Else)` 用。
pub(crate) const ELSE: Follow = Follow::Kind(TokenKind::Keyword(Keyword::Else));
/// 便捷常量：`var`。`header` 里「语句头部不许 var 声明」那条的 `got(_Var)` 用。
pub(crate) const VAR: Follow = Follow::Kind(TokenKind::Keyword(Keyword::Var));

/// `simpleStmt` 的 `default` 臂 —— 官方 `advance(_Semi, _Rbrace)`（`parser.go:2235`）。
///
/// 触发点是「`x, y` 这样的表达式列表后面跟了既不是 `=`/`:=` 也不是结束词元的东西」
/// （`expected := or = or comma`）。
pub(crate) const SEMI_OR_RBRACE_FOLLOW: &[Follow] = &[SEMI, RBRACE];

/// `header` 里 `want(_Lbrace)` 失败后 —— 官方 `advance(_Lbrace, _Rbrace)`（`parser.go:2394`）。
///
/// ⚠️ 与 [`BLOCK_STMT_FOLLOW`] **不同**（那是 `advance(_Name, _Rbrace)`）：
/// 这里只收 `{`/`}`，因为语句头部后面**必须**紧跟块，多收一个 `_Name` 会让
/// `if x y { }` 这类输入多吃一个词元、恢复位置与 oracle 分叉。
pub(crate) const LBRACE_OR_RBRACE_FOLLOW: &[Follow] = &[LBRACE, RBRACE];

/// `switchStmt` / `selectStmt` 缺 `{` —— 官方
/// `advance(_Case, _Default, _Rbrace)`（`parser.go:2504` / `:2526`）。
///
/// 同 [`CLOSE_RECOVER_FOLLOW`]：两处同内容，只留一份。
pub(crate) const CASE_DEFAULT_RBRACE_FOLLOW: &[Follow] = &[CASE, DEFAULT, RBRACE];

/// `caseClause` / `commClause` 认不出子句起始 —— 官方
/// `advance(_Colon, _Case, _Default, _Rbrace)`（`parser.go:2555` / `:2593`）。
///
/// ⚠️ 集合里有 `_Colon`：`default` 写歪成别的词时，先冲到 `:` 能让后面的
/// **裸写**（`c.Colon = p.pos()` 取的是当前位置）仍然落在冒号上。少了它会静默错位。
///
/// 同 [`CLOSE_RECOVER_FOLLOW`]：两处同内容，只留一份。
pub(crate) const COLON_CASE_DEFAULT_RBRACE_FOLLOW: &[Follow] = &[COLON, CASE, DEFAULT, RBRACE];

/// `StopOnFirst` 档用来**跳转**的负载。
///
/// 它**不是崩溃**，是控制流 —— 官方 `errorAt` 在 `errh == nil` 时 `panic(p.first)`（`F4`），
/// 由 `Parse` 外层的 `recover` 接住转成 first error（`F19`）。Rust 侧最贴近的直译就是
/// `panic_any` + `catch_unwind`。
///
/// # 为什么不用「在每个递归函数里检查一个中止标志」
///
/// 那种写法**漏检查不会报错** —— 它只会让解析在出错后继续多跑一段，
/// 而症状要到与 oracle 对撞时才暴露。`AGENTS.md` 记的「漏了不会报错」正是这个形状。
/// 栈展开则**不可能被漏掉**。
#[derive(Debug)]
pub(crate) struct Abort;

static QUIET_ABORT: OnceLock<()> = OnceLock::new();

/// 装一次「**只对我们的 abort 负载静音**」的 panic hook。
///
/// # 为什么需要
///
/// 默认 hook 会在**每次** `StopOnFirst` 解析出错时往 stderr 打一行
/// `thread '…' panicked at …`。那不是崩溃报告，却会被读成崩溃。
///
/// # 为什么是「只对我们的负载」
///
/// 全量静音会把**真实的断言失败**一起吞掉 —— 那正是 `AGENTS.md` 记的「静默失败」形状。
/// 所以这里只过滤 payload 是 [`Abort`] 的，其余**原样交给上一个 hook**。
pub(crate) fn install_quiet_abort_hook() {
    QUIET_ABORT.get_or_init(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if info.payload().downcast_ref::<Abort>().is_some() {
                return;
            }
            prev(info);
        }));
    });
}

impl Parser {
    /// 错误恢复：吃掉词元，直到落在 **followset** 上或撞到 EOF
    /// （`F2`，官方 `parser.go:345-375`）。
    ///
    /// 三条语义都不能少：
    ///
    /// 1. **EOF 永远在 followset 里** —— 不跳过 EOF。
    /// 2. `fnest > 0`（在函数体内）时 [`STOPSET`] 并入 —— 语句起始关键字是天然同步点。
    /// 3. **`followlist` 为空 ⇒ 恰好消费一个（非 EOF）词元**：保证一定前进。
    ///    少了这条，调用方在「当前词元不认识」时会原地死循环。
    ///
    /// ⚠️ 这条「空表时消费一个」的语义在官方注释里写得很直白
    /// （`if it is empty, exactly one (non-EOF) token is consumed to ensure progress`），
    /// 但它**不是**「什么都不做」—— 很容易读反。
    pub(crate) fn advance(&mut self, follow: &[Follow]) {
        let consume_one = follow.is_empty();
        loop {
            if self.at(&TokenKind::Eof) {
                break; // 1
            }
            if !consume_one {
                if self.fnest > 0 && self.at_stopset() {
                    break; // 2
                }
                if follow.iter().any(|f| f.matches(&self.tok.kind)) {
                    break;
                }
            }
            self.next();
            if consume_one {
                break; // 3
            }
        }
    }

    /// 当前词元是不是 [`STOPSET`] 里的关键字。
    pub(crate) fn at_stopset(&self) -> bool {
        match &self.tok.kind {
            TokenKind::Keyword(k) => STOPSET.contains(k),
            _ => false,
        }
    }

    /// 报告一个错误（`F4`，官方 `errorAt`）。
    ///
    /// 两档由 [`ErrMode`] 决定：
    ///
    /// - `StopOnFirst` —— 记下 `first` 后 `panic_any`，由 [`crate::parse_file_with`]
    ///   的 `catch_unwind` 接住并转成 first error 返回（`F19`）。
    /// - `Continue` —— 入 [`rgoc_lex::diag::DiagBag`] 后继续。**M2 的主判据档**。
    ///
    /// 两档都**先把诊断记进 `diags`**，所以「`diags` 非空」在两档下都是可观察的，
    /// `StopOnFirst` 只是**恰好一条**。
    pub(crate) fn error_at(&mut self, pos: Pos, code: DiagCode, message: String) {
        let d = Diag { pos, code, message };
        if self.first.is_none() {
            self.first = Some(d.clone()); // F19：外层 recover 转成 first error 返回
        }
        self.errcnt += 1;
        // 记账**在分档之前** —— 两档都留痕，所以「`diags` 非空」在两档下都可观察，
        // `StopOnFirst` 只是恰好一条。顺序写反（先 panic 后记账）会让首错在流里凭空消失。
        self.diags.push(d);
        if self.mode == ErrMode::StopOnFirst {
            std::panic::panic_any(Abort);
        }
    }

    /// 报告一个**分支检查**诊断（`T-AP16`，官方 `branches.go:64-66` 的 `ls.errf`）。
    ///
    /// 官方 `checkBranches` 的 `errh` 就是 `p.errh` —— 也就是说这些诊断
    /// **不走** `syntaxErrorAt`（既没有 `syntax error: ` 前缀，也**不受** EOF 抑制），
    /// 它们是**纯消息**。
    ///
    /// ⚠️ 这与官方**完全一致**：标点、EOF 抑制、`<tok>` 拼接**一样都没有**。
    /// 一条 `label X not defined` 在任何位置都是这条消息。
    pub(crate) fn report_branch(&mut self, pos: Pos, code: DiagCode, message: String) {
        self.error_at(pos, code, message);
    }

    /// 报告一个**语法**错误（`F5` + `T-AP16`，官方 `syntaxErrorAt`，`parser.go:252-297`）。
    ///
    /// # 三支判据（`:262-273`）——**不是**「一律加前缀」
    ///
    /// ```text
    /// msg == ""                           → 什么都不加（但仍拼 <tok>）
    /// HasPrefix(msg, "in "/"at "/"after ") → msg = " "  + msg
    /// HasPrefix(msg, "expected ")         → msg = ", " + msg
    /// 其余                                → 直接 "syntax error: " + msg，**不拼 <tok>**
    /// ```
    ///
    /// 最后一条（`default` 支，`p.errorAt(pos, "syntax error: "+msg)` 后 `return`）
    /// 是**唯一**不引用当前词元的出口 —— 例：官方 `parser.go:465` 的
    /// `unexpected semicolon or newline before {`。
    ///
    /// ⚠️ **判据是消息的**前缀，不是「错误种类」** —— 这就是为什么
    /// `expected type`（走第二类）会变成 `unexpected <tok>, expected type`，
    /// 而 `in type declaration`（走第一类）只变成 `unexpected <tok> in type declaration`。
    /// 写成一个「按 `DiagCode` 决定形态」的函数，症状是**某些消息多了逗号、
    /// 某些少了**，逐条断言才照得出。
    ///
    /// # EOF 抑制（`F5`，`:257-259`）在前
    ///
    /// 官方那是整个函数的**第一**行、早于三支判据 ⇒ `p.tok == _EOF && p.first != nil`
    /// 时**任何** `syntaxError` 都不报。写反了（放在三支之后）会让 `default` 支
    /// 绕过抑制，EOF 处多喷一串诊断。
    pub(crate) fn syntax_error_at(&mut self, pos: Pos, code: DiagCode, msg: &str) {
        // F5：已经在 EOF 且**已经有错**时不再报 —— 否则文件尾会喷一串无意义诊断。
        // 注意判据是「EOF **且已出错**」两个条件的**合取**：只判 EOF 会把
        // 「本来就只有 EOF 错」的正常情况也吞掉。
        if self.at(&TokenKind::Eof) && self.first.is_some() {
            return;
        }
        if msg.is_empty() {
            // `:263-264`：什么都不加 —— 但**照样**拼 `<tok>`（落到下面的公共尾部）。
        } else if msg.starts_with("in ") || msg.starts_with("at ") || msg.starts_with("after ") {
            // `:265-266`：前置**一个空格**（不是 `", "`）。
            let tok = self.tok_desc();
            self.error_at(pos, code, format!("syntax error: unexpected {tok} {msg}"));
            return;
        } else if msg.starts_with("expected ") {
            // `:267-268`：前置 `", "`。
            let tok = self.tok_desc();
            self.error_at(pos, code, format!("syntax error: unexpected {tok}, {msg}"));
            return;
        } else {
            // `:269-273`：普通错误 —— **不关心当前词元**，直接报。
            self.error_at(pos, code, format!("syntax error: {msg}"));
            return;
        }
        // `msg == ""` 才会到这里（其余三支都 return 了）。
        let tok = self.tok_desc();
        self.error_at(pos, code, format!("syntax error: unexpected {tok}"));
    }

    /// 当前词元在 `unexpected X` 里的 `X`（官方 `parser.go:275-293`）。
    ///
    /// 官方六档：
    ///
    /// ```text
    /// _Name      → "name "    + p.lit
    /// _Semi      → p.lit                    ← "newline" / "semicolon" / "EOF"
    /// _Literal   → "literal " + p.lit
    /// _Operator  → p.op.String()
    /// _AssignOp  → p.op.String() + "="
    /// _IncOp     → p.op.String()             ← 注意：不加 "="
    /// 其余        → tokstring(p.tok)
    /// ```
    ///
    /// ⚠️ `_Semi` 用的是 **`p.lit`** 而不是 `tokstring`：官方 `scanner.go:121/154/113`
    /// 分别把它设成 `"newline"` / `"semicolon"` / `"EOF"`。这三个字符串**都不是** `";"`
    /// ⇒ 一个「统一用 `;`」的实现**不会被** `tokstring` 那几条用例照出来
    /// （那些走的是 `tokstring` 分支，不是本分支）。
    fn tok_desc(&self) -> String {
        match &self.tok.kind {
            TokenKind::Ident(b) => format!("name {}", lit_text(b)),
            TokenKind::Semi(origin) => semi_lit(*origin).to_string(),
            TokenKind::Literal { text, .. } => format!("literal {}", lit_text(text)),
            TokenKind::Op { form, op, .. } => match form {
                OpForm::Assign => concat_op(*op).to_string(),
                // `_Operator` 与 `_IncOp` **都是**裸 `op.String()`
                // （`:285` / `:289`）—— 这两条只差 `_AssignOp` 加的那个 `"="`。
                OpForm::Plain | OpForm::IncDec => op.as_str().to_string(),
            },
            // 其余走 `tokstring`：`Follow` 的 `matches` 是「类别」判定，
            // 这里需要一个能装当前词元的 `Follow`。分隔符/关键字/EOF 直接造。
            k => tokstring(&Follow::from_kind(k)),
        }
    }
}

/// 官方 `scanner.go` 给 `_Semi` 定的三个 `lit` 值。
///
/// ⚠️ **官方对显式 `;` 说的是 `"semicolon"` 而不是 `";"`** —— `p.lit` 是
/// 「这个词元的字面/类别描述」，标点自己写自己会读成 `unexpected ;`，
/// 而官方要的是 `unexpected semicolon`。
fn semi_lit(origin: SemiOrigin) -> &'static str {
    match origin {
        SemiOrigin::Explicit => "semicolon",
        SemiOrigin::Newline => "newline",
        SemiOrigin::Eof => "EOF",
    }
}

#[cfg(test)]
mod tests {
    use rgoc_lex::token::SemiOrigin;

    use super::*;

    fn parser(src: &[u8]) -> Parser {
        Parser::new(src, "t.go", ErrMode::Continue)
    }

    // ── `advance` 的边界语义 ────────────────────────────────────────────────
    //
    // 为什么这些放在**单元测试**而不是 `tests/test_recovery.rs`：
    // `T-AP06` 还没有任何调用方用空 followlist（第一次出现要等 `T-AP07` 之后
    // 「当前词元不认识、只能硬啃一个」的分支），所以在 `parse_file` 外面**看不见**它们。
    // 而这两条恰恰最值得现在钉住 —— 写反了会让调用方原地死循环。

    /// 空 followlist ⇒ **恰好消费一个**词元（官方注释：`exactly one (non-EOF) token`）。
    #[test]
    fn advance_空表恰好消费一个词元() {
        let mut p = parser(b"a b c");
        assert!(matches!(p.tok.kind, TokenKind::Ident(_)));
        p.advance(&[]);
        match &p.tok.kind {
            TokenKind::Ident(bytes) => assert_eq!(bytes, b"b", "应前移一个词元到 `b`"),
            other => panic!("应停在第二个标识符，got {other:?}"),
        }
    }

    /// 空 followlist 遇上 EOF：**一个都不消费**（EOF 不跳过），也不能越界。
    #[test]
    fn advance_空表在_eof_处不动() {
        let mut p = parser(b"");
        p.advance(&[]);
        assert!(p.at(&TokenKind::Eof));
    }

    /// 空 followlist 在**非 EOF** 处一定前进 —— 这条是「不死循环」的保证。
    #[test]
    fn advance_空表保证前进() {
        let mut p = parser(b"} } }");
        let before = p.tok.pos;
        p.advance(&[]);
        assert_ne!(p.tok.pos, before, "空表必须消费一个词元");
    }

    /// 非空 followlist：**当前词元已在 followset 里 ⇒ 一个都不消费**。
    ///
    /// 这条与上一条是一对：官方是 `for !contains(followset, tok)` ——
    /// 先判后动，所以落在同步点上时 `advance` 是**空操作**。
    #[test]
    fn advance_非空表在_follow_上时是空操作() {
        let mut p = parser(b"} }");
        let before = p.tok.pos;
        p.advance(&[Follow::Kind(TokenKind::Delim(Delim::RBrace))]);
        assert_eq!(p.tok.pos, before, "已在 followset 上 ⇒ 不消费");
    }

    /// `fnest > 0`（函数体内）时 `stopset` **并入**：语句起始关键字也让它停下。
    #[test]
    fn advance_函数体内_stopset_生效() {
        let mut p = parser(b"} return");
        p.fnest = 1;
        p.advance(&[Follow::Kind(TokenKind::Keyword(Keyword::Import))]);
        assert!(
            p.at(&TokenKind::Keyword(Keyword::Return)),
            "`}}` 被吃掉，`return` 在 stopset 里 ⇒ 停在它上面，got {:?}",
            p.tok.kind
        );
    }

    /// 对照：`fnest == 0`（函数体外）时 `stopset` **不**并入 —— 只认 followlist。
    ///
    /// 少了这条对照，上面那条测试在「实现干脆无条件并入 stopset」时仍然是绿的。
    #[test]
    fn advance_函数体外_stopset_不生效() {
        let mut p = parser(b"} return import");
        p.advance(&[Follow::Kind(TokenKind::Keyword(Keyword::Import))]);
        assert!(
            p.at(&TokenKind::Keyword(Keyword::Import)),
            "`return` 不该被当同步点，应一路走到 followlist 里的 `import`，got {:?}",
            p.tok.kind
        );
    }

    // ── `Follow` 的**类别**语义 ──────────────────────────────────────────────
    //
    // 为什么值得单独钉：`Follow::Kind` 是**按值**比较，而三个类别成员是**按形状**比较。
    // 「类别判窄了」的症状是恢复时**提前停下**（错误消息位置与 oracle 分叉）；
    // 「判宽了」的症状是多啃词元。两者都不会 panic，只能靠断言抓。

    /// `Follow::Name` 认**任意**标识符 —— 包括**非法 UTF-8** 的原文
    /// （`Ident` 存 `Vec<u8>` 正是为了这个）。
    #[test]
    fn follow_类别_name_认任意标识符() {
        assert!(Follow::Name.matches(&TokenKind::Ident(b"x".to_vec())));
        assert!(
            Follow::Name.matches(&TokenKind::Ident(vec![0xff])),
            "非法 UTF-8 的标识符也是 `_Name`"
        );
        assert!(!Follow::Name.matches(&TokenKind::Keyword(Keyword::Var)));
        assert!(!Follow::Name.matches(&TokenKind::Semi(SemiOrigin::Explicit)));
    }

    /// `Follow::Semi` 认**任意来源**的分号（显式 / 换行自动补 / 文件尾补）。
    /// 若按值比较（只认 `Explicit`），换行自动补的分号会被漏掉 —— 而 Go 里
    /// **绝大多数分号都是自动补的**，这种漏法在真实语料上几乎必然发生。
    #[test]
    fn follow_类别_semi_认任意来源() {
        for o in [SemiOrigin::Explicit, SemiOrigin::Newline, SemiOrigin::Eof] {
            assert!(
                Follow::Semi.matches(&TokenKind::Semi(o)),
                "{o:?} 也是 `_Semi`"
            );
        }
    }

    /// `Follow::Operator` 的边界 == 官方的折叠边界（`tokens.go`）。
    ///
    /// **双向钉**：判窄了（漏掉 `&&`/`!` 等）要红，判宽了（把 `*`/`=`/`:=`/`<-`/`+=`/`++`
    /// 混进来）也要红。只写「`+` 能中」半边是抓不住「宽」那一侧的。
    #[test]
    fn follow_类别_operator_边界与官方一致() {
        let plain = |op| TokenKind::Op {
            form: OpForm::Plain,
            op,
            prec: 0,
        };
        for op in [Operator::Add, Operator::And, Operator::Not, Operator::Shl] {
            assert!(
                Follow::Operator.matches(&plain(op)),
                "{op:?} 是 `_Operator`"
            );
        }
        // 官方把这四个单独列为 _Star / _Assign / _Define / _Arrow ⇒ 不在 `_Operator` 里
        for op in [
            Operator::Mul,
            Operator::Assign,
            Operator::Define,
            Operator::Arrow,
        ] {
            assert!(
                !Follow::Operator.matches(&plain(op)),
                "{op:?} 不是 `_Operator`"
            );
        }
        // `_AssignOp` / `_IncOp` 也是独立 token
        for form in [OpForm::Assign, OpForm::IncDec] {
            assert!(
                !Follow::Operator.matches(&TokenKind::Op {
                    form,
                    op: Operator::Add,
                    prec: 0
                }),
                "{form:?} 不是 `_Operator`"
            );
        }
    }

    /// `Follow::Kind` 是**按值**比较：`}` 不认 `{`，也不认一个内容恰是 `}` 的标识符。
    #[test]
    fn follow_类别_kind_按值比较() {
        let f = Follow::Kind(TokenKind::Delim(Delim::RBrace));
        assert!(f.matches(&TokenKind::Delim(Delim::RBrace)));
        assert!(!f.matches(&TokenKind::Delim(Delim::LBrace)));
        assert!(!f.matches(&TokenKind::Ident(b"}".to_vec())));
    }

    /// 恢复点 ② / `blockStmt` 的两个 followlist 逐条对官方（`parser.go:2728` / `:2310`）。
    /// 写死断言是为了：改成员时这里会红，而不是悄悄改掉恢复行为。
    #[test]
    fn 两个恢复点的_followlist_与官方逐条一致() {
        assert_eq!(STMT_LIST_FOLLOW.len(), 4);
        assert!(STMT_LIST_FOLLOW.contains(&SEMI));
        assert!(STMT_LIST_FOLLOW.contains(&RBRACE));
        assert!(STMT_LIST_FOLLOW.contains(&Follow::Kind(TokenKind::Keyword(Keyword::Case))));
        assert!(STMT_LIST_FOLLOW.contains(&Follow::Kind(TokenKind::Keyword(Keyword::Default))));

        assert_eq!(BLOCK_STMT_FOLLOW.len(), 2);
        assert!(BLOCK_STMT_FOLLOW.contains(&Follow::Name));
        assert!(BLOCK_STMT_FOLLOW.contains(&RBRACE));
    }
}
