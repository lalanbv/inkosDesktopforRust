# 228 号：updater 双通道 + plugin 管理面轻量复核——零缺口

- 日期：2026-09-09
- 分支：develop
- 关联：166 号（updater 双通道落地）、Phase 6.5（engine bundle Ed25519）
- 推送核验：origin/develop 仍停 6bc65564——**201–224…227 共 27 提交待推送**；本批次后本地领先 28。两项默认值无新答复。

## 一、updater 双通道（mod/delta/sig/engine，1.3k 行）

| 审计点 | 结论 |
|---|---|
| 版本比较 | ✓ `is_newer` 严格 semver：`latest > cur` 才更新——**版本降级防护**；非 semver 串（nightly 等）保守视为无更新不误报 |
| engine bundle 签名 | ✓ Ed25519 `verify` 强制接入 apply 链；SigAction 三态——Verify（验签通过才落盘）/ WarnPass（过渡期：INKOS_ENGINE_PUBKEY 未配置且 release 无 .sig，warn 放行）/ **Reject（fail-closed：已配公钥但 release 未签名 → 拒绝）** |
| shell 通道 | ✓ tauri-plugin-updater + Ed25519（pubkey 嵌 tauri.conf.json） |

## 二、plugin 管理面（7.9k 行）

| 审计点 | 结论 |
|---|---|
| 权限模型 | ✓ Capability 声明制——Network `allowed_domains` 域校验（host_api 消费）、SystemCommand `allowed_commands` 裸命令名白名单 |
| manifest | ✓ 系统命令安全格式校验（白名单条目逐项验证） |
| 进程面 | ✓ process.rs 独立子进程模型（500 行）——插件不进宿主进程地址空间 |

## 三、验证

纯复核批次（零代码改动）：src-tauri check/test/clippy 于 222 号后持续绿；engine 门禁于 226 号已绿（lib 1311/集成 196/clippy 0/duel 10/10），core 1917 / studio 793 / 双 typecheck 已绿。

## 四、遗留

SigAction::WarnPass 过渡期放行依赖部署侧配置 INKOS_ENGINE_PUBKEY + 发布带签 release——转为强制模式是发版运维动作（166 号既定路径），非代码缺口。
