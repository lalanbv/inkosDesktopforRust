// @vitest-environment jsdom
//! 1053 号：ProjectArtifactDrawer 响应竞态守卫回归锁。1007 号审查备案 1 曾定性
//! 「目标快速连切时旧响应可后到覆写 content」预存类，后续轮次修复（effect cleanup
//! `cancelled` 标记，ProjectArtifactDrawer.tsx 的 .then/.catch/.finally 三点守卫），
//! 但无测试上锁。本文件锁定：path 切换后旧目标响应晚到既不得覆写正文，其失败
//! 也不得注入错误横幅覆盖好数据。惯用形与 ChaptersSection.test.tsx（1008 号同族）
//! 一致：deferred 双响应 + stubGlobal fetch + 真实 store action 驱动。
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, render, waitFor } from "@testing-library/react";
import { ProjectArtifactDrawer } from "./ProjectArtifactDrawer";
import { useChatStore } from "../../store/chat";

interface DeferredResponse {
  promise: Promise<Response>;
  resolve: (r: Response) => void;
  reject: (e: unknown) => void;
}

function deferredResponse(): DeferredResponse {
  let resolve!: DeferredResponse["resolve"];
  let reject!: DeferredResponse["reject"];
  const promise = new Promise<Response>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function artifactResponse(path: string, content: string): Response {
  return new Response(JSON.stringify({ path, content, contentType: "application/json", size: content.length }), {
    status: 200,
    headers: { "content-type": "application/json" },
  });
}

describe("ProjectArtifactDrawer 响应竞态守卫（1053）", () => {
  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
    useChatStore.setState({ projectArtifactPath: null });
  });

  it("目标连切：旧目标响应晚到不得覆写新目标正文", async () => {
    const stale = deferredResponse();
    const fresh = deferredResponse();
    vi.stubGlobal(
      "fetch",
      vi.fn().mockReturnValueOnce(stale.promise).mockReturnValueOnce(fresh.promise),
    );

    useChatStore.setState({ projectArtifactPath: "drafts/old.json" });
    const { container } = render(<ProjectArtifactDrawer />);
    // 换目标 → effect 重发 #2（fresh），#1 被 cleanup 置 cancelled
    await act(async () => {
      useChatStore.getState().openProjectArtifact("drafts/new.json");
    });
    await act(async () => {
      fresh.resolve(artifactResponse("drafts/new.json", '{"memo":"新内容版"}'));
    });
    await waitFor(() => expect(container.textContent).toContain("新内容版"));

    // #1（旧目标数据）晚到——不得覆盖
    await act(async () => {
      stale.resolve(artifactResponse("drafts/old.json", '{"memo":"旧内容版"}'));
    });
    expect(container.textContent).toContain("新内容版");
    expect(container.textContent).not.toContain("旧内容版");
  });

  it("过期失败不毁好数据：旧目标请求失败晚于新目标成功 → 正文保持且无错误横幅", async () => {
    const stale = deferredResponse();
    const fresh = deferredResponse();
    vi.stubGlobal(
      "fetch",
      vi.fn().mockReturnValueOnce(stale.promise).mockReturnValueOnce(fresh.promise),
    );

    useChatStore.setState({ projectArtifactPath: "drafts/old.json" });
    const { container } = render(<ProjectArtifactDrawer />);
    await act(async () => {
      useChatStore.getState().openProjectArtifact("drafts/new.json");
    });
    await act(async () => {
      fresh.resolve(artifactResponse("drafts/new.json", '{"memo":"新内容版"}'));
    });
    await waitFor(() => expect(container.textContent).toContain("新内容版"));

    // 旧目标请求失败晚到——cancelled catch 不得注入错误横幅
    await act(async () => {
      stale.reject(new Error("晚到失败 boom"));
    });
    expect(container.textContent).toContain("新内容版");
    expect(container.textContent).not.toContain("晚到失败 boom");
  });
});
