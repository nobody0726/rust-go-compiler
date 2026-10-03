//! 指令行解析（规则 R1）与分派顺序（规则 R1b）。
//!
//! 规则全部来自 `go_source_code/src/cmd/internal/testdir/testdir_test.go`，
//! 逐条标注了行号；构建约束判定对齐 `go1.27.1` 的 `go/build/constraint/expr.go`。
//!
//! **本模块存在的理由**：把「怎么读指令行」这件事从 harness 编排层里单独拎出来，
//! 因为它是**最容易读错、且读错后后果最严重**的一步 —— 读错就会在真实语料上
//! 误报「未知指令硬失败」（见 [`dispatch`] 的顺序契约）。

use crate::ir::Mode;

/// 官方 `switch action` 里的 **16 个**指令（M0-tests §1.2）。
///
/// **不含 `skip`** —— 它不是「模式」而是「上游设计即跳过」（官方 `:552` 直接 `t.Skip`），
/// 由 [`dispatch`] 单独处理。`03` §4 要求 harness「枚举所有指令」，枚举依据就是这张表。
pub const KNOWN_COMMANDS: [&str; 16] = [
    // 编译/运行（:542）
    "compile",
    "compiledir",
    "build",
    "builddir",
    "buildrundir",
    "run",
    "buildrun",
    "runoutput",
    "rundir",
    "runindir",
    "asmcheck",
    // 错误检查（:550）
    "errorcheck",
    "errorcheckdir",
    "errorcheckoutput",
    // 组合（:544 / :546）
    "errorcheckandrundir",
    "errorcheckwithauto",
];

/// R1 的产物：解析出的指令名 + 参数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instruction {
    /// 第一个「非空且非构建约束」行去掉 `//` 并去空白后的内容（可能**不是**合法指令）
    pub action: String,
    /// 引号感知分词得到的参数（官方 `splitQuoted`，`:2009`）
    pub args: Vec<String>,
}

/// R1 与 `splitQuoted` 的硬失败。**四种都必须硬失败，不得静默跳过。**
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    /// `.go` 文件以换行开头（官方 `:497` `t.Fatal`）
    LeadingNewline,
    /// 找不到 execution recipe（官方 `:514` `t.Fatalf("execution recipe not found")`）
    NoRecipe,
    /// 引号未闭合（官方 `:2047`）
    UnclosedQuote,
    /// 转义未完成（官方 `:2050`）
    UnfinishedEscaping,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LeadingNewline => write!(f, ".go 文件以换行开头"),
            Self::NoRecipe => write!(f, "找不到 execution recipe"),
            Self::UnclosedQuote => write!(f, "指令行里有未闭合的引号"),
            Self::UnfinishedEscaping => write!(f, "指令行里有未完成的转义"),
        }
    }
}

impl std::error::Error for ParseError {}

/// [`dispatch`] 的结果。
///
/// 为什么不直接把结果做成 [`Mode`]：官方在这条路上有三个**互不相同的出口** ——
/// 被平台过滤、上游设计即跳过、正常执行。把前两者硬塞进 `Mode` 会让
/// 「这个用例到底跑没跑」变得不可读（它们不是「跑哪种模式」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dispatch {
    /// 继续执行该用例
    Proceed(Mode),
    /// 被平台过滤（官方 `:522` `t.Skip(why)`）→ 上层记 [`crate::ir::Verdict::TargetFiltered`]
    TargetFiltered,
    /// 上游设计即跳过（官方 `:552` `t.Skip("skip")`，U13）
    SkippedByDesign,
}

/// 分派失败 —— 对应官方的 `t.Fatalf("unknown pattern: %q")`（`:558`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchError {
    /// action 不在 16 个指令里
    UnknownAction(String),
}

impl std::fmt::Display for DispatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownAction(a) => write!(f, "unknown pattern: {a:?}"),
        }
    }
}

impl std::error::Error for DispatchError {}

