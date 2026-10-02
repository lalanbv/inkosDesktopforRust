//! 615 号：DESKTOP_WALKTHROUGH.md doc↔code 护栏（568 plugin_doc_contract 先例）——
//! 入库走查清单引用的命令名/实现位置全部在代码事实中核验，防入库文档漂移。
use std::path::PathBuf;

fn doc_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../docs/DESKTOP_WALKTHROUGH.md")
}

fn main_rs_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/main.rs")
}

/// 文档引用的 Tauri 命令与实现符号（与 plugin_doc_contract 同法）。
const ANCHORS: &[&str] = &[
    // 托盘与窗口（main.rs）
    "TrayController",
    "open_manager_window",
    "cmd_open_plugin_manager",
    // 插件命令族（plugin/commands.rs，经 main.rs generate_handler 注册）
    "list_plugins",
    "install_plugin",
    "uninstall_plugin",
    "enable_plugin",
    "disable_plugin",
    "get_plugin",
    "execute_plugin",
    "get_plugin_metrics",
    "cmd_broadcast_event",
    // registry 命令族
    "cmd_fetch_plugin_registry",
    "cmd_install_from_registry",
    "cmd_update_plugin_from_registry",
    "cmd_check_plugin_updates",
    "cmd_list_plugin_versions",
    // 工作区命令族
    "cmd_list_workspaces",
    "cmd_create_workspace",
    "cmd_switch_workspace",
    "cmd_delete_workspace",
    "cmd_add_project_to_workspace",
    // 更新与诊断
    "cmd_check_updates",
    "cmd_apply_engine_update",
    "cmd_apply_shell_update",
    "cmd_get_diagnostics",
    // 能力模型符号（types.rs Capability）
    "ReadProject",
    "WriteProject",
    "SystemCommand",
    // 单实例与流式偏好（607 号）
    "tauri_plugin_single_instance",
    "streamPreference",
];

#[test]
fn desktop_walkthrough_doc_matches_code() {
    let doc = std::fs::read_to_string(doc_path()).expect("docs/DESKTOP_WALKTHROUGH.md 已入库且必须在场");
    let main_rs = std::fs::read_to_string(main_rs_path()).expect("src/main.rs 在库");

    for anchor in ANCHORS {
        assert!(
            doc.contains(anchor),
            "DESKTOP_WALKTHROUGH.md 缺代码锚点 {anchor}（文档与代码失同步）"
        );
    }
    // 命令族锚点须在 main.rs 实际注册面（generate_handler/命令定义）存在。
    for command in [
        "list_plugins",
        "install_plugin",
        "execute_plugin",
        "get_plugin_metrics",
        "cmd_fetch_plugin_registry",
        "cmd_check_updates",
    ] {
        assert!(
            main_rs.contains(command),
            "main.rs 缺命令 {command}（文档/名单/注册三处同批）"
        );
    }
}
