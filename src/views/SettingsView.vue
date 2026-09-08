<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import { useRouter } from 'vue-router'
import { ElMessage, ElMessageBox } from 'element-plus'
import { Check, CopyDocument, FolderOpened } from '@element-plus/icons-vue'
import { backend, isDesktopRuntime } from '../services/backend'
import { errorMessage } from '../services/errors'
import { useAppStore } from '../stores/app'
import type { AppSettings, CloudAccountStatus, CloudSyncConflict, CloudSyncPreflight, WordTemplate } from '../types/domain'
import { requiresFirstCloudSyncConfirmation } from '../utils/cloudSyncPolicy'
import { cloudSyncPreflightCopy } from '../utils/cloudSyncPreflight'
import { checkAndOfferAppUpdate } from '../utils/appUpdateFlow'
import { templateExportAvailability } from '../utils/paperExport'

type SettingsPage = 'general' | 'license' | 'data' | 'about'
type SettingsSection = 'basic' | 'export' | 'backup' | 'confirmations' | 'maintenance'

const appStore = useAppStore()
const router = useRouter()
const activePage = ref<SettingsPage>('general')
const activeSection = ref<SettingsSection>('basic')
const loading = ref(false)
const saving = ref(false)
const savedSnapshot = ref('')
const templates = ref<WordTemplate[]>([])
const templateListError = ref('')
const desktopAvailable = isDesktopRuntime()
const licenseBusy = ref(false)
const cloudBusy = ref(false)
const cloudSyncBusy = ref(false)
const cloudOperationBusy = computed(() => cloudBusy.value || cloudSyncBusy.value)
const cloudStatus = ref<CloudAccountStatus | null>(null)
const cloudConflicts = ref<CloudSyncConflict[]>([])
const cloudPreflight = ref<CloudSyncPreflight | null>(null)
const updateBusy = ref(false)
const updateStatus = ref('启动软件时会自动检查新版本，也可以在这里立即检查。')
const cloudApiBaseUrl = ref('')
const cloudAuthMode = ref<'login' | 'register'>('login')
const cloudAuth = reactive({ account: '', username: '', email: '', password: '' })
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

const dirty = computed(() => JSON.stringify(form) !== savedSnapshot.value)
const savedTemplateMissing = computed(() => Boolean(
  form.defaultTemplateId
  && !templateListError.value
  && !templates.value.some((template) => template.id === form.defaultTemplateId),
))
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
const desktopLicenseLabel = computed(() => {
  const state = appStore.license.desktop.state
  const trial = appStore.license.desktop.plan === 'trial'
  if (state === 'active') return trial ? '桌面专业版（免费试用）' : '桌面专业版'
  if (state === 'grace') return trial ? '免费试用（宽限期）' : '桌面专业版（宽限期）'
  if (state === 'invalid') return '许可证无效'
  return '基础桌面模式'
})
const desktopLicenseTagType = computed(() => {
  if (appStore.license.desktop.state === 'active') return 'success'
  if (appStore.license.desktop.state === 'grace') return 'warning'
  if (appStore.license.desktop.state === 'invalid') return 'danger'
  return 'info'
})
const cloudEntitlementLabel = computed(() => {
  if (!cloudStatus.value?.loggedIn) return '未登录'
  if (cloudStatus.value.canSync) return '云同步已开通'
  return '云同步未开通'
})
const cloudEntitlementTagType = computed(() => cloudStatus.value?.canSync ? 'success' : 'info')

onMounted(() => {
  void loadSettings()
  void loadCloudStatus()
})

async function loadCloudStatus() {
  try {
    cloudStatus.value = await backend.getCloudAccountStatus()
    cloudApiBaseUrl.value = cloudStatus.value.apiBaseUrl
    cloudConflicts.value = cloudStatus.value.loggedIn && cloudStatus.value.conflictCount > 0
      ? await backend.listCloudSyncConflicts()
      : []
    cloudPreflight.value = cloudStatus.value.loggedIn
      && cloudStatus.value.canSync
      && !cloudStatus.value.databaseBound
      ? await backend.getCloudSyncPreflight()
      : null
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '云账号状态读取失败'))
  }
}

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

async function saveCloudApiUrl() {
  if (!desktopAvailable || cloudOperationBusy.value) return
  const nextUrl = cloudApiBaseUrl.value.trim()
  if (cloudStatus.value?.loggedIn && nextUrl !== cloudStatus.value.apiBaseUrl) {
    try {
      await ElMessageBox.confirm(
        '更换云服务地址会退出当前云账号，但不会删除本地题库。是否继续？',
        '更换云服务地址',
        { type: 'warning', confirmButtonText: '保存并退出账号', cancelButtonText: '取消' },
      )
    } catch {
      cloudApiBaseUrl.value = cloudStatus.value.apiBaseUrl
      return
    }
  }
  cloudBusy.value = true
  try {
    cloudStatus.value = await backend.setCloudApiUrl(nextUrl)
    cloudApiBaseUrl.value = cloudStatus.value.apiBaseUrl
    ElMessage.success(nextUrl ? '云服务地址已保存' : '云服务地址已清除')
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '云服务地址保存失败'))
  } finally {
    cloudBusy.value = false
  }
}

