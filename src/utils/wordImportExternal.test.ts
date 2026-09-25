import { readFileSync } from 'node:fs'
import katex from 'katex'
import { describe, expect, it } from 'vitest'
import type { BeginWordImportResult, QuestionType, QuestionTypeDefinition } from '../types/domain'
import { buildWordAnalysisLines } from './wordImportAnalysis'
import { splitWordImportFormulaSegments } from './wordImportFormula'
import { buildWordImportRichContent } from './wordImportRichContent'
import { recognizeWordQuestions, type RecognizedWordQuestion } from './wordImportRecognition'
import { fallbackQuestionTypes } from './questionTypes'

const fixturePath = process.env.ZHITIKU_MATHTYPE_PREPARED_JSON?.trim()
const questionBankExportFixturePath = process.env.ZHITIKU_QUESTION_BANK_EXPORT_ANALYSIS_JSON?.trim()
const annotatedPhysicsFixturePath = process.env.ZHITIKU_ANNOTATED_PHYSICS_ANALYSIS_JSON?.trim()
const annotatedPhysicsPreparedFixturePath = process.env.ZHITIKU_ANNOTATED_PHYSICS_PREPARED_JSON?.trim()
const hundredQuestionsPreparedFixturePath = process.env.ZHITIKU_HUNDRED_QUESTIONS_PREPARED_JSON?.trim()

function typeCount(questions: readonly RecognizedWordQuestion[], type: QuestionType) {
  return questions.filter((question) => question.type === type).length
}

function lineText(question: RecognizedWordQuestion) {
  return question.stemLines.map((line) => line.text).join('\n')
}

describe.runIf(Boolean(hundredQuestionsPreparedFixturePath))('100-question Word import regression', () => {
  it('keeps every field, image and formula in the 83 fill-in questions', () => {
    const fixture = JSON.parse(readFileSync(hundredQuestionsPreparedFixturePath!, 'utf8')) as Pick<
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
    const converted = fixture.images.filter((image) => image.originalFilename?.endsWith('.wmf'))
    const assignedLines = questions.flatMap((question) => [
      ...question.stemLines,
      ...question.options.flat(),
      ...question.answerLines,
      ...question.explanationLines,
    ])
    const assignedImageIds = new Set(assignedLines.flatMap((line) => line.images.map((image) => image.nodeId)))
    const formulasByNodeId = new Map(fixture.formulas.map((formula) => [formula.nodeId, formula]))
    const assignedFormulaIds = new Set(assignedLines.flatMap((line) => (
      splitWordImportFormulaSegments(line.text, formulasByNodeId)
        .filter((segment) => segment.kind === 'formula')
        .map((segment) => segment.formula.nodeId)
    )))

    expect(fixture.analysis.isValid).toBe(true)
    expect(fixture.images).toHaveLength(76)
    expect(fixture.formulas).toHaveLength(268)
    expect(converted).toHaveLength(2)
    expect(converted.every((image) => image.mimeType === 'image/png')).toBe(true)
    expect(converted.every((image) => assignedImageIds.has(image.nodeId))).toBe(true)
    expect(assignedImageIds.size).toBe(fixture.images.length)
    expect(assignedFormulaIds.size).toBe(fixture.formulas.length)
    // The source contains 83 【答案】 blocks despite its "100题" filename.
    expect(fixture.analysis.paragraphs.filter((paragraph) => paragraph.text.includes('【答案】')))
      .toHaveLength(83)
    expect(questions).toHaveLength(83)
    const numberedParagraphs = fixture.analysis.paragraphs.flatMap((paragraph, index) => (
      /^\s*\d{1,4}\s*[.．、](?!\d)/u.test(paragraph.text) ? [index] : []
    ))
    expect(numberedParagraphs).toHaveLength(83)
    expect(questions.map((question) => question.stemLines[0]?.paragraphIndex)).toEqual(numberedParagraphs)
    expect(typeCount(questions, 'fill_blank')).toBe(83)
    expect(questions.every((question) => question.answerLines.length && question.explanationLines.length)).toBe(true)
    expect(questions.every((question) => question.options.length === 0)).toBe(true)
    expect(lineText(questions[0]!)).toContain('（a/b）端移')
    expect(lineText(questions[1]!)).toContain('A、C两点')
    expect(lineText(questions[32]!)).toContain('A．转轴加润滑油')
    expect(lineText(questions[81]!)).toContain('（6）若跳绳者')
    expect(lineText(questions[82]!)).toContain('A、B位置')
  })
})

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

