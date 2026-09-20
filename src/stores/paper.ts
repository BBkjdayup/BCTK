import { paperLayoutSourceSignature } from '../utils/paperSourceSignature'
import { defineStore } from 'pinia'
import { computed, ref, watch, onScopeDispose } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { backend } from '../services/backend'
import { errorMessage } from '../services/errors'
import {
  QUESTION_TYPES,
  type Paper,
  type PaperRecovery,
  type PaperFilters,
  type PaperItem,
  type PaperSummary,
  type Question,
  type QuestionType,
} from '../types/domain'
import { clonePlain } from '../utils/clonePlain'
import { fetchAllPaperSummaries, PAPER_HISTORY_PAGE_SIZE } from '../utils/paperHistory'
import { replacePaperItem } from '../utils/paperSelection'
import { reidentifyPaper } from '../utils/paperArchive'

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
  const savedSignature = ref(paperEditableSignature(current.value))
  const isDirty = computed(() => paperEditableSignature(current.value) !== savedSignature.value)
  const transitioning = ref(false)
  const recoveryReady = ref(false)
  const pendingRecovery = ref<PaperRecovery | null>(null)
  const recoveryError = ref<string | null>(null)
  const recoverySavedAt = ref<number | null>(null)
  let recoveryRevision: string | null = null
  let checkpointSignature = ''
  let recoveryQueue: Promise<unknown> = Promise.resolve()
  let saveInFlight: Promise<Paper> | null = null
  let promptOpen = false
  let recoveryTimer: ReturnType<typeof setTimeout> | undefined
  let recoveryInterval: ReturnType<typeof setInterval> | undefined

  function queueRecovery<T>(work: () => Promise<T>): Promise<T> {
    const task = recoveryQueue.then(work)
    recoveryQueue = task.catch(() => undefined)
    return task
  }

  async function checkpoint() {
    if (!recoveryReady.value || pendingRecovery.value) return
    await queueRecovery(async () => {
      if (!isDirty.value || transitioning.value) return
      const signature = paperEditableSignature(current.value)
      if (signature === checkpointSignature) return
      try {
        const record = await backend.savePaperRecovery(clonePlain(current.value))
        recoveryRevision = record.revision
        checkpointSignature = signature
        recoverySavedAt.value = record.autosavedAt
        recoveryError.value = null
      } catch (reason) {
        recoveryError.value = errorMessage(reason, '试卷恢复草稿保存失败，请手动保存后再退出')
        throw reason
      }
    })
  }

  async function clearRecovery() {
    await queueRecovery(async () => {
      if (recoveryRevision) await backend.clearPaperRecovery(recoveryRevision)
      recoveryRevision = null
      checkpointSignature = ''
      recoverySavedAt.value = null
    })
  }

  async function initializeRecovery() {
    if (recoveryReady.value) return
    try {
      pendingRecovery.value = await backend.getPaperRecovery()
      recoveryReady.value = true
      recoveryInterval = setInterval(() => { void checkpoint().catch(() => undefined) }, 30_000)
      if (!pendingRecovery.value) await checkpoint()
    } catch (reason) {
      recoveryError.value = errorMessage(reason, '读取试卷恢复草稿失败，自动保存暂未启动')
    }
  }

  async function recoverPaper() {
    const record = pendingRecovery.value
    if (!record || transitioning.value) return false
    transitioning.value = true
    try {
      if (!(await confirmLeave('恢复上次试卷'))) return false
      // Recover as a fresh draft: the original history row may have been edited
      // or deleted since the crash. Never overwrite it with an old row version.
      const recovered = normalizePaper(clonePlain(record.paper))
      const oldSignature = paperLayoutSourceSignature(recovered)
      const itemIds = new Map(recovered.items.map((item) => [item.id, crypto.randomUUID()]))
      recovered.id = crypto.randomUUID()
      recovered.items = recovered.items.map((item) => ({ ...item, id: itemIds.get(item.id)! }))
      if (recovered.layout) {
        const pending: unknown[] = [recovered.layout.data]
        while (pending.length) {
          const node = pending.pop()
          if (!node || typeof node !== 'object') continue
          const record = node as Record<string, unknown>
          if (typeof record.paperItemId === 'string' && itemIds.has(record.paperItemId)) record.paperItemId = itemIds.get(record.paperItemId)!
          pending.push(...Object.values(record))
        }
        if (recovered.layout.sourceSignature === oldSignature) recovered.layout.sourceSignature = paperLayoutSourceSignature(recovered)
      }
      recovered.status = 'draft'
      recovered.rowVersion = 0
      recovered.savedAt = null
      recovered.lastSavedAt = null
      recovered.createdAt = Date.now()
      current.value = recovered
      savedSignature.value = ''
      recoveryRevision = record.revision
      pendingRecovery.value = null
      return true
    } finally {
      transitioning.value = false
      await checkpoint().catch(() => undefined)
    }
  }

  async function discardPendingRecovery() {
    const record = pendingRecovery.value
    if (!record) return
    try {
      await ElMessageBox.confirm('确定丢弃上次未保存的试卷草稿吗？', '丢弃恢复草稿', {
        type: 'warning', confirmButtonText: '丢弃草稿', cancelButtonText: '取消',
      })
    } catch { return }
    await queueRecovery(() => backend.clearPaperRecovery(record.revision))
    pendingRecovery.value = null
    await checkpoint()
  }

  watch(() => paperEditableSignature(current.value), () => {
    clearTimeout(recoveryTimer)
    recoveryTimer = setTimeout(() => { void checkpoint().catch(() => undefined) }, 1000)
  })
  onScopeDispose(() => {
    clearTimeout(recoveryTimer)
    clearInterval(recoveryInterval)
  })

  async function confirmLeave(action: string): Promise<boolean> {
    if (promptOpen) return false
    promptOpen = true
    try {
      if (saveInFlight) await saveInFlight
      if (!isDirty.value) return true
      const signature = paperEditableSignature(current.value)
      try {
        await ElMessageBox.confirm(`当前试卷“${current.value.title || '未命名试卷'}”还有未保存的修改。${action}前如何处理？右上角关闭或 Esc 可取消操作。`, '未保存的试卷', {
          type: 'warning', confirmButtonText: '保存并继续', cancelButtonText: '放弃修改',
          distinguishCancelAndClose: true, closeOnClickModal: false,
        })
      } catch (action) {
        return action === 'cancel' && signature === paperEditableSignature(current.value)
      }
      await save(current.value.status)
      if (isDirty.value) {
        ElMessage.warning('保存期间又有修改，请核对并保存后继续。')
        return false
      }
      return true
    } catch (reason) {
      ElMessage.error(errorMessage(reason, '试卷未能保存，已保留当前编辑内容'))
      return false
    } finally { promptOpen = false }
  }

  async function prepareToClose() {
    if (transitioning.value || !(await confirmLeave('关闭软件'))) return false
    const signature = paperEditableSignature(current.value)
    await clearRecovery()
    return signature === paperEditableSignature(current.value)
  }

  async function replaceCurrent(loadNext: () => Paper | Promise<Paper>): Promise<Paper | null> {
    if (transitioning.value) return null
    transitioning.value = true
    try {
      if (!(await confirmLeave('切换试卷'))) return null
      const signature = paperEditableSignature(current.value)
      const next = normalizePaper(await loadNext())
      if (signature !== paperEditableSignature(current.value)) return null
      await clearRecovery()
      if (signature !== paperEditableSignature(current.value)) return null
      current.value = next
      savedSignature.value = paperEditableSignature(next)
      return next
    } finally {
      transitioning.value = false
      void checkpoint().catch(() => undefined)
    }
  }

  const selectedQuestionIds = computed(() => new Set(
    current.value.items
      .map((item) => item.sourceQuestionId)
      .filter((id): id is string => typeof id === 'string' && id.length > 0),
  ))

  function touch() {
    current.value.updatedAt = Date.now()
  }

  function addQuestions(questions: Question[]) {
    const selectedIds = new Set(selectedQuestionIds.value)
    const additions: PaperItem[] = []
    for (const question of questions) {
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

  async function newPaper() {
    return (await replaceCurrent(emptyPaper)) !== null
  }

  function save(status: Paper['status']): Promise<Paper> {
    if (saveInFlight) return saveInFlight.then(() => save(status))
    const task = performSave(status)
    saveInFlight = task
    const finish = () => { if (saveInFlight === task) saveInFlight = null }
    void task.then(finish, finish)
    return task
  }

  async function performSave(status: Paper['status']) {
    if (current.value.rowVersion > 0 && current.value.status === status && !isDirty.value) {
      return clonePlain(current.value)
    }
    saving.value = true
    error.value = null
    try {
      current.value.items = normalizePaperItems(current.value.items)
      let draft: Paper = {
        ...clonePlain(current.value),
        title: current.value.title.trim().normalize('NFKC').trim() || '未命名试卷',
        status,
        items: clonePlain(current.value.items),
      }
      const paperId = current.value.id
      const editableAtRequest = paperEditableSignature(current.value)
      const fork = current.value.status === 'saved' && current.value.rowVersion > 0
      const itemIds = new Map<string, string>()
      if (fork) {
        draft.items.forEach((item) => itemIds.set(item.id, crypto.randomUUID()))
        draft = reidentifyPaper(draft, crypto.randomUUID(), itemIds)
        draft.rowVersion = 0
        draft.createdAt = Date.now()
        draft.savedAt = null
        draft.lastSavedAt = null
      }
      const saved = normalizePaper(await backend.savePaper(draft))

      // The backend response represents the snapshot sent above. If the
      // teacher edited this paper while the request was pending, retain those
      // live fields and merge only the authoritative persistence metadata.
      // If another paper was opened meanwhile, do not replace it at all.
      if (current.value.id !== paperId) return saved
      current.value = paperEditableSignature(current.value) === editableAtRequest
        ? saved
        : mergeSavedPaperMetadata(fork ? reidentifyPaper(current.value, saved.id, itemIds) : current.value, saved)
      savedSignature.value = paperEditableSignature(saved)
      if (!isDirty.value) await clearRecovery()
      // Export callers must receive the persisted snapshot, not later unsaved edits.
      return clonePlain(saved)
    } catch (reason) {
      error.value = errorMessage(reason, '保存试卷失败')
      throw reason
    } finally {
      saving.value = false
    }
  }

  async function loadPaper(id: string) {
    error.value = null
    return replaceCurrent(() => readStored(id))
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
    return replaceCurrent(() => backend.copyPaper(id, baseRowVersion))
  }

  async function deleteStored(id: string, baseRowVersion: number) {
    if (current.value.id === id && (isDirty.value || saving.value || transitioning.value)) {
      throw new Error('这份试卷仍在编辑，请先保存修改或切换到新试卷后再删除。')
    }
    await backend.deletePaper(id, baseRowVersion)
    papers.value = papers.value.filter((paper) => paper.id !== id)
    historyTotal.value = Math.max(0, historyTotal.value - 1)
    if (current.value.id === id) await newPaper()
  }

  async function deleteStoredBatch(items: readonly Pick<PaperSummary, 'id' | 'rowVersion'>[]) {
    if (!items.length) return
    if (items.some((item) => item.id === current.value.id) && (isDirty.value || saving.value || transitioning.value)) {
      throw new Error('选中的试卷仍在编辑，请先保存修改或切换到新试卷后再删除。')
    }
    await backend.deletePapers(items.map((item) => ({
      id: item.id,
      baseRowVersion: item.rowVersion,
    })))
    const deletedIds = new Set(items.map((item) => item.id))
    papers.value = papers.value.filter((paper) => !deletedIds.has(paper.id))
    historyTotal.value = Math.max(0, historyTotal.value - deletedIds.size)
    if (deletedIds.has(current.value.id)) await newPaper()
  }

  return {
    current,
    papers,
    historyTotal,
    saving,
    historyLoading,
    error,
    isDirty,
    transitioning,
    pendingRecovery,
    recoveryError,
    recoverySavedAt,
    initializeRecovery,
    recoverPaper,
    discardPendingRecovery,
    checkpoint,
    prepareToClose,
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
