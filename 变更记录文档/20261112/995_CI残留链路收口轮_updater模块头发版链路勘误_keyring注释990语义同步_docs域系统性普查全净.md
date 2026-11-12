# 995 号：CI 残留链路收口轮——updater 模块头发版链路勘误 + keyring 注释 990 语义同步 + docs 域系统性普查全净

日期：2026-11-12 深夜。开工三查净（HEAD=45e066fb、工作区空、无并行会话）。993/994 方法论延伸（按链路扫非只扫 docs/）。bench 静默窗口复查仍未现（负载 26.5%/核超阈），991 延后维持。

## ① docs/ 域系统性普查（双判据，负结果亦是结果 980 先例）

13 份现行指引文档（排除 superpowers/plans 历史规划档案与历史台账）：
- 判据一（过时模式）：`cargo test -- --ignored` 裸跑 / GitHub Actions 操作指引 / workflow_dispatch / desktop-build.yml / scripts 引用形态——**全净**（唯一命中 = QUICK_START 的 desktop-build-inkos.sh，993 已核实现行发版脚本）。
- 判据二（引用物存在性）：全 docs 提取的仓库路径（scripts/src-tauri/packages/engine-rs 通配）逐个存在性核——**零缺失**。

## ② 全仓代码/脚本 CI 残留扫：两处命中全清

1. **`src-tauri/src/updater/mod.rs` 头注释（真过时指引）**：上游同步链路整段描述「M3e desktop-sync-regression.yml 定时 git merge → bump → tag → desktop-build.yml 构建发布」——两个具名 workflow 均已不存在。改写为当前真实链路：本地脚本发版（bump → tag → desktop-build-inkos.sh + desktop-package-engine.sh → 可选 sign-bundle 签名 → 手动上传 Releases）+ 上游同步人工 merge 无定时任务 + 995 勘误标注。shell 通道「M3e CI 用 private key 签 release」同步改「私钥发布方本地保管」。
2. **`src-tauri/src/secrets/store.rs` keyring 测试注释（半过时）**：「GitHub Actions macOS runner 可用」例子随 CI 移除失效；顺带 990 语义同步——「首次会弹钥匙串授权」改为 unique_service 无跨构建 ACL 弹框 + keychain 锁定 sweep 哨兵兜底；「CI / headless」改「headless」；跑法补 sweep 一键。

## ③ clippy doc lint 拦截一轮（门禁活体）

mod.rs 新注释段行首「+ 」被 rustdoc 解析为列表项（`doc list item without indentation` ×3）→ 措辞改「与」→ clippy 复绿双 0。门禁对注释面同样有牙齿（非只抓代码）。

## ④ 终态验证

clippy:gate 双 0；具名 workflow 引用（desktop-build.yml / desktop-sync-regression.yml / workflow_dispatch）全仓终扫零（唯一保留 = signing-procurement.md 994 勘误标注引述自身）；U+FFFD 净。本轮零行为改动（两 Rust 文件纯注释面，无测试计数变化——testgate 584 引用 994 同树；sweep/哨兵不受影响）。

## 备案

1. superpowers/plans/2025-01-15 历史规划档案与 phase5/harness/sea/bench-ledger 台账类文档维持原样（历史记载非现行指引，勘误会破坏档案性）。
2. 「CI 环境」类通用语义表述（headless CI 环境特征描述）不在勘误范围——判据始终收窄为**本仓已移除 workflow 的具名引用**。

下一号自 996 起；出口回归常备；用户侧待办 = Fork 推送（含 955-995）。
