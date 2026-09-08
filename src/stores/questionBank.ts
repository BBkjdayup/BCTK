import { defineStore } from 'pinia'
import { computed, reactive, ref } from 'vue'
import { backend } from '../services/backend'
import { errorMessage } from '../services/errors'
import { useAppStore } from './app'
import type {
  Question,
  QuestionBatchEditRequest,
  QuestionDraft,
  QuestionFilters,
} from '../types/domain'

export function createQuestionFilters(overrides: Partial<QuestionFilters> = {}): QuestionFilters {
  const { tagIds = [], tagMatchMode = 'all', ...filterOverrides } = overrides
  return {
    keyword: '',
    subjectId: undefined,
    chapterId: undefined,
    type: undefined,
    usage: 'all',
    deleted: false,
    page: 1,
    pageSize: 100,
    ...filterOverrides,
    tagIds: [...tagIds],
    tagMatchMode,
  }
}

function filterSignature(filters: QuestionFilters) {
  return JSON.stringify([
    filters.keyword,
    filters.subjectId ?? null,
    filters.chapterId ?? null,
    filters.type ?? null,
    filters.tagIds,
    filters.tagMatchMode,
    filters.usage,
    filters.deleted,
    filters.page,
    filters.pageSize,
  ])
}

export const useQuestionBankStore = defineStore('questionBank', () => {
  const appStore = useAppStore()
  const filters = reactive<QuestionFilters>(createQuestionFilters())
  const questions = ref<Question[]>([])
  const total = ref(0)
  const loading = ref(false)
  const selectedIds = ref<string[]>([])
  const previewQuestion = ref<Question | null>(null)
  const error = ref<string | null>(null)
  let loadSequence = 0

  const hasSelection = computed(() => selectedIds.value.length > 0)

  async function load() {
    const requestSequence = ++loadSequence
    const requestedFilters = { ...filters, tagIds: [...filters.tagIds] }
    const requestedSignature = filterSignature(requestedFilters)
    loading.value = true
    error.value = null
    try {
      const page = await backend.listQuestions(requestedFilters)
      if (requestSequence !== loadSequence || requestedSignature !== filterSignature(filters)) return
      questions.value = page.items
      total.value = page.total
      selectedIds.value = selectedIds.value.filter((id) => page.items.some((item) => item.id === id))
    } catch (reason) {
      if (requestSequence !== loadSequence || requestedSignature !== filterSignature(filters)) return
      error.value = errorMessage(reason, '题目加载失败')
    } finally {
      if (requestSequence === loadSequence) loading.value = false
    }
  }

  async function getById(id: string) {
    return backend.getQuestion(id)
  }

  async function refreshAfterQuestionMutation() {
    await Promise.all([load(), appStore.refreshTaxonomy()])
  }

  async function save(draft: QuestionDraft) {
    const saved = await backend.saveQuestion(draft)
    await refreshAfterQuestionMutation()
    return saved
  }

  async function moveToRecycle(ids: string[]) {
    await backend.moveQuestionsToRecycle(ids)
    selectedIds.value = []
    previewQuestion.value = null
    await refreshAfterQuestionMutation()
  }

  async function batchEdit(request: QuestionBatchEditRequest) {
    const result = await backend.batchEditQuestions(request)
    selectedIds.value = []
    previewQuestion.value = null
    await refreshAfterQuestionMutation()
    return result
  }

  async function restore(ids: string[]) {
    await backend.restoreQuestions(ids)
    selectedIds.value = []
    await refreshAfterQuestionMutation()
  }

  async function permanentlyDelete(ids: string[]) {
    await backend.permanentlyDeleteQuestions(ids)
    selectedIds.value = []
    await refreshAfterQuestionMutation()
  }

  function initializeFilters(overrides: Partial<QuestionFilters> = {}) {
    loadSequence += 1
    Object.assign(filters, createQuestionFilters(overrides))
    questions.value = []
    total.value = 0
    loading.value = false
    selectedIds.value = []
    previewQuestion.value = null
    error.value = null
  }

  function resetFilters() {
    initializeFilters({ deleted: filters.deleted })
  }

  return {
    filters,
    questions,
    total,
    loading,
    selectedIds,
    previewQuestion,
    error,
    hasSelection,
    load,
    getById,
    save,
    batchEdit,
    moveToRecycle,
    restore,
    permanentlyDelete,
    initializeFilters,
    resetFilters,
  }
})
