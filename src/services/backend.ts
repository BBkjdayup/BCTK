import { invoke } from '@tauri-apps/api/core'
import type {
  AppSettings,
  QuestionHistoryEntry, QuestionBatchReceipt, SavedQuestionFilter,
  AutomaticBackupRun,
  BackupCreateResult,
  BackupRecord,
  BeginWordImportResult,
  BeginExcelImportResult,
  BootstrapData,
  StartupProgress,
  DataMovePlan,
  DataMoveResult,
  DataMoveScheduled,
  DocumentEntryTemplate,
  DocumentQuestionDraftPayload,
  DocumentQuestionDraftRecord,
  ExcelImportTemplateResult,
  ExcelWorkbookInspection,
  ManagedImagePayload,
  StoreManagedImageInput,
  PageResult,
  Paper,
  PaperRecovery,
  PaperDeleteRequest,
  PaperDocxExportRequest,
  PaperDocxExportResult,
  PaperFilters,
  PaperSummary,
  Question,
  QuestionSummary,
  QuestionStemSummary,
  QuestionBatchEditRequest,
  QuestionBatchEditResult,
  QuestionDuplicateMergeRequest,
  QuestionDuplicateMergeReceipt,
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

type PreviewBackend = typeof import('./mockBackend').mockBackend

async function previewCall<K extends keyof PreviewBackend>(
  method: K,
  ...args: Parameters<PreviewBackend[K]>
): Promise<Awaited<ReturnType<PreviewBackend[K]>>> {
  if (__BROWSER_PREVIEW__) {
    const { mockBackend } = await import('./mockBackend')
    const handler = mockBackend[method] as (...values: Parameters<PreviewBackend[K]>) => ReturnType<PreviewBackend[K]>
    return await handler.apply(mockBackend, args)
  }
  throw new Error('无法连接软件服务，请重新打开软件。')
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (isDesktopRuntime()) return invoke<T>(command, args)
  throw new Error(`当前浏览器预览不支持命令：${command}`)
}

export const backend = {
  startupProgress(): Promise<StartupProgress | null> {
    return isDesktopRuntime() ? call('get_startup_progress') : Promise.resolve(null)
  },
  checkFormulaCompatibility(formulas: string[]): Promise<(string | null)[] | null> {
    return isTauriRuntime() ? call('check_formula_compatibility', { formulas }) : Promise.resolve(null)
  },
  initialize(): Promise<BootstrapData> {
    return isDesktopRuntime() ? call('app_initialize') : previewCall('initialize')
  },

  retryDatabaseInitialize(): Promise<void> {
    return isTauriRuntime() ? call('retry_database_initialize') : Promise.resolve()
  },

  saveQuestionType(request: SaveQuestionTypeRequest): Promise<QuestionTypeDefinition> {
    return isTauriRuntime()
      ? call('save_question_type', { request })
      : previewCall('saveQuestionType', request)
  },

  deleteQuestionType(code: string): Promise<void> {
    return isTauriRuntime()
      ? call('delete_question_type', { code })
      : previewCall('deleteQuestionType', code)
  },

  saveQuestionTypeOrder(items: TaxonomyOrderItem[]): Promise<void> {
    return isTauriRuntime()
      ? call('save_question_type_order', { items })
      : previewCall('saveQuestionTypeOrder', items)
  },

  listQuestions(filters: QuestionFilters): Promise<PageResult<Question>> {
    return isTauriRuntime() ? call('list_questions', { filters }) : previewCall('listQuestions', filters)
  },
  async listQuestionSummaries(filters: QuestionFilters): Promise<PageResult<QuestionSummary>> {
    if (isTauriRuntime()) return call('list_question_summaries', { filters })
    const page = await previewCall('listQuestions', filters)
    return { ...page, items: page.items.map(q => ({
      id: q.id, type: q.type, stem: { schemaVersion: 1, html: q.stem.html, plainText: q.stem.plainText },
      subjectId: q.subjectId, chapterId: q.chapterId, subjectName: q.subjectName, chapterName: q.chapterName,
      tags: q.tags, lastUsedAt: q.lastUsedAt, deletedAt: q.deletedAt, createdAt: q.createdAt, updatedAt: q.updatedAt, contentVersion: q.contentVersion,
    })) }
  },
  async getQuestions(ids: string[]): Promise<Question[]> {
    if (ids.length > 100 || new Set(ids).size !== ids.length) throw new Error('单次最多读取 100 道不重复的题目')
    if (isTauriRuntime()) return call('get_questions', { ids })
    return Promise.all(ids.map(async id => {
      const question = await previewCall('getQuestion', id)
      if (!question) throw new Error('题目已不存在，请刷新后重试')
      return question
    }))
  },

  analyzeRandomDraw(scope: RandomDrawScope): Promise<RandomDrawAnalysis> {
    return isTauriRuntime()
      ? call('analyze_random_draw', { scope })
      : previewCall('analyzeRandomDraw', scope)
  },

  drawRandomQuestions(request: RandomDrawRequest): Promise<Question[]> {
    return isTauriRuntime()
      ? call('draw_random_questions', { request })
      : previewCall('drawRandomQuestions', request)
  },

  getQuestion(id: string): Promise<Question | null> {
    return isTauriRuntime() ? call('get_question', { id }) : previewCall('getQuestion', id)
  },

  getQuestionStemSummaries(ids: string[]): Promise<QuestionStemSummary[]> {
    return isTauriRuntime()
      ? call('get_question_stem_summaries', { ids })
      : previewCall('getQuestionStemSummaries', ids)
  },

  checkQuestionDuplicate(draft: QuestionDraft): Promise<QuestionDuplicateCheck> {
    return isTauriRuntime()
      ? call('check_question_duplicate', { draft })
      : previewCall('checkQuestionDuplicate', draft)
  },

  checkQuestionDuplicatesBatch(request: QuestionDuplicateBatchRequest): Promise<QuestionDuplicateBatchResult> {
    return isTauriRuntime()
      ? call('check_question_duplicates_batch', { request })
      : previewCall('checkQuestionDuplicatesBatch', request)
  },

  cancelQuestionDuplicateScan(scanId: string): Promise<void> {
    return isTauriRuntime()
      ? call('cancel_question_duplicate_scan', { scanId })
      : previewCall('cancelQuestionDuplicateScan', scanId)
  },

  scanQuestionDuplicates(request: QuestionDuplicateScanRequest, scanId?: string): Promise<QuestionDuplicateScanResult> {
    return isTauriRuntime()
      ? call('scan_question_duplicates', { request, scanId })
      : previewCall('scanQuestionDuplicates', request)
  },

  ignoreQuestionDuplicate(request: IgnoreQuestionDuplicateRequest): Promise<void> {
    return isTauriRuntime()
      ? call('ignore_question_duplicate', { request })
      : previewCall('ignoreQuestionDuplicate', request)
  },

  saveQuestion(draft: QuestionDraft): Promise<Question> {
    return isTauriRuntime() ? call('save_question', { draft }) : previewCall('saveQuestion', draft)
  },

  saveImportedQuestions(drafts: QuestionDraft[]): Promise<string[]> {
    return isTauriRuntime()
      ? call('save_imported_questions', { drafts })
      : previewCall('saveImportedQuestions', drafts)
  },

  saveImportedQuestionOverwrites(
    request: ImportedQuestionOverwriteRequest,
  ): Promise<ImportedQuestionOverwriteResult> {
    return isTauriRuntime()
      ? call('save_imported_question_overwrites', { request })
      : previewCall('saveImportedQuestionOverwrites', request)
  },

  batchEditQuestions(request: QuestionBatchEditRequest): Promise<QuestionBatchEditResult> {
    return isTauriRuntime()
      ? call('batch_edit_questions', { request })
      : previewCall('batchEditQuestions', request)
  },

  listQuestionHistory(questionId:string):Promise<QuestionHistoryEntry[]> { return isTauriRuntime()?call('list_question_history',{questionId}):Promise.resolve([]) },
  getQuestionHistory(id:string):Promise<Question> { return call('get_question_history',{id}) },
  restoreQuestionHistory(id:string,expectedVersion:number):Promise<Question> { return call('restore_question_history',{id,expectedVersion}) },
  listQuestionBatches():Promise<QuestionBatchReceipt[]> { return isTauriRuntime()?call('list_question_batches'):Promise.resolve([]) },
  undoQuestionBatch(id:string):Promise<void> { return call('undo_question_batch',{id}) },
  listSavedQuestionFilters():Promise<SavedQuestionFilter[]> { return isTauriRuntime()?call('list_saved_question_filters'):Promise.resolve([]) },
  saveQuestionFilter(name:string,filters:QuestionFilters):Promise<void> { return call('save_question_filter',{name,filters}) },
  deleteQuestionFilter(id:string):Promise<void> { return call('delete_question_filter',{id}) },

  getQuestionDraft(questionId?: string | null): Promise<QuestionDraftRecord | null> {
    return isTauriRuntime()
      ? call('get_question_draft', { questionId: questionId ?? null })
      : previewCall('getQuestionDraft', questionId)
  },

  saveQuestionDraft(draft: QuestionDraft): Promise<QuestionDraftRecord> {
    return isTauriRuntime()
      ? call('save_question_draft', { draft })
      : previewCall('saveQuestionDraft', draft)
  },

  deleteQuestionDraft(questionId?: string | null): Promise<void> {
    return isTauriRuntime()
      ? call('delete_question_draft', { questionId: questionId ?? null })
      : previewCall('deleteQuestionDraft', questionId)
  },

  getWordImportDraft(): Promise<WordImportDraftRecord | null> {
    return isTauriRuntime() ? call('get_word_import_draft') : previewCall('getWordImportDraft')
  },

  saveWordImportDraft(payload: WordImportDraftPayload): Promise<WordImportDraftRecord> {
    return isTauriRuntime()
      ? call('save_word_import_draft', { payload })
      : previewCall('saveWordImportDraft', payload)
  },

  deleteWordImportDraft(): Promise<void> {
    return isTauriRuntime() ? call('delete_word_import_draft') : previewCall('deleteWordImportDraft')
  },

  mergeQuestionDuplicates(request: QuestionDuplicateMergeRequest): Promise<QuestionDuplicateMergeReceipt> {
    return isTauriRuntime() ? call('merge_question_duplicates', { request }) : previewCall('mergeQuestionDuplicates', request)
  },
  latestQuestionDuplicateMerge(): Promise<QuestionDuplicateMergeReceipt | null> {
    return isTauriRuntime() ? call('latest_question_duplicate_merge') : previewCall('latestQuestionDuplicateMerge')
  },
  undoQuestionDuplicateMerge(id: string): Promise<void> {
    return isTauriRuntime() ? call('undo_question_duplicate_merge', { id }) : previewCall('undoQuestionDuplicateMerge', id)
  },
  moveQuestionsToRecycle(ids: string[]): Promise<void> {
    return isTauriRuntime() ? call('move_questions_to_recycle', { ids }) : previewCall('moveQuestionsToRecycle', ids)
  },

  restoreQuestions(ids: string[]): Promise<void> {
    return isTauriRuntime() ? call('restore_questions', { ids }) : previewCall('restoreQuestions', ids)
  },

  permanentlyDeleteQuestions(ids: string[]): Promise<void> {
    return isTauriRuntime() ? call('permanently_delete_questions', { ids }) : previewCall('permanentlyDeleteQuestions', ids)
  },

  listPapers(filters: PaperFilters): Promise<PageResult<PaperSummary>> {
    return isTauriRuntime() ? call('list_papers', { filters }) : previewCall('listPapers', filters)
  },

  getPaper(id: string): Promise<Paper | null> {
    return isTauriRuntime() ? call('get_paper', { id }) : previewCall('getPaper', id)
  },

  savePaper(paper: Paper): Promise<Paper> {
    return isTauriRuntime() ? call('save_paper', { paper }) : previewCall('savePaper', paper)
  },

  getPaperRecovery(): Promise<PaperRecovery | null> {
    if (isTauriRuntime()) return call('get_paper_recovery')
    return Promise.resolve(JSON.parse(localStorage.getItem('zhitiku-preview-paper-recovery') ?? 'null') as PaperRecovery | null)
  },

  savePaperRecovery(paper: Paper): Promise<PaperRecovery> {
    if (isTauriRuntime()) return call('save_paper_recovery', { paper })
    const record: PaperRecovery = { paper: structuredClone(paper), revision: crypto.randomUUID(), autosavedAt: Date.now() }
    localStorage.setItem('zhitiku-preview-paper-recovery', JSON.stringify(record))
    return Promise.resolve(record)
  },

  async clearPaperRecovery(revision: string): Promise<void> {
    if (isTauriRuntime()) return call('clear_paper_recovery', { revision })
    const record = await this.getPaperRecovery()
    if (record?.revision === revision) localStorage.removeItem('zhitiku-preview-paper-recovery')
  },

  copyPaper(id: string, baseRowVersion: number): Promise<Paper> {
    return isTauriRuntime()
      ? call('copy_paper', { id, baseRowVersion })
      : previewCall('copyPaper', id, baseRowVersion)
  },

  deletePaper(id: string, baseRowVersion: number): Promise<void> {
    return isTauriRuntime()
      ? call('delete_paper', { id, baseRowVersion })
      : previewCall('deletePaper', id, baseRowVersion)
  },

  getDocumentQuestionDraft(): Promise<DocumentQuestionDraftRecord | null> {
    return isTauriRuntime()
      ? call('get_document_question_draft')
      : previewCall('getDocumentQuestionDraft')
  },

  saveDocumentQuestionDraft(payload: DocumentQuestionDraftPayload): Promise<DocumentQuestionDraftRecord> {
    return isTauriRuntime()
      ? call('save_document_question_draft', { payload })
      : previewCall('saveDocumentQuestionDraft', payload)
  },

  deleteDocumentQuestionDraft(): Promise<void> {
    return isTauriRuntime()
      ? call('delete_document_question_draft')
      : previewCall('deleteDocumentQuestionDraft')
  },

  listDocumentEntryTemplates(): Promise<DocumentEntryTemplate[]> {
    return isTauriRuntime()
      ? call('list_document_entry_templates')
      : previewCall('listDocumentEntryTemplates')
  },

  saveDocumentEntryTemplate(request: SaveDocumentEntryTemplateRequest): Promise<DocumentEntryTemplate> {
    return isTauriRuntime()
      ? call('save_document_entry_template', { request })
      : previewCall('saveDocumentEntryTemplate', request)
  },

  setDefaultDocumentEntryTemplate(id: string): Promise<DocumentEntryTemplate> {
    return isTauriRuntime()
      ? call('set_default_document_entry_template', { id })
      : previewCall('setDefaultDocumentEntryTemplate', id)
  },

  deleteDocumentEntryTemplate(id: string): Promise<void> {
    return isTauriRuntime()
      ? call('delete_document_entry_template', { id })
      : previewCall('deleteDocumentEntryTemplate', id)
  },

  deletePapers(requests: PaperDeleteRequest[]): Promise<void> {
    return isTauriRuntime()
      ? call('delete_papers', { requests })
      : previewCall('deletePapers', requests)
  },

  getSettings(): Promise<AppSettings> {
    return isTauriRuntime() ? call('get_settings') : previewCall('getSettings')
  },

  saveSettings(settings: AppSettings): Promise<AppSettings> {
    return isTauriRuntime() ? call('save_settings', { settings }) : previewCall('saveSettings', settings)
  },

  listTemplates(): Promise<WordTemplate[]> {
    return isTauriRuntime() ? call('list_templates') : previewCall('listTemplates')
  },

  importTemplate(sourcePath: string, name?: string): Promise<TemplateImportResult> {
    return isTauriRuntime()
      ? call('import_template', { sourcePath, name })
      : previewCall('importTemplate', sourcePath, name)
  },

  getTemplateConfigurationPreview(id: string): Promise<TemplateConfigurationPreview> {
    return isTauriRuntime()
      ? call('get_template_configuration_preview', { id })
      : previewCall('getTemplateConfigurationPreview', id)
  },

  getTemplateLayoutPreview(id: string): Promise<TemplateLayoutPreview> {
    return isTauriRuntime()
      ? call('get_template_layout_preview', { id })
      : previewCall('getTemplateLayoutPreview', id)
  },

  configureTemplateRegions(request: ConfigureTemplateRegionsRequest): Promise<WordTemplate> {
    return isTauriRuntime()
      ? call('configure_template_regions', { request })
      : previewCall('configureTemplateRegions', request)
  },

  renameTemplate(id: string, name: string, baseRowVersion: number): Promise<WordTemplate> {
    return isTauriRuntime()
      ? call('rename_template', { id, name, baseRowVersion })
      : previewCall('renameTemplate', id, name, baseRowVersion)
  },

  deleteTemplate(id: string, baseRowVersion: number): Promise<void> {
    return isTauriRuntime()
      ? call('delete_template', { id, baseRowVersion })
      : previewCall('deleteTemplate', id, baseRowVersion)
  },

  setDefaultTemplate(id: string, baseRowVersion: number): Promise<void> {
    return isTauriRuntime()
      ? call('set_default_template', { id, baseRowVersion })
      : previewCall('setDefaultTemplate', id, baseRowVersion)
  },

  listBackups(): Promise<BackupRecord[]> {
    return isTauriRuntime() ? call('list_backups') : previewCall('listBackups')
  },

  pickBackupSavePath(): Promise<string | null> {
    return isTauriRuntime() ? call('pick_backup_save_path') : previewCall('pickBackupSavePath')
  },

  createBackup(outputPath: string): Promise<BackupCreateResult> {
    return isTauriRuntime()
      ? call('create_backup', { outputPath })
      : previewCall('createBackup', outputPath)
  },

  runAutomaticBackup(): Promise<AutomaticBackupRun> {
    return isTauriRuntime()
      ? call('run_automatic_backup')
      : previewCall('runAutomaticBackup')
  },

  pickBackupFile(): Promise<string | null> {
    return isTauriRuntime() ? call('pick_backup_file') : previewCall('pickBackupFile')
  },

  prepareRestore(backupPath: string): Promise<RestorePlan> {
    return isTauriRuntime()
      ? call('prepare_restore', { backupPath })
      : previewCall('prepareRestore', backupPath)
  },

  cancelRestore(restoreToken: string): Promise<void> {
    return isTauriRuntime()
      ? call('cancel_restore', { restoreToken })
      : previewCall('cancelRestore', restoreToken)
  },

  scheduleRestore(request: ScheduleRestoreRequest): Promise<RestoreScheduled> {
    return isTauriRuntime()
      ? call('schedule_restore', { request })
      : previewCall('scheduleRestore', request)
  },

  getLastRestoreResult(): Promise<RestoreResult | null> {
    return isTauriRuntime()
      ? call('get_last_restore_result')
      : previewCall('getLastRestoreResult')
  },

  acknowledgeRestoreResult(operationId: string): Promise<void> {
    return isTauriRuntime()
      ? call('acknowledge_restore_result', { operationId })
      : previewCall('acknowledgeRestoreResult', operationId)
  },

  pickDataDirectory(): Promise<string | null> {
    return isTauriRuntime() ? call('pick_data_directory') : previewCall('pickDataDirectory')
  },

  pickExportDirectory(): Promise<string | null> {
    return isTauriRuntime() ? call('pick_export_directory') : previewCall('pickExportDirectory')
  },

  openDataDirectory(component: 'root' | 'database' | 'resources' | 'templates' | 'backup' | 'export'): Promise<void> {
    return isTauriRuntime()
      ? call('open_data_directory', { component })
      : previewCall('openDataDirectory', component)
  },

  prepareDataMove(targetDataRoot: string): Promise<DataMovePlan> {
    return isTauriRuntime()
      ? call('prepare_data_move', { targetDataRoot })
      : previewCall('prepareDataMove', targetDataRoot)
  },

  cancelDataMove(moveToken: string): Promise<void> {
    return isTauriRuntime()
      ? call('cancel_data_move', { moveToken })
      : previewCall('cancelDataMove', moveToken)
  },

  scheduleDataMove(request: ScheduleDataMoveRequest): Promise<DataMoveScheduled> {
    return isTauriRuntime()
      ? call('schedule_data_move', { request })
      : previewCall('scheduleDataMove', request)
  },

  getLastDataMoveResult(): Promise<DataMoveResult | null> {
    return isTauriRuntime()
      ? call('get_last_data_move_result')
      : previewCall('getLastDataMoveResult')
  },

  acknowledgeDataMoveResult(operationId: string): Promise<void> {
    return isTauriRuntime()
      ? call('acknowledge_data_move_result', { operationId })
      : previewCall('acknowledgeDataMoveResult', operationId)
  },

  async createInitialSubject(name: string): Promise<void> {
    if (isTauriRuntime()) {
      await call('create_initial_subject', { name })
      return
    }
    await previewCall('createInitialSubject', name)
  },

  createSubject(name: string): Promise<string> {
    return isTauriRuntime() ? call('create_subject', { name }) : previewCall('createSubject', name)
  },

  updateSubject(id: string, name: string): Promise<void> {
    return isTauriRuntime() ? call('update_subject', { id, name }) : previewCall('updateSubject', id, name)
  },

  deleteSubject(id: string): Promise<void> {
    return isTauriRuntime() ? call('delete_subject', { id }) : previewCall('deleteSubject', id)
  },

  createChapter(subjectId: string, name: string): Promise<string> {
    return isTauriRuntime()
      ? call('create_chapter', { subjectId, name })
      : previewCall('createChapter', subjectId, name)
  },

  updateChapter(id: string, name: string): Promise<void> {
    return isTauriRuntime() ? call('update_chapter', { id, name }) : previewCall('updateChapter', id, name)
  },

  deleteChapter(id: string): Promise<void> {
    return isTauriRuntime() ? call('delete_chapter', { id }) : previewCall('deleteChapter', id)
  },

  createTag(name: string): Promise<string> {
    return isTauriRuntime() ? call('create_tag', { name }) : previewCall('createTag', name)
  },

  updateTag(id: string, name: string): Promise<void> {
    return isTauriRuntime() ? call('update_tag', { id, name }) : previewCall('updateTag', id, name)
  },

  deleteTag(id: string): Promise<void> {
    return isTauriRuntime() ? call('delete_tag', { id }) : previewCall('deleteTag', id)
  },

  saveSubjectOrder(items: TaxonomyOrderItem[]): Promise<void> {
    return isTauriRuntime() ? call('save_subject_order', { items }) : previewCall('saveSubjectOrder', items)
  },

  saveChapterOrder(subjectId: string, items: TaxonomyOrderItem[]): Promise<void> {
    return isTauriRuntime()
      ? call('save_chapter_order', { subjectId, items })
      : previewCall('saveChapterOrder', subjectId, items)
  },

  backgroundTaskProgress(taskId: string): Promise<{ phase: string; completed: number; total: number | null; cancellable: boolean; cancelled: boolean; finished: boolean }> {
    return call('background_task_progress', { taskId })
  },
  cancelBackgroundTask(taskId: string): Promise<void> {
    return call('cancel_background_task', { taskId })
  },
  beginWordImport(inputPath: string, taskId?: string): Promise<BeginWordImportResult> {
    return call('begin_word_import', { request: { inputPath }, taskId })
  },

  inspectExcelWorkbook(inputPath: string): Promise<ExcelWorkbookInspection> {
    return call('inspect_excel_workbook', { inputPath })
  },

  beginExcelImport(inputPath: string, sheetName: string, taskId?: string): Promise<BeginExcelImportResult> {
    return call('begin_excel_import', { request: { inputPath, sheetName }, taskId })
  },

  createExcelImportTemplate(outputPath: string): Promise<ExcelImportTemplateResult> {
    return call('create_excel_import_template', { outputPath })
  },

  getManagedImage(resourceId: string): Promise<ManagedImagePayload> {
    return isTauriRuntime()
      ? call('get_managed_image', { resourceId })
      : previewCall('getManagedImage', resourceId)
  },

  storeManagedImages(images: StoreManagedImageInput[]): Promise<ManagedImagePayload[]> {
    return isTauriRuntime()
      ? call('store_managed_images', { request: { images } })
      : previewCall('storeManagedImages', images)
  },

  readWpsClipboardImages(paths: string[]): Promise<WpsClipboardImagePayload[]> {
    return call('read_wps_clipboard_images', { request: { paths } })
  },

  pickDocxFile(): Promise<string | null> {
    return isTauriRuntime() ? call('pick_docx_file') : previewCall('pickDocxFile')
  },

  pickDocxSavePath(suggestedName: string): Promise<string | null> {
    return call('pick_docx_save_path', { suggestedName })
  },

  pickXlsxFile(): Promise<string | null> {
    return isTauriRuntime() ? call('pick_xlsx_file') : Promise.resolve(null)
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
