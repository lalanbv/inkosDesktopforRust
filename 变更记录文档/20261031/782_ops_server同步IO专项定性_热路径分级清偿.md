# 782 号：ops/server 同步 IO 专项定性——热路径分级清偿（757 号演进路线 #3 清偿）

日期：2026-10-31。前置：757 号演进路线五条之 #3「同型 `std::fs` 调用面值得一次专项定性（小文件本地 IO 实际风险低，**按热路径分级而非全量改写**）」、779/780 号（双栈原子写收口）。

## 一、普查（分类器全量，禁 head 截断）

`std::fs::` 全仓 grep 343 处 → 生产/测试分类器（首个 `#[cfg(test)]`+`mod` 分界）后**生产站 59 处/14 文件**（测试站 284 豁免）。逐站追宿主函数 async-ness 与**调用链**定性（宿主 sync ≠ 安全——sync 助手被 async 链拉起照样阻塞 runtime）。

关键链路实锤：`run_export`/`run_import` 已走 `spawn_blocking`（设计正确）；`serve_asset` 已 `tokio::fs`；`tool_grep_book` 全程 tokio::fs；static 面两站是**启动期路由装配一次性读**（非每请求——装配时机不查则定性失真）。

## 二、分级台账（59 站闭合）

### Tier A（真清偿，转换 2 站）
1. **`interaction/project_tools.rs` `tool_grep`→`grep_walk`**：async 工具分发内同步遍历**整棵项目树**（全仓唯一大 IO 同步面在 async 里）→ `spawn_blocking` 挪出（walk 只做 read_dir+收集，panic 面无；逐文件读取本就走 tokio::fs）。
2. **`server/backup_routes.rs` 恢复前自动快照 tar**：`File::create` 直写终名+无 fsync——快照是恢复的**回滚保险件**，「恢复中掉电」窗口内撕裂 tar=保险失效，与 779 号已硬化的恢复目标本体不对称 → tmp（`.pre-restore-<ms>.tmp` 隐藏名）+`sync_all` 后 rename（762 号同款「落盘后才 rename」）+失败清理 tmp（对齐 save_secrets 惯用形；tar::Builder 直拥句柄故手搓而非 write_file_atomic_bytes）。

### Tier B（设计已在位，豁免）
- backup_routes 22 站主体：run_export/run_import 经 `spawn_blocking`，helper 在 blocking 线程内合法。
- static_routes 54/55（启动装配一次性）、serve_asset（tokio::fs）、index_body（每请求一次 stat+缓存命中零读，µs 级）。
- atomic_file_set 4 站：sync 助手本体+best-effort 父目录 fsync（779 号基础设施）。
- spill 2 站：`Permissions::from_mode` 构造器零 IO（普查误报）；local_search 1 站：构造器冷路径。
- 句柄型合法惯用形：play append_json_line+session_transcript（append+flush，68 号）、spill（create_new 唯一名+目录 0700 包容）、log_file append、export/film_routes（内存 encoder）、static 125（RwLock 误报）。

### Tier C（justify 收口：小文件×用户/管理操作频率，改造收益<成本）
- composer 6 站：async compose 管线内小 JSON 读（LLM 秒级管线中 KB 级读=噪声）。
- references 4 站、secrets/service_routes/ops_routes series_canon、play restore、log_file 轮转（罕见有界）、books mtime_of、on_log append（同步回调签名，改造>收益）。
- **secrets `save_secrets`：非漏网**——221/227/762 号三重硬化完整惯用形（uuid 唯一名+create_new 防符号链接+0600 创建即权限+sync_all+rename+父 fsync+失败清理），**优于**共享助手（0600 语义 write_file_atomic_bytes 不具备），转换反而降级。

### 相邻面备案
rusqlite `Connection::open`（play_graph sqlite 后端）同步但 play 域用户操作频率+本地小库，超出 757 号 std::fs 定界，备案不动；`thread::sleep` 仅测试与 sync 助手退避。

## 三、句柄型写补充普查（779 号普查盲区验证）

779 号主普查 grep 模式抓不到「builder 流式直写 `File::create`」形态——补充普查 `.write_all(`/`.write(`/`OpenOptions`/`File::create` 生产面 23 处：**确证漏网恰 1 站**（快照 tar，已转换）；其余全为合法惯用形或基础设施（详见 Tier B/C）。「94 站全分类清偿」宣告在句柄类上补普查后维持成立。

## 四、验证（全绿）

- 定向：project_tools 3/3（read_ls_grep_roundtrip 真走 spawn_blocking 路径）+ backup_routes 7/7（`import_confirm_writes_back_and_snapshots_overwritten` 断言 backups 目录恰 1 文件+tar 内容回读=快照零残留回归锁）。
- testgate **1950+583 零漂移**；clippy 双 0；gate:ts 八步全绿含契约差分 0 分歧（rust-bin 新鲜度闸过）。

## 五、教训

1. **普查模式必须含句柄型写**（`OpenOptions`/`File::create`+`write_all`/builder 流式）——`std::fs::write` 形态 grep 会给「全清偿」假象；补普查成本一次脚本，漏网代价是保险件失效级。
2. **定性产出=分级台账而非全量改写**（757 号原意兑现）——59 站逐站落档（Tier A 转换/B 豁免/C justify/相邻备案），后续轮可按台账复核而非重新普查。
3. **std::fs 站点定性须查装配时机**——static 面两站在启动装配路径上，不查会误判为每请求热路径（反向失真同样存在）。
