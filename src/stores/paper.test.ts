import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import type { Paper, PaperItem, Question, QuestionType } from '../types/domain'
import {
  movePaperItemWithinType,
  normalizePaperItems,
  reorderPaperItemsWithinType,
  usePaperStore,
} from './paper'

const mocks = vi.hoisted(() => ({ savePaper: vi.fn() }))

vi.mock('../services/backend', () => ({
  backend: { savePaper: mocks.savePaper },
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

  it('never adds more questions than the active desktop paper limit', () => {
    const store = usePaperStore()
    const candidates = Array.from({ length: 12 }, (_, index) => (
      question(`single-${index + 1}`, 'single_choice')
    ))

    expect(store.addQuestions(candidates, 10)).toBe(10)
    expect(store.current.items).toHaveLength(10)
    expect(store.addQuestions([question('single-13', 'single_choice')], 10)).toBe(0)
    expect(store.current.items).toHaveLength(10)
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
