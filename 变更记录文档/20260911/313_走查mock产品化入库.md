# 313 号：走查 mock 产品化入库——scripts/walkthrough-mock.mjs 可复用走查基建

- 日期：2026-09-11
- 分支：develop
- 关联：276–292/304/312 号（本会话九轮走查——mock 在 /tmp 反复重建 8+ 次，本批固化为仓库资产）
- 编号衔接：查 20260911 目录最大号 312，顺延 313
- 推送核验：origin/develop = 498253fa（300 号）未动；本地领先 301–313 共 13 提交待推

## 一、内容

`scripts/walkthrough-mock.mjs`（新，自包含 ~5KB）：生产引擎真实浏览器走查的配套 LLM 假端点。

- 分派覆盖九路 agent 提示词关键词：架构师（5 段地基）/ 素材分析师（同人正典）/ 资深编辑（PASS 审校）/ 创作总编（planner memo）/ 作家·写手（章节成文四段）/ 审稿（PASS 95）/ 其余 PASS；
- 231 号 propose→confirm 两轮状态机：自由文本轮 → propose_action 工具调用（含 action 必填——301/310 号教训固化）；工具结果轮 → 确认卡应答；
- `GET /models` 供服务测试连接探测；
- 端口参数化（默认 1234，即 LM Studio 预设端口——服务配置「测试连接」可直接命中）。

## 二、用法（脚本头注释同文）

```bash
node scripts/walkthrough-mock.mjs 1234 &
INKOS_LLM_BASE_URL=http://127.0.0.1:1234 \
INKOS_STATIC_DIR=packages/studio/dist \
INKOS_PROJECT_ROOT=<项目根> \
inkos-engine-server
# 浏览器打开 http://127.0.0.1:8787 → 服务页添加模型（lm-mock-model）→ 全功能离线走查
```

## 三、验证

`node --check` 语法 ✓；实跑冒烟（/models + 架构师分派）✓。零运行路径影响（新增独立脚本）。

## 四、遗留

301–313 共 13 提交待推送；并行会话三件第四十轮在途未落库；两项默认值、ja A/B、历史瘦身待用户决策。
