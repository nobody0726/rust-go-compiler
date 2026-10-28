//! 实例化**闭包**（不动点）—— `M12/Task 14`。
//!
//! # 这一步在算什么
//!
//! 检查面在走表达式时会遇到 `F[int]` / `F(1)`（推断出 `F·int`），每次都是
//! 往 [`rgoc_load::mono::Mono`] 里塞一条**请求**。本模块把队列抽干：
//!
//! ```text
//! while let Some(req) = take_inst() {
//!     // 把 req 的类型参数换成实参，**再走一遍**那个函数的体
//!     //   ⇒ 体里的 `G[string]` 又入队 ⇒ 下一轮
//!     accept_inst(req);
//! }
//! ```
//!
//! 队列空 = 不动点。终止性由 `Mono` 的 memo 保证（同一 `(origin, 按
//! `identical` 等价的实参)` 只入队一次）；[`MAX_INSTANCES`] 是兜底 ——
//! memo 坏了就是「条数不对」而不是挂住测试。
//!
//! # 为什么**复用同一个会话**再检查一遍体
//!
//! 官方把「实例化」当成**换一组类型参数再检查一次**（`Checker.instance` →
//! `check.instType`）；替换必须走同一套求值器，否则 `T` 的含义在两处会分叉
//! （`D-M5-2` 的同一条理由）。这里把实参声明进 `Env`，于是体里任何
//! `type_expr(T)` 都自然拿到实参 —— 不需要另造一份 `SubstMap` 的替换路径。
//!
//! # ⚠️ 实例体复查期的上报**一律回滚**
//!
//! 泛型体已经在包级检查（`stmt::check_package_bodies`）走过一遍，那一遍用
//! `T`（类型参数本身）当它的类型。实例体是**第二遍**：同一份 AST、同一个
//! 会话，任何真正与源码有关的错误都会**再报一次** ⇒ 语料里每个泛型体有错的
//! 文件都会多出一份重复行。
//!
//! 实例化**独有**的错误面（「实参不满足约束」`TYP-*`）本 Task **不建模** ——
//! 那是 `M12` 后面的账（记在 `progress.md` 的 open 里）。所以这一遍只
//! **产请求、建实例**，不产诊断。

use rgoc_ast::{Decl, FuncDecl};
use rgoc_lex::FileId;
use rgoc_load::Package;
use rgoc_load::mono::{FuncKey, InstRequest};
use rgoc_types::TypeId;

use crate::check::Session;

/// 实例条数上限（兜底，不是语义边界）。
///
/// 正常的 `memo` 保证有限；一旦它失效，队列就永不空。超限时报
/// [`rgoc_lex::DiagCode::TypInvalidInstanceCycle`] 并停下 —— 「实例化没收敛」
/// 与官方那个码的语义最近，而且**不许静默截断**（截断的产物会缺函数，
/// 症状要到链接期才出现）。
pub const MAX_INSTANCES: usize = 10_000;

/// 实例化链的**深度**上限（`M12/Task 14` 订正的主要判据）。
///
/// 官方把「实例化环」判在**嵌套深度**上，我们也一样 —— 见
/// [`rgoc_load::mono::InstRequest::depth`] 里那段实测：只按总条数封顶时，
/// `issue48711.go`（每层实参都不同的环）要跑满一万条、每条再检查一遍体，
/// 容器直接 OOM（exit 137）。
///
/// 100 是宽松值：真实的实例化链（`A` 调 `B` 调 `C`…）个位数就够，一百层
/// 只可能是环。
pub const MAX_INST_DEPTH: usize = 100;

