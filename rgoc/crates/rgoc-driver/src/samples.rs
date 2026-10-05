//! M0 的 **20 个官方语料基线样本**（`T-C-01` … `T-C-20`）—— E4 门禁的分母。
//!
//! 这张表是 `M0-tests.md` §4.2–4.4 的**可执行副本**：文档冻结了 ID、路径、分层与
//! 覆盖点，这里把它们变成代码里的一张表，`test_driver.rs` 再拿它**与文档逐条对账**。
//!
//! # 为什么要「表 + 对账」两层
//!
//! 只在文档里写清单，代码里另写一份，迟早漂；只在代码里写清单，那份表就成了
//! 「为什么是这 20 个」的唯一答案，没人能查。本仓一直在用这个双写口径 ——
//! 对照 T33 的「枚举实算分母 == 279」，那也是「文档冻结值 ↔ 代码实算值」的交叉校验。
//!
//! **对账能抓到的典型漂移**：① 路径改了（`go1.x` 换文件名）；② 分层改了
//! （`run` 改成 `errorcheck`）；③ 数量变了（有人「顺手」加一个样本 —— 那等于
//! 悄悄改了门禁分母）。

use std::fmt;
use std::path::{Path, PathBuf};

use rgoc_harness::ir::{Layer, Mode};
use rgoc_harness::runner::CaseSpec;

/// 一条样本的静态描述。
///
/// ⚠️ `path` 是**相对于语料根目录**的名字（`helloworld.go`），不是完整路径 ——
/// 完整路径由 [`LoadedSamples::load`] 按语料根拼出来，这样换语料位置不用改表。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SampleEntry {
    /// 测试 ID（`T-C-01` …）
    pub id: &'static str,
    /// 语料根下的文件名
    pub path: &'static str,
    /// 模式（与文件首行指令一致，由测试交叉校验）
    pub mode: Mode,
}

/// M0 的 20 个样本，**顺序即 ID 顺序**（T37 的 `harness list` 直接照这个顺序打印）。
///
/// 来源：`M0-tests.md` §4.2（run 8 个）/ §4.3（compile 4 个）/ §4.4（errorcheck 8 个）。
pub const CORPUS_M0_SAMPLES: [SampleEntry; 20] = [
    // ── §4.2 run 层（8 个）──
    SampleEntry {
        id: "T-C-01",
        path: "helloworld.go",
        mode: Mode::Run,
    },
    SampleEntry {
        id: "T-C-02",
        path: "closure1.go",
        mode: Mode::Run,
    },
    SampleEntry {
        id: "T-C-03",
        path: "gc1.go",
        mode: Mode::Run,
    },
    SampleEntry {
        id: "T-C-04",
        path: "printbig.go",
        mode: Mode::Run,
    },
    SampleEntry {
        id: "T-C-05",
        path: "closure4.go",
        mode: Mode::Run,
    },
    SampleEntry {
        id: "T-C-06",
        path: "func6.go",
        mode: Mode::Run,
    },
    SampleEntry {
        id: "T-C-07",
        path: "compos.go",
        mode: Mode::Run,
    },
    SampleEntry {
        id: "T-C-08",
        path: "method3.go",
        mode: Mode::Run,
    },
    // ── §4.3 compile 层（4 个）──
    SampleEntry {
        id: "T-C-09",
        path: "eof.go",
        mode: Mode::Compile,
    },
    SampleEntry {
        id: "T-C-10",
        path: "empty.go",
        mode: Mode::Compile,
    },
    SampleEntry {
        id: "T-C-11",
        path: "parentype.go",
        mode: Mode::Compile,
    },
    SampleEntry {
        id: "T-C-12",
        path: "rune.go",
        mode: Mode::Compile,
    },
    // ── §4.4 errorcheck 层（8 个）──
    SampleEntry {
        id: "T-C-13",
        path: "initloop.go",
        mode: Mode::ErrorCheck,
    },
    SampleEntry {
        id: "T-C-14",
        path: "recover5.go",
        mode: Mode::ErrorCheck,
    },
    SampleEntry {
        id: "T-C-15",
        path: "varerr.go",
        mode: Mode::ErrorCheck,
    },
    SampleEntry {
        id: "T-C-16",
        path: "convlit1.go",
        mode: Mode::ErrorCheck,
    },
    SampleEntry {
        id: "T-C-17",
        path: "init.go",
        mode: Mode::ErrorCheck,
    },
    SampleEntry {
        id: "T-C-18",
        path: "switch4.go",
        mode: Mode::ErrorCheck,
    },
    SampleEntry {
        id: "T-C-19",
        path: "typecheck.go",
        mode: Mode::ErrorCheck,
    },
    SampleEntry {
        id: "T-C-20",
        path: "mainsig.go",
        mode: Mode::ErrorCheck,
    },
];

/// 按 ID 找样本下标。**认不出一律 `None`**，不猜、不做「大小写不敏感」之类的宽容处理。
pub fn index_of(id: &str) -> Option<usize> {
    CORPUS_M0_SAMPLES.iter().position(|s| s.id == id)
}

