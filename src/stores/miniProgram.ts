import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import { clonePlain } from '../utils/clonePlain'
import type {
  MiniProgramInvite,
  MiniProgramMember,
  MiniProgramMemberStatus,
  MiniProgramPublication,
  MiniProgramSettings,
} from '../types/miniProgram'

const STORAGE_KEY = 'tkup-mini-program-management-v1'

interface PersistedMiniProgramState {
  settings: MiniProgramSettings
  publications: MiniProgramPublication[]
  invites: MiniProgramInvite[]
  members: MiniProgramMember[]
}

function dateInputValue(date: Date) {
  const year = date.getFullYear()
  const month = String(date.getMonth() + 1).padStart(2, '0')
  const day = String(date.getDate()).padStart(2, '0')
  return `${year}-${month}-${day}`
}

function defaultSettings(): MiniProgramSettings {
  return {
    quota: 1,
    freeQuota: 1,
    paidQuota: 0,
    paidStatus: 'none',
    serviceStarts: dateInputValue(new Date()),
    serviceEnds: '9999-12-31',
    paused: false,
  }
}

function clone<T>(value: T): T {
  return clonePlain(value)
}

function newId(prefix: string) {
  return `${prefix}-${crypto.randomUUID()}`
}

function normalizeState(input: unknown): PersistedMiniProgramState {
  const fallback: PersistedMiniProgramState = {
    settings: defaultSettings(),
    publications: [],
    invites: [],
    members: [],
  }
  if (!input || typeof input !== 'object') return fallback
  const source = input as Partial<PersistedMiniProgramState>
  const settings = source.settings && typeof source.settings === 'object'
    ? source.settings as Partial<MiniProgramSettings>
    : {}
  const quota = Number(settings.quota)
  const statusValues: MiniProgramMemberStatus[] = ['pending', 'active', 'stopped', 'rejected']
  const publications = Array.isArray(source.publications)
    ? source.publications.filter((item): item is MiniProgramPublication => (
      Boolean(item)
      && typeof item === 'object'
      && typeof (item as MiniProgramPublication).paperId === 'string'
      && Number.isSafeInteger((item as MiniProgramPublication).paperRowVersion)
      && Number.isFinite((item as MiniProgramPublication).publishedAt)
    ))
    : []
  const invites = Array.isArray(source.invites)
    ? source.invites.filter((item): item is MiniProgramInvite => (
      Boolean(item)
      && typeof item === 'object'
      && typeof (item as MiniProgramInvite).id === 'string'
      && typeof (item as MiniProgramInvite).code === 'string'
      && Number.isFinite((item as MiniProgramInvite).expiresAt)
    ))
    : []
  const members = Array.isArray(source.members)
    ? source.members.filter((item): item is MiniProgramMember => (
      Boolean(item)
      && typeof item === 'object'
      && typeof (item as MiniProgramMember).id === 'string'
      && typeof (item as MiniProgramMember).name === 'string'
      && typeof (item as MiniProgramMember).account === 'string'
      && statusValues.includes((item as MiniProgramMember).status)
    ))
    : []
  return {
    settings: {
      quota: Number.isSafeInteger(quota) ? Math.min(100000, Math.max(1, quota)) : fallback.settings.quota,
      serviceStarts: typeof settings.serviceStarts === 'string' ? settings.serviceStarts : fallback.settings.serviceStarts,
      serviceEnds: typeof settings.serviceEnds === 'string' ? settings.serviceEnds : fallback.settings.serviceEnds,
      paused: settings.paused === true,
      freeQuota: settings.freeQuota === 1 ? 1 : settings.quota === undefined ? fallback.settings.freeQuota : undefined,
      paidQuota: typeof settings.paidQuota === 'number' ? settings.paidQuota : fallback.settings.paidQuota,
      paidStatus: settings.paidStatus ?? fallback.settings.paidStatus,
      paidServiceStarts: settings.paidServiceStarts ?? null,
      paidServiceEnds: settings.paidServiceEnds ?? null,
    },
    publications: clone(publications),
    invites: clone(invites),
    members: clone(members),
  }
}

function loadState(): PersistedMiniProgramState {
  if (typeof localStorage === 'undefined') return normalizeState(null)
  try {
    const saved = localStorage.getItem(STORAGE_KEY)
    return saved ? normalizeState(JSON.parse(saved) as unknown) : normalizeState(null)
  } catch {
    return normalizeState(null)
  }
}

