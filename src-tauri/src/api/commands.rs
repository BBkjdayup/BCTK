use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::Duration,
};

use serde_json::json;
use sqlx::FromRow;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;

use crate::data_move as data_move_workflow;
use crate::docx::{
    AnchorKind, Diagnostic, DiagnosticSeverity, DocxLimits, PackageKind, TemplateAnchor,
    TemplateRegionConfiguration, TemplateRegionPlacement, TemplateStyleProfile,
    TemplateStyleSelection, configure_template_package, preview_template_configuration,
    preview_template_layout,
};
use crate::licensing::{LicenseFileResult, LicenseOverview};
use crate::restore as restore_workflow;

use super::{
    AppState, backups, document_entry_templates, docx, drafts, excel_import,
    models::{
        AnalyzeDocxRequestApi, AppSettingsApi, AutomaticBackupRunApi, BackupCreateResultApi,
        BackupInspectionApi, BackupRecordApi, BeginExcelImportResultApi, BeginWordImportResultApi,
        BootstrapDataApi, ChapterApi, CommandError, CommandResult,
        ConfigureTemplateRegionsRequestApi, DataMovePlanApi, DataMoveResultApi,
        DataMoveScheduledApi, DocumentEntryTemplateApi, DocumentQuestionDraftPayloadApi,
        DocumentQuestionDraftRecordApi, DocxAnalysisApi, DocxDiagnosticApi, ExcelImportRequestApi,
        ExcelImportTemplateResultApi, ExcelWorkbookInspectionApi, ExportPaperDocxRequestApi,
        IgnoreQuestionDuplicateRequestApi, ImportedQuestionOverwriteRequestApi,
        ImportedQuestionOverwriteResultApi, ManagedImagePayloadApi, PageResultApi, PaperApi,
        PaperDeleteRequestApi, PaperDocxExportResultApi, PaperFiltersApi, PaperSummaryApi,
        QuestionApi, QuestionBankExportResultApi, QuestionBatchEditRequestApi,
        QuestionBatchEditResultApi, QuestionDraftApi, QuestionDraftRecordApi,
        QuestionDuplicateBatchRequestApi, QuestionDuplicateBatchResultApi,
        QuestionDuplicateCheckApi, QuestionDuplicateScanRequestApi, QuestionDuplicateScanResultApi,
        QuestionFiltersApi, QuestionTypeDefinitionApi, RandomDrawAnalysisApi, RandomDrawRequestApi,
        RandomDrawScopeApi, ReadWpsClipboardImagesRequestApi, RestorePlanApi, RestoreResultApi,
        RestoreScheduledApi, SaveDocumentEntryTemplateRequestApi, SaveQuestionTypeRequestApi,
        ScheduleDataMoveRequestApi, ScheduleRestoreRequestApi, StoreManagedImagesRequestApi,
        SubjectApi, TagApi, TaxonomyOrderItemApi, TemplateAnchorApi, TemplateByteSpanApi,
        TemplateConfigurationParagraphApi, TemplateConfigurationPreviewApi,
        TemplateImportResultApi, TemplateLayoutPreviewApi, TemplateRegionPlacementApi,
        TemplateStyleSamplesApi, WordImportDraftPayloadApi, WordImportDraftRecordApi,
        WordTemplateApi, WpsClipboardImagePayloadApi,
    },
    paper_export, papers, question_bank_export, question_types, questions, random_draw, resources,
    settings, taxonomy, templates, templates_core,
};

#[derive(Debug, FromRow)]
struct SubjectRow {
    id: String,
    name: String,
    sort_order: i64,
    last_accessed_at_ms: Option<i64>,
    question_count: i64,
}

#[derive(Debug, FromRow)]
struct ChapterRow {
    id: String,
    subject_id: String,
    name: String,
    sort_order: i64,
    last_accessed_at_ms: Option<i64>,
    question_count: i64,
}

#[derive(Debug, FromRow)]
struct TagRow {
    id: String,
    name: String,
    created_at_ms: i64,
    updated_at_ms: i64,
    question_count: i64,
}

#[tauri::command]
pub async fn app_initialize(state: State<'_, AppState>) -> CommandResult<BootstrapDataApi> {
    let database = match state.database().await {
        Ok(database) => database,
        Err(_) => {
            return Ok(BootstrapDataApi {
                initialized: false,
                data_root: state.data_root().to_string_lossy().into_owned(),
                subjects: Vec::new(),
                tags: Vec::new(),
                question_types: Vec::new(),
                pending_draft_count: 0,
                database_healthy: false,
                database_error: state.initialization_error().await,
                app_version: state.app_version().to_owned(),
                license: state.license().overview(),
            });
        }
    };
    let pool = database.pool();
    let subject_rows = sqlx::query_as::<_, SubjectRow>(
        "SELECT s.id, s.name, s.sort_order, s.last_accessed_at_ms, \
         (SELECT COUNT(*) FROM questions q WHERE q.subject_id = s.id AND q.deleted_at_ms IS NULL) AS question_count \
         FROM subjects s ORDER BY s.sort_order, s.id",
    )
    .fetch_all(pool).await.map_err(CommandError::database)?;
    let chapter_rows = sqlx::query_as::<_, ChapterRow>(
        "SELECT c.id, c.subject_id, c.name, c.sort_order, c.last_accessed_at_ms, \
         (SELECT COUNT(*) FROM questions q WHERE q.chapter_id = c.id AND q.deleted_at_ms IS NULL) AS question_count \
         FROM chapters c ORDER BY c.subject_id, c.sort_order, c.id",
    )
    .fetch_all(pool).await.map_err(CommandError::database)?;
    let tag_rows = sqlx::query_as::<_, TagRow>(
        "SELECT t.id, t.name, t.created_at_ms, t.updated_at_ms, \
         (SELECT COUNT(*) FROM question_tags qt JOIN questions q ON q.id = qt.question_id \
          WHERE qt.tag_id = t.id AND q.deleted_at_ms IS NULL) AS question_count \
         FROM tags t ORDER BY t.name",
    )
    .fetch_all(pool)
    .await
    .map_err(CommandError::database)?;
    let pending_draft_count: i64 = sqlx::query_scalar(
        "SELECT (SELECT COUNT(*) FROM drafts) + \
                (SELECT COUNT(*) FROM document_question_drafts)",
    )
    .fetch_one(pool)
    .await
    .map_err(CommandError::database)?;

    let subjects = subject_rows
        .into_iter()
        .map(|subject| {
            let chapters = chapter_rows
                .iter()
                .filter(|chapter| chapter.subject_id == subject.id)
                .map(|chapter| ChapterApi {
                    id: chapter.id.clone(),
                    subject_id: chapter.subject_id.clone(),
                    name: chapter.name.clone(),
                    sort_order: chapter.sort_order,
                    question_count: chapter.question_count,
                    last_accessed_at: chapter.last_accessed_at_ms,
                })
                .collect();
            SubjectApi {
                id: subject.id,
                name: subject.name,
                sort_order: subject.sort_order,
                question_count: subject.question_count,
                last_accessed_at: subject.last_accessed_at_ms,
                chapters,
            }
        })
        .collect::<Vec<_>>();
    let tags = tag_rows
        .into_iter()
        .map(|tag| TagApi {
            id: tag.id,
            name: tag.name,
            question_count: tag.question_count,
            created_at: tag.created_at_ms,
            updated_at: tag.updated_at_ms,
        })
        .collect::<Vec<_>>();
    let question_types = question_types::list(pool).await?;

    Ok(BootstrapDataApi {
        initialized: !subjects.is_empty(),
        data_root: database.paths().data_root().to_string_lossy().into_owned(),
        subjects,
        tags,
        question_types,
        pending_draft_count,
        database_healthy: true,
        database_error: None,
        app_version: state.app_version().to_owned(),
        license: state.license().overview(),
    })
}

#[tauri::command]
pub async fn get_license_overview(state: State<'_, AppState>) -> CommandResult<LicenseOverview> {
    Ok(state.license().overview())
}

