// native fixture：最小可运行程序（`M7-tests.md` §3.4 的 `T-NAT-42` 成员）。
//
// 期望：stdout 精确 `hello, world\n`、stderr 空、退出码 0。
// 语料里对应的是 `test/helloworld.go`（13 字节），但**不共用那个文件** ——
// 语料只读，fixture 要能随手改。

package main

func main() {
	print("hello, world\n")
}
