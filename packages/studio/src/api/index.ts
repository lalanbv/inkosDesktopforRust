import { startStudioServer } from "./server.js";
import { resolve, join, dirname } from "node:path";
import { existsSync } from "node:fs";
import { execSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));

const root = resolve(process.argv[2] ?? process.env.INKOS_PROJECT_ROOT ?? process.cwd());
const port = parseInt(process.env.INKOS_STUDIO_PORT ?? "4567", 10);

// Find studio package root (2 levels up from src/api/)
const studioRoot = resolve(__dirname, "../..");
const distDir = join(studioRoot, "dist");

// Auto-build frontend if dist/ doesn't exist
if (!existsSync(join(distDir, "index.html"))) {
  console.log("Building frontend...");
  try {
    execSync("npx vite build", { cwd: studioRoot, stdio: "inherit" });
  } catch {
    console.error("Failed to build frontend. Run 'cd packages/studio && pnpm build' manually.");
    process.exit(1);
  }
}

startStudioServer(root, port, { staticDir: distDir }).catch((e) => {
  const message = e instanceof Error ? e.message : String(e);
  // 510 号：常见启动失败给双语可行动指引；未知错误保留完整堆栈便于排障。
  if (message.includes("inkos.json not found")) {
    console.error(
      [
        "",
        `启动失败：${root} 不是 InkOS 项目目录（缺 inkos.json）。`,
        "Failed to start studio: not an InkOS project directory (inkos.json missing).",
        "",
        "请改用项目根启动：把项目目录作为第一个参数传入，或设置 INKOS_PROJECT_ROOT。",
        "Pass the project root as the first CLI argument, or set INKOS_PROJECT_ROOT.",
        "尚未创建项目时，先在目标目录执行 inkos init。",
      ].join("\n"),
    );
  } else {
    console.error("Failed to start studio:", e);
  }
  process.exit(1);
});
