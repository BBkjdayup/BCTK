<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import {
  Check,
  CopyDocument,
  DataBoard,
  Delete,
  Document,
  Link,
  Plus,
  Refresh,
  SwitchButton,
  User,
  View,
} from '@element-plus/icons-vue'
import QuestionStemSummary from '../components/QuestionStemSummary.vue'
import SupportContactDialog from '../components/SupportContactDialog.vue'
import { backend, isDesktopRuntime } from '../services/backend'
import { useMiniProgramCloudStore } from '../stores/miniProgramCloud'
import { errorMessage } from '../services/errors'
import { useMiniProgramStore } from '../stores/miniProgram'
import type { Paper, PaperSummary } from '../types/domain'
import type { MiniProgramMemberStatus, MiniProgramPublication } from '../types/miniProgram'
import { currentPublicationRows, publicationActions, publicationRow, type PublicationRow } from '../utils/miniProgramPublicationRows'

type Section = 'papers' | 'invites' | 'members' | 'plan'
type MemberFilter = 'all' | MiniProgramMemberStatus

const live = isDesktopRuntime()
const cloudStore = useMiniProgramCloudStore()
const miniStore = live ? cloudStore : useMiniProgramStore()
const authForm = reactive({ account: '', password: '', register: false })
const authBusy = ref(false)
const cloudError = ref('')
const supportOpen = ref(false)
const supportRefreshing = ref(false)
async function refreshAuthorization() {
  if (supportRefreshing.value || !cloudStore.user) return
  supportRefreshing.value = true
  try { await cloudStore.refresh(); cloudError.value = ''; ElMessage.success('授权状态已刷新') }
  catch (reason) { ElMessage.error(errorMessage(reason, '授权状态刷新失败')) }
  finally { supportRefreshing.value = false }
}
async function connectAccount() {
  if (authBusy.value) return
  authBusy.value = true
  cloudError.value = ''
  try { await cloudStore.login(authForm.account, authForm.password, authForm.register) }
  catch (reason) { cloudError.value = errorMessage(reason, '登录未完成') }
  finally { authForm.password = ''; authBusy.value = false }
}
async function refreshCloud() {
  if (!live || !cloudStore.user) return
  try { await cloudStore.refresh(); cloudError.value = ''; if (paperFilter.value === 'withdrawn') await loadWithdrawn() }
  catch (reason) { cloudError.value = errorMessage(reason, '服务状态读取失败') }
}
async function signOut() {
  try { await cloudStore.logout() }
  catch (reason) { ElMessage.warning(errorMessage(reason, '已退出本机账号，服务端退出暂未确认')) }
}
async function perform(action: () => unknown | Promise<unknown>, message: string) {
  try { await action(); ElMessage.success(message); return true }
  catch (reason) { ElMessage.error(errorMessage(reason, '操作未完成')); return false }
}
const activeSection = ref<Section>('papers')
const papers = ref<PaperSummary[]>([])
const papersLoading = ref(false)
const paperKeyword = ref('')
const paperFilter = ref<'current' | 'withdrawn'>('current')
const paperPage = ref(1)
const paperPageSize = 20
const withdrawn = ref<MiniProgramPublication[]>([])
const withdrawnTotal = ref(0)
const withdrawnLoading = ref(false)
const withdrawnError = ref('')
let withdrawnRequest = 0
const memberFilter = ref<MemberFilter>('pending')
const previewOpen = ref(false)
const previewLoading = ref(false)
const previewPaper = ref<Paper | null>(null)
const inviteDialogOpen = ref(false)
const memberDialogOpen = ref(false)
const inviteForm = reactive({ label: '', validDays: 30 })
const memberForm = reactive({ name: '', account: '', inviteId: '' })

const sections: Array<{ id: Section; label: string; icon: typeof Document }> = [
  { id: 'papers', label: '发布试卷', icon: Document },
  { id: 'invites', label: '邀请码', icon: Link },
  { id: 'members', label: '成员审核', icon: User },
  { id: 'plan', label: '名额与授权', icon: DataBoard },
]

function toBoundary(value: string, endOfDay = false) {
  if (!value) return null
  const timestamp = new Date(`${value}T${endOfDay ? '23:59:59.999' : '00:00:00'}`).getTime()
  return Number.isFinite(timestamp) ? timestamp : null
}

const serviceStatus = computed(() => {
  if (miniStore.settings.paused) return { label: '已暂停', type: 'warning' as const }
  const starts = toBoundary(miniStore.settings.serviceStarts)
  const ends = toBoundary(miniStore.settings.serviceEnds, true)
  if (!starts || !ends || ends < starts) return { label: '未配置', type: 'info' as const }
  const now = Date.now()
  if (now < starts) return { label: '未开始', type: 'info' as const }
  if (now > ends) return { label: '已到期', type: 'danger' as const }
  return { label: '使用中', type: 'success' as const }
})

