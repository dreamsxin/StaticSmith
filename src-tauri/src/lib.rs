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
            commands::list_pages,
            commands::read_content,
            commands::save_content,
            commands::delete_content,
            commands::preview_page,
            commands::create_content,
            commands::start_preview_server,
            commands::stop_preview_server,
            commands::preview_server_url,
            commands::save_asset,
            commands::list_assets,
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
        ])
        .run(tauri::generate_context!())
        .expect("Tauri 应用启动失败");
}
