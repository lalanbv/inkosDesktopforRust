# 348 号：embedding 客户端双协议+增量向量化契约（G1 续）

- 日期：2026-09-12
- 类型：双端基础设施（TS core + Rust 引擎，golden 向量先行）
- 规划依据：[328 规划](../20260911/328_对标调研分析与改进规划.md) G1 续；契约层见 [347 号](347_语义检索层契约.md)
- 范围说明：本号交付 **embedding 双协议客户端 + chunk 指纹/增量选择契约**；
  `retrieval_chunks` 落库表、write-next 检索链切换与召回面板为 349 号
  （落库+检索端点+链路切换是同一闭环，不拆散）。

## 做了什么

1. **EmbeddingConfig + 双协议客户端**（TS `retrieval/embedding-client.ts` /
   Rust `utils/semantic_retrieval.rs` 内嵌）：
   - `openai-compatible`：`POST {baseUrl}/embeddings` `{model, input[]}` →
     `data[i].embedding`，Bearer 认证（apiKeyEnv 环境变量解析）；
   - `ollama`：`POST {baseUrl}/api/embed` `{model, input[]}` → `embeddings`（本地免 key）；
   - 批量一次往返；空批量短路不发请求；超时缺省 30s；
   - 配置校验（zod / serde）：provider 非法/baseUrl 非 URL/model 缺失 →
     `createEmbeddingClient` 返回 null → 调用方按 347 降级契约回退 FTS5。
2. **chunk 指纹** `chunkFingerprint`：FNV-1a 64 位（UTF-8 字节序，hex 16 位小写）
   ——双端一致的确定性锚，**向量含 FNV 已知值旁证**（FNV-1a("a") =
   af63dc4c8601ec8c）。
3. **增量向量化选择** `selectStaleChunks`：指纹与缓存不一致（含无缓存）的 chunk
   才重新嵌入——落库投影的增量口径。
4. **共享 golden 向量扩展** `semantic-retrieval-vectors.json` 新增两组：
   fingerprint 4 例（含 CJK）+ stale 3 例（无缓存全选/未变跳过+已变选中/
   全缓存空）。TS 8 项 + Rust 差分 8 项全绿——**FNV 双端一致性关键验证通过**。

## 验收

- core：tsc 干净；vitest 215 文件 / 1995 用例全绿（embedding 客户端 4 项：
  配置校验/双协议请求形态与鉴权头/批量透传/空批量短路）。
- studio：tsc 干净；vitest 793 全绿。
- engine-rs：`INKOS_DUEL=1 cargo test` 20 个测试目标全 ok（差分 8 项）。
- bindings 158 绿；bench:gate 通过。

## 影响面

- 新增 1 文件（embedding-client.ts）+ 1 测试；扩展 3 文件（semantic-retrieval
  双端、差分测试）。纯增量零破坏。
- 未触碰并行会话在途文件（package.json、rustbin.rs、audit-rust.mjs）。

## 下一步

- 349 号（G1 末批）：`retrieval_chunks` 落库表（MemoryDB 双端）+ 混合检索端点
  （FTS5+向量 RRF）接入 write-next 检索链 + 召回测试面板（S）+ retrieval trace
  入 Doctor。
- 之后 G5 拆书工作台 → G6 自动导演产品化。
