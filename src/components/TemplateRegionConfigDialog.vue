<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { ElMessage } from 'element-plus'
import { backend } from '../services/backend'
import { errorMessage } from '../services/errors'
import { suggestTemplateRegion } from '../utils/templateRegion'
import type {
  ConfigureTemplateRegionsRequest,
  TemplateConfigurationParagraph,
  TemplateConfigurationPreview,
  TemplateRegionPlacementMode,
  TemplateStyleSamples,
  WordTemplate,
} from '../types/domain'

const props = defineProps<{
  modelValue: boolean
  template: WordTemplate | null
}>()

const emit = defineEmits<{
  'update:modelValue': [value: boolean]
  configured: [template: WordTemplate]
}>()

const loading = ref(false)
const saving = ref(false)
const loadError = ref('')
const preview = ref<TemplateConfigurationPreview | null>(null)
const questionsMode = ref<TemplateRegionPlacementMode>('replace_range')
const questionsParagraphIndex = ref<number | null>(null)
const questionsEndParagraphIndex = ref<number | null>(null)
const sectionHeadingParagraphIndex = ref<number | null>(null)
const questionStyleParagraphIndex = ref<number | null>(null)
const optionParagraphIndex = ref<number | null>(null)
const answerParagraphIndex = ref<number | null>(null)
const explanationParagraphIndex = ref<number | null>(null)
const configureTitle = ref(false)
const titleMode = ref<'replace_paragraph' | 'after_paragraph'>('replace_paragraph')
const titleParagraphIndex = ref<number | null>(null)

const visible = computed({
  get: () => props.modelValue,
  set: (value: boolean) => emit('update:modelValue', value),
})

const paragraphOptions = computed(() => preview.value?.paragraphs ?? [])
const styleParagraphOptions = computed(() => {
  if (
    questionsMode.value !== 'replace_range'
    || questionsParagraphIndex.value === null
    || questionsEndParagraphIndex.value === null
  ) return []
  return paragraphOptions.value.filter((paragraph) => (
    paragraph.index >= questionsParagraphIndex.value!
    && paragraph.index <= questionsEndParagraphIndex.value!
  ))
})

const placementConflict = computed(() => (
  configureTitle.value
  && titleParagraphIndex.value !== null
  && (
    questionsMode.value === 'replace_range'
      ? questionsParagraphIndex.value !== null
        && questionsEndParagraphIndex.value !== null
        && titleParagraphIndex.value >= questionsParagraphIndex.value
        && titleParagraphIndex.value <= questionsEndParagraphIndex.value
      : questionsMode.value === titleMode.value
        && questionsParagraphIndex.value !== null
        && questionsParagraphIndex.value === titleParagraphIndex.value
  )
))

const rangeError = computed(() => {
  if (questionsMode.value !== 'replace_range') return ''
  if (questionsParagraphIndex.value === null || questionsEndParagraphIndex.value === null) {
    return '请选择原试题区域的开始段落和结束段落。'
  }
  if (questionsParagraphIndex.value > questionsEndParagraphIndex.value) {
    return '试题开始段落不能排在结束段落之后。'
  }
  if (questionStyleParagraphIndex.value === null) {
    return '请指定一个“题号和题干”格式样本。'
  }
  const selectedSamples = [
    sectionHeadingParagraphIndex.value,
    questionStyleParagraphIndex.value,
    optionParagraphIndex.value,
    answerParagraphIndex.value,
    explanationParagraphIndex.value,
  ].filter((value): value is number => value !== null)
  if (selectedSamples.some((index) => (
    index < questionsParagraphIndex.value!
    || index > questionsEndParagraphIndex.value!
  ))) {
    return '格式样本必须位于选定的原试题区域内。'
  }
  return ''
})

const canSave = computed(() => {
  if (!preview.value || loading.value || saving.value) return false
  if (questionsMode.value !== 'document_end' && questionsParagraphIndex.value === null) return false
  if (rangeError.value) return false
  if (configureTitle.value && titleParagraphIndex.value === null) return false
  return !placementConflict.value
})

watch(
  () => props.modelValue,
  (open) => {
    if (open) void loadPreview()
  },
)

function paragraphLabel(paragraph: TemplateConfigurationParagraph) {
  const text = paragraph.text.trim() || '（空白段落）'
  const shortened = text.length > 70 ? `${text.slice(0, 70)}…` : text
  return `第 ${paragraph.index + 1} 段 · ${shortened}`
}

