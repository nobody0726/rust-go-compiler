//! 单包多文件装载（M3/Task 23）。
//!
//! # 职责
//!
//! 从**显式文件列表**（`D-M3-14`：不评估构建约束）建出一个 [`Package`]：
//!
//! ```text
//! 读文件 → parse_file_in（共享 FileTable）→ 包名一致性 → 三层作用域
//!        universe ⊃ package ⊃ file → 顶层声明名（占位）+ import 名
//! ```
//!
//! # 与官方 `go/types` 的对应
//!
//! | 这里 | 官方 |
//! |---|---|
//! | 包名一致性（本文件） | `checker.initFiles`（`go/types/check.go:265-310`） |
//! | 顶层声明名占位（本文件） | `Checker.collectObjects` 的「先建对象后求值」 |
//! | 类型/常量求值顺序 | **Task 24**（`decl_collect.rs`）—— 本任务只放名字 |
//!
//! # 为什么**先收名字、后求值**（Task 23 只做前半）
//!
//! Go 允许前向引用（`type A B` 写在 `type B int` 之前），也允许常量组内互相引用。
//! 官方因此把「声明」与「求值」拆成两遍。本任务只做第一遍：**名字**进作用域
//! （对象是**占位**：`typ: None` / `val: None`），第二遍（Task 24）才填类型与值。
//!
//! 这样切的好处是 `T-TY-61`（同包跨文件可见）在**只有名字**时就已经成立，
//! 而「前向引用」的正确性由 Task 24 的求值顺序单独证明 —— 两件事各有测试。

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use rgoc_ast::{AstFile, Decl, ImportDecl};
use rgoc_lex::{Diag, DiagBag, DiagCode, FileId, FileTable, Pos};
use rgoc_parse::parse_file_in;
use rgoc_types::{
    ObjKind, Object, ScopeId, ScopeKind, ScopeRef, ScopeTable, TypeId, TypeParamId, TypeTable,
    Versions, declare_universe, pkg_versions,
};

use crate::decls::collect_decls;

