# 964 号 · 档案池 UI 写入口轮：POST /style-profiles {name, text} 双端服务端指纹 + 文风页「保存为写法档案」+ 指纹内核双端 golden

> 963 号备案出口：写法绑定修通后，档案池仍**没有 UI 写入口**——书籍页卡片空态提示
> 「先用文风页分析参考文本并保存档案」，而 2.0 文风页分析产物是 LLM 指南 Markdown、
> 无保存按钮，池只能经 API 写入。本轮补齐 340 号设计原意（「特征池数据来自文风页
> 分析产物」）：文风页一键以统计指纹存档，书籍页即可绑定、写作链即可注入。
> 途中发现 TS 侧统计分析器已随上游 2.0 删除、Rust 移植与原版存在四处静默分歧，
> 以双端 golden 向量表收敛。
> 验证：TS style-fingerprint 2/2（新）+style-profiles-write-forms 2/2（新）+
> StyleManager 交互 5/5（+2）+Rust style_analyzer 11/11（+1 golden）+ops_routes
> 文本形态 +1+testgate engine-rs 2094（+2）/src-tauri 584+audit:rust 双 0（自检
> 活体）+clippy 双 0+gate:ts 十步全绿（engine-contract-diff 78/0）+U+FFFD 净。

## 一、指纹内核：TS 恢复 + 双端 golden（9 向量）

- **TS 侧缺件**：上游 7aed0d82 删除 `core/src/agents/style-analyzer.ts::analyzeStyle`
  （文风分析换 LLM 指南）。写法档案池（340 号 fork 自有功能）仍以统计指纹为数据源
  ——恢复为 studio 自有模块 `api/style-fingerprint.ts::analyzeStyleFingerprint`（自
  `7aed0d82^` 逐字），三处工程差异：`analyzedAt` 可注入；段落最值改循环（原版
  `Math.min(...arr)` 展开在超大样本栈溢出）；引号剥离取并集（见下）。
- **golden**：`__tests__/golden/style-fingerprint-vectors.json`（TS 参考实现产出期望；
  TS vitest readFileSync / Rust include_str! 逐向量全等）。**Rust 首跑 4 向量分歧**
  （先写测试后修=行为红实证），逐项定型修复：
  1. **相邻排比漏计**：`count_parallelism` 命中后 `i = after_end` 又 `i += 1`，跳过紧随的
     标点——「，风很大，风很大，雨很急，雨很急」只计 1 处 < 2 处阈值，「排比」整项
     漏报（JS 全局正则 lastIndex=匹配末尾原位续扫）。
  2. **开头模式并列序**：TS `Object.entries+稳定排序`=并列按键序（数组索引形键如
     「20」数值升序居前、其余按首现序）；Rust 并列按字典序——「我们/他们」并列时反序。
     新增 `js_array_index` 判定+首现序表+稳定排序。
  3. **TTR 引号集**：原 TS 字面仅剥 ASCII `"'`、Rust 移植仅剥弯引号 `“”‘’`——双端
     早已分叉；中文对白惯用弯引号，**统一取并集**（双端同改，入表锁定）。数字类
     顺带对齐 JS 非 u 模式 `\d`=ASCII（Rust `\d` 为 Unicode 会多剥全角数字）。
  4. **UTF-16 计量**：TS `string.length`/`slice(0,2)` 为 UTF-16 码元（星平面字符计 2）
     ——Rust 中文句长/段长/TTR 分母改 `encode_utf16().count()`、开头键取前 2 码元
     （TS `new Set(string)` 按码点去重，分子保持码点集）。
- 向量覆盖：中文对白修辞全谱、相邻排比、并列首现序、整数形键优先、ASCII/弯引号
  混排、emoji UTF-16、全角数字保留、英文散文、空文本。

## 二、POST /api/v1/style-profiles 双端两形态（同判据同消息同次序）

