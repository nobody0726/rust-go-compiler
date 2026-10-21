//! 源码 import（`D-M3-5`）—— 按**显式路径映射**装载被依赖包，并给 import 环出诊断。
//!
//! # 口径（`D-M3-5` / `D-M3-14`）
//!
//! | 做 | **不**做（归谁） |
//! |---|---|
//! | 在**调用方给的 roots** 内按 `import` 路径找**同名目录**的 `.go` 文件 | `GOPATH` / 模块解析 / `go.mod` / vendor（`U-M3-07`，M4+） |
//! | 递归装载（深度上限 [`MAX_IMPORT_DEPTH`]） | 二进制 export data（标准库那种只有归档的包） |
//! | import 环诊断（**环上每个包一条**） | 构建约束筛选 —— `D-M3-14`：**不读** `//go:build` 筛文件 |
//!
//! # 与官方的对应
//!
//! `go/types` **不做**源码 import：它把这件事交给 `Config.Importer`，
//! 官方那份实现是 `go/internal/srcimporter`（**源码** importer）。所以本模块是
//! 「srcimporter 的一部分 + `go/build` 的目录探测」两件事的合并，搬运时按下面两处对齐：
//!
//! | 这里 | 官方 |
//! |---|---|
//! | 目录里挑哪些文件 | `go/build/pkg.go` 的 `ImportDir`（`_`/`.` 开头与 `_test.go` 都不算） |
//! | 环的文案 | `srcimporter.go:85` 的 `import cycle through package %q` |
//! | 环的口径 | 官方在 importer 里**只报一条纯文本**（无代号）；`M3-design` §2 加的口径是「环上每个包一条」 |
//!
//! # 四条关键设计（都踩过点）
//!
//! 1. **会话共用一张 [`FileTable`]**：入口包与被依赖包都在同一张表里取 `FileId`。
//!    各建一张表会让每个包都从 `FileId(0)` 起编号 ⇒ `Diag.pos.file` 指错文件。
//!    会话末尾把**最终**表回填给每个 [`Package`]（[`Session::backfill`]），
//!    于是任何一条诊断都能用入口包的 `file_table` 渲染。
//! 2. **环检测的键是「解析后的目录」**（`fs::canonicalize`），不是 `import` 路径：
//!    否则「入口包恰好在环上」这条最常见的形状检测不到 —— 入口包不是被 import
//!    进来的，它没有路径。目录键同时也让 `import "lib"` 与别名写法指向同一个包。
//! 3. **每个包只装载一次**（[`Session::cache`]，键同上）：重复 import 直接命中缓存，
//!    于是被依赖包自己的诊断（含**语法**诊断）不会重复出现。
//!    ⚠️ 而**失败**的 import 例外：官方对**每条** import spec 各报一次
//!    （`go/types/resolver.go` 的 `check.errorf(at, BrokenImport, …)` 用的是 spec 的位置）
//!    ⇒ 这里也对每条报（不入缓存）。
//! 4. **顺序：先求值本包，再解析它的 import。** 已知缺口：`lib.T` 这类**跨包**引用
//!    在 M3 里解析不了（`load_sources_into` 跑完才轮到 import）⇒ 进 `open_gaps`，
//!    归 M4（那时要改成「先装载依赖、再求值本包」两阶段）。

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use rgoc_ast::{Decl, ImportDecl};
use rgoc_lex::{Diag, DiagBag, DiagCode, FileId, FileTable, Pos};
use rgoc_types::{ObjKind, Object, ScopeId, TypeTable};

use crate::exported::is_exported;
use crate::package::{
    Package, Source, default_import_name, evaluate_package, imp_path, load_decls_only, read_sources,
};

/// import 链条的最大长度（**不含**入口包）—— `M3-design.md` §7.2 的「硬上限」。
///
/// 为什么必须有：环靠「正在装载栈」检测，而**不**成环的长链条只能靠上限挡住
/// （否则 1000 层 `import` 会把宿主栈吃穿）。超限**只报一条**（`NestingTooDeep`
/// 与 `rgoc-types` 的类型深度判定同一个理由：这是**我方能力边界**，不是用户写错）。
pub const MAX_IMPORT_DEPTH: usize = 100;