export const useMiniProgramStore = defineStore('miniProgram', () => {
  const loaded = loadState()
  const settings = ref<MiniProgramSettings>(loaded.settings)
  const publications = ref<MiniProgramPublication[]>(loaded.publications)
  const invites = ref<MiniProgramInvite[]>(loaded.invites)
  const members = ref<MiniProgramMember[]>(loaded.members)

  const countedMembers = computed(() => members.value.filter((member) => member.status !== 'pending' && member.status !== 'rejected'))
  const countedAccounts = computed(() => new Set(countedMembers.value.map((member) => member.account.trim().toLocaleLowerCase('zh-CN'))))
  const usedQuota = computed(() => countedAccounts.value.size)
  const remainingQuota = computed(() => Math.max(0, settings.value.quota - usedQuota.value))

  function persist() {
    if (typeof localStorage === 'undefined') return
    const state: PersistedMiniProgramState = {
      settings: clone(settings.value),
      publications: clone(publications.value),
      invites: clone(invites.value),
      members: clone(members.value),
    }
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(state))
    } catch {
      // A restricted webview can deny storage. The in-memory state remains usable.
    }
  }

  function publicationFor(paperId: string) {
    return publications.value.find((publication) => publication.paperId === paperId && publication.published !== false) ?? null
  }

  function publishPaper(paperId: string, paperRowVersion: number, metadata?: { title: string; questionCount: number }) {
    const next: MiniProgramPublication = {
      paperId,
      paperRowVersion,
      publishedAt: Date.now(),
      published: true,
      ...metadata,
    }
    const index = publications.value.findIndex((publication) => publication.paperId === paperId)
    if (index < 0) publications.value.push(next)
    else publications.value.splice(index, 1, next)
    persist()
  }

  function withdrawPaper(paperId: string) {
    const publication = publicationFor(paperId)
    if (publication) { publication.published = false; publication.publishedAt = Date.now() }
    persist()
  }

  function makeInviteCode() {
    const alphabet = 'ABCDEFGHJKLMNPQRSTUVWXYZ23456789'
    let code = ''
    do {
      const bytes = new Uint32Array(8)
      crypto.getRandomValues(bytes)
      code = Array.from(bytes, (byte) => alphabet[byte % alphabet.length]).join('')
    } while (invites.value.some((invite) => invite.code === code))
    return code
  }

  function createInvite(input: { label: string; validDays: number }) {
    const now = Date.now()
    const invite: MiniProgramInvite = {
      id: newId('invite'),
      code: makeInviteCode(),
      label: input.label.trim() || '未命名邀请码',
      createdAt: now,
      expiresAt: now + Math.max(1, Math.round(input.validDays)) * 86400000,
      active: true,
    }
    invites.value.unshift(invite)
    persist()
    return clone(invite)
  }

  function stopInvite(id: string) {
    const invite = invites.value.find((item) => item.id === id)
    if (!invite) return false
    invite.active = false
    persist()
    return true
  }

  function rotateInvite(id: string, validDays: number) {
    const invite = invites.value.find((item) => item.id === id)
    if (!invite) return null
    invite.code = makeInviteCode()
    invite.createdAt = Date.now()
    invite.expiresAt = invite.createdAt + Math.max(1, Math.round(validDays)) * 86400000
    invite.active = true
    persist()
    return clone(invite)
  }

  function addMember(input: { name: string; account: string; inviteId: string }) {
    const name = input.name.trim()
    const account = input.account.trim()
    if (!name || !account) throw new Error('请填写学员姓名和账号。')
    const member: MiniProgramMember = {
      id: newId('member'),
      name,
      account,
      inviteId: input.inviteId,
      status: 'pending',
      requestedAt: Date.now(),
      joinedAt: null,
      stoppedAt: null,
      reason: null,
    }
    members.value.unshift(member)
    persist()
    return clone(member)
  }

  function approveMember(id: string) {
    const member = members.value.find((item) => item.id === id)
    if (!member || member.status !== 'pending') return false
    const account = member.account.trim().toLocaleLowerCase('zh-CN')
    if (!countedAccounts.value.has(account) && usedQuota.value >= settings.value.quota) {
      throw new Error('本周期授权名额已用完，请联系管理员确认授权。')
    }
    if (settings.value.freeQuota === 1 && !member.seatType) {
      member.seatType = usedQuota.value === 0 ? 'free' : 'paid'
    }
    member.status = 'active'
    member.joinedAt = Date.now()
    member.reason = null
    persist()
    return true
  }

  function rejectMember(id: string, reason: string | null = null) {
    const member = members.value.find((item) => item.id === id)
    if (!member || member.status !== 'pending') return false
    member.status = 'rejected'
    member.reason = reason?.trim() || null
    persist()
    return true
  }

  function stopMember(id: string) {
    const member = members.value.find((item) => item.id === id)
    if (!member || member.status !== 'active') return false
    member.status = 'stopped'
    member.stoppedAt = Date.now()
    persist()
    return true
  }

  function restoreMember(id: string) {
    const member = members.value.find((item) => item.id === id)
    if (!member || member.status !== 'stopped') return false
    member.status = 'active'
    member.stoppedAt = null
    persist()
    return true
  }

  return {
    settings,
    publications,
    invites,
    members,
    countedMembers,
    usedQuota,
    remainingQuota,
    publicationFor,
    publishPaper,
    withdrawPaper,
    createInvite,
    stopInvite,
    rotateInvite,
    addMember,
    approveMember,
    rejectMember,
    stopMember,
    restoreMember,
  }
})
