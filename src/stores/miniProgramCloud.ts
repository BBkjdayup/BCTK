import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import { miniCloud } from '../services/miniProgramCloud'
import { backend } from '../services/backend'
import { miniProgramPaper } from '../utils/miniProgramPaper'
import { useAppStore } from './app'
import type { PageResult } from '../types/domain'
import type { MiniProgramInvite, MiniProgramMember, MiniProgramPublication, MiniProgramSettings } from '../types/miniProgram'

interface CloudState {
  bankId: string
  title: string
  settings: (MiniProgramSettings & { usedQuota: number }) | null
  publications: (MiniProgramPublication & { published: boolean })[]
  invites: Omit<MiniProgramInvite, 'code'>[]
  members: (MiniProgramMember & { countedInCycle: boolean })[]
}
const emptySettings = (): MiniProgramSettings => ({ quota: 0, serviceStarts: '', serviceEnds: '', paused: false })

export const useMiniProgramCloudStore = defineStore('miniProgramCloud', () => {
  const user = ref<{ id: string; username: string } | null>(null)
  const title = ref('我的题库')
  const settings = ref(emptySettings())
  const publications = ref<MiniProgramPublication[]>([])
  const invites = ref<MiniProgramInvite[]>([])
  const members = ref<CloudState['members']>([])
  const usedQuota = ref(0), busy = ref(false), ready = ref(false)
  let accountGeneration = 0, refreshGeneration = 0
  // Invite plaintext is shown during this desktop session only; server stores its hash.
  const codes = new Map<string, string>()
  const countedMembers = computed(() => members.value.filter(m => m.countedInCycle))
  const remainingQuota = computed(() => Math.max(0, settings.value.quota - usedQuota.value))
  function reset() {
    accountGeneration++; refreshGeneration++
    settings.value = emptySettings(); publications.value = []; invites.value = []; members.value = []
    usedQuota.value = 0; title.value = '我的题库'; codes.clear(); ready.value = false
  }
  async function refresh() {
    if (!user.value) throw new Error('请先登录小程序管理账号')
    const generation = accountGeneration, request = ++refreshGeneration
    try {
      const result = await miniCloud.request<CloudState>('GET', '/state?activePublications=true')
      if (generation !== accountGeneration || request !== refreshGeneration) return
      title.value = result.title; settings.value = result.settings ?? emptySettings()
      usedQuota.value = result.settings?.usedQuota ?? 0
      publications.value = result.publications.filter(p => p.published)
      invites.value = result.invites.map(i => ({ ...i, code: codes.get(i.id) ?? '' }))
      members.value = result.members; ready.value = true
    } catch (reason) {
      if (generation !== accountGeneration || request !== refreshGeneration) return
      ready.value = false
      throw reason
    }
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
  async function change<T>(action: () => Promise<T>) {
    if (!user.value) throw new Error('请先登录小程序管理账号')
    if (!ready.value) throw new Error('云端状态未确认，请先刷新状态')
    if (busy.value) throw new Error('上一项操作尚未完成')
    busy.value = true
    try {
      const value = await action()
      try { await refresh() } catch { ready.value = false; throw new Error('操作已提交，但列表刷新失败，请刷新确认结果') }
      return value
    } finally { busy.value = false }
  }
  function publicationFor(id: string) { return publications.value.find(p => p.paperId === id) ?? null }
  async function publishPaper(id: string, version: number) {
    return change(async () => {
      const paper = await backend.getPaper(id)
      if (!paper || paper.rowVersion !== version) throw new Error('试卷已变更，请刷新列表后重试')
      const { prepareMedia } = await import('../utils/miniProgramMedia')
      const media = await prepareMedia(paper, {
        getImage: backend.getManagedImage,
        upload: image => miniCloud.request('POST', '/assets', image),
      })
      return miniCloud.request('PUT', '/publications/' + id, { version, paper: miniProgramPaper(paper, useAppStore().questionTypes, media) })
    })
  }
  const withdrawPaper = (id: string) => change(() => miniCloud.request('DELETE', '/publications/' + id))
  async function createInvite(input: { label: string; validDays: number }) {
    return change(async () => {
      const result = await miniCloud.request<MiniProgramInvite>('POST', '/invites', { label: input.label.trim() || '未命名邀请码', expiresAt: Date.now() + input.validDays * 86400000 })
      codes.set(result.id, result.code); return result
    })
  }
  const stopInvite = (id: string) => change(() => miniCloud.request('DELETE', '/invites/' + id))
  const rotateInvite = async (id: string, validDays: number) => change(async () => {
    const result = await miniCloud.request<MiniProgramInvite>('POST', '/invites/' + id + '/rotate', { expiresAt: Date.now() + validDays * 86400000 })
    codes.set(result.id, result.code); return result
  })
  const status = (id: string, value: string) => change(async () => { await miniCloud.request('PUT', '/members/' + id, { status: value }); return true })
  function addMember(_input: { name: string; account: string; inviteId: string }): never {
    throw new Error('成员须从微信小程序提交加入申请')
  }
  return {
    user, title, settings, publications, invites, members, countedMembers, usedQuota, remainingQuota, busy, ready,
    login, logout, refresh, searchWithdrawn, publicationFor, publishPaper, withdrawPaper, createInvite, stopInvite, rotateInvite, addMember,
    approveMember: (id: string) => status(id, 'active'), rejectMember: (id: string) => status(id, 'rejected'),
    stopMember: (id: string) => status(id, 'stopped'), restoreMember: (id: string) => status(id, 'active'),
  }
})
