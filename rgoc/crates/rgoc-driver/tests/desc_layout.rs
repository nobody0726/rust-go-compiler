//! 元数据对撞：69 个语料的**每个类型**，`.s` 里的 `TypeDesc` ↔ Rust 侧算的
//! `layout` / `ptr_mask`（M9/Task 9，`T-META-02/03/06`）。
//!
//! # 这条判据抓什么
//!
//! 单条 fixture 能验「`struct{a int; b string}` 的 desc 是 24 字节」；它验不了
//! 「**所有**出现在语料里的类型都对」—— 而 desc 错了不会有任何编译错，症状是
//! M10 的扫描器随机崩（`RM-2`）。所以这里跑全语料，逐个类型比。
//!
//! # 两侧是谁
//!
//! * 一侧是**发射出来的字节**（`.s` 的 `.rodata`，40 字节头 + 三个 `.quad`）
//! * 另一侧是**独立算一遍**：`layout()` 给 size、`ptr_mask()` 给位图、
//!   `needs_zero()`/`comparable()` 给 flags
//!
//! 两侧用的是同一张 `TypeTable`（`build_frontend` 交出来的那张），所以比的不是
//! 「两个不同的类型表是否一致」，而是「**编码有没有失真**」：位序、字段偏移、
//! 截断、`ptrdata` 的口径。这类错在单条 fixture 上很容易被「刚好对」蒙过去。
//!
//! # 不覆盖什么
//!
//! `ptr_mask` 本身对不对（那是 `ptr_mask.rs` 的活，靠设计稿的五个特例钉）。
//! 这里假设它是对的，然后问「发出去的东西是不是它」。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rgoc_arm64::module::{gcdata_bytes, ptrdata_of, type_closure};
use rgoc_driver::build_cmd::{self, FrontendError};
use rgoc_meta::desc::{DescBuilder, type_hash};
use rgoc_ssa::opt::Config;
use rgoc_types::layout::{layout, ptr_mask};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("仓库根不可达")
}

fn selected() -> Vec<String> {
    let text = std::fs::read_to_string(repo_root().join("tools/run-oracle/run-selected.txt"))
        .expect("读不到 run-selected.txt");
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(ToString::to_string)
        .collect()
}

/// `.s` 里一个 desc 的原始字节与三个指针。
struct Raw {
    header: Vec<u8>,
    gcdata: Option<Vec<u8>>,
    elem: Option<String>,
}

fn parse(asm: &str) -> BTreeMap<String, Raw> {
    let lines: Vec<&str> = asm.lines().map(str::trim).collect();
    // 位图
    let mut bitmaps: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for i in 0..lines.len().saturating_sub(1) {
        let Some(label) = lines[i].strip_suffix(':') else {
            continue;
        };
        if !label.starts_with(".L") {
            continue;
        }
        if let Some(rest) = lines[i + 1].strip_prefix(".byte ") {
            bitmaps.insert(
                label.to_string(),
                rest.split(',')
                    .map(|s| s.trim().parse().expect("字节"))
                    .collect(),
            );
        }
    }
    let mut out = BTreeMap::new();
    for i in 0..lines.len().saturating_sub(4) {
        let Some(sym) = lines[i].strip_suffix(':') else {
            continue;
        };
        if !sym.starts_with("__desc_") {
            continue;
        }
        let Some(rest) = lines[i + 1].strip_prefix(".byte ") else {
            continue;
        };
        let header: Vec<u8> = rest
            .split(',')
            .map(|s| s.trim().parse().expect("字节"))
            .collect();
        let quad = |k: usize| -> Option<String> {
            let v = lines[i + k].strip_prefix(".quad ")?.trim();
            if v == "0" { None } else { Some(v.to_string()) }
        };
        let gcdata = quad(2).map(|l| {
            bitmaps
                .get(&l)
                .unwrap_or_else(|| panic!("{sym} 的 gcdata 指向不存在的 {l}"))
                .clone()
        });
        out.insert(
            sym.to_string(),
            Raw {
                header,
                gcdata,
                elem: quad(3),
            },
        );
    }
    out
}

