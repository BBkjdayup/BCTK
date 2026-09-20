import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, disposePinia, setActivePinia } from 'pinia'
import type { Paper } from '../types/domain'
import { paperLayoutSourceSignature } from '../utils/paperSourceSignature'

vi.mock('element-plus', () => ({
  ElMessageBox: { confirm: vi.fn().mockResolvedValue(undefined) },
  ElMessage: { error: vi.fn(), warning: vi.fn() },
}))

let pinia: ReturnType<typeof createPinia>
beforeEach(() => {
  localStorage.clear()
  vi.resetModules()
  pinia = createPinia()
  setActivePinia(pinia)
})
afterEach(() => { disposePinia(pinia); vi.restoreAllMocks() })

async function setup() {
  const { backend } = await import('../services/backend')
  const { usePaperStore } = await import('./paper')
  const store = usePaperStore()
  store.current.title = '试卷 A'
  store.addQuestions([(await backend.getQuestion('question-1'))!])
  const original = await store.save('saved')
  const list = () => backend.listPapers({ keyword: '', status: 'saved', page: 1, pageSize: 100 })
  return { store, backend, original, list }
}

describe('immutable paper archives', () => {
  it('keeps the complete original snapshot after clearing and rebuilding a saved paper', async () => {
    const { store, backend, original, list } = await setup()
    store.clear()
    store.addQuestions([(await backend.getQuestion('question-2'))!])
    store.current.title = '试卷 B'
    const next = await store.save('saved')
    expect(next.id).not.toBe(original.id)
    expect(next.rowVersion).toBe(1)
    expect((await list()).total).toBe(2)
    expect(await backend.getPaper(original.id)).toEqual(original)
    expect(next.items[0].sourceQuestionId).toBe('question-2')
    expect(store.current.id).toBe(next.id)
    expect(store.isDirty).toBe(false)
  })

  it('does not create duplicates on unchanged or concurrently repeated saves', async () => {
    const { store, original, list } = await setup()
    const unchanged = await store.save('saved')
    expect(unchanged).toEqual(original)
    store.current.title = '试卷 B'
    const [first, second] = await Promise.all([store.save('saved'), store.save('saved')])
    expect(first.id).toBe(second.id)
    expect(first.rowVersion).toBe(second.rowVersion)
    expect((await list()).total).toBe(2)
  })

  it('updates a draft until it is archived, then forks the next edited save', async () => {
    const { store, backend, list } = await setup()
    await store.newPaper()
    store.addQuestions([(await backend.getQuestion('question-2'))!])
    const draft = await store.save('draft')
    store.current.title = '草稿修改'
    const updated = await store.save('draft')
    const archived = await store.save('saved')
    expect(updated.id).toBe(draft.id)
    expect(updated.rowVersion).toBe(draft.rowVersion + 1)
    expect(archived.id).toBe(draft.id)
    store.current.title = '归档后修改'
    const next = await store.save('saved')
    expect(next.id).not.toBe(archived.id)
    expect(await backend.getPaper(archived.id)).toEqual(archived)
    expect((await list()).total).toBe(3)
  })

  it('keeps both the original and edits when a new archive cannot be written', async () => {
    const { store, backend, original, list } = await setup()
    store.current.title = '需要保留的修改'
    vi.spyOn(backend, 'savePaper').mockRejectedValueOnce(new Error('disk full'))
    await expect(store.save('saved')).rejects.toThrow('disk full')
    expect(store.current.id).toBe(original.id)
    expect(store.current.title).toBe('需要保留的修改')
    expect(store.isDirty).toBe(true)
    expect(await backend.getPaper(original.id)).toEqual(original)
    await store.save('saved')
    expect((await list()).total).toBe(2)
  })

  it('remaps canvas item references without losing images or manually adjusted layout', async () => {
    const { store, backend, original } = await setup()
    const oldItemId = store.current.items[0].id
    store.current.layout = {
      schemaVersion: 1, editor: 'canvas-editor', editorVersion: '0.9.137',
      sourceSignature: paperLayoutSourceSignature(store.current),
      data: { header: [{ paperItemId: oldItemId }], main: [{ valueList: [{ paperItemId: oldItemId, value: '手工分页', resourceId: 'unchanged-image' }] }] },
      options: { width: 794 }, pageSetup: { widthMm: 210, heightMm: 297 },
    }
    const next = await store.save('saved')
    expect(next.items[0].id).not.toBe(oldItemId)
    expect(next.items[0].snapshot).toEqual(original.items[0].snapshot)
    expect(next.layout!.data.header).toEqual([{ paperItemId: next.items[0].id }])
    expect(next.layout!.data.main).toEqual([{ valueList: [{ paperItemId: next.items[0].id, value: '手工分页', resourceId: 'unchanged-image' }] }])
    expect(next.layout!.sourceSignature).toBe(paperLayoutSourceSignature(next))
    expect(await backend.getPaper(original.id)).toEqual(original)
  })

  it('keeps later edits dirty while export receives only the persisted snapshot', async () => {
    const { store, backend, original } = await setup()
    const realSave = backend.savePaper.bind(backend)
    let finish!: () => void
    vi.spyOn(backend, 'savePaper').mockImplementationOnce((paper) => new Promise<Paper>((resolve, reject) => {
      finish = () => { void realSave(paper).then(resolve, reject) }
    }))
    store.current.title = '请求时标题'
    const pending = store.save('saved')
    store.current.title = '等待期间的新标题'
    finish()
    const saved = await pending
    expect(saved.title).toBe('请求时标题')
    expect(store.current.title).toBe('等待期间的新标题')
    expect(store.current.id).toBe(saved.id)
    expect(store.isDirty).toBe(true)
    expect(await backend.getPaper(original.id)).toEqual(original)
    const next = await store.save('saved')
    expect(next.id).not.toBe(saved.id)
    expect((await backend.getPaper(saved.id))!.title).toBe('请求时标题')
  })

  it('archives edits before closing without updating the old history', async () => {
    const { store, backend, original, list } = await setup()
    store.current.title = '退出前修改'
    expect(await store.prepareToClose()).toBe(true)
    expect(await backend.getPaper(original.id)).toEqual(original)
    expect((await list()).total).toBe(2)
  })

  it('rejects direct updates of archived rows even with the correct row version', async () => {
    const { backend, original } = await setup()
    await expect(backend.savePaper({ ...original, title: '绕过界面覆盖' }))
      .rejects.toMatchObject({ code: 'PAPER_ARCHIVE_IMMUTABLE' })
    expect(await backend.getPaper(original.id)).toEqual(original)
  })
})