/// 一个被解析出来的**源码目录**。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportDir {
    /// 目录（**规范化**后的形态 —— 它同时是环检测与缓存的键）。
    pub dir: PathBuf,
    /// 目录里可用的 `.go` 文件（**路径序**）。空 ⇒ 「目录在，但没有可用的 `.go`」。
    pub files: Vec<PathBuf>,
}

/// 显式路径映射：只在调用方给的根目录里找**同名目录**。
#[derive(Debug, Clone, Default)]
pub struct ImportResolver {
    roots: Vec<PathBuf>,
}

impl ImportResolver {
    /// `roots` 的**顺序有意义**：第一个命中的根生效（`D-M3-5` 的「显式路径映射」）。
    #[must_use]
    pub fn new(roots: &[PathBuf]) -> Self {
        Self {
            roots: roots.to_vec(),
        }
    }

    /// 按 `import` 路径找同名目录。
    ///
    /// `None` ⇒ 所有 root 下都**没有这个目录**。调用方据此报「不支持的形式」
    /// （见 [`load_with_imports`] 的注释：找不到源码与「不支持这种 import 形式」
    /// 在 M3 里是同一件事——我们既不做模块解析也不读 export data）。
    #[must_use]
    pub fn resolve(&self, path: &str) -> Option<ImportDir> {
        for root in &self.roots {
            let raw = root.join(path);
            if !raw.is_dir() {
                continue;
            }
            let dir = canonical(&raw);
            let mut files: Vec<PathBuf> = Vec::new();
            let Ok(entries) = fs::read_dir(&raw) else {
                // 目录**在**、但读不了 ⇒ 当成「没有可用文件」（不是「没有这个目录」）
                return Some(ImportDir { dir, files });
            };
            for e in entries.flatten() {
                let p = e.path();
                if !p.is_file() {
                    continue;
                }
                let Some(name) = p.file_name().and_then(|s| s.to_str()) else {
                    continue;
                };
                // 官方 `go/build` 的 `shouldBuild`：`_` / `.` 开头的不算；
                // 测试文件**不**属于被装载的包（`_test.go` 另建 `foo_test` 包）。
                if name.starts_with('_') || name.starts_with('.') {
                    continue;
                }
                if !name.ends_with(".go") || name.ends_with("_test.go") {
                    continue;
                }
                files.push(p);
            }
            files.sort();
            return Some(ImportDir { dir, files });
        }
        None
    }
}

/// 从**显式文件列表**装载入口包，并递归装载它的**源码 import**。
///
/// 与 [`crate::load_from_explicit_files`] 的区别只有一处：多了 `roots`
/// —— 有了它才有 import 解析（[`Package::imports`] 会被填上）。
///
/// 「找不到源码」为什么报 [`DiagCode::TypUnsupportedImportForm`] 而不是「找不到」：
/// M3 既不做模块解析也不读二进制 export data（`D-M3-5`），所以「roots 里没有这个路径的
/// 源码」与「这是标准库/模块路径」在**能力**上是同一件事 —— 官方的
/// `go/types` 在这里会说 `could not import fmt (can't find import: "fmt")`，
/// 但那是因为它有 `Config.Importer`；我们没有那一步（`D-M3-11` 的稳定拒绝码）。
pub fn load_with_imports(
    tt: &mut TypeTable,
    entry: &[PathBuf],
    roots: &[PathBuf],
    lang: Option<&str>,
) -> (Package, DiagBag) {
    let mut s = Session {
        tt,
        resolver: ImportResolver::new(roots),
        file_table: FileTable::new(),
        bag: DiagBag::new(),
        stack: Vec::new(),
        cache: BTreeMap::new(),
        depth_reported: false,
    };
    // 入口包也占**一格**：`Package::imports` 里不会出现它，但环检测必须能认出
    // 「绕回入口包」这条形状（它没有 import 位置 ⇒ `pos: None`，报环时借用闭合边）。
    s.stack.push(Frame {
        dir: entry_dir(entry),
        pos: None,
    });
    let mut pkg = s.load_package(entry, lang);
    s.stack.pop();
    s.backfill(&mut pkg);
    (pkg, s.bag)
}

/// 入口包的目录（取**第一个**文件所在目录；显式文件列表可以跨目录，M3 不细分）。
fn entry_dir(files: &[PathBuf]) -> PathBuf {
    files
        .first()
        .and_then(|f| f.parent())
        .map_or_else(PathBuf::new, canonical)
}

