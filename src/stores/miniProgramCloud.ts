import { computed, reactive, ref } from 'vue'
import { defineStore } from 'pinia'
import { miniCloud } from '../services/miniProgramCloud'
import { backend } from '../services/backend'
import { miniProgramPaper } from '../utils/miniProgramPaper'
import { createMediaPreparationCache } from '../utils/mediaCache'
import type { MediaProgress } from '../utils/miniProgramMedia'
import { useAppStore } from './app'
import type { PageResult } from '../types/domain'
import type { MiniProgramInvite, MiniProgramMember, MiniProgramPublication, MiniProgramSettings } from '../types/miniProgram'

interface CloudState {
  capabilities?: { pagedManagement?: boolean; mediaCache?: boolean }
  counts?: { members: number; invites: number; pending: number; countedMembers: number }
  bankId: string
  title: string
  settings: (MiniProgramSettings & { usedQuota: number }) | null
  publications: (MiniProgramPublication & { published: boolean })[]
  invites: Omit<MiniProgramInvite, 'code'>[]
  members: (MiniProgramMember & { countedInCycle: boolean })[]
}
export interface ManagementQuery { keyword: string; status: string; page: number; pageSize: number; countedOnly?: boolean }
type ManagementSection = 'members' | 'invites' | 'counted'
const emptySettings = (): MiniProgramSettings => ({ quota: 0, serviceStarts: '', serviceEnds: '', paused: false })

