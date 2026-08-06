//! 增量更新——客户端 bsdiff delta 应用（Phase 6.1）
//!
//! 服务端为每个新版本预生成 `old → new` 的 bsdiff delta；客户端下载 delta
//!（通常远小于全量 bundle），应用到本地 old bundle 重建 new bundle，再用
//! SHA256 校验完整性。校验失败绝不落盘——避免半截/被篡改的 bundle 替换掉
//! 可用的旧版本。
//!
//! ## 为什么 bsdiff
//! - 纯 Rust（`bsdiff` crate，safe Rust，无 C 依赖），跨平台一致
//! - 对「相邻版本高度相似」的 bundle（大多数发版只改一小部分）压缩比极高
//! - 算法成熟，space-wizards 维护的 Rust 移植
//!
//! ## 安全
//! - delta 应用是纯计算，无网络/文件副作用，沙箱友好
//! - 校验在内存中完成，通过后才由调用方原子写盘
//! - `apply_delta` 不触碰磁盘，调用方控制写盘时机与原子性
//!
//! 本模块只做「应用 + 校验」，不涉及下载/解压/签名——那些由 updater 上层
//! 组合（下载 delta → apply_delta → 签名校验 → 原子替换）。

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};

/// 应用 bsdiff delta：`old` + `patch` → `new`，并用 SHA256 校验。
///
/// `patch` 以 `&[u8]` 传入，内部转 `&mut &[u8]`（bsdiff 的游标式读取约定）。
/// 校验失败返回 `Err`，绝不返回部分结果。
pub fn apply_delta(old: &[u8], patch: &[u8], expected_sha256: &[u8]) -> Result<Vec<u8>> {
    // 预分配：new 通常与 old 同量级，避免 patch 过程中多次扩容（0GC 倾向）
    let mut new = Vec::with_capacity(old.len() + patch.len());
    let mut patch_stream = patch;
    bsdiff::patch(old, &mut patch_stream, &mut new).context("应用 bsdiff delta 失败")?;

    let mut hasher = Sha256::new();
    hasher.update(&new);
    let actual = hasher.finalize();

    if actual.as_slice() != expected_sha256 {
        bail!(
            "delta 应用后 SHA256 校验失败：期望 {}，实际 {}（delta 可能损坏或被篡改）",
            hex(expected_sha256),
            hex(actual.as_slice())
        );
    }

    Ok(new)
}

/// 计算 SHA256（供上层在生成 delta 时约定哈希，或测试构造期望值）。
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

/// 生成 bsdiff delta（供测试构造 patch，或未来内置的「本地 delta 调试」工具）。
#[cfg(test)]
pub(crate) fn make_delta(old: &[u8], new: &[u8]) -> Vec<u8> {
    let mut patch = Vec::new();
    bsdiff::diff(old, new, &mut patch).expect("bsdiff::diff 不应失败");
    patch
}

fn hex(b: &[u8]) -> String {
    // 手写 hex 编码，避免为 32 字节哈希引入额外 crate
    let mut s = String::with_capacity(b.len() * 2);
    for byte in b {
        s.push_str(&format!("{:02x}", byte));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_delta_reconstructs_new() {
        let old = b"hello world, this is the original bundle content".to_vec();
        let new = b"hello rust, this is the updated bundle content!!!".to_vec();
        let patch = make_delta(&old, &new);
        let expected = sha256(&new);

        let applied = apply_delta(&old, &patch, &expected).expect("delta 应用应成功");
        assert_eq!(applied, new);
    }

    #[test]
    fn test_apply_delta_rejects_wrong_hash() {
        // delta 正确，但 expected_sha256 故意写错 → 必须拒绝（防篡改/防损坏）
        let old = b"original bundle".to_vec();
        let new = b"updated bundle".to_vec();
        let patch = make_delta(&old, &new);
        let wrong_hash = [0u8; 32];

        let result = apply_delta(&old, &patch, &wrong_hash);
        assert!(result.is_err(), "哈希不匹配必须返回错误");
        assert!(
            result.unwrap_err().to_string().contains("SHA256"),
            "错误信息应说明是 SHA256 校验失败"
        );
    }

    #[test]
    fn test_apply_delta_realistic_bundle_roundtrip() {
        // 真实 engine bundle 形态：长重复文本块 + 局部小改动，验证 roundtrip + 校验。
        // （delta 是否更小取决于 bsdiff 算法 + 是否配合压缩，非 apply_delta 的契约，
        //   故此处只验证「正确还原 + 哈希校验通过」。）
        let chunk = b"inkos engine bundle: node runtime + modules + assets. ";
        let old: Vec<u8> = chunk.repeat(250); // ~22KB
        let mut new = old.clone();
        new[1000..1011].copy_from_slice(b"v2-patched!");

        let patch = make_delta(&old, &new);
        let applied = apply_delta(&old, &patch, &sha256(&new)).unwrap();
        assert_eq!(applied, new);
    }

    #[test]
    fn test_apply_delta_empty_old() {
        // 边界：old 为空（首装/全量场景，delta 退化为压缩 new）
        let old: Vec<u8> = vec![];
        let new = b"brand new bundle".to_vec();
        let patch = make_delta(&old, &new);

        let applied = apply_delta(&old, &patch, &sha256(&new)).unwrap();
        assert_eq!(applied, new);
    }

    #[test]
    fn test_apply_delta_identical_old_new() {
        // 边界：old == new（无变化的发版，理论上不该出现，但要稳健）
        let data = b"unchanged bundle content".to_vec();
        let patch = make_delta(&data, &data);

        let applied = apply_delta(&data, &patch, &sha256(&data)).unwrap();
        assert_eq!(applied, data);
    }
}
