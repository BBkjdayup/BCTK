import { describe, expect, it } from 'vitest'
import { remapChoiceAnswer } from './choiceAnswer'
import { emptyRichContent, plainTextRichContent, hasMeaningfulRichContent, richContentSearchText } from './richContent'

const options = ['a', 'b', 'c', 'd'].map((id, position) => ({ id, position, content: plainTextRichContent(id) }))

describe('acceptance: rich content and stable answer identity', () => {
  it('tracks single and multiple correct option identities after reordering', () => {
    const moved = [options[1]!, options[2]!, options[0]!, options[3]!]
    expect(remapChoiceAnswer(options, moved, plainTextRichContent('A')).answer.plainText).toBe('C')
    expect(remapChoiceAnswer(options, moved, plainTextRichContent('A、C')).answer.plainText).toBe('B、C')
  })
  it('adjusts following labels when an incorrect option is removed', () => {
    expect(remapChoiceAnswer(options, options.slice(1), plainTextRichContent('C')).answer.plainText).toBe('B')
  })
  it('clears a removed correct answer and requires explicit review', () => {
    const result = remapChoiceAnswer(options, options.slice(1), plainTextRichContent('A、C'))
    expect(hasMeaningfulRichContent(result.answer)).toBe(false)
    expect(result.reviewRequired).toBe(true)
  })
  it('preserves explanations and requires review rather than rewriting them as letters', () => {
    const answer = plainTextRichContent('A，因为第一个选项正确')
    expect(remapChoiceAnswer(options, options.slice(1), answer)).toEqual({ answer, reviewRequired: true })
  })
  it('recognizes formula and managed-image content while rejecting truly empty structures', () => {
    const formula = { schemaVersion: 1 as const, html: '<p><span data-latex="x^{2}"></span></p>', plainText: '' }
    expect(richContentSearchText(formula)).toBe('x^{2}')
    expect(hasMeaningfulRichContent(formula)).toBe(true)
    expect(hasMeaningfulRichContent({ ...formula, html: '<p><img data-resource-id="image-1"></p>' })).toBe(true)
    expect(hasMeaningfulRichContent({ ...formula, html: '<table><tr><td><br></td></tr></table>' })).toBe(false)
    expect(hasMeaningfulRichContent(emptyRichContent())).toBe(false)
  })
})
