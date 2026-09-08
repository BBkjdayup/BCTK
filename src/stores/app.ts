import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { backend } from '../services/backend'
import { errorMessage } from '../services/errors'
import { basicLicenseOverview } from '../utils/licensing'
import type {
  BootstrapData,
  LicenseOverview,
  QuestionTypeDefinition,
  SaveQuestionTypeRequest,
  Subject,
  Tag,
  TaxonomyOrderItem,
} from '../types/domain'

export const useAppStore = defineStore('app', () => {
  const loading = ref(true)
  const initialized = ref(false)
  const databaseHealthy = ref(true)
  const dataRoot = ref('')
  const appVersion = ref(__APP_VERSION__)
  const pendingDraftCount = ref(0)
  const subjects = ref<Subject[]>([])
  const tags = ref<Tag[]>([])
  const questionTypes = ref<QuestionTypeDefinition[]>([])
  const error = ref<string | null>(null)
  const license = ref<LicenseOverview>(basicLicenseOverview())
  let bootstrapRequestSequence = 0

  const questionCount = computed(() => subjects.value.reduce((sum, subject) => sum + subject.questionCount, 0))

  function applyBootstrap(bootstrap: BootstrapData) {
    initialized.value = bootstrap.initialized
    databaseHealthy.value = bootstrap.databaseHealthy
    error.value = bootstrap.databaseError ?? null
    dataRoot.value = bootstrap.dataRoot
    appVersion.value = bootstrap.appVersion
    pendingDraftCount.value = bootstrap.pendingDraftCount
    subjects.value = bootstrap.subjects
    tags.value = bootstrap.tags
    questionTypes.value = bootstrap.questionTypes
    license.value = bootstrap.license
  }

  async function refreshLicense() {
    license.value = await backend.getLicenseOverview()
  }

  function applyLicense(overview: LicenseOverview) {
    license.value = overview
  }

  async function refreshAfterMutation() {
    const requestSequence = ++bootstrapRequestSequence
    const bootstrap = await backend.initialize()
    if (requestSequence !== bootstrapRequestSequence) return
    applyBootstrap(bootstrap)
  }

  async function refreshTaxonomy() {
    const requestSequence = ++bootstrapRequestSequence
    try {
      const bootstrap = await backend.initialize()
      if (requestSequence !== bootstrapRequestSequence) return false
      applyBootstrap(bootstrap)
      return true
    } catch {
      return false
    } finally {
      if (requestSequence === bootstrapRequestSequence) loading.value = false
    }
  }

  async function initialize() {
    const requestSequence = ++bootstrapRequestSequence
    loading.value = true
    error.value = null
    try {
      const bootstrap: BootstrapData = await backend.initialize()
      if (requestSequence !== bootstrapRequestSequence) return
      applyBootstrap(bootstrap)
    } catch (reason) {
      if (requestSequence !== bootstrapRequestSequence) return
      databaseHealthy.value = false
      error.value = errorMessage(reason, '本地数据库加载失败')
    } finally {
      if (requestSequence === bootstrapRequestSequence) loading.value = false
    }
  }

  async function createInitialSubject(name: string) {
    await backend.createInitialSubject(name)
    await refreshAfterMutation()
  }

  async function retryDatabase() {
    loading.value = true
    error.value = null
    try {
      await backend.retryDatabaseInitialize()
      await initialize()
    } catch (reason) {
      databaseHealthy.value = false
      error.value = errorMessage(reason, '本地数据库重新加载失败')
      loading.value = false
    }
  }

  async function createSubject(name: string) {
    const id = await backend.createSubject(name)
    await refreshAfterMutation()
    return id
  }

  async function updateSubject(id: string, name: string) {
    await backend.updateSubject(id, name)
    await refreshAfterMutation()
  }

  async function deleteSubject(id: string) {
    await backend.deleteSubject(id)
    await refreshAfterMutation()
  }

  async function createChapter(subjectId: string, name: string) {
    const id = await backend.createChapter(subjectId, name)
    await refreshAfterMutation()
    return id
  }

  async function updateChapter(id: string, name: string) {
    await backend.updateChapter(id, name)
    await refreshAfterMutation()
  }

  async function deleteChapter(id: string) {
    await backend.deleteChapter(id)
    await refreshAfterMutation()
  }

  async function createTag(name: string) {
    const id = await backend.createTag(name)
    await refreshAfterMutation()
    return id
  }

  async function updateTag(id: string, name: string) {
    await backend.updateTag(id, name)
    await refreshAfterMutation()
  }

  async function deleteTag(id: string) {
    await backend.deleteTag(id)
    await refreshAfterMutation()
  }

  async function saveSubjectOrder(items: TaxonomyOrderItem[]) {
    await backend.saveSubjectOrder(items)
    await refreshAfterMutation()
  }

  async function saveChapterOrder(subjectId: string, items: TaxonomyOrderItem[]) {
    await backend.saveChapterOrder(subjectId, items)
    await refreshAfterMutation()
  }

  async function saveQuestionType(request: SaveQuestionTypeRequest) {
    const saved = await backend.saveQuestionType(request)
    await refreshAfterMutation()
    return saved
  }

  async function deleteQuestionType(code: string) {
    await backend.deleteQuestionType(code)
    await refreshAfterMutation()
  }

  async function saveQuestionTypeOrder(items: TaxonomyOrderItem[]) {
    await backend.saveQuestionTypeOrder(items)
    await refreshAfterMutation()
  }

  return {
    loading,
    initialized,
    databaseHealthy,
    dataRoot,
    appVersion,
    pendingDraftCount,
    subjects,
    tags,
    questionTypes,
    error,
    license,
    questionCount,
    initialize,
    refreshLicense,
    applyLicense,
    refreshTaxonomy,
    retryDatabase,
    createInitialSubject,
    createSubject,
    updateSubject,
    deleteSubject,
    createChapter,
    updateChapter,
    deleteChapter,
    createTag,
    updateTag,
    deleteTag,
    saveSubjectOrder,
    saveChapterOrder,
    saveQuestionType,
    deleteQuestionType,
    saveQuestionTypeOrder,
  }
})
