import { z } from "zod";

/**
 * 书籍时间线数据面（181 号 C4-b，对标 Plottr 的 plotline × chapter 矩阵）。
 *
 * - 落盘位置：`books/{id}/story/timeline.json`
 * - cells 稀疏存储：只为有 beats 的章建条目，`chapter` 为章号（≥1）
 * - 生成端点（write-next 顺手产出 beats）默认关闭，待产品决策——当前数据
 *   只来自手动 PUT（C4-c 编辑回写）或外部写入
 */

export const TimelineCellSchema = z.object({
  chapter: z.number().int().min(1),
  title: z.string().min(1).max(200).optional(),
  note: z.string().max(2_000).optional(),
});

export const TimelinePlotlineSchema = z.object({
  id: z.string().min(1).max(64),
  name: z.string().min(1).max(120),
  cells: z.array(TimelineCellSchema).default([]),
});

export const TimelineSchema = z
  .object({
    version: z.literal(1),
    bookId: z.string().min(1),
    updatedAt: z.string().min(1),
    plotlines: z.array(TimelinePlotlineSchema).max(32),
  })
  .refine(
    (timeline) => {
      const ids = timeline.plotlines.map((line) => line.id);
      return new Set(ids).size === ids.length;
    },
    { message: "plotline ids must be unique" },
  );

export type TimelineCell = z.infer<typeof TimelineCellSchema>;
export type TimelinePlotline = z.infer<typeof TimelinePlotlineSchema>;
export type Timeline = z.infer<typeof TimelineSchema>;