/// 一个已装载的包。
///
/// # 谁拥有什么
///
/// - **作用域树归本结构**（[`Package::scopes`]）—— 因为「装载的结果」必须能被
///   **返回**出去（借用式作用域树做不到，见 `rgoc-types::scope` 的 Ruling 23-1）；
/// - **类型表不归本结构**：universe 必须声明进 dump 编号用的那张表
///   （Task 3 实测：oracle 把 universe 的 `error` 也编号），而多包还要共享它
///   ⇒ 由调用方持有、以 `&mut` 传入（Ruling 23-2）。
#[derive(Debug, Clone)]
pub struct Package {
    /// 包名 —— 取**第一个**非 `_` 文件的 `package` 子句（官方 `initFiles`）。
    ///
    /// 全为 `_` 时是空串。空串**不是**「没装载」：`files` 里有没有文件才算。
    pub name: String,
    /// **被接受**的文件的 [`FileId`]（按传入顺序）。
    ///
    /// 包名不一致的文件**不在这里** —— 官方那句 `// ignore this file`。
    pub files: Vec<FileId>,
    /// 文件名表：`files` 里的 `FileId` 都指向它，用于渲染诊断位置。
    pub file_table: FileTable,
    /// 被接受文件的语法树（按 `files` 同序）—— Task 24 求值要用。
    pub asts: Vec<AstFile>,
    /// 作用域竞技场（universe / package / 每文件一层）。
    pub scopes: ScopeTable,
    /// **包作用域**（`scopes` 里的一层）。
    pub scope: ScopeId,
    /// 每个被接受文件 → 它的**文件作用域**（`import` 名住在这里）。
    pub(crate) file_scopes: Vec<(FileId, ScopeId)>,
    /// 每个文件生效的语言版本（`D-M3-15`：版本**按文件**）。
    ///
    /// 由 [`load_sources`] 在装载时算好（`pkg_versions` 的两处 `TooNew` 诊断
    /// 也在那时进诊断袋）。M3 还没有「门控点」来消费它（有门控的地方都在
    /// M4+），所以它现在只是**如实记录**各文件版本 —— 但接线在这里就位，
    /// 免得以后加门控时要回头改装载入口。
    pub versions: Versions,
    /// **dump 根**（`C7` §3 口径，声明序）—— [`collect_decls`] 填。
    ///
    /// 谁消费它：`rgoc-types::dump_types(tt, &pkg.type_roots)`。
    /// 顺序不是「类型表里的下标序」：表的下标是**创建序**，而根序是**声明序**
    /// （`C7` §3 冻结的正是后者，节 2 的编号规则要求两侧遍历顺序一致）。
    pub type_roots: Vec<TypeId>,
    /// 读不到的文件（路径, 原因）。
    ///
    /// 为什么不报诊断：`go/types` **不读文件**（调用方读），所以官方没有
    /// 「文件读不到」这一类错误，我们也就**没有**对应代号 —— 而
    /// 「没有触发者的码不许进枚举」（`M3-plan` §4）⇒ 这里如实**记下**，
    /// 由调用方（Task 36 的 CLI，它本来要先探可读性）决定怎么报。
    /// 静默跳过是不行的：那正是本仓反复记的「静默失败」形状。
    pub skipped: Vec<(String, String)>,
    /// **直接**被依赖的包：`import` 路径 → 已装载的包（Task 27）。
    ///
    /// 键是**源码里写的路径**（`import "a/b"` ⇒ `"a/b"`），值是装载结果 ——
    /// 它和入口包一样自带作用域树，但**类型住在同一个 [`TypeTable`] 里**
    /// （`D-M3-1` 的「跨包同一性」靠的就是这一点）。
    ///
    /// 只有 [`crate::load_with_imports`] 会填；[`load_sources`] 与
    /// [`load_from_explicit_files`] 留空（**没有 roots 就没有 import 解析**）。
    /// ⚠️ 环**不会**进这里：环上那条 import 没有装载结果（见 `importer` 的模块文档），
    /// 所以这棵树在结构上无环。
    pub imports: BTreeMap<String, Package>,
    /// **包级泛型函数**的类型参数表：函数名 → 它自己的 `[T any]`（声明序）。
    ///
    /// # 为什么需要这张表（M12/Task 6）
    ///
    /// `SignatureDef` **没有** `tparams` 栏（M3 的决定：类型参数只作为作用域
    /// 对象存在，不进签名 —— 进了签名就会改变 `C7` 的 dump 口径）。于是
    /// `func Id[T any](x T) T` 的 `T` 一旦离开那个作用域就无从取回，而
    /// **显式实例化**（`Id[int]`）与**单态化**（`M12a` 的主体）都必须在
    /// 调用点把「形参表」和「实参表」按位置对齐。
    ///
    /// 所以装载期把这张表**如实记下来**（就是 `declare_tparams` 已经算好的那组
    /// 条目），检查面与单态化都从这一个来源取 —— 不在两处各推一遍。
    ///
    /// 只收**包级**函数：方法的类型参数分两类（接收者的在
    /// `SignatureDef::recv_tparams`，方法自己的目前仍只活在作用域里），
    /// `F[int]` 这种写法对方法不成立（Go 不允许给方法显式指定类型实参）。
    pub generic_funcs: BTreeMap<String, Vec<TypeParamId>>,
}

