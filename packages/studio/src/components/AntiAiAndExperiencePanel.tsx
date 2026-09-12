import { useEffect, useState } from "react";
import { fetchJson } from "../hooks/use-api";
import { tr } from "../lib/app-language";

interface AntiAiRule {
  readonly id: string;
  readonly type: "phrase" | "structure" | "rhythm" | "cliche";
  readonly pattern: string;
  readonly isRegex: boolean;
  readonly severity: "critical" | "warning" | "info";
  readonly message: string;
  readonly replacement?: string;
  readonly enabled: boolean;
}

interface ExperienceEntry {
  readonly id: string;
  readonly chapter: number;
  readonly kind: "technique" | "hook" | "pacing" | "dialogue";
  readonly text: string;
  readonly enabled: boolean;
  readonly createdAt: string;
}

const severityClass: Record<AntiAiRule["severity"], string> = {
  critical: "border-rose-500/40 bg-rose-500/10 text-rose-700 dark:text-rose-400",
  warning: "border-amber-500/40 bg-amber-500/10 text-amber-700 dark:text-amber-400",
  info: "border-border/60 bg-muted/30 text-muted-foreground",
};

/**
 * R5/366 号：反AI规则 + G13 经验条目面板（书籍详情挂载）。
 * 规则全量表编辑（写作禁则 + 审查扫描共用）；经验条目 merge 保存
 * （/learn 语义——同文本去重）。
 */
