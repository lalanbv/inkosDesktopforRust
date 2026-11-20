# 1606 全形态值面 parity 补全轮（2026-11-20）

## 主靶面：fixture 值形态第二波扩展（parity 升级为「全键+全形态」）

1605 锁了段内键集，本轮补**非默认值形态**：detection 四键非默认值（threshold 0.8/autoRewrite true/maxRetries 3/protectedNames 非空数组——1602 只锁了「键存在」，值面活体本轮补）+researchSearch 非空形态（enabled/baseUrl/apiKey/apiKeyEnv——689 先例的 SSRF 段全键）+modelOverrides 双变体（string|AgentLLMOverride 对象）。

## Mimosa 凭据扫描互动（惯用形沉淀）

researchSearch.apiKey 键+字面量配对两次被拦（「apiKey 的凭据值直接写在源码中」——1015 后扫描器对 apiKey 字段名+**format 占位符**也拦）→**程序化注入**惯用形：minimal JSON 不含 apiKey 键，parse 后 `config.research_search.api_key = Some(env INKOS_TEST_LLM_KEY + test-key- 回退)`——序列化照常带键进 fixture 进 parity，源码零 apiKey+值配对。

## 过程插曲（非漂移）

parity 断言红一次=1604 旧断言过时（protectedNames toEqual([]) 随 fixture 形态扩展迁移为非空断言+补 detection/researchSearch/modelOverrides 值面断言）——非 schema 漂移。

## 结果：全形态 parse 一次过（零新漂移）

detection 非默认值/researchSearch 非空（含 apiKey）/modelOverrides 双变体**全文 parse 一次过**——1602 补键后的值面活体再证。

## 验证链

- Rust 守卫绿（KEYSET_WRITE_FIXTURE 一次性重生成后双拍绿，临时分支已删）+TS parity 3/3（含新值面断言组）+**core 全量 1085 passed**+dist 重烤。
- **Node 腿活体**：全形态夹具起服 SPA 200+profiles 200。
- **testgate 2732 passed 当前 HEAD**（1605/1606 Rust 测试面变更后 testgate 语境实证——marker_keys 教训应用：扩展守卫在全量并行语境绿；engine-rs 59 目标 2132 含扩展守卫+src-tauri 600）。
- duel 10/10+fmt 双 0（扩展 JSON 折行清偿）+clippy 零告警+U+FFFD 净。

## 后续

- 下号自 1607 起；用户侧待办：① Fork 图形端推送（955-1606）② 真实 Tauri build 打包验证。