const serviceAvailable = computed(() => serviceStatus.value.label === '使用中')
const quotaPercent = computed(() => miniStore.settings.quota
  ? Math.min(100, Math.round((miniStore.usedQuota / miniStore.settings.quota) * 100))
  : 0)
const pendingCount = computed(() => miniStore.members.filter((member) => member.status === 'pending').length)
const servicePeriodLabel = computed(() => {
  const starts = miniStore.settings.serviceStarts || '未设置'
  const ends = miniStore.settings.serviceEnds || '未设置'
  return `${starts} 至 ${ends}`
})

const currentRows = computed(() => currentPublicationRows(papers.value, miniStore.publications, paperKeyword.value))
const demoWithdrawn = computed(() => miniStore.publications.filter(p => p.published === false)
  .map(p => publicationRow(papers.value.find(local => local.id === p.paperId) ?? null, p))
  .filter(row => row.title.toLocaleLowerCase('zh-CN').includes(paperKeyword.value.trim().toLocaleLowerCase('zh-CN')))
  .sort((a, b) => b.updatedAt - a.updatedAt || a.id.localeCompare(b.id)))
const paperTotal = computed(() => paperFilter.value === 'current' ? currentRows.value.length : live ? withdrawnTotal.value : demoWithdrawn.value.length)
const filteredPapers = computed(() => {
  if (paperFilter.value === 'withdrawn' && live) {
    const locals = new Map(papers.value.map(p => [p.id, p]))
    return withdrawn.value.map(p => publicationRow(locals.get(p.paperId) ?? null, p))
  }
  const rows = paperFilter.value === 'current' ? currentRows.value : demoWithdrawn.value
  return rows.slice((paperPage.value - 1) * paperPageSize, paperPage.value * paperPageSize)
})
const publicationReady = computed(() => !live || (cloudStore.ready && !cloudStore.busy))
const paperListLoading = computed(() => papersLoading.value || withdrawnLoading.value)

async function loadWithdrawn() {
  const request = ++withdrawnRequest
  if (!live || !cloudStore.user || paperFilter.value !== 'withdrawn') return
  withdrawnLoading.value = true; withdrawnError.value = ''
  try {
    const result = await cloudStore.searchWithdrawn(paperKeyword.value.trim(), paperPage.value, paperPageSize)
    if (request !== withdrawnRequest) return
    withdrawn.value = result.items; withdrawnTotal.value = result.total
    paperPage.value = Math.min(paperPage.value, Math.max(1, Math.ceil(result.total / paperPageSize)))
  } catch (reason) {
    if (request !== withdrawnRequest) return
    withdrawn.value = []; withdrawnTotal.value = 0
    withdrawnError.value = errorMessage(reason, '读取已取消公开记录失败')
  } finally {
    if (request === withdrawnRequest) withdrawnLoading.value = false
  }
}
watch([paperFilter, paperKeyword], () => { paperPage.value = 1 })
watch(() => cloudStore.user?.id, () => { withdrawn.value = []; withdrawnTotal.value = 0; withdrawnError.value = ''; paperPage.value = 1 })
watch([paperFilter, paperKeyword, paperPage, () => cloudStore.user?.id], (_, __, onCleanup) => {
  withdrawnRequest++; withdrawnLoading.value = false
  if (paperFilter.value !== 'withdrawn' || !live || !cloudStore.user) return
  withdrawnLoading.value = true
  const timer = setTimeout(() => { void loadWithdrawn() }, 200)
  onCleanup(() => { clearTimeout(timer); withdrawnRequest++ })
})
watch(paperTotal, total => {
  if (paperFilter.value === 'current' || !live) paperPage.value = Math.min(paperPage.value, Math.max(1, Math.ceil(total / paperPageSize)))
})

const filteredInvites = computed(() => [...miniStore.invites]
  .sort((left, right) => right.createdAt - left.createdAt))

const filteredMembers = computed(() => miniStore.members
  .filter((member) => memberFilter.value === 'all' || member.status === memberFilter.value)
  .sort((left, right) => right.requestedAt - left.requestedAt))

const countedMembers = computed(() => [...miniStore.countedMembers]
  .sort((left, right) => (right.joinedAt ?? right.requestedAt) - (left.joinedAt ?? left.requestedAt)))

function formatDateTime(value: number | null) {
  if (!value) return '—'
  return new Intl.DateTimeFormat('zh-CN', {
    year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit', hour12: false,
  }).format(value)
}

