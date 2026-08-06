# Phase 4: 项目管理系统设计

**日期**: 2026-08-06  
**版本**: v1.0  
**目标**: 实现商业级项目发现、索引、管理、健康检查系统

## 一、系统概述

### 1.1 核心目标

项目管理系统是 inkosDesktop 的核心功能之一，负责：

1. **项目发现** - 自动扫描文件系统，识别项目类型
2. **项目索引** - 轻量级数据库存储项目元数据，支持快速查询
3. **项目配置** - 继承工作区配置 + 项目级覆盖
4. **项目健康检查** - 依赖检测、配置验证、环境检查
5. **项目快速启动** - 最近使用、收藏、搜索、分组

### 1.2 设计原则

1. **高性能 0GC** - Rust 原生实现，避免不必要的堆分配
2. **增量扫描** - 只扫描变更部分，缓存扫描结果
3. **异步非阻塞** - 文件系统操作使用 tokio 异步
4. **轻量级存储** - SQLite 嵌入式数据库，无外部依赖
5. **类型安全** - 编译时类型检查，避免运行时错误

### 1.3 技术栈

| 组件 | 技术选型 | 理由 |
|------|---------|------|
| 项目索引数据库 | rusqlite v0.32 | 嵌入式、0 配置、SQL 查询 |
| 文件系统扫描 | walkdir v2.5 | 跨平台、高性能、迭代器 API |
| 异步运行时 | tokio v1.40 | 已有依赖，生态成熟 |
| 元数据序列化 | serde v1.0 | 已有依赖，标准序列化 |
| 项目类型识别 | 自定义规则引擎 | 灵活、可扩展、精准 |

## 二、架构设计

### 2.1 模块划分

```
src/project/
├── mod.rs              // 模块导出
├── types.rs            // 项目类型定义（ProjectType, ProjectMeta, ProjectHealth）
├── detector.rs         // 项目类型识别器（基于文件特征）
├── scanner.rs          // 文件系统扫描器（增量扫描）
├── index.rs            // 项目索引数据库（rusqlite）
├── manager.rs          // 项目生命周期管理器（CRUD + 缓存）
├── health.rs           // 项目健康检查器（依赖 + 配置）
└── commands.rs         // Tauri 命令层
```

### 2.2 数据模型

#### 2.2.1 ProjectType（项目类型）

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjectType {
    NodeJs,      // package.json
    Python,      // requirements.txt / pyproject.toml / setup.py
    Rust,        // Cargo.toml
    Go,          // go.mod
    Java,        // pom.xml / build.gradle
    Dotnet,      // *.csproj / *.sln
    Generic,     // 无特征文件，通用项目
}
```

#### 2.2.2 ProjectMeta（项目元数据）

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMeta {
    pub id: String,                    // UUID v4
    pub name: String,                  // 项目名称（目录名或自定义）
    pub path: PathBuf,                 // 项目根目录绝对路径
    pub project_type: ProjectType,     // 项目类型
    pub workspace_id: Option<String>,  // 所属工作区
    pub created_at: i64,               // 创建时间（Unix 时间戳）
    pub last_opened_at: Option<i64>,   // 最后打开时间
    pub last_scanned_at: Option<i64>,  // 最后扫描时间
    pub is_favorite: bool,             // 是否收藏
    pub tags: Vec<String>,             // 标签（用户自定义）
    pub description: Option<String>,   // 项目描述
}
```

