import type { QuestionOption, RichContent } from '../types/domain'
import { emptyRichContent, plainTextRichContent, richContentSearchText } from './richContent'

export function remapChoiceAnswer(
  before: readonly QuestionOption[],
  after: readonly QuestionOption[],
  answer: RichContent,
): { answer: RichContent; reviewRequired: boolean } {
  const text = richContentSearchText(answer).normalize('NFKC').trim().toUpperCase()
  // A formula/image can look like a letter but is not an option label.
  if (new DOMParser().parseFromString(answer.html, 'text/html').querySelector('img, [data-latex], table')
    || /"type"\s*:\s*"(?:mathNode|image|table)"/u.test(JSON.stringify(answer.document ?? {}))) {
    return { answer, reviewRequired: true }
  }
  if (!text) return { answer, reviewRequired: false }
  if (!/^[A-Z](?:[\s、,，;；]*[A-Z])*$/u.test(text)) return { answer, reviewRequired: true }
  const letters = [...text].filter((letter) => /[A-Z]/u.test(letter))
  const positions = letters.map((letter) => {
    const option = before[letter.charCodeAt(0) - 65]
    return option ? after.findIndex((candidate) => candidate.id === option.id) : -1
  })
  if (positions.some((position) => position < 0)) {
    return { answer: emptyRichContent(), reviewRequired: true }
  }
  return {
    answer: plainTextRichContent([...new Set(positions)].sort((a, b) => a - b)
      .map((position) => String.fromCharCode(65 + position)).join('、')),
    reviewRequired: false,
  }
}