#[test]
fn 语料里每个类型的_desc_都与_layout_和_ptr_mask_一致() {
    let files = selected();
    assert_eq!(files.len(), 69, "入选语料是 69 个（M6 冻结）");

    let mut checked_types = 0usize;
    let mut checked_files = 0usize;
    let mut skipped: Vec<(String, String)> = Vec::new();
    let mut problems: Vec<String> = Vec::new();

    for rel in &files {
        let path = repo_root().join(rel);
        let fe = match build_cmd::build_frontend(std::slice::from_ref(&path), Config::all(), None) {
            Ok(f) => f,
            Err(FrontendError::Rejected(_)) => {
                // SSA 建不出图（M6 的 23 个）—— 不在本判据的分母里。
                skipped.push((rel.clone(), "SSA/HIR 拒绝".to_string()));
                continue;
            }
            Err(FrontendError::Internal(e)) => panic!("{rel}: 内部失败 {e}"),
        };
        let asm = match rgoc_arm64::module::compile(&fe.module, &fe.types) {
            Ok(a) => a,
            Err(rgoc_arm64::module::CompileError::Cg(c)) => {
                skipped.push((rel.clone(), format!("codegen 拒绝 {}", c.as_str())));
                continue;
            }
            Err(e) => panic!("{rel}: 帧布局失败 {e}"),
        };
        let descs = parse(&asm);
        checked_files += 1;

        for (hash, ty) in type_closure(&fe.module, &fe.types) {
            checked_types += 1;
            let sym = rgoc_arm64::module::desc_symbol(hash);
            let Some(raw) = descs.get(&sym) else {
                problems.push(format!("{rel}: 闭包里的类型没有 desc（{sym}）"));
                continue;
            };
            let Some(l) = layout(&fe.types, ty) else {
                problems.push(format!("{rel}: 闭包里的类型没有布局（{sym}）"));
                continue;
            };
            let mask = ptr_mask(&fe.types, ty).unwrap_or_default();
            let d = DescBuilder::new(&fe.types, ty)
                .expect("有布局就有 desc")
                .build();

            let got_size =
                u32::from_le_bytes([raw.header[0], raw.header[1], raw.header[2], raw.header[3]]);
            let got_ptrdata =
                u32::from_le_bytes([raw.header[4], raw.header[5], raw.header[6], raw.header[7]]);
            let got_hash =
                u32::from_le_bytes([raw.header[8], raw.header[9], raw.header[10], raw.header[11]]);
            if got_size != l.size {
                problems.push(format!(
                    "{rel}: {sym} 的 size：产物 {} ≠ layout {}",
                    got_size, l.size
                ));
            }
            if got_ptrdata != ptrdata_of(&mask) {
                problems.push(format!(
                    "{rel}: {sym} 的 ptrdata：产物 {} ≠ ptr_mask {}",
                    got_ptrdata,
                    ptrdata_of(&mask)
                ));
            }
            if got_hash != type_hash(&fe.types, ty) {
                problems.push(format!(
                    "{rel}: {sym} 的 hash 字段与符号名不一致（{} ≠ {}）",
                    got_hash, hash
                ));
            }
            if raw.header[12] != d.kind {
                problems.push(format!(
                    "{rel}: {sym} 的 kind：产物 {} ≠ {}",
                    raw.header[12], d.kind
                ));
            }
            if raw.header[13] != d.flags {
                problems.push(format!(
                    "{rel}: {sym} 的 flags：产物 {} ≠ {}",
                    raw.header[13], d.flags
                ));
            }
            let want_gc = if mask.iter().any(|b| *b) {
                Some(gcdata_bytes(&mask))
            } else {
                None
            };
            if raw.gcdata != want_gc {
                problems.push(format!(
                    "{rel}: {sym} 的 gcdata：产物 {:?} ≠ ptr_mask {:?}",
                    raw.gcdata, want_gc
                ));
            }
            // `elem` 必须与 `elem_of` 的口径一致（指针/切片/数组才指）
            let want_elem = match fe.types.type_of(ty) {
                rgoc_types::typ::Type::Pointer(e)
                | rgoc_types::typ::Type::Slice(e)
                | rgoc_types::typ::Type::Array { elem: e, .. } => {
                    Some(rgoc_arm64::module::desc_symbol(type_hash(&fe.types, *e)))
                }
                _ => None,
            };
            if raw.elem != want_elem {
                problems.push(format!(
                    "{rel}: {sym} 的 elem：产物 {:?} ≠ 期望 {:?}",
                    raw.elem, want_elem
                ));
            }
            if problems.len() > 20 {
                break;
            }
        }
        if problems.len() > 20 {
            break;
        }
    }

    eprintln!(
        "desc_layout: {checked_files} 个文件 / {checked_types} 个类型全对；跳过 {} 个",
        skipped.len()
    );
    assert!(
        problems.is_empty(),
        "元数据对撞失败 {} 条（前 20 条）：\n{}",
        problems.len(),
        problems.join("\n")
    );
    // 分母守卫：跳过太多说明「本判据其实没跑」。39 是 M7 已支持的那批；
    // 177 个类型是实测（M9 的语料还只有 39 个文件能到 codegen）。
    assert!(
        checked_files >= 39,
        "只跑通了 {checked_files} 个文件（应 ≥ 39）；跳过：{skipped:?}"
    );
    assert!(
        checked_types >= 150,
        "只比了 {checked_types} 个类型 —— 太少，说明闭包算漏了"
    );
}
