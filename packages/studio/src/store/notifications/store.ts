import { create } from "zustand";

/**
 * 通知中心（P4-3）：分级 INFO/WARN/ERROR，上限 50，未读计数。
 * 头插（最新在前）；纯函数内核可测。
 */

export type NotificationLevel = "info" | "warn" | "error";

export interface StudioNotification {
  readonly id: string;
  readonly level: NotificationLevel;
  readonly title: string;
  readonly detail?: string;
  readonly at: number;
  readonly read: boolean;
}

export interface PushNotificationInput {
  readonly level: NotificationLevel;
  readonly title: string;
  readonly detail?: string;
}

export const NOTIFICATIONS_LIMIT = 50;

/** 纯函数：头插 + 上限截断。 */
export function applyNotificationPush(
  list: ReadonlyArray<StudioNotification>,
  input: PushNotificationInput,
  now: number,
  cap: number = NOTIFICATIONS_LIMIT,
): ReadonlyArray<StudioNotification> {
  const entry: StudioNotification = {
    id: `nt-${now}-${Math.random().toString(36).slice(2, 8)}`,
    level: input.level,
    title: input.title,
    detail: input.detail,
    at: now,
    read: false,
  };
  return [entry, ...list].slice(0, cap);
}

/** 纯函数：未读数。 */
export function countUnread(list: ReadonlyArray<StudioNotification>): number {
  return list.reduce((sum, item) => sum + (item.read ? 0 : 1), 0);
}

interface NotificationsStore {
  notifications: ReadonlyArray<StudioNotification>;
  pushNotification: (input: PushNotificationInput) => void;
  markAllRead: () => void;
  clearNotifications: () => void;
}

export const useNotificationsStore = create<NotificationsStore>()((set, get) => ({
  notifications: [],

  pushNotification: (input) => {
    set({ notifications: applyNotificationPush(get().notifications, input, Date.now()) });
  },

  markAllRead: () => {
    if (get().notifications.every((item) => item.read)) return;
    set({ notifications: get().notifications.map((item) => ({ ...item, read: true })) });
  },

  clearNotifications: () => {
    if (get().notifications.length === 0) return;
    set({ notifications: [] });
  },
}));
