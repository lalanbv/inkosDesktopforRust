use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::Manager;

/// 诊断信息结构（cmd_get_diagnostics 返回值）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticInfo {
    pub version: String,
    pub platform: String,
    pub arch: String,
    pub app_data_dir: String,
    pub engine_dir: String,
    pub node_cache_dir: String,
    pub projects_file: String,
    pub engine_manifest: Option<String>,
    pub recent_crashes: Vec<String>,
}

/// Tauri 命令：获取诊断信息（版本/平台/路径/manifest/最近崩溃）。
///
/// # 返回
/// - `DiagnosticInfo`：JSON 序列化的诊断快照
#[tauri::command]
pub async fn cmd_get_diagnostics(app: tauri::AppHandle) -> Result<DiagnosticInfo, String> {
    use std::fs;

    // 1. 版本/平台信息
    let version = app.package_info().version.to_string();
    let platform = std::env::consts::OS.to_string();
    let arch = std::env::consts::ARCH.to_string();

    // 2. 路径信息（从 app.path() 获取）
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("获取 app_data_dir 失败: {}", e))?
        .to_string_lossy()
        .to_string();

    let engine_dir = app
        .path()
        .app_data_dir()
        .map(|p| p.join("engine"))
        .map_err(|e| format!("获取 engine_dir 失败: {}", e))?
        .to_string_lossy()
        .to_string();

    let node_cache_dir = app
        .path()
        .app_cache_dir()
        .map(|p| p.join("node"))
        .map_err(|e| format!("获取 node_cache_dir 失败: {}", e))?
        .to_string_lossy()
        .to_string();

    let projects_file = app
        .path()
        .app_data_dir()
        .map(|p| p.join("projects.json"))
        .map_err(|e| format!("获取 projects_file 失败: {}", e))?
        .to_string_lossy()
        .to_string();

    // 3. engine manifest（如果存在）
    let manifest_path = app
        .path()
        .app_data_dir()
        .ok()
        .map(|p| p.join("engine").join("manifest.json"));

    let engine_manifest = manifest_path.and_then(|p| fs::read_to_string(&p).ok());

    // 4. 最近崩溃文件（最多 5 个）
    let crash_dir: Option<PathBuf> = app
        .path()
        .app_data_dir()
        .ok()
        .map(|p| p.join("crashes"));

    let mut recent_crashes = Vec::new();
    if let Some(crash_dir) = crash_dir {
        if let Ok(entries) = fs::read_dir(&crash_dir) {
        let mut files: Vec<_> = entries
            .filter_map(Result::ok)
            .filter(|e| {
                e.path()
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("crash-") && n.ends_with(".json"))
                    .unwrap_or(false)
            })
            .collect();

        // 按修改时间排序（最新在前）
        files.sort_by_key(|e| {
            e.metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        });
        files.reverse();

        recent_crashes = files
            .iter()
            .take(5)
            .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
            .collect();
        }
    }

    Ok(DiagnosticInfo {
        version,
        platform,
        arch,
        app_data_dir,
        engine_dir,
        node_cache_dir,
        projects_file,
        engine_manifest,
        recent_crashes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diagnostic_info_serialization() {
        let info = DiagnosticInfo {
            version: "0.3.0".to_string(),
            platform: "macos".to_string(),
            arch: "aarch64".to_string(),
            app_data_dir: "/tmp/app".to_string(),
            engine_dir: "/tmp/engine".to_string(),
            node_cache_dir: "/tmp/node".to_string(),
            projects_file: "/tmp/projects.json".to_string(),
            engine_manifest: Some("v1.0.0".to_string()),
            recent_crashes: vec!["crash-1234.json".to_string()],
        };

        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"version\":\"0.3.0\""));
        assert!(json.contains("\"platform\":\"macos\""));

        let deserialized: DiagnosticInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.version, "0.3.0");
        assert_eq!(deserialized.recent_crashes.len(), 1);
    }

    // 注意：cmd_get_diagnostics 需要 Tauri AppHandle，无法单元测试，
    // 依赖集成测试或手动冒烟验证
}
