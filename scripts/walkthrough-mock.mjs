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
//   修稿编辑               → FIXED_ISSUES + REVISED_CONTENT（审改循环修稿腿，646 号）
//   审稿                   → JSON 契约（passed/overall_score/issues/summary，645 号；
//                             WALKTHROUGH_MOCK_AUDIT_SCORES 可注入降分序列驱动审改循环，646 号）
//   末条消息为 user（无工具结果）→ propose_action 工具调用（231 号 propose→confirm 协议，
//   args 含 action 必填字段——301/310 号教训）
//   其余                   → PASS
//
// 用法：`INKOS_LLM_BASE_URL=http://127.0.0.1:<port>/v1 inkos-engine-server` 启动引擎
//（/v1 必须带：模型列表与 chat/completions 都走 mock 的 /v1 面——436 号实测），
// 浏览器直连静态面（`INKOS_STATIC_DIR=packages/studio/dist`）即可离线走查全功能。

import http from "node:http";
import { writeFileSync } from "node:fs";

const ARCHITECT = "=== SECTION: story_frame ===\n## 主题与基调\n少年于微末中抬起头。\n\n=== SECTION: volume_map ===\n### 第一卷（1-30章）觉醒\n主角入宗门。\n\n=== SECTION: roles ===\n---ROLE---\ntier: major\nname: 林动\n---CONTENT---\n## 核心标签\n坚韧、藏拙。\n\n=== SECTION: book_rules ===\n## 主角\n- 名字：林动\n\n=== SECTION: pending_hooks ===\n| hook_id | 起始章节 | 类型 | 状态 | 最近推进 | 预期回收 | 回收节奏 | 上游依赖 | 回收卷 | 核心 | 半衰期 | 备注 |\n|---|---|---|---|---|---|---|---|---|---|---|---|\n| H01 | 0 | 身世 | open | 0 | 第2卷 | 慢烧 | 无 | 第2卷中段 | true |  | 祖符来历 |\n";
const REVIEW = "=== DIMENSION: 1 ===\n分数：90\n意见：冲突清晰。\n\n=== DIMENSION: 2 ===\n分数：88\n意见：开篇有力。\n\n=== DIMENSION: 3 ===\n分数：85\n意见：世界观内洽。\n\n=== DIMENSION: 4 ===\n分数：86\n意见：角色区分明显。\n\n=== DIMENSION: 5 ===\n分数：84\n意见：节奏可行。\n\n=== OVERALL ===\n总分：87\n通过：是\n总评：整体扎实。";
const PLANNER = "# 第 1 章 memo\n\n## 本章目标\n主角初次交锋夺得玉符，并在冲突中初窥镜界异象的一角。\n\n## 关联线索\n- H01 祖符来历\n- H02 婚约誓言待启\n\n## 场景与篇幅预算\n- 场景 1：坊市对峙夺回玉符｜约 700 字\n- 场景 2：玉符异象初次显现｜约 1200 字\n- 场景 3：章尾真相一角｜约 900 字\n\n## 当前任务\n林动在坊市与人对峙，当众夺回被抢走的祖符玉佩，并在冲突余波中察觉玉佩深处的异样脉动。\n\n## 读者此刻在等什么\n读者正等着看林动如何讨回祖符、迈出复仇第一步，同时盼着玉符来历这条主线悬念被揭开一角。\n\n## 该兑现的 / 暂不掀的\n- 该兑现：祖符夺回与第一步力量显现。\n- 暂不掀：祖符完整来历与宗门背后的黑手。\n\n## 日常/过渡承担什么任务\n本章无纯日常过渡；坊市群像承担世界观的铺陈与压迫感的累积，为后续宗门线蓄力。\n\n## 关键抉择过三连问\n- 主角为什么当场夺符：屈辱累积到顶点，退无可退。\n- 利益：祖符是修炼根基，也是身世线索。\n- 人设：藏拙坚韧，隐忍中带锋芒，不逞匹夫之勇。\n\n## 章尾必须发生的改变\n信息改变：玉符一角真相初显，林动确认自己能听见玉佩脉动。\n状态改变：林动正式踏上修炼路，与坊市恶霸结下死仇。\n\n## 本章 hook 账\nadvance:\n- H01 \"祖符\" → 推进（planted → pressured）\n\n## 不要做\n- 不要降智。\n- 不要提前掀开祖符完整来历。\n";
// 645 号：章节正文扩写到篇幅预算带内（zh_chars 去空白计数，3000 字设置下
// hardMin=2182）——旧 62 字恒触发「未达到篇幅预算」警告（644 走查实录）。
// 同时规避 post-write-validator 与 ai-tells 全表：无「不是…而是…」/破折号/
// 转折标记词（仿佛·忽然·竟然·猛地·不禁·宛如 全文 ≤1）/公式化转折词/元叙事，
// 段长刻意长短交错（cv≥0.15）。
const WRITER_BODY_PARAGRAPHS = [
  "林动睁开双眼时，窗纸刚好透进第一缕天光。他没有立刻起身，只按记忆里那卷残破功法的门径，把呼吸一寸一寸压慢，让体内那股微弱得近乎错觉的暖流顺着经脉缓缓游走。暖流每过一个小周天，四肢百骸便多出一分难以言说的充盈，像干裂的河床迎来了头一场春水。他握紧拳头，指节发出一连串细碎的爆响。多年屈辱，从今天起，要一笔一笔讨回来。",
  "钟声响了。",
  "三长两短，是青阳坊开市的号令。林动起身，把半块祖符玉佩贴身收好，推门而出。青石长街两侧的摊贩正支起棚架，吆喝声、讨价声、骡马蹄声混作一团热浪扑面而来。他压低斗笠，混进人流。三日之前，正是在这条街上，雷家的管事当众踩碎了他捡来的药材筐，还纵马溅了他一身泥水。那一天他忍了。彼时的他，连愤怒的资格都没有。",
  "今天不一样。今晨的周天运转让他确认了一件事：体内那股暖流确实源自祖符的苏醒。功法残卷上写得明白，引气入体只是门槛，真正的分水岭在于气感。气感，他已经有了。这意味着，他不再是那个任人踩进泥里的废人。他要当着整条坊市的面，把属于林家的东西、属于他林动的尊严，一分一分拿回来。",
  "雷家管事的摊位设在正街丁字路口，占着最好的位置，身后立着两名挎刀护院。摊位一角堆着他被抢走的药篓，篓底还压着母亲留下的一包干药材。管事跷腿嗑着瓜子，见有人走近，眼皮都没抬，只懒懒甩出一句：要买东西，先把银子拍在桌上。四下的窃笑声低低荡开，像是看惯了这样的戏码。",
  "林动停在摊前，伸出手。声音不高，却清清楚楚压过了半个街市的嘈杂：药篓是我的，三日前被你的人抢到这里，今天我来拿回去。管事终于抬眼，上下打量这个穿着洗得发白的粗布短打的少年，随即嗤笑出声，瓜子壳往地上一啐。在青阳坊，进了雷家摊子的东西就是雷家的。小子，活腻了就直说。两名护院同时按住刀柄，指节捏得咯咯作响。",
  "林动没有退。他在心里默数着周天的节拍，把那股暖流一寸寸沉入双臂。三步，两步。管事挥手喝令上前的一瞬，林动动了。他的动作谈不上任何招式，只有一个字，快。护院的刀才拔出一半，手腕已被一股蛮不讲理的巧劲扣住，整个人腾云驾雾般摔出，砸翻了半排货架。第二人挥拳砸到，被他侧身让过，一肘结结实实顶在肋下。整条长街，霎时安静得能听见风声。",
  "管事的脸色由青转白，踉跄着退了两步，撞翻身后的太师椅。那句惯常的狠话卡在喉咙里，怎么也吐不出来。林动弯腰，拾起自己的药篓，掸去尘土，动作从容得像只是来赴一场早就约好的会面。记住了，少年背对着他，声音平静，我叫林动。这份账，今天只算一半，剩下的，我上门去取。说完，他迈步走入人流，再没有回头。",
  "出了北门，登上回窑洞的土坡，攥紧的拳心才渗出一层薄汗。掌心里那点湿意提醒着他，方才那一瞬间的力量，真真切切属于他自己。土坡下的青阳坊渐渐热闹起来，叫卖声隔着半里地飘上来，混着骡马的响鼻。林动在坡顶站了一会儿。十年了，他头一回觉得自己跟这座坊市之间，隔着的只是一部还没练完的功法。",
  "入夜。油灯如豆。林动盘膝坐下，从怀中取出祖符玉佩。就在心神沉入气感的刹那，玉佩表面那道古老的裂纹里透出一线极淡的青光，一个苍老而缥缈的声音贴着他的耳骨响起：血脉相承，符启于危，小子，你总算是来了。林动霍然睁眼。窗外，夜色深处，似乎有什么庞大的东西正在缓缓苏醒。",
  "你是谁。他死死盯着掌心的玉佩，声音压得极低。窑洞里静得只剩灯芯燃烧的噼啪声。那道苍老声音沉默片刻，再度响起时带着一种俯瞰岁月般的疲惫：老夫石渊，是这枚符里残存的一缕器灵。你母亲把它缝进你衣襟的那一天，就替你选好了这条路。器灵两个字砸进耳中，林动的呼吸滞了一瞬。母亲。这两个字像一枚烧红的针，扎进他心底最深处那块从未愈合的伤疤。十年前那个雨夜，母亲把他塞进地窖，转身掩上洞门的最后一眼，他至今记得清清楚楚。她说过，等我十六岁，符会自己醒。石渊缓缓道，如今你引气入体、气感初成，符门已为你开了一条缝。但老夫把丑话说在前头，这条路上，你的每一步都踏在别人的算计里。当年林家满门的火，出自人手。",
  "林动的指甲深深掐进掌心。他想起父亲书房里那盆再没人浇水的兰草，想起族老们闪烁其词的说辞，想起自己这十年寄人篱下、看遍冷眼的每一个日夜。原来一切早有来处。三更的梆子声从远处传来。石渊留下最后一句嘱咐：七日内，雷家必来寻仇。想活到能掀开真相的那一天，今夜起，每日子时引符中青气入脉一个周天，多一个时辰都不要贪。话音落下，青光敛去，玉佩重新变回那块黯淡无光的旧物，先前的一切恍如错觉。黑暗里，林动听见自己的心跳，一下，又一下，像某种迟到了十年的战鼓。他重新闭上眼。这一次，呼吸缓缓沉入了符纹深处。",
  "翌日清晨，林动照旧去货栈帮工。麻袋压上肩头的时候，他在心里默默数着呼吸的节拍，把每一口气都压进功法的路数里。搬完第三十袋，管货的老者眯眼打量他：小子，今天脚下有根了。林动只笑笑，没接话。有些事说给别人听，是炫耀；压在自己心里，才是筋骨。午间歇工，他蹲在货栈后巷啃干饼，两个脚夫的闲谈飘进耳朵，说城东武行昨夜被人踏了场子，动手的像是个半大孩子。林动咬饼的动作停了半拍，随即恢复如常。消息散得比预想的快。雷家那边的反应，只怕今晚就会到。他把最后一口饼咽下去，拍净衣角，抬起头时目光已经落回长街尽头。该来的总会来。在它来之前，把今天的活干完，把今夜的功练足，仅此而已。他甚至有闲心在回程的渡口多站了一会儿，看渡船靠岸，看挑夫们喊着号子把一筐筐春茶抬上岸，看远处官道尽头雷家宅院的飞檐在日头底下泛着冷光。",
  "回到窑洞，天光尚余最后一抹橘色。林动把门闩落好，又搬了口破水缸抵住，这才在草席上盘膝坐定。白日里那些视线、那些窃语，此刻都已隔在门外。他摊开手掌，祖符玉佩静静躺在掌心，裂纹里的青光比昨夜又亮了一分。石渊的声音在符纹深处响起来，只说了四个字：子时，引气。林动点头，闭目。窑洞外，风声掠过坡地，远处更鼓一声声传得很远。这一夜，青阳坊无人入睡；雷家宅院里灯火通明，家丁们来回奔走的脚步声隔着几条街都能听见。风起了。有些账，要开始清了。",
];
const WRITER_BODY = WRITER_BODY_PARAGRAPHS.join("\n\n");
// 646 号：修稿腿的修订正文——原正文追加修订尾段（reviser 判 revisedContent
// 与原文全等即「未产出新内容」退出循环，故必须有差异；+100 字仍带内）。
const REVISED_BODY = WRITER_BODY
  + "\n\n坊市的喧嚣在身后一点点退去，林动的脚步却越走越稳。祖符在怀里微微发烫，像是替他记下了这条街上每一道目光。他知道，从今天起，青阳坊再没有人敢小看那个穿粗布短打的少年。";