export const useMiniProgramCloudStore = defineStore('miniProgramCloud', () => {
  const user = ref<{ id: string; username: string } | null>(null)
  const title = ref('我的题库')
  const settings = ref(emptySettings())
  const publications = ref<MiniProgramPublication[]>([])
  const invites = ref<MiniProgramInvite[]>([])
  const members = ref<CloudState['members']>([])
  const countedPage = ref<CloudState['members']>([])
  const managementMode = ref<'unknown' | 'paged' | 'legacy'>('unknown')
  const pendingCount = ref(0)
  const totals = reactive({ members: 0, invites: 0, counted: 0 })
  const managementLoading = reactive({ members: false, invites: false, counted: false })
  const queries: Record<ManagementSection, ManagementQuery | null> = { members: null, invites: null, counted: null }
  const sequences = { members: 0, invites: 0, counted: 0 }
  let legacyInvites: CloudState['invites'] = [], legacyMembers: CloudState['members'] = []
  const usedQuota = ref(0), busy = ref(false), ready = ref(false)
  let accountGeneration = 0, refreshGeneration = 0
  let mediaCache = createMediaPreparationCache()
  let supportsMediaCache = false
  const publicationProgress = ref<(Omit<MediaProgress, 'phase'> & { phase: MediaProgress['phase'] | 'submitting' }) | null>(null)
  let publicationController: AbortController | null = null
  function cancelPublication() {
    if (publicationProgress.value?.phase !== 'submitting') publicationController?.abort()
  }
  // Invite plaintext is shown during this desktop session only; server stores its hash.
  const codes = new Map<string, string>()
  const countedMembers = computed(() => countedPage.value)
  const remainingQuota = computed(() => Math.max(0, settings.value.quota - usedQuota.value))
  function reset() {
    publicationController?.abort(); publicationController = null; publicationProgress.value = null
    accountGeneration++; refreshGeneration++
    mediaCache = createMediaPreparationCache(); supportsMediaCache = false
    managementMode.value = 'unknown'; pendingCount.value = 0; countedPage.value = []
    legacyInvites = []; legacyMembers = []; busy.value = false
    for (const key of ['members', 'invites', 'counted'] as const) {
      sequences[key]++; queries[key] = null; totals[key] = 0; managementLoading[key] = false
    }
    settings.value = emptySettings(); publications.value = []; invites.value = []; members.value = []
    usedQuota.value = 0; title.value = '我的题库'; codes.clear(); ready.value = false
  }
  async function readState(omitPublications = false) {
    if (managementMode.value === 'legacy') return miniCloud.request<CloudState>('GET', '/state?activePublications=true')
    try {
      return await miniCloud.request<CloudState>('GET', '/management/state' + (omitPublications ? '?omitPublications=true' : ''))
    } catch (reason) {
      if (managementMode.value !== 'unknown' || !String(reason).startsWith('MINI_PAGED_MANAGEMENT_UNAVAILABLE:')) throw reason
      return miniCloud.request<CloudState>('GET', '/state?activePublications=true')
    }
  }
  async function refresh(affected?: 'members' | 'invites' | 'publications') {
    if (!user.value) throw new Error('请先登录小程序管理账号')
    const generation = accountGeneration, request = ++refreshGeneration
    try {
      const omitPublications = !!affected && affected !== 'publications' && managementMode.value === 'paged'
      const result = await readState(omitPublications)
      if (generation !== accountGeneration || request !== refreshGeneration) return
      managementMode.value = result.capabilities?.pagedManagement ? 'paged' : 'legacy'
      title.value = result.title; settings.value = result.settings ?? emptySettings()
      supportsMediaCache = result.capabilities?.mediaCache === true
      usedQuota.value = result.settings?.usedQuota ?? 0
      if (!omitPublications) publications.value = result.publications.filter(p => p.published)
      pendingCount.value = result.counts?.pending ?? result.members.filter(m => m.status === 'pending').length
      if (managementMode.value === 'legacy') {
        legacyInvites = result.invites; legacyMembers = result.members
        invites.value = result.invites.map(i => ({ ...i, code: codes.get(i.id) ?? '' }))
        members.value = result.members; countedPage.value = result.members.filter(m => m.countedInCycle)
      }
      const keys: ManagementSection[] = affected === 'members' ? ['members', 'counted'] : affected === 'invites' ? ['invites'] : affected === 'publications' ? [] : ['members', 'invites', 'counted']
      for (const key of keys) {
        if (queries[key]) await loadManagement(key, queries[key]!)
        if (generation !== accountGeneration || request !== refreshGeneration) return
      }
      ready.value = true
    } catch (reason) {
      if (generation !== accountGeneration || request !== refreshGeneration) return
      ready.value = false
      throw reason
    }
  }
  async function loadManagement(section: ManagementSection, query: ManagementQuery): Promise<PageResult<MiniProgramInvite | CloudState['members'][number]> | undefined> {
    if (!user.value) throw new Error('请先登录小程序管理账号')
    const generation = accountGeneration, request = ++sequences[section]
    queries[section] = { ...query }; managementLoading[section] = true
    try {
      let result: PageResult<MiniProgramInvite | CloudState['members'][number]>
      if (managementMode.value === 'legacy') {
        const needle = query.keyword.trim().toLocaleLowerCase()
        const rows = section === 'invites'
          ? legacyInvites.filter(i => i.label.toLocaleLowerCase().includes(needle) && (query.status === 'all' || (query.status === 'active' && i.active && i.expiresAt > Date.now()) || (query.status === 'expired' && i.active && i.expiresAt <= Date.now()) || (query.status === 'stopped' && !i.active))).map(i => ({ ...i, code: codes.get(i.id) ?? '' }))
          : legacyMembers.filter(m => (query.status === 'all' || m.status === query.status) && (!query.countedOnly || m.countedInCycle) && (m.name.toLocaleLowerCase().includes(needle) || m.account.toLocaleLowerCase().includes(needle)))
        rows.sort((a, b) => ('requestedAt' in b ? b.requestedAt : b.createdAt) - ('requestedAt' in a ? a.requestedAt : a.createdAt) || b.id.localeCompare(a.id))
        result = { items: rows.slice((query.page - 1) * query.pageSize, query.page * query.pageSize), total: rows.length, page: query.page, pageSize: query.pageSize }
      } else {
        result = await miniCloud.request('POST', section === 'invites' ? '/invites/search' : '/members/search', query)
      }
      if (generation !== accountGeneration || request !== sequences[section]) return
      const lastPage = Math.max(1, Math.ceil(result.total / query.pageSize))
      if (query.page > lastPage) return await loadManagement(section, { ...query, page: lastPage })
      totals[section] = result.total
      if (section === 'invites') invites.value = (result.items as MiniProgramInvite[]).map(i => ({ ...i, code: codes.get(i.id) ?? '' }))
      else if (section === 'counted') countedPage.value = result.items as CloudState['members']
      else members.value = result.items as CloudState['members']
      return result
    } catch (reason) {
      if (generation !== accountGeneration || request !== sequences[section]) return
      if (section === 'invites') invites.value = []
      else if (section === 'counted') countedPage.value = []
      else members.value = []
      totals[section] = 0
      throw reason
    } finally { if (generation === accountGeneration && request === sequences[section]) managementLoading[section] = false }
  }
  function invalidateManagement(section: ManagementSection) {
    sequences[section]++; managementLoading[section] = false
    if (section === 'members') members.value = []
    else if (section === 'invites') invites.value = []
    else countedPage.value = []
  }
  async function searchWithdrawn(keyword: string, page: number, pageSize: number) {
    if (!user.value) throw new Error('请先登录小程序管理账号')
    const generation = accountGeneration
    const result = await miniCloud.request<PageResult<MiniProgramPublication>>('POST', '/publications/search', { published: false, keyword, page, pageSize })
    if (generation !== accountGeneration) throw new Error('账号已切换，请重新查询')
    return result
  }
  async function login(account: string, password: string, register = false) {
    user.value = await miniCloud.login(account.trim(), password, register)
    reset(); await refresh()
  }
  async function logout() {
    user.value = null; reset()
    await miniCloud.logout()
  }
  async function change<T>(action: () => Promise<T>, affected: 'members' | 'invites' | 'publications') {
    if (!user.value) throw new Error('请先登录小程序管理账号')
    if (!ready.value) throw new Error('云端状态未确认，请先刷新状态')
    if (busy.value) throw new Error('上一项操作尚未完成')
    busy.value = true
    const generation = accountGeneration
    try {
      const value = await action()
      if (generation !== accountGeneration) throw new Error('账号已切换，请重新查询操作结果')
      try { await refresh(affected) } catch { if (generation === accountGeneration) ready.value = false; throw new Error('操作已提交，但列表刷新失败，请刷新确认结果') }
      return value
    } finally { if (generation === accountGeneration) busy.value = false }
  }
  function publicationFor(id: string) { return publications.value.find(p => p.paperId === id) ?? null }
  async function publishPaper(id: string, version: number) {
    return change(async () => {
      const generation = accountGeneration
      const controller = new AbortController()
      publicationController = controller
      publicationProgress.value = { completed: 0, total: 0, phase: 'preparing' }
      const ensureAccount = () => {
        if (generation !== accountGeneration) throw new Error('账号已切换，请重新发布')
        if (controller.signal.aborted) throw new Error('已取消发布')
      }
      try {
        const paper = await backend.getPaper(id)
        ensureAccount()
        if (!paper || paper.rowVersion !== version) throw new Error('试卷已变更，请刷新列表后重试')
        miniProgramPaper(paper, useAppStore().questionTypes, undefined, true)
        const { prepareMedia } = await import('../utils/miniProgramMedia')
        const media = await prepareMedia(paper, {
          getImage: backend.getManagedImage,
          upload: image => { ensureAccount(); return miniCloud.request('POST', '/assets', image) },
          cache: mediaCache,
          signal: controller.signal,
          onProgress: progress => { if (generation === accountGeneration) publicationProgress.value = progress },
          checkAssets: supportsMediaCache ? ids => { ensureAccount(); return miniCloud.request('POST', '/assets/check', { ids }) } : undefined,
        })
        ensureAccount()
        publicationProgress.value = { ...publicationProgress.value!, phase: 'submitting' }
        return await miniCloud.request('PUT', '/publications/' + id, { version, paper: miniProgramPaper(paper, useAppStore().questionTypes, media) })
      } finally {
        if (publicationController === controller) { publicationController = null; publicationProgress.value = null }
      }
    }, 'publications')
  }
  const withdrawPaper = (id: string) => change(() => miniCloud.request('DELETE', '/publications/' + id), 'publications')
  async function createInvite(input: { label: string; validDays: number }) {
    return change(async () => {
      const generation = accountGeneration
      const result = await miniCloud.request<MiniProgramInvite>('POST', '/invites', { label: input.label.trim() || '未命名邀请码', expiresAt: Date.now() + input.validDays * 86400000 })
      if (generation !== accountGeneration) throw new Error('账号已切换，请重新查询操作结果')
      codes.set(result.id, result.code); return result
    }, 'invites')
  }
  const stopInvite = (id: string) => change(() => miniCloud.request('DELETE', '/invites/' + id), 'invites')
  const rotateInvite = async (id: string, validDays: number) => change(async () => {
    const generation = accountGeneration
    const result = await miniCloud.request<MiniProgramInvite>('POST', '/invites/' + id + '/rotate', { expiresAt: Date.now() + validDays * 86400000 })
    if (generation !== accountGeneration) throw new Error('账号已切换，请重新查询操作结果')
    codes.set(result.id, result.code); return result
  }, 'invites')
  const status = (id: string, value: string) => change(async () => { await miniCloud.request('PUT', '/members/' + id, { status: value }); return true }, 'members')
  const removeMember = (id: string) => change(async () => { await miniCloud.request('DELETE', '/members/' + id); return true }, 'members')
  function addMember(_input: { name: string; account: string; inviteId: string }): never {
    throw new Error('成员须从微信小程序提交加入申请')
  }
  return {
    user, title, settings, publications, invites, members, countedMembers, usedQuota, remainingQuota, busy, ready,
    managementMode, pendingCount, totals, queries, managementLoading, loadManagement, invalidateManagement,
    publicationProgress, cancelPublication,
    login, logout, refresh, searchWithdrawn, publicationFor, publishPaper, withdrawPaper, createInvite, stopInvite, rotateInvite, addMember,
    approveMember: (id: string) => status(id, 'active'), rejectMember: (id: string) => status(id, 'rejected'),
    stopMember: (id: string) => status(id, 'stopped'), restoreMember: (id: string) => status(id, 'active'), removeMember,
  }
})
