import { createServer, type Server } from "node:http";
import { once } from "node:events";
import { afterEach, describe, expect, it } from "vitest";
import { sendWebhook } from "../notify/webhook.js";
import { sendTelegram } from "../notify/telegram.js";
import { sendFeishu } from "../notify/feishu.js";
import { sendWechatWork } from "../notify/wechat-work.js";

/**
 * 899 号：notify 四通道此前裸 fetch 无 signal——端点停摆即无限挂死调用方
 * （898 buffered LLM 同族，预算面不同：通知是小 POST，15s 足够）。真链
 * 停摆服务器断言：env 窗内报错、错误为超时形态、恰好一次请求。
 */

const servers: Server[] = [];
const held: Array<{ destroy: () => void }> = [];

afterEach(async () => {
  delete process.env.INKOS_NOTIFY_TIMEOUT_MS;
  for (const server of servers.splice(0)) {
    server.closeAllConnections();
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
  held.splice(0).forEach((socket) => socket.destroy());
});

/** 停摆端点：接受连接后持有不读不写不关（连接挂着但永不响应）。 */
async function spawnStalledEndpoint(): Promise<string> {
  const server = createServer((_req, _res) => { /* 故意不响应 */ });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  servers.push(server);
  return `http://127.0.0.1:${(server.address() as { port: number }).port}/hook`;
}

describe("notify channels bounded fetch (899)", () => {
  it("ends a stalled webhook at the configured deadline (sendWebhook)", async () => {
    process.env.INKOS_NOTIFY_TIMEOUT_MS = "100";
    const url = await spawnStalledEndpoint();
    const started = Date.now();
    await expect(sendWebhook({ url }, { event: "pipeline-complete", bookId: "b", timestamp: new Date().toISOString() }))
      .rejects.toThrow();
    expect(Date.now() - started).toBeLessThan(5_000);
  }, 20_000);

  it("ends stalled telegram, feishu and wecom sends at the deadline", async () => {
    process.env.INKOS_NOTIFY_TIMEOUT_MS = "100";
    const url = await spawnStalledEndpoint();
    const started = Date.now();
    await expect(sendTelegram({ botToken: "fixture", chatId: "c" }, "hi")).rejects.toThrow();
    await expect(sendFeishu({ webhookUrl: url }, "t", "c")).rejects.toThrow();
    await expect(sendWechatWork({ webhookUrl: url }, "c")).rejects.toThrow();
    expect(Date.now() - started).toBeLessThan(10_000);
  }, 20_000);
});