function resetForm() {
  questionsMode.value = 'replace_range'
  questionsParagraphIndex.value = null
  questionsEndParagraphIndex.value = null
  sectionHeadingParagraphIndex.value = null
  questionStyleParagraphIndex.value = null
  optionParagraphIndex.value = null
  answerParagraphIndex.value = null
  explanationParagraphIndex.value = null
  configureTitle.value = false
  titleMode.value = 'replace_paragraph'
  titleParagraphIndex.value = null
}

async function loadPreview() {
  const template = props.template
  if (!template) return
  loading.value = true
  loadError.value = ''
  preview.value = null
  resetForm()
  try {
    const result = await backend.getTemplateConfigurationPreview(template.id)
    preview.value = result
    const suggestion = suggestTemplateRegion(result.paragraphs)
    const firstNonEmpty = result.paragraphs.find((paragraph) => paragraph.text.trim())
      ?? result.paragraphs[0]
    questionsParagraphIndex.value = suggestion.startParagraphIndex
    questionsEndParagraphIndex.value = suggestion.endParagraphIndex
    sectionHeadingParagraphIndex.value = suggestion.styleSamples.sectionHeadingParagraphIndex ?? null
    questionStyleParagraphIndex.value = suggestion.styleSamples.questionParagraphIndex ?? null
    optionParagraphIndex.value = suggestion.styleSamples.optionParagraphIndex ?? null
    answerParagraphIndex.value = suggestion.styleSamples.answerParagraphIndex ?? null
    explanationParagraphIndex.value = suggestion.styleSamples.explanationParagraphIndex ?? null
    titleParagraphIndex.value = firstNonEmpty?.index ?? null
  } catch (reason) {
    loadError.value = errorMessage(reason, '无法读取模板段落，请关闭后重试。')
  } finally {
    loading.value = false
  }
}