/// 判断某行是否是 `//go:build` 构建约束。
///
/// 照 `go/build/constraint` 的 `splitGoBuild`（`expr.go:172-198`）逐条实现，
/// 边界与官方**完全一致**，因为「像不像约束」判错会让 action 取错行：
///
/// - 前缀必须**紧贴行首**：`//go:build linux` 是约束，`  //go:build linux`（缩进）不是；
/// - 前缀之后必须有空白或整行只有它：`//go:buildsomethingelse` **不是**约束。
pub fn is_go_build_line(line: &str) -> bool {
    let Some(s) = strip_one_trailing_newline(line) else {
        return false;
    };
    // ⚠️ 前缀判定的基准是**未经 trim 的整行** —— 官方 `expr.go:181` 的 HasPrefix
    // 发生在 TrimSpace **之前**。所以缩进的 `  //go:build linux` 不是构建约束；
    // 判错这条会让 harness 跳过本该当作 action 的那一行。
    let Some(rest) = s.strip_prefix("//go:build") else {
        return false;
    };
    // 官方等价物：`TrimSpace(整行)` 之后才截前缀，所以截出来的部分只可能带**前导**空白。
    // 「长度不变且非空」⇒ 前缀后紧贴内容（`//go:buildsomethingelse`）⇒ 不是约束行。
    let content = rest.trim_start();
    content.is_empty() || content.len() != rest.len()
}

/// 判断某行是否是 `// +build`（旧式）构建约束。
///
/// 照 `splitPlusBuild`（`expr.go:367-398`）：`//` 之后**空格可选**，
/// 所以 `//+build linux` 也算（官方注释里明确写了这一点）。
pub fn is_plus_build_line(line: &str) -> bool {
    let Some(s) = strip_one_trailing_newline(line) else {
        return false;
    };
    // 同 is_go_build_line：HasPrefix 判的是未经 trim 的整行（`expr.go:376`）
    let Some(rest) = s.strip_prefix("//") else {
        return false;
    };
    // 官方：剥掉 `//` 后先 TrimSpace（**加号前的空格可选**，`//+build` 也认），
    // 再要求 `+build` 前缀
    let rest = rest.trim();
    let Some(rest) = rest.strip_prefix("+build") else {
        return false;
    };
    let content = rest.trim_start();
    content.is_empty() || content.len() != rest.len()
}

/// 去掉**一个**结尾换行。
///
/// 返回 `None` 表示「这一行里还有换行」—— 官方两个 split 函数都是这么判的
/// （`expr.go:173-179` / `369-375`：单个结尾换行可以，内部换行不行）。
///
/// ⚠️ 判据是「去掉结尾换行后**还含不含**换行」，不含才返回 `Some`。
/// 反过来写会让**所有单行输入都被当成含换行**，于是构建约束一个也识别不出来
/// （T32 实测踩过：`is_go_build_line` 恒返回 false）。
fn strip_one_trailing_newline(line: &str) -> Option<&str> {
    let s = line.strip_suffix('\n').unwrap_or(line);
    if s.contains('\n') { None } else { Some(s) }
}

/// 引号感知的分词 —— 照官方 `splitQuoted`（`:2009-2052`）。
///
/// 与 `split_whitespace` 的差别就是全部意义所在：
/// **引号内的空格不分词**，而指令参数里出现带空格的路径/字符串是常态。
pub fn split_quoted(s: &str) -> Result<Vec<String>, ParseError> {
    let mut args: Vec<String> = Vec::new();
    let mut arg = String::new();
    let mut escaped = false;
    // `quoted` 是**粘性**的：一旦见过引号，直到遇到空白才复位
    let mut quoted = false;
    let mut quote: Option<char> = None;

    for c in s.chars() {
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
            continue;
        } else if let Some(q) = quote {
            // 官方是 `if rune == quote { quote = 0; continue }` —— **`continue` 不能少**：
            // 少了它，闭合引号本身会被塞进参数（实测踩过：得到 "foo bar\""）。
            if c == q {
                quote = None;
                continue;
            }
            // 引号内的空格**不是**分隔符：Go 的 switch 里 `case quote != 0` 排在
            // `case unicode.IsSpace` 之前，命中前者就不会走「切分」分支。
        } else if c == '"' || c == '\'' {
            quoted = true;
            quote = Some(c);
            continue;
        } else if c.is_whitespace() {
            if quoted || !arg.is_empty() {
                quoted = false;
                args.push(std::mem::take(&mut arg));
            }
            continue;
        }
        arg.push(c);
    }
    if quoted || !arg.is_empty() {
        args.push(arg);
    }
    if quote.is_some() {
        return Err(ParseError::UnclosedQuote);
    }
    if escaped {
        return Err(ParseError::UnfinishedEscaping);
    }
    Ok(args)
}

