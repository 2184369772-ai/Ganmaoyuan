mod analysis;
mod commands;
pub mod context_bridge;
mod core_projection;
mod deepseek;
mod inbox_routing;
mod launch_inbox;
mod models;
mod project_service;
mod release_service;
pub mod shell_integration;
mod storage;
mod weekly_review;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // Resolve and migrate the global data directory before the first UI request.
            let global_data_dir = storage::global_data_dir(app.handle())
                .map_err(|error| std::io::Error::other(format!("初始化数据目录失败：{error}")))?;
            let log_dir = global_data_dir.join("logs");
            std::fs::create_dir_all(&log_dir).map_err(|error| {
                std::io::Error::other(format!("初始化应用日志目录失败：{error}"))
            })?;
            app.handle().plugin(
                tauri_plugin_log::Builder::default()
                    .clear_targets()
                    .target(tauri_plugin_log::Target::new(
                        tauri_plugin_log::TargetKind::Folder {
                            path: log_dir,
                            file_name: Some("ganmaoyuan".into()),
                        },
                    ))
                    .level(if cfg!(debug_assertions) {
                        log::LevelFilter::Info
                    } else {
                        log::LevelFilter::Warn
                    })
                    .build(),
            )?;
            project_service::recover_interrupted_inbox_routes(app.handle()).map_err(|error| {
                log::error!("interrupted Inbox route recovery failed");
                std::io::Error::other(format!("恢复中断的 Inbox 归位事务失败：{error}"))
            })?;
            shell_integration::maybe_register_sendto_shortcut(&std::env::current_exe().map_err(
                |error| std::io::Error::other(format!("读取感冒院程序路径失败：{error}")),
            )?)
            .map_err(|error| std::io::Error::other(format!("初始化 SendTo 入口失败：{error}")))?;
            let launch_request = launch_inbox::collect_launch_request(std::env::args_os().skip(1));
            let is_primary = launch_inbox::acquire_primary_instance()
                .map_err(|error| std::io::Error::other(format!("初始化单实例状态失败：{error}")))?;
            if !launch_request.file_paths.is_empty() {
                launch_inbox::enqueue_launch_file_paths(app.handle(), &launch_request).map_err(
                    |error| std::io::Error::other(format!("写入启动拖入队列失败：{error}")),
                )?;
            }
            if !is_primary {
                launch_inbox::focus_existing_window("Ganmaoyuan");
                app.handle().exit(0);
                return Ok(());
            }
            log::info!("Ganmaoyuan startup completed");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_projects,
            commands::list_file_projections,
            commands::get_workspace_config,
            commands::initialize_workspace_root,
            commands::verify_workspace_root,
            commands::list_workspace_scan_batches,
            commands::scan_workspace_directory,
            commands::scan_desktop_directory,
            commands::scan_downloads_directory,
            commands::generate_cleanup_plan,
            commands::list_cleanup_plans,
            commands::get_cleanup_plan,
            commands::review_cleanup_plan_items,
            commands::bulk_review_cleanup_plan,
            commands::modify_cleanup_plan_item,
            commands::reset_cleanup_plan_items,
            commands::execute_cleanup_plan,
            commands::list_cleanup_execution_batches,
            commands::list_execution_records,
            commands::undo_cleanup_execution_batch,
            commands::load_project,
            commands::create_project,
            commands::import_files,
            commands::send_project_message,
            commands::stop_project_message,
            commands::save_project_draft,
            commands::finish_project_work,
            commands::generate_codex_prompt,
            commands::list_codex_tasks,
            commands::rebind_codex_task_prompt,
            commands::mark_codex_task_handed_off,
            commands::start_codex_task_run,
            commands::cancel_codex_task_run,
            commands::accept_codex_task,
            commands::reject_codex_task,
            commands::import_codex_report_text,
            commands::import_codex_report_file,
            commands::scan_codex_result_bridge,
            commands::apply_codex_report,
            commands::get_work_ledger,
            commands::get_project_context_packet,
            commands::refresh_git_snapshot,
            commands::record_user_decision_event,
            commands::capture_project_fact,
            commands::search_projects,
            commands::register_generated_file,
            commands::load_deepseek_settings,
            commands::save_deepseek_api_key,
            commands::delete_deepseek_api_key,
            commands::test_deepseek_connection,
            commands::list_deepseek_models,
            commands::grant_project_deepseek_authorization,
            commands::refresh_project_understanding,
            commands::scan_project_workspace,
            commands::open_file,
            commands::open_folder,
            commands::list_memos,
            commands::save_memos,
            commands::list_inbox_entries,
            commands::list_material_inbox,
            commands::receive_inbox_files,
            commands::ingest_material_inbox_files,
            commands::confirm_inbox_entry,
            commands::confirm_material_inbox_item,
            commands::refine_inbox_entry_with_ai,
            commands::update_inbox_route_decision,
            commands::ignore_inbox_entry,
            commands::retry_inbox_entry,
            commands::reanalyze_inbox_entry,
            commands::undo_inbox_route,
            commands::route_inbox_item_to_global,
            commands::undo_global_file,
            commands::list_global_files,
            commands::load_inbox_routing_settings,
            commands::save_inbox_routing_settings,
            commands::create_project_from_inbox,
            commands::create_project_from_material_inbox,
            commands::consume_launch_inbox_entries,
            commands::create_local_backup,
            commands::restore_local_backup,
            commands::migrate_project_root,
            commands::export_project_safe,
            commands::generate_privacy_artifacts,
            commands::load_weekly_review_dashboard,
            commands::load_weekly_review_settings,
            commands::save_weekly_review_settings,
            commands::generate_weekly_reviews,
            commands::update_weekly_report_markdown,
            commands::confirm_weekly_report,
            commands::export_weekly_report,
            commands::get_impact_analyses,
            commands::get_action_candidates,
            commands::get_state_proposals,
            commands::confirm_action_candidate,
            commands::ignore_action_candidate,
            commands::update_action_candidate,
            commands::apply_state_proposal,
            commands::undo_state_proposal,
            commands::get_daily_continue,
            commands::regenerate_daily_continue,
            commands::get_today_workspace,
            commands::update_project_attention,
            commands::update_data_health_status,
            commands::update_pending_review_status,
            commands::mark_today_done,
            commands::set_state_auto_apply,
        ])
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
                window.app_handle().exit(0);
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
