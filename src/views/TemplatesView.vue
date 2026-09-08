<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { MoreFilled, Plus, Refresh, Upload } from '@element-plus/icons-vue'
import PageHeader from '../components/PageHeader.vue'
import TemplateRegionConfigDialog from '../components/TemplateRegionConfigDialog.vue'
import { backend, isDesktopRuntime } from '../services/backend'
import type { DocxDiagnostic, WordTemplate } from '../types/domain'
import { templateExportAvailability } from '../utils/paperExport'

type TemplateMode = 'list' | 'analyzing' | 'detail'

const mode = ref<TemplateMode>('list')
const loading = ref(false)
const busy = ref(false)
const analysisProgress = ref(0)
const importingFileName = ref('')
const templates = ref<WordTemplate[]>([])
const selectedTemplateId = ref<string | null>(null)
const configurationOpen = ref(false)
const desktopAvailable = isDesktopRuntime()
let progressTimer: number | undefined

function requireDesktopTemplateAccess() {
  if (desktopAvailable) return true
  ElMessage.info('浏览器只能查看模板管理界面，不能选择、复制或删除电脑中的模板文件。请在 Windows 桌面版中执行。')
  return false
}

const selectedTemplate = computed(() =>
  templates.value.find((item) => item.id === selectedTemplateId.value) ?? null,
)

function stopProgress() {
  if (progressTimer !== undefined) {
    window.clearInterval(progressTimer)
    progressTimer = undefined
  }
}

function showList() {
  stopProgress()
  mode.value = 'list'
}

function showDetail(template: WordTemplate) {
  selectedTemplateId.value = template.id
  mode.value = 'detail'
}

function baseName(path: string) {
  return path.split(/[\\/]/).pop() || 'Word 模板.docx'
}

