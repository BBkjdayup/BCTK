<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import { onBeforeRouteLeave, useRoute, useRouter } from 'vue-router'
import { type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { ElMessage, ElMessageBox } from 'element-plus'
import { Plus, Delete, Check, DocumentAdd, UploadFilled } from '@element-plus/icons-vue'
import Sortable from 'sortablejs'
import RichTextEditor from '../components/RichTextEditor.vue'
import QuestionStemSummary from '../components/QuestionStemSummary.vue'
import { useAppStore } from '../stores/app'
import { useQuestionBankStore } from '../stores/questionBank'
import { backend } from '../services/backend'
import { errorMessage } from '../services/errors'
import {
  type QuestionDraft,
  type QuestionDuplicateCheck,
  type QuestionOption,
  type RichContent,
} from '../types/domain'
import { clonePlain } from '../utils/clonePlain'
import { emptyRichContent, plainTextRichContent } from '../utils/richContent'
import { enabledQuestionTypes, isChoiceQuestionType, questionTypeLabel } from '../utils/questionTypes'
import { resolveTaxonomyDefault } from '../utils/taxonomyDefaults'

const route = useRoute()
const router = useRouter()
const appStore = useAppStore()
const bankStore = useQuestionBankStore()
const saving = ref(false)
const dirty = ref(false)
const lastAutosavedAt = ref<number | null>(null)
const editorReady = ref(false)
const autosaving = ref(false)
const hasPersistedDraft = ref(false)
const currentQuestionVersion = ref<number | null>(null)
const duplicateDialogOpen = ref(false)
const duplicateCheck = ref<QuestionDuplicateCheck | null>(null)
const optionListRoot = ref<HTMLElement>()
let autosaveTimer: ReturnType<typeof setInterval> | undefined
let unlistenCloseRequested: UnlistenFn | undefined
let optionSortable: Sortable | null = null
let lastAutosavedSnapshot: string | null = null
let lastCleanSnapshot: string | null = null
let activeAutosave: Promise<boolean> | null = null
type DuplicateAction = 'skip' | 'overwrite' | 'keep'
let resolveDuplicateAction: ((action: DuplicateAction) => void) | undefined

const emptyRich = (): RichContent => emptyRichContent()
const createOption = (position: number): QuestionOption => ({ id: crypto.randomUUID(), position, content: emptyRich() })

const draft = reactive<QuestionDraft>({
  type: 'single_choice',
  stem: emptyRich(),
  options: [createOption(0), createOption(1), createOption(2), createOption(3)],
  answer: emptyRich(),
  explanation: emptyRich(),
  subjectId: '',
  chapterId: '',
  tagIds: [],
  resourceRefs: [],
})

const isEditing = computed(() => Boolean(route.params.id))
const copySourceId = computed(() => (
  !isEditing.value && typeof route.query.copy === 'string' ? route.query.copy : null
))
const isCopying = computed(() => Boolean(copySourceId.value))
const chapterOptions = computed(() => appStore.subjects.find((subject) => subject.id === draft.subjectId)?.chapters ?? [])
const typeOptions = computed(() => {
  const enabled = enabledQuestionTypes(appStore.questionTypes)
  const current = appStore.questionTypes.find((definition) => definition.code === draft.type)
  return current && !enabled.some((definition) => definition.code === current.code)
    ? [current, ...enabled]
    : enabled
})
const needsOptions = computed(() => isChoiceQuestionType(draft.type, appStore.questionTypes))
const isJudgment = computed(() => draft.type === 'true_false')

function syncSidebarSelection() {
  if (isEditing.value) return
  bankStore.filters.subjectId = draft.subjectId || undefined
  bankStore.filters.chapterId = draft.chapterId || undefined
  bankStore.filters.page = 1
}

function touch() {
  if (!editorReady.value || dirty.value || lastCleanSnapshot === null) return

  const markDirtyWhenContentChanged = () => {
    if (!editorReady.value || dirty.value || lastCleanSnapshot === null) return
    dirty.value = serializeDraft() !== lastCleanSnapshot
  }

  // Some controls emit input/change while they are mounting or refreshing.
  // Compare the actual draft both now and after Vue has applied a genuine
  // v-model update so those no-op events do not create a false draft warning.
  markDirtyWhenContentChanged()
  if (!dirty.value) void nextTick().then(markDirtyWhenContentChanged)
}

function serializeDraft() {
  return JSON.stringify({ ...draft, options: [...draft.options] })
}

function hasMeaningfulRichContent(content: RichContent) {
  if (content.plainText.trim()) return true
  const visibleHtml = content.html
    .replace(/<br\s*\/?\s*>/giu, '')
    .replace(/&nbsp;|&#160;|&#x0*a0;/giu, ' ')
    .replace(/<[^>]+>/gu, '')
    .trim()
  if (visibleHtml) return true
  return /<(?:img|table|hr)\b|data-latex\s*=/iu.test(content.html)
}

function hasMeaningfulQuestionContent(value: QuestionDraft) {
  return hasMeaningfulRichContent(value.stem)
    || value.options.some((option) => hasMeaningfulRichContent(option.content))
    || hasMeaningfulRichContent(value.answer)
    || hasMeaningfulRichContent(value.explanation)
    || (value.resourceRefs?.length ?? 0) > 0
}

function routeQuestionId() {
  return route.params.id ? String(route.params.id) : null
}

function updatePendingDraftCount(delta: number) {
  appStore.pendingDraftCount = Math.max(0, appStore.pendingDraftCount + delta)
}

async function persistLatestDraft(showFailure: boolean) {
  if (!hasMeaningfulQuestionContent(draft)) {
    if (!hasPersistedDraft.value) return true
    autosaving.value = true
    try {
      await deletePersistedDraft()
      return true
    } catch (reason) {
      if (showFailure) ElMessage.error(errorMessage(reason, '空白草稿清理失败，请检查本地数据库。'))
      return false
    } finally {
      autosaving.value = false
    }
  }
  const snapshot = serializeDraft()
  if (snapshot === lastAutosavedSnapshot) return true

  autosaving.value = true
  try {
    const saved = await backend.saveQuestionDraft(clonePlain(draft))
    if (!hasPersistedDraft.value) updatePendingDraftCount(1)
    hasPersistedDraft.value = true
    lastAutosavedAt.value = saved.autosavedAt
    lastAutosavedSnapshot = snapshot
    return true
  } catch (reason) {
    if (showFailure) ElMessage.error(errorMessage(reason, '草稿自动保存失败，请检查本地数据库。'))
    return false
  } finally {
    autosaving.value = false
  }
}

async function autosave(showFailure = true) {
  if (!editorReady.value || !dirty.value) return true

  // Multiple triggers (the 60-second timer, route navigation and window
  // closing) share one write. After each write, check the live editor again:
  // if the teacher typed while that request was pending, immediately persist
  // the newer snapshot before allowing navigation or shutdown to continue.
  while (editorReady.value && dirty.value) {
    if (activeAutosave) {
      if (!await activeAutosave) return false
      continue
    }
    if (!hasMeaningfulQuestionContent(draft) && !hasPersistedDraft.value) return true
    if (hasMeaningfulQuestionContent(draft) && serializeDraft() === lastAutosavedSnapshot) {
      return true
    }

    const operation = persistLatestDraft(showFailure)
    activeAutosave = operation
    let succeeded = false
    try {
      succeeded = await operation
    } finally {
      if (activeAutosave === operation) activeAutosave = null
    }
    if (!succeeded) return false
  }
  return true
}

async function deletePersistedDraft() {
  await backend.deleteQuestionDraft(routeQuestionId())
  if (hasPersistedDraft.value) updatePendingDraftCount(-1)
  hasPersistedDraft.value = false
  lastAutosavedAt.value = null
  lastAutosavedSnapshot = null
}

async function offerDraftRecovery() {
  const saved = await backend.getQuestionDraft(routeQuestionId())
  if (!saved) return

  hasPersistedDraft.value = true
  lastAutosavedAt.value = saved.autosavedAt
  if (!isEditing.value && !hasMeaningfulQuestionContent(saved.payload)) {
    // Earlier builds could persist a visually empty new-question draft after
    // editor initialization or classification-only changes. Those selections
    // are not useful without actual question content, so remove it silently.
    await deletePersistedDraft()
    return
  }
  const staleMessage = '该草稿保存后，题目已经在其他窗口更新。恢复旧草稿后再次保存，会覆盖当前最新内容；请选择是否仍要恢复。'
  const copyMessage = '发现一份尚未完成的“新增题目”草稿。恢复它会放弃当前复制内容；选择继续复制则会删除旧草稿。'
  const normalMessage = isCopying.value ? copyMessage : '发现这道题的未保存草稿，是否恢复到编辑器？'
  let restoreConfirmed = false
  try {
    await ElMessageBox.confirm(saved.stale ? staleMessage : normalMessage, saved.stale ? '草稿已过期' : isCopying.value ? '已有新增题草稿' : '恢复草稿', {
      type: saved.stale ? 'warning' : 'info',
      confirmButtonText: saved.stale ? '仍然恢复旧草稿' : isCopying.value ? '恢复原有草稿' : '恢复草稿',
      cancelButtonText: saved.stale ? '使用最新题目' : isCopying.value ? '删除旧草稿并继续复制' : '放弃草稿',
      closeOnClickModal: false,
      closeOnPressEscape: false,
    })
    restoreConfirmed = true
  } catch {
    restoreConfirmed = false
  }
  if (!restoreConfirmed) {
    await deletePersistedDraft()
    return
  }

  const recovered = clonePlain(saved.payload)
  if (saved.stale && currentQuestionVersion.value !== null) {
    // The user explicitly chose the old draft after seeing the conflict warning.
    // Rebase only the lock version; the recovered content itself is unchanged.
    recovered.baseContentVersion = currentQuestionVersion.value
  }
  Object.assign(draft, recovered)
  if (isCopying.value) await router.replace('/questions/new')
  dirty.value = true
  lastAutosavedSnapshot = saved.stale ? null : serializeDraft()
}

async function load() {
  if (route.params.id) {
    const question = await bankStore.getById(String(route.params.id))
    if (!question) {
      ElMessage.error('没有找到这道题目')
      await router.replace('/questions')
      return
    }
    Object.assign(draft, {
      questionId: question.id,
      type: question.type,
      stem: clonePlain(question.stem),
      options: clonePlain(question.options),
      answer: clonePlain(question.answer),
      explanation: clonePlain(question.explanation),
      subjectId: question.subjectId,
      chapterId: question.chapterId,
      tagIds: question.tags.map((tag) => tag.id),
      resourceRefs: clonePlain(question.resourceRefs ?? []),
      baseContentVersion: question.contentVersion,
    })
    currentQuestionVersion.value = question.contentVersion
  } else if (copySourceId.value) {
    const question = await bankStore.getById(copySourceId.value)
    if (!question) {
      ElMessage.error('没有找到要复制的原题')
      await router.replace('/questions')
      return
    }
    const copiedOptions = question.options.map((option, position) => ({
      id: crypto.randomUUID(),
      position,
      content: clonePlain(option.content),
    }))
    const copiedOptionIds = new Map(question.options.map((option, index) => [
      option.id,
      copiedOptions[index]?.id,
    ]))
    Object.assign(draft, {
      type: question.type,
      stem: clonePlain(question.stem),
      options: copiedOptions,
      answer: clonePlain(question.answer),
      explanation: clonePlain(question.explanation),
      subjectId: question.subjectId,
      chapterId: question.chapterId,
      tagIds: question.tags.map((tag) => tag.id),
      resourceRefs: (question.resourceRefs ?? []).map((entry) => ({
        ...clonePlain(entry),
        optionId: entry.contentSlot === 'option'
          ? copiedOptionIds.get(entry.optionId ?? '') ?? null
          : null,
      })),
    })
    dirty.value = true
  } else {
    const classification = resolveTaxonomyDefault(
      appStore.subjects,
      route.query.subjectId ?? bankStore.filters.subjectId,
      route.query.chapterId ?? bankStore.filters.chapterId,
    )
    draft.subjectId = classification.subject?.id ?? ''
    draft.chapterId = classification.chapter?.id ?? ''
  }

  lastCleanSnapshot = serializeDraft()
  try {
    await offerDraftRecovery()
  } catch (reason) {
    ElMessage.warning(errorMessage(reason, '读取本地草稿失败，已继续打开题目编辑器。'))
  }
  syncSidebarSelection()
  editorReady.value = true
}

watch(
  () => [route.query.subjectId, route.query.chapterId],
  ([subjectId, chapterId]) => {
    if (!editorReady.value || isEditing.value) return
    const classification = resolveTaxonomyDefault(appStore.subjects, subjectId, chapterId)
    const nextSubjectId = classification.subject?.id ?? ''
    const nextChapterId = classification.chapter?.id ?? ''
    if (draft.subjectId === nextSubjectId && draft.chapterId === nextChapterId) return
    draft.subjectId = nextSubjectId
    draft.chapterId = nextChapterId
    syncSidebarSelection()
    touch()
  },
)

watch(
  () => [
    needsOptions.value ? 'choice' : 'other',
    draft.options.map((option) => option.id).join(','),
  ].join('|'),
  () => void initializeOptionSortable(),
  { flush: 'post' },
)

function isTauriRuntime() {
  return '__TAURI_INTERNALS__' in window
}

function warnBeforeBrowserClose(event: BeforeUnloadEvent) {
  if (!dirty.value || !hasMeaningfulQuestionContent(draft)) return
  event.preventDefault()
  event.returnValue = ''
}

async function registerCloseProtection() {
  if (!isTauriRuntime()) {
    window.addEventListener('beforeunload', warnBeforeBrowserClose)
    return
  }

  try {
    unlistenCloseRequested = await getCurrentWindow().onCloseRequested(async (event) => {
      if (!dirty.value) return
      if (!hasMeaningfulQuestionContent(draft)) {
        if (!hasPersistedDraft.value) return
        event.preventDefault()
        if (await autosave()) await getCurrentWindow().destroy()
        return
      }
      event.preventDefault()
      try {
        await ElMessageBox.confirm('当前题目还有未保存的修改。关闭软件前先保存本地草稿吗？', '关闭软件', {
          type: 'warning',
          confirmButtonText: '保存草稿并关闭',
          cancelButtonText: '继续编辑',
          closeOnClickModal: false,
        })
        if (await autosave()) await getCurrentWindow().destroy()
      } catch {
        // The teacher chose to keep editing, so the close request stays cancelled.
      }
    })
  } catch {
    // Keep the browser-level protection as a fallback if window events are unavailable.
    window.addEventListener('beforeunload', warnBeforeBrowserClose)
  }
}

onMounted(() => {
  void load()
  void registerCloseProtection()
  void initializeOptionSortable()
  autosaveTimer = setInterval(() => { void autosave() }, 60000)
  window.addEventListener('keydown', onKeydown)
})

onBeforeUnmount(() => {
  optionSortable?.destroy()
  optionSortable = null
  clearInterval(autosaveTimer)
  unlistenCloseRequested?.()
  window.removeEventListener('beforeunload', warnBeforeBrowserClose)
  window.removeEventListener('keydown', onKeydown)
  resolveDuplicateAction?.('skip')
  resolveDuplicateAction = undefined
})

onBeforeRouteLeave(async () => {
  if (!dirty.value) return true
  if (!hasMeaningfulQuestionContent(draft)) return await autosave()
  try {
    await ElMessageBox.confirm('当前题目还有未保存的修改，确定离开吗？草稿会保留。', '未保存内容', {
      type: 'warning',
      confirmButtonText: '保留草稿并离开',
      cancelButtonText: '继续编辑',
    })
    return await autosave()
  } catch {
    return false
  }
})

function onKeydown(event: KeyboardEvent) {
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 's') {
    event.preventDefault()
    void save()
  }
}

function onTypeChanged() {
  const defaults = appStore.questionTypes.find((definition) => definition.code === draft.type)?.defaultOptions ?? []
  if (isJudgment.value && defaults.length >= 2) {
    draft.options = defaults.map((content, position) => ({
      ...createOption(position),
      content: plainTextRichContent(content),
    }))
  } else if (needsOptions.value && draft.options.length < 2) {
    draft.options = (defaults.length >= 2 ? defaults : ['', '', '', '']).map((content, position) => ({
      ...createOption(position),
      content: content ? plainTextRichContent(content) : emptyRich(),
    }))
  }
  if (!needsOptions.value) {
    draft.options = []
    draft.resourceRefs = (draft.resourceRefs ?? []).filter((entry) => entry.contentSlot !== 'option')
  }
  touch()
}

function onSubjectChanged() {
  draft.chapterId = chapterOptions.value[0]?.id ?? ''
  syncSidebarSelection()
  touch()
}

function onChapterChanged() {
  syncSidebarSelection()
  touch()
}

function addOption() {
  draft.options.push(createOption(draft.options.length))
  touch()
}

function updateOptionContent(index: number, content: RichContent) {
  const option = draft.options[index]
  if (!option) return
  option.content = content
  touch()
}

function removeOption(index: number) {
  if (draft.options.length <= 2) {
    ElMessage.warning('选择题至少需要两个选项')
    return
  }
  const removed = draft.options[index]
  draft.options.splice(index, 1)
  if (removed) {
    draft.resourceRefs = (draft.resourceRefs ?? []).filter((entry) => entry.optionId !== removed.id)
  }
  draft.options.forEach((option, position) => { option.position = position })
  touch()
}

function moveOption(source: number, target: number) {
  if (source === target || source < 0 || target < 0) return
  if (source >= draft.options.length || target >= draft.options.length) return
  const [item] = draft.options.splice(source, 1)
  if (!item) return
  draft.options.splice(target, 0, item)
  draft.options.forEach((option, position) => { option.position = position })
  touch()
}

async function initializeOptionSortable() {
  await nextTick()
  optionSortable?.destroy()
  optionSortable = null

  if (!optionListRoot.value || !needsOptions.value || isJudgment.value || draft.options.length < 2) return
  optionSortable = new Sortable(optionListRoot.value, {
    animation: 180,
    draggable: '.option-item',
    handle: '.option-drag-handle',
    forceFallback: true,
    fallbackOnBody: true,
    fallbackTolerance: 4,
    chosenClass: 'is-drag-chosen',
    ghostClass: 'is-drag-ghost',
    dragClass: 'is-dragging',
    onEnd: (event) => {
      const source = event.oldDraggableIndex
      const target = event.newDraggableIndex
      if (source == null || target == null) return
      moveOption(source, target)
    },
  })
}

function validate() {
  if (!draft.type) return '请选择题型'
  if (!draft.subjectId) return '请选择学科'
  if (!draft.chapterId) return '请选择章节'
  if (!draft.stem.plainText.trim()) return '请填写题干'
  if (!draft.answer.plainText.trim()) return '请填写答案'
  if (needsOptions.value && draft.options.some((option) => !option.content.plainText.trim())) return '选择题的选项不能为空'
  return null
}

function askDuplicateAction(result: QuestionDuplicateCheck) {
  resolveDuplicateAction?.('skip')
  duplicateCheck.value = result
  duplicateDialogOpen.value = true
  return new Promise<DuplicateAction>((resolve) => {
    resolveDuplicateAction = resolve
  })
}

function chooseDuplicateAction(action: DuplicateAction) {
  const resolve = resolveDuplicateAction
  resolveDuplicateAction = undefined
  duplicateDialogOpen.value = false
  resolve?.(action)
}

function onDuplicateDialogClosed() {
  if (!resolveDuplicateAction) return
  const resolve = resolveDuplicateAction
  resolveDuplicateAction = undefined
  resolve('skip')
}

async function save() {
  const validationError = validate()
  if (validationError) {
    ElMessage.error(validationError)
    return
  }
  saving.value = true
  try {
    const duplicate = await backend.checkQuestionDuplicate(clonePlain(draft))
    let duplicateAction: DuplicateAction = 'keep'
    if (duplicate.status !== 'none' && duplicate.candidate) {
      duplicateAction = await askDuplicateAction(duplicate)
      if (duplicateAction === 'skip') {
        ElMessage.info('已跳过本次保存；当前内容仍保留在编辑器和本地草稿中。')
        return
      }
    }

    const draftToSave = clonePlain(draft)
    if (duplicateAction === 'overwrite' && duplicate.candidate) {
      draftToSave.questionId = duplicate.candidate.id
      draftToSave.baseContentVersion = duplicate.candidate.contentVersion
    }
    const savedQuestion = await bankStore.save(draftToSave)
    if (duplicateAction !== 'overwrite') draft.baseContentVersion = savedQuestion.contentVersion
    lastCleanSnapshot = serializeDraft()
    dirty.value = false
    try {
      await deletePersistedDraft()
    } catch (reason) {
      ElMessage.warning(errorMessage(reason, '题目已保存，但旧草稿清理失败；下次打开时请放弃该草稿。'))
    }
    ElMessage.success(
      duplicateAction === 'overwrite'
        ? '已用当前内容覆盖题库中的原题'
        : isEditing.value ? '题目已更新' : isCopying.value ? '题目副本已保存' : '题目已保存',
    )
    await router.push('/questions')
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '保存失败，请稍后重试'))
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <section class="page-main editor-page" @input="touch" @change="touch">
    <div class="editor-actions-bar" role="toolbar" aria-label="题目保存操作">
      <div class="autosave-status">
        <span class="status-dot" />
        <span v-if="autosaving">正在保存本地草稿…</span>
        <span v-else-if="lastAutosavedAt">草稿已于 {{ new Date(lastAutosavedAt).toLocaleTimeString('zh-CN') }} 保存</span>
        <span v-else>每 60 秒自动保存草稿</span>
      </div>
      <div>
        <el-button
          v-if="!isEditing && !isCopying"
          :icon="DocumentAdd"
          :disabled="!appStore.license.capabilities.canBatchImport"
          :title="appStore.license.capabilities.canBatchImport ? '文档式批量录入' : '桌面专业版可使用批量录入'"
          @click="router.push('/questions/document')"
        >{{ appStore.license.capabilities.canBatchImport ? '文档式批量录入' : '专业版批量录入' }}</el-button>
        <el-button
          v-if="!isEditing && !isCopying"
          :icon="UploadFilled"
          :disabled="!appStore.license.capabilities.canBatchImport"
          :title="appStore.license.capabilities.canBatchImport ? 'Word / Excel 批量导入' : '桌面专业版可使用批量导入'"
          @click="router.push({
            path: '/word-import',
            query: { subjectId: draft.subjectId, chapterId: draft.chapterId },
          })"
        >{{ appStore.license.capabilities.canBatchImport ? 'Word / Excel 批量导入' : '专业版批量导入' }}</el-button>
        <el-button type="primary" :icon="Check" :loading="saving" @click="save">保存题目</el-button>
      </div>
    </div>

    <div class="editor-card surface">
      <div class="editor-grid editor-grid--meta">
        <el-form-item label="题型" required>
          <el-select v-model="draft.type" @change="onTypeChanged">
            <el-option v-for="type in typeOptions" :key="type.code" :label="type.name" :value="type.code" :disabled="!type.isEnabled" />
          </el-select>
        </el-form-item>
        <el-form-item label="学科" required>
          <el-select v-model="draft.subjectId" filterable @change="onSubjectChanged">
            <el-option v-for="subject in appStore.subjects" :key="subject.id" :label="subject.name" :value="subject.id" />
          </el-select>
        </el-form-item>
        <el-form-item label="章节" required>
          <el-select v-model="draft.chapterId" filterable @change="onChapterChanged">
            <el-option v-for="chapter in chapterOptions" :key="chapter.id" :label="chapter.name" :value="chapter.id" />
          </el-select>
        </el-form-item>
        <el-form-item label="标签">
          <el-select v-model="draft.tagIds" multiple filterable allow-create default-first-option collapse-tags>
            <el-option v-for="tag in appStore.tags" :key="tag.id" :label="tag.name" :value="tag.id" />
          </el-select>
        </el-form-item>
      </div>

      <div class="form-section">
        <label>题干 <span>*</span></label>
        <RichTextEditor
          v-model="draft.stem"
          placeholder="请输入完整题干，可直接粘贴文字或图片"
          :min-height="130"
          toolbar-mode="hidden"
          context-tools
        />
      </div>

      <div v-if="needsOptions" class="form-section">
        <div class="form-section__heading">
          <label>选项 <span>*</span></label>
          <el-button v-if="!isJudgment" size="small" :icon="Plus" @click="addOption">新增选项</el-button>
        </div>
        <div ref="optionListRoot" class="option-list">
          <div v-for="(option, index) in draft.options" :key="option.id" class="option-item">
            <span
              v-if="!isJudgment"
              class="option-drag-handle"
              aria-label="拖动调整选项顺序"
              title="按住拖动调整顺序"
            >⠿</span>
            <div class="option-letter">{{ String.fromCharCode(65 + index) }}</div>
            <RichTextEditor
              :model-value="option.content"
              :placeholder="`选项 ${String.fromCharCode(65 + index)}`"
              :min-height="48"
              toolbar-mode="hidden"
              context-tools
              @update:model-value="updateOptionContent(index, $event)"
            />
            <div v-if="!isJudgment" class="option-actions">
              <el-button text circle size="small" type="danger" title="删除" @click="removeOption(index)"><el-icon><Delete /></el-icon></el-button>
            </div>
          </div>
        </div>
      </div>

      <div class="editor-grid editor-grid--content">
        <div class="form-section">
          <label>答案 <span>*</span></label>
          <RichTextEditor
            v-model="draft.answer"
            placeholder="请输入标准答案；选择题可填写 A 或 A、B、C"
            :min-height="100"
            toolbar-mode="hidden"
            context-tools
          />
        </div>
        <div class="form-section">
          <label>解析</label>
          <RichTextEditor
            v-model="draft.explanation"
            placeholder="请输入解题思路、知识点或评分说明"
            :min-height="100"
            toolbar-mode="hidden"
            context-tools
          />
        </div>
      </div>
    </div>

    <el-dialog
      v-model="duplicateDialogOpen"
      :title="duplicateCheck?.status === 'exact' ? '发现完全重复题目' : '发现疑似重复题目'"
      width="760px"
      :close-on-click-modal="false"
      @closed="onDuplicateDialogClosed"
    >
      <template v-if="duplicateCheck?.candidate">
        <div class="duplicate-warning">
          系统根据题干、选项和答案判断为{{ duplicateCheck.status === 'exact' ? '完全重复' : '疑似重复' }}，请由您决定如何处理。
        </div>
        <div class="duplicate-compare">
          <article>
            <header><strong>正在保存的题目</strong><el-tag size="small">当前内容</el-tag></header>
            <QuestionStemSummary class="duplicate-compare__stem" :content="draft.stem" :lines="3" />
            <small>{{ questionTypeLabel(draft.type, appStore.questionTypes) }} · {{ chapterOptions.find((item) => item.id === draft.chapterId)?.name ?? '未选择章节' }}</small>
          </article>
          <article>
            <header>
              <strong>题库中的相似题</strong>
              <el-tag size="small" type="warning">相似度 {{ duplicateCheck.candidate.similarityPercent }}%</el-tag>
            </header>
            <QuestionStemSummary
              class="duplicate-compare__stem"
              :text="duplicateCheck.candidate.stemPreview"
              :lines="3"
            />
            <small>{{ questionTypeLabel(duplicateCheck.candidate.type, appStore.questionTypes) }} · {{ duplicateCheck.candidate.subjectName }} / {{ duplicateCheck.candidate.chapterName }}</small>
          </article>
        </div>
        <p v-if="isEditing" class="duplicate-note">
          选择“覆盖已有题目”会把当前内容写入上方相似题；您正在编辑的这条题目会保持修改前的已保存内容。
        </p>
      </template>
      <template #footer>
        <el-button @click="chooseDuplicateAction('skip')">取消本次录入</el-button>
        <el-button @click="chooseDuplicateAction('overwrite')">覆盖已有题目</el-button>
        <el-button type="primary" @click="chooseDuplicateAction('keep')">仍然录入新题</el-button>
      </template>
    </el-dialog>
  </section>
