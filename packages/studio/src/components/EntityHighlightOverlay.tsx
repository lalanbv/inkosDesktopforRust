import { useDeferredValue, useEffect, useMemo, useRef, useState } from "react";
// 子路径 import（组件惯例——barrel 会把 node 依赖模块拉进浏览器 bundle）。
import { scanEntityHighlights, type EntityHighlightHit } from "@actalk/inkos-core/utils/entity-highlight";
import { tr } from "../lib/app-language";

/**
 * R52/1656 号：编辑器实体高亮 overlay——textarea 背后同排版镜像 div 层
 * （命中段铺背景色，正文由上层 bg-transparent textarea 渲染=零双影），
 * 悬停命中段出实体卡摘要浮层（只读）。v8 深化三 R52 设计要点落地：
 * - **同源样式单源化**：排版类（字体/字号/行高/padding）从
 *   `EDITOR_TYPOGRAPHY_CLASS` 单源引用（ChapterReader textarea 同引），
 *   字体行高漂移错位风险收口。
 * - **滚动同步**：textarea onScroll → 镜像层 scrollTop/scrollLeft 复制
 *   （排版同源+浏览器自算 soft wrap=零手动行高度量）。
 * - **大章双闸**：>20000 码元禁用（命中集防抖重扫 useDeferredValue +
 *   镜像 DOM 停渲染两闸）。
 * - 命中集 = Codex 名册 tracking 语义（大小写敏感+exclude 条目级+最长
 *   优先，纯函数 golden 锁定）。
 */

/** 排版单源（textarea 与镜像层同引——错位即同漂）。 */
export const EDITOR_TYPOGRAPHY_CLASS =
  "font-serif text-lg leading-[1.8] p-6";

/** 大章禁用阈值（码元；v8 R52 风险对策双闸之二）。 */
export const ENTITY_OVERLAY_MAX_CHARS = 20_000;

interface CardLike {
  readonly name: string;
  readonly aliases?: ReadonlyArray<string>;
  readonly kind?: string;
  readonly summary?: string;
  readonly facts?: ReadonlyArray<string>;
}

export function EntityHighlightOverlay({
  text,
  cards,
  textareaRef,
}: {
  text: string;
  cards: ReadonlyArray<CardLike>;
  /** 宿主 textarea ref（滚动同步源）。 */
  textareaRef: React.RefObject<HTMLTextAreaElement | null>;
}) {
  const mirrorRef = useRef<HTMLDivElement | null>(null);
  const [hoverHit, setHoverHit] = useState<EntityHighlightHit | null>(null);
  // 双闸之一：编辑防抖（并发 deferred——快速键入时命中集滞后一拍）。
  const deferredText = useDeferredValue(text);

  // 大章闸：>20000 码元整体停渲染（含镜像 DOM 与扫描）。
  const disabled = deferredText.length > ENTITY_OVERLAY_MAX_CHARS;

  const hits = useMemo(
    () => (disabled ? [] : scanEntityHighlights(deferredText, cards, { minLength: 2 })),
    [deferredText, cards, disabled],
  );

  // 镜像分段：普通段透明文本 + 命中段（背景 + 悬停事件）。码点数组取段
  // ——hit 下标为码点口径（astral 文本 slice(UTF-16) 会错位，golden 同口径）。
  const codepoints = useMemo(() => Array.from(deferredText), [deferredText]);
  const segments = useMemo(() => {
    const out: Array<{ text: string; hit?: EntityHighlightHit }> = [];
    let cursor = 0;
    for (const hit of hits) {
      if (hit.start > cursor) out.push({ text: codepoints.slice(cursor, hit.start).join("") });
      out.push({ text: codepoints.slice(hit.start, hit.end).join(""), hit });
      cursor = hit.end;
    }
    if (cursor < codepoints.length) out.push({ text: codepoints.slice(cursor).join("") });
    return out;
  }, [hits, codepoints]);

  // 滚动同步：textarea 滚动 → 镜像复制（挂 window scroll 兜底不必要——
  // textarea 自身滚动容器）。
  useEffect(() => {
    const area = textareaRef.current;
    const mirror = mirrorRef.current;
    if (!area || !mirror) return;
    const sync = () => {
      mirror.scrollTop = area.scrollTop;
      mirror.scrollLeft = area.scrollLeft;
    };
    sync();
    area.addEventListener("scroll", sync);
    return () => area.removeEventListener("scroll", sync);
  }, [textareaRef]);

  const hoverCard = hoverHit === null ? null : cards[hoverHit.cardIndex] ?? null;

  return (
    <div className="absolute inset-0 pointer-events-none" data-entity-overlay={disabled ? "off" : "on"}>
      {disabled ? (
        <span className="sr-only">{tr("大章已禁用实体高亮", "Entity highlighting disabled for large chapter")}</span>
      ) : (
        <div
          ref={mirrorRef}
          aria-hidden="true"
          className={`absolute inset-0 overflow-hidden whitespace-pre-wrap break-words text-transparent ${EDITOR_TYPOGRAPHY_CLASS}`}
        >
          {segments.map((segment, index) =>
            segment.hit ? (
              <span
                key={index}
                data-entity-hit={segment.hit.entity}
                className="rounded bg-primary/25 pointer-events-auto cursor-help"
                onMouseEnter={() => setHoverHit(segment.hit ?? null)}
                onMouseLeave={() => setHoverHit(null)}
              >
                {segment.text}
              </span>
            ) : (
              <span key={index}>{segment.text}</span>
            ),
          )}
        </div>
      )}
      {hoverCard ? (
        <div
          role="tooltip"
          data-entity-card="hover"
          className="absolute left-1/2 -translate-x-1/2 top-2 z-20 max-w-sm rounded-xl border border-border/60 bg-card p-3 text-xs text-foreground shadow-lg pointer-events-auto"
        >
          <div className="font-bold">{hoverCard.name}</div>
          {hoverCard.kind ? <div className="text-[10px] uppercase tracking-wider text-muted-foreground mt-0.5">{hoverCard.kind}</div> : null}
          {hoverCard.summary ? <p className="mt-1 text-muted-foreground">{hoverCard.summary}</p> : null}
          {hoverCard.facts && hoverCard.facts.length > 0 ? (
            <ul className="mt-1 list-disc pl-4 text-muted-foreground">
              {hoverCard.facts.slice(0, 3).map((fact, factIndex) => (
                <li key={factIndex}>{fact}</li>
              ))}
            </ul>
          ) : null}
          {hoverCard.aliases && hoverCard.aliases.length > 0 ? (
            <div className="mt-1 text-[10px] text-muted-foreground">{tr("别名", "Aliases")}: {hoverCard.aliases.join(" / ")}</div>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