async function submitCloudAuth() {
  if (!desktopAvailable || cloudOperationBusy.value) return
  if (!cloudStatus.value?.configured) {
    ElMessage.warning('请先填写并保存云服务 HTTPS 地址')
    return
  }
  if (!cloudAuth.password) {
    ElMessage.warning('请输入密码')
    return
  }
  cloudBusy.value = true
  try {
    if (cloudAuthMode.value === 'register') {
      if (!cloudAuth.username.trim()) {
        ElMessage.warning('请输入用户名')
        return
      }
      cloudStatus.value = await backend.cloudRegister({
        username: cloudAuth.username.trim(),
        email: cloudAuth.email.trim() || null,
        password: cloudAuth.password,
      })
      ElMessage.success('云账号已注册并登录；云同步需开通订阅后使用')
    } else {
      if (!cloudAuth.account.trim()) {
        ElMessage.warning('请输入用户名或邮箱')
        return
      }
      cloudStatus.value = await backend.cloudLogin({
        account: cloudAuth.account.trim(),
        password: cloudAuth.password,
      })
      ElMessage.success('云账号登录成功')
    }
    cloudAuth.password = ''
    await loadCloudStatus()
  } catch (reason) {
    ElMessage.error(errorMessage(reason, cloudAuthMode.value === 'register' ? '云账号注册失败' : '云账号登录失败'))
  } finally {
    cloudBusy.value = false
  }
}

async function logoutCloudAccount() {
  if (!desktopAvailable || cloudOperationBusy.value) return
  cloudBusy.value = true
  try {
    cloudStatus.value = await backend.cloudLogout()
    cloudAuth.password = ''
    ElMessage.success('已退出云账号；本地题库未删除')
  } catch (reason) {
    await loadCloudStatus()
    ElMessage.warning(errorMessage(reason, '本机登录信息已清除，但服务器会话撤销失败'))
  } finally {
    cloudBusy.value = false
  }
}

async function syncCloudNow() {
  if (!desktopAvailable || cloudOperationBusy.value || !cloudStatus.value?.canSync) return
  cloudSyncBusy.value = true
  try {
    if (requiresFirstCloudSyncConfirmation(cloudStatus.value)) {
      const username = cloudStatus.value.user?.username || '当前账号'
      const preflight = await backend.getCloudSyncPreflight()
      cloudPreflight.value = preflight
      const copy = cloudSyncPreflightCopy(preflight)
      try {
        await ElMessageBox.confirm(
          `${copy.detail}\n\n本地题库将绑定到云账号“${username}”，以后不能改用其他账号同步。是否继续？`,
          copy.title,
          { type: 'warning', confirmButtonText: copy.confirmText, cancelButtonText: '取消' },
        )
      } catch {
        return
      }
    }
    const result = await backend.cloudSyncNow()
    await appStore.initialize()
    await loadCloudStatus()
    if (result.conflictCount > 0 || result.skippedCount > 0) {
      ElMessage.warning(result.message)
    } else {
      ElMessage.success(result.message)
    }
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '云同步失败；本地题库未被清空'))
  } finally {
    cloudSyncBusy.value = false
  }
}

async function resolveCloudConflict(conflict: CloudSyncConflict, resolution: 'keep_local' | 'use_cloud') {
  if (cloudOperationBusy.value) return
  const useCloud = resolution === 'use_cloud'
  try {
    await ElMessageBox.confirm(
      useCloud
        ? '采用云端版本会覆盖本机这一项当前内容；冲突记录中的双方内容在确认前都不会丢失。是否继续？'
        : '保留本机版本后，下一次同步会把本机内容作为新版本上传。是否继续？',
      useCloud ? '采用云端版本' : '保留本机版本',
      { type: 'warning', confirmButtonText: useCloud ? '采用云端' : '保留本机', cancelButtonText: '取消' },
    )
  } catch {
    return
  }
  cloudBusy.value = true
  try {
    cloudConflicts.value = await backend.resolveCloudSyncConflict({ id: conflict.id, resolution })
    await loadCloudStatus()
    ElMessage.success(useCloud ? '已采用云端版本' : '已保留本机版本；下次同步时上传')
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '同步冲突处理失败'))
  } finally {
    cloudBusy.value = false
  }
}

function cloudEntityKindLabel(kind: string) {
  return ({
    question: '题目', resource: '图片', tag: '标签', chapter: '章节',
    subject: '学科', question_type: '题型',
  } as Record<string, string>)[kind] || kind
}

function formatCloudTime(value: string | number | null | undefined) {
  if (value == null) return '—'
  const timestamp = typeof value === 'number' ? value : Date.parse(value)
  if (!Number.isFinite(timestamp)) return '—'
  return new Intl.DateTimeFormat('zh-CN', {
    year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit',
  }).format(timestamp)
}

async function loadSettings() {
  loading.value = true
  templateListError.value = ''
  try {
    const [settings, templateResult] = await Promise.all([
      backend.getSettings(),
      backend.listTemplates()
        .then((items) => ({ items, error: '' }))
        .catch((reason: unknown) => ({
          items: [] as WordTemplate[],
          error: errorMessage(reason, '模板列表加载失败'),
        })),
    ])
    Object.assign(form, settings)
    templates.value = templateResult.items
    templateListError.value = templateResult.error
    savedSnapshot.value = JSON.stringify(form)
    if (templateResult.error) ElMessage.warning(templateResult.error)
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '设置加载失败'))
  } finally {
    loading.value = false
  }
}

function templateOptionLabel(template: WordTemplate) {
  const availability = templateExportAvailability(template, 'paper_only')
  return availability.usable
    ? template.name
    : `${template.name}（${availability.reason.replace(/[。.]$/, '')}）`
}

function templateCanExport(template: WordTemplate) {
  return templateExportAvailability(template, 'paper_only').usable
}

