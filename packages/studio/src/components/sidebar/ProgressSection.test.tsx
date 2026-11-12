import { describe, expect, it } from "vitest";
import type { SSEMessage } from "../../hooks/use-sse";
import { deriveProgressState } from "./ProgressSection";

// 999 号：进度态全量重放——消除增量累积形态（ContextDock visible 条件挂载族
// 第三面）。dock 开合卸载期间 sse.messages 在 store 继续累积，旧增量形 effect
// 重挂载后只看最后一条，中间 log 步骤完成态全部丢失（进度条回退）。全量重放
// 幂等且卸载期事件不丢。
function log(message: string): SSEMessage {
  return { event: "log", data: { message } } as SSEMessage;
}
function ev(event: string): SSEMessage {
  return { event } as SSEMessage;
}

describe("deriveProgressState 全量重放（999）", () => {
  it("卸载期累积流重放：中间 log 步骤完成态不丢（999 主场景）", () => {
    // 模拟 dock 卸载期间累积的完整消息流——旧增量形只处理最后一条 log C，
    // A/B 的完成态丢失；全量重放 A/B 进 completed、C 为 active。
    const messages = [
      ev("write:start"),
      log("准备章节输入"),
      log("撰写章节草稿"),
      log("落盘最终章节"),
    ];
    const s = deriveProgressState(messages);
    expect(s.operation).toBe("write");
    expect(s.completedSteps.has("准备章节输入")).toBe(true);
    expect(s.completedSteps.has("撰写章节草稿")).toBe(true);
    expect(s.activeStep).toBe("落盘最终章节");
  });

  it("init 序列：book:creating → log → book:created 收敛 INIT 全集", () => {
    const messages = [
      ev("book:creating"),
      log("生成基础设定"),
      log("保存书籍配置"),
      ev("book:created"),
    ];
    const s = deriveProgressState(messages);
    expect(s.operation).toBe("init");
    expect(s.completedSteps.has("生成基础设定")).toBe(true);
    expect(s.completedSteps.has("初始化控制文档")).toBe(true);
    expect(s.activeStep).toBeNull();
  });

  it("write 完成序列：write:complete 收敛 WRITE 全集且 active 清空", () => {
    const messages = [
      ev("write:start"),
      log("撰写章节草稿"),
      ev("write:complete"),
    ];
    const s = deriveProgressState(messages);
    expect(s.operation).toBe("write");
    expect(s.completedSteps.has("撰写章节草稿")).toBe(true);
    expect(s.completedSteps.has("更新章节索引与快照")).toBe(true);
    expect(s.activeStep).toBeNull();
  });

  it("idle 下 log 不累积（operation 未启动）；空流全 idle", () => {
    const s = deriveProgressState([log("无关日志")]);
    expect(s.operation).toBe("idle");
    expect(s.completedSteps.size).toBe(0);
    expect(s.activeStep).toBeNull();
    const empty = deriveProgressState([]);
    expect(empty.operation).toBe("idle");
    expect(empty.completedSteps.size).toBe(0);
  });

  it("全量重放幂等：同输入两次调用结果一致（重挂载重放安全）", () => {
    const messages = [
      ev("write:start"),
      log("准备章节输入"),
      log("撰写章节草稿"),
    ];
    const a = deriveProgressState(messages);
    const b = deriveProgressState(messages);
    expect(a.operation).toBe(b.operation);
    expect([...a.completedSteps].sort()).toEqual([...b.completedSteps].sort());
    expect(a.activeStep).toBe(b.activeStep);
  });
});