</template>

<style scoped>
.editor-page {
  height: 100%;
}

.editor-card {
  max-width: 1180px;
  margin: 0 auto;
  padding: 20px;
}

.editor-grid {
  display: grid;
  gap: 14px;
}

.duplicate-warning {
  padding: 11px 13px;
  border: 1px solid #fed7aa;
  border-radius: 8px;
  background: #fffaf0;
  color: #9a5b13;
  font-size: 12px;
}

.duplicate-compare {
  margin-top: 14px;
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 12px;
}

.duplicate-compare article {
  min-height: 150px;
  padding: 16px;
  border: 1px solid #dfe6ef;
  border-radius: 9px;
  background: #fff;
}

.duplicate-compare header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.duplicate-compare__stem {
  margin: 18px 0;
  color: #172033;
  font-size: 13px;
  font-weight: 650;
  line-height: 1.7;
}

.duplicate-compare small,
.duplicate-note {
  color: #64748b;
  font-size: 11px;
  line-height: 1.7;
}

.duplicate-note {
  margin: 12px 0 0;
}

.editor-grid--meta {
  grid-template-columns: repeat(4, minmax(150px, 1fr));
}

.editor-grid--content {
  grid-template-columns: 1fr 1fr;
}

.editor-grid :deep(.el-form-item) {
  margin-bottom: 4px;
}

