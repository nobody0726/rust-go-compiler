// gen_unicode 从 **oracle 的 `unicode` 包**导出字符分类区间表，生成 Rust 源文件。
//
// 为什么要有这个生成器（而不是在 Rust 里用 std 的近似谓词）：
//
//	官方 scanner 的分类是 unicode.IsLetter / unicode.IsDigit（L* / Nd），
//	而 Rust std 的 char::is_alphabetic / is_numeric **都更宽**：
//	  · is_alphabetic = Alphabetic 属性 = L* + Nl + Other_Alphabetic ⇒ 多 1,749 个 Mn
//	  · is_numeric    = N*（Nd + Nl + No）                          ⇒ 多 1,154 个 No
//	差异是**静默的**：`x²` 在 Go 里是 `x` + 非法字符，用 is_numeric 会整个切成一个标识符，
//	而且"切出来一堆词元、没报错"，看起来完全正常。
//
// 所以表**由 oracle 生成**（D-M1-6），不引第三方依赖、也不用 std 的近似谓词。
//
// 用法：
//
//	go run ./gen_unicode <输出路径>
//
// 例（在容器内，仓库根为 /work）：
//
//	RGOC_TTY=0 scripts/in-container.sh bash -lc \
//	  'cd /work/tools/token-oracle && go run ./gen_unicode /work/rgoc/crates/rgoc-lex/src/unicode_tables.rs'
package main

import (
	"bytes"
	"fmt"
	"os"
	"strings"
	"unicode"
)

// 代理区：不是标量值，`char` 造不出来。
// 忘了跳过它们会在表里留下**永远查不到的区间**（死数据）—— 测试里有断言盯着。
const (
	surrogateLo = 0xD800
	surrogateHi = 0xDFFF
	maxRune     = 0x10FFFF
)

// Go 的**原始字符串字面量里不能出现反引号**，而我们要生成的正是 Markdown 反引号。
// 所以模板里用 ¶ 占位，输出前替换掉。
const tick = "\x60"

// fill 把模板里的 ¶ 换成反引号。
func fill(tpl string) string { return strings.ReplaceAll(tpl, "¶", tick) }

func main() {
	if len(os.Args) != 2 {
		fmt.Fprintln(os.Stderr, "用法：go run ./gen_unicode <输出路径>")
		os.Exit(2)
	}
	out := os.Args[1]

	letters := ranges(unicode.IsLetter)
	digits := ranges(unicode.IsDigit)

	var b bytes.Buffer
	writeHeader(&b, letters, digits)
	writeTable(&b, "LETTER_RANGES", "字母：unicode.IsLetter（Unicode 类别 L*）", letters)
	writeTable(&b, "DIGIT_RANGES", "数字：unicode.IsDigit（Unicode 类别 Nd）", digits)
	b.WriteString(fill(accessors))

	if err := os.WriteFile(out, b.Bytes(), 0o644); err != nil {
		fmt.Fprintln(os.Stderr, "写入失败：", err)
		os.Exit(2)
	}
	fmt.Printf("已生成 %s\n  LETTER_RANGES = %d 区间 / %d 码点\n  DIGIT_RANGES  = %d 区间 / %d 码点\n  Unicode 版本  = %s\n",
		out, len(letters), count(letters), len(digits), count(digits), unicode.Version)
}

// ranges 归并连续码点为闭区间 [(lo, hi), ...]，**升序且互不重叠**。
// 升序是这个表的硬要求：Rust 侧用二分查找查它。
func ranges(pred func(rune) bool) [][2]uint32 {
	var out [][2]uint32
	inRun := false
	var start uint32

	for r := rune(0); r <= maxRune; r++ {
		if r >= surrogateLo && r <= surrogateHi {
			if inRun {
				out = append(out, [2]uint32{start, uint32(r) - 1})
				inRun = false
			}
			continue
		}
		if pred(r) {
			if !inRun {
				start, inRun = uint32(r), true
			}
		} else if inRun {
			out = append(out, [2]uint32{start, uint32(r) - 1})
			inRun = false
		}
	}
	if inRun {
		out = append(out, [2]uint32{start, maxRune})
	}
	return out
}

func count(rs [][2]uint32) uint64 {
	var n uint64
	for _, r := range rs {
		n += uint64(r[1]-r[0]) + 1
	}
	return n
}

