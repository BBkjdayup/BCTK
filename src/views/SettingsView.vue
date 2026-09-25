<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import { onBeforeRouteLeave } from 'vue-router'
import { ElMessage, ElMessageBox } from 'element-plus'
import { Check } from '@element-plus/icons-vue'
import { backend, isDesktopRuntime } from '../services/backend'
import { errorMessage } from '../services/errors'
import { useAppStore } from '../stores/app'
import type { AppSettings } from '../types/domain'
import { checkAndOfferAppUpdate } from '../utils/appUpdateFlow'
import QuestionBankExportPanel from '../components/QuestionBankExportPanel.vue'

type SettingsPage = 'general' | 'backup' | 'maintenance' | 'about'
const props = withDefaults(defineProps<{ page?: SettingsPage; embedded?: boolean }>(), { page: 'general', embedded: false })
const activePage = computed(() => props.page)
const appStore = useAppStore()
const editablePage = computed(() => activePage.value !== 'about')
const loading = ref(false)
const saving = ref(false)
const savedSnapshot = ref('')
const desktopAvailable = isDesktopRuntime()
const updateBusy = ref(false)
const updateStatus = ref('启动后后台下载更新，正常关闭软件时自动安装')
const appLogoPath = '/tk-logo.png'

const form = reactive<AppSettings>({
  defaultExportDirectory: '',
  defaultTemplateId: null,
  exportFilenamePattern: '{title}_{date}',
  optionalConfirmations: true,
  recentUnusedDays: 90,
  recentAddedDays: 30,
  recentUsedDays: 30,
  recycleRetentionDays: 30,
  recyclePolicy: 'remind_only',
  automaticBackupEnabled: true,
  automaticBackupIntervalDays: 7,
  automaticBackupRetentionCount: 5,
})

const dirty = computed(() => !!savedSnapshot.value && JSON.stringify(form) !== savedSnapshot.value)
const filenameExample = computed(() => {
  const date = new Intl.DateTimeFormat('zh-CN', {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
  }).format(Date.now()).split('/').join('-')
  return (form.exportFilenamePattern.trim() || '{title}_{date}')
    .split('{title}').join('教育学期末试卷')
    .split('{date}').join(date)
})
onMounted(() => {
  void loadSettings()
})

async function checkAppUpdateManually() {
  if (!desktopAvailable || updateBusy.value) return
  updateBusy.value = true
  updateStatus.value = '正在连接更新服务…'
  try {
    const result = await checkAndOfferAppUpdate()
    if (result.outcome === 'current') {
      updateStatus.value = `当前 ${appStore.appVersion} 已是最新版本。`
    } else if (result.outcome === 'deferred') {
      updateStatus.value = `发现 ${result.update?.version ?? '新版本'}，已选择稍后安装。`
    } else if (result.outcome === 'staged') {
      updateStatus.value = `新版本 ${result.update?.version ?? ''} 已在后台下载，关闭软件时会自动安装。`
    } else {
      updateStatus.value = '更新已下载，正在启动安装程序。'
    }
  } catch (reason) {
    updateStatus.value = errorMessage(reason, '更新检查失败，请稍后重试')
    ElMessage.error(updateStatus.value)
  } finally {
    updateBusy.value = false
  }
}

async function loadSettings() {
  loading.value = true
  try {
    Object.assign(form, await backend.getSettings())
    savedSnapshot.value = JSON.stringify(form)
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '设置加载失败'))
  } finally { loading.value = false }
}

async function canLeave() {
  if (saving.value) return false
  if (!savedSnapshot.value || !dirty.value) return true
  try {
    await ElMessageBox.confirm('有未保存的设置，离开将放弃这些修改。', '未保存的修改', {
      confirmButtonText: '放弃修改', cancelButtonText: '继续编辑', type: 'warning',
    })
    return true
  } catch { return false }
}
onBeforeRouteLeave(canLeave)
defineExpose({ canLeave })

async function saveSettings() {
  if (activePage.value === 'general' && !form.defaultExportDirectory.trim()) {
    ElMessage.warning('请填写默认保存路径')
    return
  }
  if (activePage.value === 'general' && !form.exportFilenamePattern.trim()) {
    ElMessage.warning('文件命名规则不能为空')
    return
  }

  if (activePage.value === 'backup' && (form.automaticBackupIntervalDays < 1 || form.automaticBackupIntervalDays > 365)) {
    ElMessage.warning('自动备份间隔必须在 1 到 365 天之间')
    return
  }
  if (activePage.value === 'backup' && (form.automaticBackupRetentionCount < 1 || form.automaticBackupRetentionCount > 50)) {
    ElMessage.warning('自动备份保留数量必须在 1 到 50 份之间')
    return
  }

  saving.value = true
  try {
    const latest = await backend.getSettings()
    const patch: Partial<AppSettings> = activePage.value === 'general' ? {
      defaultExportDirectory: form.defaultExportDirectory.trim(),
      exportFilenamePattern: form.exportFilenamePattern.trim(),
    } : activePage.value === 'backup' ? {
      automaticBackupEnabled: form.automaticBackupEnabled,
      automaticBackupIntervalDays: form.automaticBackupIntervalDays,
      automaticBackupRetentionCount: form.automaticBackupRetentionCount,
    } : {
      recentAddedDays: form.recentAddedDays,
      recentUsedDays: form.recentUsedDays,
      recycleRetentionDays: form.recycleRetentionDays,
    }
    const saved = await backend.saveSettings({ ...latest, ...patch })
    Object.assign(form, saved)
    savedSnapshot.value = JSON.stringify(form)
    ElMessage.success(desktopAvailable ? '设置已保存' : '演示设置已保存到浏览器缓存，不会修改电脑中的桌面版设置')
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '设置保存失败'))
  } finally {
    saving.value = false
  }
}