/// 目录的**规范化**形态（当键用）；规范化失败（不存在/无权限）就原样返回。
fn canonical(p: &Path) -> PathBuf {
    fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// 正在装载的一个包。
struct Frame {
    /// 规范化后的目录 —— 环检测与缓存的**键**。
    dir: PathBuf,
    /// 把它拉进来的那条 `import` 的位置；入口包是 `None`。
    pos: Option<Pos>,
}

struct Session<'a> {
    tt: &'a mut TypeTable,
    resolver: ImportResolver,
    /// **会话级**文件表：入口包与被依赖包共用（模块文档第 1 条）。
    file_table: FileTable,
    bag: DiagBag,
    stack: Vec<Frame>,
    /// 已装载完的包：规范化目录 → 装载结果（模块文档第 3 条）。
    cache: BTreeMap<PathBuf, Package>,
    /// 深度超限**只报一次**。
    depth_reported: bool,
}

impl Session<'_> {
    /// 装载一个包（含它的直接依赖）。
    ///
    /// # 四步（M4/Task 35 把原来的「先求值、后 import」翻了过来）
    ///
    /// ```text
    /// 阶段 A（load_decls_only）  解析 + 名字 —— 还没有类型
    ///   ↓
    /// resolve_imports            递归装载依赖（每个依赖自己走完这四步）
    ///   ↓
    /// stitch_imports             把 import 名缝进文件作用域（点导入 / 改名 / 冲突）
    ///   ↓
    /// 阶段 B（evaluate_package） 类型表达式 + 常量 + 体内
    /// ```
    ///
    /// 为什么必须这个顺序：`lib.T` 是**类型表达式**，阶段 B 的 `lookup_type`
    /// 要沿作用域链找 `lib` 与 `T`；而 `lib` 的对象与 `T` 的类型都只有在依赖
    /// 装载完之后才存在。M3 的顺序（先求值本包、后 import）正是 `importer`
    /// 模块文档第 4 条记的那个缺口，这里补齐。
    fn load_package(&mut self, files: &[PathBuf], lang: Option<&str>) -> Package {
        let (sources, skipped) = read_sources(files);
        let borrowed: Vec<Source<'_>> = sources
            .iter()
            .map(|(n, b)| (n.as_str(), b.as_slice()))
            .collect();
        let (mut pkg, mut sub) = load_decls_only(self.tt, &mut self.file_table, &borrowed, lang);
        pkg.skipped = skipped;
        // 依赖先装载（递归；`pkg.imports` 在这一步填上）
        self.resolve_imports(&mut pkg, lang);
        // 再把 import 名缝进文件作用域 —— 必须早于阶段 B
        stitch_imports(&mut pkg, &mut sub);
        // 依赖齐了，本包才求值
        evaluate_package(self.tt, &mut pkg, &mut sub);
        for d in sub.into_sorted() {
            self.bag.push(d);
        }
        pkg
    }

    /// 解析一个包里所有文件的 `import`。
    ///
    /// 顺序 = **文件顺序 → 声明顺序**（与 Task 23 收 import 名的顺序同源）⇒ 诊断确定。
    fn resolve_imports(&mut self, pkg: &mut Package, lang: Option<&str>) {
        for (path, pos) in import_sites(pkg) {
            // 已装载过的路径直接跳过（缓存/环都在下面处理；**失败**的不缓存 ⇒ 每条都报）
            if pkg.imports.contains_key(&path) {
                continue;
            }
            if path.is_empty() {
                self.bag.push(Diag {
                    pos,
                    code: DiagCode::TypUnsupportedImportForm,
                    message: "unsupported import form: empty import path".to_owned(),
                });
                continue;
            }
            let Some(id) = self.resolver.resolve(&path) else {
                self.bag.push(Diag {
                    pos,
                    code: DiagCode::TypUnsupportedImportForm,
                    message: format!(
                        "unsupported import form: {path:?} (no source directory under the import roots; binary export data and module resolution are out of scope)"
                    ),
                });
                continue;
            };
            if let Some(dep) = self.cache.get(&id.dir) {
                pkg.imports.insert(path, dep.clone());
                continue;
            }
            if let Some(i) = self.stack.iter().position(|f| f.dir == id.dir) {
                self.report_cycle(i, pos, &path);
                continue;
            }
            if id.files.is_empty() {
                self.bag.push(Diag {
                    pos,
                    code: DiagCode::TypBrokenImport,
                    message: format!(
                        "could not import {path} (no Go files in {})",
                        id.dir.display()
                    ),
                });
                continue;
            }
            // 深度：`stack` 含入口帧 ⇒ 不含入口的链条长度 = `len() - 1`
            if self.stack.len() > MAX_IMPORT_DEPTH {
                if !self.depth_reported {
                    self.depth_reported = true;
                    self.bag.push(Diag {
                        pos,
                        code: DiagCode::TypNestingTooDeep,
                        message: format!(
                            "import depth exceeds {MAX_IMPORT_DEPTH} (implementation limit)"
                        ),
                    });
                }
                continue;
            }
            self.stack.push(Frame {
                dir: id.dir.clone(),
                pos: Some(pos),
            });
            let dep = self.load_package(&id.files, lang);
            self.stack.pop();
            self.cache.insert(id.dir, dep.clone());
            pkg.imports.insert(path, dep);
        }
    }

    /// 报一个 import 环：**环上每个包一条**。
    ///
    /// `i` 是 `stack` 里被**重新进入**的那一格 ⇒ `stack[i..]` 就是环上的包
    /// （`stack[..i]` 只是通向它的路）。位置取「把该包拉进环的那条 import」；
    /// 入口包没有 import（`pos: None`）⇒ 借用**闭合边** `pos_close`
    /// （发现环的那条 import —— 也就是「环在这里合上」）。
    ///
    /// 消息里的包名用**闭合边的 `import` 路径**（`path`）而不是目录：
    /// 官方文案是 `import cycle through package %q`，那里的 `%q` 是
    /// `bp.ImportPath`（`srcimporter.go:85`），也就是**源码里写的那串路径**。
    fn report_cycle(&mut self, i: usize, pos_close: Pos, path: &str) {
        for f in &self.stack[i..] {
            self.bag.push(Diag {
                pos: f.pos.unwrap_or(pos_close),
                code: DiagCode::TypImportCycle,
                message: format!("import cycle through package {path:?}"),
            });
        }
    }

    /// 会话结束后把**最终**文件表回填给每个包（模块文档第 1 条）。
    ///
    /// 递归是安全的：环上的那条 import 没有装载结果 ⇒ [`Package::imports`] 无环。
    fn backfill(&self, pkg: &mut Package) {
        pkg.file_table = self.file_table.clone();
        for dep in pkg.imports.values_mut() {
            self.backfill(dep);
        }
    }
}

