import { describe, expect, it } from 'vitest'
import { miniProgramPaper } from './miniProgramPaper'
import { plainTextRichContent } from './richContent'
import type { Paper, Question } from '../types/domain'

function paper(answer = 'A、C'): Paper {
  const rich = plainTextRichContent
  const question = { type: 'multiple_choice', stem: rich('选择符合条件的选项'), answer: rich(answer), explanation: rich('解析'),
    options: ['一', '二', '三'].map((text, i) => ({ id: 'option-' + i, position: i, content: rich(text) })),
  } as Question
  return { id: 'paper-1', title: '测试卷', status: 'saved', rowVersion: 2, items: [{ id: 'item-1', position: 0, snapshot: question }] } as Paper
}
describe('mini program publication snapshots', () => {
  it('maps answers to ordered option ids and freezes question content from the saved paper', () => {
    const p = paper(), result = miniProgramPaper(p)
    expect(result.questions[0]!.correctChoices).toEqual(['option-0', 'option-2'])
    expect(result.questions[0]!.id).toBe('item-1')
    p.items[0]!.snapshot.stem = plainTextRichContent('修改后')
    expect(result.questions[0]!.stem).not.toContain('修改后')
  })
  it('refuses to guess grading or silently drop images/formulas', () => {
    expect(() => miniProgramPaper(paper('答案可能是 A'))).toThrow('无法确定')
    expect(() => miniProgramPaper(paper('A、A'))).toThrow('无法确定')
    const p = paper(); p.items[0]!.snapshot.stem.html = '<p>图形题<img src="asset://local"></p>'
    expect(() => miniProgramPaper(p)).toThrow('资源尚未就绪')
  })
  it('strips links, event handlers, scripts and styling from published rich text', () => {
    const p = paper(); p.items[0]!.snapshot.stem.html = '<p onclick="steal()"><a href="https://tracker.invalid">题干</a><script>alert(1)</script><strong>重点</strong></p>'
    expect(miniProgramPaper(p).questions[0]!.stem).toBe('<p>题干<strong>重点</strong></p>')
  })
})