function restoreSavedSettings() {
  if (!savedSnapshot.value) return
  Object.assign(form, JSON.parse(savedSnapshot.value) as AppSettings)
  ElMessage.info('已恢复为上次保存的设置')
}

async function chooseExportDirectory() {
  if (!desktopAvailable) {
    ElMessage.info('浏览器演示不能选择电脑中的文件夹。请在 Windows 桌面版中设置真实导出目录。')
    return
  }
  try {
    const result = await backend.pickExportDirectory()
    if (result) form.defaultExportDirectory = result
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '无法打开导出文件夹选择窗口'))
  }
}

</script>

<template>
  <div class="settings-pane" :class="{ 'settings-pane--embedded': embedded }">
    <main class="page-main settings-page" v-loading="loading">
      <div class="settings-content">
        <template v-if="activePage === 'general'">
          <section class="surface settings-card">
            <div class="setting-row">
              <label for="default-export-directory">默认保存路径</label>
              <el-input id="default-export-directory" v-model="form.defaultExportDirectory" class="wide-control">
                <template #append><el-button :disabled="!desktopAvailable" @click="chooseExportDirectory">选择文件夹</el-button></template>
              </el-input>
            </div>
            <div class="setting-row setting-row--start">
              <label for="export-filename">文件命名规则</label>
              <div class="wide-control filename-control">
                <el-input id="export-filename" v-model="form.exportFilenamePattern" />
                <span class="field-hint">{{ '{title}' }} 标题 · {{ '{date}' }} 日期</span>
                <span class="filename-example">{{ filenameExample }}.docx</span>
              </div>
            </div>
          </section>
          <QuestionBankExportPanel />
        </template>

        <template v-else-if="activePage === 'backup'">
          <section class="surface settings-card">
            <div class="setting-row">
              <div class="setting-label">自动备份<small>启动软件时，按设定间隔创建备份</small></div>
              <el-switch v-model="form.automaticBackupEnabled" aria-label="启用自动备份" />
            </div>
            <div class="setting-row"><span class="setting-label">备份间隔</span><div class="number-control"><el-input-number v-model="form.automaticBackupIntervalDays" :min="1" :max="365" :disabled="!form.automaticBackupEnabled" controls-position="right" aria-label="自动备份间隔天数" /><span>天</span></div></div>
            <div class="setting-row"><span class="setting-label">保留最近</span><div class="number-control"><el-input-number v-model="form.automaticBackupRetentionCount" :min="1" :max="50" :disabled="!form.automaticBackupEnabled" controls-position="right" aria-label="自动备份保留数量" /><span>份</span></div></div>
            <div class="card-footnote">仅轮换自动备份，手动备份和恢复前备份会保留。</div>
          </section>
        </template>

        <template v-else-if="activePage === 'maintenance'">
          <section class="surface settings-card">
            <div class="setting-row"><span class="setting-label">最近新增</span><div class="number-control"><el-input-number v-model="form.recentAddedDays" :min="1" :max="3650" controls-position="right" aria-label="最近新增天数" /><span>天内</span></div></div>
            <div class="setting-row"><span class="setting-label">最近使用</span><div class="number-control"><el-input-number v-model="form.recentUsedDays" :min="1" :max="3650" controls-position="right" aria-label="最近使用天数" /><span>天内</span></div></div>
          </section>
          <section class="surface settings-card">
            <div class="setting-row"><span class="setting-label">回收站参考保留天数</span><div class="number-control"><el-input-number v-model="form.recycleRetentionDays" :min="1" :max="3650" controls-position="right" aria-label="回收站参考保留天数" /><span>天</span></div></div>
            <div class="card-footnote">题目不会自动永久删除。</div>
          </section>
        </template>

        <template v-else>
          <section class="surface settings-card about-card">
            <div class="about-identity"><img :src="appLogoPath" alt="" /><div><h2>TK试题题库</h2><span class="field-hint">版本 {{ appStore.appVersion }}</span></div></div>
            <dl class="directory-list">
              <div><dt>运行模式</dt><dd>{{ desktopAvailable ? 'Windows · 离线使用' : '浏览器演示' }}</dd></div>
              <div><dt>数据状态</dt><dd :class="desktopAvailable && !appStore.databaseHealthy ? 'text-danger' : ''">{{ desktopAvailable ? (appStore.databaseHealthy ? '数据库正常' : '数据库异常') : '演示数据' }}</dd></div>
            </dl>
            <div class="card-action-row"><span class="field-hint" role="status">{{ updateStatus }}</span><el-button :loading="updateBusy" :disabled="!desktopAvailable" @click="checkAppUpdateManually">检查更新</el-button></div>
          </section>
        </template>
      </div>
      <footer v-if="editablePage" class="surface settings-savebar">
        <span class="save-state">{{ dirty ? '有未保存的修改' : '设置已保存' }}</span>
        <div class="setting-inline"><el-button v-if="dirty" :disabled="saving" @click="restoreSavedSettings">撤销修改</el-button><el-button type="primary" :icon="Check" :loading="saving" :disabled="loading || !savedSnapshot || !dirty" @click="saveSettings">保存设置</el-button></div>
      </footer>
    </main>
  </div>