export function AntiAiAndExperiencePanel({ bookId }: { bookId: string }) {
  const [rules, setRules] = useState<AntiAiRule[]>([]);
  const [entries, setEntries] = useState<ExperienceEntry[]>([]);
  const [notice, setNotice] = useState("");
  const [ruleForm, setRuleForm] = useState<{ pattern: string; message: string; replacement: string; severity: AntiAiRule["severity"]; isRegex: boolean }>({
    pattern: "", message: "", replacement: "", severity: "warning", isRegex: false,
  });
  const [entryText, setEntryText] = useState("");

  useEffect(() => {
    void (async () => {
      try {
        const [ruleData, entryData] = await Promise.all([
          fetchJson<{ rules: AntiAiRule[] }>(`/books/${encodeURIComponent(bookId)}/anti-ai-rules`),
          fetchJson<{ entries: ExperienceEntry[] }>(`/books/${encodeURIComponent(bookId)}/experience`),
        ]);
        setRules(ruleData.rules);
        setEntries(entryData.entries);
      } catch {
        // 面板静默
      }
    })();
  }, [bookId]);

  const saveRule = async () => {
    if (!ruleForm.pattern.trim() || !ruleForm.message.trim()) return;
    const id = ruleForm.pattern.trim().toLowerCase().replace(/[^a-z0-9]+/g, "_").replace(/^_+|_+$/g, "").slice(0, 48) || `rule_${Date.now()}`;
    const next: AntiAiRule = {
      id: rules.some((rule) => rule.pattern === ruleForm.pattern.trim())
        ? rules.find((rule) => rule.pattern === ruleForm.pattern.trim())!.id
        : id,
      type: ruleForm.isRegex ? "structure" : "phrase",
      pattern: ruleForm.pattern.trim(),
      isRegex: ruleForm.isRegex,
      severity: ruleForm.severity,
      message: ruleForm.message.trim(),
      ...(ruleForm.replacement.trim() ? { replacement: ruleForm.replacement.trim() } : {}),
      enabled: true,
    };
    const merged = [...rules.filter((rule) => rule.id !== next.id), next];
    try {
      const result = await fetchJson<{ rules: AntiAiRule[] }>(`/books/${encodeURIComponent(bookId)}/anti-ai-rules`, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ rules: merged }),
      });
      setRules(result.rules);
      setRuleForm({ pattern: "", message: "", replacement: "", severity: "warning", isRegex: false });
      setNotice(tr("规则已保存", "Rule saved"));
    } catch {
      setNotice(tr("保存失败（正则或格式错误）", "Save failed (regex or format)"));
    }
  };

  const toggleRule = async (rule: AntiAiRule) => {
    const merged = rules.map((item) => (item.id === rule.id ? { ...item, enabled: !item.enabled } : item));
    try {
      const result = await fetchJson<{ rules: AntiAiRule[] }>(`/books/${encodeURIComponent(bookId)}/anti-ai-rules`, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ rules: merged }),
      });
      setRules(result.rules);
    } catch {
      setNotice(tr("更新失败", "Update failed"));
    }
  };

  const saveEntry = async () => {
    if (!entryText.trim()) return;
    const entry: ExperienceEntry = {
      id: `learned_${Date.now()}`,
      chapter: 0,
      kind: "technique",
      text: entryText.trim(),
      enabled: true,
      createdAt: new Date().toISOString(),
    };
    try {
      const result = await fetchJson<{ entries: ExperienceEntry[] }>(`/books/${encodeURIComponent(bookId)}/experience`, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ entries: [entry] }),
      });
      setEntries(result.entries);
      setEntryText("");
      setNotice(tr("经验已沉淀（同文本自动去重）", "Learned (deduped by text)"));
    } catch {
      setNotice(tr("沉淀失败", "Learn failed"));
    }
  };

  const removeEntry = async (id: string) => {
    try {
      const result = await fetchJson<{ entries: ExperienceEntry[] }>(
        `/books/${encodeURIComponent(bookId)}/experience/${encodeURIComponent(id)}`,
        { method: "DELETE" },
      );
      setEntries(result.entries);
    } catch {
      setNotice(tr("删除失败", "Delete failed"));
    }
  };

  const inputClass = "w-full text-xs border border-border rounded-md px-2 py-1.5 bg-background";

  return (
    <div className="rounded-lg border border-border/60 p-4 space-y-3">
      <h3 className="text-xs font-bold uppercase tracking-wider text-muted-foreground">
        {tr("反AI规则与经验记忆", "Anti-AI rules & experience")}
      </h3>
      {notice && <p className="text-[11px] text-emerald-600">{notice}</p>}

      <div className="space-y-1">
        <p className="text-[11px] font-bold text-muted-foreground">{tr("反AI规则（写作禁则 + 审查扫描）", "Anti-AI rules (writing bans + audit scan)")}</p>
        {rules.map((rule) => (
          <div key={rule.id} className={`flex items-center gap-2 text-xs rounded-md border px-2 py-1.5 ${severityClass[rule.severity]}`}>
            <span className="font-mono font-bold">{rule.pattern}</span>
            <span className="flex-1">{rule.message}</span>
            <button
              onClick={() => void toggleRule(rule)}
              className="px-1.5 py-0.5 text-[10px] rounded border border-current opacity-70"
            >
              {rule.enabled ? tr("停用", "off") : tr("启用", "on")}
            </button>
          </div>
        ))}
        {rules.length === 0 && (
          <p className="text-[11px] text-muted-foreground italic">
            {tr("暂无规则——审查将不做反AI扫描。", "No rules yet — audit will skip anti-AI scan.")}
          </p>
        )}
        <div className="space-y-1 border-t border-border/60 pt-2">
          <input
            value={ruleForm.pattern}
            onChange={(event) => setRuleForm((prev) => ({ ...prev, pattern: event.target.value }))}
            placeholder={tr("匹配模式（文本或正则）", "Pattern (text or regex)")}
            className={inputClass}
          />
          <input
            value={ruleForm.message}
            onChange={(event) => setRuleForm((prev) => ({ ...prev, message: event.target.value }))}
            placeholder={tr("告警文案", "Message")}
            className={inputClass}
          />
          <input
            value={ruleForm.replacement}
            onChange={(event) => setRuleForm((prev) => ({ ...prev, replacement: event.target.value }))}
            placeholder={tr("建议改法（可选）", "Suggested rewrite (optional)")}
            className={inputClass}
          />
          <div className="flex items-center gap-2">
            <select
              value={ruleForm.severity}
              onChange={(event) => setRuleForm((prev) => ({ ...prev, severity: event.target.value as AntiAiRule["severity"] }))}
              className="flex-1 text-xs border border-border rounded-md px-2 py-1.5 bg-background"
            >
              <option value="critical">{tr("严重", "critical")}</option>
              <option value="warning">{tr("警告", "warning")}</option>
              <option value="info">{tr("提示", "info")}</option>
            </select>
            <label className="flex items-center gap-1 text-[11px] text-muted-foreground">
              <input
                type="checkbox"
                checked={ruleForm.isRegex}
                onChange={(event) => setRuleForm((prev) => ({ ...prev, isRegex: event.target.checked }))}
              />
              regex
            </label>
            <button
              onClick={saveRule}
              disabled={!ruleForm.pattern.trim() || !ruleForm.message.trim()}
              className="px-3 py-1.5 text-xs rounded-md bg-primary text-primary-foreground disabled:opacity-30"
            >
              {tr("存规则", "Save rule")}
            </button>
          </div>
        </div>
      </div>

      <div className="space-y-1 border-t border-border/60 pt-2">
        <p className="text-[11px] font-bold text-muted-foreground">
          {tr("本书验证有效的手法（经验记忆，写作时自动注入）", "Proven techniques (auto-injected while writing)")}
        </p>
        {entries.map((entry) => (
          <div key={entry.id} className="flex items-center gap-2 text-xs rounded-md border border-border/60 px-2 py-1.5">
            <span className="flex-1">{entry.text}</span>
            {entry.chapter > 0 && <span className="font-mono text-[10px] text-muted-foreground">ch{entry.chapter}</span>}
            <button
              onClick={() => void removeEntry(entry.id)}
              className="px-1.5 py-0.5 text-[10px] rounded border border-border text-muted-foreground"
            >
              {tr("删", "Del")}
            </button>
          </div>
        ))}
        <div className="flex items-center gap-2">
          <input
            value={entryText}
            onChange={(event) => setEntryText(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") void saveEntry();
            }}
            placeholder={tr("沉淀一条有效手法…", "Learn a technique…")}
            className="flex-1 text-xs border border-border rounded-md px-2 py-1.5 bg-background"
          />
          <button
            onClick={saveEntry}
            disabled={!entryText.trim()}
            className="px-3 py-1.5 text-xs rounded-md bg-primary text-primary-foreground disabled:opacity-30"
          >
            {tr("沉淀", "Learn")}
          </button>
        </div>
      </div>
    </div>
  );
}