/// **R1**：解析 execution recipe（官方 `run()` 的 `:502-515`）。
///
/// ```text
/// 1. .go 文件不得以换行开头                     :497  → LeadingNewline
/// 2. 从头逐行扫描，//go:build 与 // +build 行跳过
/// 3. action = TrimSpace(TrimPrefix(line, "//"))  ← 第一个「非空且非构建约束」的行
/// 4. action 为空 → 硬失败                        :514  → NoRecipe
/// ```
///
/// 两条容易实现错的推论（都写进了测试）：
///
/// - **不能假定指令在第 1 行**，也不能假定它前面只有注释 —— 空行同样会被跳过；
/// - 一个光秃秃的 `//` 行解析出空串后**继续往下扫**（官方循环条件是 `action == ""`），
///   而不是停下报错。
///
/// 注意第 3 步**不要求**行首是注释：官方对 `package main` 这类行照样取整行当 action，
/// 错误要到 `switch` 阶段才暴露（见 [`dispatch`]）。
pub fn parse_action(src: &str) -> Result<Instruction, ParseError> {
    if src.starts_with('\n') {
        return Err(ParseError::LeadingNewline);
    }
    for raw in src.split_inclusive('\n') {
        let line = raw.strip_suffix('\n').unwrap_or(raw);
        if is_go_build_line(line) || is_plus_build_line(line) {
            continue;
        }
        let action = line.strip_prefix("//").unwrap_or(line).trim();
        if action.is_empty() {
            continue; // 官方循环条件是 action == ""，空串继续往下扫
        }
        let parts = split_quoted(action)?;
        let mut it = parts.into_iter();
        let action = it.next().unwrap_or_default();
        return Ok(Instruction {
            action,
            args: it.collect(),
        });
    }
    Err(ParseError::NoRecipe)
}

/// 把指令名映射到 [`Mode`]（不在 16 个里则 `None`）。
pub fn mode_of(action: &str) -> Option<Mode> {
    Some(match action {
        "compile" => Mode::Compile,
        "compiledir" => Mode::CompileDir,
        "build" => Mode::Build,
        "builddir" => Mode::BuildDir,
        "buildrundir" => Mode::BuildRunDir,
        "run" => Mode::Run,
        "buildrun" => Mode::BuildRun,
        "runoutput" => Mode::RunOutput,
        "rundir" => Mode::RunDir,
        "runindir" => Mode::RunIndir,
        "asmcheck" => Mode::AsmCheck,
        "errorcheck" => Mode::ErrorCheck,
        "errorcheckdir" => Mode::ErrorCheckDir,
        "errorcheckoutput" => Mode::ErrorCheckOutput,
        "errorcheckandrundir" => Mode::ErrorCheckAndRunDir,
        "errorcheckwithauto" => Mode::ErrorCheckWithAuto,
        _ => return None,
    })
}

