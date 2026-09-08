import type {
  AppSettings,
  AutomaticBackupRun,
  BackupCreateResult,
  BackupInspection,
  BackupRecord,
  ManagedImagePayload,
  StoreManagedImageInput,
  LicenseFileResult,
  LicenseOverview,
  BootstrapData,
  Chapter,
  DocxDiagnostic,
  DataMovePlan,
  DataMoveResult,
  DataMoveScheduled,
  DocumentEntryTemplate,
  DocumentQuestionDraftPayload,
  DocumentQuestionDraftRecord,
  PageResult,
  Paper,
  PaperDeleteRequest,
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
  QuestionDuplicateCandidate,
  QuestionDuplicateGroup,
  QuestionDuplicateMember,
  QuestionDuplicateScanRequest,
  QuestionDuplicateScanResult,
  IgnoreQuestionDuplicateRequest,
  ImportedQuestionOverwriteRequest,
  ImportedQuestionOverwriteResult,
  QuestionFilters,
  RandomDrawAnalysis,
  RandomDrawRequest,
  RandomDrawScope,
  QuestionTypeDefinition,
  RichContent,
  RestorePlan,
  RestoreResult,
  RestoreScheduled,
  ScheduleDataMoveRequest,
  ScheduleRestoreRequest,
  SaveDocumentEntryTemplateRequest,
  SaveQuestionTypeRequest,
  Subject,
  Tag,
  TaxonomyOrderItem,
  ConfigureTemplateRegionsRequest,
  TemplateConfigurationPreview,
  TemplateImportResult,
  TemplateLayoutPreview,
  WordImportDraftPayload,
  WordImportDraftRecord,
  WordTemplate,
} from '../types/domain'
import { clonePlain } from '../utils/clonePlain'
import {
  normalizeDocumentEntryTemplateConfig,
  systemDocumentEntryTemplate,
  validateDocumentEntryTemplateConfig,
} from '../utils/documentEntryTemplates'
import { fallbackQuestionTypes, matchesChoiceBehavior } from '../utils/questionTypes'
import { matchesQuestionUsage } from '../utils/questionUsage'

const now = Date.now()

const mockLicense: LicenseOverview = {
  deviceId: 'browser-demo-device',
  desktop: {
    state: 'active',
    plan: 'desktop_professional',
    licenseId: 'browser-demo',
    customerName: '浏览器演示',
    issuedAtMs: now,
    expiresAtMs: null,
    graceEndsAtMs: null,
    message: '浏览器演示使用完整功能，不代表真实授权状态。',
  },
  cloud: {
    state: 'notConfigured',
    syncEnabled: false,
    webAppEnabled: false,
    expiresAtMs: null,
    message: '云同步与网页版尚未接入。',
  },
  capabilities: {
    canEditSingleQuestion: true,
    canBatchImport: true,
    maxQuestionsPerPaper: null,
    canExportDocuments: true,
    canPrint: true,
    canUseCloudSync: false,
    canUseWebApp: false,
  },
}

const rich = (html: string): RichContent => ({
  schemaVersion: 1,
  html,
  plainText: html.replace(/<[^>]*>/g, '').replace(/&nbsp;/g, ' ').trim(),
})

let tags: Tag[] = [
  { id: 'tag-foundation', name: '基础概念', questionCount: 2, createdAt: now, updatedAt: now },
  { id: 'tag-exam', name: '常考题', questionCount: 2, createdAt: now, updatedAt: now },
  { id: 'tag-thinking', name: '思考题', questionCount: 1, createdAt: now, updatedAt: now },
]

const chapters: Chapter[] = [
  { id: 'chapter-edu-1', subjectId: 'subject-education', name: '第一章 教育与教育学', sortOrder: 1024, questionCount: 2 },
  { id: 'chapter-edu-2', subjectId: 'subject-education', name: '第二章 教育目的', sortOrder: 2048, questionCount: 1 },
  { id: 'chapter-psy-1', subjectId: 'subject-psychology', name: '第一章 心理学概述', sortOrder: 1024, questionCount: 1 },
]

let subjects: Subject[] = [
  {
    id: 'subject-education',
    name: '教育学',
    sortOrder: 1024,
    questionCount: 3,
    lastAccessedAt: now,
    chapters: chapters.filter((chapter) => chapter.subjectId === 'subject-education'),
  },
  {
    id: 'subject-psychology',
    name: '教育心理学',
    sortOrder: 2048,
    questionCount: 1,
    chapters: chapters.filter((chapter) => chapter.subjectId === 'subject-psychology'),
  },
]

let questionTypes: QuestionTypeDefinition[] = clonePlain(fallbackQuestionTypes)

let questions: Question[] = [
  {
    id: 'question-1',
    type: 'single_choice',
    stem: rich('<p>教育的本质属性是什么？</p>'),
    options: [
      { id: 'q1-a', position: 0, content: rich('<p>有目的地培养人的社会活动</p>') },
      { id: 'q1-b', position: 1, content: rich('<p>传递知识的单向活动</p>') },
      { id: 'q1-c', position: 2, content: rich('<p>促进经济发展的活动</p>') },
      { id: 'q1-d', position: 3, content: rich('<p>个体自发成长的过程</p>') },
    ],
    answer: rich('<p>A</p>'),
    explanation: rich('<p>教育是有目的地培养人的社会活动，这是教育区别于其他社会现象的根本特征。</p>'),
    subjectId: 'subject-education',
    chapterId: 'chapter-edu-1',
    subjectName: '教育学',
    chapterName: '第一章 教育与教育学',
    tags: [tags[0], tags[1]],
    createdAt: now - 7 * 86400000,
    updatedAt: now - 2 * 86400000,
    lastUsedAt: now - 86400000,
    contentVersion: 1,
  },
  {
    id: 'question-2',
    type: 'multiple_choice',
    stem: rich('<p>下列属于学校教育基本要素的有（ ）。</p>'),
    options: [
      { id: 'q2-a', position: 0, content: rich('<p>教育者</p>') },
      { id: 'q2-b', position: 1, content: rich('<p>受教育者</p>') },
      { id: 'q2-c', position: 2, content: rich('<p>教育影响</p>') },
      { id: 'q2-d', position: 3, content: rich('<p>社会环境</p>') },
    ],
    answer: rich('<p>A、B、C</p>'),
    explanation: rich('<p>教育者、受教育者和教育影响构成教育活动的三个基本要素。</p>'),
    subjectId: 'subject-education',
    chapterId: 'chapter-edu-1',
    subjectName: '教育学',
    chapterName: '第一章 教育与教育学',
    tags: [tags[0]],
    createdAt: now - 6 * 86400000,
    updatedAt: now - 86400000,
    lastUsedAt: null,
    contentVersion: 1,
  },
  {
    id: 'question-3',
    type: 'short_answer',
    stem: rich('<p>简述我国现阶段教育目的的基本精神。</p>'),
    options: [],
    answer: rich('<p>坚持社会主义方向；培养全面发展的人；强调教育与生产劳动相结合；注重创新精神与实践能力。</p>'),
    explanation: rich('<p>回答时应围绕方向、全面发展、实践与创新四个方面展开。</p>'),
    subjectId: 'subject-education',
    chapterId: 'chapter-edu-2',
    subjectName: '教育学',
    chapterName: '第二章 教育目的',
    tags: [tags[1], tags[2]],
    createdAt: now - 3 * 86400000,
    updatedAt: now - 3 * 3600000,
    lastUsedAt: null,
    contentVersion: 1,
  },
  {
    id: 'question-4',
    type: 'fill_blank',
    stem: rich('<p>心理现象包括心理过程和____。</p>'),
    options: [],
    answer: rich('<p>个性心理</p>'),
    explanation: rich('<p>心理现象通常分为心理过程和个性心理两个方面。</p>'),
    subjectId: 'subject-psychology',
    chapterId: 'chapter-psy-1',
    subjectName: '教育心理学',
    chapterName: '第一章 心理学概述',
    tags: [],
    createdAt: now - 86400000,
    updatedAt: now - 3600000,
    lastUsedAt: null,
    contentVersion: 1,
  },
]

const defaultSettings: AppSettings = {
  defaultExportDirectory: 'C:\\Users\\Teacher\\Documents\\教师题库\\export',
  defaultTemplateId: null,
  exportFilenamePattern: '{title}_{date}',
  optionalConfirmations: true,
  recentUnusedDays: 90,
  recentAddedDays: 30,
  recentUsedDays: 30,
  recycleRetentionDays: 30,
  recyclePolicy: 'remind_only',
  automaticBackupEnabled: true,
  automaticBackupIntervalDays: 7,
  automaticBackupRetentionCount: 5,
}

let settings: AppSettings = clonePlain(defaultSettings)

let papers: Paper[] = []
let templates: WordTemplate[] = []
let backups: BackupRecord[] = []
let questionDrafts: QuestionDraftRecord[] = []
let documentQuestionDraft: DocumentQuestionDraftRecord | null = null
let documentEntryTemplates: DocumentEntryTemplate[] = [systemDocumentEntryTemplate()]
let wordImportDraft: WordImportDraftRecord | null = null
let duplicateIgnores: Array<{
  lowId: string
  lowVersion: number
  highId: string
  highVersion: number
}> = []
const importedOverwriteReceipts = new Map<string, {
  requestHash: string
  result: ImportedQuestionOverwriteResult
}>()
let preparedRestore: { path: string; plan: RestorePlan } | null = null
let lastRestoreResult: RestoreResult | null = null
let preparedDataMove: DataMovePlan | null = null
let lastDataMoveResult: DataMoveResult | null = null
let mockDataRoot = 'C:\\Users\\Teacher\\Documents\\教师题库'
const managedImages = new Map<string, ManagedImagePayload>()
const managedImageIdsByContent = new Map<string, string>()

const WORD_IMPORT_DRAFT_STORAGE_KEY = 'zhitiku-preview-word-import-draft'
const DOCUMENT_QUESTION_DRAFT_STORAGE_KEY = 'zhitiku-preview-document-question-draft'
const DOCUMENT_ENTRY_TEMPLATES_STORAGE_KEY = 'zhitiku-preview-document-entry-templates'
const RESTORE_RESULT_STORAGE_KEY = 'zhitiku-preview-last-restore-result'
const DATA_MOVE_RESULT_STORAGE_KEY = 'zhitiku-preview-last-data-move-result'
const DATA_ROOT_STORAGE_KEY = 'zhitiku-preview-data-root'
const DUPLICATE_IGNORES_STORAGE_KEY = 'zhitiku-preview-duplicate-ignores'
const MAX_WORD_IMPORT_PAYLOAD_BYTES = 64 * 1024 * 1024
const MAX_WORD_IMPORT_SOURCE_BYTES = 100 * 1024 * 1024
const MAX_WORD_IMPORT_ITEMS = 5_000
const MAX_WORD_IMPORT_SOURCE_NAME_CHARS = 255
const MAX_WORD_IMPORT_PARSER_VERSION_CHARS = 64

const persist = () => {
  if (typeof localStorage === 'undefined') return
  localStorage.setItem('zhitiku-preview-questions', JSON.stringify(questions))
  localStorage.setItem('zhitiku-preview-settings', JSON.stringify(settings))
  localStorage.setItem('zhitiku-preview-subjects', JSON.stringify(subjects))
  localStorage.setItem('zhitiku-preview-tags', JSON.stringify(tags))
  localStorage.setItem('zhitiku-preview-papers', JSON.stringify(papers))
  localStorage.setItem('zhitiku-preview-templates', JSON.stringify(templates))
  localStorage.setItem('zhitiku-preview-backups', JSON.stringify(backups))
  localStorage.setItem('zhitiku-preview-question-drafts', JSON.stringify(questionDrafts))
  localStorage.setItem(DUPLICATE_IGNORES_STORAGE_KEY, JSON.stringify(duplicateIgnores))
  localStorage.setItem(DOCUMENT_ENTRY_TEMPLATES_STORAGE_KEY, JSON.stringify(documentEntryTemplates))
  if (documentQuestionDraft) {
    localStorage.setItem(DOCUMENT_QUESTION_DRAFT_STORAGE_KEY, JSON.stringify(documentQuestionDraft))
  } else {
    localStorage.removeItem(DOCUMENT_QUESTION_DRAFT_STORAGE_KEY)
  }
  if (wordImportDraft) {
    localStorage.setItem(WORD_IMPORT_DRAFT_STORAGE_KEY, JSON.stringify(wordImportDraft))
  } else {
    localStorage.removeItem(WORD_IMPORT_DRAFT_STORAGE_KEY)
  }
  if (lastRestoreResult) {
    localStorage.setItem(RESTORE_RESULT_STORAGE_KEY, JSON.stringify(lastRestoreResult))
  } else {
    localStorage.removeItem(RESTORE_RESULT_STORAGE_KEY)
  }
  localStorage.setItem(DATA_ROOT_STORAGE_KEY, mockDataRoot)
  if (lastDataMoveResult) {
    localStorage.setItem(DATA_MOVE_RESULT_STORAGE_KEY, JSON.stringify(lastDataMoveResult))
  } else {
    localStorage.removeItem(DATA_MOVE_RESULT_STORAGE_KEY)
  }
}

const restore = () => {
  if (typeof localStorage === 'undefined') return
  try {
    const savedQuestions = localStorage.getItem('zhitiku-preview-questions')
    const savedSettings = localStorage.getItem('zhitiku-preview-settings')
    const savedSubjects = localStorage.getItem('zhitiku-preview-subjects')
    const savedTags = localStorage.getItem('zhitiku-preview-tags')
    const savedPapers = localStorage.getItem('zhitiku-preview-papers')
    const savedTemplates = localStorage.getItem('zhitiku-preview-templates')
    const savedBackups = localStorage.getItem('zhitiku-preview-backups')
    const savedQuestionDrafts = localStorage.getItem('zhitiku-preview-question-drafts')
    const savedDocumentQuestionDraft = localStorage.getItem(DOCUMENT_QUESTION_DRAFT_STORAGE_KEY)
    const savedDocumentEntryTemplates = localStorage.getItem(DOCUMENT_ENTRY_TEMPLATES_STORAGE_KEY)
    const savedWordImportDraft = localStorage.getItem(WORD_IMPORT_DRAFT_STORAGE_KEY)
    const savedRestoreResult = localStorage.getItem(RESTORE_RESULT_STORAGE_KEY)
    const savedDataMoveResult = localStorage.getItem(DATA_MOVE_RESULT_STORAGE_KEY)
    const savedDataRoot = localStorage.getItem(DATA_ROOT_STORAGE_KEY)
    const savedDuplicateIgnores = localStorage.getItem(DUPLICATE_IGNORES_STORAGE_KEY)
    if (savedQuestions) questions = JSON.parse(savedQuestions) as Question[]
    if (savedDuplicateIgnores) {
      const restored = JSON.parse(savedDuplicateIgnores) as unknown
      duplicateIgnores = Array.isArray(restored) ? restored : []
    }
    if (savedSettings) {
      settings = {
        ...defaultSettings,
        ...(JSON.parse(savedSettings) as Partial<AppSettings>),
      }
    }
    if (savedSubjects) subjects = JSON.parse(savedSubjects) as Subject[]
    if (savedTags) tags = JSON.parse(savedTags) as Tag[]
    if (savedPapers) {
      papers = (JSON.parse(savedPapers) as Paper[]).map((paper) => ({
        ...paper,
        generationConfig: paper.generationConfig ?? null,
      }))
    }
    if (savedTemplates) templates = JSON.parse(savedTemplates) as WordTemplate[]
    if (savedBackups) backups = JSON.parse(savedBackups) as BackupRecord[]
    if (savedQuestionDrafts) questionDrafts = JSON.parse(savedQuestionDrafts) as QuestionDraftRecord[]
    if (savedDocumentEntryTemplates) {
      const restored = JSON.parse(savedDocumentEntryTemplates) as DocumentEntryTemplate[]
      if (Array.isArray(restored) && restored.length) {
        documentEntryTemplates = restored.map((template) => ({
          ...template,
          config: normalizeDocumentEntryTemplateConfig(template.config),
        }))
      }
    }
    if (!documentEntryTemplates.some((template) => template.isBuiltIn)) {
      documentEntryTemplates.unshift(systemDocumentEntryTemplate())
    }
    if (!documentEntryTemplates.some((template) => template.isDefault)) {
      documentEntryTemplates = documentEntryTemplates.map((template, index) => ({
        ...template,
        isDefault: index === 0,
      }))
    }
    if (savedDocumentQuestionDraft) {
      try {
        const restored = JSON.parse(savedDocumentQuestionDraft) as DocumentQuestionDraftRecord
        const payloadValid = restored.payload.schemaVersion === 1
          ? Array.isArray(restored.payload.items) && restored.payload.items.length > 0
          : restored.payload.schemaVersion === 2 || restored.payload.schemaVersion === 3
        if (!restored.id || !payloadValid) {
          throw new Error('invalid document question draft')
        }
        documentQuestionDraft = restored
      } catch {
        documentQuestionDraft = null
      }
    }
    if (savedWordImportDraft) {
      try {
        const restored = JSON.parse(savedWordImportDraft) as WordImportDraftRecord
        const payload = normalizedWordImportDraftPayload(restored.payload)
        if (!restored.id || !Number.isSafeInteger(restored.autosavedAt)
          || !Number.isSafeInteger(restored.createdAt) || !Number.isSafeInteger(restored.updatedAt)) {
          throw new Error('invalid Word import draft record')
        }
        wordImportDraft = { ...restored, payload }
      } catch {
        wordImportDraft = null
      }
    }
    if (savedRestoreResult) {
      const restoredResult = JSON.parse(savedRestoreResult) as RestoreResult
      if (restoredResult.operationId
        && (restoredResult.outcome === 'success' || restoredResult.outcome === 'rolled_back')) {
        lastRestoreResult = restoredResult
      }
    }
    if (savedDataRoot) mockDataRoot = savedDataRoot
    if (savedDataMoveResult) {
      const restoredResult = JSON.parse(savedDataMoveResult) as DataMoveResult
      if (restoredResult.operationId
        && (restoredResult.outcome === 'success' || restoredResult.outcome === 'rolled_back')) {
        lastDataMoveResult = restoredResult
      }
    }
    const requestedDefaultId = settings.defaultTemplateId ?? null
    const defaultTemplateId = templates.some((template) => template.id === requestedDefaultId)
      ? requestedDefaultId
      : null
    if (settings.defaultTemplateId !== defaultTemplateId) {
      settings = { ...settings, defaultTemplateId }
    }
    templates = templates.map((template) => ({
      ...template,
      isDefault: template.id === defaultTemplateId,
    }))
    if (pruneDuplicateIgnores()) {
      localStorage.setItem(DUPLICATE_IGNORES_STORAGE_KEY, JSON.stringify(duplicateIgnores))
    }
  } catch {
    // A damaged browser preview cache must never block the desktop application.
  }
}

