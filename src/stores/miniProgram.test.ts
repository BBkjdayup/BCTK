import { beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useMiniProgramStore } from './miniProgram'

describe('mini-program management quota', () => {
  beforeEach(() => {
    localStorage.clear()
    setActivePinia(createPinia())
  })

  it('retains records from former subject groups and reloads new records without a subject', () => {
    const now = Date.now()
    const invites = ['subject-1', 'subject-2'].map((subjectId, index) => ({
      id: `legacy-invite-${index}`, subjectId, code: `LEGACY0${index}`,
      label: `原分组 ${index}`, createdAt: now, expiresAt: now + 86400000, active: true,
    }))
    const members = invites.map((invite, index) => ({
      id: `legacy-member-${index}`, subjectId: invite.subjectId, inviteId: invite.id,
      name: `原成员 ${index}`, account: index === 0 ? ' Student-1 ' : 'student-1',
      status: index === 0 ? 'active' : 'stopped', requestedAt: now, joinedAt: now,
      stoppedAt: index === 0 ? null : now, reason: null,
    }))
    localStorage.setItem('tkup-mini-program-management-v1', JSON.stringify({ invites, members }))

    const store = useMiniProgramStore()
    expect(store.invites).toEqual(invites)
    expect(store.members).toEqual(members)
    expect(store.usedQuota).toBe(1)
    const invite = store.createInvite({ label: '统一入口', validDays: 30 })
    const member = store.addMember({ name: '新成员', account: 'student-2', inviteId: invite.id })
    store.approveMember(member.id)
    expect(invite).not.toHaveProperty('subjectId')
    expect(member).not.toHaveProperty('subjectId')

    setActivePinia(createPinia())
    const reloaded = useMiniProgramStore()
    expect(reloaded.invites).toHaveLength(3)
    expect(reloaded.invites).toEqual(expect.arrayContaining([...invites, invite]))
    expect(reloaded.members).toHaveLength(3)
    expect(reloaded.members).toEqual(expect.arrayContaining(members))
    expect(reloaded.members.find((item) => item.id === member.id)?.status).toBe('active')
    expect(reloaded.usedQuota).toBe(2)
  })

  it('counts unique approved accounts and keeps stopped members in the ledger', () => {
    localStorage.setItem('tkup-mini-program-management-v1', JSON.stringify({ settings: { quota: 2 } }))
    const store = useMiniProgramStore()
    const invite = store.createInvite({ label: '测试班', validDays: 30 })
    const first = store.addMember({ name: '张三', account: 'student-1', inviteId: invite.id })
    const second = store.addMember({ name: '李四', account: 'student-2', inviteId: invite.id })
    const duplicate = store.addMember({ name: '张三（重复申请）', account: 'student-1', inviteId: invite.id })

    expect(store.approveMember(first.id)).toBe(true)
    expect(store.approveMember(second.id)).toBe(true)
    expect(store.usedQuota).toBe(2)
    expect(store.remainingQuota).toBe(0)
    expect(store.approveMember(duplicate.id)).toBe(true)
    expect(store.usedQuota).toBe(2)

    expect(store.stopMember(first.id)).toBe(true)
    expect(store.usedQuota).toBe(2)
    expect(store.countedMembers.map((member) => member.id)).toEqual(expect.arrayContaining([first.id, second.id, duplicate.id]))
  })

  it('tracks a published paper version and removes it when withdrawn', () => {
    const store = useMiniProgramStore()
    expect(store.publicationFor('paper-1')).toBeNull()
    store.publishPaper('paper-1', 3)
    expect(store.publicationFor('paper-1')?.paperRowVersion).toBe(3)
    store.publishPaper('paper-1', 4)
    expect(store.publicationFor('paper-1')?.paperRowVersion).toBe(4)
    store.withdrawPaper('paper-1')
    expect(store.publicationFor('paper-1')).toBeNull()
  })
})