function formatDate(value: number) {
  return new Intl.DateTimeFormat('zh-CN', { year: 'numeric', month: '2-digit', day: '2-digit' }).format(value)
}

function inviteExpired(invite: { expiresAt: number }) {
  return invite.expiresAt < Date.now()
}

function inviteStatus(invite: { active: boolean; expiresAt: number }) {
  if (!invite.active) return { label: '已停用', type: 'info' as const }
  if (inviteExpired(invite)) return { label: '已到期', type: 'danger' as const }
  return { label: '使用中', type: 'success' as const }
}

function memberStatus(status: MiniProgramMemberStatus) {
  const labels: Record<MiniProgramMemberStatus, string> = {
    pending: '待审核', active: '使用中', stopped: '已停用', rejected: '已驳回',
  }
  const types: Record<MiniProgramMemberStatus, 'warning' | 'success' | 'info' | 'danger'> = {
    pending: 'warning', active: 'success', stopped: 'info', rejected: 'danger',
  }
  return { label: labels[status], type: types[status] }
}

function publicationStatus(row: PublicationRow) {
  if (live && !cloudStore.ready) return { label: '状态未确认', type: 'warning' as const }
  if (!row.publication) return { label: '未公开', type: 'info' as const }
  if (row.publication.published === false) return { label: '已取消公开', type: 'info' as const }
  return { label: '已公开', type: 'success' as const }
}

async function loadPapers() {
  if (papersLoading.value) return
  papersLoading.value = true
  try {
    await Promise.all([refreshCloud(), (async () => {
      try {
        const items = new Map<string, PaperSummary>()
        for (let page = 1; ; page++) {
          const result = await backend.listPapers({ keyword: '', status: 'saved', page, pageSize: 500 })
          result.items.forEach(paper => items.set(paper.id, paper))
          if (!result.items.length || page * result.pageSize >= result.total) break
        }
        papers.value = [...items.values()]
      } catch (reason) {
        papers.value = []
        ElMessage.error(errorMessage(reason, '读取本机试卷失败'))
      }
    })()])
  } finally {
    papersLoading.value = false
  }
}

function ensureService() {
  if (serviceAvailable.value) return true
  ElMessage.warning(`当前服务状态为“${serviceStatus.value.label}”，请联系管理员确认授权。`)
  activeSection.value = 'plan'
  return false
}

async function withdrawRow(row: PublicationRow) {
  if (!publicationReady.value || !publicationActions(row).withdraw) return
  const account = cloudStore.user?.id
  try {
    await ElMessageBox.confirm(`取消公开后，学员端将不能继续打开“${row.title}”。`, '取消公开试卷', {
      type: 'warning', confirmButtonText: '取消公开', cancelButtonText: '保留公开',
    })
  } catch { return }
  if (account !== cloudStore.user?.id || !publicationReady.value) return
  await perform(() => miniStore.withdrawPaper(row.id), '已取消公开')
}

async function publishRow(row: PublicationRow) {
  if (!publicationReady.value || !row.local || !publicationActions(row).publish || !ensureService()) return
  const local = row.local
  const success = await perform(() => live ? cloudStore.publishPaper(local.id, local.rowVersion)
    : useMiniProgramStore().publishPaper(local.id, local.rowVersion, local), row.publication?.published !== false && row.publication ? '已用最新版本更新公开内容' : '已公开整份试卷')
  if (success && paperFilter.value === 'withdrawn') await loadWithdrawn()
}

async function openLearnerPreview(summary: PaperSummary) {
  previewLoading.value = true
  previewOpen.value = true
  try {
    previewPaper.value = await backend.getPaper(summary.id)
  } catch (reason) {
    previewPaper.value = null
    ElMessage.error(errorMessage(reason, '读取学员端预览失败'))
  } finally {
    previewLoading.value = false
  }
}

function openInviteDialog() {
  inviteForm.label = ''
  inviteForm.validDays = 30
  inviteDialogOpen.value = true
}

async function saveInvite() {
  if (!ensureService()) return
  await perform(async () => {
    await miniStore.createInvite(inviteForm)
    inviteDialogOpen.value = false
  }, '邀请码已创建，请复制保存')
}

async function copyInvite(code: string) {
  try {
    await navigator.clipboard.writeText(code)
    ElMessage.success('邀请码已复制')
  } catch {
    ElMessage.info(`邀请码：${code}`)
  }
}

async function stopInvite(id: string) {
  try {
    await ElMessageBox.confirm('停用后，新学员不能再使用此邀请码；已加入成员不受影响。', '停用邀请码', {
      type: 'warning', confirmButtonText: '停用', cancelButtonText: '取消',
    })
  } catch { return }
  await perform(() => miniStore.stopInvite(id), '邀请码已停用')
}

