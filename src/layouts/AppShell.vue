<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { ElMessage } from 'element-plus'
import { Minus, FullScreen, Close, Clock, Connection } from '@element-plus/icons-vue'
import { useAppStore } from '../stores/app'
import { errorMessage } from '../services/errors'
import { isDesktopRuntime } from '../services/backend'
import { suppressUnconfiguredContextMenu } from '../utils/contextMenuGuard'

const route = useRoute()
const router = useRouter()
const appStore = useAppStore()
const firstSubjectName = ref('')
const creatingSubject = ref(false)
const skipOnboarding = ref(false)
const desktopAvailable = isDesktopRuntime()
const appLogoPath = '/tk-logo.png'

const navItems = [
  { label: '题库管理', path: '/questions', matches: ['/questions', '/duplicate-check', '/recycle-bin', '/taxonomy', '/tags', '/question-types'] },
  { label: '题目录入', path: '/questions/new', matches: ['/questions/new', '/questions/document', '/word-import'] },
  { label: '选题组卷', path: '/papers', matches: ['/papers'] },
  { label: '历史试卷', path: '/history', matches: ['/history'] },
  { label: '模板管理', path: '/templates', matches: ['/templates'] },
  { label: '数据备份与恢复', path: '/data', matches: ['/data'] },
  { label: '题库统计', path: '/statistics', matches: ['/statistics'] },
  { label: '系统设置', path: '/settings', matches: ['/settings'] },
]

const activePath = computed(() => {
  const path = route.path
  const exactMatch = navItems.find((item) => item.matches.includes(path))
  if (exactMatch) return exactMatch.path
  return navItems.find((item) => item.matches.some((match) => path.startsWith(`${match}/`)))?.path
})

const desktopModeLabel = computed(() => {
  if (!desktopAvailable) return '浏览器演示模式'
  if (!appStore.databaseHealthy) return '数据库异常'
  const professional = appStore.license.desktop.state === 'active' || appStore.license.desktop.state === 'grace'
  if (!professional) return '基础桌面模式'
  return appStore.license.desktop.plan === 'trial' ? '桌面专业版试用' : '桌面专业版'
})

function guardSystemPrintShortcut(event: KeyboardEvent) {
  if (!(event.ctrlKey || event.metaKey) || event.key.toLocaleLowerCase() !== 'p') return
  if (appStore.license.capabilities.canPrint) return
  event.preventDefault()
  event.stopPropagation()
  ElMessage.warning('基础桌面模式不支持打印或系统“打印为 PDF”；单题录入和最多 10 题组卷仍可使用。')
}

onMounted(() => {
  window.addEventListener('contextmenu', suppressUnconfiguredContextMenu)
  window.addEventListener('keydown', guardSystemPrintShortcut, true)
})

onBeforeUnmount(() => {
  window.removeEventListener('contextmenu', suppressUnconfiguredContextMenu)
  window.removeEventListener('keydown', guardSystemPrintShortcut, true)
})

async function minimize() {
  if (desktopAvailable) await getCurrentWindow().minimize()
}

async function toggleMaximize() {
  if (desktopAvailable) await getCurrentWindow().toggleMaximize()
}

async function closeWindow() {
  if (desktopAvailable) await getCurrentWindow().close()
}

async function createFirstSubject() {
  if (!firstSubjectName.value.trim()) {
    ElMessage.warning('请先填写一个学科名称')
    return
  }
  creatingSubject.value = true
  try {
    await appStore.createInitialSubject(firstSubjectName.value)
    ElMessage.success('首个学科已创建，现在可以录入或导入题目')
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '创建学科失败'))
  } finally {
    creatingSubject.value = false
  }
}
</script>

