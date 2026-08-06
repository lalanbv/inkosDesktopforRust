//! engine bundle 签名密钥对生成（M4c 发布方一次性工具）
//!
//! 用法：cargo run --example gen-keypair
//!
//! 输出 Ed25519 密钥对：
//!   - 私钥 hex → GitHub secret `INKOS_ENGINE_SIGNING_KEY`（保密，不入仓）
//!   - 公钥 hex → 编译期环境变量 `INKOS_ENGINE_PUBKEY`（公开，内嵌客户端）
//!
//! 作为 example 而非 bin：只在发布方本地生成密钥时运行，不进 release 产物，
//! 也不把 rand 提升为运行时依赖。

use ed25519_dalek::SigningKey;
use inkos_desktop::updater::sig::encode_hex;
use rand::rngs::OsRng;

fn main() {
    let signing = SigningKey::generate(&mut OsRng);
    let pubkey = signing.verifying_key();
    let sk = encode_hex(&signing.to_bytes());
    let pk = encode_hex(&pubkey.to_bytes());

    println!("=== engine bundle 签名密钥对（Ed25519）===\n");
    println!("私钥 → GitHub secret INKOS_ENGINE_SIGNING_KEY（保密，切勿入仓）:");
    println!("{sk}\n");
    println!("公钥 → 编译期 INKOS_ENGINE_PUBKEY（公开，内嵌客户端验证用）:");
    println!("{pk}\n");
    println!("客户端强制验证构建：");
    println!("  INKOS_ENGINE_PUBKEY={pk} cargo build --release");
}