async function rotateInvite(id: string) {
  try {
    await ElMessageBox.confirm('换新码会立即让旧邀请码失效，已加入成员不受影响。', '更换邀请码', {
      type: 'warning', confirmButtonText: '换新码', cancelButtonText: '取消',
    })
  } catch { return }
  await perform(() => miniStore.rotateInvite(id, 30), '已更换邀请码，请复制保存')
}

function openMemberDialog() {
  memberForm.name = ''
  memberForm.account = ''
  memberForm.inviteId = miniStore.invites.find((invite) => invite.active && !inviteExpired(invite))?.id ?? ''
  memberDialogOpen.value = true
}

function saveMember() {
  if (!miniStore.invites.some((invite) => invite.id === memberForm.inviteId && invite.active && !inviteExpired(invite))) {
    ElMessage.warning('请选择有效邀请码。')
    return
  }
  miniStore.addMember(memberForm)
  memberDialogOpen.value = false
  memberFilter.value = 'pending'
  activeSection.value = 'members'
  ElMessage.success('已登记成员申请，等待审核')
}

async function approveMember(id: string) {
  try {
    if (await miniStore.approveMember(id)) ElMessage.success('成员已通过，名额已计入本周期')
  } catch (reason) {
    ElMessage.warning(errorMessage(reason, '成员未能通过审核'))
  }
}

async function rejectMember(id: string) {
  try {
    await ElMessageBox.confirm('驳回后，本次申请不会计入名额。', '驳回申请', {
      type: 'warning', confirmButtonText: '确认驳回', cancelButtonText: '取消',
    })
  } catch { return }
  await perform(() => miniStore.rejectMember(id), '申请已驳回')
}

async function stopMember(id: string) {
  try {
    await ElMessageBox.confirm('停用后成员暂时不能访问学员端，但已计入的名额不会释放。', '停用成员', {
      type: 'warning', confirmButtonText: '停用', cancelButtonText: '取消',
    })
  } catch { return }
  await perform(() => miniStore.stopMember(id), '成员已停用，名额保持计入')
}

async function restoreMember(id: string) {
  await perform(() => miniStore.restoreMember(id), '成员已恢复')
}

onMounted(() => {
  void loadPapers()
})
</script>

