# 956 号 · 完全回滚语义轮：快照 introduced 清单 + 可选删除 + P9 书目同步

> 951 备案续篇（常备出口「新真实需求」唯一在册缺口）：「恢复引入的新文件
> 不在快照中（快照只含被覆盖文件），保守不引入删除语义，完整回滚需手工
> 清理新文件」——955 已把回滚做成 UI 可执行闭环，但语义仍是**部分回滚**
> （只恢复被覆盖文件，恢复期间新增的文件残留=项目处于混合态）。本轮补齐
> 完全回滚：快照 manifest 记撤销决策面（三数组），rollback 端点
> deleteIntroduced=1 时写回后一并删除恢复期间新增文件，**每份待删文件删除
> 前留档=完全回滚自身也有保险**。
> 验证：TS backup-routes 29/29（+9）+interaction 10/10（+4）+Rust
> backup_routes 31/31（+9）+testgate engine-rs 2078/src-tauri 584+clippy
> 双 0+audit:rust 双 0（必红自检活体）+gate:ts 十步全绿（engine-contract-
> diff 含新探针零漂移）+U+FFFD 净。

## 一、选点与设计

- 开场三件套：上游零落后（领先 0/本地领先 1478 含未推送批次）；audit:npm
  全绿（6 白名单+自检 4 夹具活）；backlog 无在册挂起。951 备案续篇为唯一
  产品缺口，真实场景=误恢复含已删书的旧备份后想「完全撤销这次恢复」。
- **快照 manifest 撤销决策面（三数组恒在，形状统一）**：
  - `overwriting`=被覆盖名单（写回语义——撤销时须恢复的内容）
  - `introduced`=本次操作新建名单（可选删除——恢复引入文件）
  - `removing`=本次将删的已留档名单（仅 Delete 模式非空；撤销=写回其
    删除前内容=精确逆运算）
  - 双端对齐（Rust 侧此前连 overwriting 都没有，本轮补齐）。
- **IntroducedMode 三态**：import 端点恒 `Ignore`（外部包恢复永不透传不
  删除——契约面不变，差分器 import 探针零漂移保持）；rollback 端点
  `Keep`（保守回滚，预览透传清单确认不删）/`Delete`（deleteIntroduced=1
  完全回滚）。Delete 模式 preview=confirm 同一先验（**预览预言确认结果**）。
- **删除面三面同一**：留档面=删除面=回滚删除面同一集合（不留档不删）。
  待删文件仅常规文件可删（lstat 不跟随，与 955 回滚面同判据；目录/链接/
  特殊文件报 deleteFailed 不删）；已不存在=终态已达成跳过不计；读取失败
  留档失败同样报 deleteFailed（用户要求删除而未删，不得静默）。
- **删除语义纯新增扩展**：此前 overwriting 空=无快照=纯新增恢复无任何回
  滚保险；现在改变盘面即留快照（覆盖/新建/删除任一非空），纯新增快照唯
  一条目=自身 manifest（记 introduced 名单），完全回滚=删除名单文件。
- **删除面先验于盘面**：lacks/unsafe 校验在写回前（写回一旦发生不可回退，
  先验失败须整体 400 无副作用——无新快照无 tmp 残留）。响应保持 200+
  deleteFailed 明细（写回已成功，谎报 500 会诱导用户重试=重复快照+写回）。
- **向后兼容**：955- 旧快照（manifest 无 introduced）保守回滚照常工作
  （Keep 模式不查清单）；Delete 模式 400 `snapshot manifest lacks
  introduced list`（用户要求删而快照无清单，静默忽略会误导）。

## 二、删除面安全（共享 golden 向量表）

- introduced 清单来自 manifest JSON 自由字符串，**不经 tar 条目安全三则**
  ——不安全路径若放行可删 root 外任意文件（unlink 对绝对/穿越名直达盘外）。
- 新增 `is_safe_relative_path`/`isSafeRelativePath`（纯串级跨平台一致）：
  非空；无 NUL；无反斜杠（Windows 分隔符语义保守拒）；不以 / 开头；不匹
  配 `^[A-Za-z]:`（盘符绝对 C:/x 与盘符相对 C:x 一并拒）；分段后任一段不
  为空串/./..（保守整拒不归一化）。双端断言同一向量表
  `backup-introduced-path-vectors.json`（9 accept/20 reject，Rust
  include_str/TS readFileSync，955 快照名向量同款先例）。
- 先验失败 400 `unsafe introduced path: <path>`（Delete 模式 preview 与
  confirm 同一先验；Keep 模式宽容展示不透传不 400）。

## 三、UI（BackupPanel）

- 回滚预览 introduced 非空时显示「恢复期间新增 N 个文件（勾选可一并删除，
  完全回到快照时刻）」+清单+复选框「同时删除这些新增文件（删除前自动留
  档）」——**默认不勾**（保守语义不变，删除是显式选择）；换快照预览重置
  勾选（不同快照清单不同，不得跨快照残留）。
- confirm 勾选时带 `&deleteIntroduced=1`；成功通知含「删除 N 个恢复期间
  新增文件」；deleteFailed 非空追加「K 个删除失败（前三路径）」（仍绿色
  ——主体成功，部分失败诚实可见）。

## 四、测试与必红

