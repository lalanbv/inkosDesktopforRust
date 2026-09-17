#!/usr/bin/env node
// EPUB 导出链结构冒烟（494 号，214 号方法学的活体固化）：
//   node scripts/export-epub-smoke.mjs [--engine node|rust|both] [--port 8893] [--mock-port 1234]
//
// 每个引擎腿：临时根 + fixture + POST /books/:id/export-save（epub）→
// 结构校验（mimetype 首位且 stored / container / OPF 解析 / spine≥2 /
// XML 良构 / 章节正文包含）。任一失败退出码非零。

import { spawn, execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, rmSync, writeFileSync, mkdirSync, openSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

// 515 号：本地引擎挂起时快速失败——统一 20s 超时（SSE 长连接除外）。
const fetchT = (input, init = {}) => fetch(input, { ...init, signal: AbortSignal.timeout(20_000) });
const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(scriptDir, "..");
const studioDir = join(repoRoot, "packages", "studio");
// 517 号：INKOS_SMOKE_RUST_BIN 可指 release 二进制——发布形态活体冒烟。
const rustBinary = process.env.INKOS_SMOKE_RUST_BIN
  ?? join(repoRoot, "engine-rs", "target", "debug", "inkos-engine-server");

const args = process.argv.slice(2);
const argOf = (name, fallback) => {
  const i = args.indexOf(name);
  return i >= 0 && args[i + 1] ? args[i + 1] : fallback;
};
const engineMode = argOf("--engine", "both");
const port = argOf("--port", "8893");
const mockPort = argOf("--mock-port", "1234");
const keep = args.includes("--keep");

const children = [];
const sharedChildren = [];
let failures = 0;

const startChild = (cmd, cmdArgs, opts, logPath, shared = false) => {
  const out = openSync(logPath, "a");
  const child = spawn(cmd, cmdArgs, { ...opts, stdio: ["ignore", out, out], detached: true });
  (shared ? sharedChildren : children).push(child);
  return child;
};

const waitUntil = async (fn, timeoutMs, label) => {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      if (await fn()) return true;
    } catch { /* not ready */ }
    await new Promise((r) => setTimeout(r, 500));
  }
  console.error(`[epub-smoke] 等待超时：${label}`);
  return false;
};

const BOOK = encodeURIComponent("镜花水月");

// 结构校验：python3 + zipfile/XML（214 号检查项子集，纯标准库）。
const VALIDATE_PY = `
import json, sys, zipfile
import xml.etree.ElementTree as ET


path = sys.argv[1]
result = {"ok": False, "checks": {}}
try:
    z = zipfile.ZipFile(path)
    names = z.namelist()
    first = z.infolist()[0]
    result["checks"]["mimetype_first_stored"] = (
        first.filename == "mimetype" and first.compress_type == zipfile.ZIP_STORED)
    result["checks"]["mimetype_value"] = z.read("mimetype").decode() == "application/epub+zip"
    result["checks"]["container"] = "META-INF/container.xml" in names
    container = ET.fromstring(z.read("META-INF/container.xml"))
    rootfile = container.find(".//{urn:oasis:names:tc:opendocument:xmlns:container}rootfile")
    opf_path = rootfile.get("full-path")
    opf = ET.fromstring(z.read(opf_path))
    ns = {"o": "http://www.idpf.org/2007/opf", "dc": "http://purl.org/dc/elements/1.1/"}
    title = opf.find(".//dc:title", ns)
    result["checks"]["opf_title"] = bool(title is not None and title.text)
    spine = opf.findall(".//o:spine/o:itemref", ns)
    result["checks"]["spine_ge_2"] = len(spine) >= 2
    bad = []
    for n in names:
        if n.endswith((".xhtml", ".html", ".opf")):
            try:
                ET.fromstring(z.read(n))
            except Exception as e:
                bad.append([n, str(e)[:60]])
    result["checks"]["xml_well_formed"] = not bad
    result["checks"]["bad_list"] = bad
    checks = result["checks"]
    result["ok"] = all(v is True for k, v in checks.items() if k != "bad_list")
except Exception as e:
    result["error"] = str(e)[:120]
print(json.dumps(result, ensure_ascii=False))
`;

const validateEpub = (epubPath) => {
  const out = execFileSync("python3", ["-c", VALIDATE_PY, epubPath], { encoding: "utf-8" });
  return JSON.parse(out.trim().split("\n").pop());
};

