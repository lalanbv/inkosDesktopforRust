import { Sidebar } from "./Sidebar";
import type { Nav } from "../lib/nav";
import type { NavSectionId } from "../lib/nav-sections";
import type { SSEMessage } from "../hooks/use-sse";
import type { TFunction } from "../hooks/use-i18n";

/**
 * 按区渲染的侧面板（P3-1 活动栏布局）。
 * 双轨期直接复用 Sidebar 的 zone 过滤——同一组件两种形态，逻辑零重复；
 * 旧整栏（不传 zone）保留到 P4 走查后删除。
 */
export function SidePanel(props: {
  nav: Nav;
  activePage: string;
  sse: { messages: ReadonlyArray<SSEMessage> };
  t: TFunction;
  zone: NavSectionId;
}) {
  return <Sidebar {...props} zone={props.zone} />;
}
