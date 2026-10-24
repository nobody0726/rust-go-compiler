//! 模块级发射：`_start`、包级全局、逐函数指令选择（`M7-plan.md` Task 49 的骨架）。
//!
//! # 顺序是判据的一部分
//!
//! `_start` 里的 `bl` 序列**必须**是 `init` → `main`（`Program::init_order` 在 M5 就定了，
//! SSA 只把 `inits` 合成一个 `Module::init`，`R-32-1`）。这里不排序、不重排 ——
//! 按模块给的 `main` / `init` 两个 `FuncId` 直接发射。重排就等于把「初始化顺序」
//! 这个语义从 M5 搬到 M7，而 M7 没有判据能验它。
//!
//! # 全局变量
//!
//! 每个 `GlobalId` 一个 `.data` 标签 `g<id>`，大小按 `global_types` 的布局给、
//! 内容全零（Go 的包级变量零值语义）。零大小全局只发标签不发字节。

use rgoc_ssa::ir::Module;
use rgoc_types::TypeTable;
use rgoc_types::layout::layout;

use crate::diag::CgCode;
use crate::emit::Asm;
use crate::frame::{self, FrameError};
use crate::isel;

/// 编译失败。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileError {
    /// M7 明确不支持的面。
    Cg(CgCode),
    /// 某个值的类型没有运行期布局。
    Frame(FrameError),
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cg(c) => write!(f, "{c}"),
            Self::Frame(e) => write!(f, "{e}"),
        }
    }
}

/// 把一个 SSA 模块编成 `.s` 文本。
///
/// # Errors
///
/// 不支持的面（`[]T` / 间接调用）或没有运行期布局的类型。
pub fn compile(m: &Module, tt: &TypeTable) -> Result<String, CompileError> {
    let mut asm = Asm::new();

    // ① 包级全局（`.data`，全零）。
    for (i, g) in m.globals.iter().enumerate() {
        let ty = m.global_types.get(i).copied();
        let size = ty.and_then(|t| layout(tt, t)).map_or(8, |l| l.size);
        let zeros = vec![0u8; size as usize];
        asm.data_named(&format!("g{}", g.0), &zeros);
    }

    // ② 逐函数。
    for f in &m.funcs {
        let fr = frame::layout(f, tt).map_err(CompileError::Frame)?;
        isel::func(f, &fr, tt, &mut asm).map_err(CompileError::Cg)?;
    }

    // ③ 除零的统一出口。
    //
    // `isel` 在 `sdiv` 前发 `cbz x11, .Ldiv0`（`M7-design.md` §3.9 的检查点之一）。
    // 这个标签必须**定义一次**且全模块可见 —— 每条除法都跳到同一处，
    // 而不是每处内联一份 panic 文案（那会让 `.s` 随除法个数膨胀）。
    asm.label(".Ldiv0");
    asm.ins("bl rt_panic_div_zero");
    asm.ins("brk #1");

    // ④ `_start`：init → main → exit(0)。
    asm.global("_start");
    asm.label("_start");
    // `Module::init` 是**在 `funcs` 里的下标**（不是 `FuncId`）；`main` 是 `FuncId`。
    if let Some(f) = m.init.and_then(|idx| m.funcs.get(idx)) {
        asm.ins(&format!("bl fn{}", f.src.0));
    }
    if let Some(main) = m.main {
        asm.ins(&format!("bl fn{}", main.0));
    }
    asm.ins("mov x0, #0");
    asm.ins("bl rt_exit");

    Ok(asm.render())
}
