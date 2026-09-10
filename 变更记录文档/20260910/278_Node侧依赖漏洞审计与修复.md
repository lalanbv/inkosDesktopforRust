# 278 号：Node 侧依赖漏洞审计与修复——136→23 条，critical 清零

- 日期：2026-09-10
- 分支：develop
- 关联：263 号（Rust 侧 cargo audit 0 漏洞——本批补齐 Node 侧对称面）、277 号（发现 pnpm.overrides 被静默忽略的 WARN）
- 编号衔接：查当日目录最大号 277，顺延 278
- 推送核验：origin/develop = a1b34a33 未动；本地领先 274–278 共 5 提交待推

## 一、审计方法

`pnpm audit` 默认走 npmmirror 镜像——**该镜像无 audit 端点**（`ERR_PNPM_AUDIT_ENDPOINT_NOT_EXISTS`），须显式 `--registry=https://registry.npmjs.org/`（环境坑备案）。修复前 prod 依赖树 **136 条**：critical 1 / high 40 / moderate 80 / low 10。

## 二、发现

| 类别 | 内容 |
|---|---|
| **critical** | `protobufjs <7.5.5` 任意代码执行——路径 `core > pi-ai(0.67.1 精确锁) > @google/genai > protobufjs`，无法经升级 pi-ai 消除 |
| **high 主力** | `undici` 25（core 直依赖精确 pin 6.21.3 + pi-ai 链 7.x 传递）、`hono` 30（TS server 框架，lockfile 冻结在 ^4.7.0 旧解析）、`dompurify` 14 + `mermaid` 9（studio UI 渲染链） |
| **既存配置失效** | 根 package.json 的 `pnpm.overrides`（pi-ai 双 pin）被 pnpm 11 **静默忽略**（v10 起迁往 pnpm-workspace.yaml）——实装版本未漂移仅因各消费包都是精确直 pin，属侥幸 |

## 三、修复（pnpm-workspace.yaml overrides 迁移 + 定向升级）

1. **overrides 迁到 `pnpm-workspace.yaml`**（pnpm 11 唯一生效位置）：`protobufjs ^7.5.8`（critical）、`mermaid ^11.16.1`、`dompurify ^3.4.13`、`js-yaml ^4.3.2`、`undici@7 → 7.29.0`（范围选择器，不动 core 的 6.x 直依赖）、`postcss@8 ^8.5.18`、`ws@8 ^8.21.0`、`fast-uri@3 ^3.1.6`——除 protobufjs 外均为同主版本内 patch 级，全部用范围选择器避免跨大版本强跳。
2. **直接依赖**：core `undici 6.21.3→6.28.0`（精确 pin 保持仓库风格）、`js-yaml ^4.1.1→^4.3.2`；studio 的 caret 约束经 `pnpm -r update` 上浮（hono ^4.13.7 / @hono/node-server ^1.19.17 / nanoid ^5.1.16 / streamdown ^2.6.0）。

## 四、效果与验证

| 指标 | 前 | 后 |
|---|---|---|
| 总建议数 | 136 | **23**（-83%） |
| critical | 1 | **0** |
| high | 40 | 13 |

**门禁全绿**：studio build ✓ / `pnpm -r typecheck` ✓ / `pnpm -r test` 全过（core 202 + studio 90 + cli 42 文件）/ **Playwright 36/36**（mermaid/dompurify/streamdown 均为 UI 渲染链依赖，e2e 全过证明零回归）。

## 五、残留 23 条定性（不继续强修的理由）

`brace-expansion` 8（跨 2/3/4/5 四个主版本区间的传递链，强覆写需跨主版本跳）、`picomatch`/`browserslist`/`@babel/core` 等（**shadcn/@dotenvx 构建期工具链**，不随产品分发）、`basic-ftp`/`ip-address`/`fast-xml-builder`（被精确锁定的 pi-ai 0.67.1 内部，协议客户端在桌面场景基本不触达）。上述继续下压的唯一途径是升级 pi-ai/shadcn/epub-gen-memory 主版本——与上游 v1.8.0 冻结策略冲突，留上游对齐时一并处理。

## 六、遗留

274–278 共 5 提交待推送；root package.json 仍为并行会话在途（其死字段 `pnpm` 清理顺延至其落库后）；两项默认值、ja A/B、历史瘦身待用户决策。
