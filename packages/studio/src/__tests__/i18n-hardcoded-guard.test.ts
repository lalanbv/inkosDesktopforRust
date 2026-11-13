import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import ts from "typescript";
import { describe, expect, it } from "vitest";

/**
 * 1004 号：硬编码 CJK 守卫——AST 级普查器 + 按文件预算台账。
 *
 * 与 727 号 i18n-key-integrity 互补：727 管「词典通道键完整性」，本守卫管
 * 「绕过词典/内联双语通道的裸中文字面量」。扫描口径（AST，非 grep——720 号
 * 单行信号扫描会假阳性，注释/正则/数据面必须逐节点定性）：
 *
 * 合规 i18n 通道（既存惯用形，全部识别为豁免类）：
 *   t("key")               — 词典通道（use-i18n.ts，727 守卫覆盖键完整性）
 *   tr("中文", "English")   — 内联双语通道（lib/app-language.ts，非 React 模块用）
 *   f(…, "中文", expr)      — 位置参数双语对（server.ts pick(lang, zh, en) /
 *                             commands.ts nav(id, zh, en, …) 等文件内 helper）
 *   ["中文", "English"]     — 元组对（parts-builder 工具名映射形）
 *   { label: "中文", en }   — 兄弟 en 属性（AssetLibraryPanel KINDS 形）
 *   { zh: "中文", en: … }   — 双语对象（词典本体形）
 *   const zh/en = { … }    — 双语映射变量（ContextLensPanel tierLabel 形）
 *   isZh ? "中" : "En"      — 语言条件三元/if/switch；…Zh/_ZH 命名常量对
 *
 * 豁免台账（LEDGER）内为人工定性非 UI 文案面（每文件预算 = 硬编码上限，
 * 只许减少不许新增）；台账外文件出现任何硬编码即红。
 */

const SRC_DIR = join(import.meta.dirname ?? ".", "..");
const DICT_FILE = join(SRC_DIR, "hooks", "use-i18n.ts");
const CJK = /[\u4e00-\u9fff]/;
const LANG_COND = /\b(isZh|isEn|isJa|isChinese|zh|lang|language|locale|uiLang|uiLanguage)\b/i;
const ZH_NAME = /(Zh|ZH|_zh|Cn|CN|Chinese)$|ZH_|_ZH\b/;

