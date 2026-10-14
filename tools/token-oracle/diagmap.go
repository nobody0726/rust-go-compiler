// 官方诊断消息 → 本仓 `DiagCode` 的映射表，外加一张「码表全量」自检。
//
// # 为什么要有自检
//
// 第一版映射表（`M1-design.md` §7.2）是**凭印象手写**的 19 条，漏了 4 条：
//
//	invalid digit %q in %s literal        → LEX-BAD-DIGIT
//	invalid radix point in %s literal     → LEX-BAD-RADIX-POINT
//	newline in string                     → LEX-STRING-NEWLINE
//	invalid character %q in %s escape     → LEX-ESCAPE-BAD-DIGIT
//
// 漏掉时**不会报错** —— 那 4 条消息一旦出现就被当成"没见过的错误"，行为未定义。
// 这与 `AGENTS.md` 记的「写死的键列表会变成盲区」（T38/T39/T47 三次同形）是同一个形状。
//
// 所以本表**由枚举得到**而不是凭印象写：把 scanner.go / source.go 里全部
// `errorf` / `errorAtf` / `error` 调用点列出来，逐条给代号。
// 再由 `--selfcheck` 做两件事：
//
//	① 映射表用到的代号必须都在码表里（防拼错／防映射到不存在的代号）
//	② 码表里除 `LEX-EXCEEDS-FILE-LIMIT`（本仓自己的限制，官方无对应）外，
//	   每个代号都必须**至少有一条官方消息**能产出它（防"永远不会被产出的死代号"）
package main

import (
	"fmt"
	"strings"
)

// 官方消息模板 → 代号。**按顺序匹配**，更具体的在前
// （三条 `invalid character …` 必须靠具体后缀区分，否则会全落到 LEX-BAD-CHAR）。
var diagMappings = []struct {
	template string
	match    func(string) bool
	code     string
}{
	// ── source.go（字符解码层）────────────────────────────────
	{"invalid NUL character", exact("invalid NUL character"), "LEX-INVALID-NUL"},
	{"invalid UTF-8 encoding", exact("invalid UTF-8 encoding"), "LEX-INVALID-UTF8"},
	{"invalid BOM in the middle of the file", exact("invalid BOM in the middle of the file"), "LEX-BOM-MIDFILE"},

	// ── scanner.go：注释与字符串／rune ────────────────────────
	{"comment not terminated", exact("comment not terminated"), "LEX-COMMENT-UNTERMINATED"},
	{"string not terminated", exact("string not terminated"), "LEX-STRING-UNTERMINATED"},
	{"newline in string", exact("newline in string"), "LEX-STRING-NEWLINE"},
	{"empty rune literal or unescaped '", exact("empty rune literal or unescaped '"), "LEX-RUNE-EMPTY"},
	{"more than one character in rune literal", exact("more than one character in rune literal"), "LEX-RUNE-MULTI"},
	{"newline in rune literal", exact("newline in rune literal"), "LEX-RUNE-NEWLINE"},
	{"rune literal not terminated", exact("rune literal not terminated"), "LEX-RUNE-UNTERMINATED"},

	// ── scanner.go：转义 ─────────────────────────────────────
	{"unknown escape", exact("unknown escape"), "LEX-ESCAPE-UNKNOWN"},
	{"invalid character %q in %s escape", hasSuffix(" escape"), "LEX-ESCAPE-BAD-DIGIT"},
	{"octal escape value %d > 255", hasPrefix("octal escape value"), "LEX-ESCAPE-OCTAL-OVER"},
	{"escape is invalid Unicode code point %#U", hasPrefix("escape is invalid Unicode code point"), "LEX-ESCAPE-RANGE"},

	// ── scanner.go：数字字面量 ────────────────────────────────
	{"exponent has no digits", exact("exponent has no digits"), "LEX-BAD-EXPONENT"},
	{"hexadecimal mantissa requires a 'p' exponent", exact("hexadecimal mantissa requires a 'p' exponent"), "LEX-BAD-EXPONENT"},
	{"%q exponent requires decimal mantissa", hasSuffix("exponent requires decimal mantissa"), "LEX-BAD-EXPONENT"},
	{"%q exponent requires hexadecimal mantissa", hasSuffix("exponent requires hexadecimal mantissa"), "LEX-BAD-EXPONENT"},
	{"%s literal has no digits", hasSuffix("literal has no digits"), "LEX-LIT-NO-DIGITS"},
	{"invalid digit %q in %s literal", hasPrefix("invalid digit"), "LEX-BAD-DIGIT"},
	{"invalid radix point in %s literal", hasPrefix("invalid radix point in"), "LEX-BAD-RADIX-POINT"},
	{"'_' must separate successive digits", exact("'_' must separate successive digits"), "LEX-BAD-SEP"},

	// ── scanner.go：标识符与非法字符 ──────────────────────────
	{"identifier cannot begin with digit %#U", hasPrefix("identifier cannot begin with digit"), "LEX-ID-LEADING-DIGIT"},
	// 注意顺序：`in identifier` 与 ` in … escape` 都必须排在裸的 `invalid character` 之前。
	{"invalid character %#U in identifier", hasSuffix(" in identifier"), "LEX-BAD-IDENT-CHAR"},
	{"invalid character %#U", hasPrefix("invalid character"), "LEX-BAD-CHAR"},
}

