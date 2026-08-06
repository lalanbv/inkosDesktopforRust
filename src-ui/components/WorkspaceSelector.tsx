import React, { useState } from 'react';
import { useWorkspace } from '../hooks/useWorkspace';
import { Workspace } from '../stores/workspace';

interface WorkspaceSelectorProps {
  onWorkspaceChange?: (workspace: Workspace | null) => void;
}

export function WorkspaceSelector({ onWorkspaceChange }: WorkspaceSelectorProps) {
  const {
    workspaces,
    currentWorkspace,
    loading,
    error,
    createWorkspace,
    switchWorkspace,
    deleteWorkspace,
  } = useWorkspace();

  const [isCreateDialogOpen, setIsCreateDialogOpen] = useState(false);
  const [newWorkspaceName, setNewWorkspaceName] = useState('');
  const [deleteConfirmId, setDeleteConfirmId] = useState<string | null>(null);

  const handleCreateWorkspace = async () => {
    if (!newWorkspaceName.trim()) return;

    try {
      await createWorkspace(newWorkspaceName.trim());
      setNewWorkspaceName('');
      setIsCreateDialogOpen(false);
    } catch (error) {
      console.error('创建工作区失败:', error);
    }
  };

  const handleSwitchWorkspace = async (id: string) => {
    try {
      await switchWorkspace(id);
      const workspace = workspaces.find((ws) => ws.id === id);
      onWorkspaceChange?.(workspace || null);
    } catch (error) {
      console.error('切换工作区失败:', error);
    }
  };

  const handleDeleteWorkspace = async (id: string) => {
    try {
      await deleteWorkspace(id);
      setDeleteConfirmId(null);
      if (currentWorkspace?.id === id) {
        onWorkspaceChange?.(null);
      }
    } catch (error) {
      console.error('删除工作区失败:', error);
    }
  };

  const formatDate = (dateStr: string) => {
    try {
      return new Date(dateStr).toLocaleString('zh-CN', {
        year: 'numeric',
        month: '2-digit',
        day: '2-digit',
        hour: '2-digit',
        minute: '2-digit',
      });
    } catch {
      return dateStr;
    }
  };

  if (loading && workspaces.length === 0) {
    return (
      <div className="workspace-selector loading">
        <div className="spinner" />
        <span>加载工作区...</span>
      </div>
    );
  }

  return (
    <div className="workspace-selector">
      <div className="workspace-header">
        <h3>工作区</h3>
        <button
          className="btn-create"
          onClick={() => setIsCreateDialogOpen(true)}
          disabled={loading}
        >
          + 新建
        </button>
      </div>

      {error && (
        <div className="error-message" role="alert">
          {error}
        </div>
      )}

      <div className="workspace-list">
        {workspaces.length === 0 ? (
          <div className="empty-state">
            <p>暂无工作区</p>
            <button onClick={() => setIsCreateDialogOpen(true)}>创建第一个工作区</button>
          </div>
        ) : (
          workspaces.map((workspace) => (
            <div
              key={workspace.id}
              className={`workspace-item ${
                currentWorkspace?.id === workspace.id ? 'active' : ''
              }`}
            >
              <div
                className="workspace-info"
                onClick={() => handleSwitchWorkspace(workspace.id)}
              >
                <div className="workspace-name">{workspace.name}</div>
                <div className="workspace-meta">
                  <span>{workspace.projects.length} 个项目</span>
                  <span>最后使用: {formatDate(workspace.last_used)}</span>
                </div>
              </div>

              {currentWorkspace?.id !== workspace.id && (
                <button
                  className="btn-delete"
                  onClick={(e) => {
                    e.stopPropagation();
                    setDeleteConfirmId(workspace.id);
                  }}
                  disabled={loading}
                  title="删除工作区"
                >
                  ×
                </button>
              )}
            </div>
          ))
        )}
      </div>

      {/* 创建工作区对话框 */}
      {isCreateDialogOpen && (
        <div className="dialog-overlay" onClick={() => setIsCreateDialogOpen(false)}>
          <div className="dialog" onClick={(e) => e.stopPropagation()}>
            <h4>创建新工作区</h4>
            <input
              type="text"
              placeholder="工作区名称"
              value={newWorkspaceName}
              onChange={(e) => setNewWorkspaceName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') handleCreateWorkspace();
                if (e.key === 'Escape') setIsCreateDialogOpen(false);
              }}
              autoFocus
            />
            <div className="dialog-actions">
              <button onClick={() => setIsCreateDialogOpen(false)}>取消</button>
              <button
                onClick={handleCreateWorkspace}
                disabled={!newWorkspaceName.trim() || loading}
                className="btn-primary"
              >
                创建
              </button>
            </div>
          </div>
        </div>
      )}

      {/* 删除确认对话框 */}
      {deleteConfirmId && (
        <div className="dialog-overlay" onClick={() => setDeleteConfirmId(null)}>
          <div className="dialog" onClick={(e) => e.stopPropagation()}>
            <h4>确认删除</h4>
            <p>
              确定要删除工作区 "
              {workspaces.find((ws) => ws.id === deleteConfirmId)?.name}" 吗？
            </p>
            <p className="warning">此操作不可撤销，该工作区的所有项目关联将被清除。</p>
            <div className="dialog-actions">
              <button onClick={() => setDeleteConfirmId(null)}>取消</button>
              <button
                onClick={() => handleDeleteWorkspace(deleteConfirmId)}
                disabled={loading}
                className="btn-danger"
              >
                删除
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
