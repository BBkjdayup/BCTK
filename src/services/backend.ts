import { invoke } from '@tauri-apps/api/core'
import { mockBackend } from './mockBackend'
import type {
  AppSettings,
  AutomaticBackupRun,
  BackupCreateResult,
  BackupInspection,
  BackupRecord,
  BeginWordImportResult,
  BeginExcelImportResult,
  BootstrapData,
  DataMovePlan,
  DataMoveResult,
  DataMoveScheduled,
  DocumentEntryTemplate,
  DocumentQuestionDraftPayload,
  DocumentQuestionDraftRecord,
  DocxAnalysis,
  ExcelImportTemplateResult,
  ExcelWorkbookInspection,
  ManagedImagePayload,
  StoreManagedImageInput,
  LicenseFileResult,
  LicenseOverview,
  PageResult,
  Paper,
  PaperDeleteRequest,
  PaperDocxExportRequest,
  PaperDocxExportResult,
  PaperFilters,
  PaperSummary,
  Question,
  QuestionBatchEditRequest,
  QuestionBatchEditResult,
  QuestionDraft,
  QuestionDraftRecord,
  QuestionDuplicateBatchRequest,
  QuestionDuplicateBatchResult,
  QuestionDuplicateCheck,
  QuestionDuplicateScanRequest,
  QuestionDuplicateScanResult,
  IgnoreQuestionDuplicateRequest,
  ImportedQuestionOverwriteRequest,
  ImportedQuestionOverwriteResult,
  QuestionFilters,
  RandomDrawAnalysis,
  RandomDrawRequest,
  RandomDrawScope,
  QuestionBankExportFormat,
  QuestionBankExportResult,
  QuestionTypeDefinition,
  RestorePlan,
  RestoreResult,
  RestoreScheduled,
  ScheduleRestoreRequest,
  ScheduleDataMoveRequest,
  SaveDocumentEntryTemplateRequest,
  SaveQuestionTypeRequest,
  TaxonomyOrderItem,
  ConfigureTemplateRegionsRequest,
  CloudAccountStatus,
  CloudLoginRequest,
  CloudRegisterRequest,
  CloudSyncResult,
  CloudSyncPreflight,
  CloudSyncConflict,
  ResolveCloudSyncConflictRequest,
  TemplateConfigurationPreview,
  TemplateLayoutPreview,
  TemplateImportResult,
  WordTemplate,
  WordImportDraftPayload,
  WordImportDraftRecord,
  WpsClipboardImagePayload,
} from '../types/domain'

export const isDesktopRuntime = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
const isTauriRuntime = isDesktopRuntime

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (isDesktopRuntime()) return invoke<T>(command, args)
  throw new Error(`当前浏览器预览不支持命令：${command}`)
}

