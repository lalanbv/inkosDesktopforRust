//! 安全更新端到端集成测试——组合 Phase 6.1（delta）+ Phase 6.5（签名）
//!
//! 模拟完整的安全更新链路：
//!   发布方：old bundle → new bundle，生成 delta + 对 new 签 Ed25519 签名
//!   客户端：下载 delta → apply_delta（SHA256 校验）→ sig::verify（来源认证）
//!           → 通过则得到可信 new bundle
//!
//! 任一环节被篡改（delta / 签名 / 公钥）都必须被拒绝——这是「最安全可靠」的
//! 端到端验证，而非孤立的单元测试。

use ed25519_dalek::{Signer, SigningKey};
use inkos_desktop::updater::delta::{apply_delta, sha256};
use inkos_desktop::updater::sig;
use rand::rngs::OsRng;

/// 发布方准备一次安全更新：返回 (delta, expected_sha256, signature, pubkey)
struct Release {
    delta: Vec<u8>,
    expected_sha: [u8; 32],
    sig: [u8; 64],
    pubkey: [u8; 32],
}

/// 模拟服务端：对 new bundle 生成 delta + 签名
fn prepare_release(old: &[u8], new: &[u8]) -> Release {
    let mut delta = Vec::new();
    bsdiff::diff(old, new, &mut delta).expect("bsdiff::diff");

    let signing = SigningKey::generate(&mut OsRng);
    let sig = signing.sign(new);
    let pubkey = signing.verifying_key();

    Release {
        delta,
        expected_sha: sha256(new),
        sig: sig.to_bytes(),
        pubkey: pubkey.to_bytes(),
    }
}

/// 客户端应用安全更新：delta + 校验 + 签名验证。全过则返回 new bundle。
fn apply_secure_update(old: &[u8], rel: &Release) -> Result<Vec<u8>, String> {
    let new = apply_delta(old, &rel.delta, &rel.expected_sha).map_err(|e| e.to_string())?;
    sig::verify(&new, &rel.sig, &rel.pubkey).map_err(|e| e.to_string())?;
    Ok(new)
}

#[test]
fn secure_update_roundtrip_succeeds() {
    let old = b"engine bundle v1.7.2: node runtime + modules".to_vec();
    let new = b"engine bundle v1.7.3: node runtime + modules + patched".to_vec();
    let rel = prepare_release(&old, &new);

    let reconstructed = apply_secure_update(&old, &rel).expect("合法更新应全链路通过");
    assert_eq!(reconstructed, new);
}

#[test]
fn secure_update_rejects_tampered_delta() {
    // delta 在传输中被篡改 → apply_delta 的 SHA256 校验首先拒绝
    let old = b"v1 bundle".to_vec();
    let new = b"v2 bundle updated".to_vec();
    let mut rel = prepare_release(&old, &new);

    // 篡改 delta 的某个字节
    if !rel.delta.is_empty() {
        rel.delta[0] ^= 0xff;
    }

    let result = apply_secure_update(&old, &rel);
    assert!(result.is_err(), "篡改的 delta 必须被拒绝");
    // 篡改可能在两道防线之一被拦：bsdiff::patch 解析失败，或 apply 后 SHA256 不匹配。
    // 只要是 Err 即证明安全链路生效——具体在哪道防线取决于篡改的字节位置。
}

#[test]
fn secure_update_rejects_wrong_signature() {
    // delta 正确（SHA256 通过），但签名是用另一把私钥签的 → sig::verify 拒绝
    let old = b"v1 bundle".to_vec();
    let new = b"v2 bundle".to_vec();
    let mut rel = prepare_release(&old, &new);

    // 用另一把密钥重新签名（模拟攻击者用私钥伪造签名，但客户端公钥是原发布方的）
    let attacker = SigningKey::generate(&mut OsRng);
    rel.sig = attacker.sign(&new).to_bytes();
    // rel.pubkey 仍是原发布方公钥

    let result = apply_secure_update(&old, &rel);
    assert!(result.is_err(), "错误签名必须被拒绝");
    assert!(
        result.unwrap_err().contains("签名验证失败"),
        "应在签名验证环节失败"
    );
}

#[test]
fn secure_update_rejects_tampered_new_after_apply() {
    // 边界：即便 delta 应用后字节与签名时不同（理论上 SHA256 会先拦），签名也兜底
    // 这里直接验证 sig::verify 对篡改数据的独立拦截能力
    let old = b"v1".to_vec();
    let new = b"v2".to_vec();
    let rel = prepare_release(&old, &new);

    // 手动 apply（绕过 SHA256）得到 new，再篡改 1 字节，验证 sig 拦截
    let mut tampered = new.clone();
    tampered[0] ^= 0xff;
    let r = sig::verify(&tampered, &rel.sig, &rel.pubkey);
    assert!(r.is_err(), "签名验证必须拒绝篡改后的 bundle");
}
