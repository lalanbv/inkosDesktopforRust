import { useEngineKind } from "../hooks/use-engine-kind";
import { tr } from "../lib/app-language";

/**
 * 1668：仅产品引擎（Rust）生效的功能，在 Node 回退腿上的如实标明。
 *
 * 判型见 use-engine-kind。checking/unknown 不渲染——判错型比不标更糟；产品
 * 引擎在位时功能生效，同样不渲染。
 */
export function RustEngineOnlyNotice({ feature }: { readonly feature: string }) {
  const kind = useEngineKind();
  if (kind !== "node") return null;
  return (
    <p
      role="note"
      className="text-[11px] leading-relaxed rounded-md border border-amber-500/40 bg-amber-500/10 px-2 py-1.5 text-amber-700 dark:text-amber-400"
    >
      {tr(
        `${feature}由产品引擎执行。当前为 Node 回退服务：在这里保存的内容不会参与写作与审查。`,
        `${feature} runs on the product engine. The Node fallback server is active, so what you save here does not affect writing or review.`,
      )}
    </p>
  );
}
