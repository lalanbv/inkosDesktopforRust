# M5c 前端配置管理 UI 实现

**日期**: 2026-08-06  
**里程碑**: Phase 3 / M5c  
**类型**: 前端 UI（配置管理界面）

## 变更概述

实现完整的前端配置管理界面，支持三层配置切换、实时编辑、验证、持久化，与后端配置命令完全集成。

## 核心变更

### 1. 配置状态管理（src-ui/stores/config.ts）

**新增 useConfigStore**：
```typescript
interface ConfigState {
  config: AppConfig | null;
  currentLayer: ConfigLayer;
  loading: boolean;
  error: string | null;
  validationErrors: Record<string, string>;

  fetchConfig: () => Promise<void>;
  updateConfig: (layer: ConfigLayer, config: AppConfig) => Promise<void>;
  resetConfig: (layer: ConfigLayer) => Promise<void>;
  setCurrentLayer: (layer: ConfigLayer) => void;
  validateField: (field: string, value: any) => string | null;
}
```

**类型定义**：
- `AppConfig` - 完整配置结构（logging / updates / engine / network）
- `ConfigLayer` - 配置层级（System / Workspace / Project）

**验证规则**：
- 日志级别：trace / debug / info / warn / error
- 更新通道：stable / beta / dev
- 文件大小：1-100 MB
- 备份数量：1-20
- 启动超时：5-300 秒
- 连接超时：1-60 秒
- 请求超时：5-300 秒

**后端集成**：
- `get_config` - 获取合并后的配置
- `update_config` - 更新指定层配置
- `reset_config` - 重置指定层配置

### 2. 配置 Hook（src-ui/hooks/useConfig.ts）

**新增 useConfig**：
```typescript
export function useConfig() {
  // 自动加载配置
  // 暴露状态 + 操作函数
  return {
    config,
    currentLayer,
    loading,
    error,
    validationErrors,
    updateConfig,
    resetConfig,
    setCurrentLayer,
    validateField,
    refetch,
  };
}
```

**特性**：
- 自动加载配置（组件挂载时）
- 统一的错误处理
- 验证错误状态管理

### 3. 配置面板组件（src-ui/components/ConfigPanel.tsx）

**核心功能**：

**三层切换**：
```tsx
<div className="layer-tabs">
  <button className={currentLayer === 'System' ? 'active' : ''}>系统默认</button>
  <button className={currentLayer === 'Workspace' ? 'active' : ''}>工作区</button>
  <button className={currentLayer === 'Project' ? 'active' : ''}>项目</button>
</div>
```

**配置编辑**：
- 日志配置：级别 / 文件大小 / 备份数
- 更新配置：通道 / 自动检查
- Engine 配置：启动超时
- 网络配置：连接超时 / 请求超时

**实时验证**：
```typescript
const handleFieldChange = (field: string, value: any) => {
  const error = validateField(field, value);
  if (error) {
    setLocalErrors({ ...localErrors, [field]: error });
  } else {
    // 清除错误
  }
  // 更新编辑状态
};
```

**变更检测**：
```typescript
useEffect(() => {
  if (config && editingConfig) {
    setHasChanges(JSON.stringify(config) !== JSON.stringify(editingConfig));
  }
}, [config, editingConfig]);
```

**保存 / 取消 / 重置**：
- 保存：调用 `updateConfig` → 自动重新加载
- 取消：恢复到原始配置
- 重置：弹出确认对话框 → 调用 `resetConfig`

**只读保护**：
- 系统层显示提示："系统默认配置为只读，请切换到工作区或项目层"
- 所有输入禁用（`disabled={currentLayer === 'System'}`）

### 4. 工作区选择器（src-ui/components/WorkspaceSelector.tsx）

**新增功能**：
- 工作区列表展示（名称 / 项目数 / 最后使用时间）
- 创建工作区对话框
- 删除工作区确认
- **工作区切换自动加载配置**：
  ```typescript
  await invoke('cmd_switch_workspace', { workspaceId: id });
  await invoke('load_workspace_config', { workspaceId: id });
  ```

### 5. 工作区状态管理（src-ui/stores/workspace.ts + hooks/useWorkspace.ts）

**新增 useWorkspaceStore**：
```typescript
interface WorkspaceState {
  workspaces: Workspace[];
  currentWorkspaceId: string | null;
  loading: boolean;
  error: string | null;

  fetchWorkspaces: () => Promise<void>;
  createWorkspace: (name: string) => Promise<void>;
  switchWorkspace: (id: string) => Promise<void>;
  deleteWorkspace: (id: string) => Promise<void>;
  addProjectToWorkspace: (workspaceId: string, projectPath: string) => Promise<void>;
}
```

**后端集成**：
- `cmd_list_workspaces` - 列出所有工作区
- `cmd_create_workspace` - 创建工作区
- `cmd_switch_workspace` - 切换工作区
- `cmd_delete_workspace` - 删除工作区
- `cmd_add_project_to_workspace` - 添加项目到工作区
- `load_workspace_config` - 加载工作区配置

### 6. 样式设计（ConfigPanel.css + WorkspaceSelector.css）

**设计原则**（遵循 design_sense）：

**色阶系统**：
```css
--surface-1: #0a0d12;  /* 深底 */
--surface-2: #0f131c;  /* 次级面板 */
--surface-3: #161d2b;  /* 输入框背景 */
--surface-4: #1e2636;  /* 边框 / 悬停 */
--accent: #38bdf8;     /* 主色调（青色） */
--text-primary: #e9ecef;
--text-secondary: #94a3b8;
```