/// 装载失败。
#[derive(Debug)]
pub enum LoadError {
    /// 语料根目录不存在或读不了
    Root {
        /// 试过的路径
        path: PathBuf,
        /// 底层原因
        why: String,
    },
    /// 某个样本文件不在
    File {
        /// 样本 ID
        id: &'static str,
        /// 试过的路径
        path: PathBuf,
        /// 底层原因
        why: String,
    },
    /// 读文件失败（文件在但读不了：权限、编码等）
    Read {
        /// 样本 ID
        id: &'static str,
        /// 试过的路径
        path: PathBuf,
        /// 底层原因
        why: String,
    },
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Root { path, why } => {
                write!(f, "语料根目录不可用（{}）：{why}", path.display())
            }
            Self::File { id, path, why } => {
                write!(f, "样本 {id} 的文件缺失（{}）：{why}", path.display())
            }
            Self::Read { id, path, why } => {
                write!(f, "样本 {id} 的文件读不了（{}）：{why}", path.display())
            }
        }
    }
}

impl std::error::Error for LoadError {}

/// 装好的一批用例：持有源码全文与 `.out` 内容，产出可借用的 [`CaseSpec`]。
///
/// # 为什么要自己持有而不是直接把 `CaseSpec` 存起来
///
/// [`CaseSpec`] 借的是 `&'a str` / `&'a Path`（`name` / `path` / `src`）。
/// 若把 `PathBuf` 存进 `Vec` 再借它给 `CaseSpec`，就撞上**自引用** ——
/// `Vec` 一扩容，所有借出的引用全部失效。解法是**两段式**：
/// 先把路径与源码收进 [`LoadedSamples`]（借用者住在同一个结构里，且不重排），
/// 再由 [`Self::specs`] 按需生成 `CaseSpec`。
///
/// 这也是 `CaseSpec` 必须带**磁盘路径**（而不只是源码）的原因：真实语料在
/// **只读**的 `GOROOT/test`，执行器只读它、从不写它。
#[derive(Debug, Clone)]
pub struct LoadedSamples {
    root: PathBuf,
    /// 每条：`PathBuf` 与两个 `String`（源码、`.out`），与 `CORPUS_M0_SAMPLES` 同序
    entries: Vec<LoadedEntry>,
}

/// 内部一行：短名 + 路径 + 源码 + 期望输出。
#[derive(Debug, Clone)]
struct LoadedEntry {
    /// 短文件名（`helloworld.go`）—— 诊断前缀，**借用时不会因 `Vec` 扩容而失效**
    name: String,
    path: PathBuf,
    src: String,
    expected_out: Option<String>,
}

impl LoadedSamples {
    /// 从语料根装载 `entries` 里的样本。
    ///
    /// **一个文件都不能少**：缺一个就 `Err`，不做「跳过缺失的」——
    /// 静默跳过会让 E4 的分母从 20 悄悄变成 19，而这正是 `M0-tests.md` §8 禁止的
    /// 「缩小分母以达成门禁」。
    pub fn load(root: &Path, entries: &[SampleEntry]) -> Result<Self, LoadError> {
        if !root.is_dir() {
            return Err(LoadError::Root {
                path: root.to_path_buf(),
                why: "不是目录".into(),
            });
        }
        let mut loaded = Vec::with_capacity(entries.len());
        for e in entries {
            let path = root.join(e.path);
            if !path.is_file() {
                return Err(LoadError::File {
                    id: e.id,
                    path,
                    why: "文件不存在".into(),
                });
            }
            let src = std::fs::read_to_string(&path).map_err(|why| LoadError::Read {
                id: e.id,
                path: path.clone(),
                why: why.to_string(),
            })?;
            // R2：`.out` 不存在 ⇒ 期望为**空**（`None`）。
            // ⚠️ 「文件不存在」与「内容是空串」必须能区分 —— 后者说明 `.out` 存在但为空，
            // 那是另一回事（`.out` 里的空行也算期望输出）。
            let out_path = path.with_extension("out");
            let expected_out = match std::fs::read_to_string(&out_path) {
                Ok(s) => Some(s),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(why) => {
                    return Err(LoadError::Read {
                        id: e.id,
                        path: out_path,
                        why: why.to_string(),
                    });
                }
            };
            loaded.push(LoadedEntry {
                name: e.path.to_string(),
                path,
                src,
                expected_out,
            });
        }
        Ok(Self {
            root: root.to_path_buf(),
            entries: loaded,
        })
    }

    /// 语料根目录。
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 装了几条。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否一条都没装（**空集不算错误**，但调用方应自己判分母）。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 产出可交给 [`rgoc_harness::runner::run_layer`] 的 [`CaseSpec`]。
    ///
    /// `name` 用**短文件名**（`helloworld.go`）—— 诊断前缀就是它，
    /// 而语料里的诊断带的是全路径，比较器靠 `replace_prefix` 换回去（R6 细节 3）。
    pub fn specs(&self, layer: Layer) -> Vec<CaseSpec<'_>> {
        self.entries
            .iter()
            .map(|e| CaseSpec {
                name: &e.name,
                path: &e.path,
                src: &e.src,
                expected_out: e.expected_out.clone(),
                layer,
            })
            .collect()
    }
}
