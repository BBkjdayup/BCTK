import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { backend } from '../services/backend'
import { errorMessage } from '../services/errors'
import {
  QUESTION_TYPES,
  type Paper,
  type PaperFilters,
  type PaperItem,
  type PaperSummary,
  type Question,
  type QuestionType,
} from '../types/domain'
import { clonePlain } from '../utils/clonePlain'
import { remainingPaperCapacity } from '../utils/licensing'
import { fetchAllPaperSummaries, PAPER_HISTORY_PAGE_SIZE } from '../utils/paperHistory'
import { replacePaperItem } from '../utils/paperSelection'

const questionTypeOrder = new Map<string, number>(QUESTION_TYPES.map((type, index) => [type, index]))

export function configurePaperQuestionTypeOrder(types: readonly QuestionType[]) {
  questionTypeOrder.clear()
  types.forEach((type, index) => questionTypeOrder.set(type, index))
}

function sortablePosition(item: PaperItem) {
  return Number.isFinite(item.position) ? item.position : Number.MAX_SAFE_INTEGER
}

/**
 * Canonical paper order: QUESTION_TYPES order first, then the existing
 * position inside each type. Reindexing here keeps editor, preview and saved
 * paper numbering identical.
 */
export function normalizePaperItems(items: readonly PaperItem[]): PaperItem[] {
  return items
    .map((item, originalIndex) => ({ item, originalIndex }))
    .sort((left, right) => {
      const typeDifference = (questionTypeOrder.get(left.item.snapshot.type) ?? QUESTION_TYPES.length)
        - (questionTypeOrder.get(right.item.snapshot.type) ?? QUESTION_TYPES.length)
      if (typeDifference !== 0) return typeDifference
      return sortablePosition(left.item) - sortablePosition(right.item) || left.originalIndex - right.originalIndex
    })
    .map(({ item }, position) => ({ ...item, position }))
}

export function movePaperItemWithinType(
  items: readonly PaperItem[],
  id: string,
  direction: -1 | 1,
): { items: PaperItem[]; moved: boolean } {
  const ordered = normalizePaperItems(items)
  const sourceIndex = ordered.findIndex((item) => item.id === id)
  if (sourceIndex < 0) return { items: ordered, moved: false }

  const type = ordered[sourceIndex].snapshot.type
  const sameTypeIndices = ordered
    .map((item, index) => item.snapshot.type === type ? index : -1)
    .filter((index) => index >= 0)
  const indexWithinType = sameTypeIndices.indexOf(sourceIndex)
  const targetIndex = sameTypeIndices[indexWithinType + direction]
  if (targetIndex === undefined) return { items: ordered, moved: false }

  ;[ordered[sourceIndex], ordered[targetIndex]] = [ordered[targetIndex], ordered[sourceIndex]]
  return {
    items: ordered.map((item, position) => ({ ...item, position })),
    moved: true,
  }
}

export function reorderPaperItemsWithinType(
  items: readonly PaperItem[],
  type: QuestionType,
  orderedIds: readonly string[],
): { items: PaperItem[]; moved: boolean } {
  const ordered = normalizePaperItems(items)
  const typeItems = ordered.filter((item) => item.snapshot.type === type)
  const expectedIds = new Set(typeItems.map((item) => item.id))
  const suppliedIds = new Set(orderedIds)
  const validOrder = orderedIds.length === typeItems.length
    && suppliedIds.size === expectedIds.size
    && orderedIds.every((id) => expectedIds.has(id))
  if (!validOrder) return { items: ordered, moved: false }

  const currentIds = typeItems.map((item) => item.id)
  const moved = orderedIds.some((id, index) => id !== currentIds[index])
  if (!moved) return { items: ordered, moved: false }

  const itemsById = new Map(typeItems.map((item) => [item.id, item]))
  let typeIndex = 0
  const reordered = ordered.map((item) => (
    item.snapshot.type === type
      ? itemsById.get(orderedIds[typeIndex++]) ?? item
      : item
  ))
  return {
    items: reordered.map((item, position) => ({ ...item, position })),
    moved: true,
  }
}

