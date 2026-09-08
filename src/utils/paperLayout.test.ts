import { describe, expect, it } from 'vitest'
import type { Paper, Question, QuestionTypeDefinition, TemplateLayoutPreview } from '../types/domain'
import {
  createPaperCanvasData,
  createPaperCanvasOptions,
  normalizeCanvasLatex,
  paperManagedImageResourceIds,
  paperLayoutSourceSignature,
  richContentToCanvasElements,
} from './paperLayout'

function paper(): Paper {
  const content = (plainText: string) => ({
    schemaVersion: 2 as const,
    editor: 'tiptap' as const,
    editorVersion: '3.28.0',
    document: {
      type: 'doc',
      content: [{
        type: 'paragraph',
        content: [
          { type: 'text', text: plainText, marks: [{ type: 'bold' }] },
          { type: 'mathNode', attrs: { latex: 'x^2' } },
        ],
      }],
    },
    html: `<p><strong>${plainText}</strong></p>`,
    plainText,
  })
  const question: Question = {
    id: '019f7d00-0000-7000-8000-000000000001',
    type: 'single_choice',
    stem: content('题干'),
    options: [{ id: '019f7d00-0000-7000-8000-000000000002', position: 0, content: content('选项') }],
    answer: content('A'),
    explanation: content('解析'),
    subjectId: '019f7d00-0000-7000-8000-000000000003',
    chapterId: '019f7d00-0000-7000-8000-000000000004',
    subjectName: '数学',
    chapterName: '代数',
    tags: [],
    createdAt: 1,
    updatedAt: 2,
    contentVersion: 3,
  }
  return {
    id: '019f7d00-0000-7000-8000-000000000005',
    title: '测试卷',
    compositionMode: 'manual',
    generationConfig: null,
    status: 'draft',
    items: [{ id: '019f7d00-0000-7000-8000-000000000006', sourceQuestionId: question.id, position: 0, snapshot: question }],
    exportContentMode: 'paper_answers_explanations',
    subjectSummaryText: '数学',
    layout: null,
    rowVersion: 0,
    createdAt: 1,
    updatedAt: 2,
  }
}

