import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import type { Paper, PaperItem, Question, QuestionType } from '../types/domain'
import {
  movePaperItemWithinType,
  normalizePaperItems,
  reorderPaperItemsWithinType,
  usePaperStore,
} from './paper'

const mocks = vi.hoisted(() => ({ savePaper: vi.fn(), confirm: vi.fn(), getPaper: vi.fn(), copyPaper: vi.fn(),
  getPaperRecovery: vi.fn(), savePaperRecovery: vi.fn(), clearPaperRecovery: vi.fn() }))

vi.mock('../services/backend', () => ({
  backend: mocks,
}))

vi.mock('element-plus', () => ({
  ElMessageBox: { confirm: mocks.confirm },
  ElMessage: { error: vi.fn(), warning: vi.fn() },
}))

function question(id: string, type: QuestionType): Question {
  const content = { schemaVersion: 1 as const, html: `<p>${id}</p>`, plainText: id }
  return {
    id,
    type,
    stem: content,
    options: [],
    answer: content,
    explanation: content,
    subjectId: 'subject',
    chapterId: 'chapter',
    subjectName: '学科',
    chapterName: '章节',
    tags: [],
    resourceRefs: [],
    lastUsedAt: null,
    deletedAt: null,
    createdAt: 1,
    updatedAt: 1,
    contentVersion: 1,
  }
}

function item(id: string, type: QuestionType, position: number): PaperItem {
  return { id: `item-${id}`, sourceQuestionId: id, position, snapshot: question(id, type) }
}