async function saveConfiguration() {
  if (!canSave.value || !preview.value) return
  const questions = questionsMode.value === 'document_end'
    ? { mode: 'document_end' as const }
    : questionsMode.value === 'replace_range'
      ? {
          mode: 'replace_range' as const,
          paragraphIndex: questionsParagraphIndex.value!,
          endParagraphIndex: questionsEndParagraphIndex.value!,
        }
      : {
        mode: questionsMode.value,
        paragraphIndex: questionsParagraphIndex.value!,
      }
  const styleSamples: TemplateStyleSamples | undefined = questionsMode.value === 'replace_range'
    ? {
        sectionHeadingParagraphIndex: sectionHeadingParagraphIndex.value ?? undefined,
        questionParagraphIndex: questionStyleParagraphIndex.value ?? undefined,
        optionParagraphIndex: optionParagraphIndex.value ?? undefined,
        answerParagraphIndex: answerParagraphIndex.value ?? undefined,
        explanationParagraphIndex: explanationParagraphIndex.value ?? undefined,
      }
    : undefined
  const request: ConfigureTemplateRegionsRequest = {
    templateId: preview.value.templateId,
    baseRowVersion: preview.value.rowVersion,
    questions,
    styleSamples,
    title: configureTitle.value
      ? {
          mode: titleMode.value,
          paragraphIndex: titleParagraphIndex.value!,
        }
      : null,
  }

  saving.value = true
  try {
    const configured = await backend.configureTemplateRegions(request)
    emit('configured', configured)
    visible.value = false
    ElMessage.success('模板替换区域已配置，现在可以用于试卷导出。')
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '模板配置没有保存，请重试。'))
    await loadPreview()
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <el-dialog
    v-model="visible"
    title="从样卷制作 Word 模板"
    width="860px"
    :close-on-click-modal="!saving"
    :close-on-press-escape="!saving"
    :show-close="!saving"
    destroy-on-close
  >
    <div v-loading="loading" class="region-config-dialog">
      <el-alert type="info" :closable="false" show-icon>
        软件只会修改自己保管的模板副本，不会改动你最初选择的 Word 文件。请选择原试题的开始和结束位置；保存后旧试题会被清空，但字体、字号、缩进和段距会作为格式原型保留。
      </el-alert>

      <el-alert v-if="loadError" type="error" :closable="false" show-icon>
        {{ loadError }}
        <el-button link type="primary" @click="loadPreview">重新读取</el-button>
      </el-alert>

      <template v-else-if="preview">
        <section class="region-section">
          <div class="region-heading">
            <div>
              <h3>1. 题目区域 <el-tag size="small" type="danger">必选</el-tag></h3>
              <p>推荐选择完整的原试题范围。程序已根据段落文字给出初始建议，请你确认后再保存。</p>
            </div>
            <el-tag v-if="preview.hasQuestionsAnchor" type="success" effect="plain">当前已有题目区域</el-tag>
          </div>

          <el-radio-group v-model="questionsMode" class="placement-options">
            <el-radio value="replace_range">
              <span>清空一整段原试题（推荐）</span>
              <small>删除开始到结束之间的旧试题，并把这里变成新试题替换区域；同时保存多种格式样本。</small>
            </el-radio>
            <el-radio value="document_end">
              <span>放在文档末尾</span>
              <small>保留现有内容，在末尾插入试题。不提取原题格式，仅用于没有样卷的简单模板。</small>
            </el-radio>
            <el-radio value="replace_paragraph">
              <span>替换所选段落</span>
              <small>只删除一个段落并放入全部试题，不适合包含多种排版的完整样卷。</small>
            </el-radio>
            <el-radio value="after_paragraph">
              <span>插入到所选段落之后</span>
              <small>保留所选段落，在它的下一行开始放置全部试题。</small>
            </el-radio>
          </el-radio-group>

          <div v-if="questionsMode === 'replace_range'" class="range-selects">
            <label>
              <span>原试题开始段落</span>
              <el-select
                v-model="questionsParagraphIndex"
                filterable
                placeholder="请选择第一段试题"
              >
                <el-option
                  v-for="paragraph in paragraphOptions"
                  :key="paragraph.index"
                  :label="paragraphLabel(paragraph)"
                  :value="paragraph.index"
                />
              </el-select>
            </label>
            <label>
              <span>原试题结束段落</span>
              <el-select
                v-model="questionsEndParagraphIndex"
                filterable
                placeholder="请选择最后一段试题"
              >
                <el-option
                  v-for="paragraph in paragraphOptions"
                  :key="paragraph.index"
                  :label="paragraphLabel(paragraph)"
                  :value="paragraph.index"
                />
              </el-select>
            </label>
          </div>

          <el-select
            v-else-if="questionsMode !== 'document_end'"
            v-model="questionsParagraphIndex"
            class="paragraph-select"
            filterable
            placeholder="请选择 Word 中的一个段落"
          >
            <el-option
              v-for="paragraph in paragraphOptions"
              :key="paragraph.index"
              :label="paragraphLabel(paragraph)"
              :value="paragraph.index"
            />
          </el-select>
          <div v-if="rangeError" class="placement-error">{{ rangeError }}</div>
        </section>

        <section v-if="questionsMode === 'replace_range'" class="region-section">
          <div class="region-heading">
            <div>
              <h3>2. 选择格式样本</h3>
              <p>每个样本只提供格式，文字内容不会进入模板。没有对应内容的项目可以留空，并自动沿用题干格式。</p>
            </div>
            <el-tag type="success" effect="plain">多格式原型</el-tag>
          </div>

          <div class="style-sample-grid">
            <label>
              <span>大题标题</span>
              <small>例如“一、选择题”</small>
              <el-select v-model="sectionHeadingParagraphIndex" clearable filterable placeholder="可选">
                <el-option
                  v-for="paragraph in styleParagraphOptions"
                  :key="paragraph.index"
                  :label="paragraphLabel(paragraph)"
                  :value="paragraph.index"
                />
              </el-select>
            </label>
            <label>
              <span>题号和题干 <em>必选</em></span>
              <small>例如“1. 下列说法……”</small>
              <el-select v-model="questionStyleParagraphIndex" filterable placeholder="请选择题干样本">
                <el-option
                  v-for="paragraph in styleParagraphOptions"
                  :key="paragraph.index"
                  :label="paragraphLabel(paragraph)"
                  :value="paragraph.index"
                />
              </el-select>
            </label>
            <label>
              <span>选项</span>
              <small>例如“A. 选项内容”</small>
              <el-select v-model="optionParagraphIndex" clearable filterable placeholder="可选">
                <el-option
                  v-for="paragraph in styleParagraphOptions"
                  :key="paragraph.index"
                  :label="paragraphLabel(paragraph)"
                  :value="paragraph.index"
                />
              </el-select>
            </label>
            <label>
              <span>答案</span>
              <small>仅在样卷包含答案时选择</small>
              <el-select v-model="answerParagraphIndex" clearable filterable placeholder="可选">
                <el-option
                  v-for="paragraph in styleParagraphOptions"
                  :key="paragraph.index"
                  :label="paragraphLabel(paragraph)"
                  :value="paragraph.index"
                />
              </el-select>
            </label>
            <label>
              <span>解析</span>
              <small>仅在样卷包含解析时选择</small>
              <el-select v-model="explanationParagraphIndex" clearable filterable placeholder="可选">
                <el-option
                  v-for="paragraph in styleParagraphOptions"
                  :key="paragraph.index"
                  :label="paragraphLabel(paragraph)"
                  :value="paragraph.index"
                />
              </el-select>
            </label>
          </div>
        </section>

        <section class="region-section">
          <div class="region-heading">
            <div>
              <h3>{{ questionsMode === 'replace_range' ? '3' : '2' }}. 试卷标题 <el-tag size="small" type="info">可选</el-tag></h3>
              <p>配置后，导出时会把模板中的这一位置替换为当前试卷名称。</p>
            </div>
            <el-switch
              v-model="configureTitle"
              :active-text="preview.hasTitleAnchor ? '重新指定标题位置' : '同时配置标题位置'"
            />
          </div>

          <el-alert
            v-if="preview.hasTitleAnchor && !configureTitle"
            type="success"
            :closable="false"
            show-icon
          >
            模板已有标题区域，本次不重新指定，保存时会保留原位置。
          </el-alert>

          <template v-if="configureTitle">
            <el-radio-group v-model="titleMode" class="placement-options is-compact">
              <el-radio value="replace_paragraph">
                <span>替换所选段落</span>
                <small>适合直接选择模板中原有的试卷标题。</small>
              </el-radio>
              <el-radio value="after_paragraph">
                <span>插入到所选段落之后</span>
                <small>适合把标题放在学校、班级等信息的下一行。</small>
              </el-radio>
            </el-radio-group>
            <el-select
              v-model="titleParagraphIndex"
              class="paragraph-select"
              filterable
              placeholder="请选择 Word 中的一个段落"
            >
              <el-option
                v-for="paragraph in paragraphOptions"
                :key="paragraph.index"
                :label="paragraphLabel(paragraph)"
                :value="paragraph.index"
              />
            </el-select>
            <div v-if="placementConflict" class="placement-error">
              标题区域和题目区域不能使用完全相同的插入位置，请调整其中一个。
            </div>
          </template>
        </section>

        <div class="configuration-note">
          保存后，软件会生成新的托管副本并重新分析。试题范围之外的正文，以及页眉、页脚、页面设置、表格和图片会继续保留。新题长度变化时，最终分页仍由 Word/WPS 重新计算。
        </div>
      </template>
    </div>

    <template #footer>
      <el-button :disabled="saving" @click="visible = false">取消</el-button>
      <el-button
        type="primary"
        :loading="saving"
        :disabled="!canSave"
        @click="saveConfiguration"
      >
        {{ saving ? '正在生成并检查模板' : '生成样卷模板' }}
      </el-button>
    </template>
  </el-dialog>
