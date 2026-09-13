# 429 · 依赖健康专项——audit 剩余条目批量 override 修复

日期：2026-09-14　性质：依赖健康专项批（427 号备案执行）　前置：428 号 vite override 先例

## 内容

`pnpm-workspace.yaml` overrides 批量精确钉 6 个漏洞模块到修复版（428 模式
推广）：

| override | 修复 advisory |
| --- | --- |
| brace-expansion@2.0.2 → ^2.1.4 | ReDoS（high+moderate，epub-gen>ejs / shadcn>ts-morph 链，8 条） |
| picomatch@4.0.3 → ^4.0.7 | ReDoS（vitest mocker 链，2 条） |
| browserslist@4.28.1 → ^4.28.8 | 高危数据（shadcn 链，2 条 high） |
| basic-ftp@5.2.2 → ^5.3.1 | DoS×2（pi-ai 链，high×2——本产品无 FTP 使用面） |
| fast-xml-builder@1.1.4 → ^1.1.7 | 属性注入（pi-ai 链，high） |
| ip-address@10.1.0 → ^10.7.0 | SSRF/解析（pi-ai 链，high+moderate） |

## 验证

- audit：26 → **12 条**（high 13→3 / moderate 9→5 / low 4）——**本会话累计
  31→12，消除 19 条**；剩余 12 条 = vitest 4.1.11 大版本升级项（3 moderate）
  + low 级 4 条 + ai 链残余（备案，升级牵动面大）；
- 回归：core 245 文件 2112 用例全绿、studio 97 文件 823 用例全绿、双 tsc
  净（brace-expansion / picomatch 为 glob 高频工具库，升级后全量测试零回归）；
- install 需官方源（npmmirror 同款教训）。

## 教训

- pnpm overrides 的修复版本值必须实际存在（`^4.0.14` 无此版本 → override
  静默失效），写完后以 audit 复跑确认为准。
