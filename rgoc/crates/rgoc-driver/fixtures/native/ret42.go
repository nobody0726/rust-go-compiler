// native fixture：算术 + 退出码（`T-NAT-16` 的最早端到端证据）。
//
// 期望：stdout `42\n`、stderr 空、退出码 0。
// 这里**故意用乘法而不是常量**：常量会被 `fold` 折掉，走不到 `Bin` 的指令选择。

package main

func main() {
	x := 6
	y := 7
	println(x * y)
}
