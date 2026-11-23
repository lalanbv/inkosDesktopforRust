/**
 * R55/1660 号：per-service 用量账本与预算投影（Rust 镜像 =
 * engine-rs/src/utils/usage_ledger.rs，golden usage-budget-vectors.json 锁定）。
 *
 * - 账本 = `{root}/.inkos/usage-log.jsonl` append-only（行含 **usage 精确值**
 *   ——1647 隐私红线：非 TokenMeter 估算）；dayKey 由消费端计算后入参。
 * - 预算 = services 配置可选软顶键（无计费/硬断语义）；级别 ok/warn/over
 *   （80%/100% 阈）；100% 发送前确认门可关闭（confirmAtBudget=false=仅告警）；
 *   无配置不发确认（负路径）。
 */

export interface UsageLogRow {
  readonly ts: string;
  readonly serviceKey: string;
  readonly model: string;
  readonly promptTokens: number;
  readonly completionTokens: number;
  readonly totalTokens: number;
}

/**
 * 1666 号：baseUrl → 账本 serviceKey（Rust `Url::host_str()` 镜像：**不含端口**，
 * 不可解析回退原值）。写点（chat 流水）与读点（budget-status 的条目匹配）共用
 * 本函数=serviceKey 口径单源——读侧此前用 `URL.host`（含端口），非默认端口端点
 * 的用量匹配不上账本里的行（本地模型/自定义网关都是带端口的 baseUrl）。
 */
export function serviceHostOf(baseUrl: string): string {
  try {
    return new URL(baseUrl).hostname || baseUrl;
  } catch {
    return baseUrl;
  }
}

export interface ServiceBudget {
  readonly dailyTokenBudget?: number;
  readonly monthlyTokenBudget?: number;
  /** 100% 发送前确认门（可关闭=仅告警；缺省开）。 */
  readonly confirmAtBudget?: boolean;
}

export interface ServiceUsage {
  readonly daily: number;
  readonly monthly: number;
  readonly total: number;
}

export type BudgetLevel = "ok" | "warn" | "over";

export interface BudgetStatus {
  readonly serviceKey: string;
  readonly daily: number;
  readonly monthly: number;
  /** 日/月更紧一档比例（0-1，无预算=0）。 */
  readonly ratio: number;
  readonly level: BudgetLevel;
  readonly confirmRequired: boolean;
}

/** 服务用量投影：dayKeyOf 入参映射 + 今日 dayKey（月窗口=今日同月）。 */
export function usageProjection(
  rows: ReadonlyArray<UsageLogRow>,
  dayKeyOf: (row: UsageLogRow) => string,
  todayKey: string,
): Map<string, ServiceUsage> {
  const todayMonth = todayKey.slice(0, 7);
  const out = new Map<string, ServiceUsage>();
  for (const row of rows) {
    const dayKey = dayKeyOf(row);
    const entry = out.get(row.serviceKey) ?? { daily: 0, monthly: 0, total: 0 };
    const next = {
      daily: entry.daily + (dayKey === todayKey ? row.totalTokens : 0),
      monthly: entry.monthly + (dayKey.slice(0, 7) === todayMonth ? row.totalTokens : 0),
      total: entry.total + row.totalTokens,
    };
    out.set(row.serviceKey, next);
  }
  return out;
}

/** 预算判定：无预算键=ok/不确认（确认门负路径）。 */
export function budgetStatus(
  serviceKey: string,
  usage: ServiceUsage,
  budget?: ServiceBudget,
): BudgetStatus {
  if (!budget) {
    return { serviceKey, daily: usage.daily, monthly: usage.monthly, ratio: 0, level: "ok", confirmRequired: false };
  }
  const ratios: number[] = [];
  if (typeof budget.dailyTokenBudget === "number" && budget.dailyTokenBudget > 0) {
    ratios.push(usage.daily / budget.dailyTokenBudget);
  }
  if (typeof budget.monthlyTokenBudget === "number" && budget.monthlyTokenBudget > 0) {
    ratios.push(usage.monthly / budget.monthlyTokenBudget);
  }
  const worst = ratios.length > 0 ? Math.max(...ratios) : 0;
  const level: BudgetLevel = worst >= 1 ? "over" : worst >= 0.8 ? "warn" : "ok";
  return {
    serviceKey,
    daily: usage.daily,
    monthly: usage.monthly,
    ratio: Math.round(worst * 100) / 100,
    level,
    confirmRequired: level === "over" && (budget.confirmAtBudget ?? true),
  };
}