describe('paper item ordering', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    mocks.savePaper.mockReset()
    mocks.confirm.mockReset()
    mocks.getPaperRecovery.mockResolvedValue(null)
    mocks.clearPaperRecovery.mockResolvedValue(undefined)
    mocks.savePaperRecovery.mockImplementation(async (paper: Paper) => ({ paper, revision: crypto.randomUUID(), autosavedAt: 10 }))
  })

  it('cancel preserves the title, selected questions and layout when starting another paper', async () => {
    const store = usePaperStore()
    store.current.title = '不能丢失'
    store.addQuestions([question('q1', 'single_choice')])
    mocks.confirm.mockRejectedValueOnce('close')
    const before = JSON.stringify(store.current)
    expect(await store.newPaper()).toBe(false)
    expect(JSON.stringify(store.current)).toBe(before)
  })

  it('discard starts a clean new paper and removes only its recovery checkpoint', async () => {
    const store = usePaperStore()
    await store.initializeRecovery()
    store.current.title = '可放弃'
    await store.checkpoint()
    mocks.confirm.mockRejectedValueOnce('cancel')
    expect(await store.newPaper()).toBe(true)
    expect(store.current.title).toBe('未命名试卷')
    expect(store.isDirty).toBe(false)
    expect(mocks.clearPaperRecovery).toHaveBeenCalled()
  })

  it('save failure cancels replacement and native close', async () => {
    const store = usePaperStore()
    store.current.title = '写盘失败仍保留'
    mocks.confirm.mockResolvedValue(undefined)
    mocks.savePaper.mockRejectedValue(new Error('disk full'))
    expect(await store.newPaper()).toBe(false)
    expect(await store.prepareToClose()).toBe(false)
    expect(store.current.title).toBe('写盘失败仍保留')
  })

  it('saves successfully before starting a new paper', async () => {
    const store = usePaperStore()
    store.current.title = '先保存'
    mocks.confirm.mockResolvedValueOnce(undefined)
    mocks.savePaper.mockImplementation(async (paper: Paper) => ({ ...paper, rowVersion: 1 }))
    expect(await store.newPaper()).toBe(true)
    expect(mocks.savePaper.mock.calls[0]![0].title).toBe('先保存')
    expect(store.current.title).toBe('未命名试卷')
  })

  it('cancel never opens or creates the requested history copy', async () => {
    const store = usePaperStore()
    store.current.title = '当前试卷'
    mocks.confirm.mockRejectedValue('close')
    expect(await store.loadPaper('other')).toBeNull()
    expect(await store.copyAndLoad('other', 1)).toBeNull()
    expect(mocks.getPaper).not.toHaveBeenCalled()
    expect(mocks.copyPaper).not.toHaveBeenCalled()
  })

  it('retains edits made while the next history paper is loading', async () => {
    const store = usePaperStore()
    const target = JSON.parse(JSON.stringify(store.current)) as Paper
    target.id = 'another-paper'
    let finish!: (paper: Paper) => void
    mocks.getPaper.mockImplementationOnce(() => new Promise<Paper>((resolve) => { finish = resolve }))
    const loading = store.loadPaper(target.id)
    await Promise.resolve()
    await Promise.resolve()
    store.current.title = '等待载入时的修改'
    finish(target)
    expect(await loading).toBeNull()
    expect(store.current.title).toBe('等待载入时的修改')
  })

  it('keeps the working copy when opening the next paper fails', async () => {
    const store = usePaperStore()
    store.current.title = '仍要保留'
    mocks.confirm.mockRejectedValueOnce('cancel')
    mocks.getPaper.mockRejectedValueOnce(new Error('read failed'))
    await expect(store.loadPaper('missing')).rejects.toThrow('read failed')
    expect(store.current.title).toBe('仍要保留')
    expect(store.isDirty).toBe(true)
  })

  it('restores a checkpoint as an unsaved draft without overwriting the old history row', async () => {
    const previous = usePaperStore()
    previous.current.title = '异常退出前的试卷'
    previous.addQuestions([question('q1', 'single_choice')])
    const original = JSON.parse(JSON.stringify(previous.current)) as Paper
    mocks.getPaperRecovery.mockResolvedValue({ paper: original, revision: 'old-checkpoint', autosavedAt: 1 })
    setActivePinia(createPinia())
    const store = usePaperStore()
    await store.initializeRecovery()
    expect(await store.recoverPaper()).toBe(true)
    expect(store.current.id).not.toBe(original.id)
    expect(store.current.title).toBe(original.title)
    expect(store.current.items[0]!.snapshot).toEqual(original.items[0]!.snapshot)
    expect(store.current.rowVersion).toBe(0)
    expect(store.isDirty).toBe(true)
  })

  it('keeps edits made while a save request is pending and merges the saved row version', async () => {
    let finishSave!: (paper: Paper) => void
    mocks.savePaper.mockImplementationOnce(() => new Promise<Paper>((resolve) => {
      finishSave = resolve
    }))
    const store = usePaperStore()
    store.current.title = '保存开始时的标题'
    const pending = store.save('draft')
    const sent = mocks.savePaper.mock.calls[0]?.[0] as Paper

    store.current.title = '等待保存时继续修改的标题'
    store.addQuestions([question('latest-question', 'single_choice')])
    finishSave({
      ...sent,
      rowVersion: 3,
      updatedAt: sent.updatedAt + 100,
      lastSavedAt: sent.updatedAt + 100,
    })

    await pending
    expect(store.current.title).toBe('等待保存时继续修改的标题')
    expect(store.current.items.map((entry) => entry.sourceQuestionId)).toEqual(['latest-question'])
    expect(store.current.rowVersion).toBe(3)
    expect(store.current.lastSavedAt).toBe(sent.updatedAt + 100)
  })

  it('uses QUESTION_TYPES order, preserves within-type order and reindexes continuously', () => {
    const ordered = normalizePaperItems([
      item('short-1', 'short_answer', 0),
      item('single-1', 'single_choice', 1),
      item('case-1', 'case_analysis', 2),
      item('single-2', 'single_choice', 3),
      item('multi-1', 'multiple_choice', 4),
    ])

    expect(ordered.map((entry) => entry.sourceQuestionId)).toEqual([
      'single-1', 'single-2', 'multi-1', 'short-1', 'case-1',
    ])
    expect(ordered.map((entry) => entry.position)).toEqual([0, 1, 2, 3, 4])
  })

  it('normalizes numbering immediately after adding interleaved question types', () => {
    const store = usePaperStore()

    expect(store.current.generationConfig).toBeNull()

    store.addQuestions([
      question('short-1', 'short_answer'),
      question('single-1', 'single_choice'),
      question('multi-1', 'multiple_choice'),
      question('single-2', 'single_choice'),
    ])

    expect(store.current.items.map((entry) => entry.sourceQuestionId)).toEqual([
      'single-1', 'single-2', 'multi-1', 'short-1',
    ])
    expect(store.current.items.map((entry) => entry.position)).toEqual([0, 1, 2, 3])
  })

  it('adds more than ten questions offline and still excludes duplicates', () => {
    const store = usePaperStore()
    const candidates = Array.from({ length: 12 }, (_, index) => (
      question(`single-${index + 1}`, 'single_choice')
    ))

    expect(store.addQuestions(candidates)).toBe(12)
    expect(store.current.items).toHaveLength(12)
    expect(store.addQuestions([...candidates, question('single-13', 'single_choice')])).toBe(1)
    expect(store.current.items).toHaveLength(13)
  })

  it('moves only inside the same question type and respects both boundaries', () => {
    const source = normalizePaperItems([
      item('single-1', 'single_choice', 0),
      item('single-2', 'single_choice', 1),
      item('multi-1', 'multiple_choice', 2),
      item('multi-2', 'multiple_choice', 3),
    ])

    const moved = movePaperItemWithinType(source, 'item-single-2', -1)
    expect(moved.moved).toBe(true)
    expect(moved.items.map((entry) => entry.sourceQuestionId)).toEqual([
      'single-2', 'single-1', 'multi-1', 'multi-2',
    ])
    expect(moved.items.map((entry) => entry.position)).toEqual([0, 1, 2, 3])

    const firstMultiple = movePaperItemWithinType(moved.items, 'item-multi-1', -1)
    const lastSingle = movePaperItemWithinType(moved.items, 'item-single-1', 1)
    expect(firstMultiple.moved).toBe(false)
    expect(lastSingle.moved).toBe(false)
    expect(firstMultiple.items.map((entry) => entry.sourceQuestionId)).toEqual(
      moved.items.map((entry) => entry.sourceQuestionId),
    )
  })

  it('applies a dragged order only inside its question type', () => {
    const source = normalizePaperItems([
      item('single-1', 'single_choice', 0),
      item('single-2', 'single_choice', 1),
      item('single-3', 'single_choice', 2),
      item('multi-1', 'multiple_choice', 3),
    ])

    const reordered = reorderPaperItemsWithinType(source, 'single_choice', [
      'item-single-3',
      'item-single-1',
      'item-single-2',
    ])
    expect(reordered.moved).toBe(true)
    expect(reordered.items.map((entry) => entry.sourceQuestionId)).toEqual([
      'single-3', 'single-1', 'single-2', 'multi-1',
    ])
    expect(reordered.items.map((entry) => entry.position)).toEqual([0, 1, 2, 3])

    const invalid = reorderPaperItemsWithinType(source, 'single_choice', [
      'item-single-3',
      'item-single-1',
    ])
    expect(invalid.moved).toBe(false)
    expect(invalid.items.map((entry) => entry.sourceQuestionId)).toEqual([
      'single-1', 'single-2', 'single-3', 'multi-1',
    ])
  })
})
