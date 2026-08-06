//! engine bundle Ed25519 签名工具（发布方 / CI 用）—— M4c
//!
//! 用法：
//!   sign-bundle <bundle-path> <private-key-hex>
//!
//! 读取 bundle 文件 + 32 字节私钥（hex），输出 <bundle>.sig（签名 hex 文本，
//! 与 .sha256 资产格式一致）。CI 在发版时调用本工具签 engine bundle，
//! 上传 .sig 到 release；客户端 updater 下载 .sig 用 sig::verify 验证。
//!
//! 私钥来源：CI secret INKOS_ENGINE_SIGNING_KEY（见密钥采购指引文档）。
//! 本工具不读取任何网络/环境，只处理命令行参数 → 适合 CI 沙箱。

use anyhow::{bail, Context, Result};
use inkos_desktop::updater::sig::{decode_hex, encode_hex, sign, PUBKEY_LEN};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(msg) => {
            println!("{msg}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("sign-bundle 失败: {e:#}");
            eprintln!("用法: sign-bundle <bundle-path> <private-key-hex>");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<String> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        bail!("参数错误：需要 <bundle-path> <private-key-hex>，实际 {} 个", args.len() - 1);
    }

    let bundle_path = PathBuf::from(&args[1]);
    let key_hex = &args[2];

    // 1. 读 bundle
    let data = fs::read(&bundle_path)
        .with_context(|| format!("读取 bundle 失败: {}", bundle_path.display()))?;

    // 2. 解析私钥（必须 32 字节 = hex 64 字符）
    let key_bytes = decode_hex(key_hex).context("私钥 hex 解析失败")?;
    if key_bytes.len() != PUBKEY_LEN {
        bail!(
            "Ed25519 私钥必须 {PUBKEY_LEN} 字节（hex {} 字符），实际 {} 字节",
            PUBKEY_LEN * 2,
            key_bytes.len()
        );
    }
    let mut key_arr = [0u8; PUBKEY_LEN];
    key_arr.copy_from_slice(&key_bytes);
    let signing_key = ed25519_dalek::SigningKey::from_bytes(&key_arr);

    // 3. 签名
    let sig_bytes = sign(&data, &signing_key);

    // 4. 写 .sig（hex 文本，与 .sha256 格式一致）
    //    约定：bundle 同名 + ".sig" 后缀（engine-X.tar.gz → engine-X.tar.gz.sig），
    //    与 engine.rs asset 查找 `{tarball_name}.sig` 一致。不能用 with_extension
    //    ——那会把 .gz 替换成 .sig，得到错误的 engine-X.tar.sig。
    let sig_path = PathBuf::from(format!("{}.sig", bundle_path.to_string_lossy()));
    fs::write(&sig_path, encode_hex(&sig_bytes))
        .with_context(|| format!("写入 .sig 失败: {}", sig_path.display()))?;

    // 同时打印对应公钥（便于核对 release 元数据）
    let pubkey = signing_key.verifying_key();
    Ok(format!(
        "签名写入 {}\n对应公钥（hex）: {}",
        sig_path.display(),
        encode_hex(&pubkey.to_bytes())
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use inkos_desktop::updater::sig::verify;
    use rand::rngs::OsRng;

    /// 端到端：sign-bundle 产出的 .sig 能被客户端 verify 接受
    #[test]
    fn signed_bundle_passes_client_verify() {
        let temp = tempfile::TempDir::new().unwrap();
        let bundle = temp.path().join("engine-v1.7.3.tar");
        let data = b"fake engine bundle content for signing";
        fs::write(&bundle, data).unwrap();

        // 生成测试私钥（CI 侧会有真实密钥）
        let signing = ed25519_dalek::SigningKey::generate(&mut OsRng);
        let key_hex = encode_hex(&signing.to_bytes());

        // 模拟 main 流程（不走进程 argv，直接调 run 的核心逻辑）
        let key_bytes = decode_hex(&key_hex).unwrap();
        let mut arr = [0u8; PUBKEY_LEN];
        arr.copy_from_slice(&key_bytes);
        let sk = ed25519_dalek::SigningKey::from_bytes(&arr);
        let sig_bytes = sign(data, &sk);

        // 客户端验证：用公钥验 bundle
        let pubkey = sk.verifying_key().to_bytes();
        verify(data, &sig_bytes, &pubkey).unwrap();

        // .sig 文件内容（hex）解码后也能验证
        let sig_hex = encode_hex(&sig_bytes);
        let sig_decoded = decode_hex(&sig_hex).unwrap();
        verify(data, &sig_decoded, &pubkey).unwrap();
    }
}