<template>
  <div class="app-page mini-program-page">
    <aside class="page-sidebar mini-sidebar">
      <div class="sidebar-title">小程序管理</div>
      <nav class="sidebar-menu" aria-label="小程序管理模块">
        <button
          v-for="item in sections"
          :key="item.id"
          class="sidebar-menu__item"
          :class="{ 'is-active': activeSection === item.id }"
          type="button"
          @click="activeSection = item.id"
        >
          <el-icon><component :is="item.icon" /></el-icon>
          <span>{{ item.label }}</span>
          <el-badge v-if="item.id === 'members' && pendingCount" :value="pendingCount" :max="99" />
        </button>
      </nav>
      <div class="mini-sidebar__note">
        <template v-if="live"><strong>{{ cloudStore.user?.username || '小程序管理账号' }}</strong><div v-if="cloudStore.user"><el-button link :disabled="cloudStore.busy" @click="refreshCloud">刷新状态</el-button><el-button link :disabled="cloudStore.busy" @click="signOut">退出登录</el-button></div></template>
        <strong v-else>本地界面预览</strong>
      </div>
    </aside>

    <main class="page-main mini-main" v-loading="live && cloudStore.busy">
      <div v-if="cloudError" class="mini-cloud-error" role="alert">{{ cloudError }}<el-button v-if="cloudStore.user" link @click="refreshCloud">重试</el-button></div>
      <section v-if="live && !cloudStore.user" class="surface mini-account">
        <h3>{{ authForm.register ? '注册题库拥有者账号' : '登录题库拥有者账号' }}</h3>
        <el-form label-position="top" @submit.prevent="connectAccount">
          <el-form-item label="用户名"><el-input v-model="authForm.account" autocomplete="username" maxlength="64" /></el-form-item>
          <el-form-item label="密码"><el-input v-model="authForm.password" type="password" show-password :autocomplete="authForm.register ? 'new-password' : 'current-password'" maxlength="128" /></el-form-item>
          <el-button type="primary" native-type="submit" :loading="authBusy">{{ authForm.register ? '注册并登录' : '登录' }}</el-button>
          <el-button link :disabled="authBusy" @click="authForm.register = !authForm.register">{{ authForm.register ? '已有账号，去登录' : '注册账号' }}</el-button>
        </el-form>
        <el-button class="mini-contact-entry" @click="supportOpen = true">联系开通</el-button>
      </section>
      <section v-else-if="activeSection === 'papers'" class="mini-section">
        <div class="surface mini-table-wrap mini-table-card">
          <div class="mini-table-toolbar" role="group" aria-label="试卷操作">
            <el-radio-group v-model="paperFilter" class="mini-paper-filter" size="small" aria-label="试卷范围">
              <el-radio-button label="current">当前试卷</el-radio-button>
              <el-radio-button label="withdrawn">已取消公开</el-radio-button>
            </el-radio-group>
            <el-button :icon="Refresh" :loading="paperListLoading" :disabled="live && cloudStore.busy" @click="loadPapers">刷新试卷</el-button>
            <el-input v-model="paperKeyword" class="mini-paper-search" clearable maxlength="200" placeholder="搜索试卷名称" aria-label="搜索试卷名称" />
          </div>
          <div v-if="paperFilter === 'withdrawn' && withdrawnError" class="mini-cloud-error" role="alert">{{ withdrawnError }}<el-button link @click="loadWithdrawn">重试</el-button></div>
          <div class="mini-table-body" v-loading="paperListLoading">
          <el-table :data="filteredPapers" row-key="id" height="100%">
            <el-table-column label="试卷名称" min-width="260">
              <template #default="{ row }">
                <strong>{{ row.title }}</strong>
                <span class="mini-table-sub">{{ row.local ? row.local.subjectSummaryText || '本机试卷' : '本机无原稿' }}</span>
                <span v-if="row.local && row.local.title !== row.title" class="mini-table-sub">本机名称：{{ row.local.title }}</span>
              </template>
            </el-table-column>
            <el-table-column label="题目数" width="90"><template #default="{ row }">{{ row.questionCount === null ? '—' : `${row.questionCount} 题` }}</template></el-table-column>
            <el-table-column label="版本" width="105"><template #default="{ row }">v{{ row.publication?.paperRowVersion ?? row.local?.rowVersion }}<span v-if="row.local && row.publication && row.local.rowVersion !== row.publication.paperRowVersion" class="mini-table-sub">本机 v{{ row.local.rowVersion }}</span></template></el-table-column>
            <el-table-column label="公开状态" width="110">
              <template #default="{ row }"><el-tag size="small" :type="publicationStatus(row).type">{{ publicationStatus(row).label }}</el-tag><span v-if="publicationActions(row).newerLocal" class="mini-table-sub">本机有更新</span><span v-else-if="publicationActions(row).olderLocal" class="mini-table-sub">本机版本较旧</span></template>
            </el-table-column>
            <el-table-column label="更新时间" width="130"><template #default="{ row }">{{ formatDate(row.updatedAt) }}</template></el-table-column>
            <el-table-column label="操作" width="270" fixed="right">
              <template #default="{ row }">
                <el-button v-if="row.local" link type="primary" :icon="View" @click="openLearnerPreview(row.local)">本机预览</el-button>
                <el-button v-if="publicationActions(row).publish" link type="primary" :disabled="!publicationReady || paperListLoading" @click="publishRow(row)">{{ publicationActions(row).withdraw ? '更新公开' : '公开试卷' }}</el-button>
                <el-button v-if="publicationActions(row).withdraw" link type="danger" :disabled="!publicationReady || paperListLoading" @click="withdrawRow(row)">取消公开</el-button>
                <span v-if="!row.local && !publicationActions(row).withdraw" class="mini-muted">—</span>
              </template>
            </el-table-column>
            <template #empty><el-empty :description="withdrawnError && paperFilter === 'withdrawn' ? '记录读取失败，请重试' : paperKeyword ? '没有匹配的试卷' : paperFilter === 'withdrawn' ? '暂无已取消公开记录' : '暂无本机已保存试卷或云端已公开试卷'" :image-size="72" /></template>
          </el-table>
          </div>
          <div v-if="paperTotal > paperPageSize" class="mini-pagination"><el-pagination v-model:current-page="paperPage" :page-size="paperPageSize" :total="paperTotal" layout="total, prev, pager, next" /></div>
        </div>
      </section>

      <section v-else-if="activeSection === 'invites'" class="mini-section">
        <div class="surface mini-table-wrap mini-table-card">
          <div class="mini-table-toolbar" role="group" aria-label="邀请码操作">
            <el-button type="primary" :icon="Plus" @click="openInviteDialog">新建邀请码</el-button>
          </div>
          <div class="mini-table-body">
          <el-table :data="filteredInvites" row-key="id" height="100%">
            <el-table-column label="邀请码" min-width="180">
              <template #default="{ row }"><strong class="mini-code">{{ row.code || '仅创建时显示，请换新码' }}</strong><span class="mini-table-sub">{{ row.label }}</span></template>
            </el-table-column>
            <el-table-column label="到期日期" width="135"><template #default="{ row }">{{ formatDate(row.expiresAt) }}</template></el-table-column>
            <el-table-column label="状态" width="105"><template #default="{ row }"><el-tag size="small" :type="inviteStatus(row).type">{{ inviteStatus(row).label }}</el-tag></template></el-table-column>
            <el-table-column label="操作" width="245" fixed="right">
              <template #default="{ row }">
                <el-button :disabled="!row.code" link type="primary" :icon="CopyDocument" @click="copyInvite(row.code)">复制</el-button>
                <el-button v-if="row.active" link type="primary" @click="rotateInvite(row.id)">换新码</el-button>
                <el-button v-if="row.active" link type="danger" :icon="SwitchButton" @click="stopInvite(row.id)">停用</el-button>
              </template>
            </el-table-column>
            <template #empty><el-empty description="还没有邀请码" :image-size="72" /></template>
          </el-table>
          </div>
        </div>
      </section>

      <section v-else-if="activeSection === 'members'" class="mini-section">
        <div class="surface mini-table-wrap mini-table-card">
          <div class="mini-table-toolbar mini-table-toolbar--members" role="group" aria-label="成员筛选与操作">
            <el-radio-group v-model="memberFilter" size="small">
              <el-radio-button label="pending">待审核 {{ pendingCount }}</el-radio-button>
              <el-radio-button label="active">使用中</el-radio-button>
              <el-radio-button label="stopped">已停用</el-radio-button>
              <el-radio-button label="all">全部</el-radio-button>
            </el-radio-group>
            <el-button v-if="!live" type="primary" :icon="Plus" @click="openMemberDialog">登记成员</el-button><el-button v-else :icon="Refresh" @click="refreshCloud">刷新成员</el-button>
          </div>
          <div class="mini-table-body">
          <el-table :data="filteredMembers" row-key="id" height="100%">
            <el-table-column label="成员" min-width="220"><template #default="{ row }"><strong>{{ row.name }}</strong><span class="mini-table-sub">{{ row.account }}</span></template></el-table-column>
            <el-table-column label="申请时间" width="145"><template #default="{ row }">{{ formatDateTime(row.requestedAt) }}</template></el-table-column>
            <el-table-column label="状态" width="105"><template #default="{ row }"><el-tag size="small" :type="memberStatus(row.status).type">{{ memberStatus(row.status).label }}</el-tag></template></el-table-column>
            <el-table-column label="操作" width="230" fixed="right">
              <template #default="{ row }">
                <template v-if="row.status === 'pending'"><el-button link type="primary" :icon="Check" @click="approveMember(row.id)">通过</el-button><el-button link type="danger" :icon="Delete" @click="rejectMember(row.id)">驳回</el-button></template>
                <el-button v-else-if="row.status === 'active'" link type="danger" @click="stopMember(row.id)">停用</el-button>
                <el-button v-else-if="row.status === 'stopped'" link type="primary" @click="restoreMember(row.id)">恢复</el-button>
                <span v-else class="mini-muted">无需操作</span>
              </template>
            </el-table-column>
            <template #empty><el-empty description="当前筛选下没有成员记录" :image-size="72" /></template>
          </el-table>
          </div>
        </div>
      </section>

      <section v-else class="mini-section">
        <div class="mini-plan-grid">
          <section class="surface mini-plan-details">
            <h3>当前服务周期</h3>
            <dl class="mini-plan-values">
              <div><dt>授权名额</dt><dd>{{ miniStore.settings.quota }} 位</dd></div>
              <div><dt>开始日期</dt><dd>{{ miniStore.settings.serviceStarts || '—' }}</dd></div>
              <div><dt>到期日期</dt><dd>{{ miniStore.settings.serviceEnds || '—' }}</dd></div>
              <div><dt>服务状态</dt><dd><el-tag :type="serviceStatus.type">{{ serviceStatus.label }}</el-tag></dd></div>
            </dl>
            <el-button type="primary" plain class="mini-contact-entry" @click="supportOpen = true">开通 / 续费</el-button>
          </section>
          <section class="surface mini-quota-card">
            <div class="mini-quota-card__head"><div><span class="mini-caption">本周期已使用</span><strong>{{ miniStore.usedQuota }} <small>/ {{ miniStore.settings.quota }} 位</small></strong></div><el-tag :type="serviceStatus.type">{{ serviceStatus.label }}</el-tag></div>
            <el-progress :percentage="quotaPercent" :stroke-width="12" :color="quotaPercent >= 100 ? '#dc2626' : '#2563eb'" />
            <div class="mini-quota-card__foot"><span>剩余 {{ miniStore.remainingQuota }} 位</span><span>{{ servicePeriodLabel }}</span></div>
            <p>名额按学员账号去重。同一学员跨邀请码重复申请只占一个名额；停用或恢复不会改变已使用数量。</p>
          </section>
        </div>
        <div class="surface mini-table-wrap mini-ledger">
          <div class="mini-ledger__head"><h3>授权记录</h3><span>共 {{ countedMembers.length }} 条计入记录</span></div>
          <el-table :data="countedMembers" row-key="id" height="100%">
            <el-table-column label="成员" min-width="220"><template #default="{ row }"><strong>{{ row.name }}</strong><span class="mini-table-sub">{{ row.account }}</span></template></el-table-column>
            <el-table-column label="通过时间" width="155"><template #default="{ row }">{{ formatDateTime(row.joinedAt) }}</template></el-table-column>
            <el-table-column label="状态" width="105"><template #default="{ row }"><el-tag size="small" :type="memberStatus(row.status).type">{{ memberStatus(row.status).label }}</el-tag></template></el-table-column>
          </el-table>
        </div>
      </section>
    </main>

    <SupportContactDialog v-model="supportOpen" :account="cloudStore.user?.username || ''" :live="live" :refreshing="supportRefreshing" @refresh="refreshAuthorization" />

    <el-dialog v-model="inviteDialogOpen" title="新建邀请码" width="480px">
      <el-form label-position="top" @submit.prevent="saveInvite">
        <el-form-item label="备注"><el-input v-model="inviteForm.label" maxlength="30" placeholder="例如：秋季一班" /></el-form-item>
        <el-form-item label="有效期"><el-select v-model="inviteForm.validDays" style="width: 100%"><el-option :value="7" label="7 天" /><el-option :value="30" label="30 天" /><el-option :value="90" label="90 天" /></el-select></el-form-item>
      </el-form>
      <template #footer><el-button @click="inviteDialogOpen = false">取消</el-button><el-button type="primary" @click="saveInvite">创建邀请码</el-button></template>
    </el-dialog>

    <el-dialog v-model="memberDialogOpen" title="登记成员申请" width="480px">
      <el-form label-position="top">
        <el-form-item label="成员姓名"><el-input v-model="memberForm.name" maxlength="40" placeholder="例如：张同学" /></el-form-item>
        <el-form-item label="成员账号"><el-input v-model="memberForm.account" maxlength="120" placeholder="小程序登录账号或编号" /></el-form-item>
        <el-form-item label="申请来源"><el-select v-model="memberForm.inviteId" placeholder="选择邀请码" style="width: 100%"><el-option v-for="invite in miniStore.invites.filter((item) => item.active && !inviteExpired(item))" :key="invite.id" :label="`${invite.code} · ${invite.label}`" :value="invite.id" /></el-select></el-form-item>
      </el-form>
      <template #footer><el-button @click="memberDialogOpen = false">取消</el-button><el-button type="primary" @click="saveMember">登记并待审核</el-button></template>
    </el-dialog>

    <el-dialog v-model="previewOpen" title="学员端预览" width="760px">
      <div v-loading="previewLoading" class="learner-preview">
        <template v-if="previewPaper">
          <div class="learner-preview__head"><div><h2>{{ previewPaper.title }}</h2><p>{{ previewPaper.subjectSummaryText || '未分类' }} · {{ previewPaper.items.length }} 题</p></div><el-tag type="success">整份试卷</el-tag></div>
          <ol class="learner-preview__list"><li v-for="item in previewPaper.items.slice(0, 12)" :key="item.id"><QuestionStemSummary :content="item.snapshot.stem" :lines="2" /><span>{{ item.snapshot.subjectName }} · {{ item.snapshot.chapterName }}</span></li></ol>
          <p v-if="previewPaper.items.length > 12" class="mini-muted">仅展示前 12 题预览，实际公开内容包含整份试卷。</p>
        </template>
        <el-empty v-else-if="!previewLoading" description="试卷内容暂时无法读取" />
      </div>
      <template #footer><el-button @click="previewOpen = false">关闭预览</el-button></template>
    </el-dialog>
  </div>
