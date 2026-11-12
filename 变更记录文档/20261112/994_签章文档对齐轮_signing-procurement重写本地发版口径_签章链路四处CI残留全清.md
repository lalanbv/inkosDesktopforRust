# 994 号：签章文档对齐轮——signing-procurement 重写为本地发版口径 + 签章链路四处 CI 残留全清（993 候选备案清偿）

日期：2026-11-12。开工三查净（HEAD=5d8fe676、工作区空、无并行会话）。

## ① 触发面与普查扩面

993 收尾全仓验证发现 `docs/signing-procurement.md` 整份为 workflows 时代操作文档。本轮普查把签章链路的 CI 残留从 1 处扩到 **4 处**：

1. `docs/signing-procurement.md`：全篇围绕「GitHub secret 配置 + desktop-build.yml gated 签名步骤」（两者均已不存在）；
2. `scripts/desktop-gen-updater-key.sh`（shell 通道密钥脚本）：用途注释与 echo 指引 4 处指向 GitHub Settings → Secrets；
3. `src-tauri/src/bin/sign-bundle.rs`：头注释「发布方 / CI 用」「CI 在发版时调用」「私钥来源：CI secret INKOS_ENGINE_SIGNING_KEY」；
4. `src-tauri/examples/gen-keypair.rs`：头注释与**实际 println 输出文案**「私钥 → GitHub secret INKOS_ENGINE_SIGNING_KEY」——端到端冒烟时暴露（文档重写后工具输出仍旧文案，文档与被测物不一致）。

## ② 重写口径（工具链不变，仅「谁在何时调用」从 CI 变为发布者本人）

- **signing-procurement.md** 全量重写：头部 994 勘误标注；「配置 CI」节 → 「本地签名发布流程」（`cargo build --release --bin sign-bundle` + `read -rs` env 注入私钥防 shell 历史泄漏 + bundle/.sha256/.sig 三件同传 release）；私钥保管安全红线（不入仓/不进 shell 历史/env 或密钥服务传入——Mimosa 生成前安全约束同口径）；客户端 `INKOS_ENGINE_PUBKEY` 编译期注入三态语义（fail-closed）不变；验证链路与轮换节改本地措辞；工具表 `sign-bundle` 用途改「发布方本地发版时手动调用」。
- **desktop-gen-updater-key.sh**：用途与两处 echo 指引改「发布方本机保管，发版时经 env 传入」，保留 994 勘误标注（原指引失效原因）。
- **sign-bundle.rs / gen-keypair.rs**：头注释与输出文案勘误；保留「不读网络/环境、纯命令行参数」安全性质描述（原文「适合 CI 沙箱」改「纯函数式，便于沙箱与测试」）。

## ③ 端到端冒烟（文档流程真实可跑的验收）

重写后流程活体：`cargo run --example gen-keypair` → 提取 64hex 密钥对 → `cargo run --bin sign-bundle <tmp-bundle> <priv>` → `.sig` 落盘（128 hex）全链通。冒烟同时暴露④处 gen-keypair 输出文案过时（随即勘误复验新文案输出正确）。

## ④ 门禁

clippy:gate 双 0（sign-bundle.rs/gen-keypair.rs 注释触碰后复跑）；`bash -n` shell 脚本语法核过；全仓 `GitHub secret|INKOS_ENGINE_SIGNING_KEY` 残留终扫零（唯一保留 = signing-procurement 勘误标注自身）；secure_update_integration 端到端测试在位引用 990（同树未触碰，testgate 584 口径）；U+FFFD 净。提交路径限定五文件：两文档 + 一脚本 + 两 Rust 源（注释面，clippy 已过）。

## 备案

1. `INKOS_ENGINE_SIGNING_KEY` 命名随 CI secret 消失而不再有运行时承载——文档不再引用该名；若未来恢复自动化签名再定新名。
2. 签名发布当前无常态化执行者（发版低频）；流程文档即执行手册，下次发版照做即可。
3. shell 通道 `tauri.conf.json` pubkey 仍为 dev keypair（M3e）——「生产前重新生成」提示保留于脚本注释，触发 = 正式 release 前置检查。

下一号自 995 起；出口回归常备；用户侧待办 = Fork 推送（含 955-994）。
