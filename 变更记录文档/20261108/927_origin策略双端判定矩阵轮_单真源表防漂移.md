# 927 号 · origin 策略双端判定矩阵轮：单真源对照表（39 用例）+ 双端同读机械锁——零漂移实证

> 924 声明「cover_origin.rs 与 TS cover-download-origin.ts 同表同语义」
> 但无机械验证（各自单测的用例表是两份手写表=漂移温床）。本轮建**单
> 真源对照表**（fixtures/cover-origin-cases.json，39 用例）+ 双端同读
> 同断言（TS cover-origin-matrix.test.ts / engine-rs tests/
> cover_origin_matrix.rs include_str 同表）——**双端 39 条同判定机械
> 验证完成（零漂移实证）**。works 域金标准「双端逐字共用」形态在
> origin 策略面的应用。
> 验证：TS 矩阵 1/1（39 条）+Rust 矩阵 1/1（39 条）+testgate
> engine-rs 52 目标 **2054**（+1 新目标）/584+gate:ts 十步全绿+U+FFFD
> 净。

## 一、对照表设计

- 单真源：`packages/core/src/__tests__/fixtures/cover-origin-cases.json`
  ——provider 锚+39 用例（url/policy/verdict/reason/extraOrigins/
  note），仅收**零网络确定性形态**（scheme/same-origin/IP 字面量
  v4 12 形+v6 7 形/十进制十六进制变形/域名规则/allowlist/strict/
  invalid fail-closed）；DNS 解析面（rebinding/公网域名）由各端单测
  注入 stub 覆盖，不进表。
- 双端同读：TS fs.read+Rust include_str 同一 JSON。

## 二、矩阵测试（双端各 1 用例聚合断言）

- 逐条跑 `assertCoverDownloadUrlAllowed`（TS Promise reject reason/
  Rust Result Err 消息取括号内 reason）→ mismatches 收集 → 空断言
  （失败时全量列出漂移条目）。
- **实证**：39 条双端同判定零漂移；表修正一处设计错（https://127.0.0.1:9000
  是合法 scheme 但异 origin——实判 private 拒，臆想的 scheme-not-allowed
  不成立——**对照表需反映正确语义而非臆想**，矩阵测试当场抓出）。

## 三、验证汇总

- TS cover-origin-matrix 1/1（39 条）；Rust cover_origin_matrix 1/1
  （39 条，新 tests 目标入 cargo:testgate=52 目标 **2054**（+1））；
  gate:ts 十步全绿；U+FFFD 净。

## 四、后续出口

- origin 策略双端镜像声明从「入档声明」升级为「机械锁验证」——后续
  任一端策略改动跑矩阵即暴露漂移。
- 出口清空回归常备。

**下一号自 928 起；出口清空回归常备；用户侧待办=Fork 推送 60 个提交。**