const SUBJECT_NAME_MAX = 50
const CHAPTER_NAME_MAX = 100
const TAG_NAME_MAX = 50
const SORT_STEP = 1024

function taxonomyError(message: string, code = 'VALIDATION_ERROR'): never {
  // Tauri returns serialized command errors as plain objects, not Error instances.
  throw { code, message }
}

function validatedName(value: string, label: string, maximum: number) {
  const name = value.trim().normalize('NFKC').trim()
  if (!name) taxonomyError(`${label}名称不能为空。`)
  if ([...name].length > maximum) taxonomyError(`${label}名称不能超过 ${maximum} 个字符。`)
  return name
}

function nameKey(value: string) {
  return value.trim().normalize('NFKC').toLocaleLowerCase('zh-CN')
}

function activeQuestions() {
  return questions.filter((question) => !question.deletedAt)
}

function taxonomySnapshot(): { subjects: Subject[], tags: Tag[] } {
  const active = activeQuestions()
  return {
    subjects: subjects.map((subject) => ({
      ...subject,
      questionCount: active.filter((question) => question.subjectId === subject.id).length,
      chapters: subject.chapters.map((chapter) => ({
        ...chapter,
        questionCount: active.filter((question) => question.chapterId === chapter.id).length,
      })),
    })),
    tags: tags.map((tag) => ({
      ...tag,
      questionCount: active.filter((question) => question.tags.some((item) => item.id === tag.id)).length,
    })),
  }
}

function assertExactOrder(items: TaxonomyOrderItem[], actualIds: string[], label: string) {
  const itemIds = items.map((item) => item.id)
  if (new Set(itemIds).size !== itemIds.length || new Set(items.map((item) => item.sortOrder)).size !== items.length) {
    taxonomyError('排序列表包含重复 ID 或重复排序值。')
  }
  if (items.some((item) => !Number.isSafeInteger(item.sortOrder) || item.sortOrder < 0)) {
    taxonomyError('排序值必须是互不重复的非负整数。')
  }
  const requested = [...itemIds].sort()
  const actual = [...actualIds].sort()
  if (requested.length !== actual.length || requested.some((id, index) => id !== actual[index])) {
    taxonomyError(`${label}列表已发生变化，请刷新页面后重新排序。`)
  }
}

function clone<T>(value: T): T {
  return clonePlain(value)
}