- `profile` 形态（340 号原形态）原样落盘；`text` 形态服务端算指纹落池：
  sourceName=档案名（340 号不变式），语言=请求 `language`（"en"→en，其余→zh）缺省
  回项目语言（958 号同规则），analyzedAt=服务端时刻。
- 错误面统一：name 空或二者皆缺/空白文本/profile null → 400 `name and profile (or
  text) are required`；不规范名 → 400 `invalid profile name`（957 号）。响应
  `{ok, name, profile}`（落盘全文）。
- Rust 顺带清偿既有分歧：typed `Json` 提取器缺 profile 即 422（TS 400）、空名报
  `name is required`（TS 合并消息）、2MB 默认上限截断大样本——改手工读
  `BODY_CAP_LARGE_TEXT`（273 号同款）。TS 落盘改 `writeTextAtomic`（Rust 本已原子）。

## 三、文风页「保存为写法档案」

- 输入区「分析」旁新增按钮（来源名称与正文齐备才可点；档案名=来源名称 trim）+
  说明文案「以「来源名称」为档案名存入写法档案池（统计指纹，无需模型）……」；成功
  「已保存写法档案「X」。」，失败 destructive 红（如不规范名）。不依赖 LLM：无模型
  配置也能存档。i18n 四键（zh/en）。

## 四、测试与必红

- TS：`style-fingerprint.test.ts`（全向量全等+语义锚点）；`style-profiles-write-
  forms.test.ts`（文本形态指纹=golden 且 sourceName=档案名、en 显式、profile 原样、
  五形错误面、穿越名）——stash server.ts 恰 2 红。
- Rust：`golden_vectors_match_ts_reference`（首跑 4 分歧=修前行为红）；
  `save_style_profile_text_form_matches_fingerprint_golden`（同断言对偶）——
  NEUTER964 中和文本形态恰 1 红，撤除后绿，残留 0。
- UI：StyleManager 交互 +2（禁用守卫/载荷 trim/不触达 LLM 分析端点/失败红）——
  stash 组件恰 2 红。
- **差分器**：共享面 +2 条目（POST {name,text} 后紧随 GET 列表全 body 比对，
  analyzedAt 入 VOLATILE）；探针正文含相邻排比+ASCII 引号=对分析器修复敏感——
  旧 Rust 分析器二进制上 ✗（排比缺失、TTR 0.761 vs 0.768），修复后 78/0。

## 五、真引擎 GUI 全环（walkthrough-env --engine rust，8964）

- 文风页：填来源名称「热血少年」+粘贴样章 → 「保存为写法档案」→ 绿色「已保存写法
  档案「热血少年」。」；池文件 sourceName=热血少年、修辞含「排比(2处)」。
- 书籍设置页「写法绑定」卡：下拉含热血少年 → 选中 → 「绑定到本书」→「当前绑定:
  热血少年 已绑定。写作链将注入该档案的写法特征。」
- plan+compose 第 3 章 → runtime context selectedContext 含 `style/热血少年`：句长
  12.7/段长 32–50/TTR 77%/比喻·排比·反问——**文风页存档→书籍页绑定→写作链注入**
  用户闭环首次在默认 Rust 引擎上全通。
- 环境清偿：编排进程/mock/引擎杀净、临时根删除、IAB 标签关闭、stash 栈 0。

## 六、门禁

- clippy 双 0（首跑 type_complexity 1 处=分区元组类型别名化后清零）；testgate
  engine-rs 2094（+2）/src-tauri 584（INKOS_DUEL=1）+audit:rust 双 0+必红自检活体；
  gate:ts 十步全绿；U+FFFD 净。

## 七、备案

- 特征文案沿用分析器标签原样（「排比(2处)」带计数后缀进 guidance）——339 号 golden
  锁定的特征池推导面，非本轮范围。
- Node 回退端写作链无绑定注入（963 号备案不变）。

**下一号自 965 起；出口=C 类刷新集逐事件定性 / 翻译域 2.0 全量迁移规划；用户侧
待办=Fork 推送（含 955-964）。**
