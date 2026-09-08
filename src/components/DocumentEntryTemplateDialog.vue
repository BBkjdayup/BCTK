<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import type {
  DocumentEntryOptionStyle,
  DocumentEntryTemplate,
  DocumentEntryTemplateConfig,
  SaveDocumentEntryTemplateRequest,
} from '../types/domain'
import { backend } from '../services/backend'
import { errorMessage } from '../services/errors'
import { clonePlain } from '../utils/clonePlain'
import {
  buildDocumentEntryTemplateExample,
  DEFAULT_DOCUMENT_ENTRY_TEMPLATE_CONFIG,
  normalizeDocumentEntryTemplateConfig,
  validateDocumentEntryTemplateConfig,
} from '../utils/documentEntryTemplates'

const props = defineProps<{
  modelValue: boolean
  templates: DocumentEntryTemplate[]
  activeTemplateId: string
}>()

const emit = defineEmits<{
  'update:modelValue': [value: boolean]
  'saved': [template: DocumentEntryTemplate]
  'deleted': [id: string]
  'defaulted': [template: DocumentEntryTemplate]
  'select': [id: string]
}>()

const selectedId = ref('')
const editingId = ref<string | null>(null)
const saving = ref(false)
const deleting = ref(false)
const form = reactive({
  name: '',
  setAsDefault: false,
  config: clonePlain(DEFAULT_DOCUMENT_ENTRY_TEMPLATE_CONFIG),
})

const visible = computed({
  get: () => props.modelValue,
  set: (value: boolean) => emit('update:modelValue', value),
})
const selected = computed(() => props.templates.find((template) => template.id === selectedId.value) ?? null)
const validationError = computed(() => {
  if (!form.name.trim()) return '请填写模板名称。'
  if ([...form.name.trim()].length > 40) return '模板名称不能超过 40 个字符。'
  return validateDocumentEntryTemplateConfig(form.config)
})
const preview = computed(() => buildDocumentEntryTemplateExample(form.config))
const editingExisting = computed(() => Boolean(editingId.value))

const optionStyles: Array<{ value: DocumentEntryOptionStyle; label: string }> = [
  { value: 'letter_dot', label: 'A. 选项内容' },
  { value: 'letter_comma', label: 'A、选项内容' },
  { value: 'letter_parentheses', label: '（A）选项内容' },
  { value: 'letter_brackets', label: '【A】选项内容' },
]

watch(
  () => props.modelValue,
  (open) => {
    if (!open) return
    const preferred = props.templates.find((template) => template.id === props.activeTemplateId)
      ?? props.templates.find((template) => template.isDefault)
      ?? props.templates[0]
    if (preferred) selectTemplate(preferred)
    else startNew()
  },
)

watch(
  () => props.templates,
  (templates) => {
    if (!props.modelValue || !templates.length) return
    const current = templates.find((template) => template.id === selectedId.value)
    if (current && !editingId.value) selectTemplate(current)
  },
)

function loadForm(name: string, config: DocumentEntryTemplateConfig, setAsDefault = false) {
  form.name = name
  form.setAsDefault = setAsDefault
  form.config = normalizeDocumentEntryTemplateConfig(config)
}

function selectTemplate(template: DocumentEntryTemplate) {
  selectedId.value = template.id
  editingId.value = template.isBuiltIn ? null : template.id
  loadForm(template.name, template.config, template.isDefault)
}

function startNew() {
  selectedId.value = ''
  editingId.value = null
  loadForm('我的录题模板', DEFAULT_DOCUMENT_ENTRY_TEMPLATE_CONFIG)
}

function copySelected() {
  const template = selected.value
  if (!template) {
    startNew()
    return
  }
  selectedId.value = ''
  editingId.value = null
  loadForm(`${template.name} 副本`, template.config)
}

async function save() {
  if (validationError.value || saving.value) return
  saving.value = true
  try {
    const request: SaveDocumentEntryTemplateRequest = {
      id: editingId.value,
      name: form.name.trim(),
      config: clonePlain(form.config),
      setAsDefault: form.setAsDefault,
    }
    const template = await backend.saveDocumentEntryTemplate(request)
    editingId.value = template.id
    selectedId.value = template.id
    emit('saved', template)
    ElMessage.success('录题模板已保存到 SQLite。')
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '录题模板没有保存，请检查填写内容。'))
  } finally {
    saving.value = false
  }
}

