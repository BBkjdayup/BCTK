import { describe, expect, it } from 'vitest'
import type { DocxImageOccurrence, QuestionType, QuestionTypeDefinition } from '../types/domain'
import { fallbackQuestionTypes } from './questionTypes'
import {
  recognizeWordQuestions,
  type WordAnalysisLine,
  type WordAnalysisTable,
} from './wordImportRecognition'

function lineBuilder() {
  const lines: WordAnalysisLine[] = []
  let paragraphIndex = 0
  return {
    add(text: string, images: DocxImageOccurrence[] = []) {
      lines.push({ paragraphIndex, text, images })
      paragraphIndex += 1
    },
    lines,
  }
}

function typeCount(questions: ReturnType<typeof recognizeWordQuestions>, type: QuestionType) {
  return questions.filter((question) => question.type === type).length
}

function imageOccurrence(
  id: string,
  paragraphIndex: number,
  textCharOffset: number,
): DocxImageOccurrence {
  return {
    nodeId: `node-${id}`,
    resourceId: `resource-${id}`,
    paragraphIndex,
    textCharOffset,
    originalFilename: `${id}.png`,
    mimeType: 'image/png',
    byteSize: 10,
    widthPx: 100,
    heightPx: 80,
  }
}

function firstPaperLines() {
  const builder = lineBuilder()
  builder.add('《网络设备安装与调试》期末考试备选题库')
  builder.add('一、单项选择题（备选100题）')
  builder.add('说明：每题只有一个正确答案。')
  for (let number = 1; number <= 100; number += 1) {
    builder.add(`${number}. 选择题 ${number}？ A. 甲 B. 乙 C. 丙 D. 丁`)
  }
  builder.add('二、简答题（备选24题）')
  for (let number = 1; number <= 24; number += 1) {
    builder.add(`${number}. 简答题 ${number}？`)
  }
  builder.add('三、综合应用题（备选8题）')
  for (let number = 1; number <= 8; number += 1) {
    builder.add(`${number}. 网络故障案例 ${number}`)
    builder.add(`情境：这是第 ${number} 个网络情境。`)
    builder.add('（1）分析故障原因。')
    builder.add('(2) 提出解决方案。')
  }

  builder.add('参考答案与评分要点')
  builder.add('一、单项选择题答案')
  for (let number = 1; number <= 100; number += 1) builder.add(`${number}.A`)
  builder.add('二、简答题参考要点')
  for (let number = 1; number <= 24; number += 1) {
    builder.add(`${number}. 简答题 ${number}？`)
    builder.add(`参考要点：简答题 ${number} 的答案。`)
  }
  builder.add('三、综合应用题参考要点')
  for (let number = 1; number <= 8; number += 1) {
    builder.add(`${number}. 网络故障案例 ${number}`)
    builder.add(`参考要点：案例 ${number} 的评分要点。`)
  }
  return builder.lines
}

function secondPaperLines() {
  const builder = lineBuilder()
  builder.add('一、选择题：本题共50个小题，每小题2分。')
  for (let number = 1; number <= 50; number += 1) {
    builder.add(`${number}. 设备安装选择题 ${number}？`)
    builder.add('A. 甲')
    builder.add('B. 乙')
    builder.add('C. 丙')
    builder.add('D. 丁')
  }
  builder.add('二、简答题：本题共12个小题，共60分。')
  for (let number = 1; number <= 12; number += 1) builder.add(`${number}. 简述知识点 ${number}。`)
  builder.add('三、案例分析题（本题共4个小题，共40分）')
  for (let number = 1; number <= 4; number += 1) {
    builder.add(`${number}. 案例分析 ${number}`)
    builder.add('情境：交换机出现异常。')
    builder.add('（1）定位问题。')
    builder.add('（2）说明处理方法。')
  }
  return builder.lines
}

function thirdPaperLines() {
  const builder = lineBuilder()
  builder.add('一、选择题：本题共25个小题，每小题2分。')
  for (let number = 1; number <= 25; number += 1) {
    builder.add(`${number}. 网络技术选择题 ${number}？`)
    builder.add('A. 甲                 B. 乙')
    builder.add('C. 丙                 D. 丁')
  }
  builder.add('二、简答题：本题共4个小题，共20分。')
  for (let number = 1; number <= 4; number += 1) builder.add(`${number}. 简答题 ${number}。`)
  builder.add('三、案例分析题（本题共2个小题，共15分）')
  for (let number = 1; number <= 2; number += 1) {
    builder.add(`${number}. 案例 ${number}`)
    builder.add('（1）写出处理步骤。')
    builder.add('(2) 解释处理依据。')
  }
  builder.add('四、综合应用题（本题共2个小题，共15分）')
  builder.add('主机A的IP地址为210.35.7.92，子网掩码为255.255.255.224。')
  builder.add('主机A的网络地址是多少？')
  builder.add('若主机B想与主机A通信，主机B的IP地址范围是多少？')
  builder.add('2. 二进制IP地址转换与子网划分。')
  builder.add('(1) 请把二进制地址转换成十进制。')
  builder.add('（2）请判断该IP地址的类别。')
  return builder.lines
}

