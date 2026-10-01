//! R46/568 号：插件文档契约护栏——`docs/plugin-system.md` 与代码事实的漂移拦截。
//!
//! 历史：该文档曾漂移至「Svelte 前端 + 裸 extern "C" 开发模型」而实现已是
//! 「React studio 无插件面 + 桌壳 settings.html 管理窗 + WIT Component Model
//! 双路径」（v7 对标诊断的实证，568 号清偿）。本护栏把文档锁到三源事实：
//! ① wit 契约（doc 的接口函数名必须 ⊆ wit/inkos.wit 实际函数名）
//! ② Tauri 命令名单（doc ⊇ 名单 ∧ main.rs generate_handler 源 ⊇ 名单——
//!    命令改名/增删须三处同批，名单在测试内 const 数组单点维护）
//! ③ 关键事实词（双路径/wasmtime/JSON-RPC/HostContext/能力白名单）。
//!
//! `docs/*` 为 gitignore 的本地文档（168 号既定策略）：文档缺席（新 clone）
//! 时本护栏跳过；本地门禁（cargo:testgate）恒有文档在场即恒受锁。
//! 若未来把文档提升入库，去掉 skip 分支即可全环境生效。
use std::path::PathBuf;

fn doc_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../docs/plugin-system.md")
}

fn wit_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("wit/inkos.wit")
}

fn main_rs_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/main.rs")
}

/// Tauri 命令名单（来源：main.rs `generate_handler!` 的插件面 +
/// commands/registry.rs；命令改名/增删必须同步此表与文档）。
const TAURI_COMMANDS: &[&str] = &[
    "list_plugins",
    "install_plugin",
    "uninstall_plugin",
    "enable_plugin",
    "disable_plugin",
    "get_plugin",
    "execute_plugin",
    "get_plugin_metrics",
    "cmd_broadcast_event",
    "cmd_open_plugin_manager",
    "cmd_fetch_plugin_registry",
    "cmd_install_from_registry",
    "cmd_update_plugin_from_registry",
    "cmd_check_plugin_updates",
    "cmd_list_plugin_versions",
];

#[test]
fn plugin_doc_matches_code_facts() {
    let Ok(doc) = std::fs::read_to_string(doc_path()) else {
        // docs/* 为 gitignore 本地文档：缺席环境跳过（见模块 doc）。
        eprintln!("plugin-system.md 不在场（本地文档策略），护栏跳过");
        return;
    };

    // ① wit 契约交叉核验：doc 必须覆盖 wit host interface 的全部函数名。
    let wit = std::fs::read_to_string(wit_path()).expect("wit/inkos.wit 在库");
    for func in ["read-file", "write-file", "list-dir", "http-get", "exec-command", "log"] {
        assert!(
            wit.contains(&format!("{func}: func")),
            "wit/inkos.wit 缺少 host 函数 {func}（护栏名单与 wit 契约失同步）"
        );
        assert!(doc.contains(func), "plugin-system.md 缺 WIT host 函数 {func}");
    }
    for func in ["init", "invoke", "on-event"] {
        assert!(doc.contains(func), "plugin-system.md 缺 WIT plugin 函数 {func}");
    }

    // ② Tauri 命令名单双源锁：doc ⊇ 名单 ∧ main.rs ⊇ 名单。
    let main_rs = std::fs::read_to_string(main_rs_path()).expect("src/main.rs 在库");
    for command in TAURI_COMMANDS {
        assert!(doc.contains(command), "plugin-system.md 缺 Tauri 命令 {command}");
        assert!(
            main_rs.contains(command),
            "main.rs 缺命令 {command}（名单/文档/注册三处同批）"
        );
    }

    // ③ 关键事实词：双路径架构与漂移史锚点。
    for fact in [
        "wasmtime",        // WASM 路径引擎
        "JSON-RPC",        // 进程隔离路径协议
        "HostContext",     // 权限校验单点
        "allowed_commands", // system_command 白名单（fail-closed）——旧 doc 写裸类型即漂移
        "wit/inkos.wit",   // 契约文件锚点
        "app_data",        // 日志路径事实（旧 doc 误写 ~/.inkos/logs）
    ] {
        assert!(doc.contains(fact), "plugin-system.md 缺关键事实词 {fact}");
    }
    assert!(
        !doc.contains("Svelte"),
        "plugin-system.md 残留 Svelte 漂移（实际 UI=桌壳 settings.html；React studio 无插件面）"
    );
}
