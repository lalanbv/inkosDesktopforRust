# 1057 号 · 备案② cli TUI zh 承接清偿轮

> 1056 出口执行（备案②清偿专项主项）。**1007 备案台账全部清零**。

## 一、实施（映射表 core 单源提升 + cli TUI 接线）

| 端 | 改动 |
|---|---|
| core | 新建 `llm/error-copy.ts`：`localizeKnownRuntimeMessage(message, lang)` **参数化**（core 不持全局态；en 早退/zh+ja 映射=studio 既有行为逐字节等价）+ exports subpath（浏览器面安全） |
| studio | `lib/error-copy.ts` 改**薄壳**（getAppLanguage 门控+调 core）：use-api 消费形态零变化、error-copy.test.ts 9 例生产派生锚零改动继续活体 |
| cli | `localization.ts` 新增 `localizeTuiRuntimeMessage`（resolveCliLanguage 门控）+ `dashboard.tsx` catch 接线——TUI zh 用户对 provider 六诊断块（400/401/403/429/5xx/connection）映射回 zh 展示，en 保持 EN |

## 二、红绿闭环与验证

- cli 新测试 4 例（wrapLLMError **生产派生样例**同 studio 锚——1007 教训「样例必须派生」同款）：zh 映射 / en 原样 / 5xx 无 detail 空格回归锚 / 未知透传
- **NEUTER1057 恰红 2/2**（摘映射→zh 例+5xx 例红，en 与透传例语义正确仍绿）→互逆恢复绿——分形自洽
- 途中修正：5xx 派生样例首写缺 `(no body)` 触发词（裸 503 不产出诊断块=手打即漂的再实证）——同 studio :85 同源派生形修正
- studio vitest alias 双登记 `llm/error-copy` subpath（1008 教训：subpath 须先于裸包名）
- **i18n 台账卫兵自抓台账漂移**：error-copy 预算 11 ≠ 实况 0 →条目注销（清偿后预算下调——卫兵设计意图首次真实触发）
- **验证全绿**：三包 typecheck 0 错误 + 全量 2048 例（core 1068 / studio 895 / cli 85+4）+ U+FFFD 净

## 三、L3 门禁观察

含 `INKOS_LLM_API_KEY` 字样的正则字面量与中文 replacement 文案**过闸未拦**——
再证第六次拦截定性（规则对纯赋值形态串字面量最敏感；正则/注释模板/属性访问不报）。

## 四、下一号自 1058 起

出口=下一巡检周期 / earendil-mz 单包合并（触发条件制持续）/ 用户侧待办=Fork 推送（含 955-1057）。