#### 2.2.3 ProjectHealth（项目健康状态）

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectHealth {
    pub project_id: String,
    pub checked_at: i64,                      // 检查时间
    pub status: HealthStatus,                 // 整体状态
    pub issues: Vec<HealthIssue>,             // 问题列表
    pub dependency_count: usize,              // 依赖数量
    pub missing_dependencies: Vec<String>,    // 缺失依赖
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthStatus {
    Healthy,    // 无问题
    Warning,    // 有警告
    Critical,   // 有严重问题
    Unknown,    // 未检查
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthIssue {
    pub severity: IssueSeverity,
    pub category: IssueCategory,
    pub message: String,
    pub suggestion: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IssueSeverity {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IssueCategory {
    DependencyMissing,      // 依赖缺失
    ConfigInvalid,          // 配置无效
    EnvironmentMissing,     // 环境缺失（如 Node.js / Python）
    FilePermission,         // 文件权限问题
    DiskSpace,              // 磁盘空间不足
}
```

### 2.3 数据库 Schema

```sql
-- 项目索引表
CREATE TABLE IF NOT EXISTS projects (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    path TEXT NOT NULL UNIQUE,
    project_type TEXT NOT NULL,
    workspace_id TEXT,
    created_at INTEGER NOT NULL,
    last_opened_at INTEGER,
    last_scanned_at INTEGER,
    is_favorite INTEGER NOT NULL DEFAULT 0,
    tags TEXT,  -- JSON 数组
    description TEXT
);

-- 项目健康检查表
CREATE TABLE IF NOT EXISTS project_health (
    project_id TEXT PRIMARY KEY,
    checked_at INTEGER NOT NULL,
    status TEXT NOT NULL,
    issues TEXT,  -- JSON 数组
    dependency_count INTEGER NOT NULL,
    missing_dependencies TEXT,  -- JSON 数组
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

-- 索引优化
CREATE INDEX IF NOT EXISTS idx_projects_workspace ON projects(workspace_id);
CREATE INDEX IF NOT EXISTS idx_projects_last_opened ON projects(last_opened_at DESC);
CREATE INDEX IF NOT EXISTS idx_projects_favorite ON projects(is_favorite);
CREATE INDEX IF NOT EXISTS idx_projects_type ON projects(project_type);
```

### 2.4 核心组件设计

#### 2.4.1 ProjectDetector（项目类型识别器）

**功能**: 基于文件特征识别项目类型

**识别规则**:

| 项目类型 | 特征文件 | 优先级 |
|---------|---------|--------|
| NodeJs | package.json | 1 |
| Python | pyproject.toml / requirements.txt / setup.py | 2 |
| Rust | Cargo.toml | 3 |
| Go | go.mod | 4 |
| Java | pom.xml / build.gradle | 5 |
| Dotnet | *.csproj / *.sln | 6 |
| Generic | - | 7 |

**接口设计**:

```rust
pub struct ProjectDetector;

impl ProjectDetector {
    /// 检测项目类型（同步，快速）
    pub fn detect(project_root: &Path) -> Result<ProjectType>;
    
    /// 检测项目名称（从 package.json / Cargo.toml 等读取）
    pub fn detect_name(project_root: &Path, project_type: ProjectType) -> Result<String>;
    
    /// 验证项目根目录
    pub fn is_valid_project_root(path: &Path) -> bool;
}
```

**实现要点**:
- 使用 `Path::join` + `Path::exists()` 检查特征文件
- 优先级：Node.js > Python > Rust > Go > Java > .NET > Generic
- 名称提取：解析 package.json / Cargo.toml / pyproject.toml
- 快速失败：无特征文件 → Generic

#### 2.4.2 ProjectScanner（文件系统扫描器）

**功能**: 扫描文件系统，发现项目

**扫描策略**:

1. **全量扫描** - 首次启动或手动触发
2. **增量扫描** - 监听文件系统变更（未来）
3. **深度限制** - 默认最大深度 5，避免扫描整个系统
4. **排除规则** - node_modules / target / .git / build 等

**接口设计**:

```rust
pub struct ProjectScanner {
    max_depth: usize,
    exclude_dirs: Vec<String>,
}

impl ProjectScanner {
    /// 创建扫描器
    pub fn new() -> Self;
    
    /// 配置最大深度
    pub fn with_max_depth(mut self, depth: usize) -> Self;
    
    /// 添加排除目录
    pub fn add_exclude_dir(mut self, dir: String) -> Self;
    
    /// 扫描目录（异步）
    pub async fn scan(&self, root: &Path) -> Result<Vec<ProjectMeta>>;
    
    /// 扫描单个项目（同步）
    pub fn scan_single(&self, path: &Path) -> Result<Option<ProjectMeta>>;
}
```

**实现要点**:
- 使用 `walkdir::WalkDir` 遍历目录
- 检查每个目录是否是项目根（ProjectDetector）
- 跳过排除目录（node_modules / target）
- 限制扫描深度（默认 5 层）
- 异步执行（tokio::task::spawn_blocking）

#### 2.4.3 ProjectIndex（项目索引数据库）

**功能**: 管理项目元数据持久化

**接口设计**:

```rust
pub struct ProjectIndex {
    conn: Connection,  // rusqlite::Connection
}

impl ProjectIndex {
    /// 打开/创建数据库
    pub fn open(db_path: &Path) -> Result<Self>;
    
    /// 插入项目
    pub fn insert(&self, meta: &ProjectMeta) -> Result<()>;
    
    /// 更新项目
    pub fn update(&self, meta: &ProjectMeta) -> Result<()>;
    
    /// 删除项目
    pub fn delete(&self, id: &str) -> Result<()>;
    
    /// 根据 ID 查询
    pub fn get_by_id(&self, id: &str) -> Result<Option<ProjectMeta>>;
    
    /// 根据路径查询
    pub fn get_by_path(&self, path: &Path) -> Result<Option<ProjectMeta>>;
    
    /// 列出所有项目
    pub fn list_all(&self) -> Result<Vec<ProjectMeta>>;
    
    /// 列出工作区项目
    pub fn list_by_workspace(&self, workspace_id: &str) -> Result<Vec<ProjectMeta>>;
    
    /// 列出最近打开
    pub fn list_recent(&self, limit: usize) -> Result<Vec<ProjectMeta>>;
    
    /// 列出收藏项目
    pub fn list_favorites(&self) -> Result<Vec<ProjectMeta>>;
    
    /// 搜索项目（按名称）
    pub fn search(&self, query: &str) -> Result<Vec<ProjectMeta>>;
    
    /// 保存健康检查结果
    pub fn save_health(&self, health: &ProjectHealth) -> Result<()>;
    
    /// 获取健康检查结果
    pub fn get_health(&self, project_id: &str) -> Result<Option<ProjectHealth>>;
}
```

**实现要点**:
- 使用 `rusqlite::Connection` 管理数据库
- 事务支持（批量插入）
- 预编译语句（性能优化）
- JSON 序列化（tags / issues）
- 外键约束（级联删除）

#### 2.4.4 ProjectManager（项目生命周期管理器）

**功能**: 统一项目 CRUD + 缓存

**接口设计**:

```rust
pub struct ProjectManager {
    index: Arc<ProjectIndex>,
    detector: ProjectDetector,
    scanner: ProjectScanner,
    cache: Arc<Mutex<HashMap<String, ProjectMeta>>>,  // 内存缓存
}

impl ProjectManager {
    /// 创建管理器
    pub fn new(db_path: &Path) -> Result<Self>;
    
    /// 扫描并添加项目
    pub async fn scan_and_add(&self, root: &Path) -> Result<Vec<ProjectMeta>>;
    
    /// 添加单个项目
    pub fn add_project(&self, path: &Path) -> Result<ProjectMeta>;
    
    /// 更新项目元数据
    pub fn update_project(&self, meta: &ProjectMeta) -> Result<()>;
    
    /// 删除项目
    pub fn remove_project(&self, id: &str) -> Result<()>;
    
    /// 获取项目（带缓存）
    pub fn get_project(&self, id: &str) -> Result<Option<ProjectMeta>>;
    
    /// 打开项目（更新 last_opened_at）
    pub fn open_project(&self, id: &str) -> Result<ProjectMeta>;
    
    /// 列出所有项目
    pub fn list_projects(&self) -> Result<Vec<ProjectMeta>>;
    
    /// 列出最近打开
    pub fn list_recent(&self, limit: usize) -> Result<Vec<ProjectMeta>>;
    
    /// 收藏/取消收藏
    pub fn toggle_favorite(&self, id: &str) -> Result<()>;
    
    /// 搜索项目
    pub fn search_projects(&self, query: &str) -> Result<Vec<ProjectMeta>>;
}
```

**实现要点**:
- 内存缓存（HashMap）减少数据库查询
- 写穿缓存（Write-Through）保证一致性
- 异步扫描（tokio::task::spawn_blocking）
- 自动刷新缓存（打开 / 更新时）

#### 2.4.5 ProjectHealthChecker（项目健康检查器）

**功能**: 检测项目依赖、配置、环境

**检查项**:

1. **依赖检查** - 解析 package.json / requirements.txt / Cargo.toml
2. **配置检查** - 验证项目配置文件格式
3. **环境检查** - Node.js / Python / Rust 是否安装
4. **权限检查** - 项目目录是否可读写
5. **磁盘空间检查** - 是否有足够空间

**接口设计**:

```rust
pub struct ProjectHealthChecker;

impl ProjectHealthChecker {
    /// 执行健康检查（异步）
    pub async fn check(meta: &ProjectMeta) -> Result<ProjectHealth>;
    
    /// 检查依赖（解析 package.json 等）
    fn check_dependencies(path: &Path, project_type: ProjectType) -> Result<Vec<HealthIssue>>;
    
    /// 检查配置文件
    fn check_config(path: &Path) -> Result<Vec<HealthIssue>>;
    
    /// 检查环境（Node.js / Python 版本）
    fn check_environment(project_type: ProjectType) -> Result<Vec<HealthIssue>>;
    
    /// 检查文件权限
    fn check_permissions(path: &Path) -> Result<Vec<HealthIssue>>;
}
```

**实现要点**:
- 异步执行（tokio::task::spawn_blocking）
- 并行检查多个项（tokio::join!）
- 依赖解析（serde_json / toml）
- 环境检测（which / Command::output）

## 三、任务分解

### M6a: 项目数据结构与数据库 Schema（2-3 小时）

**目标**: 定义项目元数据 + 创建 SQLite 数据库

**任务**:
1. 定义 `ProjectType` / `ProjectMeta` / `ProjectHealth` 类型
2. 创建 `src/project/types.rs`
3. 创建 `src/project/index.rs` + 数据库 Schema
4. 实现 `ProjectIndex` CRUD 操作
5. 单元测试（15+ 用例）

**交付物**:
- `types.rs` - 类型定义
- `index.rs` - 数据库操作
- `tests/project_index.rs` - 测试

### M6b: 项目类型识别器（1-2 小时）

**目标**: 基于文件特征识别项目类型

**任务**:
1. 创建 `src/project/detector.rs`
2. 实现 `ProjectDetector::detect` 方法
3. 实现 `ProjectDetector::detect_name` 方法
4. 支持 6 种项目类型识别
5. 单元测试（10+ 用例）

**交付物**:
- `detector.rs` - 识别器实现
- `tests/project_detector.rs` - 测试

### M6c: 项目扫描器（2-3 小时）

**目标**: 扫描文件系统发现项目

**任务**:
1. 创建 `src/project/scanner.rs`
2. 实现 `ProjectScanner::scan` 异步方法
3. 实现排除目录逻辑
4. 实现深度限制
5. 集成测试（5+ 用例）

**交付物**:
- `scanner.rs` - 扫描器实现
- `tests/project_scanner.rs` - 测试

### M6d: 项目生命周期管理器（2-3 小时）

**目标**: 统一项目 CRUD + 缓存

**任务**:
1. 创建 `src/project/manager.rs`
2. 实现 `ProjectManager` 核心接口
3. 实现内存缓存（HashMap）
4. 集成 `ProjectIndex` + `ProjectDetector` + `ProjectScanner`
5. 集成测试（10+ 用例）

**交付物**:
- `manager.rs` - 管理器实现
- `tests/project_manager.rs` - 测试

### M6e: 项目健康检查器（2-3 小时）

**目标**: 检测项目依赖、配置、环境

**任务**:
1. 创建 `src/project/health.rs`
2. 实现依赖检查（解析 package.json / Cargo.toml）
3. 实现配置检查
4. 实现环境检查（which 命令）
5. 集成测试（8+ 用例）

**交付物**:
- `health.rs` - 健康检查器
- `tests/project_health.rs` - 测试

### M6f: Tauri 命令层 + 前端集成（3-4 小时）

**目标**: Tauri 命令 + React UI

**任务**:
1. 创建 `src/project/commands.rs` - Tauri 命令
2. 注册命令到 main.rs
3. 创建前端 `stores/project.ts` - Zustand store
4. 创建 `hooks/useProjects.ts` - React hook
5. 创建 `components/ProjectList.tsx` - 项目列表 UI
6. 创建 `components/ProjectCard.tsx` - 项目卡片
7. 集成测试

**交付物**:
- `commands.rs` - Tauri 命令
- `stores/project.ts` - 状态管理
- `components/ProjectList.tsx` - UI 组件

## 四、接口定义

### 4.1 Tauri 命令接口

```rust
// 扫描并添加项目
#[tauri::command]
async fn scan_projects(root: String) -> Result<Vec<ProjectMeta>, String>;

// 添加单个项目
#[tauri::command]
fn add_project(path: String) -> Result<ProjectMeta, String>;

// 更新项目元数据
#[tauri::command]
fn update_project(meta: ProjectMeta) -> Result<(), String>;

// 删除项目
#[tauri::command]
fn remove_project(id: String) -> Result<(), String>;

// 获取项目详情
#[tauri::command]
fn get_project(id: String) -> Result<Option<ProjectMeta>, String>;

// 打开项目
#[tauri::command]
fn open_project(id: String) -> Result<ProjectMeta, String>;

// 列出所有项目
#[tauri::command]
fn list_projects() -> Result<Vec<ProjectMeta>, String>;

// 列出最近打开
#[tauri::command]
fn list_recent_projects(limit: usize) -> Result<Vec<ProjectMeta>, String>;

// 切换收藏状态
#[tauri::command]
fn toggle_favorite(id: String) -> Result<(), String>;

// 搜索项目
#[tauri::command]
fn search_projects(query: String) -> Result<Vec<ProjectMeta>, String>;

// 执行健康检查
#[tauri::command]
async fn check_project_health(id: String) -> Result<ProjectHealth, String>;
```

### 4.2 前端接口

```typescript
// Zustand Store
interface ProjectState {
  projects: ProjectMeta[];
  recentProjects: ProjectMeta[];
  currentProject: ProjectMeta | null;
  loading: boolean;
  error: string | null;
  
  // Actions
  fetchProjects: () => Promise<void>;
  fetchRecentProjects: () => Promise<void>;
  scanProjects: (root: string) => Promise<void>;
  addProject: (path: string) => Promise<void>;
  openProject: (id: string) => Promise<void>;
  removeProject: (id: string) => Promise<void>;
  toggleFavorite: (id: string) => Promise<void>;
  searchProjects: (query: string) => Promise<void>;
  checkHealth: (id: string) => Promise<ProjectHealth>;
}

// React Hook
function useProjects() {
  const {
    projects,
    recentProjects,
    currentProject,
    loading,
    error,
    fetchProjects,
    openProject,
    toggleFavorite,
  } = useProjectStore();
  
  return {
    projects,
    recentProjects,
    currentProject,
    loading,
    error,
    openProject,
    toggleFavorite,
    refetch: fetchProjects,
  };
}
```

## 五、非功能需求

### 5.1 性能要求

| 指标 | 目标 | 说明 |
|------|------|------|
| 项目识别速度 | < 10ms / 项目 | 单个项目类型识别 |
| 扫描速度 | < 1s / 1000 个目录 | 文件系统扫描 |
| 数据库查询 | < 5ms / 查询 | 单次查询延迟 |
| 缓存命中率 | > 90% | 内存缓存命中率 |
| 健康检查 | < 500ms / 项目 | 单个项目健康检查 |

### 5.2 可靠性要求

1. **数据一致性** - 缓存与数据库保持一致
2. **错误恢复** - 扫描失败不影响已有数据
3. **事务支持** - 批量操作使用事务
4. **数据备份** - 数据库文件定期备份

### 5.3 可扩展性

1. **项目类型可扩展** - 新增项目类型只需修改 `ProjectType` 枚举
2. **健康检查可扩展** - 新增检查项只需实现新方法
3. **排除规则可配置** - 用户可自定义排除目录

## 六、风险与应对

### 6.1 技术风险

| 风险 | 影响 | 应对策略 |
|------|------|---------|
| 大目录扫描性能 | 扫描时间过长 | 限制深度 + 排除目录 + 异步 |
| 数据库锁竞争 | 并发写入冲突 | 使用 WAL 模式 + 事务 |
| 缓存失效 | 数据不一致 | 写穿缓存 + 定期刷新 |
| 依赖解析错误 | 健康检查失败 | 捕获异常 + 记录日志 |

### 6.2 业务风险

| 风险 | 影响 | 应对策略 |
|------|------|---------|
| 项目类型误识别 | 用户体验差 | 手动修正 + 反馈机制 |
| 隐私泄露 | 扫描用户敏感目录 | 用户确认 + 排除规则 |
| 磁盘空间占用 | 数据库过大 | 定期清理 + 压缩 |

## 七、里程碑时间线

| 里程碑 | 时间 | 交付物 |
|--------|------|--------|
| M6a | Day 1-2 | 数据结构 + 数据库 |
| M6b | Day 2-3 | 项目类型识别器 |
| M6c | Day 3-4 | 项目扫描器 |
| M6d | Day 4-5 | 项目管理器 |
| M6e | Day 5-6 | 健康检查器 |
| M6f | Day 6-8 | Tauri 命令 + 前端 |
| **Phase 4 完成** | **Day 8** | 完整项目管理系统 |

## 八、验收标准

- [ ] 支持 6 种项目类型识别（Node.js / Python / Rust / Go / Java / .NET）
- [ ] 支持文件系统扫描（深度限制 + 排除目录）
- [ ] 支持项目 CRUD 操作（创建 / 读取 / 更新 / 删除）
- [ ] 支持项目搜索（按名称 / 标签）
- [ ] 支持最近打开列表（按时间排序）
- [ ] 支持收藏功能
- [ ] 支持健康检查（依赖 / 配置 / 环境）
- [ ] 前端 UI 实现（项目列表 + 卡片 + 搜索）
- [ ] 单元测试覆盖率 > 80%
- [ ] 集成测试通过
- [ ] 文档完整（变更记录 + 实现计划）

---

**设计完成日期**: 2026-08-06  
**下一步**: 实施 M6a - 项目数据结构与数据库 Schema