async function saveSettings() {
  if (!form.defaultExportDirectory.trim()) {
    ElMessage.warning('请填写默认保存路径')
    activePage.value = 'general'
    activeSection.value = 'basic'
    return
  }
  if (!form.exportFilenamePattern.trim()) {
    ElMessage.warning('文件命名规则不能为空')
    activePage.value = 'general'
    activeSection.value = 'export'
    return
  }

  if (form.automaticBackupIntervalDays < 1 || form.automaticBackupIntervalDays > 365) {
    ElMessage.warning('自动备份间隔必须在 1 到 365 天之间')
    activePage.value = 'general'
    activeSection.value = 'backup'
    return
  }
  if (form.automaticBackupRetentionCount < 1 || form.automaticBackupRetentionCount > 50) {
    ElMessage.warning('自动备份保留数量必须在 1 到 50 份之间')
    activePage.value = 'general'
    activeSection.value = 'backup'
    return
  }

  saving.value = true
  try {
    const saved = await backend.saveSettings({
      ...form,
      defaultExportDirectory: form.defaultExportDirectory.trim(),
      exportFilenamePattern: form.exportFilenamePattern.trim(),
      defaultTemplateId: form.defaultTemplateId || null,
    })
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

async function copyDataRoot() {
  if (!desktopAvailable) {
    ElMessage.info('浏览器演示无法读取电脑中的真实数据目录。')
    return
  }
  if (!appStore.dataRoot) return
  try {
    await navigator.clipboard.writeText(appStore.dataRoot)
    ElMessage.success('数据目录已复制')
  } catch {
    ElMessage.info(appStore.dataRoot)
  }
}

function formatLicenseDate(timestamp: number | null) {
  if (timestamp == null) return '—'
  return new Intl.DateTimeFormat('zh-CN', {
    year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit',
  }).format(timestamp)
}

async function copyDeviceId() {
  try {
    await navigator.clipboard.writeText(appStore.license.deviceId)
    ElMessage.success('设备编号已复制')
  } catch {
    ElMessage.info(appStore.license.deviceId)
  }
}

async function exportLicenseRequest() {
  if (!desktopAvailable || licenseBusy.value) return
  licenseBusy.value = true
  try {
    const suggestedName = `TK试题题库-授权申请-${appStore.license.deviceId.slice(0, 8)}.tkreq`
    const outputPath = await backend.pickLicenseRequestSavePath(suggestedName)
    if (!outputPath) return
    const result = await backend.exportLicenseRequest(outputPath)
    ElMessage.success(`授权申请已生成：${result.filename}`)
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '授权申请生成失败'))
  } finally {
    licenseBusy.value = false
  }
}

async function importDesktopLicense() {
  if (!desktopAvailable || licenseBusy.value) return
  licenseBusy.value = true
  try {
    const inputPath = await backend.pickDesktopLicenseFile()
    if (!inputPath) return
    const overview = await backend.importDesktopLicense(inputPath)
    appStore.applyLicense(overview)
    ElMessage.success('桌面专业版许可证已验证并导入')
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '许可证导入失败'))
  } finally {
    licenseBusy.value = false
  }
}

async function removeDesktopLicense() {
  if (!desktopAvailable || licenseBusy.value || !appStore.license.desktop.licenseId) return
  try {
    await ElMessageBox.confirm(
      '移除后软件立即回到基础桌面模式；本地题库和已有试卷不会删除。是否继续？',
      '移除桌面许可证',
      { type: 'warning', confirmButtonText: '移除许可证', cancelButtonText: '取消' },
    )
  } catch {
    return
  }
  licenseBusy.value = true
  try {
    appStore.applyLicense(await backend.removeDesktopLicense())
    ElMessage.success('许可证已移除，当前为基础桌面模式')
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '许可证移除失败'))
  } finally {
    licenseBusy.value = false
  }
}

function openDataMigration() {
  if (!desktopAvailable) {
    ElMessage.info('数据目录迁移只能在 Windows 桌面版中执行。')
    return
  }
  void router.push({ path: '/data', query: { section: 'directory' } })
}

function scrollToSection(section: SettingsSection) {
  activeSection.value = section
  document.getElementById(`settings-${section}`)?.scrollIntoView({ behavior: 'smooth', block: 'start' })
}
</script>