/** 豁免台账：文件 → { 预算, 理由 }。理由必须写明定性证据链，禁「暂时」式空由。 */
const LEDGER: Readonly<Record<string, { readonly budget: number; readonly reason: string }>> = {
  // 命令面板搜索别名（keywords 数组有意收中英混合检索词，非展示文本）。
  "lib/commands.ts": { budget: 52, reason: "keywords 搜索别名（中英混合检索词表，有意保留）" },
  // truth 文件中文格式注册表与解析（zh map 是资格注册表，EN map 镜像存在；
  // indexOf/replace/compare 解析引擎落盘的中文 truth 文件——数据格式面非 UI 文案）。
  "lib/truth-display.ts": { budget: 20, reason: "zh 注册表（EN 镜像 map 在场）+ truth 文件中文格式解析" },
  // 服务端引擎面：LLM prompt（输出质量语言）、落盘 markdown 格式、开发者日志、
  // 上游详情解析；329-334 为 pick(lang, [...].join(), …) 合规形的分类器盲区。
  "api/server.ts": { budget: 18, reason: "LLM prompt 2+落盘 markdown 4+console 3+格式解析 1+材料标题后缀 1+pick 数组合规形 6；409 协议面已转 EN(本号)" },
  // 修辞特征名按被分析文本语言原生选择（EN_RHETORICAL_PATTERNS 对偶在场；
  // Rust analyze_style 镜像 + golden 向量锁定——内容语言数据非 UI 文案）。
  "api/style-fingerprint.ts": { budget: 6, reason: "内容语言原生特征名（golden 契约面）" },
  // 系列回填落盘 markdown 头（story/series_backfill.md 引擎文件格式）。
  "pages/series-backfill-diff.ts": { budget: 6, reason: "落盘 markdown 格式（引擎文件）" },
  // 启动错误屏刻意中英并排（App.tsx:478 注释备案：语言未知故双语并排）+
  // 语言切换钮「中」自指标签。
  "App.tsx": { budget: 4, reason: "启动错误屏刻意双语（语言未知）+语言钮自指" },
  // 伏笔类型 → CSS 类映射，键为引擎伏笔类型中文名，值为样式非文案。
  "components/sidebar/PendingHooksView.tsx": { budget: 4, reason: "CSS 类映射键（非展示文本）" },
  // 创作语言选择卡：卡片内容即所选语言的样本展示（自指设计）。
  "pages/LanguageSelector.tsx": { budget: 4, reason: "自指语言选择卡（内容=所选语言样本）" },
  // error-copy 职能即 EN 协议 → zh 展示的本地化映射，replacement 本身是 zh 目标文本。
  "lib/error-copy.ts": { budget: 3, reason: "zh 本地化映射目标文本（本文件职能）" },
  // 时间线节拍注入 write-next 的 context（引擎规划输入，语言随书创作语言语义）。
  "pages/BookTimeline.tsx": { budget: 3, reason: "引擎输入 context（创作语言语义面）" },
  // 崩溃边界刻意零 i18n 依赖（文件头注释备案：崩溃源可能是任意 import 链）。
  "components/ErrorBoundary.tsx": { budget: 2, reason: "崩溃边界刻意零依赖（文件头备案）" },
  // 「运行时诊断文件 / Runtime diagnostic」斜杠双语懒形（两语并排可见）。
  "pages/TruthFiles.tsx": { budget: 2, reason: "中/En 斜杠双语懒形（并排可见）" },
  // 通知渠道专名（飞书/企业微信无既有 EN 译名）。
  "pages/project-settings-model.ts": { budget: 2, reason: "产品专名" },
  // CLI 启动失败指引（510 号双语可行动指引，终端输出）。
  "api/index.ts": { budget: 1, reason: "CLI 双语启动指引（终端输出）" },
  // tr("新专名确认卡", …).replace("卡", "")——zh 后缀手术，EN 路径无 CJK 无害。
  "components/RosterCandidatesCard.tsx": { budget: 1, reason: "zh 后缀手术（EN 路径无害）" },
  // 字数单位按内容语言选择（progress.chineseChars>0 即 zh 书显示「字」）非 UI 语言。
  "components/chat/ToolExecutionSteps.tsx": { budget: 1, reason: "内容语言条件（chineseChars>0）非 UI 语言" },
  // 1005 号：WRITE_STOPPED_MESSAGE 协议面已 EN 化（与 Rust write_next_route
  // 字面同形契约）；zh 旧字面仅存于 LEGACY_ZH_WRITE_STOPPED_MESSAGE 兼容判定
  // （zh-named 通道豁免），文件实况 0 处硬编码——预算 0=台账外零容忍同权。
  "shared/write-stopped.ts": { budget: 0, reason: "已清偿（1005 号协议面 EN 化；zh 旧字面为兼容判定常量经 zh-named 通道豁免）" },
};