export const backend = {
  initialize(): Promise<BootstrapData> {
    return isDesktopRuntime() ? call('app_initialize') : mockBackend.initialize()
  },

  retryDatabaseInitialize(): Promise<void> {
    return isTauriRuntime() ? call('retry_database_initialize') : Promise.resolve()
  },

  listQuestionTypes(): Promise<QuestionTypeDefinition[]> {
    return isTauriRuntime() ? call('list_question_types') : mockBackend.listQuestionTypes()
  },

  saveQuestionType(request: SaveQuestionTypeRequest): Promise<QuestionTypeDefinition> {
    return isTauriRuntime()
      ? call('save_question_type', { request })
      : mockBackend.saveQuestionType(request)
  },

  deleteQuestionType(code: string): Promise<void> {
    return isTauriRuntime()
      ? call('delete_question_type', { code })
      : mockBackend.deleteQuestionType(code)
  },

  saveQuestionTypeOrder(items: TaxonomyOrderItem[]): Promise<void> {
    return isTauriRuntime()
      ? call('save_question_type_order', { items })
      : mockBackend.saveQuestionTypeOrder(items)
  },

  listQuestions(filters: QuestionFilters): Promise<PageResult<Question>> {
    return isTauriRuntime() ? call('list_questions', { filters }) : mockBackend.listQuestions(filters)
  },

  analyzeRandomDraw(scope: RandomDrawScope): Promise<RandomDrawAnalysis> {
    return isTauriRuntime()
      ? call('analyze_random_draw', { scope })
      : mockBackend.analyzeRandomDraw(scope)
  },

  drawRandomQuestions(request: RandomDrawRequest): Promise<Question[]> {
    return isTauriRuntime()
      ? call('draw_random_questions', { request })
      : mockBackend.drawRandomQuestions(request)
  },

  getQuestion(id: string): Promise<Question | null> {
    return isTauriRuntime() ? call('get_question', { id }) : mockBackend.getQuestion(id)
  },

  checkQuestionDuplicate(draft: QuestionDraft): Promise<QuestionDuplicateCheck> {
    return isTauriRuntime()
      ? call('check_question_duplicate', { draft })
      : mockBackend.checkQuestionDuplicate(draft)
  },

  checkQuestionDuplicatesBatch(request: QuestionDuplicateBatchRequest): Promise<QuestionDuplicateBatchResult> {
    return isTauriRuntime()
      ? call('check_question_duplicates_batch', { request })
      : mockBackend.checkQuestionDuplicatesBatch(request)
  },

  cancelQuestionDuplicateScan(scanId: string): Promise<void> {
    return isTauriRuntime()
      ? call('cancel_question_duplicate_scan', { scanId })
      : mockBackend.cancelQuestionDuplicateScan(scanId)
  },

  scanQuestionDuplicates(request: QuestionDuplicateScanRequest): Promise<QuestionDuplicateScanResult> {
    return isTauriRuntime()
      ? call('scan_question_duplicates', { request })
      : mockBackend.scanQuestionDuplicates(request)
  },

  ignoreQuestionDuplicate(request: IgnoreQuestionDuplicateRequest): Promise<void> {
    return isTauriRuntime()
      ? call('ignore_question_duplicate', { request })
      : mockBackend.ignoreQuestionDuplicate(request)
  },

  saveQuestion(draft: QuestionDraft): Promise<Question> {
    return isTauriRuntime() ? call('save_question', { draft }) : mockBackend.saveQuestion(draft)
  },

  saveQuestions(drafts: QuestionDraft[]): Promise<Question[]> {
    return isTauriRuntime() ? call('save_questions', { drafts }) : mockBackend.saveQuestions(drafts)
  },

  saveImportedQuestions(drafts: QuestionDraft[]): Promise<string[]> {
    return isTauriRuntime()
      ? call('save_imported_questions', { drafts })
      : mockBackend.saveImportedQuestions(drafts)
  },

  saveImportedQuestionOverwrites(
    request: ImportedQuestionOverwriteRequest,
  ): Promise<ImportedQuestionOverwriteResult> {
    return isTauriRuntime()
      ? call('save_imported_question_overwrites', { request })
      : mockBackend.saveImportedQuestionOverwrites(request)
  },

  batchEditQuestions(request: QuestionBatchEditRequest): Promise<QuestionBatchEditResult> {
    return isTauriRuntime()
      ? call('batch_edit_questions', { request })
      : mockBackend.batchEditQuestions(request)
  },

  getQuestionDraft(questionId?: string | null): Promise<QuestionDraftRecord | null> {
    return isTauriRuntime()
      ? call('get_question_draft', { questionId: questionId ?? null })
      : mockBackend.getQuestionDraft(questionId)
  },

  listQuestionDrafts(): Promise<QuestionDraftRecord[]> {
    return isTauriRuntime() ? call('list_question_drafts') : mockBackend.listQuestionDrafts()
  },

  saveQuestionDraft(draft: QuestionDraft): Promise<QuestionDraftRecord> {
    return isTauriRuntime()
      ? call('save_question_draft', { draft })
      : mockBackend.saveQuestionDraft(draft)
  },

  deleteQuestionDraft(questionId?: string | null): Promise<void> {
    return isTauriRuntime()
      ? call('delete_question_draft', { questionId: questionId ?? null })
      : mockBackend.deleteQuestionDraft(questionId)
  },

  getWordImportDraft(): Promise<WordImportDraftRecord | null> {
    return isTauriRuntime() ? call('get_word_import_draft') : mockBackend.getWordImportDraft()
  },

  saveWordImportDraft(payload: WordImportDraftPayload): Promise<WordImportDraftRecord> {
    return isTauriRuntime()
      ? call('save_word_import_draft', { payload })
      : mockBackend.saveWordImportDraft(payload)
  },

  deleteWordImportDraft(): Promise<void> {
    return isTauriRuntime() ? call('delete_word_import_draft') : mockBackend.deleteWordImportDraft()
  },

  moveQuestionsToRecycle(ids: string[]): Promise<void> {
    return isTauriRuntime() ? call('move_questions_to_recycle', { ids }) : mockBackend.moveQuestionsToRecycle(ids)
  },

  restoreQuestions(ids: string[]): Promise<void> {
    return isTauriRuntime() ? call('restore_questions', { ids }) : mockBackend.restoreQuestions(ids)
  },

  permanentlyDeleteQuestions(ids: string[]): Promise<void> {
    return isTauriRuntime() ? call('permanently_delete_questions', { ids }) : mockBackend.permanentlyDeleteQuestions(ids)
  },

  listPapers(filters: PaperFilters): Promise<PageResult<PaperSummary>> {
    return isTauriRuntime() ? call('list_papers', { filters }) : mockBackend.listPapers(filters)
  },

  getPaper(id: string): Promise<Paper | null> {
    return isTauriRuntime() ? call('get_paper', { id }) : mockBackend.getPaper(id)
  },

  savePaper(paper: Paper): Promise<Paper> {
    return isTauriRuntime() ? call('save_paper', { paper }) : mockBackend.savePaper(paper)
  },

  copyPaper(id: string, baseRowVersion: number): Promise<Paper> {
    return isTauriRuntime()
      ? call('copy_paper', { id, baseRowVersion })
      : mockBackend.copyPaper(id, baseRowVersion)
  },

  deletePaper(id: string, baseRowVersion: number): Promise<void> {
    return isTauriRuntime()
      ? call('delete_paper', { id, baseRowVersion })
      : mockBackend.deletePaper(id, baseRowVersion)
  },

  getDocumentQuestionDraft(): Promise<DocumentQuestionDraftRecord | null> {
    return isTauriRuntime()
      ? call('get_document_question_draft')
      : mockBackend.getDocumentQuestionDraft()
  },

  saveDocumentQuestionDraft(payload: DocumentQuestionDraftPayload): Promise<DocumentQuestionDraftRecord> {
    return isTauriRuntime()
      ? call('save_document_question_draft', { payload })
      : mockBackend.saveDocumentQuestionDraft(payload)
  },

  deleteDocumentQuestionDraft(): Promise<void> {
    return isTauriRuntime()
      ? call('delete_document_question_draft')
      : mockBackend.deleteDocumentQuestionDraft()
  },

  listDocumentEntryTemplates(): Promise<DocumentEntryTemplate[]> {
    return isTauriRuntime()
      ? call('list_document_entry_templates')
      : mockBackend.listDocumentEntryTemplates()
  },

  saveDocumentEntryTemplate(request: SaveDocumentEntryTemplateRequest): Promise<DocumentEntryTemplate> {
    return isTauriRuntime()
      ? call('save_document_entry_template', { request })
      : mockBackend.saveDocumentEntryTemplate(request)
  },

  setDefaultDocumentEntryTemplate(id: string): Promise<DocumentEntryTemplate> {
    return isTauriRuntime()
      ? call('set_default_document_entry_template', { id })
      : mockBackend.setDefaultDocumentEntryTemplate(id)
  },

  deleteDocumentEntryTemplate(id: string): Promise<void> {
    return isTauriRuntime()
      ? call('delete_document_entry_template', { id })
      : mockBackend.deleteDocumentEntryTemplate(id)
  },

  deletePapers(requests: PaperDeleteRequest[]): Promise<void> {
    return isTauriRuntime()
      ? call('delete_papers', { requests })
      : mockBackend.deletePapers(requests)
  },

  getSettings(): Promise<AppSettings> {
    return isTauriRuntime() ? call('get_settings') : mockBackend.getSettings()
  },

  saveSettings(settings: AppSettings): Promise<AppSettings> {
    return isTauriRuntime() ? call('save_settings', { settings }) : mockBackend.saveSettings(settings)
  },

  listTemplates(): Promise<WordTemplate[]> {
    return isTauriRuntime() ? call('list_templates') : mockBackend.listTemplates()
  },

  importTemplate(sourcePath: string, name?: string): Promise<TemplateImportResult> {
    return isTauriRuntime()
      ? call('import_template', { sourcePath, name })
      : mockBackend.importTemplate(sourcePath, name)
  },

  getTemplateConfigurationPreview(id: string): Promise<TemplateConfigurationPreview> {
    return isTauriRuntime()
      ? call('get_template_configuration_preview', { id })
      : mockBackend.getTemplateConfigurationPreview(id)
  },

  getTemplateLayoutPreview(id: string): Promise<TemplateLayoutPreview> {
    return isTauriRuntime()
      ? call('get_template_layout_preview', { id })
      : mockBackend.getTemplateLayoutPreview(id)
  },

  configureTemplateRegions(request: ConfigureTemplateRegionsRequest): Promise<WordTemplate> {
    return isTauriRuntime()
      ? call('configure_template_regions', { request })
      : mockBackend.configureTemplateRegions(request)
  },

  renameTemplate(id: string, name: string, baseRowVersion: number): Promise<WordTemplate> {
    return isTauriRuntime()
      ? call('rename_template', { id, name, baseRowVersion })
      : mockBackend.renameTemplate(id, name, baseRowVersion)
  },

  deleteTemplate(id: string, baseRowVersion: number): Promise<void> {
    return isTauriRuntime()
      ? call('delete_template', { id, baseRowVersion })
      : mockBackend.deleteTemplate(id, baseRowVersion)
  },

  setDefaultTemplate(id: string, baseRowVersion: number): Promise<void> {
    return isTauriRuntime()
      ? call('set_default_template', { id, baseRowVersion })
      : mockBackend.setDefaultTemplate(id, baseRowVersion)
  },

  listBackups(): Promise<BackupRecord[]> {
    return isTauriRuntime() ? call('list_backups') : mockBackend.listBackups()
  },

  pickBackupSavePath(): Promise<string | null> {
    return isTauriRuntime() ? call('pick_backup_save_path') : mockBackend.pickBackupSavePath()
  },

  createBackup(outputPath: string): Promise<BackupCreateResult> {
    return isTauriRuntime()
      ? call('create_backup', { outputPath })
      : mockBackend.createBackup(outputPath)
  },

  saveImportedQuestion(draft: QuestionDraft): Promise<Question> {
    return isTauriRuntime() ? call('save_imported_question', { draft }) : mockBackend.saveImportedQuestion(draft)
  },

  runAutomaticBackup(): Promise<AutomaticBackupRun> {
    return isTauriRuntime()
      ? call('run_automatic_backup')
      : mockBackend.runAutomaticBackup()
  },

  pickBackupFile(): Promise<string | null> {
    return isTauriRuntime() ? call('pick_backup_file') : mockBackend.pickBackupFile()
  },

  inspectBackup(backupPath: string): Promise<BackupInspection> {
    return isTauriRuntime()
      ? call('inspect_backup', { backupPath })
      : mockBackend.inspectBackup(backupPath)
  },

  prepareRestore(backupPath: string): Promise<RestorePlan> {
    return isTauriRuntime()
      ? call('prepare_restore', { backupPath })
      : mockBackend.prepareRestore(backupPath)
  },

  cancelRestore(restoreToken: string): Promise<void> {
    return isTauriRuntime()
      ? call('cancel_restore', { restoreToken })
      : mockBackend.cancelRestore(restoreToken)
  },

  scheduleRestore(request: ScheduleRestoreRequest): Promise<RestoreScheduled> {
    return isTauriRuntime()
      ? call('schedule_restore', { request })
      : mockBackend.scheduleRestore(request)
  },

  getLastRestoreResult(): Promise<RestoreResult | null> {
    return isTauriRuntime()
      ? call('get_last_restore_result')
      : mockBackend.getLastRestoreResult()
  },

  acknowledgeRestoreResult(operationId: string): Promise<void> {
    return isTauriRuntime()
      ? call('acknowledge_restore_result', { operationId })
      : mockBackend.acknowledgeRestoreResult(operationId)
  },

  pickDataDirectory(): Promise<string | null> {
    return isTauriRuntime() ? call('pick_data_directory') : mockBackend.pickDataDirectory()
  },

  pickExportDirectory(): Promise<string | null> {
    return isTauriRuntime() ? call('pick_export_directory') : mockBackend.pickExportDirectory()
  },

  openDataDirectory(component: 'root' | 'database' | 'resources' | 'templates' | 'backup' | 'export'): Promise<void> {
    return isTauriRuntime()
      ? call('open_data_directory', { component })
      : mockBackend.openDataDirectory(component)
  },

  prepareDataMove(targetDataRoot: string): Promise<DataMovePlan> {
    return isTauriRuntime()
      ? call('prepare_data_move', { targetDataRoot })
      : mockBackend.prepareDataMove(targetDataRoot)
  },

  cancelDataMove(moveToken: string): Promise<void> {
    return isTauriRuntime()
      ? call('cancel_data_move', { moveToken })
      : mockBackend.cancelDataMove(moveToken)
  },

  scheduleDataMove(request: ScheduleDataMoveRequest): Promise<DataMoveScheduled> {
    return isTauriRuntime()
      ? call('schedule_data_move', { request })
      : mockBackend.scheduleDataMove(request)
  },

  getLastDataMoveResult(): Promise<DataMoveResult | null> {
    return isTauriRuntime()
      ? call('get_last_data_move_result')
      : mockBackend.getLastDataMoveResult()
  },

  acknowledgeDataMoveResult(operationId: string): Promise<void> {
    return isTauriRuntime()
      ? call('acknowledge_data_move_result', { operationId })
      : mockBackend.acknowledgeDataMoveResult(operationId)
  },

  async createInitialSubject(name: string): Promise<void> {
    if (isTauriRuntime()) {
      await call('create_initial_subject', { name })
      return
    }
    await mockBackend.createInitialSubject(name)
  },

  createSubject(name: string): Promise<string> {
    return isTauriRuntime() ? call('create_subject', { name }) : mockBackend.createSubject(name)
  },

  updateSubject(id: string, name: string): Promise<void> {
    return isTauriRuntime() ? call('update_subject', { id, name }) : mockBackend.updateSubject(id, name)
  },

  deleteSubject(id: string): Promise<void> {
    return isTauriRuntime() ? call('delete_subject', { id }) : mockBackend.deleteSubject(id)
  },

  createChapter(subjectId: string, name: string): Promise<string> {
    return isTauriRuntime()
      ? call('create_chapter', { subjectId, name })
      : mockBackend.createChapter(subjectId, name)
  },

  updateChapter(id: string, name: string): Promise<void> {
    return isTauriRuntime() ? call('update_chapter', { id, name }) : mockBackend.updateChapter(id, name)
  },

  deleteChapter(id: string): Promise<void> {
    return isTauriRuntime() ? call('delete_chapter', { id }) : mockBackend.deleteChapter(id)
  },

  createTag(name: string): Promise<string> {
    return isTauriRuntime() ? call('create_tag', { name }) : mockBackend.createTag(name)
  },

  updateTag(id: string, name: string): Promise<void> {
    return isTauriRuntime() ? call('update_tag', { id, name }) : mockBackend.updateTag(id, name)
  },

  deleteTag(id: string): Promise<void> {
    return isTauriRuntime() ? call('delete_tag', { id }) : mockBackend.deleteTag(id)
  },

  saveSubjectOrder(items: TaxonomyOrderItem[]): Promise<void> {
    return isTauriRuntime() ? call('save_subject_order', { items }) : mockBackend.saveSubjectOrder(items)
  },

  saveChapterOrder(subjectId: string, items: TaxonomyOrderItem[]): Promise<void> {
    return isTauriRuntime()
      ? call('save_chapter_order', { subjectId, items })
      : mockBackend.saveChapterOrder(subjectId, items)
  },

  analyzeDocx(inputPath: string): Promise<DocxAnalysis> {
    return call('analyze_docx', { request: { inputPath } })
  },

  beginWordImport(inputPath: string): Promise<BeginWordImportResult> {
    return call('begin_word_import', { request: { inputPath } })
  },

  inspectExcelWorkbook(inputPath: string): Promise<ExcelWorkbookInspection> {
    return call('inspect_excel_workbook', { inputPath })
  },

  beginExcelImport(inputPath: string, sheetName: string): Promise<BeginExcelImportResult> {
    return call('begin_excel_import', { request: { inputPath, sheetName } })
  },

  createExcelImportTemplate(outputPath: string): Promise<ExcelImportTemplateResult> {
    return call('create_excel_import_template', { outputPath })
  },

  getManagedImage(resourceId: string): Promise<ManagedImagePayload> {
    return isTauriRuntime()
      ? call('get_managed_image', { resourceId })
      : mockBackend.getManagedImage(resourceId)
  },

  storeManagedImages(images: StoreManagedImageInput[]): Promise<ManagedImagePayload[]> {
    return isTauriRuntime()
      ? call('store_managed_images', { request: { images } })
      : mockBackend.storeManagedImages(images)
  },

  readWpsClipboardImages(paths: string[]): Promise<WpsClipboardImagePayload[]> {
    return call('read_wps_clipboard_images', { request: { paths } })
  },

  pickDocxFile(): Promise<string | null> {
    return isTauriRuntime() ? call('pick_docx_file') : mockBackend.pickDocxFile()
  },

  pickDocxSavePath(suggestedName: string): Promise<string | null> {
    return call('pick_docx_save_path', { suggestedName })
  },

  pickXlsxFile(): Promise<string | null> {
    return isTauriRuntime() ? call('pick_xlsx_file') : Promise.resolve(null)
  },

  getLicenseOverview(): Promise<LicenseOverview> {
    return isTauriRuntime() ? call('get_license_overview') : mockBackend.getLicenseOverview()
  },

  getCloudAccountStatus(): Promise<CloudAccountStatus> {
    return isTauriRuntime()
      ? call('get_cloud_account_status')
      : Promise.resolve({
          configured: false,
          apiBaseUrl: '',
          loggedIn: false,
          user: null,
          entitlement: null,
          canSync: false,
          databaseBound: false,
          lastSyncAtMs: null,
          conflictCount: 0,
          message: '浏览器演示不连接云服务。',
        })
  },

  setCloudApiUrl(apiBaseUrl: string): Promise<CloudAccountStatus> {
    return isTauriRuntime()
      ? call('set_cloud_api_url', { apiBaseUrl })
      : this.getCloudAccountStatus()
  },

  cloudLogin(request: CloudLoginRequest): Promise<CloudAccountStatus> {
    return isTauriRuntime()
      ? call('cloud_login', { request })
      : this.getCloudAccountStatus()
  },

  cloudRegister(request: CloudRegisterRequest): Promise<CloudAccountStatus> {
    return isTauriRuntime()
      ? call('cloud_register', { request })
      : this.getCloudAccountStatus()
  },

  cloudLogout(): Promise<CloudAccountStatus> {
    return isTauriRuntime()
      ? call('cloud_logout')
      : this.getCloudAccountStatus()
  },

  cloudSyncNow(): Promise<CloudSyncResult> {
    return isTauriRuntime()
      ? call('cloud_sync_now')
      : Promise.reject(new Error('浏览器演示不支持云同步'))
  },

  getCloudSyncPreflight(): Promise<CloudSyncPreflight> {
    return isTauriRuntime()
      ? call('get_cloud_sync_preflight')
      : Promise.resolve({
          localEntityCount: 0,
          localQuestionCount: 0,
          cloudEntityCount: 0,
          cloudQuestionCount: 0,
          localHasData: false,
          cloudHasData: false,
          bothNonEmpty: false,
          recommendedMode: 'merge',
        })
  },

  listCloudSyncConflicts(): Promise<CloudSyncConflict[]> {
    return isTauriRuntime()
      ? call('list_cloud_sync_conflicts')
      : Promise.resolve([])
  },

  resolveCloudSyncConflict(request: ResolveCloudSyncConflictRequest): Promise<CloudSyncConflict[]> {
    return isTauriRuntime()
      ? call('resolve_cloud_sync_conflict', { request })
      : Promise.resolve([])
  },

  pickLicenseRequestSavePath(suggestedName: string): Promise<string | null> {
    return isTauriRuntime() ? call('pick_license_request_save_path', { suggestedName }) : Promise.resolve(null)
  },

  exportLicenseRequest(outputPath: string): Promise<LicenseFileResult> {
    return isTauriRuntime() ? call('export_license_request', { outputPath }) : mockBackend.exportLicenseRequest(outputPath)
  },

  pickDesktopLicenseFile(): Promise<string | null> {
    return isTauriRuntime() ? call('pick_desktop_license_file') : Promise.resolve(null)
  },

  importDesktopLicense(inputPath: string): Promise<LicenseOverview> {
    return isTauriRuntime() ? call('import_desktop_license', { inputPath }) : mockBackend.importDesktopLicense(inputPath)
  },

  removeDesktopLicense(): Promise<LicenseOverview> {
    return isTauriRuntime() ? call('remove_desktop_license') : mockBackend.removeDesktopLicense()
  },

  checkPrintAuthorization(): Promise<void> {
    return isTauriRuntime() ? call('check_print_authorization') : Promise.resolve()
  },

  pickXlsxSavePath(suggestedName: string): Promise<string | null> {
    return call('pick_xlsx_save_path', { suggestedName })
  },

  exportQuestionBank(format: QuestionBankExportFormat, outputPath: string): Promise<QuestionBankExportResult> {
    return call('export_question_bank', { format, outputPath })
  },

  exportPaperDocx(request: PaperDocxExportRequest): Promise<PaperDocxExportResult> {
    return call('export_paper_docx', { request })
  },

  openExportedFile(path: string): Promise<void> {
    return call('open_exported_file', { path })
  },

  revealExportedFile(path: string): Promise<void> {
    return call('reveal_exported_file', { path })
  },
}
