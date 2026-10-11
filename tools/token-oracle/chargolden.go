// 字符级 golden 的生成 —— 读语料（hex 行），对每个用例逐事件渲染。
//
// 输出格式刻意做成**行式文本**：Rust 侧只要按同样的规则拼出同样的文本再逐行比，
// 不需要任何解析器。格式里的每一处不对称都对应一个真实陷阱（见 renderCharEvents）。
package main

import (
	"bufio"
	"fmt"
	"os"
	"strings"

	gosyntax "tokenoracle/gosyntax"
)

// charGolden 读语料文件，把每个用例的事件流渲染出来写到 stdout。
//
// 任何**未映射**的官方诊断消息都会让整个过程立刻失败（exit 3）——
// 与词元侧同一条纪律：契约不完整时不许冻结 golden。
func charGolden(corpusPath string) int {
	f, err := os.Open(corpusPath)
	if err != nil {
		fmt.Fprintln(os.Stderr, "读取语料失败：", err)
		return 2
	}
	defer f.Close() //nolint:errcheck // 只读句柄

	out := &strings.Builder{}
	fmt.Fprintln(out, "# char-golden: 字符层逐事件（oracle = go1.27.1 的 gc/syntax `source.nextch`）")
	fmt.Fprintln(out, "# 顺序即发生顺序：D = 诊断，C = 读到一个字符。EOF 不进事件流。")
	fmt.Fprintln(out, "# 位置 1 起；列按**字节**。该文件由 `go run . --chars-golden` 生成 —— 勿手改。")

	sc := bufio.NewScanner(f)
	sc.Buffer(make([]byte, 0, 64*1024), 1024*1024)

	cases := 0
	events := 0
	for sc.Scan() {
		line := strings.TrimSpace(sc.Text())
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		bs, ok := bytesOfHex(line)
		if !ok {
			fmt.Fprintf(os.Stderr, "❌ 语料里有非法 hex 行：%q\n", line)
			return 2
		}

		fmt.Fprintf(out, "## %s\n", line)
		evs := gosyntax.DumpChars(string(bs))
		rendered, code := renderCharEvents(evs)
		if code != 0 {
			os.Stdout.WriteString(out.String())
			return code
		}
		out.WriteString(rendered)

		cases++
		events += len(evs)
	}
	if err := sc.Err(); err != nil {
		fmt.Fprintln(os.Stderr, "读语料出错：", err)
		return 2
	}

	// 空语料会让 golden 变成"永远通过"的东西 —— 那是本仓反复吃亏的形状，直接拒绝。
	if cases == 0 {
		fmt.Fprintln(os.Stderr, "❌ 语料为 0 个用例 —— 拒绝生成一份恒真的 golden")
		return 2
	}

	os.Stdout.WriteString(out.String())
	fmt.Fprintf(os.Stderr, "  %d 个用例 / %d 条事件\n", cases, events)
	return 0
}

// renderCharEvents 把一个用例的事件流渲染成文本行。
//
// 三处刻意的不对称，每一处都对应一个真实的静默失败：
//
//  1. **诊断只写代号，不写消息文本** —— 与词元侧同一条纪律（`03` §3.4：
//     文本另列指标）。消息文本改了不该让 golden 红。
//  2. **字符写 `U+XXXX w=n` 而不是直接写字符** —— `w` 是 byte-column 语义的
//     直接证据（一个 3 字节的 CJK 字符会让下一列 +3）。顺带说清一件事：
//     **合成**的 `U+FFFD`（非法编码的替身）**从不进入事件流** —— 官方在
//     `ch == RuneError && chw == 1` 时 `goto redo` 重读，它只作中间值存在。
//     事件流里的 `U+FFFD` 一定是源码里真实写着的那个（`w=3`）。
//  3. **EOF 不进事件流** —— EOF 的 `chw = 0`，"位置不再前进"这件事
//     由**没有更多事件**来表达；硬塞一个 EOF 事件反而要在两侧各写一遍
//     "EOF 的位置是什么"，那是多余的约定。
func renderCharEvents(evs []gosyntax.CharEvent) (string, int) {
	var sb strings.Builder
	for _, e := range evs {
		posStr := fmt.Sprintf("%d:%d", e.Line, e.Col)
		if e.IsDiag {
			code, ok := diagCode(e.Msg)
			if !ok {
				fmt.Fprintf(os.Stderr, "❌ 未映射的官方诊断 @ %s：%q\n", posStr, e.Msg)
				fmt.Fprintln(os.Stderr, "   ⇒ 契约缺这一条（README §6 / M1-design.md §7.2）。")
				return "", 3
			}
			fmt.Fprintf(&sb, "D %-9s%s\n", posStr, code)
			continue
		}
		// 宽度是 byte-column 语义的直接证据，必须进 golden。
		fmt.Fprintf(&sb, "C %-9sU+%04X w=%d\n", posStr, e.Ch, e.Chw)
	}
	return sb.String(), 0
}

// bytesOfHex 解析语料行：小写 hex，`-` 表示空串。
func bytesOfHex(s string) ([]byte, bool) {
	if s == "-" {
		return nil, true
	}
	if len(s)%2 != 0 {
		return nil, false
	}
	out := make([]byte, 0, len(s)/2)
	for i := 0; i < len(s); i += 2 {
		hi, ok1 := hexDigit(s[i])
		lo, ok2 := hexDigit(s[i+1])
		if !ok1 || !ok2 {
			return nil, false
		}
		out = append(out, hi<<4|lo)
	}
	return out, true
}

func hexDigit(c byte) (byte, bool) {
	switch {
	case c >= '0' && c <= '9':
		return c - '0', true
	case c >= 'a' && c <= 'f':
		return c - 'a' + 10, true
	default:
		return 0, false
	}
}