function classify(node: ts.Node, sf: ts.SourceFile): string {
  const text = node.getText(sf);
  /** 最外层值表达式：穿透模板/括号/as/?? /+/三元分支/数组实参。 */
  const valueExpr = (): ts.Node => {
    let cur: ts.Node = node;
    for (;;) {
      const p = cur.parent;
      if (!p) return cur;
      if (
        ts.isTemplateSpan(p) || ts.isTemplateExpression(p) ||
        ts.isParenthesizedExpression(p) || ts.isAsExpression(p) || ts.isNonNullExpression(p)
      ) { cur = p; continue; }
      if (ts.isConditionalExpression(p) && p.condition !== cur) { cur = p; continue; }
      if (ts.isBinaryExpression(p) && (p.operatorToken.kind === ts.SyntaxKind.PlusToken || p.operatorToken.kind === ts.SyntaxKind.QuestionQuestionToken)) { cur = p; continue; }
      if (ts.isArrayLiteralExpression(p) && ts.isCallExpression(p.parent) && p.parent.arguments.includes(p)) { cur = p; continue; }
      return cur;
    }
  };
  const isAsciiStringExpr = (n?: ts.Node): boolean => {
    if (!n) return false;
    const t = n.getText(sf);
    if (CJK.test(t)) return false;
    return /["'`]/.test(t) && /[A-Za-z]/.test(t);
  };
  const nameOf = (n?: ts.Node): string =>
    n && (ts.isIdentifier(n) || ts.isStringLiteral(n) || ts.isNoSubstitutionTemplateLiteral(n)) ? n.text : "";

  const expr = valueExpr() as ts.Expression;
  // ① 调用实参位次：tr 两参对 / 任意调用的 CJK→ASCII 相邻实参对（pick/nav 形）。
  const call = expr.parent;
  if (call && ts.isCallExpression(call)) {
    const idx = call.arguments.indexOf(expr);
    if (idx >= 0) {
      if (call.expression.getText(sf) === "tr" && call.arguments.length >= 2) {
        return idx === 0 ? "bilingual-tr" : "tr-en-cjk";
      }
      if (isAsciiStringExpr(call.arguments[idx + 1])) return "bilingual-call-pair";
    }
  }
  // ② 元组对 / 兄弟 en 属性。
  if (expr.parent && ts.isArrayLiteralExpression(expr.parent)) {
    const siblings = expr.parent.elements;
    const idx = siblings.indexOf(expr);
    if (idx >= 0 && isAsciiStringExpr(siblings[idx + 1])) return "bilingual-tuple";
  }
  {
    const owner = expr.parent && ts.isPropertyAssignment(expr.parent) ? expr.parent.parent : expr.parent;
    if (owner && ts.isObjectLiteralExpression(owner) &&
      owner.properties.some((p) => ts.isPropertyAssignment(p) && nameOf(p.name) === "en")) {
      return "bilingual-sibling-en";
    }
  }
  // ③ 语言感知上下文爬链。
  let child: ts.Node = node;
  let cur: ts.Node | undefined = node.parent;
  while (cur) {
    if (ts.isPropertyAssignment(cur)) {
      const n = nameOf(cur.name);
      if (/^(zh|en|ja)$/.test(n)) return "bilingual-object";
      if (ZH_NAME.test(n)) return "zh-named";
    }
    if (ts.isVariableDeclaration(cur)) {
      const n = nameOf(cur.name);
      if (/^(zh|en|ja)$/.test(n)) return "bilingual-map-var";
      if (ZH_NAME.test(n)) return "zh-named";
    }
    if (ts.isConditionalExpression(cur) && child !== cur.condition && LANG_COND.test(cur.condition.getText(sf))) return "lang-ternary";
    if (ts.isIfStatement(cur) && child !== cur.expression && LANG_COND.test(cur.expression.getText(sf))) return "lang-if";
    if (ts.isCaseClause(cur)) {
      const sw = cur.parent?.parent;
      if (sw && ts.isSwitchStatement(sw) && LANG_COND.test(sw.expression.getText(sf))) return "lang-switch";
    }
    if ((ts.isFunctionDeclaration(cur) || ts.isMethodDeclaration(cur)) && cur.name && ZH_NAME.test(nameOf(cur.name))) return "zh-named";
    child = cur;
    cur = cur.parent;
  }
  return "hardcoded";
}

interface Row { readonly file: string; readonly line: number; readonly cls: string; readonly text: string }

function scanSource(text: string, relFile: string): Row[] {
  const sf = ts.createSourceFile(relFile, text, ts.ScriptTarget.Latest, true, relFile.endsWith(".tsx") ? ts.ScriptKind.TSX : ts.ScriptKind.TS);
  const rows: Row[] = [];
  const visit = (node: ts.Node): void => {
    const isStr =
      ts.isStringLiteral(node) ||
      ts.isNoSubstitutionTemplateLiteral(node) ||
      node.kind === ts.SyntaxKind.TemplateHead ||
      node.kind === ts.SyntaxKind.TemplateMiddle ||
      node.kind === ts.SyntaxKind.TemplateTail ||
      node.kind === ts.SyntaxKind.JsxText;
    if (isStr) {
      const raw = node.kind === ts.SyntaxKind.JsxText ? node.getText(sf) : (node as ts.StringLiteral).text;
      if (CJK.test(raw)) {
        const { line } = sf.getLineAndCharacterOfPosition(node.getStart(sf));
        rows.push({ file: relFile, line: line + 1, cls: classify(node, sf), text: raw.trim().replace(/\s+/g, " ").slice(0, 50) });
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(sf);
  return rows;
}

function collectRelFiles(dir: string, base: string): string[] {
  const out: string[] = [];
  for (const name of readdirSync(dir)) {
    const full = join(dir, name);
    if (statSync(full).isDirectory()) {
      if (name === "node_modules" || name === "__tests__") continue;
      out.push(...collectRelFiles(full, base));
    } else if (/\.(ts|tsx)$/.test(name) && !/\.test\.tsx?$/.test(name) && join(base, "hooks", "use-i18n.ts") !== full) {
      out.push(relative(base, full));
    }
  }
  return out;
}

/** 分类器自检夹具：每个惯用形一例 + 硬编码必红例（防恒绿）。 */
const FIXTURES: ReadonlyArray<{ readonly name: string; readonly src: string; readonly expectCls: string }> = [
  { name: "tr 双语通道", src: `const a = tr("保存中", "Saving");`, expectCls: "bilingual-tr" },
  { name: "调用实参双语对（pick 形）", src: `declare function pick(lang: string, zh: string, en: string): string;\nconst v = pick(lang, "运行中", "running");`, expectCls: "bilingual-call-pair" },
  { name: "元组对", src: `const m = { read: ["读取文件", "Read file"] };`, expectCls: "bilingual-tuple" },
  { name: "兄弟 en 属性", src: `const k = [{ value: "a", label: "题材基底", en: "Genre base" }];`, expectCls: "bilingual-sibling-en" },
  // 双语对象判定要求无 en 兄弟属性（有 en 兄弟时 sibling-en 先命中，同为豁免类）。
  { name: "双语对象", src: `const d = { zh: "保存" };`, expectCls: "bilingual-object" },
  { name: "双语映射变量", src: `const zh = { a: "本书事实" };\nconst en = { a: "Book facts" };\nconst v = tr(zh.a, en.a);`, expectCls: "bilingual-map-var" },
  { name: "语言条件三元", src: `const v = isZh ? "章节" : "Chapters";`, expectCls: "lang-ternary" },
  { name: "zh 命名常量", src: `const LABEL_ZH = "关系";`, expectCls: "zh-named" },
  // 必红语义例：无任何语言感知上下文的裸中文。分类器若退化为恒绿（全部误判
 // 为豁免类），此例即红——与 848 号「形状卫兵+必红夹具」判据同族。
  { name: "裸中文（必红语义）", src: `const msg = "连接失败";`, expectCls: "hardcoded" },
  { name: "JSX 裸文本（必红语义）", src: `const el = <div>文件不存在</div>;`, expectCls: "hardcoded" },
];

describe("i18n hardcoded CJK guard (1004)", () => {
  it("分类器夹具：每个惯用形与必红语义例逐一定性正确", () => {
    for (const fx of FIXTURES) {
      const rows = scanSource(fx.src, `fixture/${fx.name}.tsx`);
      const cjkRows = rows.filter((r) => r.cls !== undefined);
      expect(cjkRows.length, `夹具「${fx.name}」应恰好产出 1 条 CJK 记录`).toBe(1);
      expect(cjkRows[0]!.cls, `夹具「${fx.name}」分类`).toBe(fx.expectCls);
    }
  });

  it("台账外文件零硬编码；台账内文件不超预算（新增即红）", () => {
    const violations: string[] = [];
    for (const rel of collectRelFiles(SRC_DIR, SRC_DIR)) {
      const rows = scanSource(readFileSync(join(SRC_DIR, rel), "utf-8"), rel).filter((r) => r.cls === "hardcoded");
      const entry = LEDGER[rel];
      if (!entry) {
        if (rows.length > 0) {
          violations.push(
            `${rel}: 出现 ${rows.length} 处裸中文（台账外零容忍）——\n` +
            rows.map((r) => `  :${r.line} ${r.text}`).join("\n") +
            `\n  → 修复：组件内用 t("<域>.<键>")（键登记进 hooks/use-i18n.ts），` +
            `非 React 模块用 tr("中文", "English")（lib/app-language）；` +
            `确属数据面/契约面则记入本守卫 LEDGER 并附定性理由。`,
          );
        }
      } else if (rows.length > entry.budget) {
        violations.push(
          `${rel}: 硬编码 ${rows.length} 处 > 台账预算 ${entry.budget}（${entry.reason}）——\n` +
          rows.map((r) => `  :${r.line} ${r.text}`).join("\n") +
          `\n  → 台账只许减少不许新增：修复新站点，或若确属豁免面则先定性再调预算并同步理由。`,
        );
      }
    }
    expect(violations, `硬编码守卫违规 ${violations.length} 项：\n${violations.join("\n\n")}`).toEqual([]);
  });

  it("台账预算与实况同步（预算虚高=台账失真，一并红）", () => {
    const drift: string[] = [];
    for (const [rel, entry] of Object.entries(LEDGER)) {
      const full = join(SRC_DIR, rel);
      let actual = 0;
      try {
        actual = scanSource(readFileSync(full, "utf-8"), rel).filter((r) => r.cls === "hardcoded").length;
      } catch {
        drift.push(`${rel}: 台账条目指向不存在的文件（文件已移动/删除须同步台账）`);
        continue;
      }
      if (actual !== entry.budget) {
        drift.push(`${rel}: 台账预算 ${entry.budget} ≠ 实况 ${actual}（清偿后须把预算下调到实况，防台账虚高掩盖新增）`);
      }
    }
    expect(drift, `台账漂移 ${drift.length} 项：\n${drift.join("\n")}`).toEqual([]);
  });
});