<template>
  <div class="app-page">
    <aside class="page-sidebar">
      <div class="sidebar-title">系统设置</div>
      <nav class="sidebar-menu">
        <button
          type="button"
          class="sidebar-menu__item"
          :class="{ 'is-active': activePage === 'general' }"
          :aria-pressed="activePage === 'general'"
          :aria-current="activePage === 'general' ? 'page' : undefined"
          @click="activePage = 'general'"
        ><span class="nav-square" aria-hidden="true" />常规与导出</button>
        <button
          type="button"
          class="sidebar-menu__item"
          :class="{ 'is-active': activePage === 'license' }"
          :aria-pressed="activePage === 'license'"
          :aria-current="activePage === 'license' ? 'page' : undefined"
          @click="activePage = 'license'"
        ><span class="nav-square" aria-hidden="true" />授权与云服务</button>
        <button
          type="button"
          class="sidebar-menu__item"
          :class="{ 'is-active': activePage === 'data' }"
          :aria-pressed="activePage === 'data'"
          :aria-current="activePage === 'data' ? 'page' : undefined"
          @click="activePage = 'data'"
        ><span class="nav-square" aria-hidden="true" />数据位置</button>
        <button
          type="button"
          class="sidebar-menu__item"
          :class="{ 'is-active': activePage === 'about' }"
          :aria-pressed="activePage === 'about'"
          :aria-current="activePage === 'about' ? 'page' : undefined"
          @click="activePage = 'about'"
        ><span class="nav-square" aria-hidden="true" />关于软件</button>
      </nav>
    </aside>

    <section class="page-main settings-page">
      <el-alert
        v-if="!desktopAvailable"
        class="browser-runtime-note"
        type="warning"
        :closable="false"
        show-icon
        title="浏览器界面演示不会读取真实数据目录，也不能选择导出文件夹或迁移数据；可编辑的设置只保存在浏览器演示缓存中。"
      />

      <div v-if="activePage === 'general'" v-loading="loading" class="settings-layout">
        <nav class="surface settings-subnav">
          <button type="button" :class="{ 'is-active': activeSection === 'basic' }" :aria-pressed="activeSection === 'basic'" :aria-current="activeSection === 'basic' ? 'location' : undefined" @click="scrollToSection('basic')">常规设置</button>
          <button type="button" :class="{ 'is-active': activeSection === 'export' }" :aria-pressed="activeSection === 'export'" :aria-current="activeSection === 'export' ? 'location' : undefined" @click="scrollToSection('export')">导出设置</button>
          <button type="button" :class="{ 'is-active': activeSection === 'backup' }" :aria-pressed="activeSection === 'backup'" :aria-current="activeSection === 'backup' ? 'location' : undefined" @click="scrollToSection('backup')">自动备份</button>
          <button type="button" :class="{ 'is-active': activeSection === 'confirmations' }" :aria-pressed="activeSection === 'confirmations'" :aria-current="activeSection === 'confirmations' ? 'location' : undefined" @click="scrollToSection('confirmations')">操作确认</button>
          <button type="button" :class="{ 'is-active': activeSection === 'maintenance' }" :aria-pressed="activeSection === 'maintenance'" :aria-current="activeSection === 'maintenance' ? 'location' : undefined" @click="scrollToSection('maintenance')">统计与回收站</button>
        </nav>

        <div class="surface settings-form-card">
          <section id="settings-basic" class="settings-section">
            <div class="settings-section-heading">
              <h2>常规设置</h2>
              <div class="settings-form-actions">
                <el-button v-if="dirty" @click="restoreSavedSettings">撤销修改</el-button>
                <el-button
                  type="primary"
                  :icon="Check"
                  :loading="saving"
                  :disabled="loading || !dirty"
                  @click="saveSettings"
                >保存设置</el-button>
              </div>
            </div>
            <div class="setting-row">
              <div>
                <strong>默认保存路径</strong>
                <span>{{ desktopAvailable ? '试卷 Word 保存窗口会默认从这里打开；后续题库导出也会沿用' : '浏览器中仅作为界面演示值，不对应电脑文件夹' }}</span>
              </div>
              <el-input v-model="form.defaultExportDirectory" class="wide-control" aria-label="默认保存路径">
                <template #append><el-button :disabled="!desktopAvailable" @click="chooseExportDirectory">{{ desktopAvailable ? '浏览…' : '桌面版选择' }}</el-button></template>
              </el-input>
            </div>
            <div class="setting-row">
              <div>
                <strong>默认试卷模板</strong>
                <span>用于试卷 Word 导出；支持受管图片、原生表格和可编辑公式，不支持的富内容会在导出前明确提示</span>
              </div>
              <el-select
                v-model="form.defaultTemplateId"
                clearable
                class="wide-control"
                :placeholder="templateListError ? '模板列表读取失败' : '未设置默认模板'"
                aria-label="默认试卷模板"
              >
                <el-option
                  v-for="template in templates"
                  :key="template.id"
                  :label="templateOptionLabel(template)"
                  :value="template.id"
                  :disabled="!templateCanExport(template)"
                />
                <el-option
                  v-if="savedTemplateMissing"
                  :label="`原默认模板已不存在 · ${form.defaultTemplateId?.slice(0, 8)}`"
                  :value="form.defaultTemplateId!"
                  disabled
                />
              </el-select>
            </div>
          </section>

          <section id="settings-export" class="settings-section">
            <h2>导出设置</h2>
            <div class="setting-row setting-row--start">
              <div>
                <strong>文件命名规则</strong>
                <span>可使用试卷标题和当前日期变量</span>
              </div>
              <div class="wide-control filename-control">
                <el-input v-model="form.exportFilenamePattern" aria-label="文件命名规则" />
                <div class="filename-help">
                  <el-tag size="small" effect="plain">{{ '{title}' }}</el-tag>
                  <el-tag size="small" effect="plain">{{ '{date}' }}</el-tag>
                  <span>示例：{{ filenameExample }}.docx</span>
                </div>
              </div>
            </div>
            <div class="setting-row">
              <div>
                <strong>同名文件处理</strong>
                <span>当前安全版本不会覆盖已有文件，请在保存窗口换一个名称</span>
              </div>
              <el-tag type="success" effect="plain">拒绝覆盖</el-tag>
            </div>
          </section>

          <section id="settings-backup" class="settings-section">
            <h2>自动备份</h2>
            <div class="setting-row">
              <div>
                <strong>启动时检查自动备份</strong>
                <span>软件启动后只在备份到期时创建一份完整 .tqb 备份；备份失败会提示，但不会阻止软件打开。</span>
              </div>
              <el-switch v-model="form.automaticBackupEnabled" aria-label="启用自动备份" />
            </div>
            <div class="days-grid" :class="{ 'is-disabled': !form.automaticBackupEnabled }">
              <label>
                <span class="days-grid__label">备份间隔</span>
                <div class="number-with-unit">
                  <el-input-number
                    v-model="form.automaticBackupIntervalDays"
                    :min="1"
                    :max="365"
                    :disabled="!form.automaticBackupEnabled"
                    controls-position="right"
                    aria-label="自动备份间隔天数"
                  />
                  <span class="number-with-unit__unit">天</span>
                </div>
              </label>
              <label>
                <span class="days-grid__label">保留最近</span>
                <div class="number-with-unit">
                  <el-input-number
                    v-model="form.automaticBackupRetentionCount"
                    :min="1"
                    :max="50"
                    :disabled="!form.automaticBackupEnabled"
                    controls-position="right"
                    aria-label="自动备份保留数量"
                  />
                  <span class="number-with-unit__unit">份</span>
                </div>
              </label>
            </div>
            <div class="info-banner">轮换清理只会删除过期的“自动备份”，不会删除手工备份、恢复前备份、升级前备份或数据迁移备份。</div>
          </section>

          <section id="settings-confirmations" class="settings-section">
            <h2>操作确认</h2>
            <div class="setting-row">
              <div>
                <strong>显示普通操作确认（预留）</strong>
                <span>这个开关还没有接入实际操作流程，目前不会改变保存、批量编辑等确认提示。</span>
              </div>
              <div class="reserved-setting">
                <el-tag size="small" type="info" effect="plain">暂不可用</el-tag>
                <el-switch :model-value="false" disabled aria-label="显示普通操作确认（预留，暂不可用）" />
              </div>
            </div>
            <div class="info-banner">永久删除、覆盖和恢复等高风险操作的确认提示不能关闭。</div>
          </section>

          <section id="settings-maintenance" class="settings-section">
            <h2>统计与回收站</h2>
            <div class="days-grid">
              <label>
                <span class="days-grid__label">最近新增</span>
                <div class="number-with-unit">
                  <el-input-number v-model="form.recentAddedDays" :min="1" :max="3650" controls-position="right" aria-label="最近新增天数" />
                  <span class="number-with-unit__unit">天</span>
                </div>
              </label>
              <label>
                <span class="days-grid__label">最近使用</span>
                <div class="number-with-unit">
                  <el-input-number v-model="form.recentUsedDays" :min="1" :max="3650" controls-position="right" aria-label="最近使用天数" />
                  <span class="number-with-unit__unit">天</span>
                </div>
              </label>
              <label>
                <span class="days-grid__label">回收站提醒</span>
                <div class="number-with-unit">
                  <el-input-number v-model="form.recycleRetentionDays" :min="1" :max="3650" controls-position="right" aria-label="回收站提醒天数" />
                  <span class="number-with-unit__unit">天</span>
                </div>
              </label>
            </div>
            <div class="setting-row">
              <div>
                <strong>达到保留期限后</strong>
                <span>“仅提醒”不会自动永久删除题目</span>
              </div>
              <el-select v-model="form.recyclePolicy" class="medium-control" aria-label="达到回收站保留期限后的处理方式">
                <el-option label="只允许手动清理" value="manual_only" />
                <el-option label="提醒后由我决定" value="remind_only" />
              </el-select>
            </div>
          </section>
        </div>
      </div>

      <div v-else-if="activePage === 'license'" class="content-grid">
        <section class="surface data-card">
          <header class="license-card-header">
            <h2>桌面功能授权</h2>
            <el-tag :type="desktopLicenseTagType" effect="plain">{{ desktopLicenseLabel }}</el-tag>
          </header>
          <p>{{ appStore.license.desktop.message }}</p>
          <div class="folder-grid license-detail-grid">
            <div><strong>授权客户</strong><span>{{ appStore.license.desktop.customerName || '未授权' }}</span></div>
            <div><strong>许可证到期</strong><span>{{ formatLicenseDate(appStore.license.desktop.expiresAtMs) }}</span></div>
            <div><strong>宽限期结束</strong><span>{{ formatLicenseDate(appStore.license.desktop.graceEndsAtMs) }}</span></div>
            <div><strong>当前组卷上限</strong><span>{{ appStore.license.capabilities.maxQuestionsPerPaper == null ? '不限题量' : `每份 ${appStore.license.capabilities.maxQuestionsPerPaper} 题` }}</span></div>
          </div>
          <div class="license-actions">
            <el-button :loading="licenseBusy" :disabled="!desktopAvailable" @click="exportLicenseRequest">生成离线授权申请</el-button>
            <el-button type="primary" :loading="licenseBusy" :disabled="!desktopAvailable" @click="importDesktopLicense">导入许可证</el-button>
            <el-button type="danger" plain :loading="licenseBusy" :disabled="!desktopAvailable || !appStore.license.desktop.licenseId" @click="removeDesktopLicense">移除许可证</el-button>
          </div>
          <div class="info-banner">授权申请 .tkreq 只包含设备公钥、设备编号、软件版本和随机校验信息，不包含题库内容。许可证 .tklic 只在本机验证，不要求联网。</div>
        </section>

        <section class="surface data-card">
          <header><h2>本机设备编号</h2><span>离线许可证与此设备绑定。</span></header>
          <div class="path-box">
            <span class="license-device-mark" aria-hidden="true">ID</span>
            <div><strong class="device-id">{{ appStore.license.deviceId || '正在读取…' }}</strong><span>更换电脑后需要重新生成授权申请。</span></div>
            <el-button :icon="CopyDocument" :disabled="!appStore.license.deviceId" @click="copyDeviceId">复制编号</el-button>
          </div>
        </section>

        <section class="surface data-card">
          <header class="license-card-header"><h2>云账号与题目同步</h2><el-tag :type="cloudEntitlementTagType" effect="plain">{{ cloudEntitlementLabel }}</el-tag></header>
          <p>{{ cloudStatus?.message || '正在读取云账号状态…' }}</p>

          <div class="cloud-endpoint-row">
            <el-input
              v-model="cloudApiBaseUrl"
              :disabled="!desktopAvailable || cloudOperationBusy"
              placeholder="https://api.tktiku.cn"
              aria-label="云服务 HTTPS 地址"
            />
            <el-button :loading="cloudBusy" :disabled="!desktopAvailable || cloudOperationBusy" @click="saveCloudApiUrl">保存地址</el-button>
          </div>
          <div class="info-banner">软件默认连接 https://api.tktiku.cn。只有迁移到其他合规服务地址时才需要修改；正式环境拒绝明文 HTTP。</div>

          <template v-if="cloudStatus?.loggedIn">
            <div class="folder-grid license-detail-grid license-detail-grid--cloud cloud-account-grid">
              <div><strong>当前账号</strong><span>{{ cloudStatus.user?.username }}</span></div>
              <div><strong>账号邮箱</strong><span>{{ cloudStatus.user?.email || '未填写' }}</span></div>
              <div><strong>云服务套餐</strong><span>{{ cloudStatus.entitlement?.plan_code || '—' }}</span></div>
              <div><strong>云授权到期</strong><span>{{ formatCloudTime(cloudStatus.entitlement?.expires_at) }}</span></div>
              <div><strong>上次同步</strong><span>{{ formatCloudTime(cloudStatus.lastSyncAtMs) }}</span></div>
              <div><strong>待处理冲突</strong><span>{{ cloudStatus.conflictCount }} 项</span></div>
            </div>
            <div v-if="cloudPreflight" class="info-banner">
              <template v-if="cloudPreflight.bothNonEmpty">
                首次同步预检：本机 {{ cloudPreflight.localQuestionCount }} 道题，云端 {{ cloudPreflight.cloudQuestionCount }} 道题。默认智能合并，两边新增题目都会保留，同名分类会自动归一。
              </template>
              <template v-else-if="cloudPreflight.localHasData">
                首次同步预检：云端为空，将从本机 {{ cloudPreflight.localQuestionCount }} 道题建立云端副本。
              </template>
              <template v-else-if="cloudPreflight.cloudHasData">
                首次同步预检：本机为空，将下载云端 {{ cloudPreflight.cloudQuestionCount }} 道题。
              </template>
              <template v-else>首次同步预检：本机和云端均为空。</template>
            </div>
            <div class="license-actions">
              <el-button type="primary" :loading="cloudSyncBusy" :disabled="cloudOperationBusy || !cloudStatus.canSync" @click="syncCloudNow">立即同步</el-button>
              <el-button :loading="cloudBusy" :disabled="cloudOperationBusy" @click="logoutCloudAccount">退出云账号</el-button>
            </div>
            <div v-if="cloudConflicts.length" class="cloud-conflicts">
              <strong>需要确认的同步冲突</strong>
              <div v-for="conflict in cloudConflicts" :key="conflict.id" class="cloud-conflict-row">
                <div>
                  <span>{{ cloudEntityKindLabel(conflict.entityKind) }} · {{ conflict.entityId.slice(0, 8) }}</span>
                  <small>{{ conflict.message }}</small>
                </div>
                <el-button-group>
                  <el-button size="small" :disabled="cloudOperationBusy" @click="resolveCloudConflict(conflict, 'keep_local')">保留本机</el-button>
                  <el-button size="small" type="primary" plain :disabled="cloudOperationBusy" @click="resolveCloudConflict(conflict, 'use_cloud')">采用云端</el-button>
                </el-button-group>
              </div>
            </div>
          </template>

          <template v-else>
            <el-radio-group v-model="cloudAuthMode" class="cloud-auth-mode" :disabled="cloudOperationBusy">
              <el-radio-button value="login">登录</el-radio-button>
              <el-radio-button value="register">注册</el-radio-button>
            </el-radio-group>
            <div class="cloud-auth-form">
              <template v-if="cloudAuthMode === 'register'">
                <el-input v-model="cloudAuth.username" :disabled="cloudOperationBusy" maxlength="32" placeholder="用户名（3—32 个字符）" autocomplete="username" />
                <el-input v-model="cloudAuth.email" :disabled="cloudOperationBusy" maxlength="254" placeholder="邮箱（可选）" autocomplete="email" />
              </template>
              <el-input v-else v-model="cloudAuth.account" :disabled="cloudOperationBusy" placeholder="用户名或邮箱" autocomplete="username" />
              <el-input v-model="cloudAuth.password" :disabled="cloudOperationBusy" type="password" show-password placeholder="密码" autocomplete="current-password" @keyup.enter="submitCloudAuth" />
              <el-button type="primary" :loading="cloudBusy" :disabled="!desktopAvailable || cloudOperationBusy || !cloudStatus?.configured" @click="submitCloudAuth">
                {{ cloudAuthMode === 'register' ? '注册并登录' : '登录云账号' }}
              </el-button>
            </div>
          </template>

          <div class="info-banner">云账号只控制题目云同步和后续网页版；桌面离线授权、15 天试用、过期后的单题录入与 10 题组卷规则均保持独立。每个本地题库首次同步后会绑定一个教师账号，防止账号间串题。</div>
        </section>
      </div>

      <div v-else-if="activePage === 'data'" class="content-grid">
        <section class="surface data-card">
          <header><h2>当前数据位置</h2><span>数据库、图片、模板和备份都由统一目录管理。</span></header>
          <div class="path-box">
            <el-icon aria-hidden="true"><FolderOpened /></el-icon>
            <div><strong>{{ desktopAvailable ? (appStore.dataRoot || '正在读取数据目录…') : '浏览器演示无法读取真实数据目录' }}</strong><span>{{ desktopAvailable ? '当前使用中的安全位置' : '请在 Windows 桌面版中查看' }}</span></div>
            <el-button :icon="CopyDocument" :disabled="!desktopAvailable || !appStore.dataRoot" @click="copyDataRoot">复制路径</el-button>
          </div>
          <div class="folder-grid">
            <div><strong>database/</strong><span>SQLite 数据库</span></div>
            <div><strong>resources/</strong><span>题目图片等资源</span></div>
            <div><strong>templates/</strong><span>Word 模板</span></div>
            <div><strong>backup/</strong><span>软件备份</span></div>
            <div><strong>export/</strong><span>默认导出文件</span></div>
          </div>
        </section>

        <section class="surface data-card">
          <header><h2>更改数据位置</h2><span>新位置必须先复制并验证成功，才能切换使用。</span></header>
          <div class="migration-row">
            <el-input :model-value="desktopAvailable ? appStore.dataRoot : '浏览器演示不可用'" readonly aria-label="当前数据目录" />
            <el-button type="primary" :disabled="!desktopAvailable" @click="openDataMigration">{{ desktopAvailable ? '前往数据管理迁移' : '桌面版才能迁移' }}</el-button>
          </div>
          <div class="warning-banner">更改数据位置不会直接移动或删除旧数据。迁移完成并校验通过后，旧目录仍会保留为安全副本。</div>
        </section>
      </div>

      <div v-else class="content-grid">
        <section class="surface about-card">
          <img class="app-mark" :src="appLogoPath" alt="" />
          <div>
            <h2>TK试题题库</h2>
            <p>{{ desktopAvailable ? 'TK试题题库 Windows 离线管理软件' : 'TK试题题库浏览器界面演示' }}</p>
            <div class="about-meta">
              <span>版本 {{ appStore.appVersion }}</span>
              <span><i class="status-dot" />{{ desktopAvailable ? '桌面离线模式' : '浏览器演示模式' }}</span>
              <span v-if="desktopAvailable" :class="appStore.databaseHealthy ? 'text-success' : 'text-danger'">{{ appStore.databaseHealthy ? '数据库正常' : '数据库异常' }}</span>
              <span v-else>不连接桌面数据库</span>
            </div>
          </div>
        </section>
        <section class="surface about-detail">
          <h2>首期范围</h2>
          <p v-if="desktopAvailable">题目录入、Word / Excel 导入、题库管理、组卷、模板管理、历史试卷、题库 Word/Excel 导出、支持图片表格公式的试卷 Word 导出、备份恢复和统计均可在本地运行。</p>
          <p v-else>当前页面只用于浏览和体验界面。Word、Excel、模板、备份、恢复、导出与目录操作均不会访问电脑中的真实文件。</p>
          <p class="muted">当前 Windows 交付包含教师账号和按账号隔离的题目云同步；网页版和试卷多版本仍属于后续开发范围。</p>
        </section>
        <section class="surface about-detail update-card">
          <div>
            <h2>软件更新</h2>
            <p>{{ updateStatus }}</p>
            <p class="muted">更新包必须通过发布签名校验才会安装；下载或检查失败不会影响离线题库功能。</p>
          </div>
          <el-button
            type="primary"
            :loading="updateBusy"
            :disabled="!desktopAvailable"
            @click="checkAppUpdateManually"
          >{{ desktopAvailable ? '检查更新' : '桌面版可用' }}</el-button>
        </section>
      </div>
    </section>
  </div>
