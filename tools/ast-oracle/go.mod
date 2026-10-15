// ast oracle —— Go 侧「标准答案的提供者」（M2 阶段，D-M2-1）。
//
// vendor 官方 cmd/compile/internal/syntax（gosyntax/，零改动），
// 解析任意 .go 文件后输出**中立 dump**（dump.go），与 rgoc 的
// rgoc-ast::dump 逐字节可 diff —— 这是 M2 oracle 对撞验收的另一半。
//
// 为什么需要独立 module：同 tools/token-oracle 的理由 —— gosyntax/ 是
// 官方包的原样副本，不属于任何 Go 主 module；自带 go.mod 划出边界，
// 也让「这是工具、不是产品」在文件系统上可见。
//
// 依赖：**仅标准库**（含 go/build/constraint / go/constant，官方源文件自身需要）。
module astoracle

go 1.27