function normalizePaper(paper: Paper): Paper {
  return {
    ...paper,
    // 浏览器演示里可能留有升级前的数据；缺少该字段代表“旧试卷未保存组卷范围”。
    generationConfig: paper.generationConfig ?? null,
    preferredTemplateId: paper.preferredTemplateId ?? null,
    layout: paper.layout ?? null,
    layoutUpdatedAt: paper.layoutUpdatedAt ?? null,
    items: normalizePaperItems(paper.items),
  }
}

function paperEditableSignature(paper: Paper) {
  return JSON.stringify({
    id: paper.id,
    title: paper.title,
    compositionMode: paper.compositionMode,
    generationConfig: paper.generationConfig ?? null,
    items: paper.items,
    exportContentMode: paper.exportContentMode,
    subjectSummaryText: paper.subjectSummaryText,
    preferredTemplateId: paper.preferredTemplateId ?? null,
    layout: paper.layout ?? null,
    layoutUpdatedAt: paper.layoutUpdatedAt ?? null,
  })
}

function mergeSavedPaperMetadata(live: Paper, saved: Paper): Paper {
  return normalizePaper({
    ...clonePlain(live),
    status: saved.status,
    rowVersion: saved.rowVersion,
    createdAt: saved.createdAt,
    updatedAt: Math.max(live.updatedAt, saved.updatedAt),
    savedAt: saved.savedAt ?? null,
    lastSavedAt: saved.lastSavedAt ?? null,
  })
}

function emptyPaper(): Paper {
  const timestamp = Date.now()
  return {
    id: crypto.randomUUID(),
    title: '未命名试卷',
    compositionMode: 'manual',
    generationConfig: null,
    status: 'draft',
    items: [],
    exportContentMode: 'paper_only',
    subjectSummaryText: '',
    preferredTemplateId: null,
    layout: null,
    layoutUpdatedAt: null,
    rowVersion: 0,
    createdAt: timestamp,
    updatedAt: timestamp,
    savedAt: null,
    lastSavedAt: null,
  }
}