#[tauri::command]
pub async fn export_license_request(
    state: State<'_, AppState>,
    output_path: String,
) -> CommandResult<LicenseFileResult> {
    let output = PathBuf::from(output_path.trim());
    if output
        .extension()
        .and_then(|value| value.to_str())
        .is_none_or(|value| !value.eq_ignore_ascii_case("tkreq"))
    {
        return Err(CommandError::new(
            "LICENSE_REQUEST_EXTENSION_INVALID",
            "授权申请必须保存为 .tkreq 文件。",
        ));
    }
    state
        .license()
        .export_activation_request(&output)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn import_desktop_license(
    state: State<'_, AppState>,
    input_path: String,
) -> CommandResult<LicenseOverview> {
    let input = PathBuf::from(input_path.trim());
    if input
        .extension()
        .and_then(|value| value.to_str())
        .is_none_or(|value| !value.eq_ignore_ascii_case("tklic"))
    {
        return Err(CommandError::new(
            "LICENSE_EXTENSION_INVALID",
            "桌面许可证必须是 .tklic 文件。",
        ));
    }
    state
        .license()
        .import_desktop_license(&input)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn remove_desktop_license(state: State<'_, AppState>) -> CommandResult<LicenseOverview> {
    state.license().remove_desktop_license().map_err(Into::into)
}

#[tauri::command]
pub async fn check_print_authorization(state: State<'_, AppState>) -> CommandResult<()> {
    state.license().require_print().map_err(Into::into)
}

#[tauri::command]
pub async fn list_question_types(
    state: State<'_, AppState>,
) -> CommandResult<Vec<QuestionTypeDefinitionApi>> {
    let database = state.database().await?;
    question_types::list(database.pool()).await
}

#[tauri::command]
pub async fn save_question_type(
    state: State<'_, AppState>,
    request: SaveQuestionTypeRequestApi,
) -> CommandResult<QuestionTypeDefinitionApi> {
    let database = state.database().await?;
    question_types::save(database.pool(), &request).await
}

#[tauri::command]
pub async fn delete_question_type(state: State<'_, AppState>, code: String) -> CommandResult<()> {
    let database = state.database().await?;
    question_types::delete(database.pool(), &code).await
}

#[tauri::command]
pub async fn save_question_type_order(
    state: State<'_, AppState>,
    items: Vec<TaxonomyOrderItemApi>,
) -> CommandResult<()> {
    let database = state.database().await?;
    question_types::save_order(database.pool(), &items).await
}

#[tauri::command]
pub async fn retry_database_initialize(state: State<'_, AppState>) -> CommandResult<()> {
    state.retry_initialize().await
}

#[tauri::command]
pub async fn list_questions(
    state: State<'_, AppState>,
    filters: QuestionFiltersApi,
) -> CommandResult<PageResultApi<QuestionApi>> {
    let database = state.database().await?;
    questions::list_questions(database.pool(), &filters).await
}

#[tauri::command]
pub async fn analyze_random_draw(
    state: State<'_, AppState>,
    scope: RandomDrawScopeApi,
) -> CommandResult<RandomDrawAnalysisApi> {
    let database = state.database().await?;
    random_draw::analyze(database.pool(), &scope).await
}

#[tauri::command]
pub async fn draw_random_questions(
    state: State<'_, AppState>,
    request: RandomDrawRequestApi,
) -> CommandResult<Vec<QuestionApi>> {
    let database = state.database().await?;
    random_draw::draw(database.pool(), &request).await
}

#[tauri::command]
pub async fn get_question(
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<Option<QuestionApi>> {
    let database = state.database().await?;
    questions::get_question(database.pool(), &id).await
}

#[tauri::command]
pub async fn check_question_duplicate(
    state: State<'_, AppState>,
    draft: QuestionDraftApi,
) -> CommandResult<QuestionDuplicateCheckApi> {
    let database = state.database().await?;
    questions::check_question_duplicate(database.pool(), &draft).await
}

#[tauri::command]
pub async fn check_question_duplicates_batch(
    state: State<'_, AppState>,
    request: QuestionDuplicateBatchRequestApi,
) -> CommandResult<QuestionDuplicateBatchResultApi> {
    let database = state.database().await?;
    questions::check_question_duplicates_batch(database.pool(), &request).await
}

#[tauri::command]
pub fn cancel_question_duplicate_scan(scan_id: String) -> CommandResult<()> {
    questions::cancel_question_duplicate_scan(&scan_id)
}

#[tauri::command]
pub async fn scan_question_duplicates(
    state: State<'_, AppState>,
    request: QuestionDuplicateScanRequestApi,
) -> CommandResult<QuestionDuplicateScanResultApi> {
    let database = state.database().await?;
    questions::scan_question_duplicates(database.pool(), &request).await
}

#[tauri::command]
pub async fn ignore_question_duplicate(
    state: State<'_, AppState>,
    request: IgnoreQuestionDuplicateRequestApi,
) -> CommandResult<()> {
    let database = state.database().await?;
    questions::ignore_question_duplicate(database.pool(), &request).await
}

#[tauri::command]
pub async fn save_question(
    state: State<'_, AppState>,
    draft: QuestionDraftApi,
) -> CommandResult<QuestionApi> {
    let database = state.database().await?;
    questions::save_question(database.pool(), &draft, state.app_version()).await
}

#[tauri::command]
pub async fn save_questions(
    state: State<'_, AppState>,
    drafts: Vec<QuestionDraftApi>,
) -> CommandResult<Vec<QuestionApi>> {
    state.license().require_batch_import()?;
    let database = state.database().await?;
    questions::save_questions(database.pool(), &drafts, state.app_version()).await
}

#[tauri::command]
pub async fn save_imported_questions(
    state: State<'_, AppState>,
    drafts: Vec<QuestionDraftApi>,
) -> CommandResult<Vec<String>> {
    state.license().require_batch_import()?;
    let database = state.database().await?;
    questions::save_imported_questions(database.pool(), &drafts, state.app_version()).await
}

#[tauri::command]
pub async fn save_imported_question_overwrites(
    state: State<'_, AppState>,
    request: ImportedQuestionOverwriteRequestApi,
) -> CommandResult<ImportedQuestionOverwriteResultApi> {
    state.license().require_batch_import()?;
    let database = state.database().await?;
    questions::save_imported_question_overwrites(database.pool(), &request, state.app_version())
        .await
}

#[tauri::command]
pub async fn save_imported_question(
    state: State<'_, AppState>,
    draft: QuestionDraftApi,
) -> CommandResult<QuestionApi> {
    state.license().require_batch_import()?;
    let database = state.database().await?;
    questions::save_question(database.pool(), &draft, state.app_version()).await
}

#[tauri::command]
pub async fn batch_edit_questions(
    state: State<'_, AppState>,
    request: QuestionBatchEditRequestApi,
) -> CommandResult<QuestionBatchEditResultApi> {
    let database = state.database().await?;
    questions::batch_edit_questions(database.pool(), &request, state.app_version()).await
}

#[tauri::command]
pub async fn get_question_draft(
    state: State<'_, AppState>,
    question_id: Option<String>,
) -> CommandResult<Option<QuestionDraftRecordApi>> {
    let database = state.database().await?;
    drafts::get_question_draft(database.pool(), question_id.as_deref()).await
}

#[tauri::command]
pub async fn list_question_drafts(
    state: State<'_, AppState>,
) -> CommandResult<Vec<QuestionDraftRecordApi>> {
    let database = state.database().await?;
    drafts::list_question_drafts(database.pool()).await
}

#[tauri::command]
pub async fn save_question_draft(
    state: State<'_, AppState>,
    draft: QuestionDraftApi,
) -> CommandResult<QuestionDraftRecordApi> {
    let database = state.database().await?;
    drafts::save_question_draft(database.pool(), &draft).await
}

#[tauri::command]
pub async fn delete_question_draft(
    state: State<'_, AppState>,
    question_id: Option<String>,
) -> CommandResult<()> {
    let database = state.database().await?;
    drafts::delete_question_draft(database.pool(), question_id.as_deref()).await
}

#[tauri::command]
pub async fn get_document_question_draft(
    state: State<'_, AppState>,
) -> CommandResult<Option<DocumentQuestionDraftRecordApi>> {
    let database = state.database().await?;
    drafts::get_document_question_draft(database.pool()).await
}

#[tauri::command]
pub async fn save_document_question_draft(
    state: State<'_, AppState>,
    payload: DocumentQuestionDraftPayloadApi,
) -> CommandResult<DocumentQuestionDraftRecordApi> {
    let database = state.database().await?;
    drafts::save_document_question_draft(database.pool(), &payload).await
}

#[tauri::command]
pub async fn delete_document_question_draft(state: State<'_, AppState>) -> CommandResult<()> {
    let database = state.database().await?;
    drafts::delete_document_question_draft(database.pool()).await
}

#[tauri::command]
pub async fn list_document_entry_templates(
    state: State<'_, AppState>,
) -> CommandResult<Vec<DocumentEntryTemplateApi>> {
    let database = state.database().await?;
    document_entry_templates::list(database.pool()).await
}

#[tauri::command]
pub async fn save_document_entry_template(
    state: State<'_, AppState>,
    request: SaveDocumentEntryTemplateRequestApi,
) -> CommandResult<DocumentEntryTemplateApi> {
    let database = state.database().await?;
    document_entry_templates::save(database.pool(), &request).await
}

#[tauri::command]
pub async fn set_default_document_entry_template(
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<DocumentEntryTemplateApi> {
    let database = state.database().await?;
    document_entry_templates::set_default(database.pool(), &id).await
}

#[tauri::command]
pub async fn delete_document_entry_template(
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<()> {
    let database = state.database().await?;
    document_entry_templates::delete(database.pool(), &id).await
}

#[tauri::command]
pub async fn get_word_import_draft(
    state: State<'_, AppState>,
) -> CommandResult<Option<WordImportDraftRecordApi>> {
    let database = state.database().await?;
    drafts::get_word_import_draft(database.pool()).await
}

#[tauri::command]
pub async fn save_word_import_draft(
    state: State<'_, AppState>,
    payload: WordImportDraftPayloadApi,
) -> CommandResult<WordImportDraftRecordApi> {
    let database = state.database().await?;
    drafts::save_word_import_draft(database.pool(), &payload).await
}

#[tauri::command]
pub async fn delete_word_import_draft(state: State<'_, AppState>) -> CommandResult<()> {
    let database = state.database().await?;
    drafts::delete_word_import_draft(database.pool()).await
}

#[tauri::command]
pub async fn move_questions_to_recycle(
    state: State<'_, AppState>,
    ids: Vec<String>,
) -> CommandResult<()> {
    let database = state.database().await?;
    questions::set_deleted(database.pool(), &ids, true, state.app_version()).await
}

#[tauri::command]
pub async fn restore_questions(state: State<'_, AppState>, ids: Vec<String>) -> CommandResult<()> {
    let database = state.database().await?;
    questions::set_deleted(database.pool(), &ids, false, state.app_version()).await
}

#[tauri::command]
pub async fn permanently_delete_questions(
    state: State<'_, AppState>,
    ids: Vec<String>,
) -> CommandResult<()> {
    let database = state.database().await?;
    questions::permanently_delete(database.pool(), &ids, state.app_version()).await
}

#[tauri::command]
pub async fn list_papers(
    state: State<'_, AppState>,
    filters: PaperFiltersApi,
) -> CommandResult<PageResultApi<PaperSummaryApi>> {
    let database = state.database().await?;
    papers::list_papers(database.pool(), &filters).await
}

#[tauri::command]
pub async fn get_paper(state: State<'_, AppState>, id: String) -> CommandResult<Option<PaperApi>> {
    let database = state.database().await?;
    papers::get_paper(database.pool(), &id).await
}

#[tauri::command]
pub async fn save_paper(state: State<'_, AppState>, paper: PaperApi) -> CommandResult<PaperApi> {
    state
        .license()
        .validate_paper_question_count(paper.items.len())?;
    let database = state.database().await?;
    papers::save_paper(database.pool(), &paper, state.app_version()).await
}

#[tauri::command]
pub async fn copy_paper(
    state: State<'_, AppState>,
    id: String,
    base_row_version: i64,
) -> CommandResult<PaperApi> {
    let database = state.database().await?;
    let question_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM paper_items WHERE paper_id = ?")
            .bind(&id)
            .fetch_one(database.pool())
            .await
            .map_err(CommandError::database)?;
    state
        .license()
        .validate_paper_question_count(usize::try_from(question_count).unwrap_or(usize::MAX))?;
    papers::copy_paper(database.pool(), &id, base_row_version, state.app_version()).await
}

#[tauri::command]
pub async fn delete_paper(
    state: State<'_, AppState>,
    id: String,
    base_row_version: i64,
) -> CommandResult<()> {
    let database = state.database().await?;
    papers::delete_paper(database.pool(), &id, base_row_version, state.app_version()).await
}

#[tauri::command]
pub async fn delete_papers(
    state: State<'_, AppState>,
    requests: Vec<PaperDeleteRequestApi>,
) -> CommandResult<()> {
    let database = state.database().await?;
    papers::delete_papers(database.pool(), &requests, state.app_version()).await
}

#[tauri::command]
pub async fn export_paper_docx(
    state: State<'_, AppState>,
    request: ExportPaperDocxRequestApi,
) -> CommandResult<PaperDocxExportResultApi> {
    state.license().require_document_export()?;
    // Export holds the maintenance write lease so another app command cannot
    // change the persisted paper snapshot or managed template mid-export.
    let database = state.exclusive_database().await?;
    paper_export::export_paper_docx(&database, request, state.app_version()).await
}

#[tauri::command]
pub async fn export_question_bank(
    state: State<'_, AppState>,
    format: String,
    output_path: String,
) -> CommandResult<QuestionBankExportResultApi> {
    state.license().require_document_export()?;
    // A maintenance read lease blocks restore/data-move switching while still
    // allowing ordinary question editing during a long export. The exporter
    // takes its own SQLite read transaction for a consistent content snapshot.
    let database = state.database().await?;
    question_bank_export::export_question_bank(&database, &format, output_path).await
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> CommandResult<AppSettingsApi> {
    let database = state.database().await?;
    settings::get_settings(database.pool()).await
}

#[tauri::command]
pub async fn save_settings(
    state: State<'_, AppState>,
    settings: AppSettingsApi,
) -> CommandResult<AppSettingsApi> {
    let database = state.database().await?;
    settings::save_settings(database.pool(), &settings).await
}

#[tauri::command]
pub async fn create_initial_subject(state: State<'_, AppState>, name: String) -> CommandResult<()> {
    let database = state.database().await?;
    taxonomy::create_initial_subject(database.pool(), &name, state.app_version()).await
}

#[tauri::command]
pub async fn create_subject(state: State<'_, AppState>, name: String) -> CommandResult<String> {
    let database = state.database().await?;
    taxonomy::create_subject(database.pool(), &name, state.app_version()).await
}

#[tauri::command]
pub async fn update_subject(
    state: State<'_, AppState>,
    id: String,
    name: String,
) -> CommandResult<()> {
    let database = state.database().await?;
    taxonomy::update_subject(database.pool(), &id, &name, state.app_version()).await
}

#[tauri::command]
pub async fn delete_subject(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    let database = state.database().await?;
    taxonomy::delete_subject(database.pool(), &id, state.app_version()).await
}

#[tauri::command]
pub async fn create_chapter(
    state: State<'_, AppState>,
    subject_id: String,
    name: String,
) -> CommandResult<String> {
    let database = state.database().await?;
    taxonomy::create_chapter(database.pool(), &subject_id, &name, state.app_version()).await
}

#[tauri::command]
pub async fn update_chapter(
    state: State<'_, AppState>,
    id: String,
    name: String,
) -> CommandResult<()> {
    let database = state.database().await?;
    taxonomy::update_chapter(database.pool(), &id, &name, state.app_version()).await
}

#[tauri::command]
pub async fn delete_chapter(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    let database = state.database().await?;
    taxonomy::delete_chapter(database.pool(), &id, state.app_version()).await
}

#[tauri::command]
pub async fn create_tag(state: State<'_, AppState>, name: String) -> CommandResult<String> {
    let database = state.database().await?;
    taxonomy::create_tag(database.pool(), &name, state.app_version()).await
}

#[tauri::command]
pub async fn update_tag(state: State<'_, AppState>, id: String, name: String) -> CommandResult<()> {
    let database = state.database().await?;
    taxonomy::update_tag(database.pool(), &id, &name, state.app_version()).await
}

#[tauri::command]
pub async fn delete_tag(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    let database = state.database().await?;
    taxonomy::delete_tag(database.pool(), &id, state.app_version()).await
}

#[tauri::command]
pub async fn save_subject_order(
    state: State<'_, AppState>,
    items: Vec<TaxonomyOrderItemApi>,
) -> CommandResult<()> {
    let database = state.database().await?;
    taxonomy::save_subject_order(database.pool(), &items, state.app_version()).await
}

#[tauri::command]
pub async fn save_chapter_order(
    state: State<'_, AppState>,
    subject_id: String,
    items: Vec<TaxonomyOrderItemApi>,
) -> CommandResult<()> {
    let database = state.database().await?;
    taxonomy::save_chapter_order(database.pool(), &subject_id, &items, state.app_version()).await
}

#[tauri::command]
pub async fn list_templates(state: State<'_, AppState>) -> CommandResult<Vec<WordTemplateApi>> {
    let database = state.database().await?;
    let records = templates::list_templates(database.pool()).await?;
    verify_template_files(records, database.paths().templates_dir().to_path_buf()).await
}

fn template_source_limits() -> DocxLimits {
    DocxLimits {
        allow_mathtype_ole: true,
        ..DocxLimits::default()
    }
}

#[tauri::command]
pub async fn import_template(
    state: State<'_, AppState>,
    source_path: String,
    name: Option<String>,
) -> CommandResult<TemplateImportResultApi> {
    let source = PathBuf::from(source_path.trim());
    let is_docx = source
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("docx"));
    if !is_docx {
        return Err(CommandError::validation(
            "当前版本只支持导入 .docx 格式的 Word 模板。",
        ));
    }

    let database = state.database().await?;
    let templates_dir = database.paths().templates_dir().to_path_buf();
    let worker_templates_dir = templates_dir.clone();
    let analyzed = tokio::task::spawn_blocking(move || {
        templates_core::import_managed_template(
            &source,
            &worker_templates_dir,
            &[],
            &template_source_limits(),
        )
    })
    .await
    .map_err(|error| {
        CommandError::new(
            "TEMPLATE_IMPORT_TASK_FAILED",
            format!("模板导入任务未能正常完成：{error}"),
        )
    })?
    .map_err(managed_template_error)?;

    let diagnostics = analyzed
        .diagnostics
        .iter()
        .map(diagnostic_api)
        .collect::<Vec<_>>();
    if !analyzed.imported {
        return Ok(TemplateImportResultApi {
            imported: false,
            template: None,
            diagnostics,
        });
    }

    let anchors = analyzed
        .anchors
        .iter()
        .map(template_anchor_api)
        .collect::<CommandResult<Vec<_>>>()?;
    let package_kind = package_kind_name(analyzed.package_kind).to_owned();
    let analysis_status = if anchors.is_empty() {
        "needs_configuration"
    } else if analyzed
        .diagnostics
        .iter()
        .any(|item| item.severity == DiagnosticSeverity::Warning)
    {
        "warning"
    } else {
        "ready"
    };
    let analysis = json!({
        "originalFileName": analyzed.original_file_name,
        "packageKind": package_kind,
        "archiveBytes": analyzed.archive_bytes,
        "partCount": analyzed.part_count,
        "totalUncompressedBytes": analyzed.total_uncompressed_bytes,
        "anchors": anchors,
        "diagnostics": diagnostics,
    });
    let file = analyzed.file.ok_or_else(|| {
        CommandError::new(
            "TEMPLATE_IMPORT_INCOMPLETE",
            "模板分析已通过，但托管文件信息缺失。",
        )
    })?;
    let display_name = name.unwrap_or_else(|| default_template_name(&analyzed.original_file_name));
    let record = templates::NewTemplateRecord {
        name: display_name,
        original_file_name: analyzed.original_file_name,
        file_rel_path: file.file_rel_path.clone(),
        file_sha256: file.file_sha256,
        file_byte_size: file.file_byte_size,
        analysis_status: analysis_status.to_owned(),
        analysis_schema_version: analyzed.analysis_schema_version,
        analysis,
        parser_version: analyzed.parser_version.to_owned(),
    };

    let committed =
        match templates::commit_import_template(database.pool(), record, state.app_version()).await
        {
            Ok(record) => record,
            Err(mut database_error) => {
                let cleanup_dir = templates_dir.clone();
                let managed_name = file.managed_file_name;
                let cleanup = tokio::task::spawn_blocking(move || {
                    templates_core::delete_managed_template_file(&cleanup_dir, &managed_name)
                })
                .await;
                match cleanup {
                    Ok(Ok(_)) => {}
                    Ok(Err(error)) => {
                        database_error
                            .message
                            .push_str(&format!(" 托管副本回滚也未完成：{}", error));
                    }
                    Err(error) => {
                        database_error
                            .message
                            .push_str(&format!(" 托管副本回滚任务未能正常完成：{}", error));
                    }
                }
                return Err(database_error);
            }
        };
    let template = verify_template_file(committed, templates_dir).await?;

    Ok(TemplateImportResultApi {
        imported: true,
        template: Some(template),
        diagnostics,
    })
}

#[tauri::command]
pub async fn get_template_configuration_preview(
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<TemplateConfigurationPreviewApi> {
    let database = state.database().await?;
    let record = templates::get_template(database.pool(), &id)
        .await?
        .ok_or_else(|| CommandError::new("TEMPLATE_NOT_FOUND", "找不到要配置的模板。"))?;
    let template_id = record.template.id.clone();
    let template_name = record.template.name.clone();
    let row_version = record.template.row_version;
    let templates_dir = database.paths().templates_dir().to_path_buf();
    let managed_file_name = record.managed_file_name;
    let expected_size = record.template.file_byte_size;
    let expected_hash = record.file_sha256;
    let preview = tokio::task::spawn_blocking(move || {
        let limits = template_source_limits();
        let bytes = templates_core::read_verified_managed_template(
            &templates_dir,
            &managed_file_name,
            expected_size,
            &expected_hash,
            &limits,
        )?;
        preview_template_configuration(&bytes, &limits)
            .map_err(templates_core::ManagedTemplateError::from)
    })
    .await
    .map_err(|error| {
        CommandError::new(
            "TEMPLATE_CONFIGURATION_PREVIEW_TASK_FAILED",
            format!("模板配置预览任务未能正常完成：{error}"),
        )
    })?
    .map_err(managed_template_error)?;
    let paragraphs = preview
        .paragraphs
        .into_iter()
        .map(|paragraph| {
            Ok(TemplateConfigurationParagraphApi {
                index: u32::try_from(paragraph.index).map_err(|_| {
                    CommandError::new(
                        "TEMPLATE_CONFIGURATION_PARAGRAPH_RANGE_INVALID",
                        "模板段落编号超出支持范围。",
                    )
                })?,
                text: paragraph.text,
            })
        })
        .collect::<CommandResult<Vec<_>>>()?;
    Ok(TemplateConfigurationPreviewApi {
        template_id,
        template_name,
        row_version,
        paragraphs,
        has_questions_anchor: preview.has_questions_anchor,
        has_title_anchor: preview.has_title_anchor,
    })
}

#[tauri::command]
pub async fn get_template_layout_preview(
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<TemplateLayoutPreviewApi> {
    let database = state.database().await?;
    let record = templates::get_template(database.pool(), &id)
        .await?
        .ok_or_else(|| CommandError::new("TEMPLATE_NOT_FOUND", "找不到要预览的模板。"))?;
    if !record.template.region_configured {
        return Err(CommandError::new(
            "TEMPLATE_REGION_NOT_CONFIGURED",
            "这个模板还没有配置试题替换区域，暂时不能用于试卷排版预览。",
        ));
    }

    let style_profile = record
        .analysis
        .get("styleProfile")
        .or_else(|| record.analysis.get("style_profile"))
        .filter(|value| !value.is_null())
        .map(|value| serde_json::from_value::<TemplateStyleProfile>(value.clone()))
        .transpose()
        .map_err(|error| {
            CommandError::new(
                "TEMPLATE_STYLE_PROFILE_CORRUPTED",
                format!("模板样式原型无法读取：{error}"),
            )
        })?;
    if style_profile
        .as_ref()
        .is_some_and(|profile| profile.schema_version != 1)
    {
        return Err(CommandError::new(
            "TEMPLATE_STYLE_PROFILE_VERSION_UNSUPPORTED",
            "模板样式原型版本不受当前程序支持。",
        ));
    }
    let total_style_bytes = style_profile
        .as_ref()
        .map(|profile| {
            profile
                .roles
                .values()
                .map(|prototype| {
                    prototype.paragraph_properties.len() + prototype.run_properties.len()
                })
                .sum::<usize>()
        })
        .unwrap_or_default();
    if total_style_bytes > 512 * 1024 {
        return Err(CommandError::new(
            "TEMPLATE_STYLE_PROFILE_TOO_LARGE",
            "模板样式原型数据异常过大，已停止生成预览。",
        ));
    }

    let template_id = record.template.id.clone();
    let template_name = record.template.name.clone();
    let row_version = record.template.row_version;
    let templates_dir = database.paths().templates_dir().to_path_buf();
    let managed_file_name = record.managed_file_name;
    let expected_size = record.template.file_byte_size;
    let expected_hash = record.file_sha256;
    let preview = tokio::task::spawn_blocking(move || {
        let limits = template_source_limits();
        let bytes = templates_core::read_verified_managed_template(
            &templates_dir,
            &managed_file_name,
            expected_size,
            &expected_hash,
            &limits,
        )?;
        preview_template_layout(&bytes, &limits).map_err(templates_core::ManagedTemplateError::from)
    })
    .await
    .map_err(|error| {
        CommandError::new(
            "TEMPLATE_LAYOUT_PREVIEW_TASK_FAILED",
            format!("模板排版预览任务未能正常完成：{error}"),
        )
    })?
    .map_err(managed_template_error)?;

    Ok(TemplateLayoutPreviewApi {
        template_id,
        template_name,
        row_version,
        page: preview.page,
        blocks: preview.blocks,
        font_theme: preview.font_theme,
        page_number_format: preview.page_number_format,
        style_profile,
    })
}

#[tauri::command]
pub async fn configure_template_regions(
    state: State<'_, AppState>,
    request: ConfigureTemplateRegionsRequestApi,
) -> CommandResult<WordTemplateApi> {
    let question_placement = template_region_placement(&request.questions)?;
    let style_selection = template_style_selection(request.style_samples.as_ref());
    let title_placement = request
        .title
        .as_ref()
        .map(template_region_placement)
        .transpose()?;
    let database = state.database().await?;
    let current = templates::get_template(database.pool(), &request.template_id)
        .await?
        .ok_or_else(|| CommandError::new("TEMPLATE_NOT_FOUND", "找不到要配置的模板。"))?;
    if current.template.row_version != request.base_row_version {
        return Err(CommandError::new(
            "TEMPLATE_VERSION_CONFLICT",
            "这个模板已在其他窗口中更新，请刷新页面后重试。",
        ));
    }
    let original_file_name = current.template.file_name.clone();
    let old_managed_file_name = current.managed_file_name.clone();
    let templates_dir = database.paths().templates_dir().to_path_buf();
    let worker_templates_dir = templates_dir.clone();
    let expected_size = current.template.file_byte_size;
    let expected_hash = current.file_sha256;
    let configuration = TemplateRegionConfiguration {
        questions: question_placement,
        styles: style_selection,
        title: title_placement,
    };
    let configured = tokio::task::spawn_blocking(move || {
        let limits = template_source_limits();
        let bytes = templates_core::read_verified_managed_template(
            &worker_templates_dir,
            &old_managed_file_name,
            expected_size,
            &expected_hash,
            &limits,
        )?;
        let package = configure_template_package(&bytes, &configuration, &limits)?;
        let style_profile = package.style_profile;
        let strict_limits = DocxLimits::default();
        let configured = templates_core::store_configured_managed_template(
            &package.bytes,
            &worker_templates_dir,
            &strict_limits,
        )?;
        Ok::<_, templates_core::ManagedTemplateError>((configured, style_profile))
    })
    .await
    .map_err(|error| {
        CommandError::new(
            "TEMPLATE_CONFIGURATION_TASK_FAILED",
            format!("模板配置任务未能正常完成：{error}"),
        )
    })?
    .map_err(managed_template_error)?;
    let (configured, style_profile) = configured;

    let anchors = configured
        .anchors
        .iter()
        .map(template_anchor_api)
        .collect::<CommandResult<Vec<_>>>()?;
    let diagnostics = configured
        .diagnostics
        .iter()
        .map(diagnostic_api)
        .collect::<Vec<_>>();
    let analysis_status = if configured
        .diagnostics
        .iter()
        .any(|item| item.severity == DiagnosticSeverity::Warning)
    {
        "warning"
    } else {
        "ready"
    };
    let analysis = json!({
        "originalFileName": original_file_name,
        "packageKind": package_kind_name(configured.package_kind),
        "archiveBytes": configured.archive_bytes,
        "partCount": configured.part_count,
        "totalUncompressedBytes": configured.total_uncompressed_bytes,
        "anchors": anchors,
        "diagnostics": diagnostics,
        "styleProfile": style_profile,
    });
    let new_managed_file_name = configured.file.managed_file_name.clone();
    let configured_record = templates::ConfiguredTemplateRecord {
        original_file_name,
        file_rel_path: configured.file.file_rel_path,
        file_sha256: configured.file.file_sha256,
        file_byte_size: configured.file.file_byte_size,
        analysis_status: analysis_status.to_owned(),
        analysis_schema_version: configured.analysis_schema_version,
        analysis,
        parser_version: configured.parser_version.to_owned(),
    };
    let committed = match templates::commit_template_configuration(
        database.pool(),
        &request.template_id,
        request.base_row_version,
        configured_record,
        state.app_version(),
    )
    .await
    {
        Ok(record) => record,
        Err(database_error) => {
            let cleanup_dir = templates_dir.clone();
            let _ = tokio::task::spawn_blocking(move || {
                templates_core::delete_managed_template_file(&cleanup_dir, &new_managed_file_name)
            })
            .await;
            return Err(database_error);
        }
    };

    let cleanup_dir = templates_dir.clone();
    let old_name = current.managed_file_name;
    let _ = tokio::task::spawn_blocking(move || {
        templates_core::delete_managed_template_file(&cleanup_dir, &old_name)
    })
    .await;
    verify_template_file(committed, templates_dir).await
}

#[tauri::command]
pub async fn rename_template(
    state: State<'_, AppState>,
    id: String,
    name: String,
    base_row_version: i64,
) -> CommandResult<WordTemplateApi> {
    let database = state.database().await?;
    let record = templates::rename_template(
        database.pool(),
        &id,
        &name,
        base_row_version,
        state.app_version(),
    )
    .await?;
    verify_template_file(record, database.paths().templates_dir().to_path_buf()).await
}

#[tauri::command]
pub async fn delete_template(
    state: State<'_, AppState>,
    id: String,
    base_row_version: i64,
) -> CommandResult<()> {
    let database = state.database().await?;
    let candidate = templates::get_delete_candidate(database.pool(), &id, base_row_version).await?;
    debug_assert_eq!(candidate.id, id);
    debug_assert_eq!(candidate.row_version, base_row_version);
    let templates_dir = database.paths().templates_dir().to_path_buf();
    let stage_dir = templates_dir.clone();
    let managed_file_name = candidate.managed_file_name;
    let tombstone = tokio::task::spawn_blocking(move || {
        templates_core::stage_managed_template_delete(&stage_dir, &managed_file_name)
    })
    .await
    .map_err(|error| {
        CommandError::new(
            "TEMPLATE_DELETE_STAGE_TASK_FAILED",
            format!("模板文件暂存删除任务未能正常完成：{error}"),
        )
    })?
    .map_err(managed_template_error)?;

    if let Err(mut database_error) = templates::commit_delete_template(
        database.pool(),
        &id,
        base_row_version,
        state.app_version(),
    )
    .await
    {
        if let Some(tombstone) = tombstone {
            let restore_dir = templates_dir.clone();
            match tokio::task::spawn_blocking(move || {
                templates_core::restore_managed_template_delete(&restore_dir, &tombstone)
            })
            .await
            {
                Ok(Ok(())) => {}
                Ok(Err(error)) => database_error
                    .message
                    .push_str(&format!(" 模板文件恢复失败：{error}")),
                Err(error) => database_error
                    .message
                    .push_str(&format!(" 模板文件恢复任务失败：{error}")),
            }
        }
        return Err(database_error);
    }

    if let Some(tombstone) = tombstone {
        let finalize_dir = templates_dir;
        let cleanup = tokio::task::spawn_blocking(move || {
            templates_core::finalize_managed_template_delete(&finalize_dir, &tombstone)
        })
        .await;
        let cleanup_error = match cleanup {
            Ok(Ok(())) => None,
            Ok(Err(error)) => Some(error.to_string()),
            Err(error) => Some(format!("清理任务未能正常完成：{error}")),
        };
        if let Some(error_message) = cleanup_error {
            let _ = templates::record_template_cleanup_warning(
                database.pool(),
                &id,
                &error_message,
                state.app_version(),
            )
            .await;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn set_default_template(
    state: State<'_, AppState>,
    id: String,
    base_row_version: i64,
) -> CommandResult<()> {
    let database = state.database().await?;
    templates::set_default_template(database.pool(), &id, base_row_version, state.app_version())
        .await?;
    Ok(())
}

#[tauri::command]
pub async fn list_backups(state: State<'_, AppState>) -> CommandResult<Vec<BackupRecordApi>> {
    let database = state.database().await?;
    backups::list_backups(&database).await
}

#[tauri::command]
pub async fn create_backup(
    state: State<'_, AppState>,
    output_path: String,
) -> CommandResult<BackupCreateResultApi> {
    let output = PathBuf::from(output_path.trim());
    let database = state.exclusive_database().await?;
    backups::create_manual_backup(&database, output, state.app_version()).await
}

#[tauri::command]
pub async fn run_automatic_backup(
    state: State<'_, AppState>,
) -> CommandResult<AutomaticBackupRunApi> {
    let database = state.exclusive_database().await?;
    backups::run_automatic_backup_if_due(&database, state.app_version()).await
}

#[tauri::command]
pub async fn inspect_backup(
    state: State<'_, AppState>,
    backup_path: String,
) -> CommandResult<BackupInspectionApi> {
    let _maintenance_lease = state.maintenance_read_lease().await;
    backups::inspect_backup_file(PathBuf::from(backup_path.trim())).await
}

#[tauri::command]
pub async fn prepare_restore(
    state: State<'_, AppState>,
    backup_path: String,
) -> CommandResult<RestorePlanApi> {
    restore_workflow::prepare_restore(&state, PathBuf::from(backup_path.trim())).await
}

#[tauri::command]
pub async fn cancel_restore(
    state: State<'_, AppState>,
    restore_token: String,
) -> CommandResult<()> {
    restore_workflow::cancel_restore(&state, restore_token.trim()).await
}

#[tauri::command]
pub async fn schedule_restore(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ScheduleRestoreRequestApi,
) -> CommandResult<RestoreScheduledApi> {
    let scheduled = restore_workflow::schedule_restore(&state, &request).await?;
    let restart_app = app.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(800));
        restart_app.restart();
    });
    Ok(scheduled)
}

#[tauri::command]
pub async fn get_last_restore_result(
    state: State<'_, AppState>,
) -> CommandResult<Option<RestoreResultApi>> {
    restore_workflow::get_last_restore_result(state.data_root())
}

#[tauri::command]
pub async fn acknowledge_restore_result(
    state: State<'_, AppState>,
    operation_id: String,
) -> CommandResult<()> {
    restore_workflow::acknowledge_restore_result(state.data_root(), operation_id.trim())
}

#[tauri::command]
pub async fn pick_data_directory(app: AppHandle) -> CommandResult<Option<String>> {
    tokio::task::spawn_blocking(move || {
        let selected = app
            .dialog()
            .file()
            .set_title("选择空的数据目标文件夹")
            .blocking_pick_folder();
        selected
            .map(|path| {
                path.into_path()
                    .map_err(|error| {
                        CommandError::new(
                            "DATA_MOVE_DIALOG_PATH_INVALID",
                            format!("无法读取目标文件夹路径：{error}"),
                        )
                    })
                    .and_then(|path| unicode_dialog_path(path, "DATA_MOVE_DIALOG_PATH_INVALID"))
            })
            .transpose()
    })
    .await
    .map_err(|error| {
        CommandError::new(
            "DATA_MOVE_DIALOG_TASK_FAILED",
            format!("目标文件夹选择窗口未能正常完成：{error}"),
        )
    })?
}

#[tauri::command]
pub async fn pick_export_directory(app: AppHandle) -> CommandResult<Option<String>> {
    tokio::task::spawn_blocking(move || {
        let selected = app
            .dialog()
            .file()
            .set_title("选择默认导出文件夹")
            .blocking_pick_folder();
        selected
            .map(|path| {
                path.into_path()
                    .map_err(|error| {
                        CommandError::new(
                            "EXPORT_DIRECTORY_PATH_INVALID",
                            format!("无法读取导出文件夹路径：{error}"),
                        )
                    })
                    .and_then(|path| unicode_dialog_path(path, "EXPORT_DIRECTORY_PATH_INVALID"))
            })
            .transpose()
    })
    .await
    .map_err(|error| {
        CommandError::new(
            "EXPORT_DIRECTORY_DIALOG_FAILED",
            format!("导出文件夹选择窗口未能正常完成：{error}"),
        )
    })?
}

#[tauri::command]
pub async fn open_data_directory(
    state: State<'_, AppState>,
    component: String,
) -> CommandResult<()> {
    let _maintenance = state.maintenance_read_lease().await;
    let path = match component.trim() {
        "root" => state.data_root().clone(),
        "database" => state.data_root().join("database"),
        "resources" => state.data_root().join("resources"),
        "templates" => state.data_root().join("templates"),
        "backup" => state.data_root().join("backup"),
        "export" => state.data_root().join("export"),
        _ => return Err(CommandError::validation("要打开的数据目录类型无效。")),
    };
    let metadata = fs::symlink_metadata(&path).map_err(|error| {
        CommandError::new(
            "DATA_DIRECTORY_OPEN_FAILED",
            format!("无法检查数据目录：{error}"),
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(CommandError::new(
            "DATA_DIRECTORY_OPEN_FAILED",
            "要打开的位置不是安全的真实目录。",
        ));
    }
    let windows_root = std::env::var_os("SystemRoot")
        .or_else(|| std::env::var_os("WINDIR"))
        .map(PathBuf::from)
        .filter(|root| root.is_absolute())
        .ok_or_else(|| {
            CommandError::new(
                "DATA_DIRECTORY_OPEN_FAILED",
                "无法确定 Windows 系统目录，因此没有启动文件资源管理器。",
            )
        })?;
    let explorer = windows_root.join("explorer.exe");
    if !explorer.is_file() {
        return Err(CommandError::new(
            "DATA_DIRECTORY_OPEN_FAILED",
            "Windows 文件资源管理器程序不存在或无法访问。",
        ));
    }
    Command::new(explorer).arg(&path).spawn().map_err(|error| {
        CommandError::new(
            "DATA_DIRECTORY_OPEN_FAILED",
            format!("无法打开 Windows 文件资源管理器：{error}"),
        )
    })?;
    Ok(())
}

#[tauri::command]
pub async fn open_exported_file(path: String) -> CommandResult<()> {
    let exported = validated_exported_document_path(&path)?;
    let explorer = windows_explorer_executable("PAPER_EXPORT_OPEN_FAILED")?;
    Command::new(explorer)
        .arg(exported)
        .spawn()
        .map_err(|error| {
            CommandError::new(
                "PAPER_EXPORT_OPEN_FAILED",
                format!("无法通过 Windows 文件资源管理器打开导出文件：{error}"),
            )
        })?;
    Ok(())
}

#[tauri::command]
pub async fn reveal_exported_file(path: String) -> CommandResult<()> {
    let exported = validated_exported_document_path(&path)?;
    let explorer = windows_explorer_executable("PAPER_EXPORT_REVEAL_FAILED")?;
    Command::new(explorer)
        .arg("/select,")
        .arg(exported)
        .spawn()
        .map_err(|error| {
            CommandError::new(
                "PAPER_EXPORT_REVEAL_FAILED",
                format!("无法在 Windows 文件资源管理器中定位导出文件：{error}"),
            )
        })?;
    Ok(())
}

fn validated_exported_document_path(raw: &str) -> CommandResult<PathBuf> {
    let requested = PathBuf::from(raw.trim());
    if raw.trim().is_empty() || !requested.is_absolute() {
        return Err(CommandError::validation("导出文件必须使用非空的绝对路径。"));
    }
    if !requested
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension.to_ascii_lowercase().as_str(), "docx" | "xlsx"))
    {
        return Err(CommandError::validation(
            "只能打开已导出的 .docx 或 .xlsx 文件。",
        ));
    }
    let metadata = fs::symlink_metadata(&requested).map_err(|error| {
        CommandError::new(
            "PAPER_EXPORT_FILE_UNAVAILABLE",
            format!("导出文件不存在或无法访问：{error}"),
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(CommandError::new(
            "PAPER_EXPORT_FILE_UNSAFE",
            "要打开的位置不是安全的普通文件。",
        ));
    }
    let canonical = fs::canonicalize(&requested).map_err(|error| {
        CommandError::new(
            "PAPER_EXPORT_FILE_UNAVAILABLE",
            format!("无法解析导出文件路径：{error}"),
        )
    })?;
    if !canonical
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension.to_ascii_lowercase().as_str(), "docx" | "xlsx"))
    {
        return Err(CommandError::new(
            "PAPER_EXPORT_FILE_UNSAFE",
            "导出文件解析后的类型不是 .docx 或 .xlsx。",
        ));
    }
    Ok(canonical)
}

fn windows_explorer_executable(code: &'static str) -> CommandResult<PathBuf> {
    let windows_root = std::env::var_os("SystemRoot")
        .or_else(|| std::env::var_os("WINDIR"))
        .map(PathBuf::from)
        .filter(|root| root.is_absolute())
        .ok_or_else(|| {
            CommandError::new(
                code,
                "无法确定 Windows 系统目录，因此没有启动文件资源管理器。",
            )
        })?;
    let explorer = windows_root.join("explorer.exe");
    let metadata = fs::symlink_metadata(&explorer).map_err(|error| {
        CommandError::new(code, format!("无法检查 Windows 文件资源管理器：{error}"))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(CommandError::new(
            code,
            "Windows 文件资源管理器程序不存在或不是安全的普通文件。",
        ));
    }
    Ok(explorer)
}

fn unicode_dialog_path(path: PathBuf, code: &'static str) -> CommandResult<String> {
    path.into_os_string().into_string().map_err(|_| {
        CommandError::new(
            code,
            "所选路径包含无法安全传给界面的字符，请换一个文件夹后重试。",
        )
    })
}

#[tauri::command]
pub async fn prepare_data_move(
    state: State<'_, AppState>,
    target_data_root: String,
) -> CommandResult<DataMovePlanApi> {
    data_move_workflow::prepare_data_move(&state, PathBuf::from(target_data_root.trim())).await
}

#[tauri::command]
pub async fn cancel_data_move(
    app: AppHandle,
    state: State<'_, AppState>,
    move_token: String,
) -> CommandResult<()> {
    let bootstrap_dir = app
        .path()
        .app_local_data_dir()
        .map_err(|error| {
            CommandError::new(
                "DATA_MOVE_BOOTSTRAP_PATH_FAILED",
                format!("无法确定数据目录引导位置：{error}"),
            )
        })?
        .join("bootstrap");
    data_move_workflow::cancel_data_move(&state, &bootstrap_dir, move_token.trim()).await
}

#[tauri::command]
pub async fn schedule_data_move(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ScheduleDataMoveRequestApi,
) -> CommandResult<DataMoveScheduledApi> {
    let bootstrap_dir = app
        .path()
        .app_local_data_dir()
        .map_err(|error| {
            CommandError::new(
                "DATA_MOVE_BOOTSTRAP_PATH_FAILED",
                format!("无法确定数据目录引导位置：{error}"),
            )
        })?
        .join("bootstrap");
    let scheduled =
        data_move_workflow::schedule_data_move(&state, &bootstrap_dir, &request).await?;
    let restart_app = app.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(800));
        restart_app.restart();
    });
    Ok(scheduled)
}

#[tauri::command]
pub async fn get_last_data_move_result(app: AppHandle) -> CommandResult<Option<DataMoveResultApi>> {
    let bootstrap_dir = app
        .path()
        .app_local_data_dir()
        .map_err(|error| {
            CommandError::new(
                "DATA_MOVE_BOOTSTRAP_PATH_FAILED",
                format!("无法确定数据目录引导位置：{error}"),
            )
        })?
        .join("bootstrap");
    data_move_workflow::get_last_data_move_result(&bootstrap_dir)
}

#[tauri::command]
pub async fn acknowledge_data_move_result(
    app: AppHandle,
    operation_id: String,
) -> CommandResult<()> {
    let bootstrap_dir = app
        .path()
        .app_local_data_dir()
        .map_err(|error| {
            CommandError::new(
                "DATA_MOVE_BOOTSTRAP_PATH_FAILED",
                format!("无法确定数据目录引导位置：{error}"),
            )
        })?
        .join("bootstrap");
    data_move_workflow::acknowledge_data_move_result(&bootstrap_dir, operation_id.trim())
}

#[tauri::command]
pub async fn pick_backup_save_path(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<Option<String>> {
    let _maintenance_lease = state.maintenance_read_lease().await;
    let backup_dir = state.data_root().join("backup");
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    tokio::task::spawn_blocking(move || {
        let selected = app
            .dialog()
            .file()
            .set_title("选择完整备份保存位置")
            .set_directory(backup_dir)
            .set_file_name(format!("TK试题题库备份_{timestamp}.tqb"))
            .add_filter("TK试题题库备份", &["tqb"])
            .blocking_save_file();
        selected
            .map(|path| {
                path.into_path()
                    .map(|path| path.to_string_lossy().into_owned())
                    .map_err(|error| {
                        CommandError::new(
                            "BACKUP_DIALOG_PATH_INVALID",
                            format!("无法读取备份文件路径：{error}"),
                        )
                    })
            })
            .transpose()
    })
    .await
    .map_err(|error| {
        CommandError::new(
            "BACKUP_DIALOG_TASK_FAILED",
            format!("备份保存窗口未能正常完成：{error}"),
        )
    })?
}

#[tauri::command]
pub async fn pick_backup_file(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<Option<String>> {
    let _maintenance_lease = state.maintenance_read_lease().await;
    let backup_dir = state.data_root().join("backup");
    tokio::task::spawn_blocking(move || {
        let selected = app
            .dialog()
            .file()
            .set_title("选择要检查或恢复的题库备份")
            .set_directory(backup_dir)
            .add_filter("TK试题题库备份", &["tqb"])
            .blocking_pick_file();
        selected
            .map(|path| {
                path.into_path()
                    .map(|path| path.to_string_lossy().into_owned())
                    .map_err(|error| {
                        CommandError::new(
                            "BACKUP_DIALOG_PATH_INVALID",
                            format!("无法读取备份文件路径：{error}"),
                        )
                    })
            })
            .transpose()
    })
    .await
    .map_err(|error| {
        CommandError::new(
            "BACKUP_DIALOG_TASK_FAILED",
            format!("备份文件选择窗口未能正常完成：{error}"),
        )
    })?
}

#[tauri::command]
pub async fn analyze_docx(
    state: State<'_, AppState>,
    request: AnalyzeDocxRequestApi,
) -> CommandResult<DocxAnalysisApi> {
    let _maintenance_lease = state.maintenance_read_lease().await;
    tokio::task::spawn_blocking(move || docx::analyze(request))
        .await
        .map_err(|error| {
            CommandError::new(
                "DOCX_ANALYSIS_TASK_FAILED",
                format!("Word 分析任务未能正常完成：{error}"),
            )
        })?
}

#[tauri::command]
pub async fn begin_word_import(
    state: State<'_, AppState>,
    request: AnalyzeDocxRequestApi,
) -> CommandResult<BeginWordImportResultApi> {
    state.license().require_batch_import()?;
    let database = state.exclusive_database().await?;
    let prepared = tokio::task::spawn_blocking(move || docx::prepare_word_import(request))
        .await
        .map_err(|error| {
            CommandError::new(
                "WORD_IMPORT_TASK_FAILED",
                format!("Word 导入准备任务未能正常完成：{error}"),
            )
        })??;
    resources::begin_word_import(database.pool(), database.paths(), prepared).await
}

#[tauri::command]
pub async fn inspect_excel_workbook(
    state: State<'_, AppState>,
    input_path: String,
) -> CommandResult<ExcelWorkbookInspectionApi> {
    let _maintenance_lease = state.maintenance_read_lease().await;
    tokio::task::spawn_blocking(move || excel_import::inspect_workbook(input_path))
        .await
        .map_err(|error| {
            CommandError::new(
                "XLSX_INSPECTION_TASK_FAILED",
                format!("Excel 检查任务未能正常完成：{error}"),
            )
        })?
}

#[tauri::command]
pub async fn begin_excel_import(
    state: State<'_, AppState>,
    request: ExcelImportRequestApi,
) -> CommandResult<BeginExcelImportResultApi> {
    state.license().require_batch_import()?;
    let database = state.exclusive_database().await?;
    let prepared = tokio::task::spawn_blocking(move || excel_import::prepare_import(request))
        .await
        .map_err(|error| {
            CommandError::new(
                "XLSX_IMPORT_TASK_FAILED",
                format!("Excel 导入准备任务未能正常完成：{error}"),
            )
        })??;
    resources::begin_excel_import(database.pool(), prepared).await
}

#[tauri::command]
pub async fn create_excel_import_template(
    output_path: String,
) -> CommandResult<ExcelImportTemplateResultApi> {
    tokio::task::spawn_blocking(move || excel_import::create_template(output_path))
        .await
        .map_err(|error| {
            CommandError::new(
                "XLSX_TEMPLATE_TASK_FAILED",
                format!("Excel 模板生成任务未能正常完成：{error}"),
            )
        })?
}

#[tauri::command]
pub async fn get_managed_image(
    state: State<'_, AppState>,
    resource_id: String,
) -> CommandResult<ManagedImagePayloadApi> {
    let database = state.database().await?;
    resources::get_managed_image(database.pool(), database.paths(), resource_id.trim()).await
}

#[tauri::command]
pub async fn store_managed_images(
    state: State<'_, AppState>,
    request: StoreManagedImagesRequestApi,
) -> CommandResult<Vec<ManagedImagePayloadApi>> {
    let database = state.database().await?;
    resources::store_managed_images(database.pool(), database.paths(), request).await
}

#[tauri::command]
pub async fn read_wps_clipboard_images(
    request: ReadWpsClipboardImagesRequestApi,
) -> CommandResult<Vec<WpsClipboardImagePayloadApi>> {
    tokio::task::spawn_blocking(move || resources::read_wps_clipboard_images(request))
        .await
        .map_err(|error| {
            CommandError::new(
                "WPS_CLIPBOARD_IMAGE_TASK_FAILED",
                format!("WPS 剪贴板图片读取任务未能正常完成：{error}"),
            )
        })?
}

#[tauri::command]
pub async fn pick_license_request_save_path(
    app: AppHandle,
    suggested_name: String,
) -> CommandResult<Option<String>> {
    let default_directory = app
        .path()
        .desktop_dir()
        .or_else(|_| app.path().document_dir())
        .map_err(|error| {
            CommandError::new(
                "LICENSE_DIALOG_DIRECTORY_FAILED",
                format!("无法确定授权申请默认保存目录：{error}"),
            )
        })?;
    let suggested_name = suggested_name.trim();
    let suggested_name = if suggested_name.len() <= 120
        && suggested_name.to_ascii_lowercase().ends_with(".tkreq")
        && !suggested_name.contains(['/', '\\', ':'])
    {
        suggested_name.to_owned()
    } else {
        "TK试题题库离线授权申请.tkreq".to_owned()
    };
    tokio::task::spawn_blocking(move || {
        let selected = app
            .dialog()
            .file()
            .set_title("保存本机离线授权申请")
            .set_directory(default_directory)
            .set_file_name(suggested_name)
            .add_filter("TK离线授权申请", &["tkreq"])
            .blocking_save_file();
        selected
            .map(|path| {
                path.into_path()
                    .map(|path| path.to_string_lossy().into_owned())
                    .map_err(|error| {
                        CommandError::new(
                            "LICENSE_DIALOG_PATH_INVALID",
                            format!("无法读取授权申请保存路径：{error}"),
                        )
                    })
            })
            .transpose()
    })
    .await
    .map_err(|error| {
        CommandError::new(
            "LICENSE_DIALOG_TASK_FAILED",
            format!("授权申请保存窗口未能正常完成：{error}"),
        )
    })?
}

#[tauri::command]
pub async fn pick_desktop_license_file(app: AppHandle) -> CommandResult<Option<String>> {
    tokio::task::spawn_blocking(move || {
        let selected = app
            .dialog()
            .file()
            .set_title("选择桌面专业许可证")
            .add_filter("TK桌面许可证", &["tklic"])
            .blocking_pick_file();
        selected
            .map(|path| {
                path.into_path()
                    .map(|path| path.to_string_lossy().into_owned())
                    .map_err(|error| {
                        CommandError::new(
                            "LICENSE_DIALOG_PATH_INVALID",
                            format!("无法读取所选许可证路径：{error}"),
                        )
                    })
            })
            .transpose()
    })
    .await
    .map_err(|error| {
        CommandError::new(
            "LICENSE_DIALOG_TASK_FAILED",
            format!("许可证选择窗口未能正常完成：{error}"),
        )
    })?
}

#[tauri::command]
pub async fn pick_docx_file(app: AppHandle) -> CommandResult<Option<String>> {
    tokio::task::spawn_blocking(move || {
        let selected = app
            .dialog()
            .file()
            .set_title("选择要导入的 Word 文档")
            .add_filter("Word 文档", &["docx"])
            .blocking_pick_file();
        selected
            .map(|path| {
                path.into_path()
                    .map(|path| path.to_string_lossy().into_owned())
                    .map_err(|error| {
                        CommandError::new(
                            "DOCX_DIALOG_PATH_INVALID",
                            format!("无法读取所选文件路径：{error}"),
                        )
                    })
            })
            .transpose()
    })
    .await
    .map_err(|error| {
        CommandError::new(
            "DOCX_DIALOG_TASK_FAILED",
            format!("Word 文件选择窗口未能正常完成：{error}"),
        )
    })?
}

#[tauri::command]
pub async fn pick_xlsx_file(app: AppHandle) -> CommandResult<Option<String>> {
    tokio::task::spawn_blocking(move || {
        let selected = app
            .dialog()
            .file()
            .set_title("选择要导入的 Excel 题库")
            .add_filter("Excel 工作簿", &["xlsx"])
            .blocking_pick_file();
        selected
            .map(|path| {
                path.into_path()
                    .map(|path| path.to_string_lossy().into_owned())
                    .map_err(|error| {
                        CommandError::new(
                            "XLSX_DIALOG_PATH_INVALID",
                            format!("无法读取所选文件路径：{error}"),
                        )
                    })
            })
            .transpose()
    })
    .await
    .map_err(|error| {
        CommandError::new(
            "XLSX_DIALOG_TASK_FAILED",
            format!("Excel 文件选择窗口未能正常完成：{error}"),
        )
    })?
}

#[tauri::command]
pub async fn pick_docx_save_path(
    app: AppHandle,
    state: State<'_, AppState>,
    suggested_name: String,
) -> CommandResult<Option<String>> {
    let safe_name = suggested_name
        .trim()
        .chars()
        .map(|character| {
            if matches!(
                character,
                '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
            ) {
                '_'
            } else {
                character
            }
        })
        .take(120)
        .collect::<String>();
    let safe_name = if safe_name.is_empty() {
        "未命名试卷.docx".to_owned()
    } else if safe_name.to_ascii_lowercase().ends_with(".docx") {
        safe_name
    } else {
        format!("{safe_name}.docx")
    };

    let database = state.database().await?;
    let app_settings = settings::get_settings(database.pool()).await?;
    let default_directory = validated_export_dialog_directory(
        &app_settings.default_export_directory,
        &state.data_root().join("export"),
    )?;

    tokio::task::spawn_blocking(move || {
        let selected = app
            .dialog()
            .file()
            .set_title("选择 Word 导出位置")
            .set_directory(default_directory)
            .set_file_name(safe_name)
            .add_filter("Word 文档", &["docx"])
            .blocking_save_file();
        selected
            .map(|path| {
                path.into_path()
                    .map(|path| path.to_string_lossy().into_owned())
                    .map_err(|error| {
                        CommandError::new(
                            "DOCX_DIALOG_PATH_INVALID",
                            format!("无法读取导出文件路径：{error}"),
                        )
                    })
            })
            .transpose()
    })
    .await
    .map_err(|error| {
        CommandError::new(
            "DOCX_DIALOG_TASK_FAILED",
            format!("Word 保存位置窗口未能正常完成：{error}"),
        )
    })?
}

#[tauri::command]
pub async fn pick_xlsx_save_path(
    app: AppHandle,
    state: State<'_, AppState>,
    suggested_name: String,
) -> CommandResult<Option<String>> {
    let safe_name = suggested_name
        .trim()
        .chars()
        .map(|character| {
            if matches!(
                character,
                '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
            ) {
                '_'
            } else {
                character
            }
        })
        .take(120)
        .collect::<String>();
    let safe_name = if safe_name.is_empty() {
        "TK试题题库.xlsx".to_owned()
    } else if safe_name.to_ascii_lowercase().ends_with(".xlsx") {
        safe_name
    } else {
        format!("{safe_name}.xlsx")
    };

    let database = state.database().await?;
    let app_settings = settings::get_settings(database.pool()).await?;
    let default_directory = validated_export_dialog_directory(
        &app_settings.default_export_directory,
        &state.data_root().join("export"),
    )?;

    tokio::task::spawn_blocking(move || {
        let selected = app
            .dialog()
            .file()
            .set_title("选择 Excel 导出位置")
            .set_directory(default_directory)
            .set_file_name(safe_name)
            .add_filter("Excel 工作簿", &["xlsx"])
            .blocking_save_file();
        selected
            .map(|path| {
                path.into_path()
                    .map(|path| path.to_string_lossy().into_owned())
                    .map_err(|error| {
                        CommandError::new(
                            "XLSX_DIALOG_PATH_INVALID",
                            format!("无法读取导出文件路径：{error}"),
                        )
                    })
            })
            .transpose()
    })
    .await
    .map_err(|error| {
        CommandError::new(
            "XLSX_DIALOG_TASK_FAILED",
            format!("Excel 保存位置窗口未能正常完成：{error}"),
        )
    })?
}

fn validated_export_dialog_directory(configured: &str, fallback: &Path) -> CommandResult<PathBuf> {
    let requested = if configured.trim().is_empty() {
        fallback.to_path_buf()
    } else {
        PathBuf::from(configured.trim())
    };
    if !requested.is_absolute() {
        return Err(CommandError::new(
            "EXPORT_DEFAULT_DIRECTORY_INVALID",
            "设置中的默认导出目录不是完整绝对路径，请到系统设置中重新选择。",
        ));
    }
    let canonical = fs::canonicalize(&requested).map_err(|error| {
        CommandError::new(
            "EXPORT_DEFAULT_DIRECTORY_UNAVAILABLE",
            format!("设置中的默认导出目录当前不可用，请到系统设置中重新选择：{error}"),
        )
    })?;
    let metadata = fs::metadata(&canonical).map_err(|error| {
        CommandError::new(
            "EXPORT_DEFAULT_DIRECTORY_UNAVAILABLE",
            format!("无法读取设置中的默认导出目录：{error}"),
        )
    })?;
    if !metadata.is_dir() {
        return Err(CommandError::new(
            "EXPORT_DEFAULT_DIRECTORY_INVALID",
            "设置中的默认导出位置不是文件夹，请到系统设置中重新选择。",
        ));
    }
    Ok(canonical)
}

async fn verify_template_files(
    records: Vec<templates::TemplateCatalogRecord>,
    templates_dir: PathBuf,
) -> CommandResult<Vec<WordTemplateApi>> {
    tokio::task::spawn_blocking(move || {
        records
            .into_iter()
            .map(|record| verified_template(record, &templates_dir))
            .collect()
    })
    .await
    .map_err(|error| {
        CommandError::new(
            "TEMPLATE_VERIFY_TASK_FAILED",
            format!("模板文件校验任务未能正常完成：{error}"),
        )
    })
}

async fn verify_template_file(
    record: templates::TemplateCatalogRecord,
    templates_dir: PathBuf,
) -> CommandResult<WordTemplateApi> {
    tokio::task::spawn_blocking(move || verified_template(record, &templates_dir))
        .await
        .map_err(|error| {
            CommandError::new(
                "TEMPLATE_VERIFY_TASK_FAILED",
                format!("模板文件校验任务未能正常完成：{error}"),
            )
        })
}

fn verified_template(
    mut record: templates::TemplateCatalogRecord,
    templates_dir: &Path,
) -> WordTemplateApi {
    let candidate = templates_dir.join(&record.managed_file_name);
    record.template.file_available = fs::symlink_metadata(candidate).is_ok_and(|metadata| {
        metadata.is_file()
            && !metadata.file_type().is_symlink()
            && metadata.len() == record.template.file_byte_size
    });
    record.template
}

fn diagnostic_api(diagnostic: &Diagnostic) -> DocxDiagnosticApi {
    DocxDiagnosticApi {
        severity: diagnostic.severity.to_string(),
        code: diagnostic.code.to_owned(),
        part_name: diagnostic.part_name.clone(),
        message: diagnostic.message.clone(),
        suggested_action: diagnostic.suggested_action.clone(),
    }
}

fn template_anchor_api(anchor: &TemplateAnchor) -> CommandResult<TemplateAnchorApi> {
    let paragraph_index = anchor
        .paragraph_index
        .map(u32::try_from)
        .transpose()
        .map_err(|_| {
            CommandError::new(
                "TEMPLATE_ANALYSIS_RANGE_INVALID",
                "模板锚点的段落编号超出支持范围。",
            )
        })?;
    Ok(TemplateAnchorApi {
        name: anchor.name.clone(),
        kind: match anchor.kind {
            AnchorKind::ContentControl => "content_control",
            AnchorKind::ParagraphMarker => "paragraph_marker",
        }
        .to_owned(),
        part_name: anchor.part_name.clone(),
        replacement_span: TemplateByteSpanApi {
            start: anchor.replacement_span.start as u64,
            end: anchor.replacement_span.end as u64,
        },
        container_span: TemplateByteSpanApi {
            start: anchor.container_span.start as u64,
            end: anchor.container_span.end as u64,
        },
        paragraph_index,
    })
}

fn template_region_placement(
    placement: &TemplateRegionPlacementApi,
) -> CommandResult<TemplateRegionPlacement> {
    let paragraph_index = || {
        placement
            .paragraph_index
            .map(|index| index as usize)
            .ok_or_else(|| CommandError::validation("请选择一个模板段落。"))
    };
    match placement.mode.as_str() {
        "replace_paragraph" => Ok(TemplateRegionPlacement::ReplaceParagraph(paragraph_index()?)),
        "replace_range" => Ok(TemplateRegionPlacement::ReplaceRange {
            start_paragraph: paragraph_index()?,
            end_paragraph: placement
                .end_paragraph_index
                .map(|index| index as usize)
                .ok_or_else(|| CommandError::validation("请选择试题区域的结束段落。"))?,
        }),
        "after_paragraph" => Ok(TemplateRegionPlacement::AfterParagraph(paragraph_index()?)),
        "document_end" => Ok(TemplateRegionPlacement::DocumentEnd),
        _ => Err(CommandError::validation("模板替换区域的放置方式无效。")),
    }
}

fn template_style_selection(samples: Option<&TemplateStyleSamplesApi>) -> TemplateStyleSelection {
    let Some(samples) = samples else {
        return TemplateStyleSelection::default();
    };
    TemplateStyleSelection {
        section_heading: samples
            .section_heading_paragraph_index
            .map(|index| index as usize),
        question: samples.question_paragraph_index.map(|index| index as usize),
        option: samples.option_paragraph_index.map(|index| index as usize),
        answer: samples.answer_paragraph_index.map(|index| index as usize),
        explanation: samples
            .explanation_paragraph_index
            .map(|index| index as usize),
    }
}

fn package_kind_name(kind: PackageKind) -> &'static str {
    match kind {
        PackageKind::Document => "document",
        PackageKind::Template => "template",
        PackageKind::Unknown => "unknown",
    }
}

fn default_template_name(original_file_name: &str) -> String {
    Path::new(original_file_name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.trim().is_empty())
        .unwrap_or("未命名模板")
        .to_owned()
}

fn managed_template_error(error: templates_core::ManagedTemplateError) -> CommandError {
    CommandError::new(error.code(), format!("模板文件处理失败：{error}"))
}
