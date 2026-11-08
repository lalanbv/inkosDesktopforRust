# 947 号 · works 备份端到端验证轮：真数据 6 条目入包实证 + 引擎启动 source_sync 竞态发现备案——净结论

> 938 works 备份补缺的真实数据端到端验证（fixture 之外的终极实证）：
> alternation --keep 真现场（双引擎交替写出的真 works 数据）→起引擎→
> backup/export→tar 条目核查——**works/alt-work 全量 6 条目入包实证
> （revisions 章节 r1/r2/rev-*、cover-prompt、draft json 全在）**。
> 过程中发现并定性一个已知形态：引擎首起 source_sync 异步窗口。零代
> 码改动轮。
> 验证：端到端 works 条目实证；gate:ts 十步全绿（当日 914-947 十四轮
> 连绿）。

## 一、端到端验证过程（含一次假阴性排除）

1. alternation --keep 留真现场（33/33 互证后的真 works 数据）。
2. 首次起引擎 curl backup/export：3270 bytes/9 条目且 **works=0**——
   假阴性（见下定性）。
3. 重起引擎二次 curl：**works/alt-work 全量 6 条目入包**（revisions/
   chapter r1/r2/rev-*、cover-prompt rev、draft rev json）——938 修复
   真实有效实证。

## 二、假阴性定性（引擎启动 source_sync 异步窗口）

- alternation 脚本手工放置的 works 形态（仅 revisions/）被引擎首起的
  source_sync（884 快照补齐：补 work.json/source/快照）**异步补齐
  中**——首次 curl 时 sync 未完成 → export 不含 works。
- 真实用户场景无此窗口（引擎长运行、works 由管线/交互写入——非
  手工放置后秒备份）。
- 备案：引擎刚启动立即 backup 可能拿到 source_sync 完成前的快照
  （下次启动再同步幂等恢复）——已知形态备案，非本轮患。

## 三、数据形态发现（备案）

- alternation 真现场 works/alt-work = revisions/（多版本工件）+
  source/ + work.json（sync 补齐后）——**真 works 数据主体在
  revisions/ 多版本工件**（不止 work.json 单文件）——备份域 works/
  目录整体复制（copy_recursive）正确覆盖多版本形态 ✓（938 的目录级
  复制设计经受住真实形态检验）。

## 四、验证

- 端到端 works 条目实证（6 条）；gate:ts 十步全绿；零代码改动轮。

**下一号自 948 起；出口清空回归常备；用户侧待办=Fork 推送 64 个提交（至 cc375e2f）。**
