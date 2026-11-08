# 938 号 · 备份域补缺轮：works 创作数据入备份导出（双端同修）——用户数据丢失风险面清偿

> 真实深察（数据安全面）：backup/export 的 BACKUP_ROOT_ENTRIES=
> ["books","inkos.json",".inkos","prompt"] **不含 `works/`**——而
> works 域用户创作数据存 `works/<id>/work.json`（work_store.rs 头注
> 自证）=**用户创作的 works 作品不进备份导出**（换机/重装→导入恢复
> →作品全丢——「最安全可靠」硬要求的数据安全核心患）。**双端同患**
> （TS server.ts:5905+Rust backup_routes.rs:38 同清单）；import 侧为
> 通用 staging 复制天然支持 works 条目（恢复端无需改动）——不对称：
> 「能恢复手工塞的，自家导出永远不含」。
> 修复=双端 BACKUP_ROOT_ENTRIES 加 "works"（一行/端）+双端测试断言
> （works 种子+inkos-backup/works/w1/work.json 条目在位）。
> 验证：TS backup-routes 4/4（works 条目断言过）+Rust backup_routes
> 7/7+testgate 2054/584+clippy 双 0+gate:ts 十步全绿+U+FFFD 三文件净。

## 一、患定性

- works 域 = 纯 Rust 桌面创作主路径（881 号：「纯 Rust 桌面端到端价
  值的真正所在」）——其数据（work.json+工件）不在备份域=**数据安全
  核心患**（备份的存在意义就是防数据丢失）。
- import 侧无患：通用 staging 复制按 tar 条目恢复（无白名单限制），
  手工塞 works 条目可恢复——export 缺口为唯一患面。
- 修复=export 域补缺（一行/端，双端同改）。

## 二、实现与测试

- Rust BACKUP_ROOT_ENTRIES [&str;4]→[&str;5] 加 "works"+注释（含
  import 端无需改动说明）；TS `as const` 同加+同注释。
- TS 测试：seedProject 加 works/w1/work.json 种子+export 条目断言
  `inkos-backup/works/w1/work.json` 在位——4/4 绿。
- Rust 测试：fixture 加 works 种子+`names.contains("inkos-backup/
  works/w1/work.json")` 断言——7/7 绿。

## 三、验证汇总

- cargo:testgate engine-rs 52 目标 **2054**/src-tauri 584；clippy 双
  0；gate:ts 十步全绿（当日 914-938 十三轮连绿）；U+FFFD 三文件净。

## 四、后续出口

- works 备份域补缺=数据安全面清偿（备份恢复闭环：export 含 works→
  import 通用复制恢复）。
- 出口清空回归常备。

**下一号自 939 起；出口清空回归常备；用户侧待办=Fork 推送 64 个提交。**