</template>

<style scoped>
.nav-square {
  width: 18px;
  height: 18px;
  flex: 0 0 18px;
  border: 1px solid #94a3b8;
  border-radius: 5px;
  background: white;
}

.sidebar-menu__item.is-active .nav-square {
  border-color: #60a5fa;
  background: #eff6ff;
}

.settings-page {
  display: flex;
  flex-direction: column;
}

.update-card {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 24px;
}

.update-card > div {
  min-width: 0;
}

.browser-runtime-note {
  margin-bottom: 14px;
}

.reserved-setting {
  display: flex;
  align-items: center;
  gap: 8px;
}

.settings-layout {
  min-height: 470px;
  flex: 1;
  display: grid;
  grid-template-columns: 220px minmax(640px, 1fr);
  gap: 14px;
}

.settings-subnav {
  padding: 10px;
  align-self: stretch;
}

.settings-subnav button {
  width: 100%;
  height: 46px;
  padding: 0 12px;
  border: 0;
  border-radius: 7px;
  background: transparent;
  color: #334155;
  font-size: 12px;
  font-weight: 600;
  text-align: left;
}

.settings-subnav button:hover {
  background: #f8fafc;
}

.settings-subnav button.is-active {
  background: #eaf3ff;
  color: #1d4ed8;
}

