/**
 * R7 结构化错误目录（368 号契约层，二轮 P2；355 号 §4 R7）。
 *
 * 唯一数据文件 = `data/author-errors.json`（双端共享：TS import JSON；
 * Rust `include_str!` 同一文件）——author-report、通知中心、Doctor 三面
 * 共用同一错误→分类→下一步的映射。
 *
 * 条目 shape：`{code, severity(auto-handled|needs-review|must-handle),
 * patterns[], messageZh/En, nextActionZh/En}`；patterns 为小写关键词
 * （对 `${event} ${message}` 小写串做 includes 匹配）；末条 unknown-error
 * 无 patterns 作为兜底（must-handle）。
 *
 * 双端：`engine-rs/src/utils/author_error_catalog.rs` 1:1 镜像；行为
 * golden = `src/__tests__/golden/author-error-catalog-vectors.json`
 * （TS 断言 `golden-author-error-catalog.test.ts`；Rust 差分
 * `tests/golden_author_error_catalog_diff.rs`）。
 */

import catalogData from "../data/author-errors.json";

export type AuthorErrorSeverity = "auto-handled" | "needs-review" | "must-handle";

export interface AuthorErrorEntry {
  readonly code: string;
  readonly severity: AuthorErrorSeverity;
  readonly patterns: ReadonlyArray<string>;
  readonly messageZh: string;
  readonly messageEn: string;
  readonly nextActionZh: string;
  readonly nextActionEn: string;
}

export interface AuthorErrorCatalog {
  readonly version: number;
  readonly errors: ReadonlyArray<AuthorErrorEntry>;
}

const CATALOG: AuthorErrorCatalog = catalogData as AuthorErrorCatalog;

export function loadAuthorErrorCatalog(): AuthorErrorCatalog {
  return CATALOG;
}

export interface ResolvedAuthorError {
  /** 命中条目 code；兜底条目为 "unknown-error"。 */
  readonly code: string;
  readonly severity: AuthorErrorSeverity;
  readonly message: string;
  readonly nextAction: string;
}

/** 目录查找：patterns 顺序匹配（haystack 小写 includes）；无命中走兜底条目。 */
export function resolveAuthorError(
  event: string,
  message: string,
  language: "zh" | "en" = "zh",
): ResolvedAuthorError {
  const isEn = language === "en";
  const haystack = `${event} ${message}`.toLowerCase();
  const errors = CATALOG.errors;
  let matched: AuthorErrorEntry | undefined;
  for (const entry of errors) {
    if (entry.patterns.some((pattern) => haystack.includes(pattern.toLowerCase()))) {
      matched = entry;
      break;
    }
  }
  if (!matched) {
    matched = errors.find((entry) => entry.patterns.length === 0);
  }
  const fallback = matched ?? errors[errors.length - 1]!;
  return {
    code: fallback.code,
    severity: fallback.severity,
    message: isEn ? fallback.messageEn : fallback.messageZh,
    nextAction: isEn ? fallback.nextActionEn : fallback.nextActionZh,
  };
}

/** 按 severity 取下一步建议（未知 severity 走兜底条目）。 */
export function nextActionFor(severity: AuthorErrorSeverity, language: "zh" | "en" = "zh"): string {
  const entry = CATALOG.errors.find((item) => item.severity === severity);
  const isEn = language === "en";
  if (entry) return isEn ? entry.nextActionEn : entry.nextActionZh;
  const fallback = CATALOG.errors[CATALOG.errors.length - 1]!;
  return isEn ? fallback.nextActionEn : fallback.nextActionZh;
}