/// 一个包里所有 `import` 的（路径, 位置），按**文件序 → 声明序**。
///
/// 只走**被接受**的文件（`pkg.asts`）—— 包名不一致被忽略的文件，官方也**不**解析它的
/// import（`// ignore this file`）。
fn import_sites(pkg: &Package) -> Vec<(String, Pos)> {
    let mut out = Vec::new();
    for ast in &pkg.asts {
        for decl in &ast.decls {
            let Decl::Import(imp) = decl else { continue };
            // 路径缺失（`import x`）⇒ 解析器已经报过 `missing import path`，这里不重复报
            if let Some(p) = import_path(imp) {
                out.push((p, import_pos(imp)));
            }
        }
    }
    out
}

/// `import` 的路径文本（**只剥引号**）。
///
/// ⚠️ **不做**转义解码：`import "\x66mt"` 这种写法拿到的是字面量里的原文
/// （官方走 `strconv.Unquote`）⇒ `open_gaps`。Task 23 收 import **名**时是同一个取巧。
fn import_path(imp: &ImportDecl) -> Option<String> {
    let lit = imp.path.as_ref()?;
    let trimmed = lit.value.trim_matches(|c| c == '"' || c == '`');
    Some(trimmed.to_string())
}

/// `import` spec 的位置（有别名取别名，否则取 spec 本身）—— 与 Task 23 同一取法。
fn import_pos(imp: &ImportDecl) -> Pos {
    imp.name.as_ref().map_or(imp.pos, |n| n.pos)
}

// ══════════════════════ 缝 import 名（M4/Task 35，`T-CK-74/75`）══════════════════

