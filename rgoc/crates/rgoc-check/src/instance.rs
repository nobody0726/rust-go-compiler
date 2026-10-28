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

/// 把队列抽干（不动点），并把**动态分派**用到的方法实例补齐。
///
/// 两个阶段交替跑到稳定：`drain` 处理实例体（体里的新请求又入队），
/// `request_typed_methods` 补「没有调用点」的那一类（见它自己的说明）。
pub(crate) fn check_instances(s: &mut Session<'_>) {
    loop {
        drain(s);
        if !request_typed_methods(s) {
            break;
        }
    }
}

/// 抽干队列（一轮）。
fn drain(s: &mut Session<'_>) {
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
        // 同时摆**实例上下文**：体里的调用点落到的实例名与包级那一遍不同
        // （`F[T]` 在这遍里是 `F·int`，包级那遍是 `F·tpN`），lowering 靠它
        // 区分同一份 AST 的两遍（`Session::note_inst_site`）。
        s.set_inst_ctx(&req.name);
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
    s.set_inst_ctx("");
}

/// 给「只在**动态分派**里出现」的泛型方法补实例请求（`M12/Task 18`）。
///
/// # 为什么单开一条反推途径
///
/// 请求原本只从**调用点**发出（`expr.rs` 的 `request_inst`）。动态分派没有
/// 调用点：itab 的槽位由 ssa 按方法名解析，而 `MethodSet` 的原料是 HIR 里的
/// 函数体。方法实例没被请求过 ⇒ 没有体 ⇒ `resolve_method` 给哨兵
/// `FuncId(u32::MAX)` ⇒ 链接期 `undefined reference to fn4294967295`
/// （`issue48049` 的 `fooer2[T].Foo`、`issue52026` 的 `Some[T].sealedOption`）。
///
/// # 判据取「宽」
///
/// 每有一个具名类型实例，就请求它原型上**全部**泛型方法的同实参实例。比
/// 「只请求真的被分派到的那些」宽：代价是多发几个函数体。不放宽的话只有两条
/// 路，都不可行 ——
/// * 等 itab 成形再判：itab 在 **ssa** 才收集，那时检查会话已经结束；
/// * 推迟到 lowering 之后：lowering 正是**消费**实例的一方，反过来喂它就是环。
///
/// 方向与仓库一贯的取法一致：多造只是浪费体积，漏造是链接期缺符号。
///
/// 返回 `true` = 有新请求入队（外层要再抽一次）。
fn request_typed_methods(s: &mut Session<'_>) -> bool {
    // 先把待办收齐：`named_instances` 借着 `s.tt`，期间不能可变借用会话。
    let mut todo: std::collections::BTreeSet<(FuncKey, Vec<TypeId>)> =
        std::collections::BTreeSet::new();
    {
        let tt = &*s.tt;
        for (_inst, origin, targs) in tt.named_instances() {
            let def = tt.named_def(origin);
            // 非泛型原型没有形参表 ⇒ 它的方法不是实例，普通路径已经发过码。
            if def.type_params.is_empty() {
                continue;
            }
            // 实参里有**没有运行期布局**的类型（`invalid` 是错误恢复的产物，
            // `T` 是包级那一遍检查泛型体时留下的类型参数实例
            // `Some·tp0`）⇒ 跳过。给它们发方法体只会得到
            // `帧布局失败：函数 Some·tp0.Value：v0 的类型 t69 没有运行期布局`。
            // 真正的实例由**实例体那一遍**建出来（那里 `T` 已绑到实参）。
            if targs
                .iter()
                .any(|&a| rgoc_types::layout::layout(&*tt, a).is_none())
            {
                continue;
            }
            for (mname, _) in &def.methods {
                let key = (def.name.clone(), mname.clone());
                if let Some(&k) = s.method_keys.get(&key) {
                    todo.insert((k, targs.to_vec()));
                }
            }
        }
    }
    let mut any = false;
    for (k, args) in todo {
        if s.mono.request(&mut *s.tt, k, &args) {
            any = true;
        }
    }
    any
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
/// # 两档来源（`M12/Task 18` 补第二档）
///
/// **包级泛型函数**：名字从 AST 的 `tparams`（parser 把 `[T, U any]` 拆成两个
/// `Field`，所以是逐名字的），id 从 `Package::generic_funcs`（Task 6 建的表），
/// 实参从请求，三者按声明序配对。
///
/// **泛型方法**：`f.tparams` 是**空**的 —— 形参由**接收者**声明
/// （`func (b Box[T]) Get() T`）。所以名字按接收者的写法读（`Box[T]` ⇒ `T`），
/// id 从接收者**基类型**的定义读（`NamedDef::type_params`，声明序），实参从
/// 请求来。两边个数不必相等，`zip` 自然截断 —— 造不出错的绑定。
fn tparam_subst(s: &Session<'_>, f: &FuncDecl, args: &[TypeId]) -> Vec<(String, TypeId)> {
    // 方法**不**走 `generic_funcs`：那张表只收包级函数（`decls.rs` 的
    // `recv.is_none()` 判据），但同名的方法与函数会共用键 ⇒ 不加这一道
    // `recv.is_none()` 就会拿函数的形参表去绑方法的实参。
    if f.recv.is_none()
        && let Some(ids) = s.pkg.generic_funcs.get(&f.name.value)
    {
        let mut out = Vec::with_capacity(args.len());
        for ((field, _id), &arg) in f.tparams.iter().zip(ids).zip(args) {
            if let Some(name) = &field.name {
                out.push((name.value.clone(), arg));
            }
        }
        return out;
    }
    let Some((base, names)) = f.recv_base() else {
        return Vec::new();
    };
    let Some(nid) = named_in_pkg(s, &base) else {
        return Vec::new();
    };
    let ids = s.tt.named_def(nid).type_params.clone();
    names
        .into_iter()
        .zip(ids)
        .zip(args)
        .map(|((name, _id), &arg)| (name, arg))
        .collect()
}

/// 包作用域里的具名类型（`type T ...`）→ 它的原型条目。
fn named_in_pkg(s: &Session<'_>, name: &str) -> Option<rgoc_types::NamedId> {
    let t = s.pkg.lookup(name)?.typ?;
    let t = rgoc_types::unalias(s.tt, t);
    match s.tt.type_of(t) {
        rgoc_types::Type::Named(n) => Some(*n),
        _ => None,
    }
}

/// 把一个请求的类型参数绑定抽出来（`lowering` 与诊断共用）。
#[must_use]
pub fn subst_of(s: &Session<'_>, req: &InstRequest) -> Vec<(String, TypeId)> {
    match func_of(s.pkg, req.origin) {
        Some((_, f)) => tparam_subst(s, f, &req.args),
        None => Vec::new(),
    }
}

/// 这个实例是不是「**泛型原型自己**」—— 实参里有**没有运行期布局**的类型。
///
/// 两种来源，都是包级那一遍检查泛型体留下的：
/// * 类型参数本身（`issue50264` 的 `Some2·tp5`、`issue51909` 的
///   `Operation·struct.Body.tp2`、`t_gen_14b` 的 `Loop·tp0`）；
/// * 错误恢复的 `invalid`（`Some·invalid`，见 `typeexpr.rs` 的挡）。
///
/// # 为什么判据落在**发码口**而不是请求口
///
/// 这类实例**不能下沉**（`v T` 的形参没有布局，帧布局阶段直接炸），但它**必须
/// 入队**：`issue48711.go` 的实例化环只从类型参数实参上长出来 —— 挡在请求口
/// 连 `TYP-INVALID-INSTANCE-CYCLE` 都报不出来（`t_gen_14d` 实测）。两者的区别
/// 是「造不造函数」与「认不认这条请求」，所以判据只能放在造函数那一处
/// （`rgoc-hir/src/lower.rs` 的 ② 段）。
///
/// 少发是安全的：泛型原型本身不发码（`is_generic_decl`），所以**没有任何活代码**
/// 会引用这些实例 —— 非泛型体里的实参不可能是类型参数。
#[must_use]
pub fn is_abstract(s: &Session<'_>, req: &InstRequest) -> bool {
    req.args
        .iter()
        .any(|&a| rgoc_types::layout::layout(s.tt, a).is_none())
}