async function runLeg(engine) {
  console.log(`\n[epub-smoke] ── 引擎腿：${engine} ──`);
  // 518 号：腿间端口位移（同 node-fallback-smoke——僵尸 keep-alive 连接隔离）。
  const legPort = Number(port) + legs.indexOf(engine) * 10;
  const base = `http://127.0.0.1:${legPort}`;
  const root = mkdtempSync(join(tmpdir(), `inkos-epub-smoke-${engine}-`));
  mkdirSync(join(root, ".inkos"), { recursive: true });
  writeFileSync(
    join(root, "inkos.json"),
    JSON.stringify({ name: `inkos-epub-smoke-${engine}`, version: "0.1.0", services: [{ service: "custom", name: "Mock", baseUrl: `http://127.0.0.1:${mockPort}/v1` }] }),
  );
  writeFileSync(join(root, ".inkos", "secrets.json"), JSON.stringify({ services: { "custom:Mock": { apiKey: "sk-mock" } } }));

  if (engine === "node") {
    startChild(
      process.execPath,
      [join(studioDir, "node_modules", "tsx", "dist", "cli.mjs"), join(studioDir, "src", "api", "index.ts"), root],
      { cwd: studioDir, env: { ...process.env, INKOS_STUDIO_PORT: legPort } },
      join(root, "server.log"),
    );
  } else {
    startChild(
      rustBinary,
      [],
      {
        cwd: repoRoot,
        env: {
          ...process.env,
          INKOS_PORT: legPort,
          INKOS_PROJECT_ROOT: root,
          INKOS_STATIC_DIR: join(studioDir, "dist"),
          INKOS_LLM_BASE_URL: `http://127.0.0.1:${mockPort}/v1`,
        },
      },
      join(root, "server.log"),
    );
  }
  const up = await waitUntil(async () => {
    try {
      const r = await fetchT(`${base}/api/v1/books`);
      return r.ok;
    } catch (e) {
      console.error(`[epub-smoke][debug] poll err: ${String(e).slice(0, 80)}`);
      return false;
    }
  }, 90_000, `${engine} 启动`);
  check(`${engine} 引擎启动`, up);
  if (!up) throw new Error(`${engine} server failed to start`);

  execFileSync("node", [join(scriptDir, "walkthrough-fixture.mjs"), root, legPort], {
    cwd: repoRoot,
    stdio: ["ignore", "ignore", "inherit"],
  });

  const res = await fetchT(`${base}/api/v1/books/${BOOK}/export-save`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ format: "epub", approvedOnly: false }),
  });
  const meta = await res.json();
  check(`[${engine}] export-save 200 且 ok+chapters`, res.status === 200 && meta.ok === true && meta.format === "epub" && (meta.chapters ?? 0) >= 2);

  const epubPath = meta.path;
  check(`[${engine}] EPUB 落盘`, existsSync(epubPath));
  const result = validateEpub(epubPath);
  if (!result.ok) {
    console.error(`    校验明细：${JSON.stringify(result)}`);
  }
  check(`[${engine}] EPUB 结构校验（214 号检查项）`, result.ok === true);
}

function check(name, ok, detail = "") {
  console.log(`${ok ? "✓" : "✗"} ${name}${detail ? `（${detail}）` : ""}`);
  if (!ok) failures += 1;
}
// ── 编排 ──
const legs = engineMode === "both" ? ["node", ...(existsSync(rustBinary) ? ["rust"] : [])] : [engineMode];
if (engineMode !== "node" && !legs.includes("rust")) {
  console.warn(`[epub-smoke] ⚠ Rust 二进制缺失（${rustBinary}）——rust 腿跳过`);
}

startChild("node", [join(scriptDir, "walkthrough-mock.mjs"), mockPort], { cwd: repoRoot }, join(tmpdir(), "inkos-epub-smoke-mock.log"), true);

try {
  for (const leg of legs) {
    await runLeg(leg);
    for (const child of children.splice(0)) {
      try { try { process.kill(-child.pid, "SIGKILL"); } catch { try { child.kill("SIGKILL"); } catch {} }; } catch { /* exited */ }
    }
    // 腿间留 1s 让端口释放，下一腿 bind 不撞 EADDRINUSE。
    await new Promise((r) => setTimeout(r, 1_000));
  }
} catch (error) {
  failures += 1;
  console.error(`[epub-smoke] 异常中断：${error?.message ?? error}`);
} finally {
  for (const child of children.splice(0)) {
    try { try { process.kill(-child.pid, "SIGKILL"); } catch { try { child.kill("SIGKILL"); } catch {} }; } catch { /* exited */ }
  }
  for (const child of sharedChildren.splice(0)) {
    try { try { process.kill(-child.pid, "SIGKILL"); } catch { try { child.kill("SIGKILL"); } catch {} }; } catch { /* exited */ }
  }
}

if (failures > 0) {
  console.error(`[epub-smoke] ✗ ${failures} 项断言失败`);
  process.exit(1);
}
console.log("[epub-smoke] ✓ EPUB 导出链结构冒烟全部通过");
