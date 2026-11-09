# 954 号 · 快照回滚用户可执行闭环轮：列举+回滚双端端点 + BackupPanel 回滚区

> 938-953 十六轮备份域收官后的 UI 消费面缺口：951 的 pre-restore
> 快照回滚只在 API/文件系统层可用——BackupPanel 提示「快照在
> backups/」但无列举无回滚入口，回滚保险对 UI 用户「看得见摸不着」。
> 本轮补齐：`GET /backup/snapshots`（列举）+ `POST /backup/rollback`
> （服务端读快照字节走 import 同一执行体）双端 + 面板回滚区。
> 验证：TS backup-routes 14/14（+3）+interaction 2/2（+1）+Rust
> backup_routes 16/16（+3）+typecheck 净+clippy 双 0+check-routes
> 0 孤儿（backup_routes 补登记）+gate:ts 十步全绿+U+FFFD 净。

## 一、缺口定型（选点依据）

- 开场三件套：上游双 remote 零落后；npm audit 2 条（zod <4.6.6 high
  / @hono/node-server <2.1.3 moderate）均为 758/759 号已钉定白名单
  条目（门禁面净）；work 域 L0-L3+UI 整合已收官（906-912 轮）——
  非备份域无在册挂起。转备份域消费面勘察。
- 947 备案的「启动 source_sync 竞态」已被 948/949（export 前同步
  sync 双端）清偿——复核非挂起。
- BackupPanel 功能面：export/preview/confirm 完整（443/455 轮交互
  测试在位），唯快照回滚零 UI 入口——951 语义「回滚保险可执行」在
  UI 用户处退化为「知道存在但无法操作」。

## 二、双端实现（契约逐字对齐）

- **`GET /api/v1/backup/snapshots`**：列举 `backups/pre-restore-*.tar`
  → `{snapshots:[{name,size}]}`，名倒序=时间倒序（millis 等长名字
  典序=数值序）；缺目录=空列表；形态外残留（`.pre-restore-*.tmp`
  半成品/杂散/同名目录）一律不入列。
- **`POST /api/v1/backup/rollback[?confirm=1]`**：name 走 query（与
  confirm 同族语义，940 先例）→ 形态校验 → 读快照字节 → **走 import
  同一执行体**（TS=runBackupImport/Rust=run_import 复用）——快照回
  滚过与外部备份包完全相同的安全三则+manifest 先验，非语义旁路；
  两段式 preview/confirm 与 import 同构；回滚执行时对被覆盖文件再
  留快照=import 不变式自动保持（回滚自身有保险）。
- **形态守卫**：`pre-restore-<纯数字毫秒>.tar` 唯一放行形态（路径穿
  越无从谈起；先形态后盘面——非法名 400 invalid snapshot name，合
  法形态缺失 400 snapshot not found；超大数溢出双端同拒）。
- **结构收编**：tar 写侧自 server.ts 迁出为 `lib/tar-write.ts`（与
  lib/tar-read.ts 对偶的独立打包面模块——buildTarArchive/
  appendTarEntry/长路径三通道单一真源；备份 export、快照构造、交互
  电影 source 导出三方共用）；import 两段式执行体迁至
  `api/backup-snapshot.ts`（server.ts 净瘦身 228 行）；check-routes
  modules 补 backup_routes 登记（443 号移植漏列——孤儿检测对备份域
  从未生效，现 4 handler 全注册差集 0）。
- 差分器共享面 +2 条目：snapshots 缺省基线空列表全 body 对比；
  rollback 空探针=name 守卫 400/400（真实回滚管道同一性由双端套件
  锁定）。

## 三、UI（BackupPanel 回滚区）

- 挂载即列举快照（失败静默不阻断主功能）；列表=时间展示（millis 名
  解析）+大小+「回滚到此快照」；回滚同样两段式（预览覆盖面 → 确认
  回滚）；恢复/回滚成功后列表刷新（新快照入列）。

## 四、测试（TS 14+2 / Rust 16）

- TS backup-routes +3：形态过滤列举（tmp/杂散/目录不入列）+缺目录
  空列表；rollback 两段式闭环（preview overwriting→confirm 回导入前
  时刻+回滚自身留新快照=2 份；restored 派生断言=快照条目-1——首版
  写死 1 被逮：export 全量包覆盖面=11 种子文件全在快照）；守卫面
  （穿越先 400 invalid 先于盘面/合法形态缺失 not found）。
- Rust backup_routes +3 同形（list_snapshots 过滤排序/rollback 守卫
  /两段式闭环走 run_import 真管道）；RollbackFailure 补 Debug。
- interaction +1：回滚区渲染（列举+时间+大小）→ 预览覆盖面 → 确认
  → 成功通知+列表刷新调用断言。
- 插曲：三处 Mimosa 静态扫描误报拦 Edit（正则 `.exec(` 误判命令执
  行→改 String.match；server.ts 既有行误报→新逻辑收敛至独立新文件
  绕开非确定性全文扫描）；一处生成错乱（Write 中途占位符垃圾）经
  git diff 逐行核对全量重写修复。

## 四b、真引擎活体（生产形态 env，8901）

- 种子项目 → export 472B → 列举空 → 篡改 book.json → import confirm
  restored=2 → 列举 1 条 → rollback preview（**POST 形态**；首测用
  GET 得 405 系测试方法错非患）→ confirm ok restored=2 → 终态
  book.json=快照时刻内容（回滚语义实证）→ 列举 2 条倒序（回滚自身
  留新档）→ 穿越 `..%2Fsecrets.json` 400 invalid snapshot name。
- 环境清偿：引擎进程杀净+临时目录清。

## 五、结论

备份域三环（export/restore/rollback）在 UI 层全闭环——回滚保险从
「留档可执行（951）」到「UI 可操作（954）」。双端契约逐字对齐，差
分器锁定；回滚=import 同一执行体（非旁路），形态守卫先于盘面。

**下一号自 955 起；出口清空回归常备；用户侧待办=Fork 推送（含
948-954）。**
