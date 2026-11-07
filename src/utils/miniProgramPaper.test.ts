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
  it('retains source identity, revision, stable classifications and proportional image size', () => {
    const p = paper(), q = p.items[0]!.snapshot
    Object.assign(q, { id:'source-question',contentVersion:7,subjectId:'subject',subjectName:'数学',chapterId:'chapter',chapterName:'函数',unitId:'unit',unitName:'函数性质',tags:[{id:'tag',name:'重点'}] })
    q.stem.html='<p><img data-resource-id="image-one" width="150"></p>'
    const published=miniProgramPaper(p,undefined,new Map([['image:image-one',{id:'asset',width:600,height:300}]]))
    expect(published.questions[0]!.metadata).toEqual({sourceId:'source-question',revision:7,subjectId:'subject',subjectName:'数学',chapterId:'chapter',chapterName:'函数',tags:['重点'],tagIds:['tag']})
    expect(published.questions[0]!.stem).toContain('data-width="150" data-height="75"')
    expect(published.questions[0]!.id).toBe('item-1')
  })
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
  it('preserves table groups and merged cells in every published field and preflight', () => {
    const p = paper()
    const html = '<table><thead><tr><th colspan="2">成绩</th></tr></thead><tbody><tr><td rowspan="2">单元一</td><td>80</td></tr><tr><td>90</td></tr></tbody><tfoot><tr><td colspan="2">合计</td></tr></tfoot></table>'
    const q = p.items[0]!.snapshot
    q.stem.html = html; q.options[0]!.content.html = html; q.explanation.html = html
    for (const preflight of [true, false]) {
      const published = miniProgramPaper(p, undefined, undefined, preflight).questions[0]!
      expect(published.stem).toBe(html)
      expect(published.options[0]!.content).toBe(html)
      expect(published.explanation).toBe(html)
    }
    q.type = 'short_answer'; q.answer.html = html
    expect(miniProgramPaper(p).questions[0]!.answer).toBe(html)
  })
  it('keeps plain tables and permits only bounded span attributes on cells', () => {
    const p = paper()
    p.items[0]!.snapshot.stem.html = '<table><tbody><tr><td>普通</td><td colspan="1" rowspan="1000" onclick="steal()" style="color:red">合并</td></tr></tbody></table>'
    expect(miniProgramPaper(p).questions[0]!.stem).toBe('<table><tbody><tr><td>普通</td><td colspan="1" rowspan="1000">合并</td></tr></tbody></table>')
  })
  it('rejects invalid spans before publication instead of silently changing table structure', () => {
    for (const tag of ['td', 'th']) for (const attr of ['colspan', 'rowspan']) {
      for (const value of ['', '0', '-1', '1.5', '1e2', '+2', '02', ' 2 ', '1001', '99999999999999999999']) {
        const p = paper()
        p.items[0]!.snapshot.stem.html = `<table><tbody><tr><${tag} ${attr}="${value}">合并</${tag}></tr></tbody></table>`
        for (const preflight of [true, false]) {
          expect(() => miniProgramPaper(p, undefined, undefined, preflight), `${tag} ${attr}=${value}`).toThrow('第 1 题题干的表格合并行列数无效')
        }
      }
    }
  })
})
