import { describe, expect, it } from 'vitest'
import { suggestTemplateRegion } from './templateRegion'

describe('suggestTemplateRegion', () => {
  it('finds a sample-paper range and separate style roles', () => {
    const result = suggestTemplateRegion([
      { index: 0, text: '学校期末考试' },
      { index: 1, text: '一、选择题' },
      { index: 2, text: '1. 第一题' },
      { index: 3, text: 'A. 选项甲' },
      { index: 4, text: '参考答案：A' },
      { index: 5, text: '解析：示例' },
      { index: 6, text: '命题人：张老师' },
    ])

    expect(result).toEqual({
      startParagraphIndex: 1,
      endParagraphIndex: 5,
      styleSamples: {
        sectionHeadingParagraphIndex: 1,
        questionParagraphIndex: 2,
        optionParagraphIndex: 3,
        answerParagraphIndex: 4,
        explanationParagraphIndex: 5,
      },
    })
  })

  it('falls back to the first and last meaningful paragraphs', () => {
    const result = suggestTemplateRegion([
      { index: 0, text: '' },
      { index: 1, text: '材料内容' },
      { index: 2, text: '请作答' },
    ])
    expect(result.startParagraphIndex).toBe(1)
    expect(result.endParagraphIndex).toBe(2)
    expect(result.styleSamples.questionParagraphIndex).toBe(1)
  })

  it('returns an empty selection for an empty document', () => {
    expect(suggestTemplateRegion([{ index: 0, text: '   ' }])).toEqual({
      startParagraphIndex: null,
      endParagraphIndex: null,
      styleSamples: {},
    })
  })
})
