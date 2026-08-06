#!/usr/bin/env bash
# 生成 shell updater 的 Ed25519 keypair（tauri-plugin-updater 签名用）。
#
# 用途：
#   1. 跑本脚本生成 keypair（私钥务必保密，勿提交）。
#   2. pubkey 写入 src-tauri/tauri.conf.json plugins.updater.pubkey。
#   3. private key 设为 GitHub secret TAURI_SIGNING_PRIVATE_KEY（desktop-build.yml 签 release）。
#      密码（若有）设为 TAURI_SIGNING_PRIVATE_KEY_PASSWORD。
#
# 当前 tauri.conf.json 的 pubkey 是 dev keypair（生成于 M3e）；生产前建议重新生成自控私钥。
set -euo pipefail

OUT="${1:-/tmp/inkos-updater-key}"
npx --yes @tauri-apps/cli@latest signer generate -w "$OUT" --password "${TAURI_KEY_PASSWORD:-}" || {
  echo "FAIL: tauri signer generate 失败（确认 npx 可用）"
  exit 1
}

echo ""
echo "=========================================================="
echo "Keypair 生成完成："
echo "  私钥（保密，设 CI secret TAURI_SIGNING_PRIVATE_KEY）：$OUT"
echo "  公钥（写入 tauri.conf.json plugins.updater.pubkey）：$OUT.pub"
echo ""
echo "公钥内容："
cat "$OUT.pub"
echo ""
echo "私钥内容（设 CI secret，勿提交）："
cat "$OUT"
echo "=========================================================="
echo "下一步："
echo "  1. 把上面的公钥贴到 src-tauri/tauri.conf.json plugins.updater.pubkey"
echo "  2. GitHub Settings → Secrets → Actions："
echo "     TAURI_SIGNING_PRIVATE_KEY = 上方私钥全文"
echo "     TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ${TAURI_KEY_PASSWORD:-（空则不设）}"
