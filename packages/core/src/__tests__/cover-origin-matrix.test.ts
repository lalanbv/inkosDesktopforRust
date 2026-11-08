// @vitest-environment node
//! 927 号：origin 策略双端判定对照矩阵（单真源表）——TS 实现读本表逐条
//! 断言；Rust 侧 tests/cover_origin_matrix.rs include_str 同表同断言。
//! 表内用例仅收零网络确定性形态（IP 字面量/scheme/same-origin/域名规则/
//! allowlist/strict）；DNS 解析面由各自单测注入 stub 覆盖。
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { assertCoverDownloadUrlAllowed, resolveCoverDownloadOriginPolicy } from "../pipeline/cover-download-origin.js";

interface MatrixCase {
  readonly url: string;
  readonly policy: "default" | "strict" | "invalid";
  readonly verdict: "allow" | "deny";
  readonly reason?: string;
  readonly extraOrigins?: ReadonlyArray<string>;
}

interface MatrixFile {
  readonly provider: string;
  readonly cases: ReadonlyArray<MatrixCase>;
}

const matrix = JSON.parse(
  await readFile(join(import.meta.dirname, "fixtures", "cover-origin-cases.json"), "utf-8"),
) as MatrixFile;

function policyFor(policy: MatrixCase["policy"], extraOrigins?: ReadonlyArray<string>) {
  const env: NodeJS.ProcessEnv = {};
  if (policy === "strict") env.INKOS_COVER_DOWNLOAD_ORIGIN_POLICY = "strict";
  if (policy === "invalid") env.INKOS_COVER_DOWNLOAD_ORIGIN_POLICY = "banana";
  if (extraOrigins?.length) env.INKOS_COVER_DOWNLOAD_EXTRA_ORIGINS = extraOrigins.join(",");
  return resolveCoverDownloadOriginPolicy(env);
}

describe("cover download origin 判定对照矩阵（927 号，双端单真源表）", () => {
  it(`TS 实现与对照表 ${matrix.cases.length} 条用例逐条一致`, async () => {
    const mismatches: Array<string> = [];
    for (const [index, kase] of matrix.cases.entries()) {
      const policy = policyFor(kase.policy, kase.extraOrigins);
      let actualVerdict: "allow" | "deny";
      let actualReason: string | undefined;
      try {
        await assertCoverDownloadUrlAllowed(kase.url, matrix.provider, policy);
        actualVerdict = "allow";
      } catch (error) {
        actualVerdict = "deny";
        actualReason = (error as { reason?: string }).reason;
      }
      const expected = kase.verdict === "allow" ? "allow" : `deny:${kase.reason}`;
      const got = actualVerdict === "allow" ? "allow" : `deny:${actualReason ?? "?"}`;
      if (expected !== got) mismatches.push(`#${index} ${kase.url} [${kase.policy}] 期望 ${expected} 实得 ${got}`);
    }
    expect(mismatches, mismatches.join("\n")).toEqual([]);
  });
});
