# 918 号 · 门禁文档对齐轮：README 测试段补门禁全家桶 + 模块级 README 快扫净

> 912/913 文档对齐收尾：914/915 门禁演进（八步→十步）后根 README 测试
> 段未反映——补「门禁脚本（提交/发版前全量）」段（gate:ts 十步明细/
> cargo:testgate/clippy:gate/audit:rust/audit:npm/bench:gate 六命令+
> 各自语义一行）。模块级 README（engine-rs/src-tauri/tests 三件）时效
> 扫：works/L0/日期声明零命中=无漂移净。
> 验证：gate:ts 十步全绿（当日 914/915/916/917/918 五轮连绿）；U+FFFD
> 净；零代码改动。

## 一、README 测试段补齐

- 原段仅列 5 条裸命令（cargo test×2/engine cargo test/pnpm test/
  llvm-cov），**门禁脚本全家桶缺失**——贡献者/未来会话看不到
  gate:ts（十步：build/typecheck/全量测试/npm 审计/rust 二进制新鲜
  度闸/双引擎一致性冒烟/契约差分三域活体/EPUB 导出冒烟/works 交替
  读写互证/路由注册完整性哨）与 cargo:testgate/clippy:gate/audit:
  rust/audit:npm/bench:gate 的存在与语义。补「门禁脚本（提交/发版
  前全量）」段：六命令+各一行语义（gate:ts 十步逐条注释）。

## 二、模块级 README 快扫净

- engine-rs/README.md / src-tauri/README.md / src-tauri/tests/
  README.md：works/L0/M1b/2026-日期声明 grep 零命中=无过时时效声明
  （engine-rs README 为「引擎移植目标与纪律」性质非状态快照，无漂移
  面）。

## 三、验证

- gate:ts 十步全绿（work-alternation 5.2s/check-routes 0.1s 新哨在
  位）；U+FFFD 净；零代码改动。

**下一号自 919 起；出口清空回归常备；用户侧待办=Fork 推送 52 个提交。**
