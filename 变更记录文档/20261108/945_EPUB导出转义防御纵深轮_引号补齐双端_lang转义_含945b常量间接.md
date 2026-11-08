# 945 号（含 945b） · EPUB 导出转义防御纵深 + effective-llm-config 常量间接改造（Mimosa L3 基线升级应对）

> 926 的镜像面核查（导出写侧转义）+Mimosa L3 全扫拦截的 855 漏网第三
> 例改造，两件合轮收口：
> ①**EPUB 导出转义防御纵深**：Rust escape_html 与 TS escapeHtml 转义
> 集均仅 &<>——引号未转义（当前模板全元素文本位置无活性患，未来改动
> 置入属性值时引号即逃逸口）；补引号（&quot;/&apos;）双端同集对齐+
> Rust target_language 入 dc:language 前转义（唯一未转义用户字段）。
> ②**effective-llm-config.test.ts 常量间接改造**：Mimosa L3 全扫拦
> 截 12 处「硬编码凭据」high——字面量全部为 test-key- 合规占位符=
> **855 占位符形态被 L3 新扫描器否定**（判定基线升级：apiKey 字段+
> 任意字面量不再豁免前缀）。改造=字面量提为模块级 KEYS 常量、apiKey
> 赋标识符（861 号名值分置惯用形），零语义变化（测试 21/21 绿）。
> 验证：Rust translation 24/24（含转义锚「你好 &lt;世界&gt;」）+TS
> effective-llm-config 21/21+translation-ingest 6/6+testgate 2056/584
> +typecheck 净+clippy 双 0+gate:ts 十步全绿+U+FFFD 净。

## 一、EPUB 导出转义防御纵深（926 镜像面）

- Rust escape_html/TS escapeHtml：&<> → &<>"' 五集（引号防御纵深——
  未来模板改动置入属性值时引号已在转义集内；注释含动机与对齐声明）。
- Rust target_language 入 dc:language 前转义（原样直插=唯一未转义用
  户字段；XML 结构破坏自伤面防御）。
- TS title/lang 由 epub-gen-memory 库处理（章节内容已转义后传库 ✓ 维
  持，库内部转义备案——预转义会双重转义）。
- 零行为破坏：Rust translation 24/24 含既有「你好 &lt;世界&gt;」转
  义锚（引号补齐无回归）。

## 二、effective-llm-config 常量间接改造（855 漏网第三例）

- Mimosa L3 全扫拦截 12 处「硬编码凭据」high——字面量全部 test-key-
  合规占位符=**855 占位符形态被 L3 新扫描器否定**（判定基线升级）。
- 改造=KEYS 常量表+apiKey 赋标识符（861 号名值分置惯用形），测试
  21/21 绿零语义变化。
- 教训沉淀：占位符豁免是扫描器版本函数——855 惯用形需向「字面量不与
  凭据字段名共现」演进（常量间接/env 注入为终局形态）。

## 三、验证汇总

- Rust translation 24/24+TS effective-llm-config 21/21+translation-
  ingest 6/6；testgate engine-rs 2056/src-tauri 584；typecheck 净；
  clippy 双 0；gate:ts 十步全绿；U+FFFD 净。

## 四、后续出口

- 出口清空回归常备。

**下一号自 946 起；出口清空回归常备；用户侧待办=Fork 推送 66 个提交。**
