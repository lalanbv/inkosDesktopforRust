# 252 号：duel 测试污染生产 dist/index.html——测试基建修复

- 日期：2026-09-09
- 分支：develop
- 关联：124 号（静态面对跑——本批修其副作用）、172 号（duel 基建）
- 推送核验：origin/develop 仍停 6bc65564——**201–251 共 52 提交待推送**；本批随 252 后本地领先 53。两项默认值无新答复。

## 一、缺陷

strangler_duel 的 `spawn_ts_sidecar` **无条件覆盖** `packages/studio/dist/index.html` 为 duel 探测页（`<!doctype html><title>duel</title>`，34 字节）——每次 duel 跑完，生产构建产物即被污染：真实引擎（INKOS_STATIC_DIR=dist）浏览器直连返回 duel 占位页（白屏）。242 号走查使用的 dist 恰好是 vite build 后、duel 跑前的窗口，掩盖了此问题；252 号走查（引擎重启后）暴露。

## 二、修复

`spawn_ts_sidecar` 改为**仅在 dist/index.html 缺失时**写占位（保留「跳过 vite 自动构建」原意）；生产 dist/index.html 存在时不触碰。duel 静态面对跑断言为 bin/TS **互相等价**比较（非固定内容），两侧 serve 生产 index.html 后测试语义不变。

## 三、验证

- 恢复生产 dist（vite build:client）→ duel 真跑 10/10 → **跑后 dist/index.html 保持生产内容**（修复前会被覆盖）。
- 其余门禁维持已绿。

## 四、遗留

无新增。
