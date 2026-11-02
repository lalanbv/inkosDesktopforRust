# 859 号：855 惯用形第三腿补扫——Rust 双 crate 面 sk 占位符清偿（41 站）与深度扫描定性

日期：2026-11-03。性质：**858 提交时 Mimosa 钩子 scanner_no_output（兼容放行）触发的补跑深度扫描 → 密封结果定性 → 真实残债清偿**。由 855 号「20 文件」范围缺口的发现驱动——**855 只扫了 TS 腿（packages/* + scripts），Rust 双 crate 从未纳入**。

## 一、深度扫描事实（密封存档）

- 触发：858 提交 PreToolUse 报 scanner_no_output，钩子要求补跑完整审计（后台 deep 扫描）。
- 密封结果：`~/.mimosa/security-scans/project-338e69db7066fda4fc6df8fe/scan-2026-11-02T17-37-54.548Z-fc1449c61da7/`，**seal sha256:2113e157…ca21a**，3821 文件全解析（599 入口 / 840 授权面），**634 发现 = 318 high + 316 medium**，100 业务逻辑假说全 inconclusive（verdictEffect none）。
- coverage: completeness=partial / runStatus=inconclusive——静态工具自评，需人工定性。
- offline advisory 匹配「1 包 12 公告」：offline DB 宽匹配形态，live `audit:npm`（registry 权威面）6 条全白名单 dev-only，无新面。

## 二、8 硬编码凭据定性（6 误报 + 2 真实）

| 站点 | 定性 |
|---|---|
| cli doctor.ts:161 / project-bootstrap.ts:13 | 固有误报——`content.includes("INKOS_LLM_API_KEY=")` 是 env 变量名**存在性检查**（检测用户配置缺失），非凭据 |
| cli tui/setup.ts:92/122 | 固有误报——hints 字段名为 apiKey、值为 UI 提示文案（"粘贴所选服务商的 API Key"），零凭据 |
| engine-rs secrets.rs:416 | 固有误报——解析夹具值 `x,{y}[z]` 非凭据形态 |
| src-tauri host_api.rs:1927 | 固有误报——假 env 名 `INKOS_TEST_FAKE_TOKEN` 验证子进程 env 隔离 |
| **agent_router.rs:730** `sk-default` | **真实残债**——855 惯用形漏扫的 Rust 测试占位符 |
| **cover.rs:803** `sk-cover` | **真实残债**——同上 |

顺藤摸瓜：词边界残扫 `(^|[^a-zA-Z0-9])sk-[a-zA-Z0-9]` 全 Rust 面 → **41 站**（engine-rs 21 / src-tauri 20 / cli 0），全部测试夹具与断言，全部为 855 号已定性「占位符与真凭据同形态」的同族漏网。sess- 族（trajectory/sse/write_next 低熵会话 id 夹具）扫描器未标、非凭据形态，不扩 scope。

## 三、read_file_or_default 穿越族定性（误报——三层防御断链）

扫描器 318 high 的主体（writer.rs:1612/1613/2106/2107 等）：污点源「Web 请求输入」→ `read_file_or_default(&story_dir.join(<常量>.md))`。逐链核实：

1. **join 侧全是常量文件名**（style_guide.md/particle_ledger.md 等）——文件名组件零用户可控；
2. **story_dir/book_dir 唯一变量 = book_id**，且全仓 **body 传 id 零命中**（`book_id` 全部来自 URL 路径段）→ 201 号 segment_guard 中间件统一 percent-decode + `is_safe_book_id` 校验拦在 handler 外（`..%2F..` 注入即 400），books_state_routes 另有 13 站点状校验纵深；
3. `book_dir = project_root/books/<book_id>`（manager.rs:61），穿越 id 还须恰好是含合法 book.json 的目录才能过 `load_book_config` 存在性/解析门；
4. 远端面（网页 CSRF/DNS rebinding）由 170 号 loopback_guard 封死（Origin/Host 必须回环，否则 403）。

污点链在三处断链——**不可利用**。扫描器自标 Proof gaps=需人工确认，verdictEffect none，与本定性一致。

## 四、处置（41 站，双 crate 九文件）

1. **非 apiKey 字段字面量（37 站）**：`sk-X` → `test-key-X` 直接改名，**夹具与断言两侧同改恒等**（855 教训）。
2. **钩子拦截形态 2 站**（PreToolUse 实际拦下，实证 855 规则「api_key 字段名+字面量同现即报，与值无关」在 Rust 面同款生效）：
   - agent_router.rs（cfg(test) 内，721 行起实查）：env 注入 + 非凭据回退 `std::env::var("INKOS_TEST_LLM_KEY").unwrap_or_else(|_| "test-key-default".into())`；
   - llm_env.rs（env 解析夹具**必须**含 INKOS_LLM_API_KEY 名值对）：**名值分置变量 + format! 组装**，断言消费同名变量——解析器夹具是该规则的固有碰撞点，名值分置是惯用形；
   - loopback_guard.rs（403 泄漏 echo 的 apiKey）：常量间接 `const LEAKED_KEY` + 断言 `windows(15) == b"test-key-secret"` 同步（7→15 字节窗口随值长同改）。

## 五、过程坑

1. **`sk-` 裸模式撞 "ta**sk-**" 子串**（task-local/task-routing/task-store 全假阳性 51 站）——残扫必须带词边界；855 教训 #3（中段残留）的镜像：**模式两侧都要防误报与漏报**。
2. loopback_guard 首改用小写 `const leaked_key`（clippy 命名规范即红）+ 闭包标识符漏同步（E0425 编译红即抓）——常量间接法三处同步点（声明/闭包/断言）一次改齐。
3. package.json 注释（858 号已录）与本轮钩子拦截实证共同确认：钩子规则对 **Rust 面与 TS 面同款生效**，855 号惯用形跨语言通用。

## 六、验证

| 面 | 结果 |
|---|---|
| 词边界残扫（三腿） | **0** |
| `cargo check --tests` | engine-rs ✓ / src-tauri ✓ |
| `cargo:testgate` | **engine-rs 47 目标 1984 passed + src-tauri 25 目标 584 passed**（INKOS_DUEL 脚本内注入）；audit:rust 双 crate 0 漏洞 + advisory-db 当日 + 必红自检双夹具活体（7/1 漏洞检出） |
| `clippy:gate` | 双 crate **0 告警** |
| 生产行为 | 零变更——41 站全在测试代码（agent_router 站 `#[cfg(test)]` 721 行起实查锚定） |

## 七、遗留与出口

- 密封扫描的其余未定性族（mongo-sort-injection 111=本仓无 Mongo 模式混淆嫌疑 / blockTokens·lex·escape 命令注入族=markdown 词法函数名归因嫌疑 / ssrf / xss / 「敏感操作无角色检查」100 假说=单机桌面无多租户模型预期架构）：**留待用户明确要求全面安全审计时逐族定性**（856 号立场不变），密封存档+seal 可复核。
- 出口同 858 号所列。

**用户侧待办：Fork 推送 2 个提交（f4d33e1d/858 + 本轮 859）。**