describe.runIf(Boolean(annotatedPhysicsFixturePath))('annotated physics Word import regression', () => {
  it('keeps per-question bracketed answers and details out of stems and options', () => {
    const analysis = JSON.parse(
      readFileSync(annotatedPhysicsFixturePath!, 'utf8'),
    ) as BeginWordImportResult['analysis']
    const lines = buildWordAnalysisLines(analysis, [], [], [])
    const questions = recognizeWordQuestions(lines, fallbackQuestionTypes)
    const choices = questions.filter((question) => question.type === 'single_choice')
    const fieldMarkerPattern = /【(?:答案|解析|详解|小问\d+详解)】/u

    expect(questions).toHaveLength(30)
    expect(typeCount(questions, 'single_choice')).toBe(12)
    expect(typeCount(questions, 'fill_blank')).toBe(12)
    expect(typeCount(questions, 'short_answer')).toBe(6)
    expect(questions.every((question) => question.answerLines.length > 0)).toBe(true)
    expect(questions.every((question) => question.explanationLines.length > 0)).toBe(true)
    expect(choices.every((question) => question.options.length === 4)).toBe(true)
    expect(questions.every((question) => (
      !fieldMarkerPattern.test(question.stemLines.map((line) => line.text).join('\n'))
      && !fieldMarkerPattern.test(question.options.flat().map((line) => line.text).join('\n'))
    ))).toBe(true)
    expect(questions[0]?.answerLines.map((line) => line.text)).toEqual(['A'])
    expect(questions.find((question) => question.sourceNumber === 16)
      ?.explanationLines.map((line) => line.text)).toEqual(expect.arrayContaining([
        '小问1：',
        '小问2：',
      ]))
  })
})

describe.runIf(Boolean(annotatedPhysicsPreparedFixturePath))('annotated physics formula regression', () => {
  it('keeps formulas attached to recognized question fields', () => {
    const fixture = JSON.parse(
      readFileSync(annotatedPhysicsPreparedFixturePath!, 'utf8'),
    ) as Pick<BeginWordImportResult, 'analysis' | 'images' | 'formulas' | 'tables'>
    const lines = buildWordAnalysisLines(
      fixture.analysis,
      fixture.images,
      fixture.formulas,
      fixture.tables,
    )
    const questions = recognizeWordQuestions(lines, fallbackQuestionTypes)
    const assignedNodeIds: string[] = []
    const appendFormulaNodeIds = (html: string) => {
      for (const match of html.matchAll(/<span\b[^>]*\bdata-node-id="([^"]+)"[^>]*>/giu)) {
        if (match[1]) assignedNodeIds.push(match[1])
      }
    }

    for (const question of questions) {
      const resourceRefs: never[] = []
      appendFormulaNodeIds(buildWordImportRichContent(
        question.stemLines,
        fixture.formulas,
        resourceRefs,
        'stem',
      ).html)
      for (const option of question.options) {
        appendFormulaNodeIds(buildWordImportRichContent(
          option,
          fixture.formulas,
          resourceRefs,
          'option',
        ).html)
      }
      appendFormulaNodeIds(buildWordImportRichContent(
        question.answerLines,
        fixture.formulas,
        resourceRefs,
        'answer',
      ).html)
      appendFormulaNodeIds(buildWordImportRichContent(
        question.explanationLines,
        fixture.formulas,
        resourceRefs,
        'explanation',
      ).html)
    }

    const assigned = new Set(assignedNodeIds)
    const missing = fixture.formulas.filter((formula) => !assigned.has(formula.nodeId))
    const renderingErrors = fixture.formulas.flatMap((formula) => {
      try {
        katex.renderToString(formula.latex, {
          displayMode: false,
          throwOnError: true,
          strict: false,
          trust: false,
          maxExpand: 1_000,
        })
        return []
      } catch (reason) {
        return [{
          paragraphIndex: formula.paragraphIndex,
          latex: formula.latex,
          error: reason instanceof Error ? reason.message : String(reason),
        }]
      }
    })
    console.info(JSON.stringify({
      questionCount: questions.length,
      sourceFormulaCount: fixture.formulas.length,
      assignedFormulaCount: assigned.size,
      duplicateFormulaCount: assignedNodeIds.length - assigned.size,
      renderingErrorCount: renderingErrors.length,
      missing: missing.map((formula) => ({
        paragraphIndex: formula.paragraphIndex,
        latex: formula.latex,
      })),
    }, null, 2))

    expect(questions).toHaveLength(30)
    expect(fixture.formulas).toHaveLength(199)
    expect(assignedNodeIds.length).toBe(assigned.size)
    expect(assigned.size).toBe(fixture.formulas.length)
    expect(renderingErrors).toEqual([])
  })
})
