import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';

export interface Workspace {
  id: string;
  name: string;
  created_at: string;
  last_used: string;
  projects: string[];
}

interface WorkspaceState {
  workspaces: Workspace[];
  currentWorkspaceId: string | null;
  loading: boolean;
  error: string | null;

  // Actions
  fetchWorkspaces: () => Promise<void>;
  createWorkspace: (name: string) => Promise<void>;
  switchWorkspace: (id: string) => Promise<void>;
  deleteWorkspace: (id: string) => Promise<void>;
  addProjectToWorkspace: (workspaceId: string, projectPath: string) => Promise<void>;
}

export const useWorkspaceStore = create<WorkspaceState>((set, get) => ({
  workspaces: [],
  currentWorkspaceId: null,
  loading: false,
  error: null,

  fetchWorkspaces: async () => {
    set({ loading: true, error: null });
    try {
      const workspaces = await invoke<Workspace[]>('cmd_list_workspaces');
      set({ workspaces, loading: false });
    } catch (error) {
      set({ error: String(error), loading: false });
    }
  },

  createWorkspace: async (name: string) => {
    set({ loading: true, error: null });
    try {
      const newWorkspace = await invoke<Workspace>('cmd_create_workspace', { name });
      set((state) => ({
        workspaces: [...state.workspaces, newWorkspace],
        loading: false,
      }));
    } catch (error) {
      set({ error: String(error), loading: false });
      throw error;
    }
  },

  switchWorkspace: async (id: string) => {
    set({ loading: true, error: null });
    try {
      await invoke('cmd_switch_workspace', { workspaceId: id });

      // 加载工作区配置
      await invoke('load_workspace_config', { workspaceId: id });

      set({ currentWorkspaceId: id, loading: false });

      // 重新获取工作区列表（更新 last_used）
      await get().fetchWorkspaces();
    } catch (error) {
      set({ error: String(error), loading: false });
      throw error;
    }
  },

  deleteWorkspace: async (id: string) => {
    set({ loading: true, error: null });
    try {
      await invoke('cmd_delete_workspace', { workspaceId: id });
      set((state) => ({
        workspaces: state.workspaces.filter((ws) => ws.id !== id),
        currentWorkspaceId: state.currentWorkspaceId === id ? null : state.currentWorkspaceId,
        loading: false,
      }));
    } catch (error) {
      set({ error: String(error), loading: false });
      throw error;
    }
  },

  addProjectToWorkspace: async (workspaceId: string, projectPath: string) => {
    set({ loading: true, error: null });
    try {
      await invoke('cmd_add_project_to_workspace', { workspaceId, projectPath });

      // 重新获取工作区列表
      await get().fetchWorkspaces();

      set({ loading: false });
    } catch (error) {
      set({ error: String(error), loading: false });
      throw error;
    }
  },
}));