describe('paper layout adapter', () => {
  it('maps short inequality aliases to symbols supported by canvas-editor', () => {
    expect(normalizeCanvasLatex('-2\\le m\\le -1')).toBe('-2\\leq m\\leq -1')
    expect(normalizeCanvasLatex('m\\ge 1')).toBe('m\\geq 1')
    expect(normalizeCanvasLatex('m\\leq 2')).toBe('m\\leq 2')
  })

  it('creates editable canvas data with questions, answers, explanations and formulas', () => {
    const data = createPaperCanvasData(paper())
    const text = data.main.map((element) => element.value).join('')

    expect(text).toContain('测试卷')
    expect(text).toContain('一、选择题：本题共1个小题，每小题      分。共      分。')
    expect(text).toContain('参考答案')
    expect(text).toContain('题目解析')
    expect(data.main.some((element) => element.type === 'latex' && element.value === 'x^2')).toBe(true)
    expect(data.main).toEqual(expect.arrayContaining([
      expect.objectContaining({
        type: 'latex',
        imgPreviewDisabled: true,
        imgToolDisabled: true,
        extension: expect.objectContaining({
          kind: 'paper-formula',
          paperItemId: '019f7d00-0000-7000-8000-000000000006',
          contentSlot: 'stem',
          ordinal: 0,
        }),
      }),
    ]))
  })

  it('converts legacy HTML formulas and tables without exposing LaTeX source text', () => {
    const elements = richContentToCanvasElements({
      schemaVersion: 1,
      html: [
        '<p>角度<span class="math-node" data-latex="675^{\\circ}">675^{\\circ}</span>的值</p>',
        '<table><tbody><tr><td>选项</td><td><span class="math-node" data-latex="\\frac{11}{4}\\pi">\\frac{11}{4}\\pi</span></td></tr></tbody></table>',
      ].join(''),
      plainText: '\\(675^{\\circ}\\)的值',
    })
    const nestedElements = elements.flatMap((element) => [
      element,
      ...(element.trList ?? []).flatMap((row) => row.tdList.flatMap((cell) => cell.value ?? [])),
    ])

    expect(elements.some((element) => element.type === 'table')).toBe(true)
    expect(nestedElements).toEqual(expect.arrayContaining([
      expect.objectContaining({ type: 'latex', value: '675^{\\circ}' }),
      expect.objectContaining({ type: 'latex', value: '\\frac{11}{4}\\pi' }),
    ]))
    expect(nestedElements.map((element) => element.value).join('')).not.toContain('\\(')
  })

  it('keeps Tiptap tables and managed images as real canvas elements', () => {
    const resourceId = '019fa7fc-2c03-7111-a8b6-4166afe943bc'
    const content = {
      schemaVersion: 2 as const,
      editor: 'tiptap' as const,
      editorVersion: '3.28.0',
      document: {
        type: 'doc',
        content: [
          {
            type: 'table',
            content: [
              {
                type: 'tableRow',
                content: [
                  { type: 'tableCell', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'x' }] }] },
                  { type: 'tableCell', content: [{ type: 'paragraph', content: [{ type: 'mathNode', attrs: { latex: '\\pi' } }] }] },
                ],
              },
              {
                type: 'tableRow',
                content: [
                  { type: 'tableCell', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'y' }] }] },
                  { type: 'tableCell', content: [{ type: 'paragraph' }] },
                ],
              },
            ],
          },
          { type: 'paragraph', content: [{ type: 'image', attrs: { resourceId, width: 458 } }] },
        ],
      },
      html: [
        '<table><tbody>',
        '<tr><td><p>x</p></td><td><p><span class="math-node" data-latex="\\pi">\\pi</span></p></td></tr>',
        '<tr><td><p>y</p></td><td><p></p></td></tr>',
        '</tbody></table>',
        `<p><img data-resource-id="${resourceId}" width="458"></p>`,
      ].join(''),
      plainText: 'x y',
    }
    const source = 'data:image/png;base64,AA=='
    const elements = richContentToCanvasElements(content, new Map([
      [resourceId, { src: source, widthPx: 1839, heightPx: 1339 }],
    ]))
    const table = elements.find((element) => element.type === 'table')
    const image = elements.find((element) => element.type === 'image')
    const nestedElements = table?.trList?.flatMap((row) => row.tdList.flatMap((cell) => cell.value)) ?? []

    expect(table?.trList).toHaveLength(2)
    expect(table?.trList?.[0]?.tdList).toHaveLength(2)
    expect(nestedElements).toEqual(expect.arrayContaining([
      expect.objectContaining({ type: 'latex', value: '\\pi' }),
    ]))
    expect(image).toEqual(expect.objectContaining({
      type: 'image',
      value: source,
      width: 458,
      height: 333,
    }))
  })

  it('collects managed image identifiers from paper rich content without duplicates', () => {
    const source = paper()
    const resourceId = '019fa7fc-2c03-7111-a8b6-4166afe943bc'
    source.items[0]!.snapshot.stem.html = `<p><img data-resource-id="${resourceId}"></p>`
    source.items[0]!.snapshot.stem.document = {
      type: 'doc',
      content: [{ type: 'paragraph', content: [{ type: 'image', attrs: { resourceId } }] }],
    }

    expect(paperManagedImageResourceIds(source)).toEqual([resourceId])
  })

  it('recognizes inline formulas from legacy plain text when HTML is unavailable', () => {
    const elements = richContentToCanvasElements({
      schemaVersion: 1,
      html: '',
      plainText: '已知\\(x^2+y^2=1\\)，求半径',
    })

    expect(elements).toEqual([
      expect.objectContaining({ value: '已知' }),
      expect.objectContaining({ type: 'latex', value: 'x^2+y^2=1' }),
      expect.objectContaining({ value: '，求半径' }),
    ])
  })

  it('changes the source signature when the title or question revision changes', () => {
    const source = paper()
    const initial = paperLayoutSourceSignature(source)
    source.title = '新标题'
    expect(paperLayoutSourceSignature(source)).not.toBe(initial)
  })

  it('applies template page settings, static body blocks and saved role styles', () => {
    const hex = (value: string) => [...new TextEncoder().encode(value)]
      .map((byte) => byte.toString(16).padStart(2, '0'))
      .join('')
    const template: TemplateLayoutPreview = {
      templateId: '019f7d00-0000-7000-8000-000000000007',
      templateName: '横向测试模板',
      rowVersion: 1,
      page: {
        widthTwips: 23811,
        heightTwips: 16838,
        marginTopTwips: 720,
        marginRightTwips: 900,
        marginBottomTwips: 1080,
        marginLeftTwips: 1260,
        headerTwips: 560,
        footerTwips: 680,
        columnCount: 2,
        columnGapTwips: 425,
        columnSeparator: true,
        documentGridType: 'lines',
        documentGridLinePitchTwips: 312,
      },
      blocks: [
        {
          kind: 'sideSeal',
          text: '班级 姓名 考号 密封线内不准答题',
          style: null,
          sideSeal: {
            horizontalRelativeFrom: 'column',
            verticalRelativeFrom: 'paragraph',
            horizontalOffsetEmu: -907415,
            verticalOffsetEmu: -862965,
            pageXEmu: -7620,
            pageYEmu: 36830,
            widthEmu: 961390,
            heightEmu: 11428095,
            textDirection: 'vert270',
            lineXEmu: 611741,
            lineWidthEmu: 19050,
            lineDash: 'dash',
            lines: [
              {
                alignment: 'center',
                runs: [
                  { text: 'CLASS', underline: false, sizeHalfPoints: 24 },
                  { text: '              ', underline: true, sizeHalfPoints: 24 },
                  { text: '    NAME', underline: false, sizeHalfPoints: 24 },
                  { text: '                 ', underline: true, sizeHalfPoints: 24 },
                  { text: '    EXAM', underline: false, sizeHalfPoints: 24 },
                  { text: '                   .', underline: true, sizeHalfPoints: 24 },
                ],
              },
              {
                alignment: 'center',
                runs: [
                  {
                    text: 'A              B              C              D              E              F              G              H',
                    underline: false,
                    sizeHalfPoints: null,
                  },
                ],
              },
            ],
          },
        },
        { kind: 'static', text: '学校：第一中学', style: null },
        { kind: 'title', text: '', style: null },
        { kind: 'questions', text: '', style: null },
      ],
      fontTheme: {
        majorLatin: 'Calibri Light',
        majorEastAsia: '宋体',
        minorLatin: 'Calibri',
        minorEastAsia: '宋体',
      },
      pageNumberFormat: '第 {pageNo} 页，共 {pageCount} 页',
      styleProfile: {
        schemaVersion: 1,
        sourceRange: null,
        roles: {
          title: {
            sourceParagraphIndex: 1,
            paragraphProperties: hex('<w:pPr><w:jc w:val="center"/></w:pPr>'),
            runProperties: hex('<w:rPr><w:rFonts w:eastAsia="宋体"/><w:b/><w:sz w:val="40"/></w:rPr>'),
          },
          sectionHeading: {
            sourceParagraphIndex: 2,
            paragraphProperties: hex('<w:pPr><w:spacing w:line="360" w:lineRule="auto"/><w:ind w:left="422" w:hanging="482"/><w:jc w:val="left"/></w:pPr>'),
            runProperties: hex('<w:rPr><w:rFonts w:eastAsiaTheme="minorEastAsia"/><w:b/><w:sz w:val="24"/></w:rPr>'),
          },
          question: {
            sourceParagraphIndex: 3,
            paragraphProperties: hex('<w:pPr><w:spacing w:line="240" w:lineRule="auto"/></w:pPr>'),
            runProperties: hex('<w:rPr><w:rFonts w:eastAsiaTheme="minorEastAsia"/><w:sz w:val="21"/></w:rPr>'),
          },
          option: {
            sourceParagraphIndex: 4,
            paragraphProperties: hex('<w:pPr><w:spacing w:line="240" w:lineRule="auto"/></w:pPr>'),
            runProperties: hex('<w:rPr><w:rFonts w:eastAsiaTheme="minorEastAsia"/><w:sz w:val="24"/></w:rPr>'),
          },
        },
      },
    }
    const source = paper()
    source.preferredTemplateId = template.templateId
    source.exportContentMode = 'paper_only'
    source.items.push({
      ...source.items[0]!,
      id: '019f7d00-0000-7000-8000-000000000008',
      position: 1,
    })
    const data = createPaperCanvasData(source, template)
    const options = createPaperCanvasOptions(template)
    const text = data.main.map((element) => element.value).join('')

    expect(text.indexOf('学校：第一中学')).toBeLessThan(text.indexOf('测试卷'))
    expect(text.indexOf('测试卷')).toBeLessThan(text.indexOf('一、选择题'))
    const sideSeal = data.main.find((element) => (
      element.type === 'image'
      && element.imgDisplay === 'float-top'
      && element.imgFloatPosition?.pageNo === 0
    ))
    expect(sideSeal).toBeDefined()
    const sideSealSvg = decodeURIComponent(sideSeal?.value ?? '')
    expect(sideSealSvg).not.toContain('fill="white"')
    expect(sideSealSvg).toContain('rotate(-90)')
    expect(sideSealSvg).toContain('text-anchor="middle"')
    expect(sideSealSvg).toContain('599.90) rotate(-90)')
    expect(sideSealSvg).toContain('text-decoration="underline"')
    // rotate(-90) paints the first run from the bottom upward, matching Word:
    // CLASS is at the bottom, NAME in the middle and EXAM at the top.
    expect(sideSealSvg.indexOf('CLASS')).toBeLessThan(sideSealSvg.indexOf('NAME'))
    expect(sideSealSvg.indexOf('NAME')).toBeLessThan(sideSealSvg.indexOf('EXAM'))
    expect(sideSealSvg).toContain('A              B              C')
    expect(sideSeal?.width).toBeCloseTo(100.93, 1)
    expect(sideSeal?.height).toBeCloseTo(1199.8, 1)
    expect(sideSeal?.imgFloatPosition?.x).toBeCloseTo(-0.8, 1)
    expect(sideSeal?.imgFloatPosition?.y).toBeCloseTo(3.87, 1)
    const sealIndex = data.main.indexOf(sideSeal!)
    expect(data.main[sealIndex + 1]).toMatchObject({ value: '\n', size: 1, rowMargin: 0 })
    expect(data.main.some((element) => (
      element.value === '测试卷'
      && element.font === '宋体'
      && element.size === 27
      && element.rowFlex === 'center'
    ))).toBe(true)
    const headingIndex = data.main.findIndex((element) => element.value.startsWith('一、选择题'))
    expect(data.main[headingIndex]).toMatchObject({
      rowFlex: 'left',
      bold: true,
      size: 16,
      rowMargin: 0.5,
    })
    expect(data.main[headingIndex - 1]).toMatchObject({
      value: '\n',
      rowFlex: 'left',
      rowMargin: 0.5,
    })
    expect(text).toContain('2. 题干')
    expect(text).not.toContain('\n\n')
    expect(data.main.some((element) => (
      element.value === '1. '
      && element.font === '宋体'
      && Math.abs((element.rowMargin ?? 0) - 0.425) < 0.001
    ))).toBe(true)
    expect(data.main.some((element) => (
      element.value.startsWith('    A. ')
      && Math.abs((element.rowMargin ?? 0) - 0.3) < 0.001
    ))).toBe(true)
    expect(options.width).toBeLessThan(options.height as number)
    expect(options.defaultFont).toBe('宋体')
    expect(options.defaultSize).toBe(14)
    expect(options.defaultRowMargin).toBeCloseTo(0.425)
    expect(options.paperDirection).toBe('horizontal')
    expect(options.column?.count).toBe(2)
    expect(options.column?.gap).toBe(28)
    expect(options.pageNumber?.format).toBe('第 {pageNo} 页，共 {pageCount} 页')
  })

  it('changes the source signature when the selected template changes', () => {
    const source = paper()
    const initial = paperLayoutSourceSignature(source)
    source.preferredTemplateId = '019f7d00-0000-7000-8000-000000000007'
    expect(paperLayoutSourceSignature(source)).not.toBe(initial)
  })

  it('uses a user-created type name in the editable paper preview', () => {
    const source = paper()
    const customCode = 'custom_0123456789abcdef0123456789abcdef'
    source.items[0]!.snapshot.type = customCode
    const definition: QuestionTypeDefinition = {
      code: customCode,
      name: '判断题',
      behavior: 'open_response',
      aliases: ['是非题'],
      defaultOptions: [],
      isBuiltin: false,
      isEnabled: true,
      sortOrder: 70,
      questionCount: 1,
      paperItemCount: 1,
      createdAt: 1,
      updatedAt: 1,
    }
    const text = createPaperCanvasData(source, null, undefined, [definition])
      .main.map((element) => element.value).join('')
    expect(text).toContain('一、判断题：本题共1个小题')
  })

  it('builds one canonical layout for a 200-question paper', () => {
    const source = paper()
    const item = source.items[0]!
    source.exportContentMode = 'paper_only'
    source.items = Array.from({ length: 200 }, (_, index) => ({
      ...item,
      id: `paper-item-${index + 1}`,
      position: index,
    }))

    const data = createPaperCanvasData(source)
    const text = data.main.map((element) => element.value).join('')

    expect(text).toContain('本题共200个小题')
    expect(text).toContain('200. 题干')
  })
})
