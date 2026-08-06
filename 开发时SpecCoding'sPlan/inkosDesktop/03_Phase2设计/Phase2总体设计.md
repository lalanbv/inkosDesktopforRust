# Phase 2: Production Readiness 总体设计

| 项 | 值 |
|---|---|
| 版本 | v1.0 |
| 日期 | 2026-08-06 |
| 前置 | M3 完成（v0.3.0-m3）：自包含分发 + 双通道自更新 + 三平台 CI + ad-hoc 签名 |
| 目标 | 生产就绪：可观测性 + E2E 回归 + 生产签名 + 用户文档 + 错误体验 + SEA 优化 |
| 里程碑 | 6 子里程碑（M4a-M4f） |

---

## 1. Phase 2 定位与 M3 的关系

### 1.1 M3 交付回顾

M3 实现了**最小可分发版本**（MVP for Distribution）：

- ✅ **自包含**：便携 Node 22 + engine bundle，零系统依赖
- ✅ **自更新**：双通道（engine GitHub SHA256 原子替换 + shell Ed25519）
- ✅ **三平台 CI**：macOS/Linux/Windows × 2 arch，tauri-action 打包
- ✅ **ad-hoc 签名**：无 Apple Developer ID 下最佳可分发性
- ✅ **核心功能**：项目选择、sidecar 启动、observer 接线、secrets keychain

**M3 可分发但未生产就绪**——缺可观测性、回归保障、用户引导、错误恢复、体积优化。

### 1.2 Phase 2 目标

将 M3 从「可分发 MVP」提升到「生产就绪」：

1. **可观测性**（M4a）：tracing 日志 + 崩溃上报 + 诊断命令，问题可定位
2. **E2E 回归**（M4b）：关键路径自动化测试，防破坏性变更
3. **生产签名**（M4c）：Apple Developer ID / Windows Authenticode，消除安全警告
4. **用户文档**（M4d）：安装/使用/故障排查指引，降低支持成本
5. **错误体验**（M4e）：优雅降级 + 自愈 + 友好错误提示，提升可用性
6. **SEA 优化**（M4f）：Node.js Single Executable（SEA）打包 engine，体积减半

---

## 2. 六子里程碑分解（M4a-M4f）

### M4a：可观测性（Observability）

**目标**：问题可定位、可复现、可诊断。

| 范围 | 交付 |
|---|---|
| **结构化日志** | `tracing` crate（分级 ERROR/WARN/INFO/DEBUG）+ 滚动文件（log_dir，保留 7 天） |
| **崩溃上报** | panic hook 捕获 + 本地 crash dump（JSON）+ 可选匿名遥测（Sentry/自托管，默认关） |
| **诊断命令** | `cmd_get_diagnostics`（返回版本/平台/日志路径/engine manifest/node 缓存/最近错误） |
| **健康端点** | supervisor 探测 `:4567/health`（inkos 内置）+ 超时/重试日志 |

**不做**（Phase 3）：实时性能监控（CPU/内存 profiling）、分布式追踪。

**优先级**：HIGH（生产问题定位的前提）

---

### M4b：E2E 回归测试（End-to-End Regression）

**目标**：关键路径自动化，CI 门禁防破坏性变更。

| 范围 | 交付 |
|---|---|
| **启动冒烟** | app 启动 → node bootstrap → sidecar 就绪 → SPA 200 → observer 接线（超时 60s 失败） |
| **updater 契约** | 模拟 GitHub latest.json → 解析版本 → 下载 mock bundle → SHA256 校验 → 原子替换验证 |
| **secrets 契约** | keychain 写 → 读 → 删除 → env 注入验证（跨平台：macOS/Linux/Windows） |
| **CI 集成** | desktop-e2e.yml（三平台 × 冒烟，PR 必过门禁） |

**测试框架**：Rust `#[test]` + 真实 binary（`cargo build --release` 产物）。

**不做**（Phase 3）：UI 自动化（Playwright/Selenium）、LLM 写作流 E2E（需 API key opt-in）。

**优先级**：HIGH（防回归的唯一保障）

---

### M4c：生产签名（Production Code Signing）

**目标**：消除操作系统安全警告，提升用户信任。

| 平台 | M3 现状 | M4c 目标 |
|---|---|---|
| **macOS** | ad-hoc 签名（Gatekeeper 警告"未验证开发者"） | Apple Developer ID 签名 + 公证（notarization） |
| **Windows** | 未签名（SmartScreen "未知发布者"警告） | Authenticode 签名（EV 或 OV 证书） |
| **Linux** | 无签名要求 | 保持现状（AppImage/deb 校验和） |

