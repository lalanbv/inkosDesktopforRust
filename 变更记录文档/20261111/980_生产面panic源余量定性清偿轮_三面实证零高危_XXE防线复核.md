# 980 号 · 生产面 panic 源余量定性清偿轮：三面实证零高危 + XXE 防线复核

> 979 备案清偿（「非 lock 族生产面 panic 源余量≈441 站分批定性」→本批=全批）。
> 三面实证：**分族统计+归属判定+守卫勘察**——结论：输入驱动 panic 站实证≈0，
> 余量主体全为静态不变量族（惯例安全），无高危修复面，零代码改动（978 形）。
> 附：XXE 防线双端对称复核（Mimosa XML 约束验收）+ picomatch peer 警告勘误
> （979 备案清偿）。
> 必红形态=audit:npm 四夹具活体（903/977/978 巡检族先例）；门禁引用 979
> 收口全绿面（树同则面不变）。

## 一、余量重盘（979 替换后）

- 初盘（状态机普查）：engine-rs unwrap 308+expect 223（114 文件）/
  src-tauri unwrap 75+expect 19（13 文件）——含测试面误归（brace 漂移 +
  文件级 tests 形态如 engine/node/tests.rs 22 站、jsonio.rs 56 站测试区）。
- **真余量远小于初盘**：src-tauri 生产区真站≈20 站（jsonio 生产区仅 2 站
  expect 带前置 ensure 不变量消息）；engine-rs server 域 serde 转换站
  15 站中 11 站 cfg(test) 归属判定=测试面，**真生产站仅 4**。

## 二、三面实证（定性方法）

1. **分族**：族 A=字面量正则 `Regex::new(r"…").unwrap()` 215 站（输入=源码
   字面量，模式合法性为编译期事实）；族 B=capture 组 `.get(N).expect("组 N
   必在")` 13 站（匹配结果结构保证）；族 C=其余（serde 转换/路径/IO/容器）。
2. **归属判定**：cfg(test) 行号对照（python 脚本逐站）——server 域 15 站
   中 11 站测试面；local_search 591/617/624 全测试区（cfg(test)@496）；
   read_all 12 站全测试；store.rs 243 测试。
3. **守卫勘察**（族 C 真生产站逐站看上下文）：
   - service_routes 1261/1383 + project_config 546/556：`json!({...})`
     构造后 `as_object_mut().unwrap()`——宏字面量 Object 结构保证；
   - long_span_fatigue 85 `sentences.last().unwrap()`——前置
     `if sentences.is_empty() { return None; }` 守卫；
   - tension_curve 272/322——前置 `len() >= window` 守卫；
   - project_files_routes 228-229 `parse().unwrap()`——content_type 来自
     `resolve_project_image_file` 返回的 **`&'static str` 静态映射表**
     （扩展名 match），"no-store"=字面量；
   - text_range_edits 328/author_edit_scope 261 `offsets.last()`——区间
     构造守卫（同族前置 len 检查）。

**定性结论**：输入驱动 panic 站（外部数据可达→容器空/类型不符→panic）
实证≈0。生产面 panic 卫生现状=静态不变量族+守卫前置族+错误面反序列化
（axum Json 强类型+APIErrorResponse），979 poison 清偿后锁面亦净。
979 备案「分批」**全批清偿为定性零高危**——负结果亦是结果（防患验证轮）。

## 三、XXE 防线双端复核（Mimosa XML 约束验收）

- 约束：「解析不可信 XML 时：解析前拒绝 DOCTYPE/ENTITY，不启用外部实体」。
- 现状：**不可信 XML 主面=EPUB 导入，双端防线对称在位**（926 号镜像）——
  Rust `translation/source.rs:170` 与 TS `core/translation/epub.ts:20`
  均显式拒绝 `<!ENTITY`（含该串即拒收+双语错误消息）；两实现均为**手写
  正则解析**（无 XML parser 实体展开机制，decodeXml/replace 仅五预定义
  实体）——DOCTYPE 放行的安全语义=「外部实体无处展开」（约束的 XXE
  防御目标达成，实现路径为解析器自身无展开能力+ENTITY 源拔除双重防线）。
- 其余 DOCTYPE/ENTITY 提及面逐一定性为无关（export-html 生成面/
  entity-roster 互动剧本实体/agent-tools、play 游戏概念面）。
- PDF 域（pdf-extract/lopdf）解析行为为库内部面（972 定性 EPUB 为主面）。

## 四、picomatch peer 警告勘误（979 备案清偿）

- 979 时 `pnpm install` 一次性警告「unmet peer picomatch@^3.0.2: found
  4.0.7」——复跑 install **零警告**；lock 双版本分流 override
  （`picomatch@<4: ^3.0.2`+`picomatch@>=4: ^4.0.4`，双版本 3.0.2/4.0.7
  共存且 fdir peer 绑 4.0.7 正确）——**瞬态重解析噪音，非稳态问题**。
  979 备案「peer 警告既有面观察」清偿=勘误归档。

## 五、必红形态与门禁

- **必红形态**（903/977/978 巡检族先例：定性轮无行为改动，无 NEUTER/stash）：
  audit:npm 四夹具活体（979 轮 GHSA 真红实证+本轮 install/audit 复跑干净
  ——形状卫兵机制活）；audit:rust 双夹具活体引用 979 收口（树同）。
- **门禁引用**：本轮零代码改动（git status 干净）——clippy 双 0/testgate
  2111+584/gate:ts 十步/U+FFFD 引用 979 收口全绿（965/976/978 先例：树同
  则面不变）；本轮新增活体=install 零警告复跑+audit:npm 复跑绿。
- 真引擎活体豁免：无行为改动面（勘察为静态扫描+源码复核，非引擎面）。

## 六、结论与出口

- 979 备案全批清偿：panic 源余量定性零高危，无后续分批（台账归档）。
- 979 picomatch 备案勘误清偿；XXE 双端防线复核达标归档。
- 出口回归常备：上游协议扩展/外部事件/新真实需求/下一巡检周期。

**下一号自 981 起；用户侧待办=Fork 推送（含 955-980）。**
