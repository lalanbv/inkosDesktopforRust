# 461 号：hash 路由穷举一致性根治——PAGE_SPEC 单一事实表（460 备案落地）

- **日期**：2026-09-15
- **类型**:refactor(studio) + test —— 缺陷类根治（类型系统级闸门）
- **关联**：460 号（analytics 深链修复与备案）、208/405 号（同款前两实例）
- **提交**：`packages/studio/src/hooks/use-hash-route.ts` + `use-hash-route.test.ts` + 本记录

## 1. 根治设计：路由三件套收敛为单一穷举事实表

460 号 analytics 深链断裂是该类缺陷第三实例（208 chapter、405 radar）——根因是 `parseHash`/`routeToHash`/`HASH_PAGES` 三处独立维护，新增页面漏配任何一处都无编译信号。本次重构为**单一事实表**：

```ts
const PAGE_SPEC: { readonly [K in HashRoute["page"]]: PageSpecFor<K> | null } = { ... }
```

- **键穷举**：mapped type 遍历 `HashRoute["page"]` 联合——新增页面而漏配此表 = TS2322 编译错（studio tsc 门禁即拦）；
- `toHash`/`sample` 按页窄化（`Extract<HashRoute, {page:K}>`），lambda 参数类型自动正确；
- `writable` 镜像旧 HASH_PAGES 语义（onboarding/radar 可产 hash 供深链解析但不写 URL，逐字保留）；
- `HASH_PAGES` 改为从表派生（writable=true 过滤），消灭双处维护；
- `routeToHash` 退化为查表（`PAGE_SPEC[route.page]?.toHash(route) ?? ""`），doctor/genres/style/truth/daemon/logs 七个 state-only 页显式 null；
- round-trip 测试逐页断言 `parseHash(toHash(sample)) === sample`——「有写入无解析」不对称被永久锁定。

## 2. 测试（+3 用例，37 全绿）

1. **穷举往返**：遍历 PAGE_SPEC 非 null 页面，`toHash(sample) → parseHash` 必须还原 sample（analytics 类缺陷的通用闸）；
2. **writable=false 语义保持**：onboarding/radar 不写 URL 但解析分支在（深链直达可用）；
3. **键集合断言**：26 页键集合与期望全量清单排序相等。

**穷举闸红绿实证**：临时删除 analytics 键 → tsc 立即报错 + 「键集合」与「往返」两测全红；恢复后全绿。

## 3. 门禁

- studio：vitest **106 文件 857 用例全绿**（+3）+ `tsc --noEmit` 净 + dist 重建。
- 测试侧类型备忘：spec 为 `PageSpecFor<K>` 联合时 `toHash(sample)` 参数取交集为 never——测试侧 `spec.sample as never` 断言（调用方保证配对）；`routeToHash` 内查表后显式宽化。
- Rust 零改动。

## 4. 遗留

- **待推送：433–461 共 29 笔**，请用户在 Fork 图形端推送后核验 origin/develop。
- 下一个编号自 **462** 起。