.editor-grid :deep(.el-select) {
  width: 100%;
}

.form-section {
  margin-top: 17px;
}

.form-section > label,
.form-section__heading label {
  margin-bottom: 8px;
  display: block;
  color: #334155;
  font-size: 12px;
  font-weight: 650;
}

.form-section label span {
  color: #dc2626;
}

.form-section__heading {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.option-list {
  display: grid;
  gap: 8px;
}

.option-item {
  display: grid;
  grid-template-columns: 20px 34px minmax(0, 1fr) 36px;
  align-items: center;
  gap: 8px;
}

.option-drag-handle {
  display: grid;
  place-items: center;
  color: #94a3b8;
  cursor: grab;
  font-size: 16px;
  line-height: 1;
  touch-action: none;
  user-select: none;
}

.option-drag-handle:active {
  cursor: grabbing;
}

.option-letter {
  width: 30px;
  height: 30px;
  display: grid;
  place-items: center;
  border-radius: 6px;
  background: #eff6ff;
  color: #2563eb;
  font-size: 12px;
  font-weight: 700;
}

.option-actions {
  display: flex;
  justify-content: center;
}

.option-item.is-drag-chosen {
  border-radius: 8px;
  background: #eff6ff;
  box-shadow: inset 0 0 0 1px #93c5fd;
}

.option-item.is-drag-ghost {
  opacity: 0.35;
}

.option-item.is-dragging,
.option-item.sortable-fallback {
  border-radius: 8px;
  background: #fff;
  box-shadow: 0 10px 24px rgb(15 23 42 / 18%);
}

.editor-actions-bar {
  position: sticky;
  top: -26px;
  z-index: 6;
  height: 64px;
  margin: -26px -28px 24px;
  padding: 0 28px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  border-bottom: 1px solid #e5eaf2;
  background: rgb(255 255 255 / 96%);
  box-shadow: 0 5px 20px rgb(15 23 42 / 5%);
  backdrop-filter: blur(10px);
}

.autosave-status {
  display: flex;
  align-items: center;
  gap: 9px;
  color: #64748b;
  font-size: 11px;
}

.editor-actions-bar > div:last-child {
  display: flex;
  gap: 8px;
}

@media (max-width: 1280px) {
  .editor-actions-bar {
    top: -20px;
    margin-top: -20px;
    margin-right: -20px;
    margin-bottom: 20px;
    margin-left: -20px;
    padding-right: 20px;
    padding-left: 20px;
  }
}

@media (max-width: 1200px) {
  .editor-grid--meta {
    grid-template-columns: repeat(2, 1fr);
  }
}
</style>
