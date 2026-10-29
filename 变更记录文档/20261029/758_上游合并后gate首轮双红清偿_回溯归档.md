# 758 号：上游合并后 gate 首轮双红清偿（typecheck + audit）——回溯归档

> 本档为回溯补录（759 号会话发现 758 号提交 `21b53d48` 未随附归档文档，
> 以提交信息为唯一事实源重建，不含提交信息之外的新主张）。

## 一、背景

b952a25a 合并 upstream/codex/short-review-evidence（25 提交，73 文件）后
gate:ts 首轮两红：typecheck 红 + audit:npm 红。

## 二、typecheck 红：ExportDownloadPreview href 类型

上游直写 `buildApiUrl` 返回值进 `href`；fork 的 `use-api` 签名为
`string | null`（上游版为 `string`）。按同文件 665 号先例显式守卫（恒非空
模板下 null 分支不可达仍守卫）。文件：
`packages/studio/src/components/chat/ToolExecutionSteps.tsx`。

## 三、audit 红：npm 公告库环境漂移 12 条

归因实锤：merge diff 无 package.json/lockfile 触碰 = 合并零依赖变化，
全部为公告库新收录。逐条定性三类处置：

- **override 升补丁线（5 包）**：undici 三 major 各达各线
  （6.29.1 / 7.30.1 / 8.12.3；688 号旧线 `^6.28.1`/`^7.29.1` 被新公告越过，
  8.x 为新增键）；fast-uri `^3.1.9`；qs `^6.16.1`（均为 688 号旧线被越过）；
  body-parser 新增键 `^2.4.0`（express 5.2.1 传入 2.3.0）。
- **白名单配对理由（2 包）**：deepmerge（low，无补丁线 patched<0.0.0，
  4.3.1 即终态，501 号 braces 同型；streamdown→rehype-sanitize 静态配置
  合并面）；zod（high，补丁线 4.6.6 跨 major、3.x 无补丁，MCP/Anthropic
  SDK peer 契约待随上游，688 号 vitest 同型；单机桌面无远程攻击面）。

## 四、终态

gate:ts 八步全绿复跑（build 15.1s / typecheck 14.2s / test 90s / audit 3s）
+ pnpm-lock 三线实达（6.29.1 / 7.30.1 / 8.12.3 唯一解析）。

## 五、方法论（759 号沿用）

公告库漂移与代码漂移同权：audit 红先归因（合并引入 vs 环境漂移）再逐条
定性；白名单零静默放行维护规则不变。
