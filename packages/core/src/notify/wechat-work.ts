import type { NotifyFormat } from "./format.js";
import { notifyFetch } from "./http.js";

export interface WechatWorkConfig {
  readonly webhookUrl: string;
}

export async function sendWechatWork(
  config: WechatWorkConfig,
  content: string,
  format: NotifyFormat = "markdown",
): Promise<void> {
  const payload = format === "text"
    ? { msgtype: "text", text: { content } }
    : { msgtype: "markdown", markdown: { content } };
  const response = await notifyFetch(config.webhookUrl, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload),
  });

  if (!response.ok) {
    const body = await response.text();
    throw new Error(`WeCom send failed: ${response.status} ${body}`);
  }
}
