import { describe, expect, it } from 'vitest'
import type { Paper, WordTemplate } from '../types/domain'
import {
  inspectPaperExportContent,
  requiredTemplateAnchors,
  suggestedPaperDocxName,
  templateExportAvailability,
} from './paperExport'

function template(
  anchorNames: string[],
  packageKind: WordTemplate['packageKind'] = 'document',
): WordTemplate {
  return {
    id: 'template-1',
    name: '期末模板',
    fileName: '期末模板.docx',
    fileSha256Hex: '00'.repeat(32),
    fileByteSize: 1024,
    analysisStatus: 'ready',
    analysisSchemaVersion: 1,
    parserVersion: 'test',
    rowVersion: 1,
    createdAt: 1,
    updatedAt: 1,
    lastVerifiedAt: 1,
    isDefault: true,
    fileAvailable: true,
    regionConfigured: true,
    packageKind,
    anchors: anchorNames.map((name, index) => ({
      name,
      kind: 'paragraph_marker',
      partName: 'word/document.xml',
      replacementSpan: { start: index * 10, end: index * 10 + 1 },
      containerSpan: { start: index * 10, end: index * 10 + 2 },
      paragraphIndex: index,
    })),
    diagnostics: [],
  }
}

function paper(): Paper {
  return {
    id: 'paper-1',
    title: '测试试卷',
    compositionMode: 'manual',
    generationConfig: null,
    status: 'draft',
    exportContentMode: 'paper_only',
    subjectSummaryText: '数学',
    rowVersion: 1,
    createdAt: 1,
    updatedAt: 1,
    items: [{
      id: 'item-1',
      sourceQuestionId: 'question-1',
      position: 0,
      snapshot: {
        id: 'question-1',
        type: 'single_choice',
        stem: { schemaVersion: 1, html: '<p>题目</p>', plainText: '题目' },
        options: [{
          id: 'option-1',
          position: 0,
          content: { schemaVersion: 1, html: '<p>A</p>', plainText: 'A' },
        }],
        answer: { schemaVersion: 1, html: '<p>A</p>', plainText: 'A', sourceOoxml: '<m:oMath />' },
        explanation: { schemaVersion: 1, html: '', plainText: '' },
        subjectId: 'subject-1',
        chapterId: 'chapter-1',
        subjectName: '数学',
        chapterName: '第一章',
        tags: [],
        resourceRefs: [{
          nodeId: 'image-1',
          resourceId: 'resource-1',
          contentSlot: 'stem',
          optionId: null,
        }],
        createdAt: 1,
        updatedAt: 1,
        lastUsedAt: null,
        deletedAt: null,
        contentVersion: 1,
      },
    }],
  }
}

describe('paper export helpers', () => {
  it('maps each content mode to the required managed template regions', () => {
    expect(requiredTemplateAnchors('paper_only')).toEqual(['ZT_QUESTIONS'])
    expect(requiredTemplateAnchors('answers_only')).toEqual(['ZT_QUESTIONS'])
    expect(requiredTemplateAnchors('paper_answers_explanations')).toEqual(['ZT_QUESTIONS'])
  })

  it('rejects missing and duplicate required template regions', () => {
    expect(templateExportAvailability(template(['ZT_QUESTIONS']), 'paper_only').usable).toBe(true)
    expect(templateExportAvailability(template([]), 'paper_and_answers')).toMatchObject({
      usable: false,
      missingAnchors: ['ZT_QUESTIONS'],
    })
    expect(templateExportAvailability(template(['ZT_QUESTIONS', 'ZT_QUESTIONS']), 'paper_only').reason)
      .toContain('重复')
  })

  it('matches the backend document kind and supported-anchor rules', () => {
    expect(templateExportAvailability(template(['ZT_QUESTIONS'], 'template'), 'paper_only'))
      .toMatchObject({ usable: false, reason: expect.stringContaining('.docx') })
    expect(templateExportAvailability(template(['ZT_QUESTIONS', 'ZT_SCHOOL']), 'paper_only'))
      .toMatchObject({ usable: false, reason: expect.stringContaining('ZT_SCHOOL') })
    expect(templateExportAvailability(template(['ZT_QUESTIONS', 'ZT_TITLE', 'ZT_TITLE']), 'paper_only'))
      .toMatchObject({ usable: false, reason: expect.stringContaining('重复') })
  })

  it('builds a safe deterministic Windows docx filename', () => {
    const result = suggestedPaperDocxName(
      '  七年级:数学/期末?  ',
      '{title}_{date}.docx',
      new Date(2026, 6, 13),
    )
    expect(result).toBe('七年级_数学_期末__2026-07-13.docx')
  })

  it('checks only content slots included in the selected export mode', () => {
    const input = paper()
    expect(inspectPaperExportContent(input, 'paper_only')).toMatchObject({
      managedImageReferenceCount: 1,
      sourceOoxmlFragmentCount: 0,
      hasUnsupportedContent: false,
    })
    expect(inspectPaperExportContent(input, 'answers_only')).toMatchObject({
      managedImageReferenceCount: 0,
      sourceOoxmlFragmentCount: 1,
      hasUnsupportedContent: true,
    })
  })

  it('accepts managed HTML images, tables and editor formula nodes for rich DOCX export', () => {
    const input = paper()
    input.items[0].snapshot.stem.html = '<p>题目<img data-resource-id="resource-1"></p><table><tr><td>1</td></tr></table><p><span class="math-node" data-latex="x">x</span></p>'
    expect(inspectPaperExportContent(input, 'paper_only')).toMatchObject({
      inlineImageCount: 1,
      tableCount: 1,
      mathNodeCount: 1,
      unresolvedImageNodeCount: 0,
      malformedMathNodeCount: 0,
      hasUnsupportedContent: false,
    })
  })

  it('blocks every additional complex HTML signature rejected by the backend', () => {
    for (const html of [
      '<math><mi>x</mi></math>',
      '<svg><path /></svg>',
      '<img src="https://example.invalid/a.png">',
      '<span src="data:image/png;base64,AA">图片</span>',
      "<span src='data:image/png;base64,AA'>图片</span>",
      '<span class="katex">x</span>',
      '<span class="mathquill-rendered-math">x</span>',
    ]) {
      const input = paper()
      input.items[0].snapshot.resourceRefs = []
      input.items[0].snapshot.stem.html = html
      expect(inspectPaperExportContent(input, 'paper_only')).toMatchObject({
        complexHtmlFragmentCount: 1,
        hasUnsupportedContent: true,
      })
    }
  })
})