</template>

<style scoped>
.region-config-dialog { min-height: 210px; display: grid; gap: 14px; }
.region-section { padding: 15px; display: grid; gap: 13px; border: 1px solid #dbe3ee; border-radius: 9px; background: #fbfdff; }
.region-heading { display: flex; align-items: flex-start; justify-content: space-between; gap: 18px; }
.region-heading h3 { margin: 0 0 5px; display: flex; align-items: center; gap: 7px; color: #172033; font-size: 14px; }
.region-heading p { margin: 0; color: #64748b; font-size: 11px; line-height: 1.6; }
.placement-options { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 8px; }
.placement-options.is-compact { grid-template-columns: repeat(2, minmax(0, 1fr)); }
.placement-options :deep(.el-radio) { height: auto; margin: 0; padding: 11px; align-items: flex-start; border: 1px solid #dbe3ee; border-radius: 7px; background: #fff; white-space: normal; }
.placement-options :deep(.el-radio.is-checked) { border-color: #60a5fa; background: #eff6ff; }
.placement-options :deep(.el-radio__label) { min-width: 0; display: grid; gap: 5px; white-space: normal; }
.placement-options span { color: #1f2937; font-size: 11px; font-weight: 600; }
.placement-options small { color: #64748b; font-size: 10px; line-height: 1.55; }
.paragraph-select { width: 100%; }
.range-selects { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 10px; }
.range-selects label,
.style-sample-grid label { min-width: 0; display: grid; gap: 5px; color: #334155; font-size: 11px; font-weight: 600; }
.style-sample-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; }
.style-sample-grid small { color: #64748b; font-size: 9px; font-weight: 400; }
.style-sample-grid em { color: #dc2626; font-size: 9px; font-style: normal; }
.placement-error { color: #dc2626; font-size: 11px; }
.configuration-note { color: #64748b; font-size: 10px; line-height: 1.6; }
@media (max-width: 700px) {
  .placement-options { grid-template-columns: 1fr; }
  .placement-options.is-compact { grid-template-columns: 1fr; }
  .range-selects,
  .style-sample-grid { grid-template-columns: 1fr; }
  .region-heading { flex-direction: column; }
}
</style>