**CI secrets gating**（M3e 已准备）：
- `APPLE_CERTIFICATE` / `APPLE_CERTIFICATE_PASSWORD` / `APPLE_ID` / `APPLE_TEAM_ID`
- `WINDOWS_CERTIFICATE` / `WINDOWS_CERTIFICATE_PASSWORD`

**交付**：
1. 采购证书指引（Apple $99/年，Windows EV ~$300/年）
2. CI 配置更新（tauri-action 传签名参数）
3. 签名验证脚本（`codesign -dv` / `signtool verify`）

**不做**（Phase 3）：自动续期、HSM 硬件密钥。

**优先级**：MEDIUM（付费门槛，可选升级；M3 ad-hoc 已可分发）

---

### M4d：用户文档（User Documentation）

**目标**：自助安装、使用、故障排查，降低支持成本。

| 文档 | 内容 |
|---|---|
| **README.md** | 项目介绍、特性、架构图（γ 模式）、快速开始、构建指引 |
| **INSTALL.md** | 三平台安装指引（下载 → 解压/安装 → 首次运行 → 权限授予 → 签名警告处理） |
| **USER_GUIDE.md** | 使用指南（项目选择 → LLM key 配置 → 写作流 → 快捷键 → 托盘菜单 → 更新） |
| **TROUBLESHOOTING.md** | 常见问题（端口占用 → node 下载失败 → sidecar 启动超时 → keychain 权限 → 日志位置） |
| **CONTRIBUTING.md** | 贡献指引（开发环境 → 构建 → 测试 → PR 流程 → 代码规范） |

**多语言**（opt-in）：中/英双语（README/INSTALL 优先；其他按需）。

**不做**（Phase 3）：视频教程、交互式引导、in-app help。

**优先级**：HIGH（用户自助的前提）

---

### M4e：错误体验优化（Error Experience）

**目标**：优雅降级、自愈、友好提示，提升可用性。

| 场景 | M3 现状 | M4e 改进 |
|---|---|---|
| **node 下载失败** | panic / 静默失败 | 重试 3 次 → 切换 mirror → 友好错误"网络问题，请检查连接" + 手动重试按钮 |
| **sidecar 启动超时** | 30s 超时 → 空白窗口 | 进度条 + 日志尾部显示 + "重启 sidecar" 按钮 |
| **端口占用** | 递增端口成功但无提示 | Toast 通知"4567 被占用，已切换到 4568" |
| **keychain 拒绝** | 返回错误但无引导 | "keychain 访问被拒绝，请在系统偏好设置授权" + 打开系统设置链接 |
| **engine 更新失败** | 回滚成功但无说明 | "更新失败（SHA 不匹配），已回滚到 v1.2.3" + 查看日志链接 |
| **磁盘满** | 下载/解压失败 → 崩溃 | "磁盘空间不足（需 500MB），请清理后重试" |

**实现**：
1. 错误分类（网络/权限/资源/配置）+ 统一 `UserFacingError` trait
2. 前端错误边界（React Error Boundary）+ 降级 UI
3. 自愈逻辑（自动重试 × 3 + mirror 切换 + 缓存清理）

**不做**（Phase 3）：AI 驱动的错误诊断、自动修复建议。

**优先级**：HIGH（用户留存的关键）

---

### M4f：SEA 优化（Single Executable Application）

**目标**：engine bundle 体积减半（~200MB → ~100MB），提升下载/更新体验。

**背景**：
- M3 engine bundle = 便携 node 22（~50MB）+ dist/（~30MB）+ **node_modules/（~120MB，主要体积）**
- node_modules 含大量非运行时文件（.ts 源码、.d.ts、README、tests/）

**方案**：Node.js SEA（Single Executable Application，Node 20+ 实验性特性）

| 步骤 | 操作 |
|---|---|
| 1. 生成 blob | `node --experimental-sea-config sea-config.json` → 生成 `sea-prep.blob`（包含 dist/ + 精简 node_modules） |
| 2. 注入 binary | `node` binary + `sea-prep.blob` → 单文件可执行（postject 工具注入）|
| 3. 签名 | macOS `codesign` / Windows `signtool` 重签（SEA 修改 binary 后需重签） |

**体积对比**（预估）：
- M3：node(50MB) + dist(30MB) + node_modules(120MB) = **200MB**
- M4f SEA：单文件可执行（含 blob） = **~100MB**（压缩后 ~40MB）

**风险**：
- Node.js SEA 仍为实验性（Node 20+），API 可能变动
- 需验证 inkos 所有依赖可 bundle（native addon 需单独处理）
- 调试复杂度增加（blob 内文件不可直接访问）

**fallback**：SEA 失败则保持 M3 方案（node + dist/ + node_modules），不阻断发布。

**不做**（Phase 3）：Bun/Deno 替代 Node、WASM 打包。

