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

use std::fs;
use std::path::PathBuf;

use rgoc_ast::{AstFile, Decl, ImportDecl};
use rgoc_lex::{Diag, DiagBag, DiagCode, FileId, FileTable, Pos};
use rgoc_parse::parse_file_in;
use rgoc_types::{
    ObjKind, Object, ScopeId, ScopeKind, ScopeRef, ScopeTable, TypeId, TypeTable, Versions,
    declare_universe, pkg_versions,
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
#[derive(Debug)]
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
    let mut sources: Vec<(String, Vec<u8>)> = Vec::with_capacity(files.len());
    let mut skipped: Vec<(String, String)> = Vec::new();
    for p in files {
        let name = p.to_string_lossy().into_owned();
        match fs::read(p) {
            Ok(bytes) => sources.push((name, bytes)),
            // 读不到 ⇒ 记下来（`Package::skipped`），不猜也不伪造诊断
            Err(e) => skipped.push((name, e.to_string())),
        }
    }
    let borrowed: Vec<Source<'_>> = sources
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    let (mut pkg, bag) = load_sources(tt, &borrowed, lang);
    pkg.skipped = skipped;
    (pkg, bag)
}

/// 从内存里的源文件装载一个包。
///
/// `sources` 的顺序就是 `Package::files` 的顺序（**确定性**从这里开始：
/// 包名取第一个、dump 的根按声明序、诊断排序的第一键是 `FileId`）。
pub fn load_sources(
    tt: &mut TypeTable,
    sources: &[Source<'_>],
    lang: Option<&str>,
) -> (Package, DiagBag) {
    let mut bag = DiagBag::new();
    let mut file_table = FileTable::new();
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
    };

    for (name, src) in sources {
        // ① 解析（**共享文件表** —— 否则每个文件都是 `FileId(0)`，Ruling 23-4）
        let r = parse_file_in(&mut file_table, src, name);
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
            collect_top_level_names(&mut pkg.scopes, pkg_scope, decl, &mut bag);
        }

        pkg.asts.push(ast);
    }

    pkg.file_table = file_table;

    // ⑤ 版本表（`D-M3-15`）：每个文件的生效版本 + 两处 `TooNew` 诊断
    let refs: Vec<&AstFile> = pkg.asts.iter().collect();
    let (versions, vbag) = pkg_versions(lang, &refs);
    for d in vbag.into_sorted() {
        bag.push(d);
    }
    pkg.versions = versions;

    // ⑥ 声明收集与求值（Task 24）：类型表达式、常量、dump 根。
    //    放在版本之后是**有意的**：解析类型时要能问「这个文件允许哪些特性」
    //    （M3 还没有门控点，但顺序先按将来正确的形状摆）。
    collect_decls(&mut pkg, tt, &mut bag);

    // ⑦ 体内作用域与名字引用（Task 25）。放在最后：它要**读**包级对象的类型
    //    （`lookup_type` 只在对象已求值时才给得出东西），而包级求值在 ⑥ 才完成。
    crate::scope::resolve_bodies(&mut pkg, tt, &mut bag);

    (pkg, bag)
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
    bag: &mut DiagBag,
) {
    match decl {
        Decl::Import(_) => {}
        Decl::Type(d) => declare(
            scopes,
            scope,
            ObjKind::TypeName,
            &d.name.value,
            d.name.pos,
            bag,
        ),
        Decl::Const(d) => {
            for n in &d.names {
                declare(scopes, scope, ObjKind::Const, &n.value, n.pos, bag);
            }
        }
        Decl::Var(d) => {
            for n in &d.names {
                declare(scopes, scope, ObjKind::Var, &n.value, n.pos, bag);
            }
        }
        Decl::Func(d) => {
            if d.recv.is_none() {
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
/// | `import "fmt"` | `fmt` | ⚠️ **临时**取路径最后一段 —— 真正的包名来自被导入包的 `package` 子句（Task 27 解析 import 时才拿得到） |
/// | `import _ "fmt"` | 不引入 | 空标识符 |
/// | `import . "fmt"` | 不引入 | 点导入把**名字**注入本文件，不是引入一个包名（Task 27） |
fn import_name(imp: &ImportDecl) -> Option<String> {
    if let Some(n) = &imp.name {
        return match n.value.as_str() {
            "_" | "." => None,
            other => Some(other.to_string()),
        };
    }
    let path = imp.path.as_ref()?;
    let trimmed = path.value.trim_matches(|c| c == '"' || c == '`');
    // 版本后缀（`example.com/m/v2`）与末尾空段都不该当成包名，但**本任务不猜**：
    // 只取最后一段，`/v2` 这类情形留给 Task 27（真正的包名来自被导入包）
    let last = trimmed.rsplit('/').next().unwrap_or(trimmed);
    if last.is_empty() {
        None
    } else {
        Some(last.to_string())
    }
}