**布局**：
- Grid-first：`grid-template-columns: repeat(auto-fit, minmax(16rem, 1fr))`
- Flex 仅用于组件内部（actions / tabs）
- 圆角：`border-radius: 999px`（按钮）/ `0.75rem`（面板）

**动画**：
```css
transition: all 0.15s;
animation: fadeIn 0.15s ease-out;
animation: slideUp 0.2s ease-out;
```

**响应式**：
- 自动适配表单字段（`auto-fit + minmax`）
- 最大宽度限制（50rem / 28rem）

## 文件清单

### 新增文件（6 个）

1. **src-ui/stores/config.ts** - 配置状态管理
2. **src-ui/stores/workspace.ts** - 工作区状态管理
3. **src-ui/hooks/useConfig.ts** - 配置 Hook
4. **src-ui/hooks/useWorkspace.ts** - 工作区 Hook
5. **src-ui/components/ConfigPanel.tsx** - 配置面板组件
6. **src-ui/components/ConfigPanel.css** - 配置面板样式
7. **src-ui/components/WorkspaceSelector.tsx** - 工作区选择器组件
8. **src-ui/components/WorkspaceSelector.css** - 工作区选择器样式

## 功能验证清单

- [x] 三层配置切换（System / Workspace / Project）
- [x] 系统层只读保护
- [x] 实时字段验证（类型 / 范围）
- [x] 变更检测（保存 / 取消按钮状态）
- [x] 配置保存 → 自动重新加载
- [x] 配置重置 → 确认对话框
- [x] 工作区切换 → 自动加载配置
- [x] 错误提示（API 错误 / 验证错误）
- [x] 加载状态展示
- [x] 工作区 CRUD（创建 / 列表 / 切换 / 删除）
- [ ] 前端集成测试（需手动验证）
- [ ] 端到端流程验证

## 集成示例

### 配置管理流程

```typescript
// 1. 加载配置（自动）
const { config, currentLayer } = useConfig();

// 2. 切换到工作区层
setCurrentLayer('Workspace');

// 3. 修改配置
await updateConfig('Workspace', {
  logging: { level: 'debug', max_file_size_mb: 20, max_backups: 5 },
  updates: { channel: 'stable', auto_check: true },
  engine: { startup_timeout_secs: 30 },
  network: { connect_timeout_secs: 10, request_timeout_secs: 60 },
});

// 4. 重置配置
await resetConfig('Workspace');
```

### 工作区管理流程

```typescript
// 1. 加载工作区列表（自动）
const { workspaces, currentWorkspace } = useWorkspace();

// 2. 创建工作区
await createWorkspace('我的工作区');

// 3. 切换工作区（自动加载配置）
await switchWorkspace('ws-123');

// 4. 删除工作区
await deleteWorkspace('ws-456');
```

## 依赖与影响

**依赖**:
- M5b（配置三层架构）→ 后端命令接口
- M3b（工作区 CRUD）→ 工作区管理命令
- Zustand（状态管理库）
- Tauri invoke API

**影响**:
- **用户体验**: 可视化配置管理，无需手动编辑 TOML
- **配置验证**: 前端 + 后端双重验证
- **工作区隔离**: 每个工作区独立配置

## 后续工作

### M5d：配置热重载（MEDIUM）
- 监听配置文件变更（fs watcher）
- 自动重新加载前端配置
- 前端通知配置更新事件

### M5e：配置导入/导出（LOW）
- 导出配置到 JSON / TOML
- 从文件导入配置
- 配置模板管理

### M6：项目管理集成（HIGH）
- 项目选择时自动加载项目配置
- 项目配置编辑（与工作区配置独立）
- 项目级配置覆盖提示

## 附录：UI 截图说明

### 配置面板布局

```
┌─────────────────────────────────────────────┐
│ 应用配置                                     │
│ [系统默认] [工作区] [项目]  ← 层级切换        │
├─────────────────────────────────────────────┤
│ 日志配置                                     │
│ ┌─────────┐ ┌─────────┐ ┌─────────┐         │
│ │日志级别  │ │文件大小  │ │备份数量 │         │
│ └─────────┘ └─────────┘ └─────────┘         │
│                                             │
│ 更新配置                                     │
│ ┌─────────┐ ┌─────────────┐                │
│ │更新通道  │ │☑ 自动检查   │                │
│ └─────────┘ └─────────────┘                │
│                                             │
│ Engine 配置 / 网络配置 ...                   │
├─────────────────────────────────────────────┤
│ [重置]             [取消] [保存]             │
└─────────────────────────────────────────────┘
```

### 工作区选择器布局

```
┌─────────────────────────────────────────────┐
│ 工作区                          [+ 新建]     │
├─────────────────────────────────────────────┤
│ ┌─────────────────────────────────────┐  [×]│
│ │ 我的工作区 ✓ (已激活)                │     │
│ │ 3 个项目 | 最后使用: 2026-08-06 10:30 │     │
│ └─────────────────────────────────────┘     │
│ ┌─────────────────────────────────────┐  [×]│
│ │ 测试工作区                           │     │
│ │ 1 个项目 | 最后使用: 2026-08-05 14:20 │     │
│ └─────────────────────────────────────┘     │
└─────────────────────────────────────────────┘
```

---

**M5c 完成标志**: 前端配置管理 UI 完整实现 + 工作区选择器集成 + 三层配置切换 + 实时验证 + 后端命令集成就绪。
