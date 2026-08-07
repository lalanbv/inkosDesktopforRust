//! 插件市场端到端集成测试——验证安全关键安装链：
//! 构造插件 → 打包 tar.gz → Ed25519 签名 + sha256 → 注册表（TOML）签名 →
//! `fetch_registry`（mock 传输：验签 + 解析）→ `find_latest` → `verify_bundle` →
//! `safe_extract_tar_gz` → `PluginManager::install_plugin`。
//!
//! 覆盖命令层（`cmd_*`）之下的完整管线——命令只是 tauri::State 包装，本测试直接
//! 驱动其底层 pub API（不可经 tauri::State 单测的部分）。

use ed25519_dalek::Signer;
use inkos_desktop::plugin::{
    manager::PluginManager,
    registry::{self, PluginRegistryIndex},
};
use inkos_desktop::updater::sig;
use sha2::{Digest, Sha256};
use std::io::Write;

/// 打包 (name, content) 条目为 tar.gz（根级条目，匹配 safe_extract_tar_gz 约定）
fn build_tar_gz(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut tar_buf = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar_buf);
        for (name, data) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, *name, std::io::Cursor::new(*data))
                .unwrap();
        }
        builder.finish().unwrap();
    }
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(&tar_buf).unwrap();
    gz.finish().unwrap()
}

#[tokio::test]
async fn marketplace_install_pipeline_end_to_end() {
    // 1. 构造插件包（plugin.toml + plugin.wasm）
    let plugin_toml = b"[plugin]\nid=\"my-plugin\"\nname=\"My\"\nversion=\"1.0.0\"\n\
description=\"e2e\"\nauthor=\"tester\"\nlicense=\"MIT\"\nabi_version=\"1\"\n\
entrypoint=\"plugin.wasm\"\n";
    let bundle = build_tar_gz(&[("plugin.toml", plugin_toml), ("plugin.wasm", b"WASM")]);

    // 2. sha256 + Ed25519 签名 bundle
    let bundle_sha = sig::encode_hex(&Sha256::digest(&bundle));
    let signing = ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng);
    let bundle_sig = sig::encode_hex(&signing.sign(&bundle).to_bytes());
    let pubkey = signing.verifying_key().to_bytes();

    // 3. 构造注册表 TOML（含 bundle 的 sha + 签名）+ 对注册表签名
    let registry_toml = format!(
        r#"
version = 1

[[plugins]]
id = "my-plugin"
name = "My"
version = "1.0.0"
description = "e2e"
author = "tester"
license = "MIT"
abi_version = "1"
min_host_version = "0.0.0"
download_url = "https://registry.example.com/my-plugin-1.0.0.tar.gz"
sha256 = "{bundle_sha}"
signature = "{bundle_sig}"
capabilities = []
"#,
    );
    let registry_sig =
        sig::encode_hex(&signing.sign(registry_toml.as_bytes()).to_bytes());

    // 4. mock 传输：registry_url → registry_toml；{url}.sig → registry_sig
    let registry_url = "https://registry.example.com/registry.toml";
    let index = registry::fetch_registry(registry_url, &pubkey, {
        let r = registry_toml.clone();
        let s = registry_sig.clone();
        move |url: &str| {
            let url = url.to_string();
            let r = r.clone();
            let s = s.clone();
            Box::pin(async move {
                if url.ends_with(".sig") {
                    Ok(s.into_bytes())
                } else {
                    Ok(r.into_bytes())
                }
            })
        }
    })
    .await
    .expect("fetch_registry 验签 + 解析应成功");

    // 5. find_latest → entry；验签 bundle（用同一公钥）
    let entry = index
        .find_latest("my-plugin")
        .expect("注册表应含 my-plugin")
        .clone();
    entry
        .verify_bundle(&bundle, &pubkey)
        .expect("bundle sha256 + 签名应验证通过");

    // 6. 安全解压到临时目录
    let install_temp = tempfile::tempdir().unwrap();
    registry::safe_extract_tar_gz(&bundle, install_temp.path())
        .expect("安全解压应成功");
    assert!(install_temp.path().join("plugin.toml").exists());
    assert!(install_temp.path().join("plugin.wasm").exists());

    // 7. PluginManager 安装（解析 plugin.toml + 复制到 plugins_dir）
    let plugins_root = tempfile::tempdir().unwrap();
    let mut manager = PluginManager::new(plugins_root.path()).expect("PluginManager 创建应成功");
    let source = install_temp.path().to_string_lossy().to_string();
    let meta = manager
        .install_plugin(&source)
        .await
        .expect("install_plugin 应成功");
    assert_eq!(meta.id, "my-plugin");
    assert_eq!(meta.version, "1.0.0");

    // 8. 二次安装同 id → 拒绝（已安装）
    assert!(manager.install_plugin(&source).await.is_err());
}

