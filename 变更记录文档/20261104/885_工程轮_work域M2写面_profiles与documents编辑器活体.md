# 885 号 · 工程轮：work 域 M2——写面补全（profiles POST/PUT + skills documents PUT）

> 881 设计 §五 M2 落地（「work 创建端点」项经核实**上游无对应端点**——server.ts
> works 域仅 GET+adopt，按设计预案不做）。验收面「创作配置编辑器在 Rust 引擎下
> 读写活体」由交替互证 27/27 + 差分写面探针 12/12 兑现。

## 一、交付面

### profiles 写面（work_routes.rs + profile_registry.rs）

- `POST /profiles`：parse_work_profile_json（strict+version literal(2)+validate
  三层，与 `.inkos/profiles` 加载面共用）→ registry **全集**查重（内置+自定义，
  TS 同款用归并后 registry 判 409）→ 原子落盘 `.inkos/profiles/<id>.json` →
  **201** `{profile}`。重复=409 PROFILE_EXISTS "Choose a new profile ID"。
- `PUT /profiles/:id`：同 parse → body.id ≠ 路径 id → 400 PROFILE_ID_MISMATCH
  "Profile identity cannot change" → 落盘 → 200（upsert 语义，TS PUT 无存在性
  检查——内置 id 亦可覆盖，归并时 replace 内置）。
- **错误面**：zod parse 失败/body 缺失 = 无局部 catch → 500 INTERNAL_ERROR 固定
  文案（TS WorkProfileSchema.parse 抛普通错同款）；typed 仅 PROFILE_EXISTS/
  PROFILE_ID_MISMATCH。
- 落盘形态：`JSON.stringify(profile, null, 2)` **无尾换行**（work.json 才有
  +"\n"，测试断言锚定）；嵌套 artifactSchemas BTreeMap 键序无契约语义（双端经
  parse 读回，884 备案沿用）。

### skills documents PUT（skill_routes.rs）

复用 import 面共享件（normalize_skill_import_files 数量/大小/去重/manifest 唯一/
前缀约束 + parse_agent_skill_document + atomic-file-set）：
body.documents→text/plain dataUrl 包裹→共享归一化→**根 SKILL.md 检查**
（INVALID_SKILL_MANIFEST；嵌套 sub/SKILL.md 过归一化但不在根=此错——测试双面锚
定归一化先序 INVALID_SKILL_IMPORT）→ parse 身份不可变（SKILL_ID_MISMATCH 400；
parse 失败=500 固定文案，TS 普通 Error 无局部 catch 同款）→ **逐段符号链接检查**
（含文件名段，ENOENT 容忍，INVALID_SKILL_PATH 400）→ 原子提交 → `{id, saved}`。
**无 skills:change 广播**（TS 同：仅 import/delete 广播）。

### 差分与互证升级

- engine-contract-diff works 活体块 +4 写面探针（POST 探针→GET 含探针→PUT 改题
  →重名 409），works 活体 **12/12 一致 0 分歧**进门禁默认链；修 TDZ
  （const 先声明后引用）+ 子集选择器泛化（works/profiles 双子集）。
- work-domain-alternation 扩至 **27/27**：③b Rust 写面腿（POST 201+重复 409+
  PUT 内置覆盖+documents PUT saved 面）+ ④ TS zod strict 解析 Rust 写的自定义/
  内置覆盖 profile（contextRecipe 保持）+ 追加文档在位 + ⑤ Node 引擎归并读出
  （profiles=10+文档追加页可见）——M2 验收句的活体证据。
- 夹具补 doc-skill 项目技能种子（写面载体）。

## 二、测试

`tests/work_domain_m2.rs` 4 用例：POST 落盘/归并/重名 409（自定义+内置双侧）/
坏 version 500/缺 body 500；PUT 内置覆盖+id 不变 400+upsert 新 id 200；
documents PUT 快乐路径（saved 面+GET 反映）/根缺失双面（IMPORT 先序+MANIFEST）/
身份不匹配/越界路径/非法 id/documents 缺失 500；符号链接段 400。
**测试自坑修正**：`json!().to_string()` 紧凑形态无空格，`.replace("\"version\": 2",…)`
匹配失败导致坏 body 实际没变坏（返回 201 是正确行为）——修 replace 而非实现。

## 三、验证清单

cargo test 全量 51 目标绿 / testgate 2012+584 / clippy 双 0 / gate:ts 八步
（含差分 works 活体 12/12）/ 交替互证 27/27 / M2 4 用例 / U+FFFD 干净。
备案：共享面 quality-trend issueCount 1 分歧维持（884 备案的既有项，非本轮）；
**M1 收官**（M1a+M1b+M2 全绿）——下一实施轮=M3 前置勘察轮（short-fiction-runner
依赖面，881 §五）；M3 落地时移除 artifact_validation 的 fail-closed 域分支。

**下一号自 886 起；出口不变；用户侧待办=Fork 推送 19 个提交（至本轮提交）。**
