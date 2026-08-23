import type { TabsAction, TabsSnapshot } from "@/lib/tabs-reducer";

export interface TabsStore extends TabsSnapshot {
  dispatch: (action: TabsAction) => void;
}
