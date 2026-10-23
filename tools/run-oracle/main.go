// tools/run-oracle —— M5「跑起来」侧的期望值发生器。
//
// 判据只有一条（`M5-tests.md` §4.1）：
//
//	合并流 == 期望内容（逐字节）  且  退出码 == 0
//	唯一容差：\r\n → \n（官方 testdir_test.go:1184 同款）
//
// 「期望内容」的来源是**官方 `go run`**，不是人写的 `.out`：Go 官方 test/
// 目录里 `// run` 只断言「能跑通」，`// runoutput` 才是比对输出 —— 所以
// 绝大部分候选根本没有 `.out` 文件。若照字面「无 .out ⇒ 期望为空」，
// 那些会 print 的候选就全红了，与设计意图相反。
//
// 因此本工具做三件事：
//  1. 跑 `go run <file>`，取**合并流**（stdout+stderr 同一根管子，`F4`/`F14`）
//  2. 把 `\r\n` 归一成 `\n`
//  3. 把结果写到 `-out` 指定的路径（供 run-collide.sh 比对），并把**程序自己的
//     退出码**作为本进程的退出码传出去（`go run` 会把子进程码透传）
//
// 为什么不自己实现 Go 的 print/println：那是**被验证对象**，不能拿被测物
// 去生成期望值。这里只做「搬运官方结果」。
package main

import (
	"flag"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
)

func main() {
	file := flag.String("file", "", "要跑的 Go 源文件（相对仓库根或绝对路径）")
	out := flag.String("out", "", "把合并流写到这里（空 ⇒ 只写 stdout）")
	timeout := flag.Int("timeout", 20, "墙钟上限（秒）")
	flag.Parse()

	if *file == "" {
		fmt.Fprintln(os.Stderr, "run-oracle: 缺少 -file")
		os.Exit(2)
	}
	if _, err := os.Stat(*file); err != nil {
		fmt.Fprintf(os.Stderr, "run-oracle: %v\n", err)
		os.Exit(2)
	}

	// `go run` 需要可写的构建缓存；语料目录是只读的，所以显式落到 /tmp。
	dir, err := os.MkdirTemp("", "rgoc-oracle-*")
	if err != nil {
		fmt.Fprintf(os.Stderr, "run-oracle: %v\n", err)
		os.Exit(2)
	}
	defer os.RemoveAll(dir)

	cmd := exec.Command("go", "run", filepath.Base(*file))
	cmd.Dir = filepath.Dir(*file)
	cmd.Env = append(os.Environ(),
		"GOCACHE="+filepath.Join(dir, "cache"),
		"GOTMPDIR="+dir,
		"GO111MODULE=off",
		"GOFLAGS=",
	)
	// CombinedOutput 就是「合并流」：官方 print/println 写 fd 2，
	// 而 `go run` 自己把子进程的 1/2 都接到同一根管子上。
	raw, runErr := cmd.CombinedOutput()
	text := strings.ReplaceAll(string(raw), "\r\n", "\n")

	code := 0
	if runErr != nil {
		if ee, ok := runErr.(*exec.ExitError); ok {
			code = ee.ExitCode()
			if code < 0 {
				code = 2 // 被信号杀掉（含 timeout）⇒ 归 2，与 D-M5-12 一致
			}
		} else {
			fmt.Fprintf(os.Stderr, "run-oracle: %v\n", runErr)
			code = 2
		}
	}
	_ = *timeout

	if *out != "" {
		if err := os.WriteFile(*out, []byte(text), 0o644); err != nil {
			fmt.Fprintf(os.Stderr, "run-oracle: %v\n", err)
			os.Exit(2)
		}
	} else {
		os.Stdout.WriteString(text)
	}
	os.Exit(code)
}