function stableJson(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map((item) => stableJson(item)).join(',')}]`
  if (value && typeof value === 'object') {
    const entries = Object.entries(value as Record<string, unknown>)
      .filter(([, item]) => item !== undefined)
      .sort(([left], [right]) => left.localeCompare(right))
    return `{${entries.map(([key, item]) => `${JSON.stringify(key)}:${stableJson(item)}`).join(',')}}`
  }
  return JSON.stringify(value)
}

function canonicalResourceRefs(resourceRefs: QuestionDraft['resourceRefs']) {
  return (resourceRefs ?? [])
    .map((resourceRef) => ({
      nodeId: resourceRef.nodeId,
      resourceId: resourceRef.resourceId,
      contentSlot: resourceRef.contentSlot,
      optionId: resourceRef.optionId ?? null,
    }))
    .sort((left, right) => stableJson(left).localeCompare(stableJson(right)))
}

function importedDraftContent(draft: QuestionDraft) {
  return {
    type: draft.type,
    stem: draft.stem,
    options: draft.options.map((option, position) => ({ ...option, position })),
    answer: draft.answer,
    explanation: draft.explanation,
    subjectId: draft.subjectId,
    chapterId: draft.chapterId,
    tagIds: [...draft.tagIds].sort(),
    resourceRefs: canonicalResourceRefs(draft.resourceRefs),
  }
}

function storedQuestionContent(question: Question) {
  return {
    type: question.type,
    stem: question.stem,
    options: question.options.map((option, position) => ({ ...option, position })),
    answer: question.answer,
    explanation: question.explanation,
    subjectId: question.subjectId,
    chapterId: question.chapterId,
    tagIds: question.tags.map((tag) => tag.id).sort(),
    resourceRefs: canonicalResourceRefs(question.resourceRefs),
  }
}

function importedDraftMatchesQuestion(draft: QuestionDraft, question: Question) {
  return stableJson(importedDraftContent(draft)) === stableJson(storedQuestionContent(question))
}

const MAX_QUESTION_DRAFT_BYTES = 12 * 1024 * 1024
const MAX_DUPLICATE_CANDIDATES = 256
const MAX_SIMILARITY_FIELD_CHARS = 8_192
const SUSPECTED_DUPLICATE_THRESHOLD_PERCENT = 82
const SIMILARITY_METHOD = 'nfkc_char_bigram_dice_v1' as const
const MAX_RICH_HTML_BYTES = 2 * 1024 * 1024
const MAX_RICH_PLAIN_BYTES = 512 * 1024
const MAX_SOURCE_OOXML_BYTES = 4 * 1024 * 1024
const MAX_TOTAL_CONTENT_BYTES = 12 * 1024 * 1024

type DuplicateContent = Pick<QuestionDraft, 'stem' | 'options' | 'answer'>

function isDuplicateWhitespace(character: string) {
  const codePoint = character.codePointAt(0) ?? -1
  return (codePoint >= 0x0009 && codePoint <= 0x000d)
    || codePoint === 0x0020
    || codePoint === 0x0085
    || codePoint === 0x00a0
    || codePoint === 0x1680
    || (codePoint >= 0x2000 && codePoint <= 0x200a)
    || codePoint === 0x2028
    || codePoint === 0x2029
    || codePoint === 0x202f
    || codePoint === 0x205f
    || codePoint === 0x3000
}

function normalizedExactQuestion(content: DuplicateContent) {
  return [...[
    content.stem.plainText,
    content.options.map((option) => option.content.plainText).join('\u001e'),
    content.answer.plainText,
  ]
    .join('\u001f')
    .normalize('NFKC')
    .toLocaleLowerCase('zh-CN')]
    .filter((character) => !isDuplicateWhitespace(character))
    .join('')
}

function boundedRawCharacters(parts: string[]) {
  const raw: string[] = []
  for (const part of parts) {
    for (const character of part) {
      raw.push(character)
      if (raw.length === MAX_SIMILARITY_FIELD_CHARS) return raw
    }
  }
  return raw
}

function normalizedSimilarityField(parts: string[]) {
  return [...boundedRawCharacters(parts).join('').normalize('NFKC').toLocaleLowerCase('zh-CN')]
    .filter((character) => !isDuplicateWhitespace(character))
    .slice(0, MAX_SIMILARITY_FIELD_CHARS)
}

function similarityPrefilterLength(content: DuplicateContent) {
  return boundedRawCharacters([content.stem.plainText]).length
    + boundedRawCharacters(content.options.map((option) => option.content.plainText)).length
    + boundedRawCharacters([content.answer.plainText]).length
}

function utf8Bytes(text: string) {
  return new TextEncoder().encode(text).byteLength
}

function richContentBytes(content: RichContent) {
  return utf8Bytes(content.html)
    + utf8Bytes(content.plainText)
    + utf8Bytes(content.sourceOoxml ?? '')
    + utf8Bytes(content.document ? JSON.stringify(content.document) : '')
}

function validateDuplicateRichContent(label: string, content: RichContent, allowEmpty: boolean) {
  if (content.schemaVersion === 1) {
    if (content.editor || content.editorVersion || content.document) {
      taxonomyError(`${label}的旧内容版本不能携带编辑器 JSON。`)
    }
  } else if (
    content.schemaVersion !== 2
    || content.editor !== 'tiptap'
    || !content.editorVersion?.trim()
    || content.document?.type !== 'doc'
  ) {
    taxonomyError(`${label}使用了软件暂不支持的内容版本。`)
  } else if (utf8Bytes(JSON.stringify(content.document)) > 4 * 1024 * 1024) {
    taxonomyError(`${label}的 Tiptap 文档 JSON 超过安全上限。`)
  }
  if (!allowEmpty && ![...content.plainText].some((character) => !isDuplicateWhitespace(character))) {
    taxonomyError(`${label}不能为空。`)
  }
  if (content.plainText.includes('\u001e') || content.plainText.includes('\u001f')) {
    taxonomyError(`${label}包含软件保留的控制字符，请删除后重试。`)
  }
  if (utf8Bytes(content.html) > MAX_RICH_HTML_BYTES) taxonomyError(`${label}的富文本超过 2 MB 的安全上限。`)
  if (utf8Bytes(content.plainText) > MAX_RICH_PLAIN_BYTES) taxonomyError(`${label}的可搜索文字过长，请拆分内容。`)
  if (utf8Bytes(content.sourceOoxml ?? '') > MAX_SOURCE_OOXML_BYTES) {
    taxonomyError(`${label}保留的 Word 原始内容超过安全上限。`)
  }
  const normalizedHtml = content.html.toLowerCase().split(/\s+/u).join(' ')
  const forbidden = ['<script', '<iframe', '<object', '<embed', '<link', '<meta', '<base', 'javascript:', 'data:text/html', 'srcdoc=']
  if (forbidden.some((pattern) => normalizedHtml.includes(pattern)) || / on[a-z]+\s*=/u.test(normalizedHtml)) {
    taxonomyError(`${label}中包含不安全的网页代码，已停止保存。`, 'UNSAFE_RICH_CONTENT')
  }
}

function validateDuplicateDraft(draft: QuestionDraft) {
  const definition = questionTypes.find((item) => item.code === draft.type && item.isEnabled)
  if (!definition) taxonomyError('所选题型不存在或已经停用。')
  if (draft.options.length > 26) taxonomyError('一道题最多允许 26 个选项。')
  if (draft.tagIds.length > 100) taxonomyError('一道题最多允许 100 个标签。')
  if (new Set(draft.tagIds).size !== draft.tagIds.length) taxonomyError('同一道题不能重复选择同一个标签。')
  validateDuplicateRichContent('题干', draft.stem, false)
  validateDuplicateRichContent('答案', draft.answer, false)
  validateDuplicateRichContent('解析', draft.explanation, true)
  draft.options.forEach((option, index) => validateDuplicateRichContent(`选项 ${index + 1}`, option.content, false))
  if (matchesChoiceBehavior(definition.behavior)) {
    if (draft.options.length < 2) taxonomyError('选择题至少需要两个非空选项。')
  } else if (draft.options.length) {
    taxonomyError('非选择题不能保存选择题选项。')
  }
  const totalBytes = richContentBytes(draft.stem)
    + richContentBytes(draft.answer)
    + richContentBytes(draft.explanation)
    + draft.options.reduce((sum, option) => sum + richContentBytes(option.content), 0)
  if (totalBytes > MAX_TOTAL_CONTENT_BYTES) {
    taxonomyError('题目内容合计超过 12 MB 的安全上限，请压缩图片或拆分内容。')
  }
}

function similarityText(content: DuplicateContent) {
  return [
    ...normalizedSimilarityField([content.stem.plainText]),
    '\u001f',
    ...normalizedSimilarityField(content.options.map((option) => option.content.plainText)),
    '\u001f',
    ...normalizedSimilarityField([content.answer.plainText]),
  ]
}

function characterBigramDicePercent(left: string[], right: string[]) {
  if (!left.length || !right.length) return 0
  const counts = (characters: string[]) => {
    const result = new Map<string, number>()
    if (characters.length === 1) {
      result.set(`${characters[0]!.codePointAt(0)},0`, 1)
      return result
    }
    for (let index = 0; index + 1 < characters.length; index += 1) {
      const key = `${characters[index]!.codePointAt(0)},${characters[index + 1]!.codePointAt(0)}`
      result.set(key, (result.get(key) ?? 0) + 1)
    }
    return result
  }
  const leftCounts = counts(left)
  const rightCounts = counts(right)
  const leftTotal = [...leftCounts.values()].reduce((sum, count) => sum + count, 0)
  const rightTotal = [...rightCounts.values()].reduce((sum, count) => sum + count, 0)
  let shared = 0
  for (const [gram, leftCount] of leftCounts) {
    shared += Math.min(leftCount, rightCounts.get(gram) ?? 0)
  }
  return Math.min(100, Math.round((shared * 200) / (leftTotal + rightTotal)))
}

function duplicateStemPreview(stem: string) {
  const collapsed = stem.trim().replace(/\s+/gu, ' ')
  const characters = [...collapsed]
  return characters.length > 160 ? `${characters.slice(0, 160).join('')}…` : collapsed
}

function duplicateCandidate(question: Question, similarityPercent: number): QuestionDuplicateCandidate {
  return {
    id: question.id,
    type: question.type,
    stemPreview: duplicateStemPreview(question.stem.plainText),
    subjectName: question.subjectName,
    chapterName: question.chapterName,
    similarityPercent,
    contentVersion: question.contentVersion,
  }
}

function duplicateMember(question: Question): QuestionDuplicateMember {
  return {
    id: question.id,
    type: question.type,
    stemPreview: duplicateStemPreview(question.stem.plainText),
    subjectId: question.subjectId,
    chapterId: question.chapterId,
    subjectName: question.subjectName,
    chapterName: question.chapterName,
    contentVersion: question.contentVersion,
    createdAt: question.createdAt,
    updatedAt: question.updatedAt,
  }
}

function canonicalDuplicatePair(first: Question, second: Question) {
  return first.id < second.id
    ? { lowId: first.id, lowVersion: first.contentVersion, highId: second.id, highVersion: second.contentVersion }
    : { lowId: second.id, lowVersion: second.contentVersion, highId: first.id, highVersion: first.contentVersion }
}

function isIgnoredDuplicatePair(first: Question, second: Question) {
  const pair = canonicalDuplicatePair(first, second)
  return duplicateIgnores.some((ignored) => (
    ignored.lowId === pair.lowId
    && ignored.lowVersion === pair.lowVersion
    && ignored.highId === pair.highId
    && ignored.highVersion === pair.highVersion
  ))
}

function pruneDuplicateIgnores() {
  const questionIds = new Set(questions.map((question) => question.id))
  const previousLength = duplicateIgnores.length
  duplicateIgnores = duplicateIgnores.filter((ignored) => (
    typeof ignored?.lowId === 'string'
    && typeof ignored?.highId === 'string'
    && ignored.lowId !== ignored.highId
    && Number.isSafeInteger(ignored.lowVersion)
    && ignored.lowVersion > 0
    && Number.isSafeInteger(ignored.highVersion)
    && ignored.highVersion > 0
    && questionIds.has(ignored.lowId)
    && questionIds.has(ignored.highId)
  ))
  return duplicateIgnores.length !== previousLength
}

function questionDraftKey(questionId?: string | null) {
  const normalizedId = questionId?.trim()
  return normalizedId ? `question:${normalizedId}` : 'question:create'
}

function normalizedQuestionDraft(input: QuestionDraft): QuestionDraft {
  const payload: QuestionDraft = {
    type: input.type,
    stem: clone(input.stem),
    options: input.options.map((option) => clone(option)),
    answer: clone(input.answer),
    explanation: clone(input.explanation),
    subjectId: input.subjectId,
    chapterId: input.chapterId,
    tagIds: [...input.tagIds],
    resourceRefs: clone(input.resourceRefs ?? []),
  }
  if (input.id) payload.id = input.id
  if (input.questionId) payload.questionId = input.questionId
  if (input.baseContentVersion !== undefined) payload.baseContentVersion = input.baseContentVersion

  const payloadBytes = new TextEncoder().encode(JSON.stringify(payload)).byteLength
  if (payloadBytes > MAX_QUESTION_DRAFT_BYTES) {
    taxonomyError('题目草稿超过 12 MB 的安全上限，请压缩图片或删减内容。', 'DRAFT_PAYLOAD_TOO_LARGE')
  }
  if (payload.questionId) {
    if (!questions.some((question) => question.id === payload.questionId)) {
      taxonomyError('草稿对应的题目已不存在。', 'QUESTION_NOT_FOUND')
    }
    if (!Number.isSafeInteger(payload.baseContentVersion) || (payload.baseContentVersion ?? 0) < 1) {
      taxonomyError('编辑题目的草稿必须包含有效的内容版本。')
    }
  } else if (payload.baseContentVersion !== undefined) {
    taxonomyError('新增题目的草稿不能包含题目内容版本。')
  }
  return payload
}

function canonicalDraftUuid(value: unknown, label: string) {
  if (typeof value !== 'string'
    || !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/u.test(value)) {
    taxonomyError(`${label}必须使用规范 UUID 格式。`)
  }
  return value
}

function normalizedWordImportFileName(value: unknown) {
  if (typeof value !== 'string') taxonomyError('来源 Word 文件名无效。')
  const normalized = value.trim().normalize('NFKC').trim()
  if (!normalized
    || [...normalized].length > MAX_WORD_IMPORT_SOURCE_NAME_CHARS
    || /[\\/:\p{Cc}]/u.test(normalized)
    || normalized === '.'
    || normalized === '..'
    || !normalized.toLocaleLowerCase('en-US').endsWith('.docx')) {
    taxonomyError('来源文件必须是不含路径、以 .docx 结尾且不超过 255 个字符的文件名。')
  }
  return normalized
}

function normalizedWordImportParserVersion(value: unknown) {
  if (typeof value !== 'string') taxonomyError('Word 分析器版本无效。')
  const normalized = value.trim().normalize('NFKC').trim()
  if (!normalized
    || [...normalized].length > MAX_WORD_IMPORT_PARSER_VERSION_CHARS
    || /[\p{Cc}\u001e\u001f]/u.test(normalized)) {
    taxonomyError('Word 分析器版本不能为空、不能含控制字符且不能超过 64 个字符。')
  }
  return normalized
}

function validateRecoverableImportQuestionDraft(draft: QuestionDraft, ordinal: number) {
  if (!draft || typeof draft !== 'object') taxonomyError(`第 ${ordinal} 项缺少题目草稿。`)
  if (draft.questionId !== undefined || draft.baseContentVersion !== undefined) {
    taxonomyError(`第 ${ordinal} 项必须是新增题目，不能携带题目 ID 或基础内容版本。`)
  }
  if (!questionTypes.some((item) => item.code === draft.type)) taxonomyError(`第 ${ordinal} 项的题型无效。`)
  if (!Array.isArray(draft.options) || draft.options.length > 26) {
    taxonomyError(`第 ${ordinal} 项最多只能包含 26 个选项。`)
  }
  if (!Array.isArray(draft.tagIds) || draft.tagIds.length > 100
    || new Set(draft.tagIds).size !== draft.tagIds.length) {
    taxonomyError(`第 ${ordinal} 项的标签列表无效。`)
  }
  validateDuplicateRichContent(`第 ${ordinal} 项题干`, draft.stem, true)
  validateDuplicateRichContent(`第 ${ordinal} 项答案`, draft.answer, true)
  validateDuplicateRichContent(`第 ${ordinal} 项解析`, draft.explanation, true)
  draft.options.forEach((option, index) => {
    validateDuplicateRichContent(`第 ${ordinal} 项选项 ${index + 1}`, option.content, true)
  })
  const totalBytes = richContentBytes(draft.stem)
    + richContentBytes(draft.answer)
    + richContentBytes(draft.explanation)
    + draft.options.reduce((sum, option) => sum + richContentBytes(option.content), 0)
  if (totalBytes > MAX_TOTAL_CONTENT_BYTES) {
    taxonomyError(`第 ${ordinal} 项的题目内容超过 12 MB 安全上限。`)
  }
}

function validateWordImportCandidate(candidate: NonNullable<WordImportDraftPayload['items'][number]['candidate']>, ordinal: number) {
  // Browser preview seed questions use readable IDs (for example question-1),
  // while the desktop database uses UUIDs. Keep the shape checks aligned but
  // deliberately do not require a UUID for this one mock-only field.
  if (typeof candidate.id !== 'string' || !candidate.id.trim()) {
    taxonomyError(`第 ${ordinal} 项的重复候选题 ID 无效。`)
  }
  const sourceKind = candidate.sourceKind ?? 'question'
  const importItemCandidate = sourceKind === 'import-item'
  if (!questionTypes.some((item) => item.code === candidate.type)
    || !Number.isSafeInteger(candidate.similarityPercent)
    || candidate.similarityPercent < 0
    || candidate.similarityPercent > 100
    || !Number.isSafeInteger(candidate.contentVersion)
    || (importItemCandidate ? candidate.contentVersion !== 0 : candidate.contentVersion < 1)
    || !['question', 'import-item'].includes(sourceKind)
    || (importItemCandidate
      && (!Number.isSafeInteger(candidate.sourceOrdinal)
        || (candidate.sourceOrdinal ?? 0) < 1
        || (candidate.sourceOrdinal ?? 0) > MAX_WORD_IMPORT_ITEMS))) {
    taxonomyError(`第 ${ordinal} 项的重复候选题信息无效。`)
  }
}

function normalizedWordImportDraftPayload(input: WordImportDraftPayload): WordImportDraftPayload {
  if (!input || typeof input !== 'object') taxonomyError('Word 导入草稿不能为空。')
  if (input.schemaVersion !== 1) taxonomyError('Word 导入草稿仅支持数据版本 1。')
  if (!Number.isSafeInteger(input.sourceFileSize)
    || input.sourceFileSize < 0
    || input.sourceFileSize > MAX_WORD_IMPORT_SOURCE_BYTES) {
    taxonomyError('来源 Word 文件大小必须在 0 到 100 MB 之间。')
  }
  if (!Array.isArray(input.items) || input.items.length > MAX_WORD_IMPORT_ITEMS) {
    taxonomyError(`一次 Word 导入最多恢复 ${MAX_WORD_IMPORT_ITEMS} 道题。`)
  }
  const sourceItemCount = input.sourceItemCount ?? null
  const omittedItemCount = input.omittedItemCount ?? 0
  if ((sourceItemCount !== null
    && (!Number.isSafeInteger(sourceItemCount) || sourceItemCount < 0 || sourceItemCount > 0xffff_ffff))
    || !Number.isSafeInteger(omittedItemCount)
    || omittedItemCount < 0
    || omittedItemCount > 0xffff_ffff
    || (sourceItemCount === null && omittedItemCount !== 0)
    || (sourceItemCount !== null && (omittedItemCount > sourceItemCount
      || input.items.length + omittedItemCount > sourceItemCount))) {
    taxonomyError('Word 导入草稿的原始识别数量、保留数量与未载入数量不一致。')
  }

  const normalized = clone(input)
  normalized.sourceFileName = normalizedWordImportFileName(input.sourceFileName)
  normalized.parserVersion = normalizedWordImportParserVersion(input.parserVersion)
  normalized.sourceItemCount = sourceItemCount
  normalized.omittedItemCount = omittedItemCount
  const ids = new Set<string>()
  const ordinals = new Set<number>()
  normalized.items.forEach((item, index) => {
    const labelOrdinal = index + 1
    canonicalDraftUuid(item.id, `第 ${labelOrdinal} 项的 ID`)
    if (ids.has(item.id)) taxonomyError('Word 导入草稿中存在重复的题目 ID。')
    ids.add(item.id)
    if (!Number.isSafeInteger(item.ordinal) || item.ordinal < 1 || item.ordinal > MAX_WORD_IMPORT_ITEMS) {
      taxonomyError(`第 ${labelOrdinal} 项的来源顺序号必须在 1 到 ${MAX_WORD_IMPORT_ITEMS} 之间。`)
    }
    if (ordinals.has(item.ordinal)) taxonomyError('Word 导入草稿中存在重复的来源顺序号。')
    ordinals.add(item.ordinal)
    if (typeof item.selected !== 'boolean' || typeof item.needsCheck !== 'boolean') {
      taxonomyError(`第 ${labelOrdinal} 项的选择或检查状态无效。`)
    }
    if (!['ok', 'warning', 'error'].includes(item.status)) {
      taxonomyError(`第 ${labelOrdinal} 项的审查状态无效。`)
    }
    validateRecoverableImportQuestionDraft(item.payload, item.ordinal)
    if (!['none', 'exact', 'suspected'].includes(item.duplicateKind)) {
      taxonomyError(`第 ${labelOrdinal} 项的重复类型无效。`)
    }
    const hasCandidate = item.candidate !== undefined && item.candidate !== null
    if ((item.duplicateKind === 'none' && hasCandidate)
      || (item.duplicateKind !== 'none' && !hasCandidate)) {
      taxonomyError(`第 ${labelOrdinal} 项的重复类型与候选题不一致。`)
    }
    if (hasCandidate) validateWordImportCandidate(item.candidate!, item.ordinal)
    const hasAction = item.action !== undefined && item.action !== null
    if (hasAction && (!['skip', 'overwrite', 'keep'].includes(item.action!)
      || item.duplicateKind === 'none' || !hasCandidate)) {
      taxonomyError(`第 ${labelOrdinal} 项的重复题处理方式无效。`)
    }
    if (item.checkError != null && !item.needsCheck) {
      taxonomyError(`第 ${labelOrdinal} 项记录了检查错误，必须标记为需要重新检查。`)
    }
  })

  if (normalized.activeItemId !== undefined && normalized.activeItemId !== null) {
    canonicalDraftUuid(normalized.activeItemId, '当前审查题目 ID')
    if (!ids.has(normalized.activeItemId)) {
      taxonomyError('当前审查题目 ID 不属于这份 Word 导入草稿。')
    }
  }
  const payloadBytes = utf8Bytes(JSON.stringify(normalized))
  if (payloadBytes > MAX_WORD_IMPORT_PAYLOAD_BYTES) {
    taxonomyError('Word 导入草稿数据超过 64 MB 安全上限。', 'DRAFT_PAYLOAD_TOO_LARGE')
  }
  return normalized
}

function questionDraftWithStale(record: QuestionDraftRecord): QuestionDraftRecord {
  const current = record.targetQuestionId
    ? questions.find((question) => question.id === record.targetQuestionId)
    : null
  return {
    ...clone(record),
    stale: record.draftKind === 'question_edit'
      && (!current || current.contentVersion !== record.baseContentVersion),
  }
}

function validatePaper(paper: Paper) {
  const title = paper.title.trim().normalize('NFKC').trim()
  if (!title) taxonomyError('试卷名称不能为空。')
  if ([...title].length > 200) taxonomyError('试卷名称不能超过 200 个字符。')
  if (paper.status === 'saved' && !paper.items.length) taxonomyError('保存到历史试卷前，请至少加入一道题。')
  if (paper.items.length > 1000) taxonomyError('一份试卷最多包含 1000 道题。')
  if (paper.items.some((item, position) => item.position !== position)) {
    taxonomyError('试卷题目顺序不连续，请重新排序后保存。')
  }
  const sourceIds = paper.items.map((item) => item.sourceQuestionId).filter((id): id is string => Boolean(id))
  if (new Set(sourceIds).size !== sourceIds.length) taxonomyError('同一道来源题目不能重复加入一份试卷。')
  const config = paper.generationConfig ?? null
  if (paper.compositionMode === 'manual' && config) {
    taxonomyError('手动组卷不能保留自动组卷条件。')
  }
  if (config) {
    if (config.subjectId !== null && (typeof config.subjectId !== 'string' || !config.subjectId.trim())) {
      taxonomyError('自动组卷的学科条件无效。')
    }
    if (!Array.isArray(config.chapterIds) || config.chapterIds.length > 10_000
      || config.chapterIds.some((id) => typeof id !== 'string' || !id.trim())
      || new Set(config.chapterIds).size !== config.chapterIds.length) {
      taxonomyError('自动组卷的章节条件无效。')
    }
    const keys = Object.keys(config.questionTypeCounts ?? {})
    if (keys.some((key) => !questionTypes.some((item) => item.code === key))) {
      taxonomyError('自动组卷包含不受支持的题型条件。')
    }
    const counts = questionTypes.map((type) => config.questionTypeCounts?.[type.code] ?? 0)
    if (counts.some((count) => !Number.isSafeInteger(count) || count < 0 || count > 1000)
      || counts.reduce((sum, count) => sum + count, 0) > 1000) {
      taxonomyError('自动组卷题量必须是 0 到 1000 的整数，且总题量不能超过 1000。')
    }
  }
  return title
}

function paperSubjectSummary(paper: Paper) {
  return [...new Set(paper.items.map((item) => item.snapshot.subjectName.trim()).filter(Boolean))].join('、')
}

function paperSummary(paper: Paper): PaperSummary {
  return {
    id: paper.id,
    title: paper.title,
    compositionMode: paper.compositionMode,
    status: paper.status,
    subjectSummaryText: paper.subjectSummaryText,
    questionCount: paper.items.length,
    rowVersion: paper.rowVersion,
    createdAt: paper.createdAt,
    updatedAt: paper.updatedAt,
    savedAt: paper.savedAt,
    lastSavedAt: paper.lastSavedAt,
  }
}

const TEMPLATE_NAME_MAX = 200
const MOCK_TEMPLATE_BYTES = 64 * 1024
const MAX_TEMPLATE_BYTES = 100 * 1024 * 1024

interface MockTemplateFile {
  fileName?: string
  size?: number
  configured?: boolean
}

function templateName(value: string) {
  return validatedName(value, '模板', TEMPLATE_NAME_MAX)
}

function templateFileName(sourcePath: string, override?: string) {
  const path = (override ?? sourcePath).trim()
  return path.split(/[\\/]/).filter(Boolean).pop() ?? ''
}

function templateNameFromFile(fileName: string) {
  return fileName.replace(/\.(?:docx|dotx)$/i, '')
}

function mockSha256Hex(value: string) {
  let hash = 0x811c9dc5
  for (const character of value) {
    hash ^= character.codePointAt(0) ?? 0
    hash = Math.imul(hash, 0x01000193) >>> 0
  }
  return hash.toString(16).padStart(8, '0').repeat(8)
}

function templateDiagnostic(
  severity: DocxDiagnostic['severity'],
  code: string,
  message: string,
  suggestedAction?: string,
): DocxDiagnostic {
  return {
    severity,
    code,
    partName: null,
    message,
    suggestedAction: suggestedAction ?? null,
  }
}

function findTemplate(id: string) {
  const template = templates.find((item) => item.id === id)
  if (!template) taxonomyError('找不到指定模板，请刷新页面后重试。', 'TEMPLATE_NOT_FOUND')
  return template
}

function assertTemplateVersion(template: WordTemplate, baseRowVersion: number) {
  if (template.rowVersion !== baseRowVersion) {
    taxonomyError('这个模板已在其他窗口中更新，请刷新页面后重试。', 'TEMPLATE_VERSION_CONFLICT')
  }
}

function assertUniqueTemplateName(name: string, exceptId?: string) {
  if (templates.some((item) => item.id !== exceptId && nameKey(item.name) === nameKey(name))) {
    taxonomyError('已经存在同名模板。')
  }
}

restore()

function randomDrawCandidates(scope: RandomDrawScope) {
  const excludedIds = new Set(scope.excludedQuestionIds)
  const subjectIds = new Set(scope.subjectIds)
  const usage = scope.usage ?? 'all'
  const currentTime = Date.now()
  return questions.filter((question) => {
    if (question.deletedAt || excludedIds.has(question.id)) return false
    if (subjectIds.size && !subjectIds.has(question.subjectId)) return false
    if (scope.chapterIds.length && !scope.chapterIds.includes(question.chapterId)) return false
    if (!matchesQuestionUsage(question.lastUsedAt, usage, currentTime)) return false
    if (!scope.tagIds.length) return true
    return scope.tagMatchMode === 'any'
      ? scope.tagIds.some((tagId) => question.tags.some((tag) => tag.id === tagId))
      : scope.tagIds.every((tagId) => question.tags.some((tag) => tag.id === tagId))
  })
}

function shuffledQuestions(values: readonly Question[]) {
  const result = [...values]
  for (let index = result.length - 1; index > 0; index -= 1) {
    const target = Math.floor(Math.random() * (index + 1))
    ;[result[index], result[target]] = [result[target], result[index]]
  }
  return result
}

export const mockBackend = {
  async initialize(): Promise<BootstrapData> {
    const snapshot = taxonomySnapshot()
    return {
      initialized: snapshot.subjects.length > 0,
      dataRoot: mockDataRoot,
      subjects: snapshot.subjects,
      tags: snapshot.tags,
      questionTypes: clone(questionTypes.map((definition) => ({
        ...definition,
        questionCount: activeQuestions().filter((question) => question.type === definition.code).length,
        paperItemCount: papers.reduce((count, paper) => (
          count + paper.items.filter((item) => item.snapshot.type === definition.code).length
        ), 0),
      }))),
      pendingDraftCount: questionDrafts.length + (documentQuestionDraft ? 1 : 0) + (wordImportDraft ? 1 : 0),
      databaseHealthy: true,
      appVersion: __APP_VERSION__,
      license: clone(mockLicense),
    }
  },

  async getLicenseOverview(): Promise<LicenseOverview> {
    return clone(mockLicense)
  },

  async exportLicenseRequest(outputPath: string): Promise<LicenseFileResult> {
    return { path: outputPath, filename: 'browser-demo.tkreq', bytes: 0 }
  },

  async importDesktopLicense(_inputPath: string): Promise<LicenseOverview> {
    return clone(mockLicense)
  },

  async removeDesktopLicense(): Promise<LicenseOverview> {
    return clone(mockLicense)
  },

  async listQuestionTypes(): Promise<QuestionTypeDefinition[]> {
    return clone(questionTypes).sort((left, right) => left.sortOrder - right.sortOrder)
  },

  async saveQuestionType(request: SaveQuestionTypeRequest): Promise<QuestionTypeDefinition> {
    const name = validatedName(request.name, '题型', 40)
    const nameKey = (value: string) => value.normalize('NFKC').trim().toLocaleLowerCase('zh-CN').replace(/\s+/gu, '')
    const aliases = [...new Map(request.aliases
      .map((alias) => validatedName(alias, '识别别名', 40))
      .filter((alias) => nameKey(alias) !== nameKey(name))
      .map((alias) => [nameKey(alias), alias])).values()]
    const keys = new Set([nameKey(name), ...aliases.map(nameKey)])
    if (questionTypes.some((definition) => definition.code !== request.code && (
      keys.has(nameKey(definition.name)) || definition.aliases.some((alias) => keys.has(nameKey(alias)))
    ))) taxonomyError('题型名称或识别别名已经被其他题型使用。')
    const existing = request.code ? questionTypes.find((item) => item.code === request.code) : undefined
    if (request.code && !existing) taxonomyError('要修改的题型已经不存在。')
    if (existing?.isBuiltin && existing.behavior !== request.behavior) taxonomyError('内置题型的基础答题结构不能修改。')
    if (existing && existing.behavior !== request.behavior && (existing.questionCount + existing.paperItemCount) > 0) {
      taxonomyError('该题型已经被题目或试卷使用，不能修改基础答题结构。')
    }
    const timestamp = Date.now()
    const saved: QuestionTypeDefinition = {
      code: existing?.code ?? `custom_${crypto.randomUUID().replace(/-/g, '')}`,
      name,
      behavior: request.behavior,
      aliases,
      defaultOptions: [...request.defaultOptions],
      isBuiltin: existing?.isBuiltin ?? false,
      isEnabled: request.isEnabled,
      sortOrder: existing?.sortOrder ?? Math.max(0, ...questionTypes.map((item) => item.sortOrder)) + 10,
      questionCount: existing?.questionCount ?? 0,
      paperItemCount: existing?.paperItemCount ?? 0,
      createdAt: existing?.createdAt ?? timestamp,
      updatedAt: timestamp,
    }
    questionTypes = existing
      ? questionTypes.map((item) => item.code === saved.code ? saved : item)
      : [...questionTypes, saved]
    return clone(saved)
  },

  async deleteQuestionType(code: string): Promise<void> {
    const definition = questionTypes.find((item) => item.code === code)
    if (!definition) taxonomyError('题型已经不存在。')
    if (definition.isBuiltin) taxonomyError('内置题型不能删除，可以将它停用。')
    if (questions.some((question) => question.type === code)
      || papers.some((paper) => paper.items.some((item) => item.snapshot.type === code))) {
      taxonomyError('该题型已经被题目或试卷使用，不能删除，可以将它停用。')
    }
    questionTypes = questionTypes.filter((item) => item.code !== code)
  },

  async saveQuestionTypeOrder(items: TaxonomyOrderItem[]): Promise<void> {
    assertExactOrder(items, questionTypes.map((item) => item.code), '题型')
    const order = new Map(items.map((item) => [item.id, item.sortOrder]))
    questionTypes = questionTypes.map((item) => ({ ...item, sortOrder: order.get(item.code) ?? item.sortOrder }))
  },

  async pickDocxFile(): Promise<string> {
    return 'C:\\Users\\Teacher\\Documents\\示例试卷模板.docx'
  },

  async listTemplates(): Promise<WordTemplate[]> {
    return clone(templates).sort((left, right) => {
      if (left.isDefault !== right.isDefault) return left.isDefault ? -1 : 1
      return right.updatedAt - left.updatedAt || left.name.localeCompare(right.name, 'zh-CN')
    })
  },

  async importTemplate(
    sourcePath: string,
    requestedName?: string,
    mockFile: MockTemplateFile = {},
  ): Promise<TemplateImportResult> {
    const fileName = templateFileName(sourcePath, mockFile.fileName)
    const extension = fileName.match(/\.(docx|dotx)$/i)?.[1]?.toLocaleLowerCase('en-US')
    if (!extension) {
      const diagnostic = templateDiagnostic(
        'error',
        'DOCX_TEMPLATE_EXTENSION_UNSUPPORTED',
        '只能导入 .docx 或 .dotx 格式的 Word 模板。',
        '请在 Word 中将文件另存为 .docx 或 .dotx 后重试。',
      )
      return { imported: false, template: null, diagnostics: [diagnostic] }
    }

    const fileByteSize = mockFile.size ?? MOCK_TEMPLATE_BYTES
    if (!Number.isSafeInteger(fileByteSize) || fileByteSize < 0 || fileByteSize > MAX_TEMPLATE_BYTES) {
      const diagnostic = templateDiagnostic(
        'error',
        'DOCX_ARCHIVE_SIZE_LIMIT',
        `模板文件大小必须在 0 到 ${MAX_TEMPLATE_BYTES} 字节之间。`,
        '请压缩模板中的图片，或选择体积更小的模板。',
      )
      return { imported: false, template: null, diagnostics: [diagnostic] }
    }

    const name = templateName(requestedName ?? templateNameFromFile(fileName))
    assertUniqueTemplateName(name)
    const timestamp = Date.now()
    const id = crypto.randomUUID()
    const configured = mockFile.configured ?? true
    const template: WordTemplate = {
      id,
      name,
      fileName,
      fileSha256Hex: mockSha256Hex(`${sourcePath}\u0000${fileName}\u0000${fileByteSize}`),
      fileByteSize,
      analysisStatus: configured ? 'ready' : 'needs_configuration',
      analysisSchemaVersion: 1,
      parserVersion: 'w1-mock-1',
      rowVersion: 1,
      createdAt: timestamp,
      updatedAt: timestamp,
      lastVerifiedAt: timestamp,
      isDefault: false,
      fileAvailable: true,
      regionConfigured: configured,
      packageKind: extension === 'dotx' ? 'template' : 'document',
      anchors: configured
        ? [{
            name: 'ZT_QUESTIONS',
            kind: 'content_control',
            partName: 'word/document.xml',
            replacementSpan: { start: 256, end: 512 },
            containerSpan: { start: 192, end: 576 },
            paragraphIndex: 0,
          }]
        : [],
      diagnostics: [],
    }
    templates.push(template)
    persist()
    return { imported: true, template: clone(template), diagnostics: [] }
  },

  async getTemplateConfigurationPreview(id: string): Promise<TemplateConfigurationPreview> {
    const template = findTemplate(id)
    return {
      templateId: template.id,
      templateName: template.name,
      rowVersion: template.rowVersion,
      paragraphs: [
        { index: 0, text: '学校：__________　班级：__________　姓名：__________' },
        { index: 1, text: template.name },
        { index: 2, text: '请认真作答，并将答案填写在指定位置。' },
        { index: 3, text: '' },
      ],
      hasQuestionsAnchor: template.anchors.some((anchor) => anchor.name === 'ZT_QUESTIONS'),
      hasTitleAnchor: template.anchors.some((anchor) => anchor.name === 'ZT_TITLE'),
    }
  },

  async getTemplateLayoutPreview(id: string): Promise<TemplateLayoutPreview> {
    const template = findTemplate(id)
    if (!template.regionConfigured) {
      taxonomyError('这个模板还没有配置试题替换区域。', 'TEMPLATE_REGION_NOT_CONFIGURED')
    }
    return {
      templateId: template.id,
      templateName: template.name,
      rowVersion: template.rowVersion,
      page: {
        widthTwips: 11906,
        heightTwips: 16838,
        marginTopTwips: 1440,
        marginRightTwips: 1440,
        marginBottomTwips: 1440,
        marginLeftTwips: 1440,
        headerTwips: 720,
        footerTwips: 720,
        columnCount: 1,
        columnGapTwips: 720,
        columnSeparator: false,
        documentGridType: null,
        documentGridLinePitchTwips: null,
      },
      blocks: [
        { kind: 'static', text: '学校：_________　班级：_________　姓名：_________', style: null },
        { kind: 'title', text: '', style: null },
        { kind: 'questions', text: '', style: null },
      ],
      fontTheme: {
        majorLatin: 'Calibri Light',
        majorEastAsia: '宋体',
        minorLatin: 'Calibri',
        minorEastAsia: '宋体',
      },
      pageNumberFormat: null,
      styleProfile: null,
    }
  },

  async configureTemplateRegions(request: ConfigureTemplateRegionsRequest): Promise<WordTemplate> {
    const template = findTemplate(request.templateId)
    assertTemplateVersion(template, request.baseRowVersion)
    const paragraphIndex = request.questions.paragraphIndex
    if (request.questions.mode !== 'document_end' && paragraphIndex === undefined) {
      taxonomyError('请选择题目替换区域对应的模板段落。')
    }
    if (request.questions.mode === 'replace_range') {
      const endParagraphIndex = request.questions.endParagraphIndex
      if (paragraphIndex === undefined || endParagraphIndex === undefined) {
        taxonomyError('请选择原试题区域的开始和结束段落。')
      }
      if (paragraphIndex > endParagraphIndex) {
        taxonomyError('试题开始段落不能排在结束段落之后。')
      }
      const questionSample = request.styleSamples?.questionParagraphIndex
      if (questionSample === undefined
        || questionSample < paragraphIndex
        || questionSample > endParagraphIndex) {
        taxonomyError('请选择试题区域内的题号和题干格式样本。')
      }
    }
    if (request.title?.mode !== undefined
      && request.title.mode !== 'document_end'
      && request.title.paragraphIndex === undefined) {
      taxonomyError('请选择试卷标题替换区域对应的模板段落。')
    }
    const titleOverlapsRange = request.title?.paragraphIndex !== undefined
      && request.questions.mode === 'replace_range'
      && request.questions.paragraphIndex !== undefined
      && request.questions.endParagraphIndex !== undefined
      && request.title.paragraphIndex >= request.questions.paragraphIndex
      && request.title.paragraphIndex <= request.questions.endParagraphIndex
    if (request.title
      && (titleOverlapsRange
        || (request.title.mode === request.questions.mode
          && request.title.paragraphIndex === request.questions.paragraphIndex))) {
      taxonomyError('试卷标题区域与题目区域不能使用同一个插入位置。')
    }

    const timestamp = Date.now()
    const questionsParagraphIndex = request.questions.mode === 'document_end'
      ? null
      : request.questions.paragraphIndex ?? null
    const configuredAnchors: WordTemplate['anchors'] = [{
      name: 'ZT_QUESTIONS',
      kind: request.questions.mode === 'replace_range' ? 'content_control' : 'paragraph_marker',
      partName: 'word/document.xml',
      replacementSpan: { start: 512, end: 640 },
      containerSpan: { start: 480, end: 672 },
      paragraphIndex: questionsParagraphIndex,
    }]
    if (request.title) {
      configuredAnchors.push({
        name: 'ZT_TITLE',
        kind: 'paragraph_marker',
        partName: 'word/document.xml',
        replacementSpan: { start: 256, end: 384 },
        containerSpan: { start: 224, end: 416 },
        paragraphIndex: request.title.mode === 'document_end'
          ? null
          : request.title.paragraphIndex ?? null,
      })
    } else {
      configuredAnchors.push(...template.anchors.filter((anchor) => anchor.name === 'ZT_TITLE'))
    }
    template.anchors = configuredAnchors
    template.analysisStatus = 'ready'
    template.regionConfigured = true
    template.fileSha256Hex = mockSha256Hex(
      `${template.fileSha256Hex}\u0000${JSON.stringify(request)}\u0000${timestamp}`,
    )
    template.fileByteSize += 128
    template.rowVersion += 1
    template.updatedAt = timestamp
    template.lastVerifiedAt = timestamp
    persist()
    return clone(template)
  },

  async renameTemplate(id: string, value: string, baseRowVersion: number): Promise<WordTemplate> {
    const template = findTemplate(id)
    assertTemplateVersion(template, baseRowVersion)
    const name = templateName(value)
    assertUniqueTemplateName(name, id)
    template.name = name
    template.rowVersion += 1
    template.updatedAt = Date.now()
    persist()
    return clone(template)
  },

  async deleteTemplate(id: string, baseRowVersion: number): Promise<void> {
    const template = findTemplate(id)
    assertTemplateVersion(template, baseRowVersion)
    templates = templates.filter((item) => item.id !== id)
    if (settings.defaultTemplateId === id) settings = { ...settings, defaultTemplateId: null }
    persist()
  },

  async setDefaultTemplate(id: string, baseRowVersion: number): Promise<void> {
    const template = findTemplate(id)
    assertTemplateVersion(template, baseRowVersion)
    if (template.anchors.filter((anchor) => anchor.name === 'ZT_QUESTIONS').length !== 1) {
      taxonomyError('请先为模板配置唯一的题目替换区域，再设为默认模板。')
    }
    templates = templates.map((item) => ({ ...item, isDefault: item.id === id }))
    settings = { ...settings, defaultTemplateId: id }
    persist()
  },

  async listBackups(): Promise<BackupRecord[]> {
    return clone(backups).sort((left, right) => right.createdAt - left.createdAt)
  },

  async pickBackupSavePath(): Promise<string> {
    return `C:\\Users\\Teacher\\Documents\\教师题库\\backup\\TK试题题库备份_${Date.now()}.tqb`
  },

  async createBackup(outputPath: string): Promise<BackupCreateResult> {
    if (!outputPath.toLocaleLowerCase('en-US').endsWith('.tqb')) {
      taxonomyError('备份文件名必须使用 .tqb 扩展名。', 'BACKUP_ARCHIVE_REJECTED')
    }
    const timestamp = Date.now()
    const filename = outputPath.split(/[\\/]/).pop() || `TK试题题库备份_${timestamp}.tqb`
    const archiveHash = mockSha256Hex(`${outputPath}\u0000${timestamp}`)
    const record: BackupRecord = {
      id: crypto.randomUUID(),
      backupKind: 'manual',
      status: 'ready',
      displayFilename: filename,
      formatVersion: 1,
      databaseSchemaVersion: 8,
      sourceDatabaseUuid: '01900000-0000-7000-8000-000000000001',
      sourceAppVersion: '0.1.0',
      archiveSha256Hex: archiveHash,
      manifestSha256Hex: mockSha256Hex(`manifest\u0000${archiveHash}`),
      archiveByteSize: 188 * 1024 * 1024,
      payloadFileCount: 24,
      payloadByteSize: 186 * 1024 * 1024,
      createdAt: timestamp,
      completedAt: timestamp,
      fileAvailable: true,
      errorCode: null,
      errorMessage: null,
    }
    backups.unshift(record)
    persist()
    return { outputPath, record: clone(record) }
  },

  async runAutomaticBackup(): Promise<AutomaticBackupRun> {
    const latest = backups
      .filter((backup) => backup.backupKind === 'automatic' && backup.status === 'ready')
      .sort((left, right) => right.createdAt - left.createdAt)[0]
    if (!settings.automaticBackupEnabled) {
      return {
        outcome: 'disabled', message: '自动备份已关闭。', record: null, prunedCount: 0,
        lastBackupAt: latest?.completedAt ?? null, nextDueAt: null, warnings: [],
      }
    }
    const interval = settings.automaticBackupIntervalDays * 86_400_000
    if (latest?.completedAt && latest.completedAt + interval > Date.now()) {
      return {
        outcome: 'not_due', message: '自动备份尚未到期。', record: null, prunedCount: 0,
        lastBackupAt: latest.completedAt, nextDueAt: latest.completedAt + interval, warnings: [],
      }
    }
    const timestamp = Date.now()
    const created = await this.createBackup(`C:\\Users\\Teacher\\Documents\\教师题库\\backup\\TK试题题库_自动备份_${timestamp}.tqb`)
    created.record.backupKind = 'automatic'
    const stored = backups.find((backup) => backup.id === created.record.id)
    if (stored) stored.backupKind = 'automatic'
    const automatic = backups.filter((backup) => backup.backupKind === 'automatic')
      .sort((left, right) => right.createdAt - left.createdAt)
    const expired = automatic.slice(settings.automaticBackupRetentionCount)
    backups = backups.filter((backup) => !expired.some((item) => item.id === backup.id))
    persist()
    return {
      outcome: 'created', message: '自动备份已创建并完成校验。', record: clone(created.record),
      prunedCount: expired.length, lastBackupAt: timestamp, nextDueAt: timestamp + interval, warnings: [],
    }
  },

  async pickBackupFile(): Promise<string> {
    return 'C:\\Users\\Teacher\\Documents\\教师题库\\backup\\示例完整备份.tqb'
  },

  async inspectBackup(backupPath: string): Promise<BackupInspection> {
    if (!backupPath.toLocaleLowerCase('en-US').endsWith('.tqb')) {
      taxonomyError('所选文件不是 .tqb 题库备份。', 'BACKUP_ARCHIVE_REJECTED')
    }
    const record = backups[0]
    const archiveHash = record?.archiveSha256Hex ?? mockSha256Hex(backupPath)
    return {
      formatVersion: record?.formatVersion ?? 1,
      createdAt: record?.createdAt ?? Date.now(),
      sourceAppVersion: record?.sourceAppVersion ?? '0.1.0',
      databaseSchemaVersion: record?.databaseSchemaVersion ?? 8,
      sourceDatabaseUuid: record?.sourceDatabaseUuid ?? '01900000-0000-7000-8000-000000000001',
      archiveSha256Hex: archiveHash ?? mockSha256Hex(backupPath),
      manifestSha256Hex: record?.manifestSha256Hex ?? mockSha256Hex(`manifest\u0000${backupPath}`),
      archiveByteSize: record?.archiveByteSize ?? 188 * 1024 * 1024,
      payloadByteSize: record?.payloadByteSize ?? 186 * 1024 * 1024,
      payloadFileCount: record?.payloadFileCount ?? 24,
    }
  },

  async prepareRestore(backupPath: string): Promise<RestorePlan> {
    const inspection = await this.inspectBackup(backupPath)
    const token = crypto.randomUUID()
    const currentQuestionCount = questions.filter((question) => question.deletedAt == null).length
    const currentPaperCount = papers.filter((paper) => paper.status === 'saved').length
    const plan: RestorePlan = {
      restoreToken: token,
      expiresAt: Date.now() + 30 * 60 * 1000,
      sourceDisplayFilename: backupPath.split(/[\\/]/).pop() || '题库备份.tqb',
      archiveInspection: inspection,
      currentSummary: {
        databaseUuid: '01900000-0000-7000-8000-000000000001',
        databaseSchemaVersion: 8,
        questionCount: currentQuestionCount,
        paperCount: currentPaperCount,
        templateCount: templates.length,
        resourceCount: 8,
      },
      incomingSummary: {
        databaseUuid: inspection.sourceDatabaseUuid,
        databaseSchemaVersion: inspection.databaseSchemaVersion,
        questionCount: Math.max(currentQuestionCount, 308),
        paperCount: Math.max(currentPaperCount, 18),
        templateCount: Math.max(templates.length, 3),
        resourceCount: 126,
      },
      currentDatabaseHealthy: true,
      warnings: [
        '确认后会先为当前数据创建完整的恢复前备份，再重启并切换数据。',
        '恢复完成后会保留原数据目录；出现异常时会自动回滚。',
      ],
      automaticPreRestoreBackupRequired: true,
      requiresRestart: true,
    }
    preparedRestore = { path: backupPath, plan }
    return clone(plan)
  },

  async cancelRestore(restoreToken: string): Promise<void> {
    if (!preparedRestore) return
    if (preparedRestore.plan.restoreToken !== restoreToken) {
      taxonomyError('恢复确认信息已失效，请重新选择备份。', 'RESTORE_PLAN_INVALID')
    }
    preparedRestore = null
  },

  async scheduleRestore(request: ScheduleRestoreRequest): Promise<RestoreScheduled> {
    const prepared = preparedRestore
    if (!prepared || prepared.plan.restoreToken !== request.restoreToken) {
      taxonomyError('恢复确认信息已失效，请重新选择备份。', 'RESTORE_PLAN_INVALID')
    }
    if (Date.now() > prepared.plan.expiresAt) {
      taxonomyError('恢复确认已过期，请重新选择并校验备份文件。', 'RESTORE_PLAN_EXPIRED')
    }
    if (!request.confirmedCurrentDataReplacement) {
      taxonomyError('必须明确确认当前题库数据将被备份内容替换。', 'VALIDATION_ERROR')
    }
    if (request.expectedArchiveSha256Hex !== prepared.plan.archiveInspection.archiveSha256Hex) {
      taxonomyError('恢复确认的备份摘要与预检结果不一致。', 'RESTORE_CONFIRMATION_MISMATCH')
    }
    const operationId = prepared.plan.restoreToken
    const preRestoreBackupFilename = `TK试题题库_恢复前_${Date.now()}.tqb`
    lastRestoreResult = {
      operationId,
      outcome: 'success',
      restoredDatabaseUuid: prepared.plan.incomingSummary.databaseUuid,
      preRestoreBackupFilename,
      summary: clone(prepared.plan.incomingSummary),
      durationMs: 1250,
      warnings: ['浏览器预览只模拟重启后的成功结果，不会改写本机真实题库文件。'],
      errorMessage: null,
    }
    preparedRestore = null
    persist()
    return { operationId, preRestoreBackupFilename, requiresRestart: false }
  },

  async getLastRestoreResult(): Promise<RestoreResult | null> {
    return lastRestoreResult ? clone(lastRestoreResult) : null
  },

  async acknowledgeRestoreResult(operationId: string): Promise<void> {
    if (lastRestoreResult && lastRestoreResult.operationId !== operationId) {
      taxonomyError('恢复结果与确认请求不一致。', 'VALIDATION_ERROR')
    }
    lastRestoreResult = null
    persist()
  },

  async pickDataDirectory(): Promise<string> {
    return 'D:\\教师题库数据_空目录'
  },

  async pickExportDirectory(): Promise<string> {
    return 'D:\\教师资料\\题库导出'
  },

  async openDataDirectory(
    _component: 'root' | 'database' | 'resources' | 'templates' | 'backup' | 'export',
  ): Promise<void> {
    // Browser preview has no permission to open Windows Explorer.
  },

  async getManagedImage(resourceId: string): Promise<ManagedImagePayload> {
    const payload = managedImages.get(resourceId)
    if (!payload) taxonomyError(`浏览器预览中没有受管图片资源：${resourceId}`, 'RESOURCE_NOT_FOUND')
    return clone(payload)
  },

  async storeManagedImages(images: StoreManagedImageInput[]): Promise<ManagedImagePayload[]> {
    return images.map((image) => {
      const contentKey = image.dataBase64.trim()
      const existingId = managedImageIdsByContent.get(contentKey)
      if (existingId) return clone(managedImages.get(existingId)!)
      const resourceId = crypto.randomUUID()
      const payload: ManagedImagePayload = {
        resourceId,
        mimeType: contentKey.startsWith('/9j/') ? 'image/jpeg' : 'image/png',
        dataBase64: contentKey,
        widthPx: null,
        heightPx: null,
      }
      managedImageIdsByContent.set(contentKey, resourceId)
      managedImages.set(resourceId, payload)
      return clone(payload)
    })
  },

  async prepareDataMove(targetDataRoot: string): Promise<DataMovePlan> {
    const target = targetDataRoot.trim()
    if (!/^[a-z]:\\/i.test(target)) {
      taxonomyError('目标数据目录必须是 Windows 绝对路径。', 'VALIDATION_ERROR')
    }
    if (target.toLocaleLowerCase('zh-CN') === mockDataRoot.toLocaleLowerCase('zh-CN')) {
      taxonomyError('目标目录不能与当前数据目录相同。', 'VALIDATION_ERROR')
    }
    const plan: DataMovePlan = {
      moveToken: crypto.randomUUID(),
      expiresAt: Date.now() + 30 * 60 * 1000,
      sourceDataRoot: mockDataRoot,
      targetDataRoot: target,
      estimatedFileCount: 152,
      estimatedByteSize: 188 * 1024 * 1024,
      warnings: [
        '执行时会冻结写入，创建一致的数据库快照并逐文件校验。',
        '迁移成功后软件会自动重启；旧数据目录会原样保留。',
      ],
      requiresRestart: true,
    }
    preparedDataMove = plan
    return clone(plan)
  },

  async cancelDataMove(moveToken: string): Promise<void> {
    if (!preparedDataMove) return
    if (preparedDataMove.moveToken !== moveToken) {
      taxonomyError('数据迁移计划已失效，请重新选择目录。', 'DATA_MOVE_PLAN_INVALID')
    }
    preparedDataMove = null
  },

  async scheduleDataMove(request: ScheduleDataMoveRequest): Promise<DataMoveScheduled> {
    const plan = preparedDataMove
    if (!plan || plan.moveToken !== request.moveToken) {
      taxonomyError('数据迁移计划已失效，请重新选择目录。', 'DATA_MOVE_PLAN_INVALID')
    }
    if (Date.now() > plan.expiresAt) {
      taxonomyError('数据迁移确认已过期，请重新选择目录。', 'DATA_MOVE_PLAN_EXPIRED')
    }
    if (!request.confirmedKeepOldData) {
      taxonomyError('必须明确确认迁移完成后保留旧数据目录。', 'VALIDATION_ERROR')
    }
    if (request.expectedSourceDataRoot !== plan.sourceDataRoot
      || request.expectedTargetDataRoot !== plan.targetDataRoot) {
      taxonomyError('数据迁移确认与预检结果不一致。', 'DATA_MOVE_CONFIRMATION_MISMATCH')
    }
    const operationId = plan.moveToken
    const oldRoot = plan.sourceDataRoot
    mockDataRoot = plan.targetDataRoot
    lastDataMoveResult = {
      operationId,
      outcome: 'success',
      activeDataRoot: mockDataRoot,
      retainedDataRoot: oldRoot,
      copiedFileCount: plan.estimatedFileCount,
      copiedByteSize: plan.estimatedByteSize,
      durationMs: 1680,
      warnings: ['浏览器预览只模拟目录切换，不会复制或移动本机文件。'],
      errorMessage: null,
    }
    preparedDataMove = null
    persist()
    return {
      operationId,
      sourceDataRoot: oldRoot,
      targetDataRoot: mockDataRoot,
      copiedFileCount: plan.estimatedFileCount,
      copiedByteSize: plan.estimatedByteSize,
      requiresRestart: false,
    }
  },

  async getLastDataMoveResult(): Promise<DataMoveResult | null> {
    return lastDataMoveResult ? clone(lastDataMoveResult) : null
  },

  async acknowledgeDataMoveResult(operationId: string): Promise<void> {
    if (lastDataMoveResult && lastDataMoveResult.operationId !== operationId) {
      taxonomyError('数据迁移结果与确认请求不一致。', 'VALIDATION_ERROR')
    }
    lastDataMoveResult = null
    persist()
  },

  async createInitialSubject(value: string): Promise<void> {
    const name = validatedName(value, '学科', SUBJECT_NAME_MAX)
    if (subjects.some((subject) => nameKey(subject.name) === nameKey(name))) {
      taxonomyError('已经存在同名学科。')
    }
    const subjectId = crypto.randomUUID()
    subjects.push({
      id: subjectId,
      name,
      sortOrder: Math.max(0, ...subjects.map((subject) => subject.sortOrder)) + SORT_STEP,
      questionCount: 0,
      lastAccessedAt: null,
      chapters: [{
        id: crypto.randomUUID(),
        subjectId,
        name: '未分类',
        sortOrder: SORT_STEP,
        questionCount: 0,
        lastAccessedAt: null,
      }],
    })
    persist()
  },

  async createSubject(value: string): Promise<string> {
    const name = validatedName(value, '学科', SUBJECT_NAME_MAX)
    if (subjects.some((subject) => nameKey(subject.name) === nameKey(name))) {
      taxonomyError('已经存在同名学科。')
    }
    const id = crypto.randomUUID()
    subjects.push({
      id,
      name,
      sortOrder: Math.max(0, ...subjects.map((subject) => subject.sortOrder)) + SORT_STEP,
      questionCount: 0,
      lastAccessedAt: null,
      chapters: [],
    })
    persist()
    return id
  },

  async updateSubject(id: string, value: string): Promise<void> {
    const subject = subjects.find((item) => item.id === id)
    if (!subject) taxonomyError('找不到指定学科，请刷新后重试。', 'NOT_FOUND')
    const name = validatedName(value, '学科', SUBJECT_NAME_MAX)
    if (subjects.some((item) => item.id !== id && nameKey(item.name) === nameKey(name))) {
      taxonomyError('已经存在同名学科。')
    }
    subject.name = name
    questions = questions.map((question) => question.subjectId === id ? { ...question, subjectName: name } : question)
    persist()
  },

  async deleteSubject(id: string): Promise<void> {
    const subject = subjects.find((item) => item.id === id)
    if (!subject) taxonomyError('找不到指定学科，请刷新后重试。', 'NOT_FOUND')
    const questionCount = questions.filter((question) => question.subjectId === id).length
    if (subject.chapters.length || questionCount) {
      taxonomyError(`该学科下还有 ${subject.chapters.length} 个章节、${questionCount} 道题，请先移动或删除关联内容。`)
    }
    subjects = subjects.filter((item) => item.id !== id)
    persist()
  },

  async createChapter(subjectId: string, value: string): Promise<string> {
    const subject = subjects.find((item) => item.id === subjectId)
    if (!subject) taxonomyError('找不到指定学科，请刷新后重试。', 'NOT_FOUND')
    const name = validatedName(value, '章节', CHAPTER_NAME_MAX)
    if (subject.chapters.some((chapter) => nameKey(chapter.name) === nameKey(name))) {
      taxonomyError('这个学科中已经存在同名章节。')
    }
    const id = crypto.randomUUID()
    subject.chapters.push({
      id,
      subjectId,
      name,
      sortOrder: Math.max(0, ...subject.chapters.map((chapter) => chapter.sortOrder)) + SORT_STEP,
      questionCount: 0,
      lastAccessedAt: null,
    })
    persist()
    return id
  },

  async updateChapter(id: string, value: string): Promise<void> {
    const subject = subjects.find((item) => item.chapters.some((chapter) => chapter.id === id))
    const chapter = subject?.chapters.find((item) => item.id === id)
    if (!subject || !chapter) taxonomyError('找不到指定章节，请刷新后重试。', 'NOT_FOUND')
    const name = validatedName(value, '章节', CHAPTER_NAME_MAX)
    if (subject.chapters.some((item) => item.id !== id && nameKey(item.name) === nameKey(name))) {
      taxonomyError('这个学科中已经存在同名章节。')
    }
    chapter.name = name
    questions = questions.map((question) => question.chapterId === id ? { ...question, chapterName: name } : question)
    persist()
  },

  async deleteChapter(id: string): Promise<void> {
    const subject = subjects.find((item) => item.chapters.some((chapter) => chapter.id === id))
    const chapter = subject?.chapters.find((item) => item.id === id)
    if (!subject || !chapter) taxonomyError('找不到指定章节，请刷新后重试。', 'NOT_FOUND')
    const questionCount = questions.filter((question) => question.chapterId === id).length
    if (questionCount) taxonomyError(`该章节中还有 ${questionCount} 道题，请先将题目移动到其他章节。`)
    subject.chapters = subject.chapters.filter((item) => item.id !== id)
    persist()
  },

  async createTag(value: string): Promise<string> {
    const name = validatedName(value, '标签', TAG_NAME_MAX)
    if (tags.some((tag) => nameKey(tag.name) === nameKey(name))) taxonomyError('已经存在同名标签。')
    const id = crypto.randomUUID()
    const timestamp = Date.now()
    tags.push({ id, name, questionCount: 0, createdAt: timestamp, updatedAt: timestamp })
    persist()
    return id
  },

  async updateTag(id: string, value: string): Promise<void> {
    const tag = tags.find((item) => item.id === id)
    if (!tag) taxonomyError('找不到指定标签，请刷新后重试。', 'NOT_FOUND')
    const name = validatedName(value, '标签', TAG_NAME_MAX)
    if (tags.some((item) => item.id !== id && nameKey(item.name) === nameKey(name))) {
      taxonomyError('已经存在同名标签。')
    }
    tag.name = name
    tag.updatedAt = Date.now()
    questions = questions.map((question) => ({
      ...question,
      tags: question.tags.map((item) => item.id === id ? { ...item, name, updatedAt: tag.updatedAt } : item),
    }))
    persist()
  },

  async deleteTag(id: string): Promise<void> {
    if (!tags.some((item) => item.id === id)) taxonomyError('找不到指定标签，请刷新后重试。', 'NOT_FOUND')
    tags = tags.filter((item) => item.id !== id)
    questions = questions.map((question) => ({
      ...question,
      tags: question.tags.filter((tag) => tag.id !== id),
    }))
    persist()
  },

  async saveSubjectOrder(items: TaxonomyOrderItem[]): Promise<void> {
    assertExactOrder(items, subjects.map((subject) => subject.id), '学科')
    const positions = new Map(items.map((item) => [item.id, item.sortOrder]))
    subjects.forEach((subject) => { subject.sortOrder = positions.get(subject.id) ?? subject.sortOrder })
    persist()
  },

  async saveChapterOrder(subjectId: string, items: TaxonomyOrderItem[]): Promise<void> {
    const subject = subjects.find((item) => item.id === subjectId)
    if (!subject) taxonomyError('找不到指定学科，请刷新后重试。', 'NOT_FOUND')
    assertExactOrder(items, subject.chapters.map((chapter) => chapter.id), '章节')
    const positions = new Map(items.map((item) => [item.id, item.sortOrder]))
    subject.chapters.forEach((chapter) => { chapter.sortOrder = positions.get(chapter.id) ?? chapter.sortOrder })
    persist()
  },

  async listPapers(filters: PaperFilters): Promise<PageResult<PaperSummary>> {
    const keyword = filters.keyword.trim().normalize('NFKC').toLocaleLowerCase('zh-CN')
    let result = papers.filter((paper) => filters.status === 'all' || paper.status === filters.status)
    if (keyword) {
      result = result.filter((paper) => paper.title.normalize('NFKC').toLocaleLowerCase('zh-CN').includes(keyword))
    }
    result.sort((left, right) => {
      const leftTime = left.status === 'saved' ? (left.savedAt ?? left.updatedAt) : left.updatedAt
      const rightTime = right.status === 'saved' ? (right.savedAt ?? right.updatedAt) : right.updatedAt
      return rightTime - leftTime || left.id.localeCompare(right.id)
    })
    const page = Math.max(1, filters.page)
    const pageSize = Math.min(500, Math.max(1, filters.pageSize))
    const start = (page - 1) * pageSize
    return {
      items: result.slice(start, start + pageSize).map((paper) => paperSummary(paper)),
      total: result.length,
      page,
      pageSize,
    }
  },

  async getPaper(id: string): Promise<Paper | null> {
    const paper = papers.find((item) => item.id === id)
    return paper ? clone(paper) : null
  },

  async savePaper(input: Paper): Promise<Paper> {
    const title = validatePaper(input)
    const timestamp = Date.now()
    const existingIndex = papers.findIndex((paper) => paper.id === input.id)
    const existing = existingIndex >= 0 ? papers[existingIndex] : null
    if (!existing && input.rowVersion !== 0) taxonomyError('这份试卷已不存在，请返回试卷列表后重试。', 'PAPER_NOT_FOUND')
    if (existing && existing.rowVersion !== input.rowVersion) {
      taxonomyError('这份试卷已在其他窗口中更新，请重新打开后再操作。', 'PAPER_VERSION_CONFLICT')
    }
    if (existing?.status === 'saved' && input.status === 'draft') {
      taxonomyError('已保存的历史试卷不能改回草稿；请使用“再次编辑”创建副本。')
    }

    const saved: Paper = {
      ...clone(input),
      id: input.id || crypto.randomUUID(),
      title,
      generationConfig: input.compositionMode === 'automatic'
        ? clone(input.generationConfig ?? null)
        : null,
      subjectSummaryText: paperSubjectSummary(input),
      rowVersion: (existing?.rowVersion ?? 0) + 1,
      createdAt: existing?.createdAt ?? timestamp,
      updatedAt: timestamp,
      savedAt: input.status === 'saved' ? (existing?.savedAt ?? timestamp) : null,
      lastSavedAt: timestamp,
      items: input.items.map((item, position) => ({ ...clone(item), position })),
    }
    if (existingIndex >= 0) papers.splice(existingIndex, 1, saved)
    else papers.push(saved)
    if (saved.status === 'saved') {
      const usedIds = new Set(saved.items.map((item) => item.sourceQuestionId).filter(Boolean))
      questions = questions.map((question) => usedIds.has(question.id)
        ? { ...question, lastUsedAt: timestamp }
        : question)
    }
    persist()
    return clone(saved)
  },

  async copyPaper(id: string, baseRowVersion: number): Promise<Paper> {
    const source = papers.find((paper) => paper.id === id)
    if (!source) taxonomyError('找不到要复制的试卷。', 'PAPER_NOT_FOUND')
    if (source.rowVersion !== baseRowVersion) {
      taxonomyError('这份试卷已在其他窗口中更新，请重新打开后再操作。', 'PAPER_VERSION_CONFLICT')
    }
    const timestamp = Date.now()
    const suffix = '（副本）'
    const title = `${[...source.title].slice(0, 200 - [...suffix].length).join('')}${suffix}`
    const copied: Paper = {
      ...clone(source),
      id: crypto.randomUUID(),
      title,
      status: 'draft',
      rowVersion: 1,
      createdAt: timestamp,
      updatedAt: timestamp,
      savedAt: null,
      lastSavedAt: timestamp,
      items: source.items.map((item, position) => ({
        ...clone(item),
        id: crypto.randomUUID(),
        position,
      })),
    }
    papers.push(copied)
    persist()
    return clone(copied)
  },

  async deletePaper(id: string, baseRowVersion: number): Promise<void> {
    const existing = papers.find((paper) => paper.id === id)
    if (!existing) taxonomyError('找不到要删除的试卷。', 'PAPER_NOT_FOUND')
    if (existing.rowVersion !== baseRowVersion) {
      taxonomyError('这份试卷已在其他窗口中更新，请重新打开后再操作。', 'PAPER_VERSION_CONFLICT')
    }
    papers = papers.filter((paper) => paper.id !== id)
    persist()
  },

  async deletePapers(requests: PaperDeleteRequest[]): Promise<void> {
    if (!requests.length) return
    const seenIds = new Set<string>()
    for (const request of requests) {
      if (seenIds.has(request.id)) {
        taxonomyError('批量删除中包含重复的试卷。', 'VALIDATION_ERROR')
      }
      seenIds.add(request.id)
      const existing = papers.find((paper) => paper.id === request.id)
      if (!existing) taxonomyError('找不到要删除的试卷。', 'PAPER_NOT_FOUND')
      if (existing.rowVersion !== request.baseRowVersion) {
        taxonomyError('这份试卷已在其他窗口中更新，请重新打开后再操作。', 'PAPER_VERSION_CONFLICT')
      }
    }
    papers = papers.filter((paper) => !seenIds.has(paper.id))
    persist()
  },

  async listQuestions(filters: QuestionFilters): Promise<PageResult<Question>> {
    const keyword = filters.keyword.trim().toLocaleLowerCase('zh-CN')
    const currentTime = Date.now()
    let result = questions.filter((question) => Boolean(question.deletedAt) === filters.deleted)

    if (keyword) {
      result = result.filter((question) => {
        const searchable = [
          question.stem.plainText,
          question.options.map((option) => option.content.plainText).join(' '),
          question.answer.plainText,
          question.explanation.plainText,
          question.tags.map((tag) => tag.name).join(' '),
        ].join(' ').toLocaleLowerCase('zh-CN')
        return searchable.includes(keyword)
      })
    }
    if (filters.subjectId) result = result.filter((question) => question.subjectId === filters.subjectId)
    if (filters.chapterId) result = result.filter((question) => question.chapterId === filters.chapterId)
    if (filters.type) result = result.filter((question) => question.type === filters.type)
    if (filters.tagIds.length) {
      const matchesTag = filters.tagMatchMode === 'any'
        ? (question: Question) => filters.tagIds.some((tagId) => question.tags.some((tag) => tag.id === tagId))
        : (question: Question) => filters.tagIds.every((tagId) => question.tags.some((tag) => tag.id === tagId))
      result = result.filter(matchesTag)
    }
    result = result.filter((question) => matchesQuestionUsage(question.lastUsedAt, filters.usage, currentTime))

    result.sort((a, b) => b.updatedAt - a.updatedAt)
    const start = (filters.page - 1) * filters.pageSize
    return {
      items: result.slice(start, start + filters.pageSize),
      total: result.length,
      page: filters.page,
      pageSize: filters.pageSize,
    }
  },

  async getQuestion(id: string): Promise<Question | null> {
    return questions.find((question) => question.id === id) ?? null
  },

  async checkQuestionDuplicate(draft: QuestionDraft): Promise<QuestionDuplicateCheck> {
    validateDuplicateDraft(draft)
    const active = questions
      .filter((question) => !question.deletedAt && question.id !== draft.questionId)
      .sort((left, right) => right.updatedAt - left.updatedAt || left.id.localeCompare(right.id))
    const exactKey = normalizedExactQuestion(draft)
    const exact = active.find((question) => normalizedExactQuestion(question) === exactKey)
    if (exact) {
      return {
        status: 'exact',
        candidate: duplicateCandidate(exact, 100),
        evaluatedCandidateCount: 1,
        suspectedThresholdPercent: SUSPECTED_DUPLICATE_THRESHOLD_PERCENT,
        similarityMethod: SIMILARITY_METHOD,
      }
    }

    const inputText = similarityText(draft)
    const inputPrefilterLength = similarityPrefilterLength(draft)
    const candidates = active
      .map((question) => ({ question, text: similarityText(question) }))
      .sort((left, right) => {
        return Math.abs(similarityPrefilterLength(left.question) - inputPrefilterLength)
          - Math.abs(similarityPrefilterLength(right.question) - inputPrefilterLength)
          || right.question.updatedAt - left.question.updatedAt
          || left.question.id.localeCompare(right.question.id)
      })
      .slice(0, MAX_DUPLICATE_CANDIDATES)

    let best: { question: Question; similarityPercent: number } | null = null
    for (const candidate of candidates) {
      const similarityPercent = characterBigramDicePercent(inputText, candidate.text)
      if (!best || similarityPercent > best.similarityPercent) {
        best = { question: candidate.question, similarityPercent }
      }
    }
    return {
      status: best && best.similarityPercent >= SUSPECTED_DUPLICATE_THRESHOLD_PERCENT
        ? 'suspected'
        : 'none',
      candidate: best ? duplicateCandidate(best.question, best.similarityPercent) : null,
      evaluatedCandidateCount: candidates.length,
      suspectedThresholdPercent: SUSPECTED_DUPLICATE_THRESHOLD_PERCENT,
      similarityMethod: SIMILARITY_METHOD,
    }
  },

  async checkQuestionDuplicatesBatch(request: QuestionDuplicateBatchRequest): Promise<QuestionDuplicateBatchResult> {
    const maxItems = request.mode === 'import' ? 5_000 : 100
    if (!request.items.length || request.items.length > maxItems) {
      taxonomyError(`当前模式单次批量重复检查需要包含 1 到 ${maxItems} 道题目。`)
    }
    const seenIds = new Set<string>()
    const earlierByFingerprint = new Map<string, number>()
    const lengths = request.items.map((entry) => similarityPrefilterLength(entry.draft))
    const sortedIndices = request.items.map((_, index) => index)
      .sort((left, right) => lengths[left]! - lengths[right]! || left - right)
    const positions = new Map(sortedIndices.map((index, position) => [index, position]))
    const results = []
    for (const [index, entry] of request.items.entries()) {
      if (seenIds.has(entry.clientId)) taxonomyError('批量重复检查的题目标识不能重复。')
      seenIds.add(entry.clientId)
      const exactFingerprint = normalizedExactQuestion(entry.draft)
      const previousIndex = earlierByFingerprint.get(exactFingerprint)
      let check: QuestionDuplicateCheck
      if (request.mode === 'import') {
        if (previousIndex !== undefined) {
          const previous = request.items[previousIndex]!
          check = {
            status: 'exact',
            candidate: {
              id: previous.clientId,
              type: previous.draft.type,
              stemPreview: duplicateStemPreview(previous.draft.stem.plainText),
              subjectName: '本批导入',
              chapterName: `第 ${previousIndex + 1} 题`,
              similarityPercent: 100,
              contentVersion: 0,
              sourceKind: 'import-item',
              sourceOrdinal: previousIndex + 1,
            },
            evaluatedCandidateCount: 1,
            suspectedThresholdPercent: SUSPECTED_DUPLICATE_THRESHOLD_PERCENT,
            similarityMethod: SIMILARITY_METHOD,
          }
        } else {
          const position = positions.get(index) ?? 0
          const candidates: number[] = []
          let left = position - 1
          let right = position + 1
          while (candidates.length < MAX_DUPLICATE_CANDIDATES && (left >= 0 || right < sortedIndices.length)) {
            const leftIndex = left >= 0 ? sortedIndices[left] : undefined
            const rightIndex = right < sortedIndices.length ? sortedIndices[right] : undefined
            const takeLeft = leftIndex !== undefined && (rightIndex === undefined
              || Math.abs(lengths[leftIndex]! - lengths[index]!) <= Math.abs(lengths[rightIndex]! - lengths[index]!))
            const candidateIndex = takeLeft ? leftIndex! : rightIndex!
            if (takeLeft) left -= 1
            else right += 1
            if (candidateIndex < index) candidates.push(candidateIndex)
          }
          const inputText = similarityText(entry.draft)
          let best: { index: number; similarityPercent: number } | null = null
          for (const candidateIndex of candidates) {
            const similarityPercent = characterBigramDicePercent(
              inputText,
              similarityText(request.items[candidateIndex]!.draft),
            )
            if (!best || similarityPercent > best.similarityPercent) {
              best = { index: candidateIndex, similarityPercent }
            }
          }
          if (best && best.similarityPercent >= SUSPECTED_DUPLICATE_THRESHOLD_PERCENT) {
            const previous = request.items[best.index]!
            check = {
              status: 'suspected',
              candidate: {
                id: previous.clientId,
                type: previous.draft.type,
                stemPreview: duplicateStemPreview(previous.draft.stem.plainText),
                subjectName: '本批导入',
                chapterName: `第 ${best.index + 1} 题`,
                similarityPercent: best.similarityPercent,
                contentVersion: 0,
                sourceKind: 'import-item',
                sourceOrdinal: best.index + 1,
              },
              evaluatedCandidateCount: candidates.length,
              suspectedThresholdPercent: SUSPECTED_DUPLICATE_THRESHOLD_PERCENT,
              similarityMethod: SIMILARITY_METHOD,
            }
          } else {
            check = {
              status: 'none',
              candidate: null,
              evaluatedCandidateCount: candidates.length,
              suspectedThresholdPercent: SUSPECTED_DUPLICATE_THRESHOLD_PERCENT,
              similarityMethod: SIMILARITY_METHOD,
            }
          }
        }
      } else {
        const databaseCheck = await this.checkQuestionDuplicate(entry.draft)
        check = request.mode === 'exact' && databaseCheck.status !== 'exact'
          ? {
              status: 'none',
              candidate: null,
              evaluatedCandidateCount: 0,
              suspectedThresholdPercent: SUSPECTED_DUPLICATE_THRESHOLD_PERCENT,
              similarityMethod: SIMILARITY_METHOD,
            }
          : databaseCheck
      }
      if (request.mode !== 'import' && check.status !== 'exact' && previousIndex !== undefined) {
        const previous = request.items[previousIndex]!
        check = {
          status: 'exact',
          candidate: {
            id: previous.clientId,
            type: previous.draft.type,
            stemPreview: duplicateStemPreview(previous.draft.stem.plainText),
            subjectName: '本批导入',
            chapterName: `第 ${previousIndex + 1} 题`,
            similarityPercent: 100,
            contentVersion: 0,
            sourceKind: 'import-item',
            sourceOrdinal: previousIndex + 1,
          },
          evaluatedCandidateCount: 1,
          suspectedThresholdPercent: SUSPECTED_DUPLICATE_THRESHOLD_PERCENT,
          similarityMethod: SIMILARITY_METHOD,
        }
      }
      if (!earlierByFingerprint.has(exactFingerprint)) earlierByFingerprint.set(exactFingerprint, index)
      results.push({ clientId: entry.clientId, exactFingerprint, check })
    }
    return { items: results }
  },

  async cancelQuestionDuplicateScan(_scanId: string): Promise<void> {
    // Browser demo checks are synchronous and finish before another event can
    // request cancellation. The desktop backend performs cooperative cancel.
  },

  async analyzeRandomDraw(scope: RandomDrawScope): Promise<RandomDrawAnalysis> {
    const candidates = randomDrawCandidates(scope)
    const availableByType = candidates.reduce<Record<string, number>>((counts, question) => {
      counts[question.type] = (counts[question.type] ?? 0) + 1
      return counts
    }, {})
    return {
      availableTotal: candidates.length,
      availableByType,
    }
  },

  async drawRandomQuestions(request: RandomDrawRequest): Promise<Question[]> {
    const candidates = randomDrawCandidates(request.scope)
    if (request.countMode === 'total') {
      return shuffledQuestions(candidates).slice(0, Math.max(0, request.totalCount))
    }
    const selected: Question[] = []
    for (const [type, requested] of Object.entries(request.questionTypeCounts)) {
      if (!requested) continue
      selected.push(...shuffledQuestions(candidates.filter((question) => question.type === type)).slice(0, requested))
    }
    return selected
  },

  async scanQuestionDuplicates(request: QuestionDuplicateScanRequest): Promise<QuestionDuplicateScanResult> {
    const subject = subjects.find((entry) => entry.id === request.subjectId)
    const chapter = request.chapterId
      ? subject?.chapters.find((entry) => entry.id === request.chapterId)
      : null
    if (!subject || (request.chapterId && !chapter)) {
      taxonomyError('要查重的学科或章节已经不存在，请刷新后重试。', 'DUPLICATE_SCAN_SCOPE_NOT_FOUND')
    }

    const active = questions.filter((question) => !question.deletedAt)
    const scope = active.filter((question) => (
      question.subjectId === request.subjectId
      && (!request.chapterId || question.chapterId === request.chapterId)
    ))
    const scopeIds = new Set(scope.map((question) => question.id))
    const exactByKey = new Map<string, Question[]>()
    for (const question of active) {
      const key = normalizedExactQuestion(question)
      const members = exactByKey.get(key) ?? []
      members.push(question)
      exactByKey.set(key, members)
    }
    const exactGroups: QuestionDuplicateGroup[] = [...exactByKey.values()]
      .filter((members) => members.length > 1 && members.some((member) => scopeIds.has(member.id)))
      .map((members) => {
        const ordered = [...members].sort((left, right) => left.createdAt - right.createdAt || left.id.localeCompare(right.id))
        return {
          id: `exact:${ordered[0]!.id}`,
          duplicateKind: 'exact' as const,
          similarityPercent: 100,
          members: ordered.map(duplicateMember),
        }
      })
      .sort((left, right) => left.id.localeCompare(right.id))

    const seenPairs = new Set<string>()
    const suspectedGroups: QuestionDuplicateGroup[] = []
    for (const question of scope) {
      const inputLength = similarityPrefilterLength(question)
      const candidates = active
        .filter((candidate) => candidate.id !== question.id)
        .sort((left, right) => (
          Math.abs(similarityPrefilterLength(left) - inputLength)
          - Math.abs(similarityPrefilterLength(right) - inputLength)
          || right.updatedAt - left.updatedAt
          || left.id.localeCompare(right.id)
        ))
        .slice(0, MAX_DUPLICATE_CANDIDATES)
      const inputText = similarityText(question)
      for (const candidate of candidates) {
        const pairIds = [question.id, candidate.id].sort()
        const pairKey = `${pairIds[0]}:${pairIds[1]}`
        if (seenPairs.has(pairKey)
          || normalizedExactQuestion(question) === normalizedExactQuestion(candidate)
          || isIgnoredDuplicatePair(question, candidate)) {
          continue
        }
        seenPairs.add(pairKey)
        const similarityPercent = characterBigramDicePercent(inputText, similarityText(candidate))
        if (similarityPercent < SUSPECTED_DUPLICATE_THRESHOLD_PERCENT) continue
        const members = [question, candidate]
          .sort((left, right) => left.createdAt - right.createdAt || left.id.localeCompare(right.id))
        suspectedGroups.push({
          id: `suspected:${pairKey}`,
          duplicateKind: 'suspected',
          similarityPercent,
          members: members.map(duplicateMember),
        })
      }
    }
    suspectedGroups.sort((left, right) => (
      right.similarityPercent - left.similarityPercent || left.id.localeCompare(right.id)
    ))
    return {
      scannedQuestionCount: scope.length,
      comparedQuestionCount: active.length,
      suspectedThresholdPercent: SUSPECTED_DUPLICATE_THRESHOLD_PERCENT,
      exactGroups,
      suspectedGroups,
    }
  },

  async ignoreQuestionDuplicate(request: IgnoreQuestionDuplicateRequest): Promise<void> {
    const first = questions.find((question) => !question.deletedAt && question.id === request.firstQuestionId)
    const second = questions.find((question) => !question.deletedAt && question.id === request.secondQuestionId)
    if (!first || !second || first.id === second.id) {
      taxonomyError('其中一道题已经不存在，请重新查重。', 'DUPLICATE_PAIR_STALE')
    }
    if (first.contentVersion !== request.firstContentVersion
      || second.contentVersion !== request.secondContentVersion) {
      taxonomyError('其中一道题已被修改，请重新查重后再判断。', 'DUPLICATE_PAIR_STALE')
    }
    if (normalizedExactQuestion(first) === normalizedExactQuestion(second)) {
      taxonomyError('完全重复题不能标记为非重复，请选择保留项并将其余题目移入回收站。', 'DUPLICATE_PAIR_EXACT')
    }
    const pair = canonicalDuplicatePair(first, second)
    duplicateIgnores = duplicateIgnores.filter((ignored) => (
      ignored.lowId !== pair.lowId || ignored.highId !== pair.highId
    ))
    duplicateIgnores.push(pair)
    persist()
  },

  async getQuestionDraft(questionId?: string | null): Promise<QuestionDraftRecord | null> {
    const draftKey = questionDraftKey(questionId)
    const record = questionDrafts.find((item) => item.draftKey === draftKey)
    return record ? questionDraftWithStale(record) : null
  },

  async listQuestionDrafts(): Promise<QuestionDraftRecord[]> {
    return questionDrafts
      .map(questionDraftWithStale)
      .sort((left, right) => right.updatedAt - left.updatedAt || left.id.localeCompare(right.id))
  },

  async saveQuestionDraft(input: QuestionDraft): Promise<QuestionDraftRecord> {
    const payload = normalizedQuestionDraft(input)
    const draftKey = questionDraftKey(payload.questionId)
    const existingIndex = questionDrafts.findIndex((item) => item.draftKey === draftKey)
    const existing = existingIndex >= 0 ? questionDrafts[existingIndex] : null
    const timestamp = Date.now()
    const record: QuestionDraftRecord = {
      id: existing?.id ?? crypto.randomUUID(),
      draftKey,
      draftKind: payload.questionId ? 'question_edit' : 'question_create',
      targetQuestionId: payload.questionId ?? null,
      baseContentVersion: payload.baseContentVersion ?? null,
      payloadSchemaVersion: 1,
      payload,
      autosavedAt: timestamp,
      createdAt: existing?.createdAt ?? timestamp,
      updatedAt: timestamp,
      stale: false,
    }
    if (existingIndex >= 0) questionDrafts.splice(existingIndex, 1, record)
    else questionDrafts.push(record)
    persist()
    return questionDraftWithStale(record)
  },

  async deleteQuestionDraft(questionId?: string | null): Promise<void> {
    const draftKey = questionDraftKey(questionId)
    questionDrafts = questionDrafts.filter((item) => item.draftKey !== draftKey)
    persist()
  },

  async getDocumentQuestionDraft(): Promise<DocumentQuestionDraftRecord | null> {
    return documentQuestionDraft ? clone(documentQuestionDraft) : null
  },

  async saveDocumentQuestionDraft(
    input: DocumentQuestionDraftPayload,
  ): Promise<DocumentQuestionDraftRecord> {
    const payload = clone(input)
    if (payload.schemaVersion === 1) {
      if (!payload.items.length || payload.items.length > 100) {
        taxonomyError('文档式录入一次必须保留 1 至 100 道题。')
      }
      const ids = new Set<string>()
      payload.items.forEach((item, index) => {
        if (!item.id || ids.has(item.id) || item.ordinal !== index + 1) {
          taxonomyError('文档式录入草稿中的题目 ID 或顺序无效。')
        }
        ids.add(item.id)
        if (item.payload.questionId || item.payload.baseContentVersion) {
          taxonomyError('文档式录入草稿只能保存尚未入库的新题。')
        }
      })
      if (payload.activeItemId && !ids.has(payload.activeItemId)) {
        taxonomyError('当前题目不属于这份文档式录入草稿。')
      }
    } else {
      const validLegacy = payload.schemaVersion === 2
        && payload.templateId === 'labeled_fields_v1'
      const validCurrent = payload.schemaVersion === 3
        && Boolean(payload.templateId)
        && Boolean(payload.templateNameSnapshot.trim())
        && !validateDocumentEntryTemplateConfig(payload.templateConfigSnapshot)
      if (
        (!validLegacy && !validCurrent)
        || !payload.source
        || payload.source.schemaVersion !== 2
        || payload.source.editor !== 'tiptap'
        || payload.source.document?.type !== 'doc'
      ) {
        taxonomyError('自由文档录入草稿的数据版本或模板无效。')
      }
    }
    if (new Blob([JSON.stringify(payload)]).size > 32 * 1024 * 1024) {
      taxonomyError('文档式录入草稿超过 32 MB，请先保存部分题目后再继续录入。')
    }
    const timestamp = Date.now()
    const record: DocumentQuestionDraftRecord = {
      id: documentQuestionDraft?.id ?? crypto.randomUUID(),
      payload,
      autosavedAt: timestamp,
      createdAt: documentQuestionDraft?.createdAt ?? timestamp,
      updatedAt: timestamp,
    }
    documentQuestionDraft = record
    persist()
    return clone(record)
  },

  async deleteDocumentQuestionDraft(): Promise<void> {
    documentQuestionDraft = null
    persist()
  },

  async listDocumentEntryTemplates(): Promise<DocumentEntryTemplate[]> {
    return clone(documentEntryTemplates)
      .sort((left, right) => Number(right.isDefault) - Number(left.isDefault)
        || Number(right.isBuiltIn) - Number(left.isBuiltIn)
        || right.updatedAt - left.updatedAt)
  },

  async saveDocumentEntryTemplate(
    input: SaveDocumentEntryTemplateRequest,
  ): Promise<DocumentEntryTemplate> {
    const request = clone(input)
    const name = validatedName(request.name, '录题模板', 40)
    const configError = validateDocumentEntryTemplateConfig(request.config)
    if (configError) taxonomyError(configError)
    const existing = request.id
      ? documentEntryTemplates.find((template) => template.id === request.id)
      : null
    if (request.id && !existing) taxonomyError('要修改的录题模板已经不存在。')
    if (existing?.isBuiltIn) taxonomyError('系统默认模板不能直接修改，请先复制成自定义模板。')
    if (!existing && documentEntryTemplates.length >= 50) {
      taxonomyError('录题模板最多保存 50 套，请删除不再使用的模板后重试。')
    }
    if (documentEntryTemplates.some((template) => (
      template.id !== request.id && nameKey(template.name) === nameKey(name)
    ))) taxonomyError('已经存在同名录题模板。')

    const timestamp = Date.now()
    const saved: DocumentEntryTemplate = {
      id: existing?.id ?? crypto.randomUUID(),
      name,
      config: normalizeDocumentEntryTemplateConfig(request.config),
      isBuiltIn: false,
      isDefault: request.setAsDefault || Boolean(existing?.isDefault),
      createdAt: existing?.createdAt ?? timestamp,
      updatedAt: timestamp,
    }
    if (saved.isDefault) {
      documentEntryTemplates = documentEntryTemplates.map((template) => ({
        ...template,
        isDefault: false,
      }))
    }
    const existingIndex = documentEntryTemplates.findIndex((template) => template.id === saved.id)
    if (existingIndex >= 0) documentEntryTemplates.splice(existingIndex, 1, saved)
    else documentEntryTemplates.push(saved)
    persist()
    return clone(saved)
  },

  async setDefaultDocumentEntryTemplate(id: string): Promise<DocumentEntryTemplate> {
    const selected = documentEntryTemplates.find((template) => template.id === id)
    if (!selected) taxonomyError('要设为默认的录题模板已经不存在。')
    const timestamp = Date.now()
    documentEntryTemplates = documentEntryTemplates.map((template) => ({
      ...template,
      isDefault: template.id === id,
      updatedAt: template.id === id ? timestamp : template.updatedAt,
    }))
    persist()
    return clone(documentEntryTemplates.find((template) => template.id === id)!)
  },

  async deleteDocumentEntryTemplate(id: string): Promise<void> {
    const selected = documentEntryTemplates.find((template) => template.id === id)
    if (!selected) taxonomyError('要删除的录题模板已经不存在。')
    if (selected.isBuiltIn) taxonomyError('系统默认模板不能删除。')
    documentEntryTemplates = documentEntryTemplates.filter((template) => template.id !== id)
    if (selected.isDefault) {
      documentEntryTemplates = documentEntryTemplates.map((template) => ({
        ...template,
        isDefault: template.isBuiltIn,
      }))
    }
    persist()
  },

  async getWordImportDraft(): Promise<WordImportDraftRecord | null> {
    return wordImportDraft ? clone(wordImportDraft) : null
  },

  async saveWordImportDraft(input: WordImportDraftPayload): Promise<WordImportDraftRecord> {
    const payload = normalizedWordImportDraftPayload(input)
    const timestamp = Date.now()
    const previous = wordImportDraft
    const next: WordImportDraftRecord = {
      id: wordImportDraft?.id ?? crypto.randomUUID(),
      payload,
      autosavedAt: timestamp,
      createdAt: wordImportDraft?.createdAt ?? timestamp,
      updatedAt: timestamp,
    }
    wordImportDraft = next
    try {
      persist()
    } catch (reason) {
      wordImportDraft = previous
      throw reason
    }
    return clone(next)
  },

  async deleteWordImportDraft(): Promise<void> {
    const previous = wordImportDraft
    wordImportDraft = null
    try {
      persist()
    } catch (reason) {
      wordImportDraft = previous
      throw reason
    }
  },

  async saveQuestion(draft: QuestionDraft): Promise<Question> {
    const subject = subjects.find((item) => item.id === draft.subjectId)
    const chapter = subject?.chapters.find((item) => item.id === draft.chapterId)
    if (!subject || !chapter) throw new Error('请选择有效的学科和章节')
    const selectedTags = tags.filter((tag) => draft.tagIds.includes(tag.id))
    const existingIndex = draft.questionId ? questions.findIndex((item) => item.id === draft.questionId) : -1
    const existing = existingIndex >= 0 ? questions[existingIndex] : null
    if (draft.questionId && !existing) {
      taxonomyError('这道题目已不存在，请返回题库后重试。', 'QUESTION_NOT_FOUND')
    }
    if (existing?.deletedAt) {
      taxonomyError('这道题已经进入回收站，不能被编辑或覆盖；请先恢复后再操作。', 'QUESTION_IN_RECYCLE')
    }
    if (existing && draft.baseContentVersion !== existing.contentVersion) {
      taxonomyError('这道题已在其他窗口中更新，请重新打开后再操作。', 'QUESTION_VERSION_CONFLICT')
    }
    const saved: Question = {
      id: existing?.id ?? crypto.randomUUID(),
      type: draft.type,
      stem: draft.stem,
      options: draft.options.map((option, position) => ({ ...option, position })),
      answer: draft.answer,
      explanation: draft.explanation,
      subjectId: draft.subjectId,
      chapterId: draft.chapterId,
      subjectName: subject.name,
      chapterName: chapter.name,
      tags: selectedTags,
      resourceRefs: clone(draft.resourceRefs ?? []),
      createdAt: existing?.createdAt ?? Date.now(),
      updatedAt: Date.now(),
      lastUsedAt: existing?.lastUsedAt ?? null,
      deletedAt: existing?.deletedAt ?? null,
      contentVersion: (existing?.contentVersion ?? 0) + 1,
    }
    if (existingIndex >= 0) questions.splice(existingIndex, 1, saved)
    else questions.unshift(saved)
    persist()
    return saved
  },

  async saveQuestions(drafts: QuestionDraft[]): Promise<Question[]> {
    if (!drafts.length) taxonomyError('请至少录入一道题目。')
    if (drafts.length > 100) taxonomyError('文档式录入一次最多保存 100 道题目。')
    const snapshot = clone(questions)
    const saved: Question[] = []
    try {
      for (const draft of drafts) saved.push(await this.saveQuestion(draft))
      return saved
    } catch (reason) {
      questions = snapshot
      persist()
      throw reason
    }
  },

  async saveImportedQuestions(drafts: QuestionDraft[]): Promise<string[]> {
    if (!drafts.length) taxonomyError('请至少导入一道题目。')
    if (drafts.length > 100) taxonomyError('批量导入一次最多保存 100 道题目。')
    const snapshot = clone(questions)
    const savedIds: string[] = []
    const importIds = new Set<string>()
    try {
      for (const draft of drafts) {
        if (draft.questionId || draft.baseContentVersion) {
          taxonomyError('批量新增接口不能用于覆盖已有题目。')
        }
        if (!draft.id) taxonomyError('批量导入题目缺少稳定标识。')
        if (importIds.has(draft.id)) taxonomyError('批量导入包含重复的题目标识。')
        importIds.add(draft.id)
        const existing = questions.find((question) => question.id === draft.id)
        if (existing) {
          if (!importedDraftMatchesQuestion(draft, existing)) {
            taxonomyError(
              '相同导入标识已对应另一份题目内容，请重新检查导入草稿。',
              'IMPORT_ID_CONFLICT',
            )
          }
          savedIds.push(existing.id)
          continue
        }
        const saved = await this.saveQuestion(draft)
        const stored = questions.find((question) => question.id === saved.id)
        if (!stored) taxonomyError('批量导入后无法读取题目。')
        stored.id = draft.id
        savedIds.push(draft.id)
      }
      persist()
      return savedIds
    } catch (reason) {
      questions = snapshot
      persist()
      throw reason
    }
  },

  async saveImportedQuestionOverwrites(
    request: ImportedQuestionOverwriteRequest,
  ): Promise<ImportedQuestionOverwriteResult> {
    canonicalDraftUuid(request.operationId, '批量覆盖操作标识')
    if (!request.items.length) {
      taxonomyError('请至少选择一道要覆盖的题目。', 'IMPORT_OVERWRITE_EMPTY')
    }
    if (request.items.length > 100) {
      taxonomyError('一次最多覆盖 100 道题，请分批导入。', 'IMPORT_OVERWRITE_TOO_LARGE')
    }

    const clientIds = new Set<string>()
    const questionIds = new Set<string>()
    for (const item of request.items) {
      canonicalDraftUuid(item.clientId, '导入条目标识')
      if (clientIds.has(item.clientId)) {
        taxonomyError('批量覆盖包含无效或重复的导入条目标识。', 'IMPORT_OVERWRITE_DUPLICATE_CLIENT_ID')
      }
      clientIds.add(item.clientId)
      if (!item.draft.questionId) {
        taxonomyError(
          '批量覆盖中的题目缺少原题标识，请重新查重后再试。',
          'IMPORT_OVERWRITE_TARGET_REQUIRED',
        )
      }
      if (questionIds.has(item.draft.questionId)) {
        taxonomyError(
          '同一批次不能多次覆盖同一道原题，请先处理源文件中的重复项。',
          'IMPORT_OVERWRITE_DUPLICATE_TARGET',
        )
      }
      questionIds.add(item.draft.questionId)
      if (!Number.isSafeInteger(item.draft.baseContentVersion)
        || (item.draft.baseContentVersion ?? 0) < 1) {
        taxonomyError(
          '批量覆盖中的题目缺少有效版本，请重新查重后再试。',
          'IMPORT_OVERWRITE_VERSION_REQUIRED',
        )
      }
    }

    const requestHash = stableJson(request.items)
    const receipt = importedOverwriteReceipts.get(request.operationId)
    if (receipt) {
      if (receipt.requestHash !== requestHash) {
        taxonomyError(
          '这个批量覆盖操作标识已经用于其他内容，请重新发起导入。',
          'IMPORT_OVERWRITE_OPERATION_CONFLICT',
        )
      }
      return { ...clone(receipt.result), replayed: true }
    }

    const snapshot = clone(questions)
    try {
      const resultItems: ImportedQuestionOverwriteResult['items'] = []
      for (const item of request.items) {
        const saved = await this.saveQuestion(item.draft)
        resultItems.push({
          clientId: item.clientId,
          questionId: saved.id,
          contentVersion: saved.contentVersion,
        })
      }
      const result: ImportedQuestionOverwriteResult = {
        operationId: request.operationId,
        updatedCount: resultItems.length,
        items: resultItems,
        replayed: false,
      }
      importedOverwriteReceipts.set(request.operationId, {
        requestHash,
        result: clone(result),
      })
      return result
    } catch (reason) {
      questions = snapshot
      persist()
      throw reason
    }
  },

  async batchEditQuestions(request: QuestionBatchEditRequest): Promise<QuestionBatchEditResult> {
    if (!request.questions.length) taxonomyError('请先选择至少一道要修改的题目。', 'BATCH_EDIT_EMPTY')
    if (request.questions.length > 500) {
      taxonomyError('一次最多批量修改 500 道题，请分批操作。', 'BATCH_EDIT_TOO_LARGE')
    }
    const targetIds = request.questions.map((target) => target.id)
    if (new Set(targetIds).size !== targetIds.length) {
      taxonomyError('批量修改列表中包含重复题目，请重新选择。', 'BATCH_EDIT_DUPLICATE_ID')
    }
    if (!['keep', 'reset_never'].includes(request.usageOperation)) {
      taxonomyError('最近使用状态操作无效，请重新打开批量编辑窗口。')
    }
    if (!['keep', 'append', 'replace', 'remove'].includes(request.tagOperation.mode)) {
      taxonomyError('标签操作无效，请重新打开批量编辑窗口。')
    }
    if (new Set(request.tagOperation.tagIds).size !== request.tagOperation.tagIds.length) {
      taxonomyError('标签列表中包含重复项，请重新选择。')
    }
    if (request.tagOperation.tagIds.length > 100) taxonomyError('一次最多选择 100 个标签。')
    if (request.tagOperation.mode === 'keep' && request.tagOperation.tagIds.length) {
      taxonomyError('保持标签不变时不能同时提交标签列表。')
    }
    if (['append', 'remove'].includes(request.tagOperation.mode) && !request.tagOperation.tagIds.length) {
      taxonomyError('追加或删除标签时，请至少选择一个标签。')
    }
    if (!request.classification && request.usageOperation === 'keep' && request.tagOperation.mode === 'keep') {
      taxonomyError('尚未选择任何要修改的项目。', 'BATCH_EDIT_NO_CHANGES')
    }

    let targetSubject: Subject | undefined
    let targetChapter: Chapter | undefined
    if (request.classification) {
      targetSubject = subjects.find((subject) => subject.id === request.classification?.subjectId)
      targetChapter = targetSubject?.chapters.find((chapter) => chapter.id === request.classification?.chapterId)
      if (!targetSubject || !targetChapter) {
        taxonomyError('所选学科和章节已经不存在或不匹配，请刷新后重试。', 'BATCH_CLASSIFICATION_INVALID')
      }
    }
    const selectedTags = request.tagOperation.tagIds.map((tagId) => {
      const tag = tags.find((candidate) => candidate.id === tagId)
      if (!tag) taxonomyError('所选标签已经不存在，请刷新后重试。', 'BATCH_TAG_NOT_FOUND')
      return tag
    })

    // Validate every target before constructing the replacement array. This
    // mirrors the desktop transaction: one stale/deleted row aborts all rows.
    const targets = new Map<string, Question>()
    for (const target of request.questions) {
      const question = questions.find((candidate) => candidate.id === target.id)
      if (!question) taxonomyError('选中的题目中有题目已经不存在，本次修改未执行。', 'QUESTION_NOT_FOUND')
      if (question.deletedAt) {
        taxonomyError('选中的题目中有题目已进入回收站，本次修改未执行。', 'QUESTION_IN_RECYCLE')
      }
      if (question.contentVersion !== target.expectedContentVersion) {
        taxonomyError(
          '选中的题目中有题目已在其他窗口被修改，本次修改未执行；请刷新后重试。',
          'CONTENT_CONFLICT',
        )
      }
      targets.set(target.id, question)
    }

    const timestamp = Date.now()
    const nextQuestions = questions.map((question) => {
      if (!targets.has(question.id)) return question
      let nextTags = question.tags
      if (request.tagOperation.mode === 'replace') nextTags = [...selectedTags]
      if (request.tagOperation.mode === 'append') {
        const existingIds = new Set(question.tags.map((tag) => tag.id))
        nextTags = [...question.tags, ...selectedTags.filter((tag) => !existingIds.has(tag.id))]
      }
      if (request.tagOperation.mode === 'remove') {
        const removedIds = new Set(selectedTags.map((tag) => tag.id))
        nextTags = question.tags.filter((tag) => !removedIds.has(tag.id))
      }
      if (nextTags.length > 100) taxonomyError('题目最多允许 100 个标签，本次修改未执行。')
      return {
        ...question,
        subjectId: targetSubject?.id ?? question.subjectId,
        chapterId: targetChapter?.id ?? question.chapterId,
        subjectName: targetSubject?.name ?? question.subjectName,
        chapterName: targetChapter?.name ?? question.chapterName,
        tags: nextTags,
        lastUsedAt: request.usageOperation === 'reset_never' ? null : question.lastUsedAt,
        updatedAt: timestamp,
        contentVersion: question.contentVersion + 1,
      }
    })
    const previous = questions
    questions = nextQuestions
    try {
      persist()
    } catch (reason) {
      questions = previous
      throw reason
    }
    return {
      updatedCount: request.questions.length,
      questions: request.questions.map((target) => ({
        id: target.id,
        contentVersion: target.expectedContentVersion + 1,
      })),
    }
  },

  async moveQuestionsToRecycle(ids: string[]): Promise<void> {
    const deletedAt = Date.now()
    questions = questions.map((question) => ids.includes(question.id) ? { ...question, deletedAt, updatedAt: deletedAt } : question)
    persist()
  },

  async restoreQuestions(ids: string[]): Promise<void> {
    questions = questions.map((question) => ids.includes(question.id) ? { ...question, deletedAt: null, updatedAt: Date.now() } : question)
    persist()
  },

  async permanentlyDeleteQuestions(ids: string[]): Promise<void> {
    questions = questions.filter((question) => !(ids.includes(question.id) && question.deletedAt))
    pruneDuplicateIgnores()
    persist()
  },

  async saveImportedQuestion(draft: QuestionDraft): Promise<Question> {
    return this.saveQuestion(draft)
  },

  async getSettings(): Promise<AppSettings> {
    return { ...settings }
  },

  async saveSettings(next: AppSettings): Promise<AppSettings> {
    settings = { ...next }
    templates = templates.map((template) => ({
      ...template,
      isDefault: template.id === (settings.defaultTemplateId ?? null),
    }))
    persist()
    return { ...settings }
  },
}
