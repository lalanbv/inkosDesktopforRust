//! Ed25519 签名验证（Phase 6.5）—— engine bundle 来源认证
//!
//! engine 通道（自定义 updater，不同于 shell 通道的 tauri-plugin-updater）
//! 当前用 SHA256 保证完整性（M3a），但 SHA256 只防意外损坏，不防主动篡改
//! （攻击者可同时替换 bundle 与 .sha256）。Ed25519 签名用发布方私钥签 bundle，
//! 客户端用内嵌公钥验证——即使下载通道被 MITM，无私钥就无法伪造合法签名。
//!
//! ## 与 shell 通道的关系
//! shell 通道（应用自身更新）已用 tauri-plugin-updater 的 Ed25519（pubkey 在
//! tauri.conf.json）。本模块给 engine bundle 提供对称能力，密钥可共用同一对
//! 或独立——由发布流程约定。
//!
//! ## 当前状态
//! 提供 `verify` 纯函数（可独立测试）。接入 engine::EngineChannel::apply 的
//! 下载-校验流程是后续步骤（需 release 附 .sig 资产 + 公钥内嵌约定）。

use anyhow::{bail, Context, Result};
use ed25519_dalek::{Signature, Signer, Verifier, VerifyingKey};

/// Ed25519 签名长度（字节）
pub const SIG_LEN: usize = 64;
/// Ed25519 公钥长度（字节）
pub const PUBKEY_LEN: usize = 32;

/// 验证 Ed25519 签名。
///
/// - `data`：被签名的原始数据（如 engine bundle 字节）
/// - `sig`：64 字节签名
/// - `pubkey`：32 字节公钥
///
/// 长度不符提前 bail（给出明确错误，而非底层数组转换 panic）。
/// 验证失败返回 Err，调用方应据此拒绝该 bundle。
pub fn verify(data: &[u8], sig: &[u8], pubkey: &[u8]) -> Result<()> {
    if pubkey.len() != PUBKEY_LEN {
        bail!("Ed25519 公钥长度必须 {PUBKEY_LEN} 字节，实际 {}", pubkey.len());
    }
    if sig.len() != SIG_LEN {
        bail!("Ed25519 签名长度必须 {SIG_LEN} 字节，实际 {}", sig.len());
    }

    let mut pk_arr = [0u8; PUBKEY_LEN];
    pk_arr.copy_from_slice(pubkey);
    let mut sig_arr = [0u8; SIG_LEN];
    sig_arr.copy_from_slice(sig);

    let vk = VerifyingKey::from_bytes(&pk_arr).context("无效的 Ed25519 公钥")?;
    let signature = Signature::from_bytes(&sig_arr);
    vk.verify(data, &signature).context("Ed25519 签名验证失败")?;
    Ok(())
}

/// 用私钥对 data 签名，返回 64 字节签名（发布方 / CI 用）。
pub fn sign(data: &[u8], signing_key: &ed25519_dalek::SigningKey) -> [u8; SIG_LEN] {
    signing_key.sign(data).to_bytes()
}

/// hex 编码（与 .sha256 / .sig 资产的文本格式一致）
pub fn encode_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    use std::fmt::Write;
    for b in bytes {
        let _ = write!(s, "{:02x}", b);
    }
    s
}

/// hex 解码（读 .sig / 私钥文件用）。长度必须偶数，字符须合法。
pub fn decode_hex(s: &str) -> Result<Vec<u8>> {
    let bytes = s.as_bytes();
    if !bytes.len().is_multiple_of(2) {
        bail!("hex 长度必须为偶数，实际 {}", bytes.len());
    }
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for chunk in bytes.chunks(2) {
        out.push((hex_val(chunk[0])? << 4) | hex_val(chunk[1])?);
    }
    Ok(out)
}

fn hex_val(c: u8) -> Result<u8> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        _ => bail!("无效 hex 字符: {}", c as char),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use rand::rngs::OsRng;

    #[test]
    fn test_verify_accepts_valid_signature() {
        let signing = SigningKey::generate(&mut OsRng);
        let data = b"engine bundle v1.7.3 content";
        let sig = signing.sign(data);
        let pubkey = signing.verifying_key().to_bytes();

        verify(data, &sig.to_bytes(), &pubkey).expect("合法签名应通过");
    }

    #[test]
    fn test_verify_rejects_tampered_data() {
        let signing = SigningKey::generate(&mut OsRng);
        let sig = signing.sign(b"original bundle");
        let pubkey = signing.verifying_key().to_bytes();

        // 数据被篡改 → 签名不再匹配
        let result = verify(b"tampered bundle", &sig.to_bytes(), &pubkey);
        assert!(result.is_err(), "篡改数据必须验证失败");
    }

    #[test]
    fn test_verify_rejects_wrong_key() {
        let signing_a = SigningKey::generate(&mut OsRng);
        let signing_b = SigningKey::generate(&mut OsRng);
        let data = b"bundle";
        let sig = signing_a.sign(data);

        // 用 B 的公钥验 A 的签名 → 失败
        let result = verify(data, &sig.to_bytes(), &signing_b.verifying_key().to_bytes());
        assert!(result.is_err(), "错误公钥必须验证失败");
    }

    #[test]
    fn test_verify_rejects_bad_lengths() {
        let data = b"x";
        // 公钥长度错
        assert!(verify(data, &[0u8; 64], &[0u8; 31]).is_err());
        // 签名长度错
        assert!(verify(data, &[0u8; 63], &[0u8; 32]).is_err());
    }

    #[test]
    fn test_verify_handles_empty_data() {
        // 空数据也是合法签名输入（边界）
        let signing = SigningKey::generate(&mut OsRng);
        let sig = signing.sign(b"");
        let pubkey = signing.verifying_key().to_bytes();
        verify(b"", &sig.to_bytes(), &pubkey).unwrap();
    }

    #[test]
    fn test_sign_then_verify_roundtrip() {
        let signing = SigningKey::generate(&mut OsRng);
        let data = b"engine bundle v1.7.3";
        let sig_bytes = sign(data, &signing);
        let pubkey = signing.verifying_key().to_bytes();
        // 签名产物能被 verify 接受
        verify(data, &sig_bytes, &pubkey).unwrap();
    }

    #[test]
    fn test_hex_encode_decode_roundtrip() {
        let original = [0u8, 15, 16, 255, 0xab, 0xcd];
        let encoded = encode_hex(&original);
        assert_eq!(encoded, "000f10ffabcd");
        let decoded = decode_hex(&encoded).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn test_decode_hex_rejects_invalid() {
        assert!(decode_hex("abc").is_err()); // 奇数长度
        assert!(decode_hex("xy").is_err()); // 非 hex 字符
        assert!(decode_hex("ZZZZ").is_err());
    }
}