.settings-form-card {
  min-height: 0;
  overflow: auto;
  scroll-behavior: smooth;
}

.settings-section {
  padding: 22px;
  scroll-margin-top: 12px;
}

.settings-section-heading {
  min-height: 32px;
  margin-bottom: 8px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
}

.settings-form-actions {
  display: flex;
  align-items: center;
  gap: 8px;
}

.settings-form-actions :deep(.el-button + .el-button) {
  margin-left: 0;
}

.settings-section + .settings-section {
  border-top: 1px solid var(--border);
}

.settings-section h2,
.data-card h2,
.about-card h2,
.about-detail h2 {
  margin: 0;
  color: #111827;
  font-size: 15px;
}

.setting-row {
  min-height: 64px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 28px;
}

.setting-row--start {
  align-items: flex-start;
  padding-top: 16px;
}

.setting-row > div:first-child {
  min-width: 220px;
  display: flex;
  flex-direction: column;
  gap: 5px;
}

.setting-row strong {
  color: #1f2937;
  font-size: 12px;
}

.setting-row span {
  color: #64748b;
  font-size: 10px;
}

.wide-control {
  width: min(100%, 600px);
}

.medium-control {
  width: 250px;
}

.filename-control {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.filename-help {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px;
}

.filename-help > span:last-child {
  margin-left: 3px;
  color: #64748b;
  font-size: 10px;
}

.days-grid {
  margin: 16px 0 8px;
  display: grid;
  grid-template-columns: repeat(2, minmax(260px, 1fr));
  gap: 10px 20px;
}

.days-grid label {
  min-height: 46px;
  display: grid;
  grid-template-columns: minmax(100px, 1fr) 160px;
  align-items: center;
  gap: 8px;
  color: #334155;
  font-size: 11px;
}

.days-grid__label {
  min-width: 0;
}

.number-with-unit {
  width: 160px;
  display: grid;
  grid-template-columns: minmax(0, 128px) 24px;
  align-items: center;
  gap: 8px;
}

.number-with-unit :deep(.el-input-number) {
  width: 100%;
}

.number-with-unit__unit {
  color: #475569;
  line-height: 1;
  white-space: nowrap;
}

.data-card,
.about-detail {
  padding: 22px;
}

.data-card header {
  margin-bottom: 18px;
}

.data-card header span {
  margin-top: 6px;
  display: block;
  color: #64748b;
  font-size: 11px;
}

.license-card-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
}

