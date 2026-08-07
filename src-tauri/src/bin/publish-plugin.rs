//! 插件发布工具（市场生产端）——签名 bundle + 生成注册表条目 TOML。
//!
//! 消费侧 `cmd_install_from_registry` 下载 → `verify_bundle` 验签 → 安装；
//! 本工具产出的 `[[plugins]]` 条目可直接 append 到 `registry.toml`，补全
//! 「消费 + 生产」生态。
//!
//! 用法：
//!   publish-plugin <bundle.tar.gz> <plugin.toml> <private-key-hex> <download-url> [min-host-version]
//!
//! - bundle.tar.gz：预构建的插件包（须含根级 plugin.toml + .wasm，见消费侧
//!   `safe_extract_tar_gz` 约定）。发布方自行打包（build 与 sign 分离）。
//! - plugin.toml：插件清单（取 id/version/capabilities 等元数据）。
//! - private-key-hex：32 字节 Ed25519 私钥（hex）。
//! - download-url：bundle 发布 URL（须 https://，消费侧条目校验强制）。
//! - min-host-version：最低宿主版本（默认 0.0.0 = 任意）。
//!
//! 输出：注册表条目 TOML（stdout）+ 对应公钥（hex，用于核对 release 元数据）。
//! 不读网络/环境，只处理命令行参数 → 适合 CI 沙箱（同 sign-bundle）。

use anyhow::{bail, Context, Result};
use inkos_desktop::plugin::{
    manifest::PluginManifest,
    registry::RegistryEntry,
};
use inkos_desktop::updater::sig::{decode_hex, encode_hex, sign, PUBKEY_LEN};
use sha2::{Digest, Sha256};
use std::{env, fs, process::ExitCode};

fn main() -> ExitCode {
    match run() {
        Ok(msg) => {
            println!("{msg}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("publish-plugin 失败: {e:#}");
            eprintln!(
                "用法: publish-plugin <bundle.tar.gz> <plugin.toml> <private-key-hex> <download-url> [min-host-version]"
            );
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<String> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 5 || args.len() > 6 {
        bail!(
            "参数错误：需要 <bundle.tar.gz> <plugin.toml> <private-key-hex> <download-url> [min-host-version]，实际 {} 个",
            args.len() - 1
        );
    }
    let bundle_path = &args[1];
    let manifest_path = &args[2];
    let key_hex = &args[3];
    let download_url = &args[4];
    let min_host_version = args.get(5).cloned().unwrap_or_else(|| "0.0.0".to_string());

    if !download_url.starts_with("https://") {
        bail!("download_url 必须为 https://（消费侧条目校验强制）: {}", download_url);
    }

    let bundle = fs::read(bundle_path)
        .with_context(|| format!("读取 bundle 失败: {}", bundle_path))?;
    let manifest_str = fs::read_to_string(manifest_path)
        .with_context(|| format!("读取 plugin.toml 失败: {}", manifest_path))?;
    let manifest: PluginManifest =
        toml::from_str(&manifest_str).context("解析 plugin.toml 失败")?;

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

    let entry = build_entry(&bundle, &manifest, &signing_key, download_url, &min_host_version)?;
    let entry_toml = toml::to_string(&entry).context("序列化注册表条目失败")?;
    let pubkey = signing_key.verifying_key();
    Ok(format!(
        "注册表条目 TOML（append 到 registry.toml 的 [[plugins]]）：\n[[plugins]]\n{entry_toml}\n对应公钥（hex）: {}",
        encode_hex(&pubkey.to_bytes())
    ))
}

/// 签 bundle + 从清单构造 RegistryEntry（含字段校验）。可测核心：run 的纯逻辑部分。
fn build_entry(
    bundle: &[u8],
    manifest: &PluginManifest,
    signing_key: &ed25519_dalek::SigningKey,
    download_url: &str,
    min_host_version: &str,
) -> Result<RegistryEntry> {
    let sha256 = encode_hex(&Sha256::digest(bundle));
    let signature = encode_hex(&sign(bundle, signing_key));
    let entry = RegistryEntry {
        id: manifest.plugin.id.clone(),
        name: manifest.plugin.name.clone(),
        version: manifest.plugin.version.clone(),
        description: manifest.plugin.description.clone(),
        author: manifest.plugin.author.clone(),
        homepage: manifest.plugin.homepage.clone(),
        license: manifest.plugin.license.clone(),
        abi_version: manifest.plugin.abi_version.clone(),
        min_host_version: min_host_version.to_string(),
        download_url: download_url.to_string(),
        sha256,
        signature,
        capabilities: manifest.capabilities.clone(),
        dependencies: manifest.dependencies.clone(),
    };
    // 发布侧早校验（id 白名单 + semver + url https + sha/sig 格式 + capability 语法）
    entry.validate().context("产出的注册表条目校验失败")?;
    Ok(entry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::OsRng;

    fn test_manifest() -> PluginManifest {
        toml::from_str(
            r#"
capabilities = ["read_project", "network:api.example.com"]
[plugin]
id = "test-plugin"
name = "Test"
version = "1.2.0"
description = "a test plugin"
author = "tester"
license = "MIT"
abi_version = "1"
entrypoint = "plugin.wasm"
"#,
        )
        .unwrap()
    }

    /// 端到端：build_entry 产出的条目字段合法 + 消费侧 verify_bundle 接受
    #[test]
    fn build_entry_produces_verifiable_registry_entry() {
        let manifest = test_manifest();
        let bundle = b"fake plugin bundle bytes for signing";
        let signing = ed25519_dalek::SigningKey::generate(&mut OsRng);
        let entry = build_entry(
            bundle,
            &manifest,
            &signing,
            "https://cdn.example.com/test-plugin-1.2.0.tar.gz",
            "0.1.0",
        )
        .unwrap();

        assert_eq!(entry.id, "test-plugin");
        assert_eq!(entry.version, "1.2.0");
        assert_eq!(entry.capabilities.len(), 2);

        // 消费侧验签接受：sha256 + Ed25519（用产出的公钥）
        let pubkey = signing.verifying_key().to_bytes();
        entry.verify_bundle(bundle, &pubkey).unwrap();
    }

    /// 非法 id（路径逃逸）→ build_entry 的 validate 拒绝
    #[test]
    fn build_entry_rejects_unsafe_id() {
        let mut manifest = test_manifest();
        manifest.plugin.id = "../pwned".to_string();
        let bundle = b"x";
        let signing = ed25519_dalek::SigningKey::generate(&mut OsRng);
        assert!(build_entry(
            bundle,
            &manifest,
            &signing,
            "https://cdn.example.com/x.tar.gz",
            "0.1.0"
        )
        .is_err());
    }

    /// 非 https 的 download_url → build_entry 的 validate 拒绝
    #[test]
    fn build_entry_rejects_non_https_url() {
        let manifest = test_manifest();
        let bundle = b"x";
        let signing = ed25519_dalek::SigningKey::generate(&mut OsRng);
        assert!(build_entry(
            bundle,
            &manifest,
            &signing,
            "http://insecure.example.com/x.tar.gz",
            "0.1.0"
        )
        .is_err());
    }
}
