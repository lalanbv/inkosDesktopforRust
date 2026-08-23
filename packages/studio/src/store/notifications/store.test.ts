import { describe, expect, it } from "vitest";
import { applyNotificationPush, countUnread, NOTIFICATIONS_LIMIT } from "./store";

describe("applyNotificationPush", () => {
  it("prepends newest and marks it unread", () => {
    const first = applyNotificationPush([], { level: "info", title: "A" }, 1_000);
    const second = applyNotificationPush(first, { level: "warn", title: "B" }, 2_000);
    expect(second.map((item) => item.title)).toEqual(["B", "A"]);
    expect(second[0].read).toBe(false);
    expect(second[0].level).toBe("warn");
  });

  it("caps the list at the notification limit", () => {
    let list = applyNotificationPush([], { level: "info", title: "seed" }, 0);
    for (let i = 0; i < NOTIFICATIONS_LIMIT + 10; i++) {
      list = applyNotificationPush(list, { level: "info", title: `n${i}` }, i + 1);
    }
    expect(list).toHaveLength(NOTIFICATIONS_LIMIT);
    expect(list[0].title).toBe(`n${NOTIFICATIONS_LIMIT + 9}`);
  });
});

describe("countUnread", () => {
  it("counts only unread entries", () => {
    const list = [
      { id: "a", level: "info" as const, title: "x", at: 1, read: false },
      { id: "b", level: "info" as const, title: "y", at: 2, read: true },
      { id: "c", level: "error" as const, title: "z", at: 3, read: false },
    ];
    expect(countUnread(list)).toBe(2);
    expect(countUnread([])).toBe(0);
  });
});
