import { describe, expect, it } from 'vitest'
import {
  buildDocumentEntryTemplateExample,
  convertDocumentEntryMarkers,
  DEFAULT_DOCUMENT_ENTRY_TEMPLATE_CONFIG,
  validateDocumentEntryTemplateConfig,
} from './documentEntryTemplates'
import { plainTextLinesRichContent } from './richContent'

describe('document entry templates', () => {
  it('builds the gray example from the selected markers and option style', () => {
    const example = buildDocumentEntryTemplateExample({
      ...DEFAULT_DOCUMENT_ENTRY_TEMPLATE_CONFIG,
      typeMarker: '【类型】',
      stemMarker: '【题干】',
      answerMarker: '【答案】',
      explanationMarker: '【解析】',
      optionStyle: 'letter_parentheses',
    })

    expect(example).toContain('【类型】可不填，软件会根据选项和答案自动识别')
    expect(example).toContain('【题干】这里输入题目的题干')
    expect(example).toContain('（A）这里输入选项 A（选择题时填写）')
    expect(example).toContain('2. 这里输入题目的题干')
    expect(DEFAULT_DOCUMENT_ENTRY_TEMPLATE_CONFIG.recognizeQuestionNumbers).toBe(true)
    expect(DEFAULT_DOCUMENT_ENTRY_TEMPLATE_CONFIG.stripRecognizedQuestionNumbers).toBe(true)
  })

  it('rejects ambiguous or multiline recognition markers', () => {
    expect(validateDocumentEntryTemplateConfig({
      ...DEFAULT_DOCUMENT_ENTRY_TEMPLATE_CONFIG,
      stemMarker: '题型：内容',
    })).toContain('互为开头')
    expect(validateDocumentEntryTemplateConfig({
      ...DEFAULT_DOCUMENT_ENTRY_TEMPLATE_CONFIG,
      answerMarker: '答案：\n',
    })).toContain('换行')
  })

  it('converts only recognized line-start markers', () => {
    const source = plainTextLinesRichContent([
      '题型：单选题',
      '题目：题干中可以出现“答案：”而不会被替换。',
      'A. 第一项',
      'B. 第二项',
      '答案：A',
      '解析：说明',
    ])
    const target = {
      ...DEFAULT_DOCUMENT_ENTRY_TEMPLATE_CONFIG,
      typeMarker: '【类型】',
      stemMarker: '【题干】',
      answerMarker: '【正确答案】',
      explanationMarker: '【说明】',
      optionStyle: 'letter_brackets' as const,
    }
    const result = convertDocumentEntryMarkers(
      source,
      DEFAULT_DOCUMENT_ENTRY_TEMPLATE_CONFIG,
      target,
    )

    expect(result.replacementCount).toBe(6)
    expect(result.source.plainText).toContain('【题干】题干中可以出现“答案：”而不会被替换。')
    expect(result.source.plainText).toContain('【A】第一项')
    expect(result.source.plainText).toContain('【正确答案】A')
  })
})
