# 990 号：e2e_secrets 九枚专项清偿轮——ACL 授权框根因修复（unique_service）+ sweep 全量收编 23 枚 + 豁免族归零

日期：2026-11-12。988/989 挂起清偿三部曲收官轮。开工三查净（工作区空、990 空闲、无并行会话；HEAD=d36b7f02）。

## ① 根因分析：旧 ignore 理由「首次访问弹授权框」的真实机制

988/989 两轮豁免 e2e_secrets 的理由是「dark wake 时首次访问弹授权框阻塞」——本轮深挖发现真实机制不同：9 枚旧测试用**固定 service 名**（`inkos-e2e-*`），keychain 条目跨运行持久，而 macOS keychain ACL 按**创建时二进制签名**授权访问者——重构建后的测试二进制（cdhash 变化）访问旧二进制创建的持久条目 = 跨签名访问 → securityd 弹授权框 → 无人值守挂死。实证对照：lib 四枚 keyring 测试（988 号实跑绿）用 `unique_service`（PID+纳秒唯一命名空间），同进程内创建+访问 = 无跨签名面，从不弹框。dark wake/keychain 锁定是另一重环境门控（securityd 调用阻塞），与弹框无关。

## ② 修复：unique_service + cleanup + ignore 理由勘误

- `unique_service(label)`：对齐 store.rs lib 测试的实证形（`__test__inkosDesktop_e2e_{label}_{pid}_{nanos}`），9 枚逐测替换固定名；并行安全性随唯一命名空间顺带获得（旧注释「固定名防 `__index__` 互踩」的设计动机被唯一性替代）。
- `cleanup(store)`：收尾逐键删除（走公开 trait 面 `read_all`→`delete`；`entry_for`/`read_index` 是模块私有集成测不可达——残留一枚空 `__index__` 于唯一命名空间内无害，与 lib 测试的模块内深度清理差异头注释备案）。
- `#[ignore]` **保留**（环境门控语义不变）：CI/headless 无 keychain、keychain 锁定时 securityd 阻塞——只是弹框根因已除。9 枚 ignore 理由文案逐条勘误为新表述。

## ③ 活体：9/9 绿 ×2 + sweep 全量 23/23

- 探针跑（120s timeout 哨兵防万一挂死）：**9/9 绿 18.59s，零弹框零挂死**；稳态复测 9/9 绿 18.55s。
- sweep 收编第十 scope（`--test e2e_secrets`，新增 `extraTestArgs: ["--test-threads=1"]` 机制——keychain 串行惯例，且修正 scope 串接缺陷：原 `--` 后拼接会破坏 libtest 参数解析，现走独立字段 concat）：**23/23 全绿 exit 0，src-tauri 全部挂起用例全量收编，豁免族归零**（988 lib+watcher 10 枚 → 989 桌面 e2e 4 枚 → 990 keychain 契约 9 枚）。

## ④ 门禁

clippy:gate 双 0；cargo:testgate src-tauri 25 目标 584 passed 持平 + audit:rust 双 0；engine-rs/TS 树零改动（引用 988/987）；U+FFFD 净。提交路径限定三文件：`src-tauri/tests/e2e_secrets.rs` / `scripts/cargo-ignored-sweep.mjs` / 本文档。

## 备案

1. e2e_secrets 仍要求桌面解锁会话（keychain 锁定时 sweep 该 scope 会阻塞——securityd 阻塞无 timeout，sweep 无内层哨兵属已知边界，无人值守场景由 CI 环境无 keychain 自然禁跑）。
2. `cleanup` 残留空 `__index__` 条目属设计豁免（见②）；若未来 keychain 条目枚举工具面世可深化。
3. 挂起用例三部曲（988/989/990）总结：23 枚全部清偿或收编，零豁免残留；「被 ignore 的测试路径=零观测盲区」族债双栈（Rust+TS）全清。

下一号自 991 起；出口回归常备；用户侧待办 = Fork 推送（含 955-990）。
