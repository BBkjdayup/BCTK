import { describe, expect, it } from 'vitest'
import type { RichContent } from '../types/domain'
import { plainTextRichContent } from './richContent'
import { recognizeLabeledQuestionDocument } from './documentTemplateRecognition'
import { DEFAULT_DOCUMENT_ENTRY_TEMPLATE_CONFIG } from './documentEntryTemplates'

function documentFromLines(lines: string[]): RichContent {
  return {
    schemaVersion: 2,
    editor: 'tiptap',
    editorVersion: '3.28.0',
    document: {
      type: 'doc',
      content: lines.map((text) => ({
        type: 'paragraph',
        content: text ? [{ type: 'text', text }] : undefined,
      })),
    },
    html: lines.map((text) => `<p>${text}</p>`).join(''),
    plainText: lines.join('\n'),
  }
}

describe('labeled document question recognition', () => {
  it('recognizes manually typed labels and multiple question types', () => {
    const result = recognizeLabeledQuestionDocument(documentFromLines([
      '题型：单选题',
      '题目：OSI 模型共有多少层？',
      'A. 5 层',
      'B. 7 层',
      'C. 8 层',
      'D. 9 层',
      '答案：B',
      '解析：OSI 参考模型共有七层。',
      '题型：简答题',
      '题目：简述交换机的主要作用。',
      '答案：根据 MAC 地址表转发以太网帧。',
      '解析：答案包含学习、转发和过滤等关键词。',
    ]))

    expect(result.questions).toHaveLength(2)
    expect(result.questions[0]?.type).toBe('single_choice')
    expect(result.questions[0]?.draft.stem.plainText).toBe('OSI 模型共有多少层？')
    expect(result.questions[0]?.draft.options.map((option) => option.content.plainText))
      .toEqual(['5 层', '7 层', '8 层', '9 层'])
    expect(result.questions[0]?.draft.answer.plainText).toBe('B')
    expect(result.questions[1]?.type).toBe('short_answer')
    expect(result.questions[1]?.draft.options).toHaveLength(0)
    expect(result.questions.every((question) => question.issues.length === 0)).toBe(true)
  })

  it('recognizes numbered question starts alongside fixed stem markers and strips numbers by default', () => {
    const result = recognizeLabeledQuestionDocument(documentFromLines([
      '题型：单选题',
      '1. OSI 模型共有多少层？',
      'A. 5 层',
      'B. 7 层',
      '答案：B',
      '解析：OSI 参考模型共有七层。',
      '题型：简答题',
      '２． 简述交换机的主要作用。',
      '答案：根据 MAC 地址表转发以太网帧。',
      '解析：1.2 GHz 是说明内容中的小数，不应拆成新题。',
    ]))

    expect(result.questions).toHaveLength(2)
    expect(result.questions.map((question) => question.draft.stem.plainText)).toEqual([
      'OSI 模型共有多少层？',
      '简述交换机的主要作用。',
    ])
    expect(result.questions[1]?.draft.explanation.plainText).toContain('1.2 GHz')
    expect(result.questions.every((question) => (
      !question.issues.some((issue) => issue.severity === 'error')
    ))).toBe(true)
  })

  it('also strips a number after the configured fixed stem marker', () => {
    const result = recognizeLabeledQuestionDocument(documentFromLines([
      '题目：3、交换机根据什么地址转发数据帧？',
      'A. IP 地址',
      'B. MAC 地址',
      '答案：B',
    ]))

    expect(result.questions).toHaveLength(1)
    expect(result.questions[0]?.draft.stem.plainText).toBe('交换机根据什么地址转发数据帧？')
  })

  it('can retain recognized question numbers when the template disables stripping', () => {
    const result = recognizeLabeledQuestionDocument(documentFromLines([
      '1. 需要保留题号的题干。',
      '答案：示例答案',
    ]), {
      ...DEFAULT_DOCUMENT_ENTRY_TEMPLATE_CONFIG,
      stripRecognizedQuestionNumbers: false,
    })

    expect(result.questions).toHaveLength(1)
    expect(result.questions[0]?.draft.stem.plainText).toBe('1. 需要保留题号的题干。')
  })

  it('keeps multiline field content until the next manually typed label', () => {
    const result = recognizeLabeledQuestionDocument(documentFromLines([
      '题型：案例分析题',
      '题目：阅读以下网络故障情境。',
      '某交换机端口持续发生抖动。',
      '请分析可能原因并提出处理步骤。',
      '答案：先检查物理链路。',
      '再检查端口协商和日志。',
      '解析：按定位顺序给分。',
    ]))

    expect(result.questions[0]?.draft.stem.plainText)
      .toBe('阅读以下网络故障情境。\n某交换机端口持续发生抖动。\n请分析可能原因并提出处理步骤。')
    expect(result.questions[0]?.draft.answer.plainText)
      .toBe('先检查物理链路。\n再检查端口协商和日志。')
  })

  it('recognizes labels separated by soft line breaks inside one paragraph', () => {
    const lines = [
      '题型：单选题',
      '题目：交换机根据什么地址转发以太网帧？',
      'A. IP 地址',
      'B. MAC 地址',
      '答案：B',
      '解析：交换机学习并查询 MAC 地址表。',
    ]
    const content = lines.flatMap((text, index) => [
      { type: 'text', text },
      ...(index < lines.length - 1 ? [{ type: 'hardBreak' }] : []),
    ])
    const source: RichContent = {
      schemaVersion: 2,
      editor: 'tiptap',
      editorVersion: '3.28.0',
      document: {
        type: 'doc',
        content: [{ type: 'paragraph', content }],
      },
      html: `<p>${lines.join('<br>')}</p>`,
      plainText: lines.join('\n'),
    }

    const result = recognizeLabeledQuestionDocument(source)

    expect(result.questions).toHaveLength(1)
    expect(result.sourceBlockCount).toBe(lines.length)
    expect(result.questions[0]?.draft.stem.plainText)
      .toBe('交换机根据什么地址转发以太网帧？')
    expect(result.questions[0]?.draft.options.map((option) => option.content.plainText))
      .toEqual(['IP 地址', 'MAC 地址'])
    expect(result.questions[0]?.draft.answer.plainText).toBe('B')
    expect(result.questions[0]?.issues).toHaveLength(0)
  })

  it('reports incomplete placeholders without discarding the recognized item', () => {
    const result = recognizeLabeledQuestionDocument(documentFromLines([
      '题型：多选题',
      '题目：请选择正确项目。',
      'A. 项目一',
      '答案：',
    ]))

    expect(result.questions).toHaveLength(1)
    expect(result.questions[0]?.issues.map((entry) => entry.severity)).toContain('error')
    expect(result.questions[0]?.issues.map((entry) => entry.message).join('；')).toContain('答案')
    expect(result.questions[0]?.type).toBe('multiple_choice')
    expect(result.questions[0]?.issues.map((entry) => entry.message).join('；')).toContain('选项不足两个')
  })

  it('warns and infers a type when the type label is omitted', () => {
    const result = recognizeLabeledQuestionDocument(documentFromLines([
      '题目：以下哪项正确？',
      'A. 第一项',
      'B. 第二项',
      '答案：A',
    ]))

    expect(result.questions[0]?.type).toBe('single_choice')
    expect(result.questions[0]?.issues[0]?.severity).toBe('warning')
  })

  it('allows an empty type label and infers multiple choice from a strict multi-letter answer', () => {
    const result = recognizeLabeledQuestionDocument(documentFromLines([
      '题型：',
      '题目：请选择正确项目。',
      'A. 第一项',
      'B. 第二项',
      'C. 第三项',
      '答案：A、C',
      '解析：',
    ]))

    expect(result.questions).toHaveLength(1)
    expect(result.questions[0]?.type).toBe('multiple_choice')
    expect(result.questions[0]?.issues.some((entry) => entry.severity === 'error')).toBe(false)
    expect(result.questions[0]?.issues.map((entry) => entry.message).join('；')).toContain('自动识别为“多选题”')
  })

  it('does not treat ordinary answer text containing A-H letters as a multiple-choice answer', () => {
    const result = recognizeLabeledQuestionDocument(documentFromLines([
      '题目：访问控制列表的英文缩写是什么？',
      'A. ACL',
      'B. VLAN',
      '答案：采用 ACL 实现访问控制',
    ]))

    expect(result.questions[0]?.type).toBe('single_choice')
    expect(result.questions[0]?.issues.map((entry) => entry.message).join('；'))
      .toContain('答案不是纯选项编号格式')
  })

  it('treats a question with only a stem and answer as short answer', () => {
    const result = recognizeLabeledQuestionDocument(documentFromLines([
      '题型：',
      '题目：简述交换机的主要作用。',
      'A. ',
      'B. ',
      '答案：根据 MAC 地址表转发数据帧。',
      '解析：',
    ]))

    expect(result.questions[0]?.type).toBe('short_answer')
    expect(result.questions[0]?.draft.options).toEqual([])
    expect(result.questions[0]?.issues.some((entry) => entry.severity === 'error')).toBe(false)
  })

  it('warns instead of guessing choice type when an option-like answer has no options', () => {
    const result = recognizeLabeledQuestionDocument(documentFromLines([
      '题目：以下哪项正确？',
      '答案：A',
    ]))

    expect(result.questions[0]?.type).toBe('short_answer')
    expect(result.questions[0]?.issues.map((entry) => entry.message).join('；'))
      .toContain('答案看起来像选择题编号')
  })

  it('keeps an incomplete explicit choice type and blocks saving until options are fixed', () => {
    const result = recognizeLabeledQuestionDocument(documentFromLines([
      '题型：单选题',
      '题目：没有提供选项的题目。',
      '答案：参考答案内容',
    ]))

    expect(result.questions[0]?.type).toBe('single_choice')
    expect(result.questions[0]?.draft.options).toEqual([])
    expect(result.questions[0]?.issues.some((entry) => entry.severity === 'error')).toBe(true)
    expect(result.questions[0]?.issues.map((entry) => entry.message).join('；'))
      .toContain('选项不足两个')
  })

  it('preserves a specifically labeled blank option for correction in the review page', () => {
    const result = recognizeLabeledQuestionDocument(documentFromLines([
      '题型：单选题',
      '题目：请选择正确项目。',
      'A. 项目一',
      'B. ',
      '答案：A',
    ]))

    expect(result.questions[0]?.type).toBe('single_choice')
    expect(result.questions[0]?.draft.options.map((option) => option.content.plainText))
      .toEqual(['项目一', ''])
    expect(result.questions[0]?.issues.map((entry) => entry.message).join('；'))
      .toContain('选项 B 没有内容')
  })

  it('normalizes an inferred true-false answer without a short-answer warning', () => {
    const result = recognizeLabeledQuestionDocument(documentFromLines([
      '题目：交换机工作在数据链路层。',
      '答案：F',
    ]))

    expect(result.questions[0]?.type).toBe('true_false')
    expect(result.questions[0]?.draft.options.map((option) => option.content.plainText))
      .toEqual(['正确', '错误'])
    expect(result.questions[0]?.draft.answer.plainText).toBe('B')
    expect(result.questions[0]?.issues.map((entry) => entry.message).join('；'))
      .not.toContain('按简答题处理')
  })

  it('does not turn unrelated prose into a question', () => {
    const result = recognizeLabeledQuestionDocument(plainTextRichContent('这是一段没有模板标签的普通文字。'))

    expect(result.questions).toHaveLength(0)
    expect(result.globalIssues.some((entry) => entry.severity === 'error')).toBe(true)
  })

  it('recognizes a custom literal template and bracketed options', () => {
    const config = {
      ...DEFAULT_DOCUMENT_ENTRY_TEMPLATE_CONFIG,
      typeMarker: '【类型】',
      stemMarker: '【题干】',
      answerMarker: '【正确答案】',
      explanationMarker: '【解题说明】',
      optionStyle: 'letter_brackets' as const,
      questionSeparator: '下一题',
      compatibleDefaultMarkers: false,
    }
    const result = recognizeLabeledQuestionDocument(documentFromLines([
      '【类型】单选题',
      '【题干】交换机根据什么地址转发数据帧？',
      '【A】IP 地址',
      '【B】MAC 地址',
      '【正确答案】B',
      '【解题说明】查询 MAC 地址表。',
    ]), config)

    expect(result.questions).toHaveLength(1)
    expect(result.questions[0]?.draft.options.map((option) => option.content.plainText))
      .toEqual(['IP 地址', 'MAC 地址'])
    expect(result.questions[0]?.draft.answer.plainText).toBe('B')
    expect(result.questions[0]?.issues).toHaveLength(0)
  })

  it('can keep default aliases enabled for a custom template', () => {
    const result = recognizeLabeledQuestionDocument(documentFromLines([
      '题型：简答题',
      '题目：说明模板兼容功能。',
      '答案：可以继续识别系统默认标记。',
    ]), {
      ...DEFAULT_DOCUMENT_ENTRY_TEMPLATE_CONFIG,
      typeMarker: '类型→',
      stemMarker: '问题→',
      answerMarker: '结果→',
      explanationMarker: '说明→',
      compatibleDefaultMarkers: true,
    })

    expect(result.questions).toHaveLength(1)
    expect(result.questions[0]?.draft.stem.plainText).toBe('说明模板兼容功能。')
  })
})
