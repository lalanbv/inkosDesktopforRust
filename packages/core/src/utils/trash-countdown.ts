// R60 第三批/1685 号：回收站倒计时的浏览器安全面——state/trash.ts 里是整条
// node:fs 依赖链，studio 的浏览器组件（TrashPanel / ChapterTrashPanel）经桶导入
// 会把不可摇树的 CJS 全量拉进 vite 图（AsyncLocalStorage 报「browser-external」，
// gate:ts build 腿红）。本模块只依赖纯算术，走独立子路径导出
//（package.json `./utils/trash-countdown`）；判定式本体仍在 state/trash.ts
//（两腿同判定向量 `golden/trash-retention-vectors.json` 钉在那里）。

const DAY_MS = 86_400_000;

/**
 * 界面倒计时：距自动清空还剩几个自然日（向上取整，剩半天也报 1）；
 * `≤ 0` = 已到点（下次启动清扫会清）；时刻解不出来 = null（不显示徽标）。
 */
export function trashDaysLeft(trashedAtIso: string, retentionDays: number, nowMillis: number): number | null {
  const at = Date.parse(trashedAtIso);
  if (!Number.isFinite(at)) return null;
  return Math.ceil((at + retentionDays * DAY_MS - nowMillis) / DAY_MS);
}
