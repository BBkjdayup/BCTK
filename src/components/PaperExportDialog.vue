<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { DocumentChecked, FolderOpened, RefreshRight } from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus'
import { useRouter } from 'vue-router'
import { backend, isDesktopRuntime } from '../services/backend'
import { errorMessage } from '../services/errors'
import type {
  DocxDiagnostic,
  ExportContentMode,
  Paper,
  PaperDocxExportResult,
  WordTemplate,
} from '../types/domain'
import {
  EXPORT_CONTENT_MODE_HELP,
  EXPORT_CONTENT_MODE_LABELS,
  inspectPaperExportContent,
  requiredTemplateAnchors,
  suggestedPaperDocxName,
  templateExportAvailability,
} from '../utils/paperExport'

export interface PaperExportPreparation {
  title: string
  contentMode: ExportContentMode
  templateId: string
}

const props = defineProps<{
  modelValue: boolean
  paper: Paper
  preparePaper: (preparation: PaperExportPreparation) => Promise<Paper>
}>()

const emit = defineEmits<{
  'update:modelValue': [value: boolean]
  'paper-saved': [paper: Paper]
}>()

const loading = ref(false)
const exporting = ref(false)
const resultAction = ref<'open' | 'reveal' | ''>('')
const exportStage = ref('')
const loadError = ref('')
const formError = ref('')
const templates = ref<WordTemplate[]>([])
const defaultTemplateId = ref<string | null>(null)
const filenamePattern = ref('{title}_{date}')
const paperTitle = ref('')
const contentMode = ref<ExportContentMode>('paper_only')
const selectedTemplateId = ref('')
const outputPath = ref('')
const exportResult = ref<PaperDocxExportResult | null>(null)
const desktopAvailable = isDesktopRuntime()
const router = useRouter()

const visible = computed({
  get: () => props.modelValue,
  set: (value: boolean) => emit('update:modelValue', value),
})

const templateOptions = computed(() => templates.value.map((template) => ({
  template,
  availability: templateExportAvailability(template, contentMode.value),
})))
const usableTemplates = computed(() => templateOptions.value.filter((option) => option.availability.usable))
const selectedTemplate = computed(() => templates.value.find((item) => item.id === selectedTemplateId.value) ?? null)
const selectedAvailability = computed(() => selectedTemplate.value
  ? templateExportAvailability(selectedTemplate.value, contentMode.value)
  : null)
const contentRisk = computed(() => inspectPaperExportContent(props.paper, contentMode.value))
const requiredAnchorsText = computed(() => requiredTemplateAnchors(contentMode.value).join('、'))
const canExport = computed(() => desktopAvailable
  && !loading.value
  && !exporting.value
  && Boolean(props.paper.items.length)
  && Boolean(paperTitle.value.trim())
  && Boolean(outputPath.value)
  && Boolean(selectedAvailability.value?.usable)
  && !contentRisk.value.hasUnsupportedContent)

watch(
  () => props.modelValue,
  (open) => {
    if (open) void initialize()
  },
  { immediate: true },
)

watch(contentMode, () => {
  formError.value = ''
  exportResult.value = null
  if (!selectedAvailability.value?.usable) selectPreferredTemplate()
})

function selectPreferredTemplate() {
  const options = usableTemplates.value
  const preferred = options.find(({ template }) => template.id === props.paper.preferredTemplateId)
    ?? options.find(({ template }) => template.id === defaultTemplateId.value)
    ?? options.find(({ template }) => template.isDefault)
    ?? options[0]
  selectedTemplateId.value = preferred?.template.id ?? ''
}

async function initialize() {
  loading.value = true
  loadError.value = ''
  formError.value = ''
  exportResult.value = null
  outputPath.value = ''
  paperTitle.value = props.paper.title
  contentMode.value = props.paper.exportContentMode
  selectedTemplateId.value = ''
  try {
    const [loadedTemplates, settings] = await Promise.all([
      backend.listTemplates(),
      backend.getSettings(),
    ])
    templates.value = loadedTemplates
    defaultTemplateId.value = settings.defaultTemplateId ?? null
    filenamePattern.value = settings.exportFilenamePattern || '{title}_{date}'
    selectPreferredTemplate()
  } catch (reason) {
    loadError.value = errorMessage(reason, '读取模板和导出设置失败，请重试。')
  } finally {
    loading.value = false
  }
}