impl Package {
    /// 包作用域的只读句柄（沿链可到 universe）。
    #[must_use]
    pub fn pkg_scope(&self) -> ScopeRef<'_> {
        self.scopes.scope(self.scope)
    }

    /// 某个文件的文件作用域（没有该文件 ⇒ `None`）。
    #[must_use]
    pub fn file_scope(&self, file: FileId) -> Option<ScopeId> {
        self.file_scopes
            .iter()
            .find(|(f, _)| *f == file)
            .map(|(_, s)| *s)
    }

    /// 从**包作用域**沿链查名字（含 universe）。
    #[must_use]
    pub fn lookup(&self, name: &str) -> Option<&Object> {
        self.scopes.lookup(self.scope, name)
    }

    /// 从**某个文件**的作用域沿链查名字（文件层 → 包层 → universe）。
    #[must_use]
    pub fn lookup_in_file(&self, file: FileId, name: &str) -> Option<&Object> {
        let s = self.file_scope(file)?;
        self.scopes.lookup(s, name)
    }

    /// 被接受文件的个数。
    #[must_use]
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// 某个文件引入的**包名 → `import` 路径**（按声明序）。
    ///
    /// 不含 `.` / `_`（它们不引入包名）与**缺路径**的 spec（解析器已报错）。
    /// 名字取「显式别名」，否则取路径最后一段 —— 这是**阶段 A** 的口径；
    /// 真正的包名来自被依赖包的 `package` 子句，由
    /// [`crate::importer::stitch_imports`] 在依赖装载后校正（`Ruling 35-2`）。
    #[must_use]
    pub fn import_bindings(&self, file: FileId) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let Some(ast) = self.asts.iter().find(|a| a.pos.file == file) else {
            return out;
        };
        for decl in &ast.decls {
            let Decl::Import(imp) = decl else { continue };
            let Some(path) = imp_path(imp) else { continue };
            if let Some(n) = &imp.name {
                if n.value == "_" || n.value == "." {
                    continue;
                }
                out.push((n.value.clone(), path));
            } else {
                // 真名优先取被依赖包的 `package` 子句（与 `stitch_imports` 的改名
                // 同源）；依赖没装载时退回路径最后一段（阶段 A 的临时名）。
                let name = self
                    .imports
                    .get(&path)
                    .map(|d| d.name.clone())
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| default_import_name(&path));
                out.push((name, path));
            }
        }
        out
    }

    /// 某个文件的**点导入**路径（按声明序）。
    #[must_use]
    pub fn dot_imports(&self, file: FileId) -> Vec<String> {
        let mut out = Vec::new();
        let Some(ast) = self.asts.iter().find(|a| a.pos.file == file) else {
            return out;
        };
        for decl in &ast.decls {
            let Decl::Import(imp) = decl else { continue };
            if imp.name.as_ref().is_some_and(|n| n.value == ".")
                && let Some(p) = imp_path(imp)
            {
                out.push(p);
            }
        }
        out
    }

    /// 这个文件里名为 `name` 的 import 绑定指向哪个**已装载**的包。
    ///
    /// `None` ⇒ 这个名字不是包名 / 那个 import 没装载成功（环、失败、无 roots）。
    #[must_use]
    pub fn imported(&self, file: FileId, name: &str) -> Option<&Package> {
        let path = self
            .import_bindings(file)
            .into_iter()
            .find(|(n, _)| n == name)
            .map(|(_, p)| p)?;
        self.imports.get(&path)
    }
}

/// 一个待装载的**内存源文件**（`T-TY-52/61/66` 的单元测试走这条）。
pub type Source<'a> = (&'a str, &'a [u8]);

/// 从**显式文件列表**装载一个包 —— M3 唯一的装载入口。
///
/// `lang` 是 `-lang go1.N` 的覆盖值；`None` ⇒ 按每个文件自己的
/// `//go:build` 版本（`D-M3-15`：版本门控**按文件**）。
pub fn load_from_explicit_files(
    tt: &mut TypeTable,
    files: &[PathBuf],
    lang: Option<&str>,
) -> (Package, DiagBag) {
    let (sources, skipped) = read_sources(files);
    let borrowed: Vec<Source<'_>> = sources
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    let (mut pkg, bag) = load_sources(tt, &borrowed, lang);
    pkg.skipped = skipped;
    (pkg, bag)
}

