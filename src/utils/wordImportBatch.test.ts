import { describe, expect, it } from 'vitest'
import type { QuestionDraft, RichContent } from '../types/domain'
import {
  applyWordImportBatchClassification,
  canOpenWordImportDuplicateDialog,
  pendingWordImportItems,
  planWordImportOverwrites,
  setWordImportBatchSelection,
  wordImportDuplicateActionTargets,
  wordImportBatchSelectionState,
  wordImportDuplicateScanPercentage,
  wordImportQuestionSemanticSignature,
} from './wordImportBatch'

interface TestItem {
  id: string
  selected: boolean
  subjectId: string
  chapterId: string
}

function questions(count: number): TestItem[] {
  return Array.from({ length: count }, (_, index) => ({
    id: `question-${index + 1}`,
    selected: true,
    subjectId: 'subject-original',
    chapterId: 'chapter-original',
  }))
}

function selectFirst(items: TestItem[], count: number) {
  setWordImportBatchSelection(items, false)
  for (const item of items.slice(0, count)) item.selected = true
}

describe('Word import batch processing', () => {
  it('keeps a 1200-question duplicate scan denominator at 1200 across both phases', () => {
    expect(wordImportDuplicateScanPercentage({
      phase: 'database', completed: 600, total: 1_200, includesImportBatchPass: true,
    })).toBe(25)
    expect(wordImportDuplicateScanPercentage({
      phase: 'database', completed: 1_200, total: 1_200, includesImportBatchPass: true,
    })).toBe(50)
    expect(wordImportDuplicateScanPercentage({
      phase: 'import-batch', completed: 600, total: 1_200, includesImportBatchPass: true,
    })).toBe(75)
    expect(wordImportDuplicateScanPercentage({
      phase: 'import-batch', completed: 1_200, total: 1_200, includesImportBatchPass: true,
    })).toBe(100)
  })

  it('uses the full percentage range when no batch-internal pass is needed', () => {
    expect(wordImportDuplicateScanPercentage({
      phase: 'database', completed: 600, total: 1_200, includesImportBatchPass: false,
    })).toBe(50)
    expect(wordImportDuplicateScanPercentage({
      phase: 'database', completed: 1_200, total: 1_200, includesImportBatchPass: false,
    })).toBe(100)
  })

  it('reports none, partial and all selection states', () => {
    const items = questions(3)

    setWordImportBatchSelection(items, false)
    expect(wordImportBatchSelectionState(items)).toEqual({
      selectedCount: 0,
      allSelected: false,
      indeterminate: false,
    })

    items[0]!.selected = true
    expect(wordImportBatchSelectionState(items)).toEqual({
      selectedCount: 1,
      allSelected: false,
      indeterminate: true,
    })

    setWordImportBatchSelection(items, true)
    expect(wordImportBatchSelectionState(items)).toEqual({
      selectedCount: 3,
      allSelected: true,
      indeterminate: false,
    })
  })

  it('processes 19 questions in 5, 5 and 9 while keeping the current batch selected', () => {
    let pending = questions(19)

    selectFirst(pending, 5)
    const firstBatch = applyWordImportBatchClassification(pending, 'network', 'chapter-1')
    expect(firstBatch).toHaveLength(5)
    expect(firstBatch.every((item) => item.selected && item.chapterId === 'chapter-1')).toBe(true)
    pending = pendingWordImportItems(pending, new Set(firstBatch.map((item) => item.id)))
    expect(pending).toHaveLength(14)

    selectFirst(pending, 5)
    const secondBatch = applyWordImportBatchClassification(pending, 'network', 'chapter-2')
    expect(secondBatch).toHaveLength(5)
    expect(secondBatch.every((item) => item.selected && item.chapterId === 'chapter-2')).toBe(true)
    pending = pendingWordImportItems(pending, new Set(secondBatch.map((item) => item.id)))
    expect(pending).toHaveLength(9)

    setWordImportBatchSelection(pending, true)
    const finalBatch = applyWordImportBatchClassification(pending, 'network', 'chapter-3')
    pending = pendingWordImportItems(pending, new Set(finalBatch.map((item) => item.id)))
    expect(pending).toHaveLength(0)
  })

  it('keeps unselected and failed questions in the pending list', () => {
    const items = questions(19)
    selectFirst(items, 5)
    const selected = applyWordImportBatchClassification(items, 'network', 'chapter-1')

    const completedIds = new Set(selected.slice(0, 4).map((item) => item.id))
    const pending = pendingWordImportItems(items, completedIds)

    expect(pending).toHaveLength(15)
    expect(pending.some((item) => item.id === selected[4]?.id && item.selected)).toBe(true)
    expect(pending.filter((item) => !item.selected)).toHaveLength(14)
  })

  it('does not allow a conflict decision while the complete duplicate scan is still running', () => {
    const item = {
      ...questions(1)[0]!,
      duplicateKind: 'exact' as const,
      duplicateCandidate: { sourceKind: 'question' as const },
      duplicateChecking: false,
      duplicateNeedsCheck: false,
    }

    expect(canOpenWordImportDuplicateDialog(item, true)).toBe(false)
    expect(canOpenWordImportDuplicateDialog(item, false)).toBe(true)
  })

  it('applies one decision to every ready and selected conflict of the same kind', () => {
    const base = questions(5).map((item, index) => ({
      ...item,
      duplicateKind: index === 4 ? 'suspected' as const : 'exact' as const,
      duplicateCandidate: { sourceKind: index === 3 ? 'import-item' as const : 'question' as const },
      duplicateChecking: false,
      duplicateNeedsCheck: false,
    }))
    base[2]!.selected = false

    expect(wordImportDuplicateActionTargets(base, base[0]!, 'keep', true).map((item) => item.id))
      .toEqual(['question-1', 'question-2', 'question-4'])
    expect(wordImportDuplicateActionTargets(base, base[0]!, 'keep', false).map((item) => item.id))
      .toEqual(['question-1'])
    expect(wordImportDuplicateActionTargets(base, base[0]!, 'overwrite', true).map((item) => item.id))
      .toEqual(['question-1', 'question-2'])
  })
})

