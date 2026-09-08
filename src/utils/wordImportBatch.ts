import type { QuestionDraft, QuestionDuplicateCandidate, RichContent } from '../types/domain'

export interface WordImportBatchSelectable {
  id: string
  selected: boolean
}

export interface WordImportBatchClassifiable extends WordImportBatchSelectable {
  subjectId: string
  chapterId: string
}

export interface WordImportBatchSelectionState {
  selectedCount: number
  allSelected: boolean
  indeterminate: boolean
}

export type WordImportDuplicateKind = 'none' | 'exact' | 'suspected'
export type WordImportDuplicateAction = 'skip' | 'overwrite' | 'keep'
export type WordImportDuplicateScanPhase = 'idle' | 'database' | 'import-batch'

export interface WordImportDuplicateScanProgress {
  phase: WordImportDuplicateScanPhase
  completed: number
  total: number
  includesImportBatchPass: boolean
}

export interface WordImportOverwriteTargeted {
  duplicateCandidate: Pick<QuestionDuplicateCandidate, 'id' | 'sourceKind'> | null
}

export interface WordImportOverwriteDuplicate<T> {
  targetQuestionId: string
  item: T
  representative: T
}

export interface WordImportOverwriteConflict<T> {
  targetQuestionId: string
  items: T[]
}

export interface WordImportOverwritePlan<T> {
  /** One write per database question when every request for that target is equivalent. */
  representatives: T[]
  /** Equivalent requests that can be completed as source-file duplicates without another write. */
  duplicates: WordImportOverwriteDuplicate<T>[]
  /** Targets receiving different content. No item in a conflict is selected for writing. */
  conflicts: WordImportOverwriteConflict<T>[]
}

export interface WordImportDuplicateResolvable extends WordImportBatchSelectable {
  duplicateKind: WordImportDuplicateKind
  duplicateCandidate: { sourceKind?: 'question' | 'import-item' } | null
  duplicateChecking: boolean
  duplicateNeedsCheck: boolean
  duplicateCheckError?: string
}

/**
 * Maps the two duplicate-check phases onto one continuous percentage without
 * presenting the number of checks as though it were the number of questions.
 */
export function wordImportDuplicateScanPercentage(
  progress: WordImportDuplicateScanProgress,
): number {
  if (progress.total <= 0 || progress.phase === 'idle') return 0
  const ratio = Math.min(1, Math.max(0, progress.completed / progress.total))
  if (!progress.includesImportBatchPass) return Math.round(ratio * 100)
  if (progress.phase === 'database') return Math.round(ratio * 50)
  return Math.round(50 + ratio * 50)
}

export function canOpenWordImportDuplicateDialog(
  item: WordImportDuplicateResolvable,
  duplicateScanRunning: boolean,
) {
  return !duplicateScanRunning
    && !item.duplicateChecking
    && !item.duplicateNeedsCheck
    && !item.duplicateCheckError
    && item.duplicateKind !== 'none'
    && Boolean(item.duplicateCandidate)
}

export function wordImportDuplicateActionTargets<T extends WordImportDuplicateResolvable>(
  items: readonly T[],
  current: T,
  action: WordImportDuplicateAction,
  applyToSameKind: boolean,
): T[] {
  if (!applyToSameKind) return [current]
  return items.filter((item) => (
    (item.selected || item.id === current.id)
    && item.duplicateKind === current.duplicateKind
    && item.duplicateKind !== 'none'
    && item.duplicateCandidate
    && !item.duplicateChecking
    && !item.duplicateNeedsCheck
    && !item.duplicateCheckError
    && (action !== 'overwrite' || item.duplicateCandidate.sourceKind !== 'import-item')
  ))
}

export function wordImportBatchSelectionState(
  items: readonly WordImportBatchSelectable[],
): WordImportBatchSelectionState {
  const selectedCount = items.reduce((count, item) => count + Number(item.selected), 0)
  return {
    selectedCount,
    allSelected: items.length > 0 && selectedCount === items.length,
    indeterminate: selectedCount > 0 && selectedCount < items.length,
  }
}

export function setWordImportBatchSelection(
  items: readonly WordImportBatchSelectable[],
  selected: boolean,
) {
  for (const item of items) item.selected = selected
}

export function applyWordImportBatchClassification<T extends WordImportBatchClassifiable>(
  items: readonly T[],
  subjectId: string,
  chapterId: string,
): T[] {
  const updated: T[] = []
  for (const item of items) {
    if (!item.selected) continue
    item.subjectId = subjectId
    item.chapterId = chapterId
    updated.push(item)
  }
  return updated
}