</template>

<style scoped>
.settings-pane { display: flex; min-width: 0; min-height: 0; flex: 1; }
.settings-pane--embedded { height: 100%; }
.settings-pane--embedded .settings-page { padding: 0; }
.settings-pane--embedded .setting-label small { max-width: 220px; }
.settings-pane--embedded .settings-savebar { border-radius: 0; box-shadow: none; }

.sidebar-menu__item .el-icon { font-size: 16px; }
.settings-page { display: flex; flex-direction: column; gap: 16px; }
.settings-content { display: grid; gap: 16px; align-content: start; }
.settings-card { overflow: hidden; }
.setting-row { min-height: 82px; margin: 0 24px; padding: 20px 0; display: flex; align-items: center; justify-content: space-between; gap: 32px; }
.setting-row + .setting-row { border-top: 1px solid #edf1f5; }
.setting-row > label, .setting-label { flex-shrink: 0; color: #334155; font-size: 13px; font-weight: 500; }
.setting-label small { display: block; margin-top: 7px; color: #94a3b8; font-size: 12px; font-weight: 400; }
.setting-row--start { align-items: flex-start; }
.setting-row--start > label { padding-top: 8px; }
.setting-value { color: #64748b; font-size: 13px; }
.wide-control { width: min(62%, 600px); }
.medium-control { width: 230px; }
.filename-control { display: flex; flex-direction: column; gap: 8px; }
.filename-example { padding: 9px 12px; background: #f8fafc; border-radius: 6px; color: #64748b; font-size: 12px; overflow-wrap: anywhere; }
.field-hint { color: #94a3b8; font-size: 12px; line-height: 1.6; }
.number-control { display: flex; align-items: center; gap: 12px; color: #64748b; font-size: 12px; }
.number-control :deep(.el-input-number) { width: 140px; }
.number-control > span { width: 28px; }
.setting-inline { display: flex; align-items: center; gap: 10px; }
.setting-inline :deep(.el-button + .el-button) { margin-left: 0; }
.card-footnote { padding: 14px 24px; border-top: 1px solid #edf1f5; color: #94a3b8; font-size: 12px; }
.settings-savebar { position: sticky; bottom: 0; margin-top: auto; padding: 14px 20px; display: flex; align-items: center; justify-content: space-between; gap: 16px; flex-shrink: 0; }
.save-state { color: #64748b; font-size: 12px; }
.path-heading { padding: 24px; display: flex; align-items: center; gap: 14px; border-bottom: 1px solid #edf1f5; }
.path-heading .el-icon { color: #64748b; font-size: 24px; }
.path-heading strong { flex: 1; min-width: 0; overflow-wrap: anywhere; color: #334155; font-size: 13px; font-weight: 500; }
.directory-list { margin: 0; padding: 4px 24px; }
.directory-list > div { min-height: 58px; display: flex; align-items: center; justify-content: space-between; gap: 20px; border-bottom: 1px solid #edf1f5; font-size: 13px; }
.directory-list > div:last-child { border-bottom: 0; }
.directory-list dt { color: #64748b; }
.directory-list dd { margin: 0; color: #334155; overflow-wrap: anywhere; }
.card-action-row { padding: 18px 24px; display: flex; align-items: center; justify-content: space-between; gap: 20px; border-top: 1px solid #edf1f5; }
.about-identity { padding: 32px 24px; display: flex; align-items: center; gap: 18px; border-bottom: 1px solid #edf1f5; }
.about-identity img { width: 56px; height: 56px; object-fit: contain; }
.about-identity h2 { margin: 0 0 8px; color: #1f2937; font-size: 20px; font-weight: 600; }
@media (max-width: 1000px) {
  .setting-row { gap: 20px; margin: 0 18px; }
  .setting-label small { max-width: 200px; }
  .path-heading, .card-action-row { flex-wrap: wrap; }
}
</style>
