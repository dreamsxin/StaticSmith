//! Tauri 应用装配：注册状态、插件与 IPC 命令。

mod commands;
mod error;
mod recent;
mod state;

use state::AppState;

/// 桌面端入口。`main.rs` 与移动端入口都调用它。
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "staticsmith_app_lib=info,staticsmith_core=info".into()),
        )
        .init();

    install_panic_hook();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::init_project,
            commands::is_project,
            commands::open_project,
            commands::recent_projects,
            commands::forget_project,
            commands::close_project,
            commands::project_summary,
            commands::read_config,
            commands::save_config,
            commands::read_config_source,
            commands::save_config_source,
            commands::list_pages,
            commands::search_content,
            commands::read_content,
            commands::save_content,
            commands::delete_content,
            commands::preview_page,
            commands::read_front_matter,
            commands::apply_front_matter,
            commands::create_content,
            commands::start_preview_server,
            commands::stop_preview_server,
            commands::preview_server_url,
            commands::start_mcp_server,
            commands::stop_mcp_server,
            commands::mcp_server_status,
            commands::save_asset,
            commands::list_assets,
            commands::list_outputs,
            commands::audit_seo,
            commands::audit_media,
            commands::audit_links,
            commands::list_sections,
            commands::create_section,
            commands::rename_section,
            commands::remove_section,
            commands::save_section_meta,
            commands::batch_edit_tags,
            commands::batch_set_draft,
            commands::batch_move,
            commands::batch_delete,
            commands::batch_preview,
            commands::preview_replace,
            commands::apply_replace,
            commands::scan_import,
            commands::import_content,
            commands::export_theme,
            commands::scan_theme,
            commands::import_theme,
            commands::remove_media,
            commands::list_templates,
            commands::template_tree,
            commands::read_template,
            commands::save_template,
            commands::build_plan,
            commands::run_build,
            commands::output_dir,
            commands::deploy_site,
            commands::check_deploy,
            commands::save_secret,
            commands::has_secret,
            commands::delete_secret,
            commands::deploy_account,
        ])
        .run(tauri::generate_context!())
        .expect("Tauri 应用启动失败");
}

/// 把「静默闪退」变成「日志里能查到的闪退」。
///
/// `Cargo.toml` 的 release profile 设了 `panic = "abort"`，所以 panic 不能被
/// `catch_unwind` 兜住——渲染线程池、预览 HTTP 线程、文件监听线程、内嵌 MCP 线程
/// 里任何一处 panic 都会让整个窗口当场消失。既然拦不住，至少要留下现场：
/// 钩子在 abort 之前跑，把线程名、位置与消息写进 tracing。
///
/// 这不能替代「别 panic」。它只是让事后能查出是谁炸的——真正要杜绝丢稿，
/// 得去掉 `panic = "abort"` 并在线程边界上兜住，那是另一笔账（见 docs/development.md）。
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("<未命名>");
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "<位置未知>".to_string());
        tracing::error!("线程 {name} 在 {location} panic：{}", panic_message(info));
        // 仍然交给默认钩子：它负责往 stderr 打印，行为不变。
        previous(info);
    }));
}

fn panic_message(info: &std::panic::PanicHookInfo<'_>) -> String {
    let payload = info.payload();
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<非字符串 payload>".to_string()
    }
}