/// 把队列抽干（不动点）。
pub(crate) fn check_instances(s: &mut Session<'_>) {
    let mut n = 0usize;
    while let Some(req) = s.take_inst() {
        // ① 深度封顶（环检测）。**在处理之前**判 —— 处理一条就要再检查一遍体，
        //    而环的第 101 层和第 100 层一样没有信息量。
        if req.depth > MAX_INST_DEPTH {
            report_cycle(s, &req, "instantiation cycle: instantiation depth exceeds");
            break;
        }
        n += 1;
        if n > MAX_INSTANCES {
            report_cycle(s, &req, "instantiation did not converge: more than");
            break;
        }
        // ② 摆深度：这一轮的体里发出的请求都是 `depth + 1`。
        s.set_inst_depth(req.depth);
        if let Some((file, f)) = func_of(s.pkg, req.origin) {
            let subst = tparam_subst(s, f, &req.args);
            // 实例体复查：诊断回滚（见模块文档）。
            let mark = s.diag_len();
            let any = s.reported_any();
            crate::stmt::check_instance_body(s, file, f, &subst);
            s.rollback_diags(mark, any);
        }
        s.accept_inst(req);
    }
    // 回到包级深度：后面的诊断/续跑 lowering 不该继承某一层的深度。
    s.set_inst_depth(0);
}

/// 报「实例化没收敛」。
///
/// ⚠️ 消息里**只写 origin 的名字**，不写 `req.name`：环上的实参每层都在长
/// （`[]int` → `[][]int` → …），第 100 层的实例名有几千个字符。实测那样打出
/// 来是一屏乱码，而读者要的只是「哪个函数在环上」。
fn report_cycle(s: &mut Session<'_>, req: &InstRequest, why: &str) {
    let (pos, name) = func_of(s.pkg, req.origin).map_or_else(
        || {
            (
                rgoc_lex::Pos {
                    base: None,
                    file: FileId::from_index(0),
                    line: 0,
                    col: 0,
                },
                String::new(),
            )
        },
        |(_, f)| (f.pos, f.name.value.clone()),
    );
    s.report_hard(rgoc_lex::Diag {
        pos,
        code: rgoc_lex::DiagCode::TypInvalidInstanceCycle,
        message: format!("{why} in {name}"),
    });
}

/// `FuncKey` → 那个 `FuncDecl` 与它所在的文件。
///
/// 走访序必须与 `Session::new` 里登记 `FuncKey` 的循环**逐字相同**
/// （`pkg.asts` 序 → `decls` 序 → 只数 `Decl::Func`）。两处不一致的直接症状是
/// 「实例体用的是别的函数」—— 而这种错一条诊断都不会有。
fn func_of(pkg: &Package, key: FuncKey) -> Option<(FileId, &FuncDecl)> {
    let mut k = 0usize;
    for (i, ast) in pkg.asts.iter().enumerate() {
        for d in &ast.decls {
            if let Decl::Func(f) = d {
                if k == key {
                    return Some((pkg.files[i], f));
                }
                k += 1;
            }
        }
    }
    None
}

/// 类型参数名 → 实参。
///
/// 两个来源按**声明序**配对：名字从 AST 的 `tparams`（parser 把 `[T, U any]`
/// 拆成两个 `Field`，所以是逐名字的），id 从 `Package::generic_funcs`
/// （Task 6 建的表），实参从请求。
///
/// 三者长度不必相等（装载面拆不出 / 实参个数已被别处判过），`zip` 自然截断
/// —— 造不出错的绑定。
fn tparam_subst(s: &Session<'_>, f: &FuncDecl, args: &[TypeId]) -> Vec<(String, TypeId)> {
    let Some(ids) = s.pkg.generic_funcs.get(&f.name.value) else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(args.len());
    for ((field, _id), &arg) in f.tparams.iter().zip(ids).zip(args) {
        if let Some(name) = &field.name {
            out.push((name.value.clone(), arg));
        }
    }
    out
}

/// 把一个请求的类型参数绑定抽出来（`lowering` 与诊断共用）。
#[must_use]
pub fn subst_of(s: &Session<'_>, req: &InstRequest) -> Vec<(String, TypeId)> {
    match func_of(s.pkg, req.origin) {
        Some((_, f)) => tparam_subst(s, f, &req.args),
        None => Vec::new(),
    }
}