async function setDefault() {
  const template = selected.value
  if (!template || template.isDefault) return
  try {
    const updated = await backend.setDefaultDocumentEntryTemplate(template.id)
    emit('defaulted', updated)
    ElMessage.success(`“${updated.name}”已设为默认录题模板。`)
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '默认录题模板没有修改。'))
  }
}

async function removeSelected() {
  const template = selected.value
  if (!template || template.isBuiltIn || deleting.value) return
  try {
    await ElMessageBox.confirm(
      `删除“${template.name}”后，已有草稿仍会使用保存的模板快照继续识别。`,
      '删除录题模板',
      {
        confirmButtonText: '删除模板',
        cancelButtonText: '取消',
        type: 'warning',
      },
    )
  } catch {
    return
  }
  deleting.value = true
  try {
    await backend.deleteDocumentEntryTemplate(template.id)
    emit('deleted', template.id)
    const fallback = props.templates.find((item) => item.id !== template.id && item.isBuiltIn)
      ?? props.templates.find((item) => item.id !== template.id)
    if (fallback) selectTemplate(fallback)
    ElMessage.success('自定义录题模板已删除。')
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '录题模板没有删除。'))
  } finally {
    deleting.value = false
  }
}

function useSelected() {
  const id = editingId.value || selectedId.value
  if (!id) {
    ElMessage.info('请先保存这套自定义模板。')
    return
  }
  emit('select', id)
  visible.value = false
}
</script>

<template>
  <el-dialog
    v-model="visible"
    title="录题模板设置"
    width="980px"
    :close-on-click-modal="!saving"
    :close-on-press-escape="!saving"
    destroy-on-close
  >
    <div class="template-dialog">
      <aside class="template-list">
        <div class="template-list__header">
          <strong>已保存模板</strong>
          <el-button size="small" @click="startNew">新建</el-button>
        </div>
        <button
          v-for="template in templates"
          :key="template.id"
          type="button"
          class="template-list__item"
          :class="{ active: selectedId === template.id }"
          @click="selectTemplate(template)"
        >
          <span>{{ template.name }}</span>
          <small>
            {{ template.isBuiltIn ? '系统模板' : '自定义模板' }}
            <em v-if="template.isDefault">默认</em>
          </small>
        </button>
        <el-button class="copy-button" :disabled="!selected" @click="copySelected">复制当前模板</el-button>
      </aside>

      <main class="template-form">
        <el-alert
          v-if="selected?.isBuiltIn && !editingExisting"
          type="info"
          :closable="false"
          show-icon
        >
          系统默认模板不能直接修改。点击“复制当前模板”即可在它的基础上创建自己的格式。
        </el-alert>

        <section>
          <h3>模板名称</h3>
          <el-input v-model="form.name" maxlength="40" show-word-limit :disabled="selected?.isBuiltIn && !editingExisting" />
        </section>

        <section>
          <h3>识别标记</h3>
          <p>这些文字会参与题目识别，可以输入“题目：”“【题干】”或“问题→”等普通文字。</p>
          <div class="field-grid">
            <label><span>题型标记</span><el-input v-model="form.config.typeMarker" /></label>
            <label><span>题目标记</span><el-input v-model="form.config.stemMarker" /></label>
            <label><span>答案标记</span><el-input v-model="form.config.answerMarker" /></label>
            <label><span>解析标记</span><el-input v-model="form.config.explanationMarker" /></label>
            <label>
              <span>选项格式</span>
              <el-select v-model="form.config.optionStyle">
                <el-option v-for="item in optionStyles" :key="item.value" :label="item.label" :value="item.value" />
              </el-select>
            </label>
            <label>
              <span>题目分隔线（可留空）</span>
              <el-input v-model="form.config.questionSeparator" placeholder="例如 ---" />
            </label>
          </div>
          <el-switch
            v-model="form.config.compatibleDefaultMarkers"
            active-text="同时兼容系统默认的题型、题目、答案、解析和常见选项格式"
          />
          <div class="recognition-switches">
            <el-switch
              v-model="form.config.recognizeQuestionNumbers"
              active-text="同时识别 1.、2.、3. 等连续数字题号"
            />
            <el-switch
              v-model="form.config.stripRecognizedQuestionNumbers"
              :disabled="form.config.recognizeQuestionNumbers === false"
              active-text="识别后从题干中移除题号"
            />
          </div>
          <p class="recognition-hint">
            数字题号与上面的固定“题目标记”可以同时使用；1.2 等小数不会被拆成新题。
          </p>
        </section>

        <section>
          <h3>灰色提示文字</h3>
          <p>这些内容只用于空白页面提示，不会参与识别。</p>
          <div class="field-grid">
            <label><span>题目提示</span><el-input v-model="form.config.stemPrompt" /></label>
            <label><span>选项提示</span><el-input v-model="form.config.optionPrompt" /></label>
            <label><span>答案提示</span><el-input v-model="form.config.answerPrompt" /></label>
            <label><span>解析提示</span><el-input v-model="form.config.explanationPrompt" /></label>
          </div>
        </section>

        <section class="preview-section">
          <h3>实时预览</h3>
          <pre>{{ preview }}</pre>
        </section>

        <el-alert v-if="validationError" type="error" :closable="false" show-icon>
          {{ validationError }}
        </el-alert>
        <el-checkbox v-model="form.setAsDefault" :disabled="selected?.isBuiltIn && !editingExisting">
          保存后设为默认录题模板
        </el-checkbox>
      </main>
    </div>

    <template #footer>
      <div class="dialog-footer">
        <div>
          <el-button
            v-if="selected && !selected.isDefault"
            :disabled="saving"
            @click="setDefault"
          >设为默认</el-button>
          <el-button
            v-if="selected && !selected.isBuiltIn"
            type="danger"
            plain
            :loading="deleting"
            @click="removeSelected"
          >删除</el-button>
        </div>
        <div>
          <el-button :disabled="saving" @click="visible = false">关闭</el-button>
          <el-button
            type="primary"
            plain
            :loading="saving"
            :disabled="Boolean(validationError) || (selected?.isBuiltIn && !editingExisting)"
            @click="save"
          >保存模板</el-button>
          <el-button type="primary" :disabled="!editingId && !selectedId" @click="useSelected">
            使用这套模板
          </el-button>
        </div>
      </div>
    </template>
  </el-dialog>