#[test]
fn marketplace_rejects_tampered_bundle() {
    // bundle 被篡改 → verify_bundle 的 sha256 不匹配 → 拒（即便签名来自合法旧版）
    let plugin_toml = b"[plugin]\nid=\"p\"\nname=\"P\"\nversion=\"1.0.0\"\n\
description=\"\"\nauthor=\"\"\nlicense=\"MIT\"\nabi_version=\"1\"\nentrypoint=\"plugin.wasm\"\n";
    let bundle = build_tar_gz(&[("plugin.toml", plugin_toml), ("plugin.wasm", b"WASM")]);
    let bundle_sha = sig::encode_hex(&Sha256::digest(&bundle));
    let signing = ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng);
    let bundle_sig = sig::encode_hex(&signing.sign(&bundle).to_bytes());
    let pubkey = signing.verifying_key().to_bytes();

    // 用合法 bundle 构造条目，但验签时传入篡改后的 bundle
    let toml_str = format!(
        r#"version = 1
[[plugins]]
id = "p"
name = "P"
version = "1.0.0"
description = ""
author = ""
license = "MIT"
abi_version = "1"
min_host_version = "0.0.0"
download_url = "https://x/p.tar.gz"
sha256 = "{bundle_sha}"
signature = "{bundle_sig}"
capabilities = []
"#,
    );
    let idx = PluginRegistryIndex::parse(&toml_str).unwrap();
    let entry = idx.find_latest("p").unwrap().clone();

    let mut tampered = bundle.clone();
    tampered[0] ^= 0xff;
    assert!(
        entry.verify_bundle(&tampered, &pubkey).is_err(),
        "篡改的 bundle 必须 verify_bundle 失败"
    );
}

#[tokio::test]
async fn marketplace_update_overwrites_installed() {
    // install v1.0.0 → update_plugin v1.1.0（覆盖，无中间缺失态）→ 断言版本升级
    let mk = |ver: &str| {
        let toml = format!(
            "[plugin]\nid=\"up\"\nname=\"Up\"\nversion=\"{ver}\"\ndescription=\"\"\n\
author=\"\"\nlicense=\"MIT\"\nabi_version=\"1\"\nentrypoint=\"plugin.wasm\"\n"
        );
        build_tar_gz(&[("plugin.toml", toml.as_bytes()), ("plugin.wasm", ver.as_bytes())])
    };
    let v1_temp = tempfile::tempdir().unwrap();
    let v2_temp = tempfile::tempdir().unwrap();
    registry::safe_extract_tar_gz(&mk("1.0.0"), v1_temp.path()).unwrap();
    registry::safe_extract_tar_gz(&mk("1.1.0"), v2_temp.path()).unwrap();

    let plugins_root = tempfile::tempdir().unwrap();
    let mut manager = PluginManager::new(plugins_root.path()).unwrap();
    manager
        .install_plugin(&v1_temp.path().to_string_lossy())
        .await
        .expect("初装 v1.0.0");
    let meta = manager
        .update_plugin(&v2_temp.path().to_string_lossy())
        .await
        .expect("update_plugin 覆盖到 v1.1.0");
    assert_eq!(meta.version, "1.1.0");

    let installed = manager.list_plugins();
    assert_eq!(
        installed
            .iter()
            .find(|m| m.id == "up")
            .map(|m| m.version.as_str()),
        Some("1.1.0"),
        "list_plugins 应反映 v1.1.0"
    );
}

#[tokio::test]
async fn marketplace_install_resolves_dependencies() {
    // lib（无依赖）+ app（依赖 lib ^1.0.0）→ resolve_dependencies 后序 [lib, app]
    // → 按序解压 + install_plugin → 两者均已装
    let mk = |id: &str, ver: &str| {
        let toml = format!(
            "[plugin]\nid=\"{id}\"\nname=\"{id}\"\nversion=\"{ver}\"\ndescription=\"\"\n\
author=\"\"\nlicense=\"MIT\"\nabi_version=\"1\"\nentrypoint=\"plugin.wasm\"\n"
        );
        build_tar_gz(&[("plugin.toml", toml.as_bytes()), ("plugin.wasm", id.as_bytes())])
    };
    let sha = "0".repeat(64);
    let sig = "0".repeat(128);
    let registry_toml = format!(
        r#"version = 1

[[plugins]]
id = "lib"
name = "lib"
version = "1.0.0"
description = ""
author = ""
license = "MIT"
abi_version = "1"
min_host_version = "0.0.0"
download_url = "https://x/lib.tar.gz"
sha256 = "{sha}"
signature = "{sig}"
capabilities = []

[[plugins]]
id = "app"
name = "app"
version = "1.0.0"
description = ""
author = ""
license = "MIT"
abi_version = "1"
min_host_version = "0.0.0"
download_url = "https://x/app.tar.gz"
sha256 = "{sha}"
signature = "{sig}"
capabilities = []
dependencies = {{ lib = "^1.0.0" }}
"#,
    );
    let idx = PluginRegistryIndex::parse(&registry_toml).expect("注册表解析");
    let app = idx.find_latest("app").expect("含 app").clone();

    // 后序拓扑：lib（无依赖）在前，app 在后
    let order = registry::resolve_dependencies(&app, &idx).expect("依赖解析");
    assert_eq!(
        order.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
        vec!["lib", "app"],
        "安装顺序应为 [lib, app]"
    );

    // 按序解压 + 安装（模拟 cmd_install_from_registry 的依赖循环）
    let plugins_root = tempfile::tempdir().unwrap();
    let mut manager = PluginManager::new(plugins_root.path()).unwrap();
    for e in &order {
        let bundle = mk(&e.id, &e.version);
        let t = tempfile::tempdir().unwrap();
        registry::safe_extract_tar_gz(&bundle, t.path()).unwrap();
        manager
            .install_plugin(&t.path().to_string_lossy())
            .await
            .unwrap_or_else(|err| panic!("安装 {} 失败: {err}", e.id));
    }

    let installed: Vec<String> = manager.list_plugins().into_iter().map(|m| m.id).collect();
    assert!(installed.contains(&"lib".to_string()), "lib 应已装");
    assert!(installed.contains(&"app".to_string()), "app 应已装");
}