<template>
  <div class="shell" :class="{ 'is-browser-preview': !desktopAvailable }">
    <header class="titlebar" data-tauri-drag-region>
      <div class="titlebar__brand" data-tauri-drag-region>
        <img class="app-logo" :src="appLogoPath" alt="" />
        <span>TK试题题库</span>
        <span class="titlebar__version">v{{ appStore.appVersion }} · {{ desktopAvailable ? 'Windows 桌面版' : '浏览器界面演示' }}</span>
      </div>
      <div v-if="desktopAvailable" class="titlebar__controls">
        <button aria-label="最小化" title="最小化" @click="minimize"><el-icon><Minus /></el-icon></button>
        <button aria-label="最大化或还原" title="最大化或还原" @click="toggleMaximize"><el-icon><FullScreen /></el-icon></button>
        <button class="titlebar__close" aria-label="关闭" title="关闭" @click="closeWindow"><el-icon><Close /></el-icon></button>
      </div>
    </header>

    <div v-if="!desktopAvailable" class="browser-preview-banner" role="status" aria-live="polite">
      <strong>浏览器界面演示</strong>
      <span>不会读写真实文件；Word / Excel 导入、模板导入、备份恢复、目录选择和数据迁移只能在桌面版执行。</span>
    </div>

    <nav class="topnav">
      <div class="topnav__items">
        <button
          v-for="item in navItems"
          :key="item.path"
          class="topnav__item"
          :class="{ 'is-active': activePath === item.path }"
          @click="router.push(item.path)"
        >
          {{ item.label }}
        </button>
      </div>
      <div class="topnav__status">
        <el-icon><Clock /></el-icon>
        <span class="status-dot" />
        <span>{{ desktopModeLabel }}</span>
        <el-icon><Connection /></el-icon>
      </div>
    </nav>

    <main class="shell__content">
      <div v-if="appStore.loading" class="shell-loading">
        <el-skeleton :rows="7" animated />
      </div>
      <div v-else-if="!appStore.databaseHealthy" class="database-error">
        <div class="database-error__card surface danger-card">
          <div class="database-error__icon">!</div>
          <h1>本地数据库加载失败</h1>
          <p>{{ appStore.error || '为了保护现有题库，软件已经停止所有写入操作。' }}</p>
          <div class="database-error__actions">
            <el-button @click="closeWindow">安全退出</el-button>
            <el-button type="primary" :loading="appStore.loading" @click="appStore.retryDatabase">重新加载</el-button>
          </div>
        </div>
      </div>
      <div v-else-if="!appStore.initialized && !skipOnboarding" class="onboarding">
        <section class="onboarding__card surface">
          <img class="onboarding__logo" :src="appLogoPath" alt="" />
          <h1>欢迎使用TK试题题库</h1>
          <p v-if="desktopAvailable">软件已完成本地数据库和数据目录初始化。创建第一个学科后，就可以开始录题、导入 Word 和组卷。</p>
          <p v-else>这是浏览器界面演示。可以浏览和体验题库界面，但不会读取 Word、写入备份或迁移电脑中的真实文件。</p>
          <div class="onboarding__steps">
            <div><strong>1</strong><span>创建学科</span><small>例如教育学、语文或数学</small></div>
            <div><strong>2</strong><span>录入或导入</span><small>支持手动录入和 .docx</small></div>
            <div><strong>3</strong><span>选题并导出</span><small>组卷后生成 Word 文件</small></div>
          </div>
          <div class="onboarding__form">
            <el-input v-model="firstSubjectName" size="large" placeholder="请输入第一个学科名称" @keyup.enter="createFirstSubject" />
            <el-button type="primary" size="large" :loading="creatingSubject" @click="createFirstSubject">创建并进入题库</el-button>
          </div>
          <button class="onboarding__skip" @click="skipOnboarding = true; router.push('/questions')">暂时跳过，先看看空题库</button>
        </section>
      </div>
      <router-view v-else />
    </main>
  </div>
</template>

<style scoped>
.shell {
  height: 100%;
  display: grid;
  grid-template-rows: var(--titlebar-height) var(--topnav-height) minmax(0, 1fr);
  background: #f4f7fb;
}

.shell.is-browser-preview {
  grid-template-rows: var(--titlebar-height) auto var(--topnav-height) minmax(0, 1fr);
}

.browser-preview-banner {
  min-height: 38px;
  padding: 8px 18px;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 10px;
  border-bottom: 1px solid #f59e0b;
  background: #fff7d6;
  color: #78350f;
  font-size: 12px;
  line-height: 1.5;
  text-align: center;
}

.browser-preview-banner strong {
  flex: 0 0 auto;
  padding: 2px 7px;
  border-radius: 4px;
  background: #f59e0b;
  color: #fff;
}

.titlebar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  border-bottom: 1px solid #edf1f6;
  background: #f9fbfd;
  color: #475569;
  user-select: none;
}

.titlebar__brand {
  height: 100%;
  padding-left: 14px;
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  font-weight: 700;
}

.app-logo {
  width: 20px;
  height: 20px;
  display: block;
  object-fit: contain;
}

