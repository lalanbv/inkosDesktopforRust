import { useMemo, useState } from "react";
import { CheckCircle2, Circle, Loader2 } from "lucide-react";
import { fetchJson, useApi } from "../hooks/use-api";
import { useColors } from "../hooks/use-colors";
import { useServiceStore } from "../store/service";
import { tr } from "../lib/app-language";
import type { Theme } from "../hooks/use-theme";
import type { Nav } from "../lib/nav";
import {
  computeOnboardingSteps,
  isOnboardingComplete,
  isOnboardingUnlocked,
  markOnboardingComplete,
  type OnboardingStepId,
} from "./onboarding-state";

/**
 * R9/370 号：新手创作向导（首跑三步：厂商 → Key → 能力探测）。
 * 探测复用既有 POST /services/:service/test（probe 管道）；步骤勾选读真实
 * 状态（services/config + secret + doctor llmConnected）；探测通过放行
 * AI 功能（完成标记 + Doctor llmConnected 双保险）。
 */
export function OnboardingWizard({ nav, theme }: { nav: Nav; theme: Theme }) {
  const c = useColors(theme);
  const { services } = useServiceStore();
  const doctor = useApi<{ llmConnected: boolean }>("/doctor");

  const [selectedServiceId, setSelectedServiceId] = useState<string>("");
  const [apiKey, setApiKey] = useState("");
  const [baseUrl, setBaseUrl] = useState("");
  const [probedOk, setProbedOk] = useState(false);
  const [probing, setProbing] = useState(false);
  const [probeError, setProbeError] = useState("");
  const [completed, setCompleted] = useState(isOnboardingComplete());

  const configuredServiceIds = useMemo(
    () => services.filter((service) => service.connected).map((service) => service.service),
    [services],
  );

  const [hasSecret, setHasSecret] = useState(false);
  void (async () => {
    if (!selectedServiceId) return;
    try {
      const data = await fetchJson<{ apiKey: string }>(
        `/services/${encodeURIComponent(selectedServiceId)}/secret`,
      );
      setHasSecret(Boolean(data.apiKey));
    } catch {
      setHasSecret(false);
    }
  })();

  const steps = computeOnboardingSteps({
    configuredServiceIds,
    selectedServiceId: selectedServiceId || undefined,
    selectedServiceHasSecret: hasSecret,
    llmConnected: doctor.data?.llmConnected ?? false,
    probedOk,
  });
  const unlocked = isOnboardingUnlocked(steps) || completed;

  const runProbe = async () => {
    if (!selectedServiceId) return;
    setProbing(true);
    setProbeError("");
    try {
      const result = await fetchJson<{ ok: boolean; error?: string }>(
        `/services/${encodeURIComponent(selectedServiceId)}/test`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ apiKey, baseUrl: baseUrl || undefined }),
        },
      );
      if (result.ok) {
        setProbedOk(true);
        markOnboardingComplete();
        setCompleted(true);
      } else {
        setProbeError(result.error ?? tr("探测未通过", "Probe failed"));
      }
    } catch (error) {
      setProbeError(String(error));
    }
    setProbing(false);
  };

  const stepIcon = (id: OnboardingStepId, index: number) => {
    const step = steps.find((item) => item.id === id)!;
    return step.done ? (
      <CheckCircle2 size={20} className="text-emerald-500" />
    ) : probing && id === "probe" ? (
      <Loader2 size={20} className="animate-spin text-muted-foreground" />
    ) : (
      <Circle size={20} className="text-muted-foreground" data-step={index} />
    );
  };

  return (
    <div className="max-w-2xl mx-auto space-y-6" data-slot="onboarding-wizard">
      <div>
        <h1 className="text-lg font-bold">{tr("新手创作向导", "Onboarding wizard")}</h1>
        <p className="text-xs text-muted-foreground mt-1">
          {tr(
            "三步接入模型服务：选择厂商 → 填入 Key → 能力探测。探测通过后即可使用 AI 写作。",
            "Connect a model service in three steps: pick a provider, enter the key, run the capability probe. AI writing unlocks once the probe passes.",
          )}
        </p>
      </div>

      <div className="rounded-lg border border-border/60 p-4 space-y-3">
        {stepIcon("provider", 1)}
        <div className="flex items-start gap-3" data-slot="step-provider">
          <div className="flex-1 space-y-1">
            <h2 className="text-sm font-bold">1. {tr("选择厂商", "Pick a provider")}</h2>
            <div className="grid grid-cols-2 sm:grid-cols-3 gap-2">
              {services.map((service) => (
                <button
                  key={service.service}
                  onClick={() => setSelectedServiceId(service.service)}
                  className={`rounded-md border px-2 py-2 text-xs text-left ${
                    selectedServiceId === service.service
                      ? "border-primary ring-1 ring-primary"
                      : "border-border"
                  }`}
                >
                  <div className="font-bold">{service.label}</div>
                  <div className="text-[10px] text-muted-foreground">
                    {service.connected
                      ? tr("已连接", "connected")
                      : service.apiKeyOptional
                        ? tr("Key 可选", "key optional")
                        : tr("需配置", "needs setup")}
                  </div>
                </button>
              ))}
            </div>
          </div>
        </div>

        <div className="flex items-start gap-3" data-slot="step-key">
          {stepIcon("key", 2)}
          <div className="flex-1 space-y-1">
            <h2 className="text-sm font-bold">2. {tr("填入 API Key", "Enter the API key")}</h2>
            <input
              value={apiKey}
              onChange={(event) => setApiKey(event.target.value)}
              placeholder={tr("sk-…", "sk-…")}
              className="w-full text-xs border border-border rounded-md px-2 py-1.5 bg-background font-mono"
            />
            <input
              value={baseUrl}
              onChange={(event) => setBaseUrl(event.target.value)}
              placeholder={tr("自定义 Base URL（可选）", "Custom base URL (optional)")}
              className="w-full text-xs border border-border rounded-md px-2 py-1.5 bg-background font-mono"
            />
            {hasSecret && (
              <p className="text-[11px] text-emerald-600">
                {tr("该服务商已保存 Key，可直接探测。", "A key is already saved for this provider — you can probe directly.")}
              </p>
            )}
          </div>
        </div>

        <div className="flex items-start gap-3" data-slot="step-probe">
          {stepIcon("probe", 3)}
          <div className="flex-1 space-y-1">
            <h2 className="text-sm font-bold">3. {tr("能力探测", "Capability probe")}</h2>
            <button
              onClick={runProbe}
              disabled={!selectedServiceId || probing}
              className="px-3 py-1.5 text-xs rounded-md bg-primary text-primary-foreground disabled:opacity-30"
            >
              {probing ? tr("探测中…", "Probing…") : tr("开始探测", "Run probe")}
            </button>
            {probeError && <p className="text-[11px] text-destructive">{probeError}</p>}
            {unlocked && (
              <p className="text-[11px] text-emerald-600 font-bold" data-slot="onboarding-unlocked">
                {tr("AI 功能已解锁——去创建你的第一本书吧！", "AI features unlocked — create your first book!")}
              </p>
            )}
          </div>
        </div>
      </div>

      <div className="flex items-center gap-3">
        <button
          onClick={() => nav.toBookCreate()}
          disabled={!unlocked}
          className="px-4 py-2 text-sm font-bold rounded-lg bg-primary text-primary-foreground disabled:opacity-30"
          data-slot="onboarding-to-book-create"
        >
          {tr("去建书", "Create a book")}
        </button>
        <button onClick={() => nav.toDashboard()} className={`text-xs ${c.link}`}>
          {tr("稍后再说，回首页", "Later, back to dashboard")}
        </button>
      </div>
    </div>
  );
}
