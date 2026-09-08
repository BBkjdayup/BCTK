use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    docx::{TemplateFontTheme, TemplateLayoutBlock, TemplatePageSetup, TemplateStyleProfile},
    licensing::{LicenseError, LicenseOverview},
};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomaticGenerationConfigDto {
    #[serde(default, alias = "subject_id")]
    pub subject_id: Option<String>,
    #[serde(default, alias = "chapter_ids")]
    pub chapter_ids: Vec<String>,
    #[serde(default, alias = "question_type_counts")]
    pub question_type_counts: BTreeMap<String, u32>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RichContentApi {
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editor_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document: Option<serde_json::Value>,
    pub html: String,
    pub plain_text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ooxml: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterApi {
    pub id: String,
    pub subject_id: String,
    pub name: String,
    pub sort_order: i64,
    pub question_count: i64,
    pub last_accessed_at: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubjectApi {
    pub id: String,
    pub name: String,
    pub sort_order: i64,
    pub question_count: i64,
    pub last_accessed_at: Option<i64>,
    pub chapters: Vec<ChapterApi>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagApi {
    pub id: String,
    pub name: String,
    pub question_count: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaxonomyOrderItemApi {
    pub id: String,
    pub sort_order: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionTypeDefinitionApi {
    pub code: String,
    pub name: String,
    pub behavior: String,
    pub aliases: Vec<String>,
    pub default_options: Vec<String>,
    pub is_builtin: bool,
    pub is_enabled: bool,
    pub sort_order: i64,
    pub question_count: i64,
    pub paper_item_count: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveQuestionTypeRequestApi {
    pub code: Option<String>,
    pub name: String,
    pub behavior: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub default_options: Vec<String>,
    pub is_enabled: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapDataApi {
    pub initialized: bool,
    pub data_root: String,
    pub subjects: Vec<SubjectApi>,
    pub tags: Vec<TagApi>,
    pub question_types: Vec<QuestionTypeDefinitionApi>,
    pub pending_draft_count: i64,
    pub database_healthy: bool,
    pub database_error: Option<String>,
    pub app_version: String,
    pub license: LicenseOverview,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionOptionApi {
    pub id: String,
    pub position: u32,
    pub content: RichContentApi,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuestionResourceRefApi {
    pub node_id: String,
    pub resource_id: String,
    pub content_slot: String,
    #[serde(default)]
    pub option_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionApi {
    pub id: String,
    #[serde(rename = "type")]
    pub question_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question_type_name: Option<String>,
    pub stem: RichContentApi,
    pub options: Vec<QuestionOptionApi>,
    pub answer: RichContentApi,
    pub explanation: RichContentApi,
    pub subject_id: String,
    pub chapter_id: String,
    pub subject_name: String,
    pub chapter_name: String,
    pub tags: Vec<TagApi>,
    #[serde(default)]
    pub resource_refs: Vec<QuestionResourceRefApi>,
    pub last_used_at: Option<i64>,
    pub deleted_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
    pub content_version: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionDraftApi {
    pub id: Option<String>,
    pub question_id: Option<String>,
    #[serde(rename = "type")]
    pub question_type: String,
    pub stem: RichContentApi,
    pub options: Vec<QuestionOptionApi>,
    pub answer: RichContentApi,
    pub explanation: RichContentApi,
    pub subject_id: String,
    pub chapter_id: String,
    #[serde(default)]
    pub tag_ids: Vec<String>,
    #[serde(default)]
    pub resource_refs: Vec<QuestionResourceRefApi>,
    pub base_content_version: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionDraftRecordApi {
    pub id: String,
    pub draft_key: String,
    pub draft_kind: String,
    pub target_question_id: Option<String>,
    pub base_content_version: Option<i64>,
    pub payload_schema_version: u32,
    pub payload: QuestionDraftApi,
    pub autosaved_at: i64,
    pub created_at: i64,
    pub updated_at: i64,
    pub stale: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DocumentQuestionDraftItemApi {
    pub id: String,
    pub ordinal: u32,
    pub payload: QuestionDraftApi,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum DocumentEntryOptionStyleApi {
    #[serde(rename = "letter_dot")]
    Dot,
    #[serde(rename = "letter_comma")]
    Comma,
    #[serde(rename = "letter_parentheses")]
    Parentheses,
    #[serde(rename = "letter_brackets")]
    Brackets,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DocumentEntryTemplateConfigApi {
    pub schema_version: u32,
    pub type_marker: String,
    pub stem_marker: String,
    pub answer_marker: String,
    pub explanation_marker: String,
    pub option_style: DocumentEntryOptionStyleApi,
    pub question_separator: String,
    pub compatible_default_markers: bool,
    #[serde(default = "default_enabled")]
    pub recognize_question_numbers: bool,
    #[serde(default = "default_enabled")]
    pub strip_recognized_question_numbers: bool,
    pub stem_prompt: String,
    pub option_prompt: String,
    pub answer_prompt: String,
    pub explanation_prompt: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentEntryTemplateApi {
    pub id: String,
    pub name: String,
    pub config: DocumentEntryTemplateConfigApi,
    pub is_built_in: bool,
    pub is_default: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveDocumentEntryTemplateRequestApi {
    pub id: Option<String>,
    pub name: String,
    pub config: DocumentEntryTemplateConfigApi,
    pub set_as_default: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DocumentQuestionDraftPayloadApi {
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_item_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<DocumentQuestionDraftItemApi>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_name_snapshot: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_config_snapshot: Option<DocumentEntryTemplateConfigApi>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<RichContentApi>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentQuestionDraftRecordApi {
    pub id: String,
    pub payload: DocumentQuestionDraftPayloadApi,
    pub autosaved_at: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuestionDuplicateCandidateApi {
    pub id: String,
    #[serde(rename = "type")]
    pub question_type: String,
    pub stem_preview: String,
    pub subject_name: String,
    pub chapter_name: String,
    pub similarity_percent: u32,
    pub content_version: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ordinal: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WordImportDraftItemApi {
    pub id: String,
    pub ordinal: u32,
    pub selected: bool,
    pub payload: QuestionDraftApi,
    pub status: String,
    pub diagnostic: Option<String>,
    pub duplicate_kind: String,
    pub candidate: Option<QuestionDuplicateCandidateApi>,
    pub action: Option<String>,
    pub needs_check: bool,
    pub check_error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WordImportDraftPayloadApi {
    pub schema_version: u32,
    pub source_file_name: String,
    pub source_file_size: u64,
    pub parser_version: String,
    #[serde(default)]
    pub import_session_id: Option<String>,
    #[serde(default)]
    pub source_item_count: Option<u32>,
    #[serde(default)]
    pub omitted_item_count: u32,
    pub active_item_id: Option<String>,
    pub items: Vec<WordImportDraftItemApi>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WordImportDraftRecordApi {
    pub id: String,
    pub payload: WordImportDraftPayloadApi,
    pub autosaved_at: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionDuplicateCheckApi {
    /// `exact` uses the persisted SHA-256 fingerprint, `suspected` uses the
    /// bounded NFKC character-bigram Dice score, and `none` is below the
    /// documented threshold.
    pub status: String,
    pub candidate: Option<QuestionDuplicateCandidateApi>,
    pub evaluated_candidate_count: u32,
    pub suspected_threshold_percent: u32,
    pub similarity_method: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuestionDuplicateBatchEntryApi {
    pub client_id: String,
    pub draft: QuestionDraftApi,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuestionDuplicateBatchRequestApi {
    pub items: Vec<QuestionDuplicateBatchEntryApi>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub scan_id: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionDuplicateBatchItemResultApi {
    pub client_id: String,
    pub exact_fingerprint: String,
    pub check: QuestionDuplicateCheckApi,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionDuplicateBatchResultApi {
    pub items: Vec<QuestionDuplicateBatchItemResultApi>,
}

fn default_enabled() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuestionDuplicateScanRequestApi {
    pub subject_id: String,
    #[serde(default)]
    pub chapter_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IgnoreQuestionDuplicateRequestApi {
    pub first_question_id: String,
    pub first_content_version: i64,
    pub second_question_id: String,
    pub second_content_version: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionDuplicateMemberApi {
    pub id: String,
    #[serde(rename = "type")]
    pub question_type: String,
    pub stem_preview: String,
    pub subject_id: String,
    pub chapter_id: String,
    pub subject_name: String,
    pub chapter_name: String,
    pub content_version: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionDuplicateGroupApi {
    pub id: String,
    pub duplicate_kind: String,
    pub similarity_percent: u32,
    pub members: Vec<QuestionDuplicateMemberApi>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionDuplicateScanResultApi {
    pub scanned_question_count: u32,
    pub compared_question_count: u32,
    pub suspected_threshold_percent: u32,
    pub exact_groups: Vec<QuestionDuplicateGroupApi>,
    pub suspected_groups: Vec<QuestionDuplicateGroupApi>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionFiltersApi {
    #[serde(default)]
    pub keyword: String,
    pub subject_id: Option<String>,
    pub chapter_id: Option<String>,
    #[serde(rename = "type")]
    pub question_type: Option<String>,
    #[serde(default)]
    pub tag_ids: Vec<String>,
    #[serde(default = "default_tag_match_mode")]
    pub tag_match_mode: String,
    #[serde(default = "default_usage")]
    pub usage: String,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default = "default_page")]
    pub page: u32,
    #[serde(default = "default_page_size")]
    pub page_size: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RandomDrawScopeApi {
    #[serde(default)]
    pub subject_ids: Vec<String>,
    #[serde(default)]
    pub chapter_ids: Vec<String>,
    #[serde(default)]
    pub tag_ids: Vec<String>,
    #[serde(default = "default_tag_match_mode")]
    pub tag_match_mode: String,
    #[serde(default = "default_usage")]
    pub usage: String,
    #[serde(default)]
    pub excluded_question_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RandomDrawAnalysisApi {
    pub available_total: u32,
    pub available_by_type: BTreeMap<String, u32>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RandomDrawRequestApi {
    pub scope: RandomDrawScopeApi,
    /// One of `total` or `by_type`.
    pub count_mode: String,
    #[serde(default)]
    pub total_count: u32,
    #[serde(default)]
    pub question_type_counts: BTreeMap<String, u32>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuestionBatchTargetApi {
    pub id: String,
    pub expected_content_version: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuestionBatchClassificationApi {
    pub subject_id: String,
    pub chapter_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuestionBatchTagOperationApi {
    /// One of `keep`, `append`, `replace`, or `remove`.
    pub mode: String,
    #[serde(default)]
    pub tag_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuestionBatchEditRequestApi {
    pub questions: Vec<QuestionBatchTargetApi>,
    pub classification: Option<QuestionBatchClassificationApi>,
    /// `keep` preserves the timestamp; `reset_never` clears it.
    pub usage_operation: String,
    pub tag_operation: QuestionBatchTagOperationApi,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuestionBatchVersionApi {
    pub id: String,
    pub content_version: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionBatchEditResultApi {
    pub updated_count: u32,
    pub questions: Vec<QuestionBatchVersionApi>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportedQuestionOverwriteItemApi {
    /// Stable ID of the source import item. It is echoed in the lightweight result.
    pub client_id: String,
    pub draft: QuestionDraftApi,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportedQuestionOverwriteRequestApi {
    /// Stable UUID retained by the caller while retrying an uncertain response.
    pub operation_id: String,
    pub items: Vec<ImportedQuestionOverwriteItemApi>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedQuestionOverwriteResultItemApi {
    pub client_id: String,
    pub question_id: String,
    pub content_version: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedQuestionOverwriteResultApi {
    pub operation_id: String,
    pub updated_count: u32,
    pub items: Vec<ImportedQuestionOverwriteResultItemApi>,
    pub replayed: bool,
}

fn default_usage() -> String {
    "all".to_owned()
}
fn default_tag_match_mode() -> String {
    "all".to_owned()
}
fn default_page() -> u32 {
    1
}
fn default_page_size() -> u32 {
    20
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageResultApi<T> {
    pub items: Vec<T>,
    pub total: i64,
    pub page: u32,
    pub page_size: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaperItemApi {
    pub id: String,
    pub source_question_id: Option<String>,
    pub position: u32,
    pub snapshot: QuestionApi,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaperApi {
    pub id: String,
    pub title: String,
    pub composition_mode: String,
    #[serde(default)]
    pub generation_config: Option<AutomaticGenerationConfigDto>,
    pub status: String,
    pub items: Vec<PaperItemApi>,
    pub export_content_mode: String,
    pub subject_summary_text: String,
    #[serde(default)]
    pub preferred_template_id: Option<String>,
    #[serde(default)]
    pub layout: Option<serde_json::Value>,
    #[serde(default)]
    pub layout_updated_at: Option<i64>,
    pub row_version: i64,
    pub created_at: i64,
    pub updated_at: i64,
    pub saved_at: Option<i64>,
    pub last_saved_at: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaperSummaryApi {
    pub id: String,
    pub title: String,
    pub composition_mode: String,
    pub status: String,
    pub subject_summary_text: String,
    pub question_count: i64,
    pub row_version: i64,
    pub created_at: i64,
    pub updated_at: i64,
    pub saved_at: Option<i64>,
    pub last_saved_at: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PaperDeleteRequestApi {
    pub id: String,
    pub base_row_version: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaperFiltersApi {
    #[serde(default)]
    pub keyword: String,
    #[serde(default = "default_paper_status")]
    pub status: String,
    #[serde(default = "default_page")]
    pub page: u32,
    #[serde(default = "default_paper_page_size")]
    pub page_size: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportPaperDocxRequestApi {
    pub paper_id: String,
    pub expected_paper_row_version: i64,
    pub template_id: String,
    pub output_path: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaperDocxExportResultApi {
    pub exported: bool,
    pub export_run_id: Option<String>,
    pub paper_id: String,
    pub paper_row_version: i64,
    pub template_id: String,
    pub template_name: String,
    pub content_mode: String,
    pub output_path: String,
    pub output_filename: String,
    pub output_bytes: Option<u64>,
    pub rewritten_parts: Vec<String>,
    pub diagnostics: Vec<DocxDiagnosticApi>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionBankExportResultApi {
    pub format: String,
    pub output_path: String,
    pub output_filename: String,
    pub output_bytes: u64,
    pub question_count: u32,
}

fn default_paper_status() -> String {
    "all".to_owned()
}

fn default_paper_page_size() -> u32 {
    50
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettingsApi {
    pub default_export_directory: String,
    pub default_template_id: Option<String>,
    pub export_filename_pattern: String,
    pub optional_confirmations: bool,
    pub recent_unused_days: u32,
    pub recent_added_days: u32,
    pub recent_used_days: u32,
    pub recycle_retention_days: u32,
    pub recycle_policy: String,
    pub automatic_backup_enabled: bool,
    pub automatic_backup_interval_days: u32,
    pub automatic_backup_retention_count: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzeDocxRequestApi {
    pub input_path: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocxDiagnosticApi {
    pub severity: String,
    pub code: String,
    pub part_name: Option<String>,
    pub message: String,
    pub suggested_action: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateByteSpanApi {
    pub start: u64,
    pub end: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateAnchorApi {
    pub name: String,
    pub kind: String,
    pub part_name: String,
    pub replacement_span: TemplateByteSpanApi,
    pub container_span: TemplateByteSpanApi,
    pub paragraph_index: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WordTemplateApi {
    pub id: String,
    pub name: String,
    /// The original display file name, never a client-controlled managed path.
    pub file_name: String,
    pub file_sha256_hex: String,
    pub file_byte_size: u64,
    pub analysis_status: String,
    pub analysis_schema_version: u32,
    pub parser_version: String,
    pub row_version: i64,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_verified_at: Option<i64>,
    pub is_default: bool,
    pub file_available: bool,
    pub region_configured: bool,
    pub package_kind: String,
    pub anchors: Vec<TemplateAnchorApi>,
    pub diagnostics: Vec<DocxDiagnosticApi>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateImportResultApi {
    pub imported: bool,
    pub template: Option<WordTemplateApi>,
    pub diagnostics: Vec<DocxDiagnosticApi>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TemplateRegionPlacementApi {
    pub mode: String,
    #[serde(default)]
    pub paragraph_index: Option<u32>,
    #[serde(default)]
    pub end_paragraph_index: Option<u32>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TemplateStyleSamplesApi {
    #[serde(default)]
    pub section_heading_paragraph_index: Option<u32>,
    #[serde(default)]
    pub question_paragraph_index: Option<u32>,
    #[serde(default)]
    pub option_paragraph_index: Option<u32>,
    #[serde(default)]
    pub answer_paragraph_index: Option<u32>,
    #[serde(default)]
    pub explanation_paragraph_index: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfigureTemplateRegionsRequestApi {
    pub template_id: String,
    pub base_row_version: i64,
    pub questions: TemplateRegionPlacementApi,
    #[serde(default)]
    pub style_samples: Option<TemplateStyleSamplesApi>,
    #[serde(default)]
    pub title: Option<TemplateRegionPlacementApi>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateConfigurationParagraphApi {
    pub index: u32,
    pub text: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateConfigurationPreviewApi {
    pub template_id: String,
    pub template_name: String,
    pub row_version: i64,
    pub paragraphs: Vec<TemplateConfigurationParagraphApi>,
    pub has_questions_anchor: bool,
    pub has_title_anchor: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateLayoutPreviewApi {
    pub template_id: String,
    pub template_name: String,
    pub row_version: i64,
    pub page: TemplatePageSetup,
    pub blocks: Vec<TemplateLayoutBlock>,
    pub font_theme: TemplateFontTheme,
    pub page_number_format: Option<String>,
    pub style_profile: Option<TemplateStyleProfile>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupRecordApi {
    pub id: String,
    pub backup_kind: String,
    pub status: String,
    pub display_filename: String,
    pub format_version: u32,
    pub database_schema_version: u32,
    pub source_database_uuid: String,
    pub source_app_version: String,
    pub archive_sha256_hex: Option<String>,
    pub manifest_sha256_hex: Option<String>,
    pub archive_byte_size: Option<u64>,
    pub payload_file_count: u64,
    pub payload_byte_size: u64,
    pub created_at: i64,
    pub completed_at: Option<i64>,
    /// `None` means the user saved outside the managed backup directory, so
    /// the catalogue deliberately does not retain the external absolute path.
    pub file_available: Option<bool>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupInspectionApi {
    pub format_version: u32,
    pub created_at: i64,
    pub source_app_version: String,
    pub database_schema_version: u32,
    pub source_database_uuid: String,
    pub archive_sha256_hex: String,
    pub manifest_sha256_hex: String,
    pub archive_byte_size: u64,
    pub payload_byte_size: u64,
    pub payload_file_count: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupCreateResultApi {
    pub output_path: String,
    pub record: BackupRecordApi,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomaticBackupRunApi {
    pub outcome: String,
    pub message: String,
    pub record: Option<BackupRecordApi>,
    pub pruned_count: u32,
    pub last_backup_at: Option<i64>,
    pub next_due_at: Option<i64>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RestoreDataSummaryApi {
    pub database_uuid: String,
    pub database_schema_version: i64,
    pub question_count: u64,
    pub paper_count: u64,
    pub template_count: u64,
    pub resource_count: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestorePlanApi {
    pub restore_token: String,
    pub expires_at: i64,
    pub source_display_filename: String,
    pub archive_inspection: BackupInspectionApi,
    pub current_summary: Option<RestoreDataSummaryApi>,
    pub incoming_summary: RestoreDataSummaryApi,
    pub current_database_healthy: bool,
    pub warnings: Vec<String>,
    pub automatic_pre_restore_backup_required: bool,
    pub requires_restart: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScheduleRestoreRequestApi {
    pub restore_token: String,
    pub expected_archive_sha256_hex: String,
    pub confirmed_current_data_replacement: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreScheduledApi {
    pub operation_id: String,
    pub pre_restore_backup_filename: Option<String>,
    pub requires_restart: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RestoreResultApi {
    pub operation_id: String,
    pub outcome: String,
    pub restored_database_uuid: Option<String>,
    pub pre_restore_backup_filename: Option<String>,
    pub summary: Option<RestoreDataSummaryApi>,
    pub duration_ms: i64,
    pub warnings: Vec<String>,
    pub error_message: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataMovePlanApi {
    pub move_token: String,
    pub expires_at: i64,
    pub source_data_root: String,
    pub target_data_root: String,
    pub estimated_file_count: u64,
    pub estimated_byte_size: u64,
    pub warnings: Vec<String>,
    pub requires_restart: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScheduleDataMoveRequestApi {
    pub move_token: String,
    pub expected_source_data_root: String,
    pub expected_target_data_root: String,
    pub confirmed_keep_old_data: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataMoveScheduledApi {
    pub operation_id: String,
    pub source_data_root: String,
    pub target_data_root: String,
    pub copied_file_count: u64,
    pub copied_byte_size: u64,
    pub requires_restart: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DataMoveResultApi {
    pub operation_id: String,
    pub outcome: String,
    pub active_data_root: String,
    pub retained_data_root: String,
    pub copied_file_count: u64,
    pub copied_byte_size: u64,
    pub duration_ms: i64,
    pub warnings: Vec<String>,
    pub error_message: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocxParagraphApi {
    pub index: u32,
    pub text: String,
    pub formula_count: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocxAnalysisApi {
    pub source_path: String,
    pub archive_bytes: u64,
    pub package_kind: String,
    pub is_valid: bool,
    pub visible_text: String,
    pub paragraph_count: u32,
    pub formula_count: u32,
    pub part_count: u32,
    pub total_uncompressed_bytes: u64,
    pub paragraphs: Vec<DocxParagraphApi>,
    pub diagnostics: Vec<DocxDiagnosticApi>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocxImageOccurrenceApi {
    pub node_id: String,
    pub resource_id: String,
    pub paragraph_index: u32,
    pub text_char_offset: u32,
    pub original_filename: Option<String>,
    pub mime_type: String,
    pub byte_size: u64,
    pub width_px: u32,
    pub height_px: u32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocxFormulaOccurrenceApi {
    pub node_id: String,
    pub paragraph_index: u32,
    pub text_char_offset: u32,
    pub latex: String,
    pub source_kind: String,
    pub product_version: u8,
    pub product_subversion: u8,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocxTableCellApi {
    pub paragraph_indices: Vec<u32>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocxTableOccurrenceApi {
    pub table_index: u32,
    pub rows: Vec<Vec<DocxTableCellApi>>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BeginWordImportResultApi {
    pub analysis: DocxAnalysisApi,
    pub draft: WordImportDraftRecordApi,
    pub images: Vec<DocxImageOccurrenceApi>,
    pub formulas: Vec<DocxFormulaOccurrenceApi>,
    pub tables: Vec<DocxTableOccurrenceApi>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExcelImportRequestApi {
    pub input_path: String,
    #[serde(default)]
    pub sheet_name: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExcelWorkbookInspectionApi {
    pub source_path: String,
    pub source_file_name: String,
    pub source_file_size: u64,
    pub sheet_names: Vec<String>,
    pub default_sheet_name: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExcelImportRowApi {
    pub row_number: u32,
    pub question_type: String,
    pub subject_name: String,
    pub chapter_name: String,
    pub stem: String,
    pub options: Vec<String>,
    pub answer: String,
    pub explanation: String,
    pub tag_names: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExcelImportAnalysisApi {
    pub source_path: String,
    pub source_file_name: String,
    pub source_file_size: u64,
    pub selected_sheet_name: String,
    pub sheet_names: Vec<String>,
    pub header_row_number: u32,
    pub source_item_count: u32,
    pub omitted_item_count: u32,
    pub formula_cell_count: u32,
    pub rows: Vec<ExcelImportRowApi>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BeginExcelImportResultApi {
    pub analysis: ExcelImportAnalysisApi,
    pub draft: WordImportDraftRecordApi,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExcelImportTemplateResultApi {
    pub output_path: String,
    pub output_filename: String,
    pub output_bytes: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedImagePayloadApi {
    pub resource_id: String,
    pub mime_type: String,
    pub data_base64: String,
    pub width_px: Option<i64>,
    pub height_px: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StoreManagedImageInputApi {
    pub data_base64: String,
    pub original_filename: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StoreManagedImagesRequestApi {
    pub images: Vec<StoreManagedImageInputApi>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadWpsClipboardImagesRequestApi {
    pub paths: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WpsClipboardImagePayloadApi {
    pub path: String,
    pub mime_type: String,
    pub data_base64: String,
    pub width_px: u32,
    pub height_px: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg(test)]
pub struct ExportDocxRequestApi {
    pub template_path: String,
    pub output_path: String,
    /// Optional complete replacement for `word/document.xml`.
    /// No other package part can be replaced through the Tauri command.
    #[serde(default)]
    pub document_xml: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocxExportResultApi {
    pub exported: bool,
    pub template_path: String,
    pub output_path: String,
    pub package_kind: String,
    pub output_bytes: Option<u64>,
    pub input_parts: u32,
    pub raw_copied_parts: u32,
    pub rewritten_parts: Vec<String>,
    pub diagnostics: Vec<DocxDiagnosticApi>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub code: String,
    pub message: String,
}

impl CommandError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn database(error: impl std::fmt::Display) -> Self {
        Self::new("DATABASE_ERROR", format!("本地数据库操作失败：{error}"))
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self::new("VALIDATION_ERROR", message)
    }
}

impl From<LicenseError> for CommandError {
    fn from(error: LicenseError) -> Self {
        Self::new(error.code, error.message)
    }
}

pub type CommandResult<T> = Result<T, CommandError>;
