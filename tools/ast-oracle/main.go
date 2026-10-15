// ast oracle 命令行入口。
//
// 用法：go run . <file.go>
//
// 读入一个 Go 源文件，用 vendor 的官方 parser（CheckBranches 开启，
// 与 M2 parser 的行为口径一致）解析，把中立 dump（dump.go）写到 stdout；
// 语法诊断按发生顺序写到 stderr（`file:line:col: msg`，官方 Pos.String()）。
//
// 退出码：0 = 无语法错；1 = 有语法错（dump 照常输出 —— 官方「报错但继续，
// 把能恢复的部分也交出去」的口径，rgoc 侧同）。
//
// 注意（M1 实测的坑，README §「实测的坑」同款）：
//   - 必须 cd 进本目录再 go run .（自带 go.mod，仓库根没有）；
//   - 捕获输出必须 RGOC_TTY=0，否则伪终端把 \n 翻成 \r\n；
//   - 本目录下不要放样本 .go（go run . 会把同目录 .go 一起编译）。
package main

import (
	"fmt"
	"os"

	"astoracle/gosyntax" // package name 是官方的 syntax，导入路径才是 astoracle/gosyntax
)

func main() {
	if len(os.Args) != 2 {
		fmt.Fprintf(os.Stderr, "usage: go run . <file.go>\n")
		os.Exit(2)
	}
	filename := os.Args[1]

	// 诊断收集：官方 errh 按**发生顺序**回调；EOF 抑制、
	// `syntax error: ` 前缀等语义全在 vendor 的 parser 里，这里不加工。
	var nerr int
	errh := func(err error) {
		nerr++
		fmt.Fprintln(os.Stderr, err)
	}

	// ParseFile = NewFileBase(filename) + Parse(..., CheckBranches)。
	// pragh 传 nil：官方对 nil 的处理是忽略 //go: 指令的效果（收集仍在
	// parser 内部完成），与 rgoc parse_file 的口径一致。
	f, _ := syntax.ParseFile(filename, errh, nil, syntax.CheckBranches)
	if f == nil {
		// 连 File 都没有：首错即炸（F4 panic 路径，如 io 错误）。
		os.Exit(2)
	}
	os.Stdout.WriteString(dump(f))
	if nerr > 0 {
		os.Exit(1)
	}
}
