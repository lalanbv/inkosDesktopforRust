#!/usr/bin/env node
// 走查用 LLM mock（312 号）：生产引擎真实浏览器走查的配套假端点。
//
//   node scripts/walkthrough-mock.mjs [port]   # 默认 1234
//
// 分派（按 system 关键词，与 engine-rs 九路 agent 提示词对应）：
//   网络小说架构师/总架构师 → 5 段地基（story_frame/volume_map/roles/book_rules/pending_hooks）
//   素材分析师             → 同人正典文档
//   资深小说编辑           → PASS 审校
//   创作总编               → planner memo
//   作家/写手              → 章节成文（CHAPTER_TITLE/CONTENT/POST_SETTLEMENT/RUNTIME_STATE_DELTA）
//   审稿                   → PASS 95
//   末条消息为 user（无工具结果）→ propose_action 工具调用（231 号 propose→confirm 协议，
//   args 含 action 必填字段——301/310 号教训）
//   其余                   → PASS
//
// 用法：`INKOS_LLM_BASE_URL=http://127.0.0.1:<port>/v1 inkos-engine-server` 启动引擎
//（/v1 必须带：模型列表与 chat/completions 都走 mock 的 /v1 面——436 号实测），
// 浏览器直连静态面（`INKOS_STATIC_DIR=packages/studio/dist`）即可离线走查全功能。

import http from "node:http";

const ARCHITECT = "=== SECTION: story_frame ===\n## 主题与基调\n少年于微末中抬起头。\n\n=== SECTION: volume_map ===\n### 第一卷（1-30章）觉醒\n主角入宗门。\n\n=== SECTION: roles ===\n---ROLE---\ntier: major\nname: 林动\n---CONTENT---\n## 核心标签\n坚韧、藏拙。\n\n=== SECTION: book_rules ===\n## 主角\n- 名字：林动\n\n=== SECTION: pending_hooks ===\n| hook_id | 起始章节 | 类型 | 状态 | 最近推进 | 预期回收 | 回收节奏 | 上游依赖 | 回收卷 | 核心 | 半衰期 | 备注 |\n|---|---|---|---|---|---|---|---|---|---|---|---|\n| H01 | 0 | 身世 | open | 0 | 第2卷 | 慢烧 | 无 | 第2卷中段 | true |  | 祖符来历 |\n";
const REVIEW = "=== DIMENSION: 1 ===\n分数：90\n意见：冲突清晰。\n\n=== DIMENSION: 2 ===\n分数：88\n意见：开篇有力。\n\n=== DIMENSION: 3 ===\n分数：85\n意见：世界观内洽。\n\n=== DIMENSION: 4 ===\n分数：86\n意见：角色区分明显。\n\n=== DIMENSION: 5 ===\n分数：84\n意见：节奏可行。\n\n=== OVERALL ===\n总分：87\n通过：是\n总评：整体扎实。";
const PLANNER = "# 第 1 章 memo\n\n## 本章目标\n主角初次交锋夺得玉符\n\n## 关联线索\n- H01\n\n## 场景与篇幅预算\n- 场景 1：坊市对峙夺回玉符｜约 700 字\n- 场景 2：玉符异象初次显现｜约 1200 字\n- 场景 3：章尾真相一角｜约 900 字\n\n## 当前任务\n林动在坊市与人对峙，夺回被夺的玉符。\n\n## 读者此刻在等什么\n期待玉符来历揭开。\n本章部分兑现。\n\n## 该兑现的 / 暂不掀的\n- 该兑现：玉符第一步。\n\n## 日常/过渡承担什么任务\n不适用 - 本章无日常过渡。\n\n## 关键抉择过三连问\n- 主角：为什么？利益？人设？\n\n## 章尾必须发生的改变\n信息改变：玉符一角真相。\n\n## 本章 hook 账\nadvance:\n- H01 \"祖符\" → 推进（planted → pressured）\n\n## 不要做\n- 不要降智。\n\n";
const WRITER = "=== CHAPTER_TITLE ===\n风起\n\n=== CHAPTER_CONTENT ===\n林动睁开双眼，灵气顺着经脉游走。他握紧拳头，多年屈辱自今日起一笔一笔讨回来。远处钟声响起，少年迈步而出，踏入坊市的喧嚣之中。\n\n=== POST_SETTLEMENT ===\n结算完成。\n\n=== RUNTIME_STATE_DELTA ===\n```json\n{\"chapter\": 1, \"chapterSummary\": {\"chapter\": 1, \"title\": \"风起\", \"characters\": \"林动\", \"events\": \"醒来\", \"stateChanges\": \"无\", \"hookActivity\": \"H01 推进\", \"mood\": \"紧张\", \"chapterType\": \"推进章\"}}\n```\n";
const CANON = "=== SECTION: world_rules ===\n剑气纵横三千里。\n=== SECTION: character_profiles ===\n| 角色 | 身份 | 性格底色 | 语癖/口头禅 | 说话风格 | 行为模式 | 关键关系 | 信息边界 |\n|------|------|----------|-------------|----------|----------|----------|----------|\n| 林川 | 云州少年 | 坚韧 | 剑不离手 | 简短 | 练剑不辍 | 师父 | 不知身世 |\n=== SECTION: key_events ===\n| 序号 | 事件 | 涉及角色 | 约束 |\n|------|------|----------|------|\n| 1 | 出城 | 林川 | 起点 |\n=== SECTION: power_system ===\n剑道九品。\n=== SECTION: writing_style ===\n短句。";
const SETTLER_TEMPLATE = {
  chapter: 1,
  chapterSummary: { chapter: 1, title: "镜中醒来", characters: "苏檀", events: "坠入镜界通过试炼", stateChanges: "入宗为外门弟子", hookActivity: "H01 推进", mood: "紧张", chapterType: "推进章", conflictLevel: 6, revealLevel: 4 },
  hookOps: { upsert: [
    { hookId: "H01", startChapter: 1, type: "身世", status: "progressing", lastAdvancedChapter: 1, expectedPayoff: "第10章", notes: "镜中世界来历初现端倪。", kind: "suspense" },
    { hookId: "H02", startChapter: 2, type: "情感线", status: "open", lastAdvancedChapter: 0, expectedPayoff: "第8章", notes: "与镜灵的婚约誓言待启。", kind: "emotion" },
    { hookId: "H03", startChapter: 2, type: "信物", status: "open", lastAdvancedChapter: 0, expectedPayoff: "第6章", notes: "碎镜夜半发光之谜。", kind: "artifact" },
  ] },
};