.data-card .license-card-header .el-tag {
  width: auto;
  margin-top: 0;
  display: inline-flex;
}

.license-detail-grid {
  grid-template-columns: repeat(4, minmax(0, 1fr));
}

.license-detail-grid--cloud {
  grid-template-columns: repeat(2, minmax(0, 1fr));
}

.license-actions {
  margin: 14px 0;
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px;
}

.cloud-endpoint-row {
  margin: 14px 0 10px;
  display: grid;
  grid-template-columns: minmax(320px, 1fr) auto;
  gap: 8px;
}

.cloud-auth-mode {
  margin-top: 14px;
}

.cloud-auth-form {
  max-width: 620px;
  margin: 10px 0 14px;
  display: grid;
  grid-template-columns: repeat(2, minmax(180px, 1fr)) auto;
  gap: 8px;
}

.cloud-account-grid {
  grid-template-columns: repeat(3, minmax(0, 1fr));
}

.cloud-conflicts {
  margin: 14px 0;
  padding: 12px;
  border: 1px solid #fde68a;
  border-radius: 8px;
  background: #fffbeb;
}

.cloud-conflicts > strong {
  color: #92400e;
  font-size: 12px;
}

.cloud-conflict-row {
  padding-top: 10px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.cloud-conflict-row > div:first-child {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 3px;
}

.cloud-conflict-row span {
  color: #78350f;
  font-size: 11px;
}

.cloud-conflict-row small {
  color: #a16207;
  font-size: 9px;
}

.license-device-mark {
  width: 34px;
  height: 34px;
  display: grid;
  place-items: center;
  border-radius: 9px;
  background: #dbeafe;
  color: #2563eb;
  font-size: 11px;
  font-weight: 700;
}

.device-id {
  font-family: Consolas, 'Courier New', monospace;
}

.path-box {
  min-height: 76px;
  padding: 14px;
  display: grid;
  grid-template-columns: 34px minmax(0, 1fr) auto;
  align-items: center;
  gap: 12px;
  border: 1px solid #dbeafe;
  border-radius: 8px;
  background: #f8fbff;
  color: #2563eb;
}

.path-box > div {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 5px;
}

.path-box strong {
  overflow: hidden;
  color: #334155;
  font-size: 12px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.path-box span {
  color: #64748b;
  font-size: 10px;
}

.folder-grid {
  margin-top: 14px;
  display: grid;
  grid-template-columns: repeat(5, minmax(120px, 1fr));
  gap: 8px;
}

.folder-grid > div {
  padding: 12px;
  display: flex;
  flex-direction: column;
  gap: 5px;
  border: 1px solid var(--border);
  border-radius: 7px;
}

.folder-grid strong {
  color: #334155;
  font-size: 11px;
}

.folder-grid span {
  color: #94a3b8;
  font-size: 9px;
}

.migration-row {
  display: grid;
  grid-template-columns: minmax(320px, 1fr) auto;
  gap: 8px;
}

.data-card .warning-banner {
  margin-top: 14px;
}

.about-card {
  padding: 28px;
  display: flex;
  align-items: center;
  gap: 18px;
}

.app-mark {
  width: 58px;
  height: 58px;
  display: block;
  object-fit: contain;
}

.about-card p {
  margin: 6px 0 12px;
  color: #64748b;
  font-size: 11px;
}

.about-meta {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 16px;
  color: #475569;
  font-size: 11px;
}

.about-meta span {
  display: inline-flex;
  align-items: center;
  gap: 7px;
}

.about-detail p {
  max-width: 820px;
  color: #334155;
  font-size: 12px;
  line-height: 1.8;
}

@media (max-width: 1280px) {
  .settings-layout {
    grid-template-columns: 190px minmax(600px, 1fr);
  }

  .folder-grid {
    grid-template-columns: repeat(3, 1fr);
  }
}
</style>
