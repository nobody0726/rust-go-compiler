// types oracle —— Go 侧「标准答案的提供者」（M3 阶段，D-M3-10）。
//
// 用**标准库** go/parser + go/types 检查 fixture，按 C7 契约输出中性类型 dump，
// 与 rgoc 的 rgoc-types/src/dump.rs 逐字节可 diff。
//
// 为什么需要独立 module：同 tools/ast-oracle / tools/token-oracle 的理由 ——
// 「这是工具，不是产品」在文件系统上可见，且它不需要进入 rgoc workspace
// 的构建（零第三方依赖，宿主侧完全不参与）。
//
// 依赖：**仅标准库**（go/parser、go/types、go/constant …）。
module typesoracle

go 1.27
