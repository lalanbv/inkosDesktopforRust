import type {
  AgentTool,
  AgentToolResult,
  AgentToolUpdateCallback,
} from "@mariozechner/pi-agent-core";

/**
 * 1018 号：mz 0.73.1 的 AgentTool 泛型逆变收紧——execute params 从 any 收到
 * `Static<TParameters>`，`AgentTool<any, any>` 容器（工具注册/包装面）与具体
 * 参数化工具之间不再互赋（源工具的窄参数函数不可赋给会被传 unknown 的目标）。
 *
 * 仓内「任意工具容器」统一用本宽形态：execute 的 params 保持 any——运行时
 * 形状由各工具的 parameters schema 与 Value 校验链（tool-arguments/
 * structured-arguments）保证，容器面不重复声明参数类型。
 */
export type AnyAgentTool = Omit<AgentTool<any, any>, "execute"> & {
  readonly execute: (
    toolCallId: string,
    params: any,
    signal?: AbortSignal,
    onUpdate?: AgentToolUpdateCallback<any>,
  ) => Promise<AgentToolResult<any>>;
};
