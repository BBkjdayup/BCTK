import { describe, expect, it, vi } from 'vitest'
import type { Paper, Question, RichContent } from '../types/domain'
import type { PaperCanvasFormulaChange } from './paperLayout'
import { applyPaperFormulaChange, updateRichContentFormula } from './paperFormula'

function rich(latex: string): RichContent {
  return {
    schemaVersion: 2,
    editor: 'tiptap',
    editorVersion: '3.28.0',
    document: {
      type: 'doc',
      content: [{
        type: 'paragraph',
        content: [{ type: 'text', text: '范围：' }, { type: 'mathNode', attrs: { latex } }],
      }],
    },
    html: `<p>范围：<span class="math-node" data-latex="${latex}">${latex}</span></p>`,
    plainText: `范围：\\(${latex}\\)`,
    sourceOoxml: '<m:oMath />',
  }
}

function paper(): Paper {
  const question: Question = {
    id: 'question-1',
    type: 'single_choice',
    stem: rich('x^2'),
    options: [{ id: 'option-a', position: 0, content: rich('-2\\le m\\le -1') }],
    answer: rich('A'),
    explanation: rich('x'),
    subjectId: 'subject-1',
    chapterId: 'chapter-1',
    subjectName: '数学',
    chapterName: '函数',
    tags: [],
    createdAt: 1,
    updatedAt: 1,
    contentVersion: 1,
  }
  return {
    id: 'paper-1',
    title: '测试卷',
    compositionMode: 'manual',
    generationConfig: null,
    status: 'draft',
    items: [{ id: 'paper-item-1', sourceQuestionId: question.id, position: 0, snapshot: question }],
    exportContentMode: 'paper_only',
    subjectSummaryText: '数学',
    rowVersion: 1,
    createdAt: 1,
    updatedAt: 1,
  }
}

describe('paper formula editing', () => {
  it('updates HTML, Tiptap JSON and plain text while discarding stale source OOXML', () => {
    const result = updateRichContentFormula(rich('-2\\le m\\le -1'), {
      kind: 'paper-formula',
      paperItemId: 'paper-item-1',
      contentSlot: 'option',
      optionId: 'option-a',
      ordinal: 0,
      sourceLatex: '-2\\le m\\le -1',
    }, '-3\\le m\\le 0')

    expect(result?.html).toContain('data-latex="-3\\le m\\le 0"')
    expect(result?.plainText).toBe('范围：\\(-3\\le m\\le 0\\)')
    expect(result?.document).toMatchObject({
      content: [{ content: [{}, { attrs: { latex: '-3\\le m\\le 0' } }] }],
    })
    expect(result).not.toHaveProperty('sourceOoxml')
  })

  it('changes only the current paper snapshot and not its source question object', () => {
    vi.spyOn(Date, 'now').mockReturnValue(100)
    const source = paper()
    const originalQuestion = structuredClone(source.items[0]!.snapshot)
    const change: PaperCanvasFormulaChange = {
      reference: {
        kind: 'paper-formula',
        paperItemId: 'paper-item-1',
        contentSlot: 'option',
        optionId: 'option-a',
        ordinal: 0,
        sourceLatex: '-2\\le m\\le -1',
      },
      latex: '-3\\le m\\le 0',
    }

    expect(applyPaperFormulaChange(source, change)).toBe(true)
    expect(source.items[0]?.snapshot.options[0]?.content.plainText).toContain('-3\\le m\\le 0')
    expect(source.items[0]?.snapshot.contentVersion).toBe(2)
    expect(source.items[0]?.snapshot.updatedAt).toBe(100)
    expect(originalQuestion.options[0]?.content.plainText).toContain('-2\\le m\\le -1')
  })
})
