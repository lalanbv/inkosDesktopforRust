/**
 * 1666 号：同一键上的异步任务**进程内串行化**。
 *
 * 用途：「读旧 + 拼接 + 原子写」形态的 jsonl 账本追加（usage 账本 / prompt
 * 版本账本）。这类追加对单次写是原子的，但「读—改—写」整体不是：两个并发
 * 追加各自读到 N 行、各写 N+1 行 = 丢一行。实测 20 路并发追加后只剩 1 行
 * （`ledger-append.flow.test`）。按目标路径排队后，每个追加都读到前一个的
 * 结果。只管进程内——两个进程（两条引擎腿）同时写同一账本不在此列。
 */

const tails = new Map<string, Promise<unknown>>();

/**
 * 在 `key` 的队尾执行 `task`：同一 key 的任务严格先后执行；不同 key 互不
 * 阻塞。前序任务失败不影响后续任务启动；本任务的错误原样抛给调用方。
 */
export function runSerially<T>(key: string, task: () => Promise<T>): Promise<T> {
  const previous = tails.get(key) ?? Promise.resolve();
  const run = previous.then(task, task);
  // 队尾只负责排序，吞掉错误不断链；队尾即本任务时摘除（长驻进程防 Map 无界）。
  const tail: Promise<unknown> = run.catch(() => undefined);
  tails.set(key, tail);
  void tail.then(() => {
    if (tails.get(key) === tail) tails.delete(key);
  });
  return run;
}
