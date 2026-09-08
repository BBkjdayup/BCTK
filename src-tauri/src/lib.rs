mod api;
mod backup;
mod data_move;
mod db;
mod docx;
pub mod licensing;
mod restore;
mod uninstall;
#[cfg(windows)]
mod webview_context_menu;

use api::{
    AppState, CloudService, acknowledge_data_move_result, acknowledge_restore_result, analyze_docx,
    analyze_random_draw, app_initialize, batch_edit_questions, begin_excel_import,
    begin_word_import, cancel_data_move, cancel_question_duplicate_scan, cancel_restore,
    check_print_authorization, check_question_duplicate, check_question_duplicates_batch,
    cloud_login, cloud_logout, cloud_register, cloud_sync_now, configure_template_regions,
    copy_paper, create_backup, create_chapter, create_excel_import_template,
    create_initial_subject, create_subject, create_tag, delete_chapter,
    delete_document_entry_template, delete_document_question_draft, delete_paper, delete_papers,
    delete_question_draft, delete_question_type, delete_subject, delete_tag, delete_template,
    delete_word_import_draft, draw_random_questions, export_license_request, export_paper_docx,
    export_question_bank, get_cloud_account_status, get_cloud_sync_preflight,
    get_document_question_draft, get_last_data_move_result, get_last_restore_result,
    get_license_overview, get_managed_image, get_paper, get_question, get_question_draft,
    get_settings, get_template_configuration_preview, get_template_layout_preview,
    get_word_import_draft, ignore_question_duplicate, import_desktop_license, import_template,
    inspect_backup, inspect_excel_workbook, list_backups, list_cloud_sync_conflicts,
    list_document_entry_templates, list_papers, list_question_drafts, list_question_types,
    list_questions, list_templates, move_questions_to_recycle, open_data_directory,
    open_exported_file, permanently_delete_questions, pick_backup_file, pick_backup_save_path,
    pick_data_directory, pick_desktop_license_file, pick_docx_file, pick_docx_save_path,
    pick_export_directory, pick_license_request_save_path, pick_xlsx_file, pick_xlsx_save_path,
    prepare_data_move, prepare_restore, read_wps_clipboard_images, remove_desktop_license,
    rename_template, resolve_cloud_sync_conflict, restore_questions, retry_database_initialize,
    reveal_exported_file, run_automatic_backup, save_chapter_order, save_document_entry_template,
    save_document_question_draft, save_imported_question, save_imported_question_overwrites,
    save_imported_questions, save_paper, save_question, save_question_draft, save_question_type,
    save_question_type_order, save_questions, save_settings, save_subject_order,
    save_word_import_draft, scan_question_duplicates, schedule_data_move, schedule_restore,
    set_cloud_api_url, set_default_document_entry_template, set_default_template,
    store_managed_images, update_chapter, update_subject, update_tag,
};
use tauri::Manager;

async fn initialize_data_root(
    data_root: std::path::PathBuf,
    security_root: std::path::PathBuf,
    license_root: std::path::PathBuf,
    legacy_security_root: Option<std::path::PathBuf>,
    app_version: String,
) -> AppState {
    match restore::apply_pending_restore(&data_root).await {
        Ok(_) => {
            AppState::initialize_with_license_roots(
                data_root,
                security_root,
                license_root,
                legacy_security_root,
                app_version,
            )
            .await
        }
        Err(error) => AppState::unavailable_with_license_roots(
            data_root,
            security_root,
            license_root,
            legacy_security_root,
            app_version,
            format!("恢复维护未完成，数据库保持关闭：{}", error.message),
        ),
    }
}

