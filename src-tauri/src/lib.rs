mod api;
mod backup;
mod data_move;
mod db;
mod docx;
mod mini_cloud;
mod restore;
mod startup;
mod uninstall;
#[cfg(windows)]
mod webview_context_menu;

use api::{
    acknowledge_data_move_result, acknowledge_restore_result, analyze_random_draw, app_initialize,
    batch_edit_questions, begin_excel_import, begin_word_import, cancel_data_move,
    cancel_question_duplicate_scan, cancel_restore, check_question_duplicate,
    check_question_duplicates_batch, clear_paper_recovery, configure_template_regions, copy_paper,
    create_backup, create_chapter, create_excel_import_template, create_initial_subject,
    create_subject, create_tag, delete_chapter, delete_document_entry_template,
    delete_document_question_draft, delete_paper, delete_papers, delete_question_draft,
    delete_question_filter, delete_question_type, delete_subject, delete_tag, delete_template,
    delete_word_import_draft, draw_random_questions, export_paper_docx, export_question_bank,
    get_document_question_draft, get_last_data_move_result, get_last_restore_result,
    get_managed_image, get_paper, get_paper_recovery, get_question, get_question_draft,
    get_question_history, get_question_stem_summaries, get_settings,
    get_template_configuration_preview, get_template_layout_preview, get_word_import_draft,
    ignore_question_duplicate, import_template, inspect_excel_workbook, list_backups,
    list_document_entry_templates, list_papers, list_question_batches, list_question_history,
    list_questions, list_saved_question_filters, list_templates, move_questions_to_recycle,
    open_data_directory, open_exported_file, permanently_delete_questions, pick_backup_file,
    pick_backup_save_path, pick_data_directory, pick_docx_file, pick_docx_save_path,
    pick_export_directory, pick_xlsx_file, pick_xlsx_save_path, prepare_data_move, prepare_restore,
    read_wps_clipboard_images, rename_template, restore_question_history, restore_questions,
    retry_database_initialize, reveal_exported_file, run_automatic_backup, save_chapter_order,
    save_document_entry_template, save_document_question_draft, save_imported_question_overwrites,
    save_imported_questions, save_paper, save_paper_recovery, save_question, save_question_draft,
    save_question_filter, save_question_type, save_question_type_order, save_settings,
    save_subject_order, save_word_import_draft, scan_question_duplicates, schedule_data_move,
    schedule_restore, set_default_document_entry_template, set_default_template,
    store_managed_images, undo_question_batch, update_chapter, update_subject, update_tag,
};
use tauri::Manager;

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
        .manage(mini_cloud::MiniCloud::default())
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
            let app_version = app.package_info().version.to_string();
            app.manage(startup::StartupState::default());
            let handle = app.handle().clone();
            let progress = app.state::<startup::StartupState>().progress.clone();
            tauri::async_runtime::spawn(async move {
                // Filesystem work, SQLite checks and migration parsing must not
                // occupy the window thread or an async executor worker.
                let result = tauri::async_runtime::spawn_blocking(move || {
                    tauri::async_runtime::block_on(startup::initialize(
                        default_data_root,
                        app_local_root,
                        app_version,
                        &progress,
                    ))
                })
                .await;
                let startup = handle.state::<startup::StartupState>();
                match result {
                    Ok(Ok(state)) => {
                        let error = state.initialization_error().await;
                        handle.manage(state);
                        startup.registered(error);
                    }
                    Ok(Err(message)) => startup.failed(message),
                    Err(error) => {
                        startup.failed(format!("启动任务意外结束，请重新打开软件：{error}"))
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            mini_cloud::mini_cloud_login,
            mini_cloud::mini_cloud_logout,
            mini_cloud::mini_cloud_request,
            app_initialize,
            api::get_startup_progress,
            retry_database_initialize,
            save_question_type,
            delete_question_type,
            save_question_type_order,
            list_questions,
            analyze_random_draw,
            draw_random_questions,
            get_question,
            list_question_history,
            get_question_history,
            restore_question_history,
            list_question_batches,
            undo_question_batch,
            list_saved_question_filters,
            save_question_filter,
            delete_question_filter,
            get_question_stem_summaries,
            check_question_duplicate,
            check_question_duplicates_batch,
            cancel_question_duplicate_scan,
            scan_question_duplicates,
            api::check_formula_compatibility,
            api::list_question_summaries,
            api::get_questions,
            api::background_task_progress,
            api::cancel_background_task,
            ignore_question_duplicate,
            save_question,
            save_imported_questions,
            save_imported_question_overwrites,
            batch_edit_questions,
            get_question_draft,
            save_question_draft,
            get_paper_recovery,
            save_paper_recovery,
            clear_paper_recovery,
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
            api::merge_question_duplicates,
            api::latest_question_duplicate_merge,
            api::undo_question_duplicate_merge,
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
