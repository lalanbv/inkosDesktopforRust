# 274 号：273 号遗留清偿——translation dataUrl 解析收敛 TS 逐字+prompt-pack 上限定性

- 日期：2026-09-10
- 分支：develop
- 关联：273 号（本批清偿其遗留 #1/#2）、272 号（audit-rust.mjs 边界提醒——并行会话代码仍在其途未落库，维持待其收尾）
- 编号衔接：查当日目录最大号 273，顺延 274
- 推送核验：origin/develop 仍 = d26658b1；本地领先 3（272/273/274 号）待用户 Fork 推送

## 一、遗留 #1 清偿：translation 上传 dataUrl 解析收敛（273 号遗留 §四.1）

`upload_translation` 的 dataUrl 解析从本地实现切换为 273 号 `upload_common::parse_data_url_with_mime`（TS `parseDataUrl` 逐字），删除本地包装函数。收敛内容（后两项为顺带修复的**真实行为偏差**，非仅文案）：

| 维度 | 收敛前（本地实现） | 收敛后（TS 逐字） |
|---|---|---|
| 解析失败错误码 | `INVALID_TRANSLATION_UPLOAD` / "Translation upload has an invalid data URL" | `INVALID_ATTACHMENT_DATA_URL` / "Attachment must be a base64 data URL" |
| 非 base64 data URL | **接受**（原始字节落盘） | 拒绝 400（TS 正则强制 `;base64,`） |
| base64 解码语义 | `general_purpose::STANDARD` **严格**——含换行/空白的 base64 直接失败 | `decode_base64_lenient` 宽松（Node `Buffer.from(s,"base64")` 语义） |

缺失 dataUrl 路径本就与 TS 一致（`INVALID_TRANSLATION_UPLOAD` / "Upload is missing dataUrl"），不动。80MB 业务上限与上轮的 128MB 读取上限不变。

**验证**：translations70 e2e 3/3（含新增断言：`data:text/plain,raw` → 400 `INVALID_ATTACHMENT_DATA_URL`）；全量 `cargo test`（INKOS_DUEL=1）1624 通过 0 失败；clippy --all-targets 0 警告。

## 二、遗留 #2 定性：`put_prompt_pack` 维持 2MB 默认（273 号遗留 §四.2）

核对 TS `PUT /prompt-packs/:promptId`（server.ts L4307）：content 为**用户手写的提示包覆盖文本**（写盘为纯文本覆盖），无业务上限；现实尺度 KB 级。Rust 现状 2MB（axum 提取器默认）= 70 万汉字，不存在可信用户流触达。**决策：维持默认，不放宽**——为大载荷统一放宽反而削弱小载荷端点的意外巨包防护。此定性关闭该遗留项。

## 三、并行会话状态

`package.json` / `src-tauri/src/engine/rustbin.rs` / `scripts/audit-rust.mjs` 连续第三轮核查仍在途未落库，272 号提出的 cargo-audit 运行期失败边界提醒维持待其收尾处置，本批继续未触碰。

## 四、遗留

两项默认值、ja A/B、历史瘦身——待用户决策（承接 273 号；推送一项随本批累计 3 提交待推）。
