// native fixture：什么都不做（`T-NAT-43` 的「空输出」形态）。
//
// 期望：stdout 空、stderr 空、退出码 0。
// 这条 fixture 看着没用，其实钉的是「`main` 正常返回 ⇒ `exit_group(0)`」——
// 语料里 42/46 就是这种形态，如果启动序列写错（例如忘了 exit），
// 这里会以「退出码是 main 的返回值」或「进程没退出」的形式暴露。

package main

func main() {
}
