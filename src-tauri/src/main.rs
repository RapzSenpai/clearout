#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clearout_lib::commands;

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::inventory::get_installed_apps,
            commands::uninstall::run_uninstaller,
            commands::scan::scan_leftovers,
            commands::delete::delete_items,
            commands::locks::check_locks,
            commands::ai::ask_ai,
            commands::verify::verify_scan,
            commands::export::export_report_json,
            commands::export::export_report_txt,
            commands::open_location::open_location,
            commands::history::list_reports,
            commands::history::load_report,
            commands::history::delete_report,
            commands::history::clear_reports,
            commands::trash::list_trash,
            commands::trash::restore_trash,
            commands::trash::clear_trash,
            commands::scheduler::get_scheduler_status,
            commands::scheduler::set_scheduler,
            commands::scheduler::is_admin,
            commands::registry_backup::list_registry_backups,
            commands::registry_backup::restore_registry_backup,
            commands::registry_backup::delete_registry_backup,
            commands::registry_backup::clear_registry_backups,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
