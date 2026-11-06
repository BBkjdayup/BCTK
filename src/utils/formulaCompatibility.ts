import type { RichContent } from '../types/domain'

export interface FormulaField { label: string; content: RichContent }
export function questionFormulaFields(question: { stem: RichContent; answer: RichContent; explanation: RichContent; options: (RichContent | { content: RichContent })[] }, prefix = ''): FormulaField[] {
  return [
    { label: prefix + '题干', content: question.stem },
    ...question.options.map((option, i) => ({ label: `${prefix}选项 ${String.fromCharCode(65 + i)}`, content: 'content' in option ? option.content : option })),
    { label: prefix + '答案', content: question.answer },
    { label: prefix + '解析', content: question.explanation },
  ]
}
export function formulaOccurrences(fields: FormulaField[]) {
  return fields.flatMap(field => {
    const doc = new DOMParser().parseFromString(field.content.html || '', 'text/html')
    return [...doc.querySelectorAll('[data-latex]')].filter(node => !node.parentElement?.closest('[data-latex]'))
      .map(node => ({ label: field.label, latex: node.getAttribute('data-latex') || '' }))
  })
}