// 全部**词法** DiagCode（与 `rgoc-lex/src/diag.rs` 里 `LEX-` 前缀的变体**一一对应**，共 23 个）。
//
// 顺序即 `diag.rs` 里的声明顺序 == `as_str()` 的字典序（那条不变量由 Rust 侧断言钉住；
// 这里抄一遍是为了让 Go 侧也能自查"有没有多余的/漏掉的代号"）。
//
// ⚠️ `diag.rs` 里还有 M2 新增的 `SYN-` 前缀**语法**代号（由 parser 产出），**不在本表里**：
// 下面的自检 ② 要求每个代号都能被官方 `scanner.go` / `source.go` 消息产出，而 `SYN-`
// 代号在这张词法表里永远不可能被产出 —— 塞进来只会多一串要豁免的死条目。它们由
// `tools/ast-oracle` 的镜像表覆盖（T-AP16）。Rust 侧的对撞断言已相应限定为 `LEX-` 子表。
var allCodes = []string{
	"LEX-BAD-CHAR",
	"LEX-BAD-DIGIT",
	"LEX-BAD-EXPONENT",
	"LEX-BAD-IDENT-CHAR",
	"LEX-BAD-RADIX-POINT",
	"LEX-BAD-SEP",
	"LEX-BOM-MIDFILE",
	"LEX-COMMENT-UNTERMINATED",
	"LEX-ESCAPE-BAD-DIGIT",
	"LEX-ESCAPE-OCTAL-OVER",
	"LEX-ESCAPE-RANGE",
	"LEX-ESCAPE-UNKNOWN",
	"LEX-EXCEEDS-FILE-LIMIT",
	"LEX-ID-LEADING-DIGIT",
	"LEX-INVALID-NUL",
	"LEX-INVALID-UTF8",
	"LEX-LIT-NO-DIGITS",
	"LEX-RUNE-EMPTY",
	"LEX-RUNE-MULTI",
	"LEX-RUNE-NEWLINE",
	"LEX-RUNE-UNTERMINATED",
	"LEX-STRING-NEWLINE",
	"LEX-STRING-UNTERMINATED",
}

// noOfficialSource 是本仓自己的限制，官方没有对应行为 —— 由它豁免自检 ②。
const noOfficialSource = "LEX-EXCEEDS-FILE-LIMIT"

// wantMappings 是映射条目数的冻结值（scanner.go 23 处 + source.go 3 处错误调用点，
// 其中两处 `string not terminated` 共用模板 ⇒ 25 条）。多一条少一条都会让自检失败，
// 逼着改的人想清楚"这条是新增的官方消息，还是抄错了"。
const wantMappings = 25

func exact(s string) func(string) bool { return func(m string) bool { return m == s } }
func hasPrefix(p string) func(string) bool {
	return func(m string) bool { return strings.HasPrefix(m, p) }
}
func hasSuffix(p string) func(string) bool {
	return func(m string) bool { return strings.HasSuffix(m, p) }
}

// diagCode 把官方消息文本映射成代号。第二个返回值为 false 表示**未映射** ——
// 调用方必须立刻失败，不许猜。
func diagCode(msg string) (string, bool) {
	for _, m := range diagMappings {
		if m.match(msg) {
			return m.code, true
		}
	}
	return "", false
}

func printCodeTable() {
	fmt.Printf("共 %d 条映射（冻结值 %d），码表 %d 个代号：\n\n",
		len(diagMappings), wantMappings, len(allCodes))
	for _, m := range diagMappings {
		fmt.Printf("  %-24s  %s\n", m.code, m.template)
	}
	fmt.Printf("\n  %-24s  %s\n", noOfficialSource, "（官方无对应 —— 本仓实现限制）")
}

func runSelfCheck() int {
	fail := 0
	report := func(ok bool, format string, args ...any) {
		mark := "✓"
		if !ok {
			mark = "✗"
			fail++
		}
		fmt.Printf("  %s %s\n", mark, fmt.Sprintf(format, args...))
	}

	fmt.Println("DiagCode 映射表自检：")

	// ① 映射用到的代号必须都在码表里
	inCodes := map[string]bool{}
	for _, c := range allCodes {
		inCodes[c] = true
	}
	for _, m := range diagMappings {
		report(inCodes[m.code], "映射代号 %s 在码表里", m.code)
	}

	// ② 码表里每个代号都要有官方消息能产出它（除本仓限制）
	reachable := map[string]bool{}
	for _, m := range diagMappings {
		reachable[m.code] = true
	}
	for _, c := range allCodes {
		if c == noOfficialSource {
			continue
		}
		report(reachable[c], "%s 至少有一条官方消息能产出它", c)
	}

	// ③ 条目数冻结
	report(len(diagMappings) == wantMappings,
		"映射条目数 == %d（实际 %d）", wantMappings, len(diagMappings))

	// ④ 码表无重复
	seen := map[string]bool{}
	dup := false
	for _, c := range allCodes {
		if seen[c] {
			dup = true
		}
		seen[c] = true
	}
	report(!dup, "码表无重复项")
	report(len(allCodes) == 23, "码表共 23 个代号（实际 %d）", len(allCodes))

	if fail > 0 {
		fmt.Printf("\n✗ 自检未通过（%d 项）\n", fail)
		return 1
	}
	fmt.Println("\n✓ 自检通过")
	return 0
}