</template>

<style scoped>
.template-dialog { height: min(650px, 72vh); display: grid; grid-template-columns: 220px minmax(0, 1fr); overflow: hidden; border: 1px solid #dbe3ee; border-radius: 9px; }
.template-list { min-height: 0; padding: 12px; display: flex; flex-direction: column; gap: 7px; overflow: auto; border-right: 1px solid #dbe3ee; background: #f8fafc; }
.template-list__header { margin-bottom: 4px; display: flex; align-items: center; justify-content: space-between; }
.template-list__item { width: 100%; padding: 10px; display: grid; gap: 5px; border: 1px solid transparent; border-radius: 7px; background: transparent; color: #334155; text-align: left; }
.template-list__item:hover { background: #fff; }
.template-list__item.active { border-color: #93c5fd; background: #eff6ff; color: #1d4ed8; }
.template-list__item span { overflow: hidden; font-size: 12px; font-weight: 650; text-overflow: ellipsis; white-space: nowrap; }
.template-list__item small { color: #64748b; font-size: 10px; }
.template-list__item em { margin-left: 6px; padding: 1px 5px; border-radius: 8px; background: #dbeafe; color: #1d4ed8; font-style: normal; }
.copy-button { margin-top: auto; }
.recognition-switches { margin-top: 9px; display: flex; flex-wrap: wrap; gap: 8px 22px; }
.recognition-hint { margin: 6px 0 0; color: #64748b; font-size: 11px; }
.template-form { min-width: 0; min-height: 0; padding: 16px 18px 24px; display: grid; align-content: start; gap: 15px; overflow: auto; background: #fff; }
.template-form section { display: grid; gap: 8px; }
.template-form h3 { margin: 0; color: #172033; font-size: 13px; }
.template-form p { margin: -3px 0 0; color: #64748b; font-size: 10px; line-height: 1.6; }
.field-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 10px 12px; }
.field-grid label { min-width: 0; display: grid; gap: 5px; color: #475569; font-size: 10px; }
.preview-section pre { min-height: 190px; margin: 0; padding: 14px; overflow: auto; border: 1px solid #dbe3ee; border-radius: 7px; background: #f8fafc; color: #94a3b8; font-family: SimSun, "宋体", serif; font-size: 12px; line-height: 1.75; white-space: pre-wrap; }
.dialog-footer { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
.dialog-footer > div { display: flex; gap: 8px; }
@media (max-width: 900px) {
  .template-dialog { grid-template-columns: 180px minmax(0, 1fr); }
  .field-grid { grid-template-columns: 1fr; }
}
</style>