/// 把一个包的 `import` **缝进文件作用域** —— 三种形态的名字进入规则。
///
/// # 与官方的对应（`go/types/resolver.go:262-334` + `:461-478`）
///
/// | 规则 | 官方 | 这里 |
/// |---|---|---|
/// | 无别名 ⇒ 用**被导入包自己的包名**（不是路径最后一段） | `name := imp.name` | 阶段 A 只能拿到路径最后一段（那时依赖还没装载）⇒ 这里**改名**（`Ruling 35-2`） |
/// | 别名 `f` ⇒ 绑定 `f` | `name = d.spec.Name.Name` | 阶段 A 已绑好，这里只做冲突检查 |
/// | `_` ⇒ **不绑定** | `declare` 对 `_` 早退 | 阶段 A 就没绑，这里跳过 |
/// | `.` ⇒ 把被导入包的**导出名**注入本文件 | `:311-333`（`token.IsExported` 过滤 + `redeclared in this block`） | 同 |
/// | `import init "x"` ⇒ 错 | `:288`（`InvalidInitDecl`） | 同 |
/// | 文件层名字与**包层**同名 ⇒ 错 | `:461-478`（`already declared through import/dot-import of %s`） | 同 |
///
/// # 为什么拆成「先规划、后落地」
///
/// 规划阶段要**读** `pkg.imports`（被依赖包的导出表），落地阶段要**写**
/// `pkg.scopes` —— 两者都在 `pkg` 里，同时借会打架。于是先把要注入的对象
/// **克隆**出来（`Object` 是 `String + 几个 Option`，克隆很便宜），再统一写。
///
/// # 只对**已装载**的 import 报冲突
///
/// 官方在 import 失败时 `return`（`:278`），**不建** `PkgName` ⇒ 也不会有冲突。
/// 我们的阶段 A 无论成败都先占了个临时名，所以这里只对 `pkg.imports` 里真有的
/// 路径做改名 / 注入 / 冲突 —— 失败的那条留给 `resolve_imports` 的
/// `TYP-UNSUPPORTED-IMPORT-FORM` / `TYP-BROKEN-IMPORT`。
pub fn stitch_imports(pkg: &mut Package, bag: &mut DiagBag) {
    let plans = plan_stitches(pkg);
    // ── 落地：改名 / 点导入注入 ──────────────────────────────────────────
    for (fscope, from, to, pos) in plans.renames {
        let Some(mut obj) = pkg.scopes.remove(fscope, &from) else {
            continue; // 阶段 A 没绑（`_` / `.` / 路径缺失）⇒ 没得改
        };
        obj.name.clone_from(&to);
        if pkg.scopes.insert(fscope, obj).is_some() {
            bag.push(Diag {
                pos,
                code: DiagCode::TypDuplicateDecl,
                message: format!("{to} redeclared in this block"),
            });
        }
    }
    for (fscope, dot_pos, items) in plans.dots {
        for obj in items {
            let name = obj.name.clone();
            if pkg.scopes.get(fscope).lookup_local(&name).is_some() {
                // 官方 `:325`：`err.addf(d.spec.Name, "%s redeclared in this block", ...)`
                bag.push(Diag {
                    pos: dot_pos,
                    code: DiagCode::TypDuplicateDecl,
                    message: format!("{name} redeclared in this block"),
                });
                continue;
            }
            pkg.scopes.insert(fscope, obj);
        }
    }
    // ── `import init "x"` ────────────────────────────────────────────────
    for pos in plans.init_errs {
        bag.push(Diag {
            pos,
            code: DiagCode::TypInvalidInitDecl,
            message: "cannot import package as init - init must be a func".to_owned(),
        });
    }
    // ── 文件层 × 包层同名（官方 `:461-478`）───────────────────────────────
    for (fscope, name, qual) in plans.collisions {
        let Some(alt_pos) = pkg
            .scopes
            .get(pkg.scope)
            .lookup_local(&name)
            .and_then(|o| o.pos)
        else {
            continue;
        };
        let msg = if pkg
            .scopes
            .get(fscope)
            .lookup_local(&name)
            .is_some_and(|o| o.kind == ObjKind::PkgName)
        {
            format!("{name} already declared through import of {qual}")
        } else {
            format!("{name} already declared through dot-import of {qual}")
        };
        bag.push(Diag {
            pos: alt_pos,
            code: DiagCode::TypDuplicateDecl,
            message: msg,
        });
    }
}