// settler 分派：从消息里抽「第 N 章」动态对齐 delta.chapter（写第 2 章时固定 1 会被 reducer 拒绝）。
function settlerDelta(msgs) {
  const text = msgs.map((m) => m?.content ?? "").join("\n");
  const match = text.match(/第\s*(\d+)\s*章/);
  const n = match ? Number(match[1]) : 1;
  const delta = JSON.parse(JSON.stringify(SETTLER_TEMPLATE));
  delta.chapter = n;
  delta.chapterSummary.chapter = n;
  return "=== RUNTIME_STATE_DELTA ===\n```json\n" + JSON.stringify(delta) + "\n```\n";
}
const ARGS = "{\"action\": \"create_book\", \"instruction\": \"创建玄幻小说《镜花水月》，主角苏檀。世界观：镜中世界反向修行。核心冲突：镜像侵蚀现实。\", \"createBook\": {\"title\": \"镜花水月\", \"genre\": \"xuanhuan\", \"platform\": \"qidian\", \"targetChapters\": 12, \"chapterWordCount\": 3000, \"language\": \"zh\"}}";
let PROPOSE_SEQ = 0;
// 失败注入（439 号）：置 1 后架构师链的 LLM 调用一律 400——驱动前端确认卡降级态
//（438 号「已执行 · 生产链失败」）真机走查。400=非瞬态，不触发重试/接管链；
// fixture 数据面（write-next 走 planner/writer/settler/审稿）不含架构师，不受影响。
const FAIL_ARCHITECT = process.env.WALKTHROUGH_MOCK_FAIL_ARCHITECT === "1";
const PORT = Number(process.argv[2] ?? 1234);

http.createServer((req, res) => {
  if (req.method === "GET" && req.url.includes("/models")) {
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(JSON.stringify({ object: "list", data: [{ id: "lm-mock-model", object: "model" }] }));
    return;
  }
  if (req.method === "POST" && req.url.includes("chat/completions")) {
    let body = "";
    req.on("data", (c) => (body += c));
    req.on("end", () => {
      let sys = "";
      let msgs = [];
      try {
        const p = JSON.parse(body);
        sys = p?.messages?.[0]?.content ?? "";
        msgs = p?.messages ?? [];
      } catch {}
      const lastRole = msgs.length ? msgs[msgs.length - 1].role : "user";
      let content;
      if (sys.includes("同人架构师") || sys.includes("网络小说架构师") || sys.includes("总架构师")) {
        if (FAIL_ARCHITECT) {
          res.writeHead(400, { "Content-Type": "application/json" });
          res.end(JSON.stringify({ error: { message: "mock injected production failure (WALKTHROUGH_MOCK_FAIL_ARCHITECT=1)" } }));
          return;
        }
        content = ARCHITECT;
      }
      else if (sys.includes("资深小说编辑")) content = REVIEW;
      else if (sys.includes("素材分析师") || sys.includes("同人")) content = CANON;
      else if (sys.includes("创作总编")) content = PLANNER;
      else if (sys.includes("作家") || sys.includes("写手")) content = WRITER;
      else if (sys.includes("审稿")) content = "PASS\n95";
      else if (sys.includes("状态追踪分析师")) content = settlerDelta(msgs);
      else if (sys.includes("continuity validator")) content = "PASS";
      else if (lastRole === "user") {
        // propose_action 工具调用（231 号协议：action 必填 + createBook 结构化）。
        // toolCallId 必须逐次唯一（436 号）：前端确认卡锁定键=execId（派生自
        // toolCallId），固定 id 会让后续同形提议复用首轮"已执行"锁，卡直接
        // 锁死不可确认——真 LLM 每次 tool call id 均不同，mock 必须对齐。
        const chunk = { choices: [{ delta: { tool_calls: [{ index: 0, id: `call_propose_${++PROPOSE_SEQ}`, function: { name: "propose_action", arguments: ARGS } }] } }] };
        res.writeHead(200, { "Content-Type": "text/event-stream" });
        res.end(`data: ${JSON.stringify(chunk)}\n\ndata: [DONE]\n\n`);
        return;
      } else {
        content = "已生成确认卡，请在下方点击确认。";
      }
      const chunk = { choices: [{ delta: { content } }] };
      res.writeHead(200, { "Content-Type": "text/event-stream" });
      res.end(`data: ${JSON.stringify(chunk)}\n\ndata: [DONE]\n\n`);
    });
    return;
  }
  res.writeHead(404).end();
}).listen(PORT, () => console.log(`walkthrough mock on ${PORT}`));
