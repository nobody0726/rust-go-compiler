// token oracle 的命令行入口 —— 本目录里**唯一**的 `package main`。
//
// 职责：把 `gosyntax.Scan` 得到的词元/诊断/注释三条流，按 README §5 的冻结格式
// 渲染成**逐字节可比较**的文本。
//
// 三条纪律（README §5.5，全部是实测踩出来的）：
//  1. 只录有效域内的字段 —— 无效域写 "-"，绝不录残留值
//  2. 诊断与词元分两条流、各自排序
//  3. 诊断只存位置 + 代号，不存消息文本
//
// 另加一条本仓的通则：**遇到映射表之外的消息立刻失败**，不猜、不跳过。
package main

import (
	"flag"
	"fmt"
	"os"
	"sort"
	"strings"

	// 目录叫 gosyntax，但包声明名是官方原来的 `syntax`（官方文件不得改动）。
	// 这里按**目录名**别名导入，让调用点与 README 的措辞一致，也避免与标准库
	// 里可能出现的同名标识混淆。
	gosyntax "tokenoracle/gosyntax"
)

func main() {
	mode := flag.String("mode", "", "注释上报模式：留空=不上报 | comments | directives")
	listCodes := flag.Bool("list-codes", false, "打印 DiagCode ↔ 官方消息模板对照表后退出")
	selfCheck := flag.Bool("selfcheck", false, "自检映射表与码表是否一致后退出")
	genCorpus := flag.Bool("gen-char-corpus", false, "生成**字符级**对撞语料到 stdout 后退出")
	charsGolden := flag.Bool("chars-golden", false, "读语料，输出**字符级** golden 到 stdout 后退出")
	corpus := flag.String("corpus", "fixtures/char_corpus.txt", "字符级语料路径（--chars-golden 用）")
	flag.Parse()

	if *selfCheck {
		os.Exit(runSelfCheck())
	}
	if *listCodes {
		printCodeTable()
		return
	}
	if *genCorpus {
		printCharCorpus()
		return
	}
	if *charsGolden {
		os.Exit(charGolden(*corpus))
	}

	args := flag.Args()
	if len(args) != 1 {
		fmt.Fprintln(os.Stderr, "用法：go run . [--mode comments|directives] <source-file>")
		fmt.Fprintln(os.Stderr, "      go run . --selfcheck | --list-codes | --gen-char-corpus | --chars-golden")
		os.Exit(2)
	}
	path := args[0]

	var scanMode uint
	switch *mode {
	case "":
		scanMode = 0
	case "comments":
		scanMode = gosyntax.ModeComments
	case "directives":
		scanMode = gosyntax.ModeDirectives
	default:
		fmt.Fprintf(os.Stderr, "未知 --mode %q（可选 comments / directives）\n", *mode)
		os.Exit(2)
	}

	src, err := os.ReadFile(path)
	if err != nil {
		fmt.Fprintln(os.Stderr, "读取失败：", err)
		os.Exit(2)
	}

	var diags []diagRec
	var messages []msgRec

	tokens := gosyntax.Scan(string(src), scanMode, func(line, col uint, text string) {
		// 官方约定：注释/指令的文本以 '/' 开头（parser.go 用的是同一个判据）。
		if strings.HasPrefix(text, "/") {
			messages = append(messages, msgRec{line, col, text})
			return
		}
		code, ok := diagCode(text)
		if !ok {
			fmt.Fprintf(os.Stderr, "❌ 未映射的官方诊断 @ %d:%d：%q\n", line, col, text)
			fmt.Fprintln(os.Stderr, "   ⇒ 契约缺这一条（README §6 / M1-design.md §7.2）。")
			fmt.Fprintln(os.Stderr, "   修法：在 diagMappings 里加映射，并同步 rgoc-lex 的 DiagCode。")
			fmt.Fprintln(os.Stderr, "   不许猜一个最像的代号 —— 那等于在契约不完整时冻结 golden。")
			os.Exit(3)
		}
		diags = append(diags, diagRec{line, col, code})
	})

	// 纪律 2：两条流各自排序（C1 §4 的 line → col → code）。
	// 词元流也显式排一次 —— 扫描器本来就是顺序产出的，但这让"各自有序"
	// 成为**格式的性质**，而不是"依赖扫描器内部不会乱序"的隐含假设。
	// 相等位置用稳定排序保序：EOF 处的分号与 EOF 同位置，顺序不能颠倒。
	sort.SliceStable(tokens, func(i, j int) bool {
		if tokens[i].Line != tokens[j].Line {
			return tokens[i].Line < tokens[j].Line
		}
		return tokens[i].Col < tokens[j].Col
	})
	sort.SliceStable(diags, func(i, j int) bool {
		if diags[i].line != diags[j].line {
			return diags[i].line < diags[j].line
		}
		if diags[i].col != diags[j].col {
			return diags[i].col < diags[j].col
		}
		return diags[i].code < diags[j].code
	})
	sort.SliceStable(messages, func(i, j int) bool {
		if messages[i].line != messages[j].line {
			return messages[i].line < messages[j].line
		}
		if messages[i].col != messages[j].col {
			return messages[i].col < messages[j].col
		}
		return messages[i].text < messages[j].text
	})

	out := &strings.Builder{}
	fmt.Fprintf(out, "# golden: %s\n", path)
	fmt.Fprintln(out, "# source: oracle")

	fmt.Fprintln(out, "== tokens ==")
	for _, t := range tokens {
		fmt.Fprintf(out, "  %-7s%-9slit=%s%s\n",
			pos(t.Line, t.Col), t.Name, litField(t), extraFields(t))
	}

	fmt.Fprintln(out, "== diags ==")
	for _, d := range diags {
		fmt.Fprintf(out, "  %-7s%s\n", pos(d.line, d.col), d.code)
	}

	// 只有开启 --mode 时才有这一节；默认输出与冻结的 golden 格式**逐字节相同**。
	if scanMode != 0 {
		fmt.Fprintln(out, "== messages ==")
		for _, m := range messages {
			fmt.Fprintf(out, "  %-7s%s\n", pos(m.line, m.col), quote([]byte(m.text)))
		}
	}

	os.Stdout.WriteString(out.String())
}

