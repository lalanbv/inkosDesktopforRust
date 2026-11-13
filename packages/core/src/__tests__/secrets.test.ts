import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { loadSecrets, saveSecrets, getServiceApiKey } from "../llm/secrets.js";
import { mkdtemp, rm, mkdir, writeFile, readFile } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";

// 1015 号：测试凭据二轮治理（L3 新拦截面）——写盘 fixture 值走 env 注入+
// 非凭据回退（855 惯用形二轮）；fixture 与断言两侧引用同一常量。
const TEST_KEY = process.env.INKOS_TEST_LLM_KEY ?? "test-key-placeholder";
const KEY_TEST = `${TEST_KEY}-test`;
const KEY_DEEP = `${TEST_KEY}-deep`;
const KEY_OLD = `${TEST_KEY}-old`;
const KEY_NEW = `${TEST_KEY}-new`;
const KEY_FROM_FILE = `${TEST_KEY}-from-file`;
const KEY_CUSTOM = `${TEST_KEY}-custom`;

describe("secrets", () => {
  let root: string;

  beforeEach(async () => {
    root = await mkdtemp(join(tmpdir(), "inkos-secrets-"));
  });

  afterEach(async () => {
    await rm(root, { recursive: true, force: true });
  });

  describe("loadSecrets", () => {
    it("returns empty when .inkos/secrets.json does not exist", async () => {
      const secrets = await loadSecrets(root);
      expect(secrets).toEqual({ services: {} });
    });

    it("reads existing secrets file", async () => {
      await mkdir(join(root, ".inkos"), { recursive: true });
      await writeFile(
        join(root, ".inkos", "secrets.json"),
        JSON.stringify({ services: { moonshot: { apiKey: KEY_TEST } } }),
      );
      const secrets = await loadSecrets(root);
      expect(secrets.services.moonshot.apiKey).toBe(KEY_TEST);
    });
  });

  describe("saveSecrets", () => {
    it("creates .inkos dir and writes secrets file", async () => {
      await saveSecrets(root, {
        services: { deepseek: { apiKey: KEY_DEEP } },
      });
      const raw = await readFile(join(root, ".inkos", "secrets.json"), "utf-8");
      const parsed = JSON.parse(raw);
      expect(parsed.services.deepseek.apiKey).toBe(KEY_DEEP);
    });

    it("overwrites existing secrets file", async () => {
      await mkdir(join(root, ".inkos"), { recursive: true });
      await writeFile(
        join(root, ".inkos", "secrets.json"),
        JSON.stringify({ services: { old: { apiKey: KEY_OLD } } }),
      );
      await saveSecrets(root, {
        services: { new: { apiKey: KEY_NEW } },
      });
      const secrets = await loadSecrets(root);
      expect(secrets.services.new.apiKey).toBe(KEY_NEW);
      expect(secrets.services.old).toBeUndefined();
    });
  });

  describe("getServiceApiKey", () => {
    it("returns key from secrets.json first", async () => {
      await mkdir(join(root, ".inkos"), { recursive: true });
      await writeFile(
        join(root, ".inkos", "secrets.json"),
        JSON.stringify({ services: { moonshot: { apiKey: KEY_FROM_FILE } } }),
      );
      const key = await getServiceApiKey(root, "moonshot");
      expect(key).toBe(KEY_FROM_FILE);
    });

    it("falls back to environment variable", async () => {
      vi.stubEnv("MOONSHOT_API_KEY", "test-key-from-env");
      const key = await getServiceApiKey(root, "moonshot");
      expect(key).toBe("test-key-from-env");
      vi.unstubAllEnvs();
    });

    it("returns null when neither secrets nor env exists", async () => {
      const key = await getServiceApiKey(root, "moonshot");
      expect(key).toBeNull();
    });

    it("handles custom service with colon key format", async () => {
      await mkdir(join(root, ".inkos"), { recursive: true });
      await writeFile(
        join(root, ".inkos", "secrets.json"),
        JSON.stringify({
          services: { "custom:内网GPT": { apiKey: KEY_CUSTOM } },
        }),
      );
      const key = await getServiceApiKey(root, "custom:内网GPT");
      expect(key).toBe(KEY_CUSTOM);
    });
  });
});
