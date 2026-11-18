# 1577 check-routes 自检机制化 + doctor 真人走查轮（2026-11-19）

## 定性

按 /goal 全量测试：1576 对 check-routes 正则的修复只有单次绿态验证、无持续自检——按 847 号教训（解析型门禁须配必红自检，check-routes 是解析型门禁却从未配过）补齐机制化自检；doctor 体检页桌面级真人走查四拍（该 UI 面此前从未走过）。基线门禁链零信号。

## 修复（1 处）

**`scripts/check-routes.mjs` 机制化自检**（每次主跑前执行，走同一解析路径）：
- 解析函数抽出导出（`parseRegistered`/`scanHandlerNames`/`diffOrphans`），主流程与自检共用——847 号「自检走同一解析路径」精神。
- **4 夹具精确集合断言**（854 号断言宽度精神）：①必红例（未注册真 handler 必须检出，防恒绿）；②吞尾例（1576 事故回归哨：普通 `pub async fn` 不得误报 + 其后真 handler 不得被吞，正则回潮裸 `.*?` 即红）；③绿例（注册齐全必须 0 孤儿）；④注册解析哨（modText 无注册行时同夹具必须报孤儿，防注册解析死链假绿）。
- 自检失败 exit 3——「本次孤儿=0 结论作废」语义；**自检自身红路证明**：临时回潮正则 → exit 3 精确捕获（`total=1 orphans=[plain_helper]`）→ 复原绿。

## 真人级走查（doctor 体检页，生产形态 Rust 引擎）

环境：`inkos-engine-server`（debug，rust-bin 腿新鲜指纹）+ mock Ollama（真 11434，`/v1/models`）+ 临时项目 root（`INKOS_PROJECT_ROOT`）+ `INKOS_LLM_BASE_URL=http://127.0.0.1:11434/v1`。

| 拍 | 形态 | 期望 | 实测 |
|---|---|---|---|
| 1 | Rust API 正常 env | llmConnected:true | ✅ Rust 完整形态响应（retrieval/bookIssues 齐） |
| 2 | **死代理 env 重启**（HTTPS_PROXY=127.0.0.1:9319 无监听） | 仍 true（doctor_models_probe 旁路运行时生效，修复前必败） | ✅ true + mock 日志第 3 笔 `GET /v1/models` 直达 |
| 3 | 死代理 env + 非回环 radar override（192.168.1.10:8080/v1） | false（env 代理对非回环真实生效对照） | ✅ false |
| 4 | IAB 浏览器 doctor 页（环境诊断） | 体检卡渲染+交互 | ✅ 七行体检卡全渲染、「重新检查」refetch 正常、零控制台异常 |

期间核验一个易混点：`dist/index.js`（Node 回退腿）的 doctor 是 legacy 形态（无 retrieval/bookIssues，不读 Rust bin 的 env 通道）——本轮修复面在 Rust 侧，走查主证用 Rust bin；Node 回退腿服务 SPA 供 UI 渲染拍。

## 验证

- 基线：sweep 23/23、cargo:testgate 双 crate 2723 passed；收口：clippy 0、fmt 0、gate:ts 十腿全绿（check-routes 0.1s 绿含新自检）；U+FFFD 净。
- 走查环境收尾：3 进程停、IAB 标签关、临时 root 清、无孤儿残留。

## 后续

- 下号自 1578 起；用户侧待办不变（Fork 推送 955-1577 + 正式打包三脚本重烤）。