async function chooseOutputPath() {
  if (!desktopAvailable || exporting.value) return
  formError.value = ''
  try {
    const selected = await backend.pickDocxSavePath(
      suggestedPaperDocxName(paperTitle.value, filenamePattern.value),
    )
    if (selected) outputPath.value = selected
  } catch (reason) {
    formError.value = errorMessage(reason, '无法打开 Windows 保存文件窗口。')
  }
}

async function exportPaper() {
  if (!canExport.value) return
  exporting.value = true
  formError.value = ''
  exportResult.value = null
  try {
    exportStage.value = '正在保存当前试卷与题目快照…'
    const savedPaper = await props.preparePaper({
      title: paperTitle.value,
      contentMode: contentMode.value,
      templateId: selectedTemplateId.value,
    })
    emit('paper-saved', savedPaper)

    exportStage.value = '正在读取模板并生成 Word 文件…'
    const result = await backend.exportPaperDocx({
      paperId: savedPaper.id,
      expectedPaperRowVersion: savedPaper.rowVersion,
      templateId: selectedTemplateId.value,
      outputPath: outputPath.value,
    })
    exportResult.value = result
    if (!result.exported) {
      formError.value = diagnosticFailureMessage(result.diagnostics)
      return
    }
    ElMessage.success('Word 文件已真实生成并写入所选位置。')
  } catch (reason) {
    formError.value = errorMessage(reason, 'Word 导出失败，试卷已保存，但没有报告文件生成成功。')
  } finally {
    exporting.value = false
    exportStage.value = ''
  }
}

async function openExportResult(action: 'open' | 'reveal') {
  if (!exportResult.value?.exported || resultAction.value) return
  resultAction.value = action
  try {
    if (action === 'open') {
      await backend.openExportedFile(exportResult.value.outputPath)
    } else {
      await backend.revealExportedFile(exportResult.value.outputPath)
    }
  } catch (reason) {
    ElMessage.error(errorMessage(
      reason,
      action === 'open' ? '无法打开导出的 Word 文件。' : '无法在文件资源管理器中定位导出文件。',
    ))
  } finally {
    resultAction.value = ''
  }
}

function diagnosticFailureMessage(diagnostics: DocxDiagnostic[]) {
  return diagnostics.find((item) => item.severity === 'error')?.message
    ?? '后端没有确认文件生成成功，请根据下方诊断处理后重试。'
}

function diagnosticType(severity: DocxDiagnostic['severity']) {
  if (severity === 'error') return 'danger'
  if (severity === 'warning') return 'warning'
  return 'info'
}

function diagnosticLabel(severity: DocxDiagnostic['severity']) {
  if (severity === 'error') return '错误'
  if (severity === 'warning') return '提醒'
  return '信息'
}