type diagRec struct {
	line, col uint
	code      string
}

type msgRec struct {
	line, col uint
	text      string
}

func pos(line, col uint) string { return fmt.Sprintf("%d:%d", line, col) }

// litField 实现纪律 1：**只录有效域内的字段**，否则写 "-"。
//
// 反面教材（实测）：`a >>= 1` 里 `>>=` 那个词元的 lit 是上一个词元残留的 "a"。
// 录进去就是把实现细节当契约 —— 换个实现残留值就不同，比对会莫名其妙地红。
func litField(t gosyntax.Tok) string {
	if !t.LitValid {
		return "-"
	}
	return quote([]byte(t.Lit))
}

// extraFields 只输出**当前类别下有效**的附加字段。
func extraFields(t gosyntax.Tok) string {
	if t.IsLiteral {
		return fmt.Sprintf(" kind=%d bad=%t", t.LitKind, t.Bad)
	}
	if t.HasOp {
		return fmt.Sprintf(" op=%s prec=%d", t.Op, t.Prec)
	}
	return ""
}

// quote 包上双引号并转义。
//
// **刻意不用 Go 的 %q**：`%q` 对 >=0x80 的**可打印**字符按原样输出，
// 而 Rust 侧的 `{:?}` 对控制字符的写法与 Go 不同 —— 两侧一旦不一致，
// golden 就成了"只有 Go 能通过"的东西。改为自实现的**纯字节级**转义（README §5.3），
// 规则简单到两侧各自实现也不会跑偏。它同时解决了 Rust `String` **装不了非法 UTF-8**
// 这件事（实测 F9：`1\xff2` 的非法字节留在 lit 里）。
func quote(b []byte) string { return `"` + escape(b) + `"` }

func escape(b []byte) string {
	var sb strings.Builder
	for _, c := range b {
		switch {
		case c == '\\':
			sb.WriteString(`\\`)
		case c == '"':
			sb.WriteString(`\"`)
		case c == '\n':
			sb.WriteString(`\n`)
		case c == '\r':
			sb.WriteString(`\r`)
		case c == '\t':
			sb.WriteString(`\t`)
		case c >= 0x20 && c <= 0x7e:
			sb.WriteByte(c)
		default:
			fmt.Fprintf(&sb, `\x%02x`, c)
		}
	}
	return sb.String()
}
