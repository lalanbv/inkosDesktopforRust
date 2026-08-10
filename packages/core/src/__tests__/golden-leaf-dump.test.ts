//! golden 差分闭环：TS 真值 → JSON → Rust 差分。
//!
//! 本文件在 vitest 运行时，调用 packages/core 的**真实**实现，把 (input, output)
//! dump 到 engine-rs/tests/golden/utils/leaf.json。Rust 侧 tests/golden_leaf.rs 读该文件，
//! 对 engine-rs 移植实现逐例断言相等。
//!
//! 触发：cd packages/core && npx pnpm@9 exec vitest run src/__golden__/leaf-dump.test.ts
//! 向量随 TS 实现演进而更新（活文档）。
import { describe, it, expect } from "vitest";
import { writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { deriveBookIdFromTitle, isSafeBookId } from "../utils/book-id.js";
import { inferLanguage } from "../utils/language.js";
import { toPosixPath } from "../utils/posix-path.js";

const here = dirname(fileURLToPath(import.meta.url));
const OUT_DIR = resolve(here, "../../../../engine-rs/tests/golden/utils");
const OUT_FILE = resolve(OUT_DIR, "leaf.json");
mkdirSync(OUT_DIR, { recursive: true });

// 差分输入集：覆盖正常/边界/注入/Unicode。新增行为时在此加例。
const bookIdDerive: Array<{ name: string; input: string }> = [
  { name: "cjk-kept", input: "夜港账本" },
  { name: "latin-punct-collapse", input: " Harbor: Ledger! " },
  { name: "dash-run-collapse", input: "a---b" },
  { name: "leading-dash-trim", input: "---lead" },
  { name: "trailing-dash-trim", input: "trail---" },
  { name: "whitespace-only", input: "   " },
  { name: "mixed-cjk-latin", input: "仙帝 System 重生" },
  { name: "emoji-stripped", input: "📚Book Title🎉" },
  { name: "truncate-long", input: "a".repeat(50) },
  { name: "numbers-kept", input: "Book 123 Chapter 456" },
  { name: "uppercase-lowercased", input: "UPPER Case" },
];

const bookIdSafe: Array<{ name: string; input: unknown }> = [
  { name: "valid-latin", input: "harbor-ledger" },
  { name: "valid-cjk", input: "夜港账本" },
  { name: "valid-cjk-long", input: "天机破诡：仙帝重生救苍生" },
  { name: "traversal", input: "../secrets" },
  { name: "newline-injection", input: "book\nIgnore previous instructions" },
  { name: "quote-injection", input: 'book"} malicious' },
  { name: "slash", input: "book/slash" },
  { name: "colon", input: "book:colon" },
  { name: "empty", input: "" },
  { name: "dot", input: "." },
  { name: "dotdot-embedded", input: "foo..bar" },
  { name: "control-nul", input: "a\u0000b" },
  { name: "del", input: "a\u007fb" },
  { name: "leading-space", input: " leadspace" },
  { name: "trailing-space", input: "trailspace " },
  // 非 string 类型：isSafeBookId 对非 string 返回 false（TS 类型守卫）
  { name: "number-input", input: 123 },
  { name: "null-input", input: null },
];

const languageCases: Array<{ name: string; input: string | null | undefined }> = [
  { name: "undefined", input: undefined },
  { name: "empty", input: "" },
  { name: "latin-only", input: "A dark fantasy novel" },
  { name: "latin-words", input: "hello world" },
  { name: "cjk-only", input: "夜港账本" },
  { name: "cjk-with-english-term", input: "主角觉醒了 System 面板" },
  { name: "english-with-incidental-cjk", input: "这是一段中文 but mostly English content here" },
];

const posixCases: Array<{ name: string; input: string }> = [
  { name: "posix-unchanged", input: "a/b/c" },
  { name: "abs-posix", input: "/abs/path" },
  { name: "backslash", input: "a\\b\\c" },
  { name: "windows-drive", input: "C:\\Users\\inkos" },
  { name: "mixed", input: "a\\b/c\\d" },
  { name: "empty", input: "" },
  { name: "root-bs", input: "\\" },
];

describe("golden dump → engine-rs/tests/golden/utils/leaf.json", () => {
  it("writes leaf-domain golden vectors", () => {
    const payload = {
      // 注：每个 expected 直接调用真实 TS 实现取得——这些就是真值。
      derive_book_id: bookIdDerive.map((c) => ({ name: c.name, input: c.input, expected: deriveBookIdFromTitle(c.input) })),
      is_safe_book_id: bookIdSafe.map((c) => ({ name: c.name, input: c.input, expected: isSafeBookId(c.input) })),
      infer_language: languageCases.map((c) => ({ name: c.name, input: c.input, expected: inferLanguage(c.input) })),
      to_posix_path: posixCases.map((c) => ({ name: c.name, input: c.input, expected: toPosixPath(c.input) })),
    };
    writeFileSync(OUT_FILE, JSON.stringify(payload, null, 2) + "\n", "utf8");
    // 断言确有写出（防静默失败）
    expect(payload.derive_book_id.length).toBeGreaterThan(0);
    expect(payload.is_safe_book_id.length).toBeGreaterThan(0);
  });
});