.titlebar__version {
  color: #94a3b8;
  font-weight: 400;
}

.titlebar__controls {
  height: 100%;
  display: flex;
}

.titlebar__controls button {
  width: 44px;
  height: 100%;
  display: grid;
  place-items: center;
  border: 0;
  background: transparent;
  color: #64748b;
}

.titlebar__controls button:hover {
  background: #edf2f7;
}

.titlebar__controls .titlebar__close:hover {
  background: #e81123;
  color: #fff;
}

.topnav {
  padding: 0 18px;
  display: flex;
  align-items: stretch;
  justify-content: space-between;
  border-bottom: 1px solid var(--border);
  background: #fff;
  box-shadow: 0 1px 2px rgb(15 23 42 / 3%);
}

.topnav__items {
  min-width: 0;
  display: flex;
  align-items: stretch;
}

.topnav__item {
  position: relative;
  padding: 0 15px;
  border: 0;
  background: transparent;
  color: #475569;
  font-size: 12px;
  white-space: nowrap;
}

.topnav__item:hover {
  color: var(--blue-600);
  background: #fbfdff;
}

.topnav__item.is-active {
  color: var(--blue-600);
  font-weight: 700;
}

.topnav__item.is-active::after {
  position: absolute;
  right: 14px;
  bottom: 0;
  left: 14px;
  height: 2px;
  border-radius: 2px 2px 0 0;
  background: var(--blue-600);
  content: '';
}

.topnav__status {
  display: flex;
  align-items: center;
  gap: 8px;
  color: #64748b;
  font-size: 11px;
  white-space: nowrap;
}

.shell__content {
  min-width: 0;
  min-height: 0;
  overflow: hidden;
}

.shell-loading {
  width: min(880px, 80%);
  margin: 80px auto;
  padding: 30px;
  border-radius: 12px;
  background: #fff;
}

.database-error {
  height: 100%;
  display: grid;
  place-items: center;
  padding: 30px;
}

.database-error__card {
  width: 520px;
  padding: 40px;
  text-align: center;
}

.database-error__icon {
  width: 56px;
  height: 56px;
  margin: 0 auto 18px;
  display: grid;
  place-items: center;
  border-radius: 50%;
  background: #fee2e2;
  color: #dc2626;
  font-size: 26px;
  font-weight: 700;
}

.database-error h1 {
  margin: 0 0 10px;
  font-size: 22px;
}

.database-error p {
  margin: 0 0 24px;
  color: #64748b;
  line-height: 1.7;
}

.database-error__actions {
  display: flex;
  justify-content: center;
  gap: 10px;
}

.onboarding {
  height: 100%;
  display: grid;
  place-items: center;
  padding: 32px;
  background: radial-gradient(circle at 50% 35%, #f7faff 0, #f1f5fa 58%, #edf2f7 100%);
}

.onboarding__card {
  width: 620px;
  padding: 38px 42px 30px;
  text-align: center;
}

.onboarding__logo {
  width: 58px;
  height: 58px;
  margin: 0 auto 16px;
  display: block;
  object-fit: contain;
}

.onboarding h1 {
  margin: 0;
  font-size: 24px;
}

.onboarding > section > p {
  max-width: 470px;
  margin: 10px auto 22px;
  color: #64748b;
  font-size: 12px;
  line-height: 1.8;
}

.onboarding__steps {
  margin-bottom: 20px;
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 10px;
}

.onboarding__steps div {
  padding: 13px 9px;
  display: grid;
  gap: 5px;
  border: 1px solid #e5eaf2;
  border-radius: 8px;
  background: #fbfdff;
}

.onboarding__steps strong {
  width: 24px;
  height: 24px;
  margin: 0 auto;
  display: grid;
  place-items: center;
  border-radius: 50%;
  background: #eff6ff;
  color: #2563eb;
  font-size: 11px;
}

.onboarding__steps span {
  font-size: 11px;
  font-weight: 650;
}

.onboarding__steps small {
  color: #94a3b8;
  font-size: 9px;
}

.onboarding__form {
  display: grid;
  grid-template-columns: 1fr 155px;
  gap: 8px;
}

.onboarding__skip {
  margin-top: 14px;
  border: 0;
  background: transparent;
  color: #64748b;
  font-size: 10px;
}

@media (max-width: 1280px) {
  .topnav__item {
    padding: 0 9px;
    font-size: 11px;
  }
}
</style>