/// The NSIS uninstaller runs a temporary copy of the application in this
/// restricted cleanup mode after the normal UI process has been stopped.
pub fn uninstall_cleanup_command_exit_code() -> Option<i32> {
    let mut arguments = std::env::args_os().skip(1);
    let command = arguments.next()?;
    if command != "--tktiku-uninstall-delete-managed-data-v1" {
        return None;
    }
    let Some(record_path) = arguments.next() else {
        return Some(2);
    };
    if arguments.next().is_some() {
        return Some(2);
    }
    Some(
        match uninstall::cleanup_managed_data_from_record(std::path::Path::new(&record_path)) {
            Ok(()) => 0,
            Err(_) => 1,
        },
    )
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            #[cfg(windows)]
            webview_context_menu::install(app)?;

            let resolver = app.path();
            let base = resolver
                .document_dir()
                .or_else(|_| resolver.app_data_dir())?;
            let default_data_root = base.join("教师题库");
            let app_local_root = resolver.app_local_data_dir()?;
            let bootstrap_dir = app_local_root.join("bootstrap");
            let license_root = app_local_root.join("licensing");
            let security_root = app_local_root
                .parent()
                .ok_or_else(|| std::io::Error::other("无法确定持久授权状态父目录"))?
                .join("com.zhitiku.desktop.entitlement");
            let legacy_security_root = Some(license_root.clone());
            let cloud_root = app_local_root.join("cloud");
            let startup = data_move::resolve_startup_data_root(&bootstrap_dir, default_data_root)
                .map_err(|error| std::io::Error::other(error.message))?;
            let app_version = app.package_info().version.to_string();
            let state = tauri::async_runtime::block_on(async {
                let pending_validation_error = data_move::validate_pending_target(&startup)
                    .await
                    .err()
                    .map(|error| error.message);
                let mut selected_state = if let Some(error) = pending_validation_error {
                    AppState::unavailable_with_license_roots(
                        startup.selected_data_root.clone(),
                        security_root.clone(),
                        license_root.clone(),
                        legacy_security_root.clone(),
                        app_version.clone(),
                        error,
                    )
                } else {
                    initialize_data_root(
                        startup.selected_data_root.clone(),
                        security_root.clone(),
                        license_root.clone(),
                        legacy_security_root.clone(),
                        app_version.clone(),
                    )
                    .await
                };
                let selected_error = selected_state.initialization_error().await;
                match data_move::finalize_pending_startup(&bootstrap_dir, &startup, selected_error)
                {
                    Ok(Some(result)) if result.outcome == "rolled_back" => {
                        let fallback = std::path::PathBuf::from(result.active_data_root);
                        selected_state = match data_move::validate_existing_data_root(&fallback) {
                            Ok(()) => {
                                initialize_data_root(
                                    fallback,
                                    security_root.clone(),
                                    license_root.clone(),
                                    legacy_security_root.clone(),
                                    app_version.clone(),
                                )
                                .await
                            }
                            Err(error) => AppState::unavailable_with_license_roots(
                                fallback,
                                security_root.clone(),
                                license_root.clone(),
                                legacy_security_root.clone(),
                                app_version.clone(),
                                format!(
                                    "迁移目标未启用，但原数据目录也无法安全打开：{}",
                                    error.message
                                ),
                            ),
                        };
                        selected_state
                    }
                    Ok(_) => selected_state,
                    Err(error) => AppState::unavailable_with_license_roots(
                        startup.selected_data_root,
                        security_root,
                        license_root,
                        legacy_security_root,
                        app_version.clone(),
                        format!("数据目录切换状态无法安全提交：{}", error.message),
                    ),
                }
            });
            if let Err(error) =
                uninstall::register_managed_data_root(&app_local_root, state.data_root())
            {
                eprintln!("无法登记安全卸载数据目录：{error}");
            }
            app.manage(state);
            let cloud = CloudService::initialize(cloud_root.clone(), &app_version)
                .unwrap_or_else(|_| CloudService::unavailable(cloud_root, &app_version));
            app.manage(cloud);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_initialize,
            get_license_overview,
            get_cloud_account_status,
            get_cloud_sync_preflight,
            set_cloud_api_url,
            cloud_register,
            cloud_login,
            cloud_logout,
            cloud_sync_now,
            list_cloud_sync_conflicts,
            resolve_cloud_sync_conflict,
            pick_license_request_save_path,
            export_license_request,
            pick_desktop_license_file,
            import_desktop_license,
            remove_desktop_license,
            check_print_authorization,
            retry_database_initialize,
            list_question_types,
            save_question_type,
            delete_question_type,
            save_question_type_order,
            list_questions,
            analyze_random_draw,
            draw_random_questions,
            get_question,
            check_question_duplicate,
            check_question_duplicates_batch,
            cancel_question_duplicate_scan,
            scan_question_duplicates,
            ignore_question_duplicate,
            save_question,
            save_imported_question,
            save_imported_questions,
            save_imported_question_overwrites,
            save_questions,
            batch_edit_questions,
            get_question_draft,
            list_question_drafts,
            save_question_draft,
            delete_question_draft,
            get_document_question_draft,
            save_document_question_draft,
            delete_document_question_draft,
            list_document_entry_templates,
            save_document_entry_template,
            set_default_document_entry_template,
            delete_document_entry_template,
            get_word_import_draft,
            save_word_import_draft,
            delete_word_import_draft,
            move_questions_to_recycle,
            restore_questions,
            permanently_delete_questions,
            list_papers,
            get_paper,
            save_paper,
            copy_paper,
            delete_paper,
            delete_papers,
            export_paper_docx,
            export_question_bank,
            get_settings,
            save_settings,
            create_initial_subject,
            create_subject,
            update_subject,
            delete_subject,
            create_chapter,
            update_chapter,
            delete_chapter,
            create_tag,
            update_tag,
            delete_tag,
            save_subject_order,
            save_chapter_order,
            list_templates,
            import_template,
            get_template_configuration_preview,
            get_template_layout_preview,
            configure_template_regions,
            rename_template,
            delete_template,
            set_default_template,
            list_backups,
            create_backup,
            run_automatic_backup,
            inspect_backup,
            prepare_restore,
            cancel_restore,
            schedule_restore,
            get_last_restore_result,
            acknowledge_restore_result,
            pick_data_directory,
            pick_export_directory,
            open_data_directory,
            open_exported_file,
            reveal_exported_file,
            prepare_data_move,
            cancel_data_move,
            schedule_data_move,
            get_last_data_move_result,
            acknowledge_data_move_result,
            pick_backup_save_path,
            pick_backup_file,
            pick_docx_file,
            pick_xlsx_file,
            pick_docx_save_path,
            pick_xlsx_save_path,
            analyze_docx,
            begin_word_import,
            inspect_excel_workbook,
            begin_excel_import,
            create_excel_import_template,
            get_managed_image,
            store_managed_images,
            read_wps_clipboard_images,
        ])
        .run(tauri::generate_context!())
        .expect("TK试题题库启动失败");
}