</template>

<style scoped>
.mini-account { width: min(460px, 100%); padding:28px; margin:30px auto; }
.mini-account h3 { margin:0 0 24px; }
.mini-cloud-error { color:#b42318; background:#fff1f0; padding:10px 16px; margin-bottom:12px; }
.mini-program-page { min-height: 0; }
.mini-sidebar { display: flex; flex-direction: column; }
.mini-sidebar .sidebar-menu__item { font-size: 12px; }
.mini-sidebar .el-icon { margin-right: 2px; font-size: 15px; }
.mini-sidebar .el-badge { margin-left: auto; }
.mini-sidebar__note { margin: auto 16px 18px; padding: 12px; border: 1px solid #dbeafe; border-radius: 8px; background: #f8fbff; color: #64748b; font-size: 11px; line-height: 1.7; }
.mini-sidebar__note strong { color: #1e40af; font-size: 12px; }
.mini-sidebar__note p { margin: 6px 0 0; }
.mini-main { display: flex; flex-direction: column; overflow: hidden; }
.mini-caption { color: #94a3b8; font-size: 10px; }
.mini-section { min-height: 0; display: flex; flex-direction: column; flex: 1; }
.mini-table-wrap { min-height: 0; flex: 1; overflow: hidden; }
.mini-table-card { display: flex; flex-direction: column; }
.mini-table-toolbar { display: flex; align-items: center; justify-content: flex-end; flex-wrap: wrap; gap: 10px; padding: 12px 16px; border-bottom: 1px solid #edf1f5; flex: 0 0 auto; }
.mini-table-toolbar--members { justify-content: space-between; }
.mini-paper-search { width: 230px; max-width: 100%; }
.mini-paper-filter { margin-right: auto; }
.mini-pagination { padding: 10px 16px; border-top: 1px solid #edf1f5; display: flex; justify-content: flex-end; flex-shrink: 0; }
.mini-table-body { flex: 1; min-height: 0; overflow: hidden; }
.mini-table-wrap :deep(.el-table) { height: 100%; }
.mini-table-sub { display: block; margin-top: 3px; color: #94a3b8; font-size: 10px; font-weight: 400; }
.mini-code { color: #1d4ed8; letter-spacing: .08em; font-variant-numeric: tabular-nums; }
.mini-muted { color: #94a3b8; font-size: 11px; }
.mini-plan-grid { display: grid; grid-template-columns: minmax(300px, 0.75fr) minmax(360px, 1.25fr); gap: 14px; margin-bottom: 14px; flex: 0 0 auto; }
.mini-plan-details, .mini-quota-card { padding: 18px; }
.mini-plan-details h3 { margin: 0 0 16px; color: #1f2937; font-size: 14px; }
.mini-plan-values { margin: 0; display: grid; gap: 16px; font-size: 12px; }
.mini-plan-values > div { display: flex; align-items: center; justify-content: space-between; gap: 16px; }
.mini-plan-values dt { color: #64748b; }
.mini-plan-values dd { margin: 0; color: #1f2937; font-weight: 500; }
.mini-contact-entry { margin-top: 20px; }
.mini-quota-card { display: flex; flex-direction: column; justify-content: center; gap: 18px; }
.mini-quota-card__head { display: flex; align-items: flex-start; justify-content: space-between; gap: 10px; }
.mini-quota-card__head strong { display: block; margin-top: 4px; color: #111827; font-size: 30px; line-height: 1.1; font-weight: 700; font-variant-numeric: tabular-nums; }
.mini-quota-card__head small { color: #64748b; font-size: 13px; font-weight: 500; }
.mini-quota-card__foot { display: flex; justify-content: space-between; gap: 12px; color: #64748b; font-size: 11px; }
.mini-quota-card p { margin: 0; color: #64748b; font-size: 11px; line-height: 1.7; }
.mini-ledger { min-height: 230px; }
.mini-ledger__head { height: 48px; padding: 0 16px; display: flex; align-items: center; justify-content: space-between; border-bottom: 1px solid #edf1f5; }
.mini-ledger__head h3 { margin: 0; color: #1f2937; font-size: 14px; }
.mini-ledger__head span { color: #94a3b8; font-size: 11px; }
.learner-preview { min-height: 240px; }
.learner-preview__head { display: flex; align-items: flex-start; justify-content: space-between; gap: 12px; margin-bottom: 14px; }
.learner-preview__head h2 { margin: 0; color: #111827; font-size: 18px; }
.learner-preview__head p { margin: 5px 0 0; color: #64748b; font-size: 12px; }
.learner-preview__list { margin: 14px 0 0; padding: 0 0 0 28px; color: #334155; }
.learner-preview__list li { padding: 10px 8px; border-bottom: 1px solid #edf1f5; }
.learner-preview__list li > span { display: block; margin-top: 4px; color: #94a3b8; font-size: 10px; }
@media (max-width: 1050px) {
  .mini-plan-grid { grid-template-columns: 1fr; }
}
</style>