**优先级**：LOW（优化项，非阻断；M3 体积可接受）

---

## 3. Phase 2 实施顺序与依赖

```
M4a（可观测性）─────────────┐
                           ├──→ M4b（E2E 回归）─────→ M4e（错误体验）
M4d（用户文档）────────────┘                              │
                                                         ├──→ Phase 2 验收
M4c（生产签名）────────────────────────────────────────────┤
                                                         │
M4f（SEA 优化）──────────────────────────────────────────┘
```

**推荐顺序**：
1. **M4a**（可观测性）—— E2E 和错误优化的诊断基础
2. **M4b**（E2E 回归）—— 并行 M4d（文档）
3. **M4e**（错误体验）—— 基于 M4a 日志优化
4. **M4c**（生产签名）—— 独立，可最后（付费可选）
5. **M4f**（SEA 优化）—— 独立，风险高可延后 Phase 3

---

## 4. 验收标准（Phase 2 完成定义）

| 维度 | 标准 |
|---|---|
| **可观测** | ✅ tracing 日志滚动 7 天 + 诊断命令返回完整信息 + panic hook 生成 crash dump |
| **回归** | ✅ 启动/updater/secrets E2E 通过 + CI 三平台门禁绿 |
| **签名** | ✅ macOS notarized + Windows Authenticode（或文档指引 + CI gating 就绪） |
| **文档** | ✅ README/INSTALL/USER_GUIDE/TROUBLESHOOTING/CONTRIBUTING 五文档齐全 |
| **错误体验** | ✅ 六大场景（node 下载/sidecar 超时/端口占用/keychain/更新失败/磁盘满）友好降级 |
| **SEA** | ✅ 单文件可执行生成 + 体积 <120MB（或文档说明 fallback 原因） |
| **零回归** | ✅ M3 所有功能仍工作（143 测试 + M3 冒烟全绿） |

**最终交付**：tag `v0.4.0-phase2`，GitHub Release 含三平台签名包 + 完整文档。

---

## 5. Phase 2 vs Phase 3 边界

**Phase 2（生产就绪）**：
- 核心路径稳定（E2E 保障）
- 问题可定位（日志 + 诊断）
- 用户可自助（文档 + 友好错误）
- 分发无警告（签名，可选）
- 体积优化（SEA，可选）

**Phase 3（高级特性，延后）**：
- 实时性能监控（CPU/内存 profiling）
- UI 自动化测试（Playwright）
- LLM 写作流 E2E（需 key opt-in）
- 多语言完整支持
- AI 错误诊断
- HSM 硬件密钥
- Bun/Deno/WASM 替代
- 插件系统
- 多项目并行

---

## 6. 风险与缓解

| 风险 | 概率 | 影响 | 缓解 |
|---|---|---|---|
| SEA 实验性 API 变动 | 中 | 中 | fallback M3 方案，Phase 2 不强依赖 |
| 签名证书采购延期 | 中 | 低 | CI gating 预留，ad-hoc 可先发布 |
| E2E 在 CI 环境 flaky | 高 | 中 | 重试 3 次 + 隔离环境 + 超时加长 |
| 错误场景遗漏 | 中 | 中 | 基于 M4a 日志真实用户反馈迭代 |

---

## 7. 资源与时间估算

| 里程碑 | 工作量（人天） | 关键路径 |
|---|---|---|
| M4a 可观测性 | 3-5 | 是（M4b/M4e 前置） |
| M4b E2E 回归 | 5-7 | 是（门禁） |
| M4c 生产签名 | 2-3（+ 证书采购时间） | 否（可选） |
| M4d 用户文档 | 3-4 | 否（可并行） |
| M4e 错误体验 | 4-6 | 是（用户留存） |
| M4f SEA 优化 | 5-8（+ 验证时间） | 否（可延后） |
| **总计** | **22-33 人天** | 关键路径 12-18 天 |

---

## 8. 设计自检

- [x] 每模块单一职责（可观测/测试/签名/文档/错误/SEA 互不耦合）
- [x] 失败可降级（SEA 失败→M3 方案；签名未配→ad-hoc；E2E flaky→人工复查）
- [x] 零破坏性（M3 功能全保留）
- [x] 文档先行（TROUBLESHOOTING 指导 M4e 错误设计）
- [x] 可增量交付（M4a-f 可独立发布，不互相阻断）
- [x] 7 轴对齐（最优/兼容/扩展/安全/高性能/0GC/可实施）

---

## 9. 下一步

1. **用户批准** Phase 2 设计（6 子里程碑范围 + 优先级 + 验收标准）
2. **编写实施计划**（M4a-f 各一份，TDD task 分解）
3. **启动 M4a**（可观测性，关键路径首项）

