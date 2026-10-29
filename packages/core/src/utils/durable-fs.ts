/**
 * 落盘持久性助手（761 号，镜像 Rust 760 号 `write_tmp_durable` /
 * `sync_parent_dir_best_effort`，双栈行为对齐）。
 *
 * `writeFile` + `rename` 只保证进程崩溃下「目标要么旧要么完整新」；掉电/
 * 内核崩溃下 rename 项（元数据日志）可先于数据块落盘被重放 → 零长/半截
 * 目标。这里把数据 fsync 到稳定存储后再交给 rename，并在 rename 后尽力
 * fsync 父目录（rename 项本身落盘）。
 */
import { open } from "node:fs/promises";

/** 写文件并 fsync：句柄写入 → sync → close（句柄异常路径也保证 close）。 */
export async function writeFileDurable(path: string, data: string | Uint8Array): Promise<void> {
  const handle = await open(path, "w");
  try {
    await handle.writeFile(data);
    await handle.sync();
  } finally {
    await handle.close();
  }
}

/**
 * 尽力 fsync 目录（rename 项落盘）。Windows 不支持打开目录句柄、部分文件
 * 系统不支持目录 fsync——一律吞错：数据已 sync，最坏回退到仅进程崩溃安全
 * 的既有保证，不让已成功的写入因目录 sync 失败而报错。
 */
export async function syncDirBestEffort(dir: string): Promise<void> {
  let handle: Awaited<ReturnType<typeof open>>;
  try {
    handle = await open(dir, "r");
  } catch {
    return;
  }
  try {
    await handle.sync();
  } catch {
    // 尽力语义：目录 fsync 不可用时静默
  } finally {
    await handle.close().catch(() => undefined);
  }
}
