import { useEffect } from 'react';
import { useWorkspaceStore } from '../stores/workspace';

export function useWorkspace() {
  const {
    workspaces,
    currentWorkspaceId,
    loading,
    error,
    fetchWorkspaces,
    createWorkspace,
    switchWorkspace,
    deleteWorkspace,
    addProjectToWorkspace,
  } = useWorkspaceStore();

  // 自动加载工作区列表
  useEffect(() => {
    fetchWorkspaces();
  }, [fetchWorkspaces]);

  const currentWorkspace = workspaces.find((ws) => ws.id === currentWorkspaceId);

  return {
    workspaces,
    currentWorkspace,
    currentWorkspaceId,
    loading,
    error,
    createWorkspace,
    switchWorkspace,
    deleteWorkspace,
    addProjectToWorkspace,
    refetch: fetchWorkspaces,
  };
}
