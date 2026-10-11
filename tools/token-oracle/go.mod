// token oracle —— Go 侧参考实现（M1 阶段）。
//
// 为什么需要独立 module：`gosyntax/` 是**官方五文件的原样副本**（package syntax），
// 它是一个 `internal` 包的搬运版，不属于 rgoc 的 Rust workspace，也不该混进任何
// Go 主 module。给它一个自带 go.mod 的边界，既隔离依赖，也让"这是工具、不是产品"
// 这件事在文件系统上可见。
//
// 依赖：**仅标准库**（strconv / strings / unicode / unicode/utf8 / fmt / os / sort）。
module tokenoracle

go 1.21
