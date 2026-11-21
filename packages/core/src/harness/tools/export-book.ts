import type { AgentTool } from "@mariozechner/pi-agent-core";
import { Type, type Static } from "@sinclair/typebox";
import { writeExportArtifact, type ExportStateLike } from "../../interaction/export-artifact.js";

const ExportBookParameters = Type.Object({
  format: Type.Optional(Type.Union([
    Type.Literal("txt"),
    Type.Literal("md"),
    Type.Literal("epub"),
    // R50/1647 号：md-zip 打包格式补入工具面（此前仅 GET /export 可达）。
    Type.Literal("md-zip"),
  ])),
  // R50/1647 号：章节勾选子集（GET /export `chapters` CSV 同语义——正整数
  // 过滤、缺省=全部；调用方须自行过滤非正项，与 server 端一致）。
  chapters: Type.Optional(Type.Array(Type.Integer())),
  // R50/1647 号：仅导出 approved 章（fb8c8e3e 遗失回填；置位时 chapters
  // 被忽略——与 Rust build_export_artifact 互斥优先级一致）。
  approvedOnly: Type.Optional(Type.Boolean()),
});

type ExportBookInput = Static<typeof ExportBookParameters>;

export function createExportBookTool(
  state: ExportStateLike,
  bookId: string,
  options: { readonly outputPath?: string } = {},
): AgentTool<typeof ExportBookParameters> {
  return {
    name: "export_book",
    label: "Export Work",
    description: "Export this long-form Work as Markdown, text, EPUB, or a zipped Markdown bundle; chapters selects a chapter subset.",
    parameters: ExportBookParameters,
    async execute(_toolCallId, input: ExportBookInput) {
      const details = await writeExportArtifact(state, bookId, {
        format: input.format ?? "txt",
        ...(input.chapters ? { chapterFilter: input.chapters } : {}),
        ...(input.approvedOnly ? { approvedOnly: true } : {}),
        ...(options.outputPath ? { outputPath: options.outputPath } : {}),
      });
      return {
        content: [{
          type: "text",
          text: `Exported "${bookId}": ${details.chaptersExported} chapters, ${details.totalWords} counted units to ${details.outputPath}. Delivery checks: ${details.delivery.status}.`,
        }],
        details,
      };
    },
  };
}