function rich(text: string, nodeId: string): RichContent {
  return {
    schemaVersion: 2,
    editor: 'tiptap',
    editorVersion: '2.0.0',
    document: {
      type: 'doc',
      content: [{
        type: 'paragraph',
        attrs: { nodeId },
        content: [{ type: 'text', text }],
      }],
    },
    html: `<p data-node-id="${nodeId}">${text}</p>`,
    plainText: text,
  }
}

function overwriteDraft(prefix: string): QuestionDraft {
  const firstOptionId = `${prefix}-option-a`
  const secondOptionId = `${prefix}-option-b`
  return {
    id: `${prefix}-draft`,
    questionId: `${prefix}-old-target`,
    baseContentVersion: prefix === 'first' ? 4 : 99,
    type: 'single_choice',
    stem: rich('题干', `${prefix}-stem-node`),
    options: [
      { id: firstOptionId, position: 7, content: rich('选项 A', `${prefix}-option-a-node`) },
      { id: secondOptionId, position: 2, content: rich('选项 B', `${prefix}-option-b-node`) },
    ],
    answer: rich('A', `${prefix}-answer-node`),
    explanation: rich('解析', `${prefix}-explanation-node`),
    subjectId: 'subject-1',
    chapterId: 'chapter-1',
    tagIds: prefix === 'first' ? ['tag-2', 'tag-1'] : ['tag-1', 'tag-2'],
    resourceRefs: prefix === 'first'
      ? [
          {
            nodeId: `${prefix}-option-b-image-node`,
            resourceId: 'resource-option-b',
            contentSlot: 'option',
            optionId: secondOptionId,
          },
          {
            nodeId: `${prefix}-stem-image-node`,
            resourceId: 'resource-stem',
            contentSlot: 'stem',
          },
        ]
      : [
          {
            nodeId: `${prefix}-stem-image-node`,
            resourceId: 'resource-stem',
            contentSlot: 'stem',
          },
          {
            nodeId: `${prefix}-option-b-image-node`,
            resourceId: 'resource-option-b',
            contentSlot: 'option',
            optionId: secondOptionId,
          },
        ],
  }
}

interface OverwriteTestItem {
  id: string
  duplicateCandidate: { id: string, sourceKind: 'question' | 'import-item' }
  draft: QuestionDraft
}

function overwriteItem(id: string, targetQuestionId: string, draft = overwriteDraft(id)): OverwriteTestItem {
  return {
    id,
    duplicateCandidate: { id: targetQuestionId, sourceKind: 'question' },
    draft,
  }
}