/// `stitch_imports` 的**只读**规划结果。
#[derive(Default)]
struct Plans {
    /// `(文件作用域, 临时名, 真名, 报点)` —— 无别名 import 的改名。
    renames: Vec<(ScopeId, String, String, Pos)>,
    /// `(文件作用域, 要注入的导出对象)` —— 点导入。
    dots: Vec<(ScopeId, Pos, Vec<Object>)>,
    /// `import init "x"` 的报点。
    init_errs: Vec<Pos>,
    /// `(文件作用域, 名字, 包限定串)` —— 与包层同名的候选。
    collisions: Vec<(ScopeId, String, String)>,
}

/// 官方 `types.Package.String()`：`package <名字> ("<路径>")`。
///
/// 消息里那句 `already declared through import of %s` 的 `%s` 就是它
/// （`pkg.Imported()` / `obj.Pkg()` 都渲染成这个形状）—— 探针实测：
/// `lib already declared through import of package lib ("lib")`。
fn pkg_qual(name: &str, path: &str) -> String {
    format!("package {name} (\"{path}\")")
}

fn plan_stitches(pkg: &Package) -> Plans {
    let mut plans = Plans::default();
    // 每个文件：作用域 + 它的 import spec（声明序）
    let files: Vec<(FileId, ScopeId)> = pkg
        .asts
        .iter()
        .filter_map(|a| Some((a.pos.file, pkg.file_scope(a.pos.file)?)))
        .collect();
    for (fid, fscope) in files {
        // `pkg.imports` 里真有的那几条：名字 → 路径
        let mut bound: Vec<(String, String)> = Vec::new();
        for ast in pkg.asts.iter().filter(|a| a.pos.file == fid) {
            for decl in &ast.decls {
                let Decl::Import(imp) = decl else { continue };
                let Some(path) = imp_path(imp) else { continue };
                let Some(dep) = pkg.imports.get(&path) else {
                    continue; // 没装载成功 ⇒ 官方连 PkgName 都不建
                };
                let qual = pkg_qual(&dep.name, &path);
                let explicit = imp.name.as_ref().map(|n| (n.value.as_str(), n.pos));
                let pos = import_pos(imp);
                match explicit {
                    Some(("_", _)) => {}
                    Some((".", dot_pos)) => {
                        let items = dot_objects(dep);
                        // 注入的每个名字都要过「文件层 × 包层」那道检查
                        for o in &items {
                            plans
                                .collisions
                                .push((fscope, o.name.clone(), qual.clone()));
                        }
                        plans.dots.push((fscope, dot_pos, items));
                    }
                    Some((name, _)) => bound.push((name.to_string(), path.clone())),
                    None => {
                        // 无别名：真名 = 被导入包的包名；与阶段 A 的临时名不同就改名
                        let temp = default_import_name(&path);
                        let real = if dep.name.is_empty() {
                            temp.clone()
                        } else {
                            dep.name.clone()
                        };
                        if real != temp {
                            plans.renames.push((fscope, temp, real.clone(), pos));
                        }
                        bound.push((real, path.clone()));
                    }
                }
                // `import init "x"`：官方 `:288`，报在 spec 的名字位上
                if imp.name.as_ref().is_some_and(|n| n.value == "init") {
                    plans.init_errs.push(pos);
                }
            }
        }
        // 与包层同名的候选（只对**已装载**的绑定 —— 见函数文档末段）
        for (name, path) in bound {
            let qual = pkg
                .imports
                .get(&path)
                .map_or_else(|| pkg_qual(&name, &path), |d| pkg_qual(&d.name, &path));
            plans.collisions.push((fscope, name, qual));
        }
    }
    plans
}

/// 点导入要注入的对象：被导入包**包作用域本层**的**导出**名（官方 `:311-333`）。
///
/// 顺序 = 名字字典序（`ScopeTable` 的 `BTreeMap` 序）—— 官方遍历 Go map 是
/// 随机序，但它的诊断最后按位置排序，所以顺序不影响可观测结果；我们取确定序。
fn dot_objects(dep: &Package) -> Vec<Object> {
    let scope = dep.scopes.get(dep.scope);
    scope
        .names()
        .filter(|n| is_exported(n))
        .filter_map(|n| scope.lookup_local(n).cloned())
        .collect()
}