/// 读到的源文件：`(名字, 字节)`。
pub(crate) type RawSource = (String, Vec<u8>);

/// 读文件：`(源文件, 读不到的)` —— 读不到**不报诊断**（见 [`Package::skipped`]）。
pub(crate) fn read_sources(files: &[PathBuf]) -> (Vec<RawSource>, Vec<(String, String)>) {
    let mut sources: Vec<RawSource> = Vec::with_capacity(files.len());
    let mut skipped: Vec<(String, String)> = Vec::new();
    for p in files {
        let name = p.to_string_lossy().into_owned();
        match fs::read(p) {
            Ok(bytes) => sources.push((name, bytes)),
            // 读不到 ⇒ 记下来（`Package::skipped`），不猜也不伪造诊断
            Err(e) => skipped.push((name, e.to_string())),
        }
    }
    (sources, skipped)
}

/// 从内存里的源文件装载一个包（自建文件表 —— 单包入口都走这条）。
///
/// `sources` 的顺序就是 `Package::files` 的顺序（**确定性**从这里开始：
/// 包名取第一个、dump 的根按声明序、诊断排序的第一键是 `FileId`）。
pub fn load_sources(
    tt: &mut TypeTable,
    sources: &[Source<'_>],
    lang: Option<&str>,
) -> (Package, DiagBag) {
    let mut file_table = FileTable::new();
    load_sources_into(tt, &mut file_table, sources, lang)
}

/// 同 [`load_sources`]，但**用调用方的文件表**（Task 27 的装载会话走这条）。
///
/// 为什么需要它：会话里入口包与被依赖包**必须共用一张 [`FileTable`]**，
/// 否则每个包都从 `FileId(0)` 起编号，诊断位置（`Pos.file`）就会指错文件。
/// `Package::file_table` 在会话末尾会被回填成**最终**那张表（见 [`crate::importer`]）。
///
/// 本函数 = [`load_decls_only`] + [`evaluate_package`]（两阶段背靠背）。
/// 会话入口**不**走这里 —— 它要在两阶段**之间**插 import 解析（Task 35）。
pub fn load_sources_into(
    tt: &mut TypeTable,
    file_table: &mut FileTable,
    sources: &[Source<'_>],
    lang: Option<&str>,
) -> (Package, DiagBag) {
    let (mut pkg, mut bag) = load_decls_only(tt, file_table, sources, lang);
    // 单包入口的 `imports` 恒空 ⇒ 缝只对「`import init` / 已装载依赖」有事可做；
    // 那正是官方的口径（import 失败时 `return`，连名字都不建）。
    crate::importer::stitch_imports(&mut pkg, &mut bag);
    evaluate_package(tt, &mut pkg, &mut bag);
    (pkg, bag)
}

/// **阶段 A**：解析 + 包名一致性 + 三层作用域 + import 名 + 顶层名 + 版本表。
///
/// 做完这一步，`Package` 里**还没有类型与常量**（`Object::typ` / `Object::val`
/// 全空，`type_roots` 也空）—— 那些要等 [`evaluate_package`]。
///
/// 为什么允许「半成品」存在：`pkg.imports` 只能在**名字都收齐之后**才解析
/// （否则 `import` 语句的语义就要求先知道包名），而被依赖包的求值又要排在
/// 本包求值**之前** —— 于是「收名字」与「求值」之间必须有一个可插入的接缝。
pub fn load_decls_only(
    tt: &mut TypeTable,
    file_table: &mut FileTable,
    sources: &[Source<'_>],
    lang: Option<&str>,
) -> (Package, DiagBag) {
    let mut bag = DiagBag::new();
    let mut scopes = ScopeTable::new();
    let universe = declare_universe(tt, &mut scopes);
    let pkg_scope = scopes.child(ScopeKind::Package, universe);

    let mut pkg = Package {
        name: String::new(),
        files: Vec::new(),
        file_table: FileTable::new(),
        asts: Vec::new(),
        scopes,
        scope: pkg_scope,
        file_scopes: Vec::new(),
        versions: Versions::default(),
        type_roots: Vec::new(),
        skipped: Vec::new(),
        imports: BTreeMap::new(),
        generic_funcs: BTreeMap::new(),
    };

    for (name, src) in sources {
        // ① 解析（**共享文件表** —— 否则每个文件都是 `FileId(0)`，Ruling 23-4）
        let r = parse_file_in(file_table, src, name);
        for d in r.diags.into_sorted() {
            bag.push(d);
        }
        // 建不出文件（缺 `package` 子句 / 子句有错）⇒ 官方 `return nil`，没有可装载的东西
        let Some(ast) = r.file else { continue };

        let fid = ast.pos.file;
        // ② 包名一致性（官方 `initFiles` 的 switch：**看 pkg.name 是否为空**）
        match pkg.name.as_str() {
            // `case "":` —— 第一个有名字的文件定调；`name != "_"` 才定
            "" => {
                if ast.pkg_name.value != "_" {
                    pkg.name = ast.pkg_name.value.clone();
                } else {
                    // 官方 `check.go:299`：`check.error(file.Name, BlankPkgName, "invalid package name _")`
                    // ——位置取**包名标识符**处（与 `MismatchedPkgName` 取 `package` 关键字处不同）。
                    // 官方此处 `fallthrough`：`_` 包名的文件**照收**（本函数一致）。
                    // 触发面也同样受 `pkg.name` 是否为空约束 ⇒ 第一个有名字的文件之后
                    // 再出现 `package _` 落进下面的 `default`（`MismatchedPkgName`）。
                    bag.push(Diag {
                        pos: ast.pkg_name.pos,
                        code: DiagCode::TypBlankPkgName,
                        message: "invalid package name _".to_owned(),
                    });
                }
                pkg.files.push(fid);
            }
            // `case name:` —— 同名 ⇒ 收
            n if ast.pkg_name.value == n => {
                pkg.files.push(fid);
            }
            // `default:` —— 不一致 ⇒ 报告 + **整个忽略这个文件**
            n => {
                bag.push(Diag {
                    pos: ast.pos,
                    code: DiagCode::TypMismatchedPkgName,
                    message: format!("package {}; expected package {n}", ast.pkg_name.value),
                });
                continue;
            }
        }

        // ③ 该文件的**文件作用域**（`import` 名只在这里可见）
        let fscope = pkg.scopes.child(ScopeKind::File, pkg_scope);
        pkg.file_scopes.push((fid, fscope));
        for decl in &ast.decls {
            if let Decl::Import(imp) = decl
                && let Some(n) = import_name(imp)
            {
                let pos = imp.name.as_ref().map(|n| n.pos).unwrap_or(imp.pos);
                // 同名 import 在**同一个文件**里重复 ⇒ 官方 `resolver.go:323`
                // 走的是同一族 `%s redeclared in this block`（代号 `DuplicateDecl`）
                crate::scope::declare_name(
                    &mut pkg.scopes,
                    fscope,
                    ObjKind::PkgName,
                    &n,
                    pos,
                    None,
                    &mut bag,
                );
            }
        }

        // ④ 顶层声明名进**包作用域**（占位：类型与值留给 Task 24）
        for decl in &ast.decls {
            collect_top_level_names(&mut pkg.scopes, pkg_scope, decl, &pkg.name, &mut bag);
        }

        pkg.asts.push(ast);
    }

    pkg.file_table = file_table.clone();

    // ⑤ 版本表（`D-M3-15`）：每个文件的生效版本 + 两处 `TooNew` 诊断
    let refs: Vec<&AstFile> = pkg.asts.iter().collect();
    let (versions, vbag) = pkg_versions(lang, &refs);
    for d in vbag.into_sorted() {
        bag.push(d);
    }
    pkg.versions = versions;

    (pkg, bag)
}

/// **阶段 B**：声明求值（⑥）+ 体内作用域与名字引用（⑦）—— M4/Task 35。
///
/// 为什么把它从 [`load_sources_into`] 里拆出来：跨包限定名（`lib.T`）要能在
/// **类型表达式**里解析，而那要求 `pkg.imports` 在求值之前就已填好。单包入口
/// （[`load_sources_into`]）的 `imports` 恒空 ⇒ 拆开对它是**零语义变化**；
/// 会话入口（[`crate::importer::load_with_imports`]）则按
/// 「阶段 A → 解析 import（递归）→ **缝 import 名** → 阶段 B」四步走
/// —— 这正是 M3 `importer` 模块文档第 4 条记的那个缺口（「先装载依赖、再求值本包」）。
///
/// 调用方**必须**先调 [`crate::importer::stitch_imports`]（若有 import）：
/// 点导入把被依赖包的导出名注入**文件作用域**，而 ⑥ 的 `lookup_type` 要沿
/// 作用域链找名字 —— 缝晚了就找不到。
pub fn evaluate_package(tt: &mut TypeTable, pkg: &mut Package, bag: &mut DiagBag) {
    // ⑥ 声明收集与求值（Task 24）：类型表达式、常量、dump 根。
    //    放在版本之后是**有意的**：解析类型时要能问「这个文件允许哪些特性」
    //    （M3 还没有门控点，但顺序先按将来正确的形状摆）。
    collect_decls(pkg, tt, bag);

    // ⑦ 体内作用域与名字引用（Task 25）。放在最后：它要**读**包级对象的类型
    //    （`lookup_type` 只在对象已求值时才给得出东西），而包级求值在 ⑥ 才完成。
    crate::scope::resolve_bodies(pkg, tt, bag);

    // ⑧ 实例的方法集与原型对齐（`M12/Task 18`）：声明按序求值 ⇒ 早建的实例抄到
    //    的是**当时**的原型方法表（可能还是空的，方法在后面的行才挂上）。
    //    详见 `rgoc_types::instantiate::refresh_instance_methods`。
    rgoc_types::instantiate::refresh_instance_methods(tt);
}

/// 顶层声明的**名字**（不含 import —— 它们进文件作用域）。
///
/// 与官方 `collectObjects` 的分工一致：`func` **没有接收者**才进包作用域；
/// 方法名属于接收者的方法集（M3 还不做方法集 ⇒ 方法名此处**不声明**，
/// 由 M4 的 `TYP-*` 补齐 —— 记在 `open_gaps`）。
fn collect_top_level_names(
    scopes: &mut ScopeTable,
    scope: ScopeId,
    decl: &Decl,
    pkg_name: &str,
    bag: &mut DiagBag,
) {
    // `init` / `main` 的**特殊地位**（M4/Task 34，官方 `go/types/resolver.go`）：
    //
    // * `init` **永远不进包作用域**（`resolver.go:437`：`don't declare init
    //   functions in the package scope - they are invisible`）⇒ 同包多个 `init`
    //   合法（不进作用域就不会「重名」），`init` 也不能被引用；
    // * 非函数的 `init` / `main`（`package main` 里）⇒ 官方 `declarePkgObj`
    //   直接 `return`（`resolver.go:111/118`）：**既不进作用域，也不报重名**；
    //   诊断归检查器的 `main.rs`（`cannot declare … - must be func`）。
    //
    // 本层只负责「不声明」；消息与报点全部由 `rgoc-check::main` 统一出
    // —— 否则装载面与检查面会各报一条（C8 保留重复行）。
    let reserved = |name: &str| name == "init" || (name == "main" && pkg_name == "main");
    match decl {
        Decl::Import(_) => {}
        Decl::Type(d) => {
            if reserved(&d.name.value) {
                return;
            }
            declare(
                scopes,
                scope,
                ObjKind::TypeName,
                &d.name.value,
                d.name.pos,
                bag,
            );
        }
        Decl::Const(d) => {
            for n in &d.names {
                if reserved(&n.value) {
                    continue;
                }
                declare(scopes, scope, ObjKind::Const, &n.value, n.pos, bag);
            }
        }
        Decl::Var(d) => {
            for n in &d.names {
                if reserved(&n.value) {
                    continue;
                }
                declare(scopes, scope, ObjKind::Var, &n.value, n.pos, bag);
            }
        }
        Decl::Func(d) => {
            // ⚠️ **函数**只避 `init`：`func main` 官方照常进包作用域
            // （`resolver.go:443`）⇒ 第二个 `func main` 要报「重名」。
            // `main` 的 `reserved` 只对**非函数**声明生效（`declarePkgObj`）。
            if d.recv.is_none() && d.name.value != "init" {
                declare(scopes, scope, ObjKind::Func, &d.name.value, d.name.pos, bag);
            }
        }
    }
}

/// 声明一个包级名字（占位对象）。
///
/// 两条纪律：
///
/// - **`_` 永不绑定**（Go 规范：空标识符不引入绑定）⇒ 不进作用域；
/// - **同层重名：先声明者胜**（`lookup_local` 先查，后来的那个被丢而不是顶掉先来的
///   —— 顶掉会让「谁胜」依赖声明顺序，而「谁胜」在 Go 里是有定义的）。
///
/// Task 23 在这里留下「Task 25 在此挂 `DuplicateDecl`」，Task 25 就挂上了：
/// 现在重名会出一条 [`rgoc_lex::DiagCode::TypDuplicateDecl`]（消息按作用域层
/// 分措辞，见 [`crate::scope::declare_name`]）。
fn declare(
    scopes: &mut ScopeTable,
    scope: ScopeId,
    kind: ObjKind,
    name: &str,
    pos: Pos,
    bag: &mut DiagBag,
) {
    crate::scope::declare_name(scopes, scope, kind, name, pos, None, bag);
}

/// `import` 引入了哪个名字（`None` ⇒ 本任务不引入）。
///
/// | 写法 | 引入 | 说明 |
/// |---|---|---|
/// | `import f "fmt"` | `f` | 显式别名 |
/// | `import "fmt"` | `fmt` | ⚠️ **临时**取路径最后一段 —— 真正的包名来自被导入包的 `package` 子句（Task 27 解析 import 时才拿得到；M4/Task 35 的 `stitch_imports` 负责校正，见 `Ruling 35-2`） |
/// | `import _ "fmt"` | 不引入 | 空标识符 |
/// | `import . "fmt"` | 不引入 | 点导入把**名字**注入本文件（`stitch_imports`），不是引入一个包名 |
fn import_name(imp: &ImportDecl) -> Option<String> {
    if let Some(n) = &imp.name {
        return match n.value.as_str() {
            "_" | "." => None,
            other => Some(other.to_string()),
        };
    }
    imp_path(imp).map(|p| default_import_name(&p))
}

/// `import` 的路径文本（**只剥引号**，不做转义解码）。
///
/// 与 [`crate::importer`] 的 `import_path` 同口径 —— 两处都从同一份 AST 取，
/// 差异只在于本函数不关心 `Pos`。
pub(crate) fn imp_path(imp: &ImportDecl) -> Option<String> {
    let lit = imp.path.as_ref()?;
    let trimmed = lit.value.trim_matches(|c| c == '"' || c == '`');
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}

/// 无别名的 `import` 的**临时**名字：路径最后一段（空 ⇒ 整个路径）。
pub(crate) fn default_import_name(path: &str) -> String {
    let last = path.rsplit('/').next().unwrap_or(path);
    if last.is_empty() {
        path.to_string()
    } else {
        last.to_string()
    }
}
