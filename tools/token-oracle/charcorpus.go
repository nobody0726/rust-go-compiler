// 字符级对撞语料 —— 一批**字节串**，用来把官方 `source.nextch`（字符层）
// 与我们的 `SourceReader::advance` 逐事件对撞。
//
// # 为什么要有这个语料
//
// `M1-design.md` §8 的切面 ①（字符解码与位置）判据是「位置必须与 oracle 逐字节相等」。
// 计划 T-L07 原本只给了 6 条**手写**用例 —— 那不足以支撑这句话：位置是**每个字节**
// 推出来的，手写的几十条里未必有一条恰好落在出错的字节上。
//
// 这里改为**密集扫字节空间**：单字节全扫 + 两个"结构相关字节"的全组合 + 有名字的
// 多字节用例。生成是**确定性**的（无随机、无 map 迭代序），所以语料文件可以入库，
// 两侧（Go 生成 golden、Rust 跑对撞）读**同一份**文件 —— 单一事实来源。
//
// 单源很重要：`AGENTS.md` 记的「两侧各维护一份清单 = 必然漂移」已三次同形（T38/T39/T47）。
package main

import "fmt"

// interestingBytes 是"结构相关字节"：单个字节的行为都能从 UTF-8 的编码规则
// 推出来，真正会出错的是**它们的组合**（前导字节后面跟了什么）。
//
// 分类说明：
//
//	0x00                 NUL（唯一的"报错但仍占一列"的 ASCII）
//	0x09 0x0A 0x0D 0x20  制表 / 换行 / 回车 / 空格 —— 空白类，换行还改行号
//	0x1F 0x7F            控制字符与 DEL —— 合法 UTF-8 但不合法标识符
//	0x22 0x27 0x5C       引号（" '）与反斜杠 —— 字面量边界的开启者
//	0x30 0x41 0x61       数字 / 大写 / 小写字母
//	0x80 0x9F 0xA0 0xBF  裸续接字节（非法作为前导）
//	0xC0 0xC1            过长编码的前导（本可用 2 字节内的码点却编成 2 字节）
//	0xC2 0xDF            合法的 2 字节前导（下界 / 上界）
//	0xE0 0xE4 0xED 0xEF  3 字节前导：下界 / CJK / 代理区 / BOM
//	0xF0 0xF4 0xF5       4 字节前导：下界 / 上界(U+10FFFF) / 越界
//	0xFE 0xFF            永远非法
var interestingBytes = []byte{
	0x00, 0x09, 0x0A, 0x0D, 0x1F, 0x20, 0x22, 0x27, 0x30, 0x41,
	0x5C, 0x61, 0x7F, 0x80, 0x9F, 0xA0, 0xBF, 0xC0, 0xC1, 0xC2,
	0xDF, 0xE0, 0xE4, 0xED, 0xEF, 0xF0, 0xF4, 0xF5, 0xFE, 0xFF,
}

// namedCases 是无法由"两个字节的组合"表达的用例：多字节序列的完整形态。
//
// 每一条都对应一个**具体会静默出错的机制**，注释里写明是哪个。
// 用 Go 字符串字面量（`\xNN` 可以表达任意字节）而不是 `[]byte{…}`：这样整个
// 列表能放在一个 `[]string` 里，`[]byte(s)` 转换只在取用处发生一次。
var namedCases = []string{
	"\xef\xbb\xbf",              // BOM 单独（文件只有 3 字节）⇒ EOF 落在 1:4
	"\xef\xbb\xbfx",             // BOM 在文件首 ⇒ 被吃掉，x @1:4
	"\xef\xbb\xbf\xef\xbb\xbfx", // ★ 双 BOM：第二个**必须**报 BOM-MIDFILE
	"x\xef\xbb\xbfy",            // BOM 在文件中 ⇒ 报错但并入标识符
	"x\r\ny",                    // CRLF：只有 \n 改行号
	"x\ry",                      // 裸 CR：当空白，不改行号
	"\r\n",                      // 文件以 CRLF 开头
	"\xe4\xb8\xad\n",            // 中（3 字节，U+4E2D）
	"\xf0\x9f\x98\x80\n",        // 😀（4 字节，U+1F600）
	"\xef\xbf\xbd",              // ★ 字面 U+FFFD（3 字节）：**不是**非法编码！
	"\xf4\x8f\xbf\xbf",          // U+10FFFF（合法上界）
	"\xf4\x90\x80\x80",          // ★ U+110000（越界 ⇒ 逐字节退，4 条诊断）
	"\xf0\x9f",                  // ★ 截断的 4 字节序列
	"\xe4",                      // ★ 截断的 3 字节序列
	"\xed\xa0\x80",              // ★ 代理区 U+D800（逐字节退 ⇒ 3 条诊断）
	"\xed\x9f\xbf",              // U+D7FF（代理区下界之下，合法）
	"\xc0\x80",                  // ★ 过长编码（2 字节表示 U+0000）
	"\xe0\x80\x80",              // ★ 过长编码（3 字节表示 U+0000）
	"\xe0\xa0\x80",              // U+0800（3 字节的合法下界）
	"\xee\x80\x80",              // U+E000（私有区 —— 合法 UTF-8，但标识符里报错）
	"a\x00b\n",                  // NUL 夹在标识符中间
	"\x00\x00",                  // 两个 NUL（一次推进两条诊断）
	"a\x00\x00b",                // 同上，带上下文
	"\xff\xfe",                  // 两个永远非法的字节
	"abc\n",                     // 纯 ASCII 基线
	"ab\nc",                     // 换行后列归零
	"\t\v\f",                    // 制表 / 垂直制表 / 换页
}

// charCorpus 生成语料：顺序固定（去重保序），所以输出可逐字节复现。
func charCorpus() [][]byte {
	seen := map[string]bool{}
	var out [][]byte
	add := func(bs []byte) {
		k := string(bs)
		if seen[k] {
			return
		}
		seen[k] = true
		out = append(out, bs)
	}

	// ① 全部单字节（0x00..0xFF）—— 一条不落，因为"这个字节是什么"是最基本的判据
	for b := 0; b <= 0xFF; b++ {
		add([]byte{byte(b)})
	}
	// ② 结构相关字节的两两组合 —— 前导字节后面跟错了字节，是字符层最容易错的地方
	for _, a := range interestingBytes {
		for _, b := range interestingBytes {
			add([]byte{a, b})
		}
	}
	// ③ 有名字的多字节用例
	for _, s := range namedCases {
		add([]byte(s))
	}
	return out
}

// printCharCorpus 把语料写成**一行一个 hex** 的文本（空串写成 `-`）。
//
// 用 hex 而不是原始字节：语料里含 NUL / 换行 / 非法 UTF-8，直接写会让文件
// 无法用普通文本工具看，也无法区分"行尾的换行是数据的还是分隔的"。
func printCharCorpus() {
	for _, bs := range charCorpus() {
		fmt.Println(hexOf(bs))
	}
}

// hexOf 转小写 hex；空串用 `-`（`""` 与"空行"在文本里不可区分）。
func hexOf(bs []byte) string {
	if len(bs) == 0 {
		return "-"
	}
	const digits = "0123456789abcdef"
	buf := make([]byte, 0, len(bs)*2)
	for _, b := range bs {
		buf = append(buf, digits[b>>4], digits[b&0xf])
	}
	return string(buf)
}
