# 515 号：三活体套件 fetch 超时加固与进程组清理

日期：2026-09-17（工作起于 09-16，跨日收口）
范围：`scripts/node-fallback-smoke.mjs` / `scripts/engine-contract-diff.mjs` / `scripts/export-epub-smoke.mjs`（纯测试基建，零产品代码改动）

## 背景

499 号 pi-ai 0.73 升级（拖入 aws-sdk 巨图）后，tsx 冷启动偶发超过套件原有 30s 启动等待；本地引擎偶发挂起时，套件内无超时的 `fetch` 会无限期阻塞，表现为 CI/本地门禁"卡死"而非失败。本轮对三个活体套件做统一的可靠性加固。

## 改动

### 1. fetchT 超时包装（三套件统一）

```js
// 515 号：本地引擎挂起时快速失败——统一 20s 超时（SSE 长连接除外）。
const fetchT = (input, init = {}) => fetch(input, { ...init, signal: AbortSignal.timeout(20_000) });
```

- 全部 `await fetch(` → `await fetchT(`；本地引擎 20s 无响应即失败，快速暴露而非挂死。
- **SSE 长连接除外**：`engine-contract-diff.mjs` 的事件收集器保留原 `fetch` + 自身 `AbortController`（收集需长挂，20s 会误杀）。
- 若 init 已带 `signal` 则被覆盖——三套件内除 SSE 收集器外无此用法，安全。

### 2. 子进程清理升级：SIGTERM → SIGKILL + 进程组

- 根因：`spawn` 的 node 腿是 `tsx` 包装进程，SIGTERM 只杀 tsx 父进程，**孙进程（axum/express 服务器）不可达**→ 僵尸存活占端口，下次套件启动 `EADDRINUSE`。
- 修复：`spawn(..., { detached: true })` 建独立进程组，清理改 `process.kill(-child.pid, "SIGKILL")`（负 pid = 杀整组）。SIGKILL 不可被忽略，孙进程必死。

### 3. node 腿启动等待 30s → 90s

- `node-fallback-smoke.mjs` 与 `export-epub-smoke.mjs` 的启动轮询超时放宽到 90s，吸收 pi-ai 0.73 后 tsx 冷启动的长尾（实测冷启 >30s 出现过）。

### 4. 修复过程引入并当场修复的缺陷：fetchT 误插进 python 模板串

- `export-epub-smoke.mjs` 的 helper 插入逻辑用"最后一个以 import 开头的行"作锚点，落在了 `VALIDATE_PY` **python 模板字符串内部**（模板里 `import xml.etree.ElementTree as ET` 是最后一条 import）——JS 运行时 `fetchT is not defined`，node 腿启动等待失败。
- 修复：摘除模板串内误插行，helper 移至 JS 层 `scriptDir` 锚点前（import 块之后）。
- **教训入册**：文本插入锚点必须排除模板字符串内部；`grep "^import"` 会命中模板串内的他语言 import 行。锚点应选结构性唯一标识（如 `const scriptDir =`），并在插入后立即 `node --check` + grep 复核。

## 验证

- `node --check` 三脚本语法通过；模板串内 0 处 fetchT 残留（3 处均在 JS 层：1 定义 + 2 调用）。
- 三活体套件全绿：
  - `export-epub-smoke.mjs`：node 腿 + rust 腿各 4 断言（export-save 200 / EPUB 落盘 / 214 号结构校验）。
  - `node-fallback-smoke.mjs`：双引擎一致性冒烟 8/8（fixture 一致性 / director PUT / task-routing / no-store 双面 / run-log）。
  - `engine-contract-diff.mjs`：37 端点 0 分歧 + SSE 事件名集合双端一致。
- `pnpm gate:ts:fast` 全绿（typecheck 17.2s / test 67.0s / audit 1.8s）。

## 关联

- 485/486（套件固化）、487/488/490（契约差分器）、492（SSE 差分）、494（EPUB 冒烟）、499（pi-ai 0.73——启动长尾根因）、509（gate:ts——三套件的门禁入口）。
- cargo 链接仍被 Xcode 许可阻断（`sudo xcodebuild -license accept` 待用户执行），待清算队列不变：481 全量 cargo test、489/500 号 4 新单测、duel、bench:gate。