function formatBytes(bytes: number) {
  if (!Number.isFinite(bytes) || bytes < 0) return '未知大小'
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`
}

function formatDate(timestamp: number) {
  if (!Number.isFinite(timestamp)) return '未知时间'
  return new Intl.DateTimeFormat('zh-CN', {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  }).format(new Date(timestamp))
}

function analysisLabel(template: WordTemplate) {
  if (!template.fileAvailable) return '文件不可用'
  if (template.analysisStatus === 'failed') return '分析失败'
  if (template.packageKind !== 'document') return '需要转换为 .docx'
  if (!templateCanExport(template)) return '需要配置替换区域'
  if (template.analysisStatus === 'needs_configuration') return '需要配置替换区域'
  if (template.analysisStatus === 'warning') return '分析有警告'
  return '分析完成'
}

function analysisTagType(template: WordTemplate): 'success' | 'warning' | 'danger' | 'info' {
  if (!template.fileAvailable || template.analysisStatus === 'failed') return 'danger'
  if (!templateCanExport(template)
    || template.analysisStatus === 'needs_configuration'
    || template.analysisStatus === 'warning') return 'warning'
  return 'success'
}

function templateCanExport(template: WordTemplate) {
  return templateExportAvailability(template, 'paper_only').usable
}

function openConfiguration(template: WordTemplate) {
  if (!requireDesktopTemplateAccess()) return
  if (!template.fileAvailable) {
    ElMessage.warning('软件管理的模板副本当前不可用，请重新导入原 .docx 文件。')
    return
  }
  if (template.packageKind !== 'document') {
    ElMessage.warning('替换区域配置只支持 .docx 文档，请先在 Word 中将模板另存为 .docx 后重新导入。')
    return
  }
  selectedTemplateId.value = template.id
  configurationOpen.value = true
}

function onTemplateConfigured(template: WordTemplate) {
  const index = templates.value.findIndex((item) => item.id === template.id)
  if (index >= 0) templates.value[index] = template
  selectedTemplateId.value = template.id
  mode.value = 'detail'
}

function errorMessage(error: unknown) {
  if (typeof error === 'object' && error !== null && 'message' in error) {
    const message = (error as { message?: unknown }).message
    if (typeof message === 'string' && message.trim()) return message
  }
  return typeof error === 'string' && error.trim() ? error : '操作没有完成，请稍后重试。'
}

function isDialogCancel(error: unknown) {
  return error === 'cancel' || error === 'close'
}

function diagnosticSummary(diagnostics: DocxDiagnostic[]) {
  const firstError = diagnostics.find((item) => item.severity === 'error')
  return firstError?.message ?? diagnostics[0]?.message ?? '该文件未通过安全性或格式检查，未保存到模板库。'
}

async function loadTemplates(showError = true) {
  loading.value = true
  try {
    templates.value = await backend.listTemplates()
    if (selectedTemplateId.value && !templates.value.some((item) => item.id === selectedTemplateId.value)) {
      selectedTemplateId.value = null
      mode.value = 'list'
    }
  } catch (error) {
    if (showError) ElMessage.error(errorMessage(error))
  } finally {
    loading.value = false
  }
}

async function startImport() {
  if (!requireDesktopTemplateAccess()) return
  if (busy.value) return

  let sourcePath: string | null
  try {
    sourcePath = await backend.pickDocxFile()
  } catch (error) {
    ElMessage.error(errorMessage(error))
    return
  }
  if (!sourcePath) return
  if (!sourcePath.toLocaleLowerCase('en-US').endsWith('.docx')) {
    ElMessage.warning('当前版本只支持导入 .docx 文件。')
    return
  }

  importingFileName.value = baseName(sourcePath)
  analysisProgress.value = 12
  mode.value = 'analyzing'
  busy.value = true
  stopProgress()
  progressTimer = window.setInterval(() => {
    analysisProgress.value = Math.min(88, analysisProgress.value + 4)
  }, 280)

  try {
    const result = await backend.importTemplate(sourcePath)
    analysisProgress.value = 100
    stopProgress()
    await loadTemplates(false)

    if (!result.imported || !result.template) {
      mode.value = 'list'
      ElMessage.error(diagnosticSummary(result.diagnostics))
      return
    }

    selectedTemplateId.value = result.template.id
    mode.value = 'detail'
    if (!templateCanExport(result.template)) {
      ElMessage.warning('模板已安全导入，请选择试题内容在 Word 中的放置位置。')
      openConfiguration(result.template)
    } else if (result.template.analysisStatus === 'warning') {
      ElMessage.warning('模板已导入，请查看分析警告。')
    } else {
      ElMessage.success('模板已导入并由软件统一管理。')
    }
  } catch (error) {
    mode.value = 'list'
    ElMessage.error(errorMessage(error))
  } finally {
    stopProgress()
    busy.value = false
  }
}

async function renameTemplate(template: WordTemplate) {
  if (!requireDesktopTemplateAccess()) return
  try {
    const result = await ElMessageBox.prompt('请输入新的模板名称。', '重命名模板', {
      inputValue: template.name,
      inputPattern: /\S+/,
      inputErrorMessage: '模板名称不能为空',
      confirmButtonText: '保存',
      cancelButtonText: '取消',
    })
    busy.value = true
    const updated = await backend.renameTemplate(template.id, result.value, template.rowVersion)
    const index = templates.value.findIndex((item) => item.id === updated.id)
    if (index >= 0) templates.value[index] = updated
    ElMessage.success('模板名称已更新。')
  } catch (error) {
    if (!isDialogCancel(error)) {
      ElMessage.error(errorMessage(error))
      await loadTemplates(false)
    }
  } finally {
    busy.value = false
  }
}

async function deleteTemplate(template: WordTemplate) {
  if (!requireDesktopTemplateAccess()) return
  try {
    await ElMessageBox.confirm(
      `确定删除模板“${template.name}”吗？软件管理目录中的对应副本也会删除。`,
      '删除模板',
      {
        type: 'warning',
        confirmButtonText: '删除',
        cancelButtonText: '取消',
      },
    )
    busy.value = true
    await backend.deleteTemplate(template.id, template.rowVersion)
    await loadTemplates(false)
    ElMessage.success('模板已删除。')
  } catch (error) {
    if (!isDialogCancel(error)) {
      ElMessage.error(errorMessage(error))
      await loadTemplates(false)
    }
  } finally {
    busy.value = false
  }
}

async function setDefault(template: WordTemplate) {
  if (!requireDesktopTemplateAccess()) return
  if (template.isDefault || busy.value) return
  if (!templateCanExport(template)) {
    ElMessage.warning('请先配置唯一的题目替换区域，再把模板设为默认。')
    openConfiguration(template)
    return
  }
  try {
    busy.value = true
    await backend.setDefaultTemplate(template.id, template.rowVersion)
    await loadTemplates(false)
    ElMessage.success(`“${template.name}”已设为默认模板。`)
  } catch (error) {
    ElMessage.error(errorMessage(error))
    await loadTemplates(false)
  } finally {
    busy.value = false
  }
}

onMounted(() => loadTemplates())
onBeforeUnmount(stopProgress)
</script>

<template>
  <div class="app-page">
    <aside class="page-sidebar template-sidebar">
      <div class="sidebar-title">模板管理</div>
      <nav class="sidebar-menu">
        <button class="sidebar-menu__item" :class="{ 'is-active': mode === 'list' }" @click="showList">
          <span class="nav-icon" />模板列表
        </button>
        <button class="sidebar-menu__item" :class="{ 'is-active': mode === 'analyzing' }" :disabled="busy || !desktopAvailable" @click="startImport">
          <span class="nav-icon" />导入模板
        </button>
      </nav>
      <div class="sidebar-note">
        <template v-if="desktopAvailable">只导入 <strong>.docx</strong> 文件。通过安全检查后，软件会复制一份到自己的 templates 目录；原文件不受影响。</template>
        <template v-else>浏览器只展示模板界面，不会选择、复制或删除电脑中的文件。模板文件操作请使用 Windows 桌面版。</template>
      </div>
    </aside>

    <section v-if="mode === 'list'" class="page-main templates-page" v-loading="loading">
      <div class="surface template-toolbar">
        <span>共 {{ templates.length }} 个模板</span>
        <div class="template-toolbar__actions">
          <el-button :icon="Refresh" :loading="loading" @click="loadTemplates()">刷新</el-button>
          <el-button type="primary" :icon="Upload" :loading="busy" :disabled="!desktopAvailable" @click="startImport">{{ desktopAvailable ? '导入 .docx' : '桌面版才能导入' }}</el-button>
        </div>
      </div>

      <el-alert
        v-if="!desktopAvailable"
        class="browser-runtime-note"
        type="warning"
        :closable="false"
        show-icon
        title="此列表属于浏览器界面演示；导入、重命名、设为默认和删除均已关闭，不会显示虚假的文件操作成功。"
      />

      <div v-if="templates.length" class="template-grid">
        <article
          v-for="template in templates"
          :key="template.id"
          class="surface template-card"
          :class="{ 'is-default': template.isDefault, 'is-unavailable': !template.fileAvailable }"
        >
          <div class="card-badges">
            <el-tag v-if="template.isDefault" size="small">默认</el-tag>
            <el-tag size="small" :type="analysisTagType(template)">{{ analysisLabel(template) }}</el-tag>
          </div>
          <button class="template-preview" @click="showDetail(template)">
            <div class="mini-page"><i v-for="line in 7" :key="line" /></div>
          </button>
          <h3 :title="template.name">{{ template.name }}</h3>
          <p>{{ template.fileName }} · {{ formatBytes(template.fileByteSize) }}</p>
          <p>更新于 {{ formatDate(template.updatedAt) }}</p>
          <div class="template-actions">
            <el-button link type="primary" @click="showDetail(template)">分析详情</el-button>
            <el-button
              link
              type="primary"
              :disabled="busy || !desktopAvailable || !template.fileAvailable"
              @click="openConfiguration(template)"
            >{{ templateCanExport(template) ? '调整区域' : '配置区域' }}</el-button>
            <el-button link type="primary" :disabled="busy || !desktopAvailable" @click="renameTemplate(template)">重命名</el-button>
            <el-dropdown trigger="click" :disabled="busy || !desktopAvailable">
              <el-button link type="primary" :icon="MoreFilled">更多</el-button>
              <template #dropdown>
                <el-dropdown-menu>
                  <el-dropdown-item :disabled="template.isDefault || !templateCanExport(template)" @click="setDefault(template)">设为默认模板</el-dropdown-item>
                  <el-dropdown-item divided @click="deleteTemplate(template)">删除模板</el-dropdown-item>
                </el-dropdown-menu>
              </template>
            </el-dropdown>
          </div>
        </article>

        <button class="import-card" :disabled="busy || !desktopAvailable" @click="startImport">
          <span><el-icon><Plus /></el-icon></span>
          <strong>导入 Word 模板</strong>
          <small>{{ desktopAvailable ? '仅支持 .docx 文件' : '仅 Windows 桌面版可用' }}</small>
        </button>
      </div>

      <div v-else-if="!loading" class="surface empty-state">
        <el-empty description="还没有模板">
          <el-button type="primary" :icon="Upload" :disabled="!desktopAvailable" @click="startImport">{{ desktopAvailable ? '导入第一个 .docx 模板' : '桌面版才能导入模板' }}</el-button>
        </el-empty>
      </div>
    </section>

    <section v-else-if="mode === 'analyzing'" class="page-main analyze-page">
      <div class="breadcrumb">模板管理 / 安全导入</div>
      <PageHeader title="正在检查 Word 模板" subtitle="会先检查文件结构和安全风险，再写入模板库。" />
      <div class="surface analysis-stage">
        <div class="word-file-icon"><strong>W</strong><small>DOCX</small></div>
        <h2>{{ importingFileName }}</h2>
        <p>分析过程中不会修改您选择的原文件。</p>
        <div class="analysis-progress">
          <el-progress :percentage="analysisProgress" :stroke-width="8" />
          <span>{{ analysisProgress < 45 ? '正在读取并限制文件大小…' : analysisProgress < 80 ? '正在检查 Word 包结构与安全性…' : '正在识别可替换锚点…' }}</span>
        </div>
      </div>
    </section>

    <section v-else class="page-main detail-page">
      <div class="breadcrumb">模板管理 / 分析详情</div>
      <PageHeader :title="selectedTemplate?.name ?? '模板详情'" subtitle="这里展示软件实际保存的分析结果，不会执行最终 Word 导出。">
        <el-button @click="showList">返回列表</el-button>
        <el-button
          v-if="selectedTemplate"
          :type="templateCanExport(selectedTemplate) ? 'default' : 'primary'"
          :disabled="busy || !desktopAvailable || !selectedTemplate.fileAvailable"
          @click="openConfiguration(selectedTemplate)"
        >{{ templateCanExport(selectedTemplate) ? '调整替换区域' : '配置替换区域' }}</el-button>
        <el-button
          v-if="selectedTemplate && !selectedTemplate.isDefault"
          type="primary"
          :disabled="busy || !desktopAvailable || !templateCanExport(selectedTemplate)"
          @click="setDefault(selectedTemplate)"
        >设为默认</el-button>
      </PageHeader>

      <template v-if="selectedTemplate">
        <el-alert
          v-if="!selectedTemplate.fileAvailable"
          title="软件管理目录中的模板文件缺失或大小不一致，请重新导入。"
          type="error"
          :closable="false"
          show-icon
        />
        <el-alert
          v-else-if="!templateCanExport(selectedTemplate)"
          title="模板已安全保存，但还不能用于导出。点击“配置替换区域”，指定所有试题内容在 Word 中的放置位置。"
          type="warning"
          :closable="false"
          show-icon
        />

        <div class="detail-grid">
          <section class="surface detail-card">
            <h3>文件与版本</h3>
            <dl>
              <div><dt>原始文件名</dt><dd>{{ selectedTemplate.fileName }}</dd></div>
              <div><dt>文件大小</dt><dd>{{ formatBytes(selectedTemplate.fileByteSize) }}</dd></div>
              <div><dt>Word 包类型</dt><dd>{{ selectedTemplate.packageKind }}</dd></div>
              <div><dt>分析状态</dt><dd><el-tag size="small" :type="analysisTagType(selectedTemplate)">{{ analysisLabel(selectedTemplate) }}</el-tag></dd></div>
              <div><dt>分析器版本</dt><dd>{{ selectedTemplate.parserVersion }}</dd></div>
              <div><dt>数据版本</dt><dd>{{ selectedTemplate.rowVersion }}</dd></div>
              <div><dt>SHA-256</dt><dd class="hash-value">{{ selectedTemplate.fileSha256Hex }}</dd></div>
            </dl>
          </section>

          <section class="surface detail-card">
            <h3>可替换锚点（{{ selectedTemplate.anchors.length }}）</h3>
            <div v-if="selectedTemplate.anchors.length" class="anchor-list">
              <article v-for="anchor in selectedTemplate.anchors" :key="`${anchor.name}-${anchor.containerSpan.start}`">
                <strong>{{ anchor.name }}</strong>
                <span>{{ anchor.kind === 'content_control' ? 'Word 内容控件' : '整段标记' }}</span>
                <small>{{ anchor.partName }} · 字节 {{ anchor.replacementSpan.start }}–{{ anchor.replacementSpan.end }}</small>
              </article>
            </div>
            <el-empty v-else :image-size="70" description="未识别到锚点" />
          </section>

          <section class="surface detail-card diagnostics-card">
            <h3>分析诊断（{{ selectedTemplate.diagnostics.length }}）</h3>
            <div v-if="selectedTemplate.diagnostics.length" class="diagnostic-list">
              <article v-for="(diagnostic, index) in selectedTemplate.diagnostics" :key="`${diagnostic.code}-${index}`" :class="`is-${diagnostic.severity}`">
                <div><el-tag size="small" :type="diagnostic.severity === 'error' ? 'danger' : diagnostic.severity === 'warning' ? 'warning' : 'info'">{{ diagnostic.severity }}</el-tag><strong>{{ diagnostic.code }}</strong></div>
                <p>{{ diagnostic.message }}</p>
                <small v-if="diagnostic.suggestedAction">建议：{{ diagnostic.suggestedAction }}</small>
              </article>
            </div>
            <el-empty v-else :image-size="70" description="没有发现问题" />
          </section>
        </div>
      </template>
    </section>

    <TemplateRegionConfigDialog
      v-model="configurationOpen"
      :template="selectedTemplate"
      @configured="onTemplateConfigured"
    />
  </div>
</template>

<style scoped>
.template-sidebar { padding-top: 2px; }
.nav-icon { width: 18px; height: 18px; border: 1.5px solid #94a3b8; border-radius: 5px; }
.sidebar-menu__item.is-active .nav-icon { border-color: #3b82f6; }
.sidebar-note { margin: 22px 14px; padding: 13px 12px; border: 1px solid var(--border); border-radius: 8px; background: #fff; color: #526176; font-size: 11px; line-height: 1.75; }
.breadcrumb { margin-bottom: 8px; color: #94a3b8; font-size: 11px; }
.template-toolbar { min-height: 58px; margin-bottom: 14px; padding: 9px 12px; display: flex; align-items: center; justify-content: space-between; gap: 16px; }
.template-toolbar > span { color: #64748b; font-size: 11px; }
.template-toolbar__actions { display: flex; align-items: center; gap: 8px; }
.template-toolbar__actions :deep(.el-button + .el-button) { margin-left: 0; }
.browser-runtime-note { margin-bottom: 14px; }
.templates-page, .analyze-page, .detail-page { display: flex; flex-direction: column; }
.template-grid { display: grid; grid-template-columns: repeat(4, minmax(220px, 1fr)); gap: 14px; }
.template-card { position: relative; min-width: 0; padding: 12px; overflow: hidden; }
.template-card.is-default { border-color: #2563eb; box-shadow: 0 0 0 1px #2563eb, var(--shadow-sm); }
.template-card.is-unavailable { border-color: #fca5a5; }
.card-badges { position: absolute; z-index: 2; top: 10px; right: 10px; display: flex; gap: 5px; }
.template-preview { width: 100%; height: 205px; display: grid; place-items: center; border: 1px solid #d7dee9; border-radius: 7px; background: #f8fafc; }
.mini-page { width: 118px; height: 166px; padding: 30px 18px; display: flex; flex-direction: column; gap: 7px; border: 1px solid #d8dee8; background: #fff; box-shadow: 0 5px 12px rgb(15 23 42 / 10%); }
.mini-page i { height: 4px; background: #d6dce5; }
.mini-page i:first-child { width: 72%; margin: 0 auto 5px; background: #64748b; }
.template-card h3 { margin: 12px 0 4px; overflow: hidden; color: #111827; font-size: 13px; text-overflow: ellipsis; white-space: nowrap; }
.template-card p { margin: 3px 0 0; overflow: hidden; color: #64748b; font-size: 10px; text-overflow: ellipsis; white-space: nowrap; }
.template-actions { margin-top: 8px; display: flex; align-items: center; gap: 2px; }
.import-card { min-height: 337px; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 7px; border: 1px dashed #cbd5e1; border-radius: 10px; background: #fff; color: #475569; }
.import-card:hover { border-color: #3b82f6; color: #2563eb; }
.import-card span { width: 44px; height: 44px; margin-bottom: 4px; display: grid; place-items: center; border-radius: 50%; background: #eff6ff; color: #2563eb; font-size: 20px; }
.import-card strong { font-size: 13px; }
.import-card small { color: #64748b; }
.empty-state { min-height: 480px; display: grid; place-items: center; }
.analysis-stage { min-height: 500px; flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center; }
.word-file-icon { width: 72px; height: 84px; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 5px; border: 1px solid #bfdbfe; border-radius: 10px; background: #eff6ff; color: #2563eb; }
.word-file-icon strong { font-size: 18px; }
.word-file-icon small { font-size: 10px; }
.analysis-stage h2 { margin: 14px 0 4px; font-size: 17px; }
.analysis-stage > p { margin: 0; color: #64748b; font-size: 11px; }
.analysis-progress { width: min(520px, 80%); margin-top: 25px; }
.analysis-progress > span { display: block; color: #334155; font-size: 11px; }
.detail-page > .el-alert { margin-bottom: 14px; }
.detail-grid { display: grid; grid-template-columns: minmax(330px, .8fr) minmax(390px, 1.2fr); gap: 14px; }
.detail-card { padding: 18px; }
.detail-card h3 { margin: 0 0 14px; font-size: 14px; }
.detail-card dl { margin: 0; }
.detail-card dl > div { min-height: 40px; display: grid; grid-template-columns: 110px minmax(0, 1fr); align-items: center; border-bottom: 1px solid #edf1f5; font-size: 11px; }
.detail-card dt { color: #64748b; }
.detail-card dd { min-width: 0; margin: 0; color: #1f2937; font-weight: 600; }
.hash-value { overflow: hidden; font-family: Consolas, monospace; font-size: 10px; text-overflow: ellipsis; white-space: nowrap; }
.anchor-list, .diagnostic-list { display: flex; flex-direction: column; gap: 9px; }
.anchor-list article, .diagnostic-list article { padding: 11px 12px; border: 1px solid #e2e8f0; border-radius: 7px; background: #f8fafc; }
.anchor-list article { display: grid; grid-template-columns: minmax(120px, 1fr) auto; gap: 5px 12px; }
.anchor-list strong { color: #1d4ed8; font-size: 12px; }
.anchor-list span { color: #475569; font-size: 11px; }
.anchor-list small { grid-column: 1 / -1; color: #64748b; font-family: Consolas, monospace; font-size: 10px; }
.diagnostics-card { grid-column: 1 / -1; }
.diagnostic-list article.is-error { border-color: #fecaca; background: #fff7f7; }
.diagnostic-list article.is-warning { border-color: #fde68a; background: #fffdf3; }
.diagnostic-list article > div { display: flex; align-items: center; gap: 8px; }
.diagnostic-list article strong { font-family: Consolas, monospace; font-size: 11px; }
.diagnostic-list p { margin: 8px 0 3px; color: #334155; font-size: 11px; }
.diagnostic-list small { color: #64748b; font-size: 10px; }
@media (max-width: 1280px) { .template-grid { grid-template-columns: repeat(3, minmax(210px, 1fr)); } }
@media (max-width: 980px) { .template-grid { grid-template-columns: repeat(2, minmax(210px, 1fr)); } .detail-grid { grid-template-columns: 1fr; } .diagnostics-card { grid-column: auto; } }
</style>
