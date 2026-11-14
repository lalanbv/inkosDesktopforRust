import { getAppLanguage } from "./app-language";
// 1057 号：映射表提升 core 单源导出（1007 备案②清偿——cli TUI 面同样承接），
// 本壳仅保留 studio 全局语言门控；use-api 消费形态与既有测试锚零变化。
import { localizeKnownRuntimeMessage as localizeKnownRuntimeMessageCore } from "@actalk/inkos-core/llm/error-copy";

export function localizeKnownRuntimeMessage(message: string): string {
  return localizeKnownRuntimeMessageCore(message, getAppLanguage());
}
