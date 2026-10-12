# GAP-01：功能 ID → 四维归属（M1 进度）

> **范围**：`docs/01-feature-set.md` 共 210 个功能 ID。M1 只判定 **LEX-01..13** 的归属，不填满 210 个（`M0-report.md` §7.2 明确警告）。
> **维度定义**：`docs/03-roadmap.md` §0.3 —— Frontend / Interpreter / Native / Runtime-metadata。
> **来源**：`docs/milestones/M1-tests.md` §2 的逐项覆盖映射。

## LEX-01..13 四维归属

| 功能 ID | 描述 | Frontend | Interpreter | Native | Runtime-metadata | 测试 ID |
|---|---|---|---|---|---|---|
| LEX-01 | UTF-8 源码表示（BOM / NUL / 非法 UTF-8） | ✅ | — | — | — | T-L-13, T-L-14, T-L-15, T-L-16 |
| LEX-02 | Unicode 字母/数字分类 | ✅ | — | — | — | T-L-11, T-L-12, T-L-19, T-L-24, T-L-25 |
| LEX-03 | 行/块注释 | ✅ | — | — | — | T-L-45, T-L-46, T-L-52, T-L-70 |
| LEX-04 | Token 四分类与最长匹配 | ✅ | — | — | — | T-L-20..T-L-29 |
| LEX-05 | 25 个保留关键字 | ✅ | — | — | — | T-L-22, T-L-30 |
| LEX-06 | 48 条操作符与标点 | ✅ | — | — | — | T-L-28, T-L-31 |
| LEX-07 | 分号自动插入 | ✅ | — | — | — | T-L-40..T-L-48 |
| LEX-08 | 标识符词法 | ✅ | — | — | — | T-L-20, T-L-21, T-L-23, T-L-24 |
| LEX-09 | 整数字面量 | ✅ | — | — | — | T-L-33, T-L-38 |
| LEX-10 | 浮点字面量 | ✅ | — | — | — | T-L-34, T-L-38 |
| LEX-11 | 虚数字面量 | ✅ | — | — | — | T-L-35 |
| LEX-12 | Rune 字面量与转义 | ✅ | — | — | — | T-L-36, T-L-53 |
| LEX-13 | 字符串字面量 | ✅ | — | — | — | T-L-37, T-L-53 |

## 判定理由

- **LEX-01..13 全部归属 Frontend**：它们只决定「字节流如何被切成带位置的词元流」，不产生运行时语义、不生成机器码、不维护 runtime 元数据。
- 字面量的**值**（如 `0x1.8p-2` 的数值）属于 M3 常量体系，不在 M1 判定；因此 LEX-09/10/11 不进入 Interpreter/Native。
- `print`/`println` 内建的行为（stdout/stderr）属于 M5/M7，与 LEX-13 字符串的扫描边界无关。

## 未判定部分

其余 197 个功能 ID（CON/TYP/SCP/EXP/STM/BIF/PKG/SYS/AST/PIPE）待 M2 起按阶段推进，不在本文件填写。