describe('Word import overwrite planning', () => {
  it('ignores operation, option and node IDs while retaining resource locations', () => {
    expect(wordImportQuestionSemanticSignature(overwriteDraft('first')))
      .toBe(wordImportQuestionSemanticSignature(overwriteDraft('second')))
  })

  it('writes one representative and skips equivalent source rows for each target', () => {
    const first = overwriteItem('first', 'stored-1', overwriteDraft('first'))
    const repeated = overwriteItem('second', 'stored-1', overwriteDraft('second'))
    const other = overwriteItem('other', 'stored-2', overwriteDraft('other'))

    const plan = planWordImportOverwrites([first, repeated, other], (item) => item.draft)

    expect(plan.representatives).toEqual([first, other])
    expect(plan.duplicates).toEqual([{
      targetQuestionId: 'stored-1',
      item: repeated,
      representative: first,
    }])
    expect(plan.conflicts).toEqual([])
  })

  it('returns every row as a conflict when one target receives different content', () => {
    const first = overwriteItem('first', 'stored-1', overwriteDraft('first'))
    const changed = overwriteDraft('second')
    changed.answer = rich('B', 'different-answer-node')
    const second = overwriteItem('second', 'stored-1', changed)
    const safe = overwriteItem('safe', 'stored-2', overwriteDraft('safe'))

    const plan = planWordImportOverwrites([first, second, safe], (item) => item.draft)

    expect(plan.representatives).toEqual([safe])
    expect(plan.duplicates).toEqual([])
    expect(plan.conflicts).toEqual([{ targetQuestionId: 'stored-1', items: [first, second] }])
  })

  it.each([
    ['type', (draft: QuestionDraft) => { draft.type = 'multiple_choice' }],
    ['stem text', (draft: QuestionDraft) => { draft.stem = rich('不同题干', 'node') }],
    ['rich structure', (draft: QuestionDraft) => {
      draft.stem.document = { type: 'doc', content: [{ type: 'blockquote', content: [] }] }
    }],
    ['source OOXML', (draft: QuestionDraft) => { draft.stem.sourceOoxml = '<w:r><w:t>公式</w:t></w:r>' }],
    ['option order', (draft: QuestionDraft) => { draft.options.reverse() }],
    ['answer', (draft: QuestionDraft) => { draft.answer = rich('B', 'node') }],
    ['explanation', (draft: QuestionDraft) => { draft.explanation = rich('不同解析', 'node') }],
    ['subject', (draft: QuestionDraft) => { draft.subjectId = 'subject-2' }],
    ['chapter', (draft: QuestionDraft) => { draft.chapterId = 'chapter-2' }],
    ['tags', (draft: QuestionDraft) => { draft.tagIds.push('tag-3') }],
    ['resource', (draft: QuestionDraft) => { draft.resourceRefs![0]!.resourceId = 'other-resource' }],
    ['resource option position', (draft: QuestionDraft) => {
      const optionReference = draft.resourceRefs!.find((reference) => reference.contentSlot === 'option')!
      optionReference.optionId = draft.options[0]!.id
    }],
  ])('treats changed %s as different content', (_label, mutate) => {
    const first = overwriteDraft('first')
    const changed = overwriteDraft('second')
    mutate(changed)

    const plan = planWordImportOverwrites([
      overwriteItem('first', 'stored-1', first),
      overwriteItem('second', 'stored-1', changed),
    ], (item) => item.draft)

    expect(plan.conflicts).toHaveLength(1)
    expect(plan.representatives).toHaveLength(0)
    expect(plan.duplicates).toHaveLength(0)
  })

  it('rejects non-database candidates before planning writes', () => {
    const item = overwriteItem('first', 'earlier-import-item')
    item.duplicateCandidate.sourceKind = 'import-item'
    expect(() => planWordImportOverwrites([item], (entry) => entry.draft))
      .toThrow('覆盖归并只接受指向题库原题的导入项。')
  })

  it('coalesces a large repeated import without scheduling repeated writes', () => {
    const items = Array.from({ length: 1_200 }, (_, index) => (
      overwriteItem(`row-${index + 1}`, 'stored-1', overwriteDraft(`row-${index + 1}`))
    ))
    const plan = planWordImportOverwrites(items, (item) => item.draft)

    expect(plan.representatives).toHaveLength(1)
    expect(plan.duplicates).toHaveLength(1_199)
    expect(plan.conflicts).toHaveLength(0)
  })
})