/// **R1b + switch**：按官方顺序分派。
///
/// ```text
/// 1. 平台过滤 shouldTest(header, goos, goarch) → t.Skip(why)   :517-524
/// 2. switch action                                             :541-558
///      case "skip" → t.Skip("skip")                            :552-556
///      default     → t.Fatalf("unknown pattern: %q")            :558
/// ```
///
/// ⚠️ **顺序是硬契约**：`platform_ok` 是**必填参数**而不是内部计算，
/// 就是为了让「先过滤、后判指令」这件事在类型层面无法颠倒。理由：
/// 顶层 `test/linkmain.go` 首行是 `//go:build ignore`，按 R1 跳过后
/// 下一条注释被当成 action（`Copyright 2015 …`）—— 一个**非法指令**；
/// 但官方因为**先**做平台过滤而 `t.Skip`，从不 `Fatalf`。
/// 顺序颠倒的 harness 会在真实语料上误报「未知指令硬失败」。
///
/// 本函数**不做**平台过滤本身（`shouldTest` 需要 build 约束求值 + GOOS/GOARCH，
/// 属于 T33 的 `corpus.rs`），只接受它的结论。
pub fn dispatch(ins: &Instruction, platform_ok: bool) -> Result<Dispatch, DispatchError> {
    // 官方顺序：平台过滤在 switch 之前（:522 早于 :541）
    if !platform_ok {
        return Ok(Dispatch::TargetFiltered);
    }
    if ins.action == "skip" {
        return Ok(Dispatch::SkippedByDesign);
    }
    mode_of(&ins.action)
        .map(Dispatch::Proceed)
        .ok_or_else(|| DispatchError::UnknownAction(ins.action.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 指令名 ↔ Mode 是**双射**：16 个指令名都能查到，且查到的 Mode 的
    /// `name()` 恰好等于原名。这条防的是「加了 Mode 变体却忘了加指令名」
    /// （或反过来）—— 那种漏项会让真实语料里的文件被误判成未知指令。
    #[test]
    fn 指令名与_mode_互为逆映射() {
        // 16 个指令名 + skip == 全部 Mode 变体。数组长度是编译期属性，
        // 纯文本断言查不出来，所以这条在**运行时**兜住漏项。
        assert_eq!(
            KNOWN_COMMANDS.len() + 1,
            Mode::ALL.len(),
            "KNOWN_COMMANDS（{} 个）+ skip 应等于 Mode 的全部变体（{} 个）",
            KNOWN_COMMANDS.len(),
            Mode::ALL.len()
        );
        for m in Mode::ALL {
            if m == Mode::Skip {
                continue; // skip 不是模式，由 dispatch 单独处理
            }
            assert!(
                KNOWN_COMMANDS.contains(&m.name()),
                "Mode::{m:?}（指令名 {:?}）不在 KNOWN_COMMANDS 里 —— 加变体时漏了指令名",
                m.name()
            );
        }
        for name in KNOWN_COMMANDS {
            let m = mode_of(name).unwrap_or_else(|| panic!("{name} 没有对应 Mode"));
            assert_eq!(m.name(), name, "{name} 的 Mode.name() 必须是它自己");
        }
        // v0 支持集必须恰好是 Run / Compile / ErrorCheck 三个（**集合**比较：
        // KNOWN_COMMANDS 的排列顺序照官方 :542/:550，不是按字母排的）
        let mut v0: Vec<&str> = KNOWN_COMMANDS
            .iter()
            .copied()
            .filter(|n| mode_of(n).is_some_and(Mode::is_v0_supported))
            .collect();
        v0.sort_unstable();
        assert_eq!(v0, vec!["compile", "errorcheck", "run"]);
    }

    /// `dispatch` 在平台放行时**绝不**返回 `TargetFiltered`，
    /// 不放行时**绝不**返回 `Proceed` —— 这就是顺序契约的另一半。
    #[test]
    fn dispatch_的两个出口互斥() {
        let run = parse_action("// run\n").unwrap();
        assert!(matches!(
            dispatch(&run, true),
            Ok(Dispatch::Proceed(Mode::Run))
        ));
        assert!(matches!(
            dispatch(&run, false),
            Ok(Dispatch::TargetFiltered)
        ));

        let bad = Instruction {
            action: "Copyright 2015 …".into(),
            args: vec![],
        };
        assert!(matches!(
            dispatch(&bad, true),
            Err(DispatchError::UnknownAction(_))
        ));
        assert!(matches!(
            dispatch(&bad, false),
            Ok(Dispatch::TargetFiltered)
        ));
    }

    #[test]
    fn 参数字面量_往返() {
        // 官方 T29 冻结的排除参数形态之一：`-gcflags=-l=4` 不该被拆成两个
        let i = parse_action("// run -gcflags=-l=4\n").unwrap();
        assert_eq!(i.args, vec!["-gcflags=-l=4"]);
        // 转义：官方 splitQuoted 遇到 \ 会跳过下一个字符的「特殊判定」
        let v = split_quoted(r#"a\ b"#).unwrap();
        assert_eq!(v, vec!["a b"], r"转义空格不分词");
    }
}