export const usePaperStore = defineStore('paper', () => {
  const current = ref<Paper>(emptyPaper())
  const papers = ref<PaperSummary[]>([])
  const historyTotal = ref(0)
  const saving = ref(false)
  const historyLoading = ref(false)
  const error = ref<string | null>(null)

  const selectedQuestionIds = computed(() => new Set(
    current.value.items
      .map((item) => item.sourceQuestionId)
      .filter((id): id is string => typeof id === 'string' && id.length > 0),
  ))

  function touch() {
    current.value.updatedAt = Date.now()
  }

  function addQuestions(questions: Question[], maxQuestions: number | null = null) {
    const selectedIds = new Set(selectedQuestionIds.value)
    const additions: PaperItem[] = []
    const remaining = remainingPaperCapacity(current.value.items.length, maxQuestions)
    for (const question of questions) {
      if (additions.length >= remaining) break
      if (selectedIds.has(question.id)) continue
      selectedIds.add(question.id)
      additions.push({
        id: crypto.randomUUID(),
        sourceQuestionId: question.id,
        position: current.value.items.length + additions.length,
        snapshot: clonePlain(question),
      })
    }
    if (!additions.length) return 0
    current.value.items = normalizePaperItems([...current.value.items, ...additions])
    touch()
    return additions.length
  }

  function removeItem(id: string) {
    current.value.items = normalizePaperItems(current.value.items.filter((item) => item.id !== id))
    touch()
  }

  function moveItemWithinType(id: string, direction: -1 | 1) {
    const result = movePaperItemWithinType(current.value.items, id, direction)
    current.value.items = result.items
    if (result.moved) touch()
    return result.moved
  }

  function reorderItemsWithinType(type: QuestionType, orderedIds: readonly string[]) {
    const result = reorderPaperItemsWithinType(current.value.items, type, orderedIds)
    current.value.items = result.items
    if (result.moved) touch()
    return result.moved
  }

  function replaceItem(id: string, question: Question) {
    const result = replacePaperItem(current.value.items, id, question)
    if (!result.replaced) return result
    current.value.items = normalizePaperItems(result.items)
    touch()
    return { ...result, items: current.value.items }
  }

  function normalizeCurrentItems() {
    current.value.items = normalizePaperItems(current.value.items)
  }

  function clear() {
    current.value.items = []
    touch()
  }

  function newPaper() {
    current.value = emptyPaper()
  }

  async function save(status: Paper['status']) {
    saving.value = true
    error.value = null
    try {
      current.value.items = normalizePaperItems(current.value.items)
      const draft: Paper = {
        ...clonePlain(current.value),
        title: current.value.title.trim().normalize('NFKC').trim() || '未命名试卷',
        status,
        items: clonePlain(current.value.items),
      }
      const paperId = current.value.id
      const editableAtRequest = paperEditableSignature(current.value)
      const saved = normalizePaper(await backend.savePaper(draft))

      // The backend response represents the snapshot sent above. If the
      // teacher edited this paper while the request was pending, retain those
      // live fields and merge only the authoritative persistence metadata.
      // If another paper was opened meanwhile, do not replace it at all.
      if (current.value.id !== paperId) return saved
      current.value = paperEditableSignature(current.value) === editableAtRequest
        ? saved
        : mergeSavedPaperMetadata(current.value, saved)
      return current.value
    } catch (reason) {
      error.value = errorMessage(reason, '保存试卷失败')
      throw reason
    } finally {
      saving.value = false
    }
  }

  async function loadPaper(id: string) {
    error.value = null
    const paper = await backend.getPaper(id)
    if (!paper) throw new Error('找不到这份试卷，可能已被删除。')
    current.value = normalizePaper(paper)
    return current.value
  }

  async function readStored(id: string) {
    const paper = await backend.getPaper(id)
    if (!paper) throw new Error('找不到这份试卷，可能已被删除。')
    return normalizePaper(paper)
  }

  async function loadPapers(filters: PaperFilters = {
    keyword: '', status: 'all', page: 1, pageSize: 500,
  }) {
    historyLoading.value = true
    error.value = null
    try {
      const result = await backend.listPapers(filters)
      papers.value = result.items
      historyTotal.value = result.total
      return result
    } catch (reason) {
      error.value = errorMessage(reason, '加载试卷列表失败')
      throw reason
    } finally {
      historyLoading.value = false
    }
  }

  async function loadAllPapers(filters: PaperFilters = {
    keyword: '', status: 'all', page: 1, pageSize: PAPER_HISTORY_PAGE_SIZE,
  }) {
    historyLoading.value = true
    error.value = null
    try {
      const result = await fetchAllPaperSummaries(backend.listPapers, filters)
      papers.value = result
      historyTotal.value = result.length
      return result
    } catch (reason) {
      error.value = errorMessage(reason, '加载完整试卷列表失败')
      throw reason
    } finally {
      historyLoading.value = false
    }
  }

  async function copyAndLoad(id: string, baseRowVersion: number) {
    error.value = null
    current.value = normalizePaper(await backend.copyPaper(id, baseRowVersion))
    return current.value
  }

  async function deleteStored(id: string, baseRowVersion: number) {
    await backend.deletePaper(id, baseRowVersion)
    papers.value = papers.value.filter((paper) => paper.id !== id)
    historyTotal.value = Math.max(0, historyTotal.value - 1)
    if (current.value.id === id) newPaper()
  }

  async function deleteStoredBatch(items: readonly Pick<PaperSummary, 'id' | 'rowVersion'>[]) {
    if (!items.length) return
    await backend.deletePapers(items.map((item) => ({
      id: item.id,
      baseRowVersion: item.rowVersion,
    })))
    const deletedIds = new Set(items.map((item) => item.id))
    papers.value = papers.value.filter((paper) => !deletedIds.has(paper.id))
    historyTotal.value = Math.max(0, historyTotal.value - deletedIds.size)
    if (deletedIds.has(current.value.id)) newPaper()
  }

  return {
    current,
    papers,
    historyTotal,
    saving,
    historyLoading,
    error,
    selectedQuestionIds,
    addQuestions,
    removeItem,
    moveItemWithinType,
    reorderItemsWithinType,
    replaceItem,
    normalizeCurrentItems,
    clear,
    newPaper,
    save,
    loadPaper,
    readStored,
    loadPapers,
    loadAllPapers,
    copyAndLoad,
    deleteStored,
    deleteStoredBatch,
  }
})