export function pendingWordImportItems<T extends Pick<WordImportBatchSelectable, 'id'>>(
  items: readonly T[],
  completedIds: ReadonlySet<string>,
): T[] {
  return items.filter((item) => !completedIds.has(item.id))
}

function stableJson(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(stableJson).join(',')}]`
  if (value && typeof value === 'object') {
    const entries = Object.entries(value as Record<string, unknown>)
      .filter(([, item]) => item !== undefined)
      .sort(([left], [right]) => left.localeCompare(right))
    return `{${entries.map(([key, item]) => `${JSON.stringify(key)}:${stableJson(item)}`).join(',')}}`
  }
  return JSON.stringify(value)
}

function canonicalRichDocument(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(canonicalRichDocument)
  if (!value || typeof value !== 'object') return value
  return Object.fromEntries(Object.entries(value as Record<string, unknown>)
    // Editor node IDs identify one parsing/editing occurrence, not its content.
    .filter(([key, item]) => item !== undefined && key !== 'nodeId' && key !== 'data-node-id')
    .map(([key, item]) => [key, canonicalRichDocument(item)]))
}

function canonicalRichHtml(html: string) {
  return html
    .replace(/\r\n?/gu, '\n')
    .replace(/\sdata-node-id\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>]+)/giu, '')
}

function canonicalRichContent(content: RichContent) {
  return {
    schemaVersion: content.schemaVersion,
    editor: content.editor ?? null,
    document: content.document ? canonicalRichDocument(content.document) : null,
    html: canonicalRichHtml(content.html),
    plainText: content.plainText.replace(/\r\n?/gu, '\n'),
    sourceOoxml: content.sourceOoxml ?? null,
  }
}

/**
 * Produces a stable description of the question content written by an import.
 * Operation metadata and generated IDs are intentionally excluded.
 */
export function wordImportQuestionSemanticSignature(draft: QuestionDraft): string {
  const optionPositions = new Map(draft.options.map((option, index) => [option.id, index]))
  const resources = (draft.resourceRefs ?? []).map((reference) => ({
    resourceId: reference.resourceId,
    contentSlot: reference.contentSlot,
    optionPosition: reference.contentSlot === 'option'
      ? optionPositions.get(reference.optionId ?? '') ?? null
      : null,
  })).sort((left, right) => stableJson(left).localeCompare(stableJson(right)))

  return stableJson({
    type: draft.type,
    stem: canonicalRichContent(draft.stem),
    options: draft.options.map((option) => canonicalRichContent(option.content)),
    answer: canonicalRichContent(draft.answer),
    explanation: canonicalRichContent(draft.explanation),
    subjectId: draft.subjectId,
    chapterId: draft.chapterId,
    tagIds: [...draft.tagIds].sort((left, right) => left.localeCompare(right)),
    resourceRefs: resources,
  })
}

/**
 * Coalesces overwrite requests that target the same stored question. Different
 * content for one target is returned as a conflict so the caller can stop and
 * ask the user which source row should win.
 */
export function planWordImportOverwrites<T extends WordImportOverwriteTargeted>(
  items: readonly T[],
  draftForItem: (item: T) => QuestionDraft,
): WordImportOverwritePlan<T> {
  const byTarget = new Map<string, T[]>()
  for (const item of items) {
    const candidate = item.duplicateCandidate
    if (!candidate || candidate.sourceKind === 'import-item') {
      throw new Error('覆盖归并只接受指向题库原题的导入项。')
    }
    const grouped = byTarget.get(candidate.id)
    if (grouped) grouped.push(item)
    else byTarget.set(candidate.id, [item])
  }

  const representatives: T[] = []
  const duplicates: WordImportOverwriteDuplicate<T>[] = []
  const conflicts: WordImportOverwriteConflict<T>[] = []
  for (const [targetQuestionId, grouped] of byTarget) {
    const bySignature = new Map<string, T[]>()
    for (const item of grouped) {
      const signature = wordImportQuestionSemanticSignature(draftForItem(item))
      const equivalent = bySignature.get(signature)
      if (equivalent) equivalent.push(item)
      else bySignature.set(signature, [item])
    }
    if (bySignature.size > 1) {
      conflicts.push({ targetQuestionId, items: [...grouped] })
      continue
    }

    const representative = grouped[0]!
    representatives.push(representative)
    for (const item of grouped.slice(1)) {
      duplicates.push({ targetQuestionId, item, representative })
    }
  }

  return { representatives, duplicates, conflicts }
}