const headerTpl = `//! 字符分类表 —— **自动生成，请勿手改**（D-M1-6）。
//!
//! # 来源
//!
//! 由 go1.27.1 的 unicode 包导出。生成时的 unicode.Version = **%s**。
//!
//! # 生成方式
//!
//! ¶¶¶text
//! RGOC_TTY=0 scripts/in-container.sh bash -lc \
//!   'cd /work/tools/token-oracle && go run ./gen_unicode /work/rgoc/crates/rgoc-lex/src/unicode_tables.rs'
//! ¶¶¶
//!
//! 生成器：tools/token-oracle/gen_unicode/main.go（约 100 行，可读）。
//! **要改这个文件就得改生成器** —— 手改会在下次再生成时被覆盖，
//! 而且会破坏「表 == oracle 的区间，一条不多一条不少」这条可校验性。
//!
//! # 为什么不用 Rust std 的近似谓词
//!
//! 官方 scanner 用的是 unicode.IsLetter / unicode.IsDigit，而 Rust std：
//!
//! | 谓词 | Go（oracle） | Rust std | 差异 |
//! |---|---|---|---|
//! | 字母 IsLetter(L*) | %d 码点 | is_alphabetic 147,421 | **多 1,749**（全是 Mn 组合符） |
//! | 数字 IsDigit(Nd) | %d 码点 | is_numeric 1,924 | **多 1,154**（全是 No） |
//!
//! 差异是**静默的**：x² 在 Go 里是 x + 非法字符；
//! 若用 is_numeric，² 会被当成标识符续接字符 ⇒ **整个切成一个标识符**，
//! 而且「切出来一堆词元、没报错」，看起来完全正常。
//!
//! # 表的内容边界
//!
//! 两张表**严格等于** unicode.IsLetter / unicode.IsDigit 的区间：
//!
//! - **不含代理区**（U+D800..U+DFFF）—— 它不是标量值，char 造不出来
//! - **不含下划线 U+005F** —— 见 is_letter 的说明
//!
//! 区间按码点**升序、互不重叠** —— 这是 lookup 用二分查找的前提。
`

func writeHeader(b *bytes.Buffer, letters, digits [][2]uint32) {
	fmt.Fprintf(b, fill(headerTpl), unicode.Version, count(letters), count(digits))
}

func writeTable(b *bytes.Buffer, name, doc string, rs [][2]uint32) {
	fmt.Fprintf(b, "\n/// %s。\n///\n/// 共 **%d** 个区间 / **%d** 个码点（升序、互不重叠）。\n",
		doc, len(rs), count(rs))
	fmt.Fprintf(b, "pub const %s: &[(u32, u32)] = &[\n", name)
	for _, r := range rs {
		fmt.Fprintf(b, "    (0x%04X, 0x%04X),\n", r[0], r[1])
	}
	b.WriteString("];\n")
}

const accessors = `
/// 是否算「字母」—— 对应官方 scanner.atIdentChar 的第一个 case。
///
/// # 下划线为什么在这里而不是在表里
///
/// Go 规范的 letter = unicode_letter | "_" 把下划线**算作字母**；
/// 官方 isLetter() 也就是 ASCII 字母 || '_'。但 Unicode **没有**把它归入 L* ——
/// 所以它不在生成表里，而是由本函数补上。
///
/// 这样只要往 LETTER_RANGES 里多塞一个 (0x005F, 0x005F)，测试里的两条断言立刻会红
/// —— 表能逐字对着 oracle 校验：「== unicode.IsLetter 的区间，一条不多一条不少」。
#[inline]
pub fn is_letter(c: char) -> bool {
    c == '_' || lookup(LETTER_RANGES, c)
}

/// 是否是「十进制数字」（Unicode 类别 Nd）—— 对应 atIdentChar 的第二个 case。
///
/// 注意**不是** N*：² ¼ 这类上标/分数（No）不算，它们是非法标识符字符。
#[inline]
pub fn is_digit(c: char) -> bool {
    lookup(DIGIT_RANGES, c)
}

/// 在升序、互不重叠的区间表里二分查找。
///
/// 比较器的方向容易写反：binary_search_by 的闭包接收的是**表里的元素**，
/// 要返回的是「该元素相对**目标**的大小」（元素区间整体落在目标之下 ⇒ Less）。
fn lookup(table: &[(u32, u32)], c: char) -> bool {
    let cp = c as u32;
    table
        .binary_search_by(|&(lo, hi)| {
            if hi < cp {
                core::cmp::Ordering::Less
            } else if lo > cp {
                core::cmp::Ordering::Greater
            } else {
                core::cmp::Ordering::Equal
            }
        })
        .is_ok()
}
`
