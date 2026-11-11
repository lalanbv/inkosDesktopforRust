// 985 号：进程内观测表通用有界插入——超过 cap 按 Map 插入序逐出最旧键
// （Map 保序：首键=最旧）。逐出语义与引擎侧同族：Rust agent_route
// context_meter_registry（713 号）与 pending_session_aborts（984 号）均为
// cap 64 最旧逐出——长驻进程内存有界，TS 镜像对齐。
// 覆写已有键不增长：delete+set 重置插入序（被覆写键变最新在尾）。
export function insertWithOldestEviction<K, V>(
  map: Map<K, V>,
  key: K,
  value: V,
  cap: number,
): void {
  map.delete(key);
  if (map.size >= cap) {
    const oldest = map.keys().next().value;
    if (oldest !== undefined) map.delete(oldest);
  }
  map.set(key, value);
}