function formatBytes(value: number | null) {
  if (value === null || !Number.isFinite(value) || value < 0) return '后端未返回文件大小'
  if (value < 1024) return `${value} B`
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`
  return `${(value / 1024 / 1024).toFixed(2)} MB`
}

function resetResult() {
  exportResult.value = null
  formError.value = ''
}

function closeDialog() {
  if (exporting.value) {
    ElMessage.info('正在导出，请等待本次操作完成。')
    return
  }
  visible.value = false
}

function goToTemplateConfiguration() {
  if (exporting.value) return
  visible.value = false
  void router.push({ name: 'templates' })
}
</script>

<template>
  <el-dialog
    v-model="visible"
    title="导出 Word 试卷"
    width="720px"
    :close-on-click-modal="!exporting"
    :close-on-press-escape="!exporting"
    :show-close="!exporting"
    destroy-on-close
    @close="closeDialog"
  >
    <div v-loading="loading" class="paper-export-dialog" aria-live="polite">
      <template v-if="exportResult?.exported">
        <section class="export-success">
          <div class="export-success__icon">✓</div>
          <div>
            <h3>Word 文件已生成</h3>
            <p>以下名称、路径和大小均来自本次真实导出结果。</p>
          </div>
        </section>
        <dl class="result-details">
          <div><dt>文件名</dt><dd>{{ exportResult.outputFilename }}</dd></div>
          <div><dt>保存位置</dt><dd class="path-value">{{ exportResult.outputPath }}</dd></div>
          <div><dt>真实大小</dt><dd>{{ formatBytes(exportResult.outputBytes) }}</dd></div>
          <div><dt>使用模板</dt><dd>{{ exportResult.templateName }}</dd></div>
          <div><dt>输出内容</dt><dd>{{ EXPORT_CONTENT_MODE_LABELS[exportResult.contentMode] }}</dd></div>
        </dl>
        <el-alert type="info" :closable="false" show-icon>
          公式已导出为 Word/WPS 可编辑公式，受管图片和普通表格也会保留；文字继续继承模板样式。请在 Word 或 WPS 中打开文件核对分页后再打印。
        </el-alert>
        <div class="result-actions">
          <el-button
            :icon="FolderOpened"
            :loading="resultAction === 'reveal'"
            :disabled="Boolean(resultAction)"
            @click="openExportResult('reveal')"
          >打开所在目录</el-button>
          <el-button
            type="primary"
            :icon="DocumentChecked"
            :loading="resultAction === 'open'"
            :disabled="Boolean(resultAction)"
            @click="openExportResult('open')"
          >打开 Word 文件</el-button>
        </div>
        <div v-if="exportResult.diagnostics.length" class="diagnostic-list">
          <h4>本次导出诊断</h4>
          <article v-for="(item, index) in exportResult.diagnostics" :key="`${item.code}-${index}`">
            <el-tag size="small" :type="diagnosticType(item.severity)">{{ diagnosticLabel(item.severity) }}</el-tag>
            <div>
              <strong>{{ item.message }}</strong>
              <span v-if="item.suggestedAction">建议：{{ item.suggestedAction }}</span>
              <code>{{ item.code }}</code>
            </div>
          </article>
        </div>
      </template>

      <template v-else>
        <el-alert v-if="!desktopAvailable" type="info" :closable="false" show-icon>
          浏览器预览只能查看这个导出流程，不能在电脑中选择路径或写入 .docx 文件。请在 Windows 桌面版中使用。
        </el-alert>
        <el-alert v-if="loadError" type="error" :closable="false" show-icon>
          {{ loadError }}
          <el-button link type="primary" :icon="RefreshRight" @click="initialize">重新读取</el-button>
        </el-alert>

        <el-form v-if="!loadError" label-position="top" class="export-form">
          <el-form-item label="试卷标题" required>
            <el-input v-model="paperTitle" maxlength="200" show-word-limit placeholder="请输入试卷标题" />
            <div class="field-help">导出前会先按这个标题保存当前试卷与题目快照。</div>
          </el-form-item>

          <el-form-item label="Word 模板" required>
            <el-select v-model="selectedTemplateId" placeholder="请选择可用模板" style="width: 100%">
              <el-option
                v-for="option in templateOptions"
                :key="option.template.id"
                :label="option.template.name"
                :value="option.template.id"
                :disabled="!option.availability.usable"
              >
                <div class="template-option">
                  <span>{{ option.template.name }}<em v-if="option.template.isDefault">默认</em></span>
                  <small v-if="!option.availability.usable">{{ option.availability.reason }}</small>
                  <small v-else>{{ option.template.fileName }}</small>
                </div>
              </el-option>
            </el-select>
            <div v-if="!loading && !templates.length" class="field-error">
              还没有模板。请先导入一个 .docx 模板。
              <el-button link type="primary" @click="goToTemplateConfiguration">前往模板管理</el-button>
            </div>
            <div v-else-if="!loading && !usableTemplates.length" class="field-error">
              当前没有适合“{{ EXPORT_CONTENT_MODE_LABELS[contentMode] }}”的模板，需要唯一的 {{ requiredAnchorsText }} 区域。
              <el-button link type="primary" @click="goToTemplateConfiguration">前往配置替换区域</el-button>
            </div>
            <div v-else-if="!loading" class="field-help">默认模板会优先选中；不可用模板保留显示，并注明不能使用的原因。</div>
          </el-form-item>

          <el-form-item label="输出内容" required>
            <el-radio-group v-model="contentMode" class="content-mode-list">
              <el-radio v-for="(label, value) in EXPORT_CONTENT_MODE_LABELS" :key="value" :value="value">
                <span>{{ label }}</span><small>{{ EXPORT_CONTENT_MODE_HELP[value] }}</small>
              </el-radio>
            </el-radio-group>
            <div class="field-help">当前模式要求模板包含：{{ requiredAnchorsText }}。</div>
          </el-form-item>

          <el-alert
            v-if="contentRisk.hasUnsupportedContent"
            type="warning"
            :closable="false"
            show-icon
          >
            <div>以下内容还不能安全转换为 Word，本次导出会被阻止：</div>
            <ul class="risk-list">
              <li v-if="contentRisk.sourceOoxmlFragmentCount">Word OOXML 公式片段 {{ contentRisk.sourceOoxmlFragmentCount }} 个</li>
              <li v-if="contentRisk.unresolvedImageNodeCount">没有受管资源 ID 的图片 {{ contentRisk.unresolvedImageNodeCount }} 个</li>
              <li v-if="contentRisk.malformedMathNodeCount">缺少公式内容的节点 {{ contentRisk.malformedMathNodeCount }} 个</li>
              <li v-if="contentRisk.complexHtmlFragmentCount">不受支持的嵌套表格、媒体或富文本片段 {{ contentRisk.complexHtmlFragmentCount }} 处</li>
            </ul>
            <div>更换输出内容模式，或回到提示所对应的题目进行修正。</div>
          </el-alert>
          <el-alert v-else type="info" :closable="false" show-icon>
            本次将导出 Word 原生可编辑公式 {{ contentRisk.mathNodeCount }} 个、受管图片 {{ contentRisk.inlineImageCount }} 个、表格 {{ contentRisk.tableCount }} 个；文字继续继承模板格式。分页以最终在 Word 或 WPS 中打开的文件为准。
          </el-alert>

          <el-form-item label="保存位置" required class="output-field">
            <div class="output-picker">
              <el-input :model-value="outputPath" readonly placeholder="点击右侧按钮选择 .docx 保存位置" />
              <el-button :icon="FolderOpened" :disabled="!desktopAvailable || exporting" @click="chooseOutputPath">
                选择位置
              </el-button>
            </div>
            <div class="field-help">软件只会在你确认的路径生成文件；取消保存窗口不会产生文件。</div>
          </el-form-item>
        </el-form>

        <el-alert v-if="formError" type="error" :closable="false" show-icon>{{ formError }}</el-alert>
        <div v-if="exporting" class="export-stage">{{ exportStage }}</div>

        <div v-if="exportResult?.diagnostics.length" class="diagnostic-list">
          <h4>后端诊断</h4>
          <article v-for="(item, index) in exportResult.diagnostics" :key="`${item.code}-${index}`">
            <el-tag size="small" :type="diagnosticType(item.severity)">{{ diagnosticLabel(item.severity) }}</el-tag>
            <div>
              <strong>{{ item.message }}</strong>
              <span v-if="item.suggestedAction">建议：{{ item.suggestedAction }}</span>
              <code>{{ item.code }}</code>
            </div>
          </article>
        </div>
      </template>
    </div>

    <template #footer>
      <template v-if="exportResult?.exported">
        <el-button :disabled="exporting" @click="resetResult">按当前设置再次导出</el-button>
        <el-button type="primary" @click="closeDialog">完成</el-button>
      </template>
      <template v-else>
        <el-button :disabled="exporting" @click="closeDialog">取消</el-button>
        <el-button
          type="primary"
          :icon="DocumentChecked"
          :loading="exporting"
          :disabled="!canExport"
          @click="exportPaper"
        >
          {{ exporting ? '正在导出' : '保存试卷并导出 Word' }}
        </el-button>
      </template>
    </template>
  </el-dialog>
</template>

<style scoped>
.paper-export-dialog { min-height: 160px; display: grid; gap: 14px; }
.export-form { display: grid; gap: 2px; }
.export-form :deep(.el-form-item) { margin-bottom: 15px; }
.field-help, .field-error { margin-top: 5px; font-size: 11px; line-height: 1.6; }
.field-help { color: #64748b; }
.field-error { color: #dc2626; }
.template-option { min-width: 0; display: flex; justify-content: space-between; gap: 12px; }
.template-option span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.template-option em { margin-left: 6px; padding: 1px 5px; border-radius: 4px; background: #dbeafe; color: #2563eb; font-size: 9px; font-style: normal; }
.template-option small { max-width: 330px; overflow: hidden; color: #94a3b8; font-size: 10px; text-overflow: ellipsis; white-space: nowrap; }
.content-mode-list { width: 100%; display: grid; grid-template-columns: 1fr 1fr; gap: 7px; }
.content-mode-list :deep(.el-radio) { height: auto; margin: 0; padding: 9px 10px; align-items: flex-start; border: 1px solid #e2e8f0; border-radius: 6px; }
.content-mode-list :deep(.el-radio__label) { display: grid; gap: 3px; white-space: normal; }
.content-mode-list small { color: #64748b; font-size: 10px; line-height: 1.45; }
.output-picker { width: 100%; display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 8px; }
.output-field :deep(.el-form-item__content) { display: block; }
.export-stage { padding: 11px; border-radius: 6px; background: #eff6ff; color: #1d4ed8; font-size: 12px; text-align: center; }
.risk-list { margin: 6px 0; padding-left: 18px; }
.export-success { padding: 6px 0 12px; display: flex; align-items: center; justify-content: center; gap: 14px; }
.export-success__icon { width: 44px; height: 44px; display: grid; place-items: center; border-radius: 50%; background: #dcfce7; color: #15803d; font-size: 24px; font-weight: 700; }
.export-success h3 { margin: 0 0 5px; font-size: 18px; }
.export-success p { margin: 0; color: #64748b; font-size: 11px; }
.result-details { margin: 0; border: 1px solid #e2e8f0; border-radius: 8px; overflow: hidden; }
.result-details > div { min-height: 42px; padding: 8px 12px; display: grid; grid-template-columns: 90px minmax(0, 1fr); align-items: center; border-bottom: 1px solid #edf1f5; font-size: 11px; }
.result-details > div:last-child { border-bottom: 0; }
.result-details dt { color: #64748b; }
.result-details dd { margin: 0; color: #1f2937; font-weight: 600; }
.path-value { overflow-wrap: anywhere; }
.result-actions { display: flex; justify-content: center; gap: 8px; }
.diagnostic-list { display: grid; gap: 7px; }
.diagnostic-list h4 { margin: 2px 0; font-size: 12px; }
.diagnostic-list article { padding: 9px; display: grid; grid-template-columns: auto minmax(0, 1fr); align-items: start; gap: 8px; border: 1px solid #e2e8f0; border-radius: 6px; }
.diagnostic-list article > div { min-width: 0; display: grid; gap: 3px; font-size: 11px; }
.diagnostic-list span { color: #64748b; line-height: 1.5; }
.diagnostic-list code { color: #94a3b8; font-size: 9px; overflow-wrap: anywhere; }
@media (max-width: 680px) { .content-mode-list { grid-template-columns: 1fr; } }
</style>
