// `-I` 根指错 vs 不支持形式的**诊断拆分**夹具（`T-CK-74`）。
//
// 只 import、不使用 —— 这样唯一的变量就是「根指到哪」：
// 根下没有这个目录 ⇒ TYP-UNSUPPORTED-IMPORT-FORM；
// 目录在但里面没有 .go ⇒ TYP-BROKEN-IMPORT。
package p

import "lib"
