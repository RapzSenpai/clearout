#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clearout_lib::commands;

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::inventory::get_installed_apps,
            commands::uninstall::run_uninstaller,
            commands::uninstall::preview_uninstall,
            commands::scan::scan_leftovers,
            commands::delete::delete_items,
            commands::delete::registry_impact,
            commands::secure_storage::save_api_key,
            commands::secure_storage::api_key_status,
            commands::secure_storage::delete_api_key,
            commands::read_app_logs,
            commands::ai::ask_ai,
            commands::ai::test_ai_connection,
            commands::export::export_report_json,
            commands::export::export_report_txt,
            commands::open_location::open_location,
            commands::history::list_reports,
            commands::history::load_report,
            commands::history::delete_report,
            commands::history::clear_reports,
            commands::scheduler::get_scheduler_status,
            commands::scheduler::set_scheduler,
            commands::scheduler::is_admin,
            commands::registry_backup::list_registry_backups,
            commands::registry_backup::restore_registry_backup,
            commands::registry_backup::delete_registry_backup,
            commands::registry_backup::clear_registry_backups,
        ])
        .run(tauri::generate_context!())
        .unwrap_or_else(|e| {
            eprintln!("error while running tauri application: {}", e);
            std::process::exit(1);
        });
}
