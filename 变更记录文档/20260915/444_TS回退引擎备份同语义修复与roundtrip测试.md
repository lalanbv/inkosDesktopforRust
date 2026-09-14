# 444 号：TS 回退引擎备份同语义修复 + 导出→导入 roundtrip 测试补缺

- **日期**：2026-09-15
- **类型**：fix(studio) —— 回退引擎侧备份断裂对齐 + 覆盖缺口补测
- **关联**：386 号（R18 TS 原始实现）、443 号（Rust 移植与「能工作契约」定型）
- **提交**：`packages/studio/src/api/server.ts` + `server.test.ts` + 本记录

## 1. 选题与覆盖缺口定位

443 号在 Rust 侧坐实 R18 三处「产后搁浅」断裂后，TS 回退引擎（164 号选择序中的 miss 回退路径）仍带同款两处断裂。同时发现 **server.test.ts 对备份端点零测试覆盖**——这正是三处断裂（manifest 裸名查找/前缀写回/面板线格式）从未被抓住的根因。

## 2. 修复（server.ts，与 Rust backup_routes.rs 同契约）

1. **manifest 查找**：`inkos-backup/backup-manifest.json`（导出实际产物名）优先、裸名兜底——此前裸名精确查找永不命中，自家导出包导入必 400 `backup-manifest.json missing`。
2. **恢复路径剥包根前缀**：覆盖检测与写回均落项目根相对路径——此前按归档名原样写回 `root/inkos-backup/...`，恢复永远落不进真实书目。
3. **manifest 元数据不落盘**：写回跳过 manifest 条目，`restored` 计实际写回文件数（与 Rust `restored` 语义一致）。

## 3. 测试补缺（server.test.ts 新增 describe「backup export/import roundtrip (444 号)」）

- **roundtrip 主测**：临时根 + b1 书 fixture → `GET /backup/export`（断言 gzip 魔数 0x1f8b + content-type）→ 破坏 `books/b1/book.json` → POST 预览（断言 overwriting 含 `books/b1/book.json`、fileCount>2）→ `?confirm=1`（断言 ok/snapshot/restored>1 且 < fileCount——manifest 不计）→ 写回内容为导出时版本 + `backups/pre-restore-*.tar` 快照含损坏版本。
- **manifest 缺失拒绝**：无 manifest 的 tar → 400 逐字 `backup-manifest.json missing`。
- **回归价值**：roundtrip 主测在修复前必挂（manifest 永不命中 400）——正是 386 号缺失的那道闸。

## 4. 门禁

- studio：vitest 97 文件 **829 用例全绿**（+2 新测）+ `tsc --noEmit` 净（修一处 Buffer→Uint8Array BodyInit 类型）+ `pnpm build` 全量（含 build:server）。
- Rust 零改动（443 门禁沿用）。

## 5. 遗留

- **待推送：433–444 共 12 笔**，请用户在 Fork 图形端推送后核验 origin/develop。
- 备份双引擎至此同契约：桌面默认 Rust 路径（443）与 Node 回退路径（本号）行为一致。
- 下一个编号自 **445** 起。