describe('Word question recognition compatibility', () => {
  it('round-trips the exported 33-question bank without treating answers or explanations as questions', () => {
    const builder = lineBuilder()
    builder.add('一、网设 / 1 / 单选题：本题共26个小题，每小题      分。共      分。')
    for (let number = 1; number <= 26; number += 1) {
      builder.add(`${number}. 选择题 ${number}？`)
      builder.add('A. 甲')
      builder.add('B. 乙')
      builder.add('C. 丙')
      builder.add('D. 丁')
    }
    builder.add('二、网设 / 1 / 简答题：本题共4个小题，每小题      分。共      分。')
    for (let number = 27; number <= 30; number += 1) builder.add(`${number}. 简答题 ${number}？`)
    builder.add('三、网设 / 1 / 综合应用题：本题共1个小题，每小题      分。共      分。')
    builder.add('31. 综合应用题 31？')
    builder.add('四、网设 / 1 / 案例分析题：本题共2个小题，每小题      分。共      分。')
    builder.add('32. 案例分析题 32？')
    builder.add('33. 案例分析题 33？')
    builder.add('参考答案')
    for (let number = 1; number <= 33; number += 1) builder.add(`${number}. 答案 ${number}`)
    builder.add('题目解析')
    for (let number = 1; number <= 33; number += 1) builder.add(`${number}. 解析内容 ${number}`)

    const questions = recognizeWordQuestions(builder.lines)

    expect(questions).toHaveLength(33)
    expect(typeCount(questions, 'single_choice')).toBe(26)
    expect(typeCount(questions, 'short_answer')).toBe(7)
    expect(questions.every((question) => question.answerLines.length > 0)).toBe(true)
    expect(questions.every((question) => question.explanationLines.length > 0)).toBe(true)
    expect(questions[26]?.answerLines[0]?.text).toBe('答案 27')
    expect(questions[32]?.explanationLines[0]?.text).toBe('解析内容 33')
  })

  it('recognizes the 100 + 24 + 8 question bank and fills all local-section answers', () => {
    const questions = recognizeWordQuestions(firstPaperLines())

    expect(questions).toHaveLength(132)
    expect(typeCount(questions, 'single_choice')).toBe(100)
    expect(typeCount(questions, 'short_answer')).toBe(32)
    expect(questions.filter((question) => question.type === 'single_choice' && question.answerLines.length)).toHaveLength(100)
    expect(questions.filter((question) => question.type === 'short_answer' && question.answerLines.length)).toHaveLength(32)
    expect(questions.find((question) => question.section === 'short_answer' && question.sourceNumber === 1)
      ?.answerLines[0]?.text).toBe('简答题 1 的答案。')
    expect(questions.find((question) => question.section === 'application' && question.sourceNumber === 1)
      ?.answerLines[0]?.text).toBe('案例 1 的评分要点。')
  })

  it('recognizes all 50 + 12 + 4 questions even when the paper has no answers', () => {
    const questions = recognizeWordQuestions(secondPaperLines())

    expect(questions).toHaveLength(66)
    expect(typeCount(questions, 'single_choice')).toBe(50)
    expect(typeCount(questions, 'short_answer')).toBe(16)
    expect(questions.every((question) => question.answerLines.length === 0)).toBe(true)
  })

  it('recognizes 25 + 4 + 2 + 2 and infers the first auto-numbered application question', () => {
    const questions = recognizeWordQuestions(thirdPaperLines())

    expect(questions).toHaveLength(33)
    expect(typeCount(questions, 'single_choice')).toBe(25)
    expect(typeCount(questions, 'short_answer')).toBe(8)
    const applications = questions.filter((question) => question.section === 'application')
    expect(applications.map((question) => question.sourceNumber)).toEqual([1, 2])
    expect(applications[0]?.stemLines.map((line) => line.text).join('\n')).toContain('主机A的IP地址')
    expect(applications[1]?.stemLines.map((line) => line.text)).toContain('(1) 请把二进制地址转换成十进制。')
    expect(applications[1]?.stemLines.map((line) => line.text)).toContain('（2）请判断该IP地址的类别。')
    expect(questions[0]?.options.map((option) => option[0]?.text)).toEqual(['甲', '乙', '丙', '丁'])
  })

  it('keeps a picture in the stem and creates four reviewable option placeholders', () => {
    const builder = lineBuilder()
    const image: DocxImageOccurrence = {
      nodeId: 'node-1',
      resourceId: 'resource-1',
      paragraphIndex: 2,
      textCharOffset: 0,
      originalFilename: 'options.png',
      mimeType: 'image/png',
      byteSize: 10,
      widthPx: 100,
      heightPx: 80,
    }
    builder.add('一、选择题（共1题）')
    builder.add('第1题 下图中正确的网络拓扑是？')
    builder.add('', [image])

    const [question] = recognizeWordQuestions(builder.lines)

    expect(question?.stemLines.some((line) => line.images[0]?.resourceId === 'resource-1')).toBe(true)
    expect(question?.options.map((option) => option[0]?.text)).toEqual([
      '见题干图片中的 A 项',
      '见题干图片中的 B 项',
      '见题干图片中的 C 项',
      '见题干图片中的 D 项',
    ])
  })

  it('assigns the four independently embedded graph images to A, B, C and D', () => {
    const builder = lineBuilder()
    builder.add('一、选择题（共20题）')
    builder.add('9. 函数图像是（    ）')
    builder.add('A．B．', [
      imageOccurrence('a', 2, 2),
      imageOccurrence('b', 2, 4),
    ])
    builder.add('C．D．', [
      imageOccurrence('c', 3, 2),
      imageOccurrence('d', 3, 4),
    ])

    const [question] = recognizeWordQuestions(builder.lines)

    expect(question?.stemLines.flatMap((line) => line.images)).toHaveLength(0)
    expect(
      question?.options.map((option) => (
        option.flatMap((line) => line.images).map((image) => image.resourceId)
      )),
    ).toEqual([
      ['resource-a'],
      ['resource-b'],
      ['resource-c'],
      ['resource-d'],
    ])
  })

  it('separates the real math exam fill-blank and solution section headings', () => {
    const builder = lineBuilder()
    builder.add('一、选择题(本大题共20小题)')
    builder.add('20. 已知圆，选择正确答案。')
    builder.add('A．甲 B．乙 C．丙 D．丁')
    builder.add('二、填空题（共5个小题，每小题4分，共20分）')
    builder.add('21. 第一空是_________．')
    builder.add('25. 第五空是_________．')
    builder.add('三、解答题（5个小题，共40分）')
    builder.add('26. 已知三角形ABC，求直线方程。')
    builder.add('30. 已知两点，求中垂线方程。')

    const questions = recognizeWordQuestions(builder.lines)

    expect(questions.map((question) => [question.sourceNumber, question.type])).toEqual([
      [20, 'single_choice'],
      [21, 'fill_blank'],
      [25, 'fill_blank'],
      [26, 'short_answer'],
      [30, 'short_answer'],
    ])
    expect(questions[0]?.options[3]?.map((line) => line.text).join('')).toBe('丁')
    expect(questions[2]?.stemLines.map((line) => line.text).join('')).not.toContain('解答题')
  })

  it('keeps the question 28 two-row six-column Word table inside the stem', () => {
    const table: WordAnalysisTable = {
      rows: [
        Array.from({ length: 6 }, (_, index) => ({
          lines: [{
            paragraphIndex: 68 + index,
            text: index === 0 ? 'x' : '',
            images: [],
          }],
        })),
        Array.from({ length: 6 }, (_, index) => ({
          lines: [{
            paragraphIndex: 74 + index,
            text: index === 0 ? 'y' : '',
            images: [],
          }],
        })),
      ],
    }
    const lines: WordAnalysisLine[] = [
      { paragraphIndex: 56, text: '三、解答题（5个小题，共40分）', images: [] },
      { paragraphIndex: 66, text: '28. 已知函数，完成下列问题。', images: [] },
      { paragraphIndex: 67, text: '（1）利用五点法作图。', images: [] },
      { paragraphIndex: 68, text: '', images: [], tables: [table] },
      { paragraphIndex: 80, text: '（2）求函数的值域。', images: [] },
    ]

    const [question] = recognizeWordQuestions(lines)

    expect(question?.sourceNumber).toBe(28)
    expect(question?.type).toBe('short_answer')
    expect(question?.stemLines.flatMap((line) => line.tables ?? [])).toEqual([table])
    expect(question?.stemLines.map((line) => line.text)).toEqual([
      '已知函数，完成下列问题。',
      '（1）利用五点法作图。',
      '',
      '（2）求函数的值域。',
    ])
  })

  it('retains compatibility without section headings and never promotes parenthesized subquestions', () => {
    const builder = lineBuilder()
    builder.add('第1题 以下说法正确的是？ A. 甲 B. 乙 C. 丙 D. 丁')
    builder.add('2．案例分析：阅读材料并回答问题。')
    builder.add('（1）分析原因。')
    builder.add('(2) 给出方案。')

    const questions = recognizeWordQuestions(builder.lines)

    expect(questions).toHaveLength(2)
    expect(questions.map((question) => question.type)).toEqual(['single_choice', 'short_answer'])
    expect(questions[1]?.stemLines.map((line) => line.text)).toContain('（1）分析原因。')
    expect(questions[1]?.stemLines.map((line) => line.text)).toContain('(2) 给出方案。')
  })

  it('recognizes the built-in judgment type', () => {
    const builder = lineBuilder()
    builder.add('一、判断题（共2小题）')
    builder.add('1. TCP 是面向连接的协议。（ ）')
    builder.add('2. UDP 保证可靠交付。（ ）')

    const recognized = recognizeWordQuestions(builder.lines, fallbackQuestionTypes)
    expect(recognized).toHaveLength(2)
    expect(recognized.every((question) => question.type === 'true_false')).toBe(true)
    expect(recognized.every((question) => question.unknownTypeName == null)).toBe(true)
    expect(recognized.every((question) => question.unknownTypeName === null)).toBe(true)
  })

  it('uses the final type segment in an exported classification heading and restores blank explanations', () => {
    const builder = lineBuilder()
    builder.add('一、网设 / 第一章 / 单选题：本题共2个小题')
    builder.add('1. 第一题？')
    builder.add('A. 甲')
    builder.add('B. 乙')
    builder.add('2. 第二题？')
    builder.add('A. 丙')
    builder.add('B. 丁')
    builder.add('参考答案')
    builder.add('1. A')
    builder.add('2. B')
    builder.add('题目解析')
    builder.add('1. （未填写）')
    builder.add('2. 真实解析')

    // A previous parser could accidentally create this composite custom type.
    // Its presence must not override the actual trailing "单选题" segment.
    const accidentalCompositeType: QuestionTypeDefinition = {
      code: 'custom_abcdefabcdefabcdefabcdefabcdefab',
      name: '网设 / 第一章 / 单选题',
      behavior: 'open_response',
      aliases: [],
      defaultOptions: [],
      isBuiltin: false,
      isEnabled: true,
      sortOrder: 70,
      questionCount: 0,
      paperItemCount: 0,
      createdAt: 1,
      updatedAt: 1,
    }
    const questions = recognizeWordQuestions(
      builder.lines,
      [...fallbackQuestionTypes, accidentalCompositeType],
    )

    expect(questions).toHaveLength(2)
    expect(questions.map((question) => question.type)).toEqual(['single_choice', 'single_choice'])
    expect(questions.map((question) => question.options.length)).toEqual([2, 2])
    expect(questions[0]?.stemLines.map((line) => line.text).join('\n')).not.toContain('A. 甲')
    expect(questions[0]?.explanationLines).toEqual([])
    expect(questions[1]?.explanationLines.map((line) => line.text)).toEqual(['真实解析'])
  })

  it('keeps per-question answers and explanations after the last option out of that option', () => {
    const builder = lineBuilder()
    builder.add('一、单项选择题(共60题).')
    builder.add('1.从投资者(业主)角度分析，工程造价是指建设一项工程预期或实际开支的()')
    builder.add('A.全部建筑安装工程费用')
    builder.add('B.建设工程总费用。')
    builder.add('C.全部固定资产投资费用。')
    builder.add('D.建设工程动态投资费用。')
    builder.add('答案:C工程总费用。考点来源:第一章第一节工程造价的基本内容。')
    builder.add('解析:从投资者(业主)角度分析，工程造价是指建设一项工程预期或实际开支的全部固定资产投资费用。')
    builder.add('从市场交易角度分析，工程造价是指在工程发承包交易活动中形成的建筑安装工程费用。')

    const [question] = recognizeWordQuestions(builder.lines)

    expect(question?.options).toHaveLength(4)
    expect(question?.options[3]?.map((line) => line.text)).toEqual(['建设工程动态投资费用。'])
    expect(question?.answerLines.map((line) => line.text)).toEqual([
      'C工程总费用。考点来源:第一章第一节工程造价的基本内容。',
    ])
    expect(question?.explanationLines.map((line) => line.text)).toEqual([
      '从投资者(业主)角度分析，工程造价是指建设一项工程预期或实际开支的全部固定资产投资费用。',
      '从市场交易角度分析，工程造价是指在工程发承包交易活动中形成的建筑安装工程费用。',
    ])
  })
})