- Rust +9（快照决策面精确/纯新增留快照/预览透传+import 永不透传/完全回
  滚闭环含保险快照自洽与撤销/保守语义不变/旧快照 lacks 先验+向后兼容/
  golden 向量+篡改先验+keep 宽容/外部删除终态/符号链接 deleteFailed）。
  插曲：5 用例首跑红为**测试期望错非实现错**（backups 目录非空断言误写
  is_empty/保险快照 manifest 分列误并 overwriting/外部删除用例误用含覆
  盖面包/symlink 前未移除恢复写出的常规文件）——修正后 31/31。
- TS +9 同形对齐（端点级 app.request 走真 server）。interaction +4
  （清单展示+默认不勾+不带参/勾选带参+通知含计数与失败明细/无清单不渲
  染+换快照重置勾选/P9 失效广播）。插曲：vi.mock 需补 invalidateApiPaths
  导出（8 红根因）。
- 必红三形：Rust NEUTER956_DELETE 中和删除循环恰 1 红（removed 0≠1 行为
  红）；NEUTER956_MANIFEST 中和 manifest 退化恰 6 红（3 决策面断言红+
  3 完全回滚 lacks 400 行为红）；恢复 31/31 绿+NEUTER 残留 0。TS stash
  双修复文件恰 8 红（7 条端点级行为红：manifest 无 introduced/400 缺失/
  snapshot false 等）恢复 29/29。UI stash 面板恰 4 红（3 完全回滚+P9）
  恢复 10/10。

## 五、真引擎活体（生产形态 env，8904/8906 新构建）

- 误恢复场景全流程：备份含 b2→误删 b2+改 b1→import 预览（newFiles=6=
  b2 六文件，955 断言保持：fileCount=newFiles+overwriting、restored=
  预览总数）→confirm 产恢复留档快照→rollback preview Keep 透传
  introduced=6→confirm deleteIntroduced=1 返回 `{restored:7,removed:6,
  snapshot:true}`→终态 b2 再删 b1 回恢复前书目→**回滚自身保险快照保守
  回滚 restored=13 复活 b2**（撤销完全回滚闭环）。备案：删除面只删文件
  不删空目录（b2 目录残留无 book.json 不入书目——保守语义延续 951）。
- 守卫面：legacy 快照 Delete 400 lacks（盘面零变更无新快照）+篡改快照
  preview/confirm 双 400 unsafe+evil 未写盘+legacy 保守回滚 restored=1。
- 插曲：活体首跑在 11:02 旧二进制（955 代码）上得出「Rust 未透传/未删」
  ——重建二进制后复跑全通；教训=活体前核验二进制 mtime 新于源码改动
  （rust-bin 门禁有同源核验但只管 gate 时刻）。

## 六、GUI 真人走查反挖（IAB，8905/8906 新引擎+生产 bundle）

- 完全回滚链路全通：回滚预览渲染 introduced 区块与复选框→勾选→确认→
  通知含「删除 2 个恢复期间新增文件」→服务端 removed=2→b2 从盘面消失。
- 自动化插曲（**非产品缺陷**）：首轮用「原生 setter+change 冒泡」勾选，
  DOM checked 变 true 但 React state 未更新（React 受控 checkbox 的 onChange
  监听 click 事件而非 change），confirm 未带参——fetch 记录器定位后改 DOM
  级 `el.click()`（真实 click 事件=真人点击等价）全链正确。走查惯用形
  「受控 select 改值用原生 setter+change」不适用于 checkbox/radio。
- **P9 书目列表不同步（走查反挖）**：完全回滚删除 b2 后，书籍列表（UI
  书目 API 缓存）仍显示 b2，与服务端 books API 不一致——恢复/回滚会增删
  书，但 postApi 的自动失效面不含 backup 路由（deriveInvalidationPaths
  对其返回空），BackupPanel 走 fetchJson 不触发任何失效。修复=确认恢复
  与确认回滚成功后显式 `invalidateApiPaths(["/api/v1/books"])`；interaction
  +1 锁（监听 inkos:api-invalidate 事件断言）；P9 必红=stash 面板恰红；
  8906 复验 UI 书目=API 书目同步。
- 环境清偿：引擎杀净（pgrep 0）/临时目录清/IAB 标签关（tabs=[]）。

## 七、门禁与结构

- 差分器 +1 条目：rollback `?name=pre-restore-1.tar&deleteIntroduced=1`
  守卫探针（合法形态缺失先 400 snapshot not found——形态守卫先于盘面，
  删除语义不触达），engine-contract-diff 十步零漂移。
- gate:ts 十步全绿；testgate engine-rs 2078（+9）/src-tauri 584；clippy
  双 0；audit:rust 双 0+必红自检活体；U+FFFD 净；提交 8 文件
  （+1174/-74）。

## 八、结论

951 备案的「部分回滚边界」清偿：备份域从「回滚保险可留可执行可操作」
进化到「完全回滚可执行且自身有保险」——任意一次恢复操作（覆盖+新增+删
除）都有精确逆运算，撤销语义在快照 manifest 决策面（三数组）中自描述；
删除面三面同一（留档=删除=回滚删除），不安全路径 golden 向量表双端锁死，
先验于盘面变更。GUI 走查第二只眼再度反挖出 UI 层缺陷（P9 书目同步——
静态审查与 curl 活体都看不见），三层审查（静态→活体→GUI）纪律持续有效。

**下一号自 957 起；出口清空回归常备；用户侧待办=Fork 推送（含 948-956）。**