// 650 号：PATCHES 形态的局部修补对（patch-only 路由）——TARGET 为 WRITER 正文
// 精确句，REPLACEMENT 为等义改写（字数近同：局部修补不显著改变篇幅）。
const PATCH_TARGET = "他握紧拳头，指节发出一连串细碎的爆响。";
const PATCH_REPLACEMENT = "他缓缓收紧五指，指节间爆出一串沉闷的脆响。";
// 651 号：analyzer 保真形态——UPDATED_HOOKS 与 walkthrough-fixture 预置池同源
// （14 列 R23 台账，分类列驱动 memory.db promises 投影：悬念/情感/物品/世界观
// 四 kind 必须齐）。650 号的最小占位把伏笔池写空致投影缺失、fixture 断言红。
const ANALYZER_HOOKS_TABLE = `| hook_id | 起始章节 | 类型 | 状态 | 最近推进 | 预期回收 | 回收节奏 | 上游依赖 | 回收卷 | 核心 | 半衰期 | 升级 | 备注 | 分类 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| H01 | 1 | 身世 | progressing | 3 | 第10章 | slow-burn | 无 | 第一卷 | 是 | 10 | 是 | 镜中世界的来历真相。 | 悬念 |
| H02 | 2 | 情感线 | open | 3 | 第8章 | near-term | 无 | 第一卷 | 否 | 8 | 否 | 苏檀与镜灵的婚约誓言。 | 情感 |
| H03 | 2 | 信物 | pressured | 3 | 第6章 | near-term | H01 | 第一卷 | 否 | 6 | 否 | 母亲留下的碎镜在镜界发光。 | 物品 |
| H04 | 3 | 背景 | open | 0 |  | slow-burn | 无 | 第二卷 | 否 |  | 是 | 镜宗与皇室的隐秘盟约。 | 世界观 |`;
const WRITER = `=== CHAPTER_TITLE ===\n风起\n\n=== CHAPTER_CONTENT ===\n${WRITER_BODY}\n\n=== POST_SETTLEMENT ===\n结算完成。\n\n=== RUNTIME_STATE_DELTA ===\n\`\`\`json\n{"chapter": 1, "chapterSummary": {"chapter": 1, "title": "风起", "characters": "林动", "events": "坊市夺回祖符，玉符异象初显", "stateChanges": "林动踏上修炼路，与雷家结仇", "hookActivity": "H01 推进", "mood": "紧张", "chapterType": "推进章"}}\n\`\`\`\n`;
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
// 652 号：最后一次结算章号记录给 analyzer 分派复用（CHAPTER_SUMMARY/UPDATED_STATE 的当前章号）。
// 653 号：取**最后一个**「第 N 章」命中——planner 文本「# 第 1 章 memo」先于目标章引用出现，
// 取首个会让 LAST_SETTLER_CHAPTER/delta.chapter 恒偏 1（652c 实录：写第 2 章时 analyzer 摘要行章号=1）。
let LAST_SETTLER_CHAPTER = 1;
function settlerDelta(msgs) {
  const text = msgs.map((m) => m?.content ?? "").join("\n");
  // 653 号：章号锚=「## 已有章节摘要」表格的最大已有章号+1（写第 N 章=已有
  // N-1 章+1，连续写作单调自洽）。不可用「第 N 章」文本命中：消息里的命中全
  // 是 memo 标题（「# 第 1 章 memo」）与伏笔表预期回收（「第8章」）——取首
  // 恒偏 1、取末跳到回收章（652c/653b 两轮实测），settler 输入本身不含目标章号。
  const summaryBlock = text.match(/## 已有章节摘要\n([\s\S]*?)(?=\n## |$)/)?.[1] ?? "";
  const summaryMax = [...summaryBlock.matchAll(/^\| (\d+) \|/gm)].reduce((m, x) => Math.max(m, Number(x[1])), 0);
  const n = summaryMax + 1;
  LAST_SETTLER_CHAPTER = n;
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
// 慢速流（644 号）：每块 delta 间隔毫秒数（默认 0=一次性发射，现行为）。正文类
// 响应按 8 字符/块逐块发射——write-next 停止按钮的活体走查需要足够长的流中窗口
//（642 号可停性验证）；0 时逐字节等价旧行为，不影响 639 等既有走查形态。
const STREAM_DELAY_MS = Number(process.env.WALKTHROUGH_MOCK_STREAM_DELAY_MS ?? "0");
// 审稿降分注入（646 号）：逗号分隔的分数序列（如 "62,91"）——第 N 次审稿调用
// 取第 N 个值（越界取最后一个）；未设=恒 88（645 形态，审改循环不触发）。
// 分数 <85 时 passed:false 并附一条 structural issue，驱动 chapter-review-cycle
// 的修稿轮次真实触发（审改循环/降分重写分支的活体走查面）。
const AUDIT_SCORES = (process.env.WALKTHROUGH_MOCK_AUDIT_SCORES ?? "")
  .split(",").map((s) => Number(s.trim())).filter((n) => Number.isFinite(n));
let AUDIT_CALL_SEQ = 0;
// 降分 issue 的 repair_scope（650 号）："local"（默认 structural）→ reviser
// resolveAutoOutputMode 判 patch-only → 修稿腿走 PATCHES 局部修补路由，
// 与 646 号的 REVISED_CONTENT 整章重写路径互为对偶覆盖。
const AUDIT_SCOPE = process.env.WALKTHROUGH_MOCK_AUDIT_SCOPE === "local" ? "local" : "structural";
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
      let wantStream = true;
      try {
        const p = JSON.parse(body);
        sys = p?.messages?.[0]?.content ?? "";
        msgs = p?.messages ?? [];
        wantStream = p?.stream !== false;
      } catch {}
      // 622 号：协议保真——按请求 stream 旗标返回形态。此前恒返 SSE，Rust
      // 非流式线上路径（stream:false → 整体 JSON 解析，streaming_client 108 号）
      // 在 mock 环境必失败，stream 偏好链（108/606/613/621 号）从未被活体验证。
      // Node 侧 pi-ai completeSimple 内部恒 streamSimple（0.87 compat.js），不受影响。
      const emitJson = (message, finishReason) => {
        res.writeHead(200, { "Content-Type": "application/json" });
        res.end(JSON.stringify({
          id: "chatcmpl-mock",
          object: "chat.completion",
          created: 0,
          model: "lm-mock-model",
          choices: [{ index: 0, message, finish_reason: finishReason }],
          usage: { prompt_tokens: 100, completion_tokens: 50, total_tokens: 150 },
        }));
      };
      const emitSse = (chunk, finish) => {
        res.writeHead(200, { "Content-Type": "text/event-stream" });
        res.end(`data: ${JSON.stringify(chunk)}\n\ndata: ${JSON.stringify(finish)}\n\ndata: [DONE]\n\n`);
      };
      // 644 号：慢速流形态（WALKTHROUGH_MOCK_STREAM_DELAY_MS>0 时启用）——正文
      // 按 8 字符/块逐块发射，块间 delay；finish chunk 与 [DONE] 收尾保持 OpenAI
      // 流式规范（548 号）。引擎侧 abort 断开连接时 close 事件清 timer（真停链路：
      // 引擎中止 → provider 断流 → 管线消费中止）。
      const emitSseSlow = (content, finish) => {
        res.writeHead(200, { "Content-Type": "text/event-stream" });
        const delta = (c) => `data: ${JSON.stringify({ choices: [{ delta: { content: c }, finish_reason: null }] })}\n\n`;
        if (!(STREAM_DELAY_MS > 0)) {
          res.end(`${delta(content)}data: ${JSON.stringify(finish)}\n\ndata: [DONE]\n\n`);
          return;
        }
        const pieces = [];
        for (let i = 0; i < content.length; i += 8) pieces.push(content.slice(i, i + 8));
        let idx = 0;
        const timer = setInterval(() => {
          if (res.writableEnded || res.destroyed) {
            clearInterval(timer);
            return;
          }
          if (idx < pieces.length) {
            res.write(delta(pieces[idx++]));
          } else {
            clearInterval(timer);
            res.end(`data: ${JSON.stringify(finish)}\n\ndata: [DONE]\n\n`);
          }
        }, STREAM_DELAY_MS);
        // 连接提前断开（引擎 abort→provider 断流）即停发射。挂 res 不挂 req：
        // IncomingMessage 的 close 在请求体消费完（end 后）就触发，挂 req 会把
        // timer 立刻清掉、流永不推进（644 号走查首跑挂死根因）。
        res.on("close", () => clearInterval(timer));
      };
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
      else if (sys.includes("修稿编辑")) {
        // 646 号：审改循环修稿腿。auto 模式输出节按 reviser 路由指令探测：
        // patch-only（650 号，AUDIT_SCOPE=local 全 local issue → 「只输出
        // PATCHES」）→ PATCHES 局部修补形态；rewrite-only / allow-full →
        // REVISED_CONTENT 整章重写形态。
        // 须排在「审稿」分支之前：修稿 persona 是「修稿编辑」但其任务描述
        // 含「根据审稿意见对章节进行修正」，includes("审稿") 会先截胡
        // （首跑实录：修稿调用拿到审稿 JSON → REVISED_CONTENT 为空 →
        // 「未产出新内容」退出循环）。
        if (sys.includes("只输出 PATCHES")) {
          // TARGET 必须是 WRITER 正文的精确引用（applySpotFixPatches 先精确
          // 后空白归一匹配；匹配不到会被 skip，全 skip 即「未产出新内容」）。
          content = "=== FIXED_ISSUES ===\n替换了首段一处生硬动作描写，语义不变。\n\n=== PATCHES ===\n--- PATCH 1 ---\nTARGET_TEXT:\n" + PATCH_TARGET + "\nREPLACEMENT_TEXT:\n" + PATCH_REPLACEMENT + "\n--- END PATCH ---";
        } else {
          content = "=== FIXED_ISSUES ===\n压缩了开篇铺陈，冲突提前入场，章尾钩子保留。\n\n=== REVISED_CONTENT ===\n" + REVISED_BODY;
        }
      }
      else if (sys.includes("审稿")) {
        // 645 号：对齐 continuity 审稿 JSON 契约（parseAuditResult 四策略均要
        // 求 JSON：passed/overall_score/issues/summary）。旧形态 "PASS\n95"
        // 恒 parseFailed → 审改循环跳过 + 章节 audit-failed（644 走查实录）。
        // 646 号：WALKTHROUGH_MOCK_AUDIT_SCORES 注入按调用次序递进取分，
        // <85 时 passed:false + structural issue，真实触发审改循环修稿轮。
        let score = 88;
        if (AUDIT_SCORES.length > 0) {
          score = AUDIT_SCORES[Math.min(AUDIT_CALL_SEQ, AUDIT_SCORES.length - 1)];
          AUDIT_CALL_SEQ += 1;
        }
        const passed = score >= 85;
        content = JSON.stringify({
          passed,
          overall_score: score,
          issues: passed ? [] : [{
            severity: "critical",
            repair_scope: AUDIT_SCOPE,
            category: AUDIT_SCOPE === "local" ? "措辞" : "开篇拖沓",
            description: AUDIT_SCOPE === "local"
              ? `mock 注入降分（第 ${AUDIT_CALL_SEQ} 次审稿 ${score} 分）：首段「握紧拳头」动作描写生硬，措辞需要局部打磨。`
              : `mock 注入降分（第 ${AUDIT_CALL_SEQ} 次审稿 ${score} 分）：开篇铺陈过长，进入主线偏慢，冲突入场偏晚。`,
            suggestion: AUDIT_SCOPE === "local"
              ? "替换该句动作为更具体的身体反应。"
              : "压缩首段铺陈，让坊市冲突提前入场，保留玉符异象作为章尾钩子。",
          }],
          summary: passed
            ? "开篇冲突清晰，主线推进扎实，节奏与伏笔承接到位。"
            : `结构完成度不足（mock 注入 ${score} 分），需修稿后复审。`,
        });
      }
      else if (sys.includes("小说连续性分析师")) {
        // 650 号：buildPersistenceOutput→ChapterAnalyzer（审改循环修订后
        // finalContent≠初稿时被调）——此前无分派落入 else 确认卡文本，
        // analyzer 拿到非法载荷后管线静默悬挂（修订产物从未落盘，646/650
        // 全中）。对齐其 === TAG === 输出契约：content 由 persistenceOutput
        // 强制回写审改后 finalContent（此处占位不毁正文）；UPDATED_HOOKS 为
        // 保真形态（651 号，与 fixture 预置池同源——占位会把伏笔池写空致
        // promises 投影缺失、fixture 断言红）。
        // 652 号：CHAPTER_SUMMARY=「已选章节摘要证据」已有行回显+当前章新行
        // （章号取自 mock 进程内最后一次 settler delta）——缺 TAG 会让
        // parseWriterOutput 提取空串覆盖 chapter_summaries.md（650b 实录缩水：
        // chapter_summaries 587→341、current_state 432→292；audit_drift
        // 761→313 系审计注入面正常差异）。证据行的出场人物列 mock 固定为主角。
        const evidenceRows = (sys.match(/^- story\/chapter_summaries\.md#\d+: .*$/gm) ?? [])
          .map((line) => line.replace(/^- story\/chapter_summaries\.md#\d+: /, ""))
          .map((rest) => {
            const parts = rest.split(" | ");
            return `| ${parts[0] ?? ""} | ${parts[1] ?? ""} | 苏檀 | ${parts[2] ?? ""} | ${parts[3] ?? ""} | ${parts[4] ?? ""} | 紧张 | 推进章 | 6 | 4 |`;
          });
        const summaryRows = [...new Set([...evidenceRows, `| ${LAST_SETTLER_CHAPTER} | 风起 | 林动 | 坊市夺回祖符，玉符异象初显 | 踏上修炼路 | H01 推进 | 紧张 | 推进章 | 6 | 4 |`])].join("\n");
        content = "=== CHAPTER_TITLE ===\n风起\n\n=== CHAPTER_CONTENT ===\n（正文以审改后版本为准。）\n\n=== PRE_WRITE_CHECK ===\n\n=== POST_SETTLEMENT ===\n分析模式无结算。\n\n=== UPDATED_STATE ===\n| 字段 | 值 |\n|------|-----|\n| 当前章节 | " + LAST_SETTLER_CHAPTER + " |\n| 当前位置 | 青阳坊→回窑洞 |\n| 主角状态 | 引气入体、气感初成 |\n| 当前目标 | 七日内应对雷家寻仇 |\n| 当前限制 | 修为浅薄 |\n| 当前敌我 | 与雷家结仇 |\n| 当前冲突 | 祖符来历待揭 |\n\n=== UPDATED_LEDGER ===\n\n=== UPDATED_HOOKS ===\n" + ANALYZER_HOOKS_TABLE
          + "\n\n=== CHAPTER_SUMMARY ===\n| 章节 | 标题 | 出场人物 | 关键事件 | 状态变化 | 伏笔动态 | 情绪基调 | 章节类型 | 冲突强度 | 揭示强度 |\n| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |\n" + summaryRows
          + "\n\n=== UPDATED_SUBPLOTS ===\n\n=== UPDATED_EMOTIONAL_ARCS ===\n\n=== UPDATED_CHARACTER_MATRIX ===\n";
      }
      else if (sys.includes("状态追踪分析师")) content = settlerDelta(msgs);
      else if (sys.includes("continuity validator")) content = "PASS";
      else if (sys.includes("事实提取专家")) content = "（无新增可观察事实变化。）"; // 483 号：observer 阶段锚点（空内容会被 TS 管线判空流致命）
      else if (lastRole === "user") {
        // propose_action 工具调用（231 号协议：action 必填 + createBook 结构化）。
        // toolCallId 必须逐次唯一（436 号）：前端确认卡锁定键=execId（派生自
        // toolCallId），固定 id 会让后续同形提议复用首轮"已执行"锁，卡直接
        // 锁死不可确认——真 LLM 每次 tool call id 均不同，mock 必须对齐。
        const toolCall = { id: `call_propose_${++PROPOSE_SEQ}`, type: "function", function: { name: "propose_action", arguments: ARGS } };
        if (!wantStream) {
          emitJson({ role: "assistant", content: null, tool_calls: [toolCall] }, "tool_calls");
          return;
        }
        const chunk = { choices: [{ delta: { tool_calls: [{ index: 0, id: toolCall.id, function: toolCall.function }] }, finish_reason: null }] };
        // 548 号（R30）：finish chunk 为 OpenAI 流式规范必需——pi-ai 0.87 严检
        // "Stream ended without finish_reason"（0.73 宽容缺失，0.87 起报错）。
        const finish = { choices: [{ delta: {}, finish_reason: "tool_calls" }] };
        emitSse(chunk, finish);
        return;
      } else {
        content = "已生成确认卡，请在下方点击确认。";
      }
      if (!wantStream) {
        emitJson({ role: "assistant", content }, "stop");
        return;
      }
      // 644 号：正文类响应走慢速流形态（delay=0 时一次性发射，与旧 emitSse 等价）。
      emitSseSlow(content, { choices: [{ delta: {}, finish_reason: "stop" }] });
    });
    return;
  }
  res.writeHead(404).end();
}).listen(PORT, () => console.log(`walkthrough mock on ${PORT}`));
