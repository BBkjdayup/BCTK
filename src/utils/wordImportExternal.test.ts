import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import type { BeginWordImportResult, QuestionType, QuestionTypeDefinition } from '../types/domain'
import { buildWordAnalysisLines } from './wordImportAnalysis'
import { splitWordImportFormulaSegments } from './wordImportFormula'
import { recognizeWordQuestions, type RecognizedWordQuestion } from './wordImportRecognition'
import { fallbackQuestionTypes } from './questionTypes'

const fixturePath = process.env.ZHITIKU_MATHTYPE_PREPARED_JSON?.trim()
const questionBankExportFixturePath = process.env.ZHITIKU_QUESTION_BANK_EXPORT_ANALYSIS_JSON?.trim()

function typeCount(questions: readonly RecognizedWordQuestion[], type: QuestionType) {
  return questions.filter((question) => question.type === type).length
}

function lineText(question: RecognizedWordQuestion) {
  return question.stemLines.map((line) => line.text).join('\n')
}

describe.runIf(Boolean(fixturePath))('real Word import regression', () => {
  it('recognizes the supplied 30-question math exam without crossing section boundaries', () => {
    const fixture = JSON.parse(readFileSync(fixturePath!, 'utf8')) as Pick<
      BeginWordImportResult,
      'analysis' | 'images' | 'formulas' | 'tables'
    >
    const lines = buildWordAnalysisLines(
      fixture.analysis,
      fixture.images,
      fixture.formulas,
      fixture.tables,
    )
    const questions = recognizeWordQuestions(lines, fallbackQuestionTypes)
    const formulasByNodeId = new Map(fixture.formulas.map((formula) => [formula.nodeId, formula]))

    expect(questions).toHaveLength(30)
    expect(typeCount(questions, 'single_choice')).toBe(20)
    expect(typeCount(questions, 'fill_blank')).toBe(5)
    expect(typeCount(questions, 'short_answer')).toBe(5)

    const question20 = questions.find((question) => question.sourceNumber === 20)
    const question21 = questions.find((question) => question.sourceNumber === 21)
    const question25 = questions.find((question) => question.sourceNumber === 25)
    const question26 = questions.find((question) => question.sourceNumber === 26)
    const question28 = questions.find((question) => question.sourceNumber === 28)

    expect(question20?.options[question20.options.length - 1]?.map((line) => line.text).join('\n'))
      .not.toMatch(/填空题/u)
    expect(question21?.type).toBe('fill_blank')
    expect(lineText(question25!)).not.toMatch(/解答题/u)
    expect(question26?.stemLines.flatMap((line) => (
      splitWordImportFormulaSegments(line.text, formulasByNodeId)
        .filter((segment) => segment.kind === 'formula')
        .map((segment) => segment.formula.latex)
    ))).toContain(String.raw`\triangle ABC`)
    expect(question28?.stemLines.some((line) => (line.tables?.length ?? 0) > 0)).toBe(true)
    expect(question28?.stemLines.some((line) => line.images.length > 0)).toBe(true)
  })
})

describe.runIf(Boolean(questionBankExportFixturePath))('exported question bank Word import regression', () => {
  it('keeps the supplied 33-question export at 33 questions and restores round-trip fields', () => {
    const analysis = JSON.parse(
      readFileSync(questionBankExportFixturePath!, 'utf8'),
    ) as BeginWordImportResult['analysis']
    const lines = buildWordAnalysisLines(analysis, [], [], [])
    const accidentalCompositeType: QuestionTypeDefinition = {
      code: 'custom_abcdefabcdefabcdefabcdefabcdefab',
      name: '网设 / 1 / 单选题',
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
      lines,
      [...fallbackQuestionTypes, accidentalCompositeType],
    )

    expect(questions).toHaveLength(33)
    expect(typeCount(questions, 'single_choice')).toBe(26)
    expect(typeCount(questions, 'short_answer')).toBe(7)
    expect(questions.every((question) => question.answerLines.length > 0)).toBe(true)
    expect(questions.every((question) => question.explanationLines.length === 0)).toBe(true)
    expect(questions.slice(0, 26).every((question) => question.options.length === 4)).toBe(true)
    expect(questions[26]?.answerLines[0]?.text).toBe('A')
    expect(questions[32]?.explanationLines).toEqual([])
  })
})
