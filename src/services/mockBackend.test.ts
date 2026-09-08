import { beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  DocumentQuestionDraftPayload,
  Paper,
  QuestionDraft,
  QuestionFilters,
  RandomDrawUsageFilter,
  WordImportDraftPayload,
} from '../types/domain'

const baseFilters = (): QuestionFilters => ({
  keyword: '',
  tagIds: [],
  tagMatchMode: 'all',
  usage: 'all',
  deleted: false,
  page: 1,
  pageSize: 20,
})

const wordImportDraftPayload = (): WordImportDraftPayload => {
  const empty = { schemaVersion: 1 as const, html: '', plainText: '' }
  const itemId = '018f47f4-6322-7a0a-9d03-0242ac120002'
  return {
    schemaVersion: 1,
    sourceFileName: '  Ｌｅｓｓｏｎ．ｄｏｃｘ  ',
    sourceFileSize: 1_024,
    parserVersion: '  ｗ１-ooxml-1  ',
    sourceItemCount: 1,
    omittedItemCount: 0,
    activeItemId: itemId,
    items: [{
      id: itemId,
      ordinal: 1,
      selected: true,
      payload: {
        type: 'short_answer',
        stem: empty,
        options: [],
        answer: empty,
        explanation: empty,
        subjectId: '',
        chapterId: '',
        tagIds: [],
      },
      status: 'warning',
      diagnostic: '待补充题干和答案',
      duplicateKind: 'none',
      candidate: null,
      action: null,
      needsCheck: true,
      checkError: null,
    }],
  }
}

beforeEach(() => {
  localStorage.clear()
  vi.resetModules()
})

describe('mockBackend', () => {
  it('provides the same taxonomy and question list shape as the Rust API', async () => {
    const { mockBackend } = await import('./mockBackend')
    const bootstrap = await mockBackend.initialize()
    const page = await mockBackend.listQuestions(baseFilters())

    expect(bootstrap.databaseHealthy).toBe(true)
    expect(bootstrap.subjects.length).toBeGreaterThanOrEqual(2)
    expect(page.total).toBe(4)
    expect(page.items[0]).toHaveProperty('contentVersion')
  })

  it('combines keyword, taxonomy and usage filters', async () => {
    const { mockBackend } = await import('./mockBackend')
    const page = await mockBackend.listQuestions({
      ...baseFilters(),
      keyword: '基本要素',
      subjectId: 'subject-education',
      usage: 'never',
    })

    expect(page.total).toBe(1)
    expect(page.items[0]?.id).toBe('question-2')
  })

  it('supports both union and intersection matching for multiple tags', async () => {
    const { mockBackend } = await import('./mockBackend')
    const anyTag = await mockBackend.listQuestions({
      ...baseFilters(),
      tagIds: ['tag-foundation', 'tag-exam'],
      tagMatchMode: 'any',
    })
    const allTags = await mockBackend.listQuestions({
      ...baseFilters(),
      tagIds: ['tag-foundation', 'tag-exam'],
      tagMatchMode: 'all',
    })

    expect(anyTag.items.map((item) => item.id).sort()).toEqual(['question-1', 'question-2', 'question-3'])
    expect(allTags.items.map((item) => item.id)).toEqual(['question-1'])
  })

  it('filters random-draw candidates by the selected usage period', async () => {
    vi.useFakeTimers()
    try {
      vi.setSystemTime(new Date(2026, 8, 9, 12, 0, 0))
      const { mockBackend } = await import('./mockBackend')
      const analyze = (usage: RandomDrawUsageFilter) => mockBackend.analyzeRandomDraw({
        subjectIds: [],
        chapterIds: [],
        tagIds: [],
        tagMatchMode: 'any',
        usage,
        excludedQuestionIds: [],
      })

      await expect(analyze('all')).resolves.toMatchObject({ availableTotal: 4 })
      await expect(analyze('never')).resolves.toMatchObject({ availableTotal: 3 })
      await expect(analyze('unused_this_semester')).resolves.toMatchObject({ availableTotal: 3 })
      await expect(analyze('unused_this_month')).resolves.toMatchObject({ availableTotal: 3 })
      await expect(analyze('unused_this_week')).resolves.toMatchObject({ availableTotal: 3 })
      await expect(analyze('unused_today')).resolves.toMatchObject({ availableTotal: 4 })
    } finally {
      vi.useRealTimers()
    }
  })

  it('saves, searches, recycles and restores a question without losing content', async () => {
    const { mockBackend } = await import('./mockBackend')
    const rich = (text: string) => ({ schemaVersion: 1 as const, html: `<p>${text}</p>`, plainText: text })
    const draft: QuestionDraft = {
      type: 'short_answer',
      stem: rich('用于自动化测试的唯一题干'),
      options: [],
      answer: rich('测试答案'),
      explanation: rich('测试解析'),
      subjectId: 'subject-education',
      chapterId: 'chapter-edu-1',
      tagIds: ['tag-thinking'],
    }

    const saved = await mockBackend.saveQuestion(draft)
    const searched = await mockBackend.listQuestions({ ...baseFilters(), keyword: '唯一题干' })
    expect(searched.items.map((item) => item.id)).toContain(saved.id)

    await mockBackend.moveQuestionsToRecycle([saved.id])
    const recycled = await mockBackend.listQuestions({ ...baseFilters(), deleted: true })
    expect(recycled.items.map((item) => item.id)).toContain(saved.id)
    await expect(mockBackend.saveQuestion({
      ...structuredClone(draft),
      questionId: saved.id,
      baseContentVersion: saved.contentVersion,
    })).rejects.toMatchObject({ code: 'QUESTION_IN_RECYCLE' })

    await mockBackend.restoreQuestions([saved.id])
    const restored = await mockBackend.getQuestion(saved.id)
    expect(restored?.deletedAt).toBeNull()
    expect(restored?.answer.plainText).toBe('测试答案')
  })

  it('round-trips a document-entry draft and saves all questions together', async () => {
    const { mockBackend } = await import('./mockBackend')
    const rich = (text: string) => ({
      schemaVersion: 1 as const,
      html: text ? `<p>${text}</p>` : '',
      plainText: text,
    })
    const itemId = crypto.randomUUID()
    const question: QuestionDraft = {
      type: 'short_answer',
      stem: rich('文档式录入测试题'),
      options: [],
      answer: rich('文档式录入答案'),
      explanation: rich(''),
      subjectId: 'subject-education',
      chapterId: 'chapter-edu-1',
      tagIds: [],
    }
    const payload: DocumentQuestionDraftPayload = {
      schemaVersion: 1,
      activeItemId: itemId,
      items: [{ id: itemId, ordinal: 1, payload: question }],
    }

    await mockBackend.saveDocumentQuestionDraft(payload)
    const restoredLegacy = (await mockBackend.getDocumentQuestionDraft())?.payload
    expect(restoredLegacy?.schemaVersion).toBe(1)
    expect(restoredLegacy?.schemaVersion === 1 ? restoredLegacy.items : []).toHaveLength(1)
    const saved = await mockBackend.saveQuestions([question])
    expect(saved).toHaveLength(1)
    expect(saved[0]?.stem.plainText).toBe('文档式录入测试题')

    await mockBackend.deleteDocumentQuestionDraft()
    expect(await mockBackend.getDocumentQuestionDraft()).toBeNull()
  })

  it('persists a free labeled document before recognition', async () => {
    const { mockBackend } = await import('./mockBackend')
    const payload: DocumentQuestionDraftPayload = {
      schemaVersion: 2,
      templateId: 'labeled_fields_v1',
      source: {
        schemaVersion: 2,
        editor: 'tiptap',
        editorVersion: '3.28.0',
        document: {
          type: 'doc',
          content: [{
            type: 'paragraph',
            content: [{ type: 'text', text: '题目：自由录入示例' }],
          }],
        },
        html: '<p>题目：自由录入示例</p>',
        plainText: '题目：自由录入示例',
      },
    }

    const saved = await mockBackend.saveDocumentQuestionDraft(payload)
    expect(saved.payload.schemaVersion).toBe(2)
    expect(saved.payload.schemaVersion === 2 ? saved.payload.source.plainText : '')
      .toBe('题目：自由录入示例')
  })

  it('applies batch metadata edits atomically and rejects one stale target without partial changes', async () => {
    const { mockBackend } = await import('./mockBackend')
    const first = await mockBackend.getQuestion('question-1')
    const second = await mockBackend.getQuestion('question-2')
    expect(first).not.toBeNull()
    expect(second).not.toBeNull()

    const request = {
      questions: [
        { id: first!.id, expectedContentVersion: first!.contentVersion },
        { id: second!.id, expectedContentVersion: second!.contentVersion + 1 },
      ],
      classification: { subjectId: 'subject-psychology', chapterId: 'chapter-psy-1' },
      usageOperation: 'reset_never' as const,
      tagOperation: { mode: 'replace' as const, tagIds: ['tag-thinking'] },
    }
    await expect(mockBackend.batchEditQuestions(request)).rejects.toMatchObject({ code: 'CONTENT_CONFLICT' })
    const unchanged = await mockBackend.getQuestion(first!.id)
    expect(unchanged).toMatchObject({
      subjectId: first!.subjectId,
      chapterId: first!.chapterId,
      contentVersion: first!.contentVersion,
    })
    expect(unchanged?.tags.map((tag) => tag.id)).toEqual(first!.tags.map((tag) => tag.id))

    request.questions[1]!.expectedContentVersion = second!.contentVersion
    const result = await mockBackend.batchEditQuestions(request)
    expect(result.updatedCount).toBe(2)
    const updated = await mockBackend.getQuestion(first!.id)
    expect(updated).toMatchObject({
      subjectId: 'subject-psychology',
      chapterId: 'chapter-psy-1',
      lastUsedAt: null,
      contentVersion: first!.contentVersion + 1,
    })
    expect(updated?.tags.map((tag) => tag.id)).toEqual(['tag-thinking'])
  })

  it('detects exact and suspected duplicates and excludes the question being edited', async () => {
    const { backend } = await import('./backend')
    const source = await backend.getQuestion('question-1')
    expect(source).not.toBeNull()
    const sourceDraft: QuestionDraft = {
      type: source!.type,
      stem: structuredClone(source!.stem),
      options: structuredClone(source!.options),
      answer: structuredClone(source!.answer),
      explanation: structuredClone(source!.explanation),
      subjectId: source!.subjectId,
      chapterId: source!.chapterId,
      tagIds: source!.tags.map((tag) => tag.id),
    }

    const exact = await backend.checkQuestionDuplicate(sourceDraft)
    expect(exact).toMatchObject({
      status: 'exact',
      evaluatedCandidateCount: 1,
      similarityMethod: 'nfkc_char_bigram_dice_v1',
      candidate: { id: source!.id, similarityPercent: 100, contentVersion: source!.contentVersion },
    })

    const suspected = await backend.checkQuestionDuplicate({
      ...structuredClone(sourceDraft),
      answer: { schemaVersion: 1, html: '<p>B</p>', plainText: 'B' },
    })
    expect(suspected.status).toBe('suspected')
    expect(suspected.candidate?.id).toBe(source!.id)
    expect(suspected.candidate!.similarityPercent).toBeGreaterThanOrEqual(suspected.suspectedThresholdPercent)
    expect(suspected.candidate!.similarityPercent).toBeLessThan(100)

    const editingSelf = await backend.checkQuestionDuplicate({
      ...structuredClone(sourceDraft),
      questionId: source!.id,
      baseContentVersion: source!.contentVersion,
    })
    expect(editingSelf.candidate?.id).not.toBe(source!.id)
    expect(editingSelf.status).toBe('none')

    await expect(backend.checkQuestionDuplicate({
      ...structuredClone(sourceDraft),
      stem: { ...sourceDraft.stem, plainText: `${sourceDraft.stem.plainText}\u001f保留字符` },
    })).rejects.toMatchObject({ code: 'VALIDATION_ERROR' })
    await expect(backend.checkQuestionDuplicate({
      ...structuredClone(sourceDraft),
      answer: { schemaVersion: 1, html: '', plainText: '' },
    })).rejects.toMatchObject({ code: 'VALIDATION_ERROR' })
  })

  it('detects similar imported rows across 100-item database scan boundaries', async () => {
    const { backend } = await import('./backend')
    const rich = (text: string) => ({ schemaVersion: 1 as const, html: `<p>${text}</p>`, plainText: text })
    const imported = Array.from({ length: 102 }, (_, index) => ({
      clientId: crypto.randomUUID(),
      draft: {
        id: crypto.randomUUID(),
        type: 'short_answer' as const,
        stem: rich(`批次边界填充题目 ${index}，内容彼此独立`),
        options: [],
        answer: rich(`填充答案 ${index}`),
        explanation: rich(''),
        subjectId: 'subject-education',
        chapterId: 'chapter-edu-1',
        tagIds: [],
      },
    }))
    imported[0]!.draft.stem = rich('某学校网络管理员需要为教学楼配置访问控制策略并限制访客访问服务器')
    imported[0]!.draft.answer = rich('在边界设备配置访问控制列表并只允许指定网段')
    imported[101]!.draft.stem = rich('某学校网络管理员需要为教学楼配置访问控制策略并禁止访客访问服务器')
    imported[101]!.draft.answer = rich('在边界设备配置访问控制列表并只允许指定网段')

    const result = await backend.checkQuestionDuplicatesBatch({ mode: 'import', items: imported })
    expect(result.items[101]?.check).toMatchObject({
      status: 'suspected',
      candidate: { id: imported[0]!.clientId, sourceKind: 'import-item' },
    })
  })

  it('only treats a stable imported question ID as idempotent when the complete content matches', async () => {
    const { backend } = await import('./backend')
    const rich = (text: string) => ({ schemaVersion: 1 as const, html: `<p>${text}</p>`, plainText: text })
    const stableId = crypto.randomUUID()
    const draft: QuestionDraft = {
      id: stableId,
      type: 'short_answer',
      stem: rich('幂等批量导入题干'),
      options: [],
      answer: rich('幂等批量导入答案'),
      explanation: rich(''),
      subjectId: 'subject-education',
      chapterId: 'chapter-edu-1',
      tagIds: [],
    }

    await expect(backend.saveImportedQuestions([structuredClone(draft)])).resolves.toEqual([stableId])
    await expect(backend.saveImportedQuestions([structuredClone(draft)])).resolves.toEqual([stableId])
    await expect(backend.saveImportedQuestions([{
      ...structuredClone(draft),
      explanation: rich('后来变更的解析'),
    }])).rejects.toMatchObject({ code: 'IMPORT_ID_CONFLICT' })
    expect((await backend.listQuestions(baseFilters())).items.filter((item) => item.id === stableId)).toHaveLength(1)
  })

  it('rolls back the complete imported batch when a stable ID conflicts', async () => {
    const { backend } = await import('./backend')
    const rich = (text: string) => ({ schemaVersion: 1 as const, html: `<p>${text}</p>`, plainText: text })
    const existingId = crypto.randomUUID()
    const original: QuestionDraft = {
      id: existingId,
      type: 'short_answer',
      stem: rich('已存在的批量导入题目'),
      options: [],
      answer: rich('原答案'),
      explanation: rich('原解析'),
      subjectId: 'subject-education',
      chapterId: 'chapter-edu-1',
      tagIds: ['tag-thinking'],
      resourceRefs: [{
        nodeId: 'node-original',
        resourceId: 'resource-original',
        contentSlot: 'stem',
      }],
    }
    await backend.saveImportedQuestions([structuredClone(original)])

    const newId = crypto.randomUUID()
    const newDraft: QuestionDraft = {
      ...structuredClone(original),
      id: newId,
      stem: rich('本批次前面准备新增的题目'),
    }
    const conflictingDraft: QuestionDraft = {
      ...structuredClone(original),
      answer: rich('冲突后的答案'),
      tagIds: [],
      resourceRefs: [],
    }

    await expect(backend.saveImportedQuestions([newDraft, conflictingDraft]))
      .rejects.toMatchObject({ code: 'IMPORT_ID_CONFLICT' })
    expect(await backend.getQuestion(newId)).toBeNull()
    expect(await backend.getQuestion(existingId)).toMatchObject({
      answer: { plainText: '原答案' },
      explanation: { plainText: '原解析' },
      tags: [{ id: 'tag-thinking' }],
      resourceRefs: [{ resourceId: 'resource-original' }],
    })
  })

  it('overwrites imported duplicates atomically and rolls back a stale batch', async () => {
    const { backend } = await import('./backend')
    const first = await backend.getQuestion('question-1')
    const second = await backend.getQuestion('question-2')
    expect(first).not.toBeNull()
    expect(second).not.toBeNull()

    const overwriteDraft = (question: NonNullable<typeof first>, suffix: string): QuestionDraft => ({
      id: crypto.randomUUID(),
      questionId: question.id,
      baseContentVersion: question.contentVersion,
      type: question.type,
      stem: { ...structuredClone(question.stem), plainText: `${question.stem.plainText}${suffix}` },
      options: structuredClone(question.options),
      answer: structuredClone(question.answer),
      explanation: structuredClone(question.explanation),
      subjectId: question.subjectId,
      chapterId: question.chapterId,
      tagIds: question.tags.map((tag) => tag.id),
      resourceRefs: structuredClone(question.resourceRefs ?? []),
    })
    const firstDraft = overwriteDraft(first!, '（批量覆盖）')
    const staleSecondDraft = overwriteDraft(second!, '（不会写入）')
    staleSecondDraft.baseContentVersion = second!.contentVersion + 1

    await expect(backend.saveImportedQuestionOverwrites({
      operationId: crypto.randomUUID(),
      items: [
        { clientId: firstDraft.id!, draft: firstDraft },
        { clientId: staleSecondDraft.id!, draft: staleSecondDraft },
      ],
    })).rejects.toMatchObject({ code: 'QUESTION_VERSION_CONFLICT' })

    expect(await backend.getQuestion(first!.id)).toMatchObject({
      contentVersion: first!.contentVersion,
      stem: { plainText: first!.stem.plainText },
    })
    expect(await backend.getQuestion(second!.id)).toMatchObject({
      contentVersion: second!.contentVersion,
      stem: { plainText: second!.stem.plainText },
    })
  })

  it('safely replays the same imported overwrite operation and rejects changed content', async () => {
    const { backend } = await import('./backend')
    const source = await backend.getQuestion('question-1')
    expect(source).not.toBeNull()
    const clientId = crypto.randomUUID()
    const operationId = crypto.randomUUID()
    const draft: QuestionDraft = {
      id: clientId,
      questionId: source!.id,
      baseContentVersion: source!.contentVersion,
      type: source!.type,
      stem: structuredClone(source!.stem),
      options: structuredClone(source!.options),
      answer: structuredClone(source!.answer),
      explanation: { schemaVersion: 1, html: '<p>批量覆盖后的解析</p>', plainText: '批量覆盖后的解析' },
      subjectId: source!.subjectId,
      chapterId: source!.chapterId,
      tagIds: source!.tags.map((tag) => tag.id),
      resourceRefs: structuredClone(source!.resourceRefs ?? []),
    }
    const request = { operationId, items: [{ clientId, draft }] }

    const first = await backend.saveImportedQuestionOverwrites(structuredClone(request))
    const replay = await backend.saveImportedQuestionOverwrites(structuredClone(request))
    expect(first).toMatchObject({ operationId, updatedCount: 1, replayed: false })
    expect(replay).toEqual({ ...first, replayed: true })
    expect(await backend.getQuestion(source!.id)).toMatchObject({
      contentVersion: source!.contentVersion + 1,
      explanation: { plainText: '批量覆盖后的解析' },
    })

    const changed = structuredClone(request)
    changed.items[0]!.draft.answer = { schemaVersion: 1, html: '<p>另一答案</p>', plainText: '另一答案' }
    await expect(backend.saveImportedQuestionOverwrites(changed))
      .rejects.toMatchObject({ code: 'IMPORT_OVERWRITE_OPERATION_CONFLICT' })
  })

  it('rejects duplicate overwrite targets before changing any question', async () => {
    const { backend } = await import('./backend')
    const source = await backend.getQuestion('question-1')
    expect(source).not.toBeNull()
    const draft = (clientId: string): QuestionDraft => ({
      id: clientId,
      questionId: source!.id,
      baseContentVersion: source!.contentVersion,
      type: source!.type,
      stem: structuredClone(source!.stem),
      options: structuredClone(source!.options),
      answer: structuredClone(source!.answer),
      explanation: structuredClone(source!.explanation),
      subjectId: source!.subjectId,
      chapterId: source!.chapterId,
      tagIds: source!.tags.map((tag) => tag.id),
    })
    const firstId = crypto.randomUUID()
    const secondId = crypto.randomUUID()

    await expect(backend.saveImportedQuestionOverwrites({
      operationId: crypto.randomUUID(),
      items: [
        { clientId: firstId, draft: draft(firstId) },
        { clientId: secondId, draft: draft(secondId) },
      ],
    })).rejects.toMatchObject({ code: 'IMPORT_OVERWRITE_DUPLICATE_TARGET' })
    expect(await backend.getQuestion(source!.id)).toMatchObject({
      contentVersion: source!.contentVersion,
    })
  })

  it('does not consider recycled questions as duplicate candidates', async () => {
    const { mockBackend } = await import('./mockBackend')
    const source = await mockBackend.getQuestion('question-1')
    expect(source).not.toBeNull()
    const draft: QuestionDraft = {
      type: source!.type,
      stem: structuredClone(source!.stem),
      options: structuredClone(source!.options),
      answer: structuredClone(source!.answer),
      explanation: structuredClone(source!.explanation),
      subjectId: source!.subjectId,
      chapterId: source!.chapterId,
      tagIds: source!.tags.map((tag) => tag.id),
    }

    await mockBackend.moveQuestionsToRecycle([source!.id])
    const result = await mockBackend.checkQuestionDuplicate(draft)
    expect(result.candidate?.id).not.toBe(source!.id)
    expect(result.status).toBe('none')
  })

  it('scans one taxonomy scope against the whole bank and expires ignored pairs after an edit', async () => {
    const { mockBackend } = await import('./mockBackend')
    const rich = (text: string) => ({ schemaVersion: 1 as const, html: `<p>${text}</p>`, plainText: text })
    const source = await mockBackend.getQuestion('question-1')
    expect(source).not.toBeNull()
    const exactCopy = await mockBackend.saveQuestion({
      type: source!.type,
      stem: structuredClone(source!.stem),
      options: structuredClone(source!.options),
      answer: structuredClone(source!.answer),
      explanation: structuredClone(source!.explanation),
      subjectId: 'subject-psychology',
      chapterId: 'chapter-psy-1',
      tagIds: [],
    })

    const firstDraft: QuestionDraft = {
      type: 'short_answer',
      stem: rich('某学校网络管理员需要为教学楼配置访问控制策略并限制访客访问服务器'),
      options: [],
      answer: rich('在边界设备配置访问控制列表并只允许指定网段'),
      explanation: rich(''),
      subjectId: 'subject-education',
      chapterId: 'chapter-edu-1',
      tagIds: [],
    }
    const secondDraft: QuestionDraft = {
      ...structuredClone(firstDraft),
      stem: rich('某学校网络管理员需要为教学楼配置访问控制策略并禁止访客访问服务器'),
      subjectId: 'subject-psychology',
      chapterId: 'chapter-psy-1',
    }
    const first = await mockBackend.saveQuestion(firstDraft)
    const second = await mockBackend.saveQuestion(secondDraft)
    const request = { subjectId: 'subject-education', chapterId: 'chapter-edu-1' }
    const initial = await mockBackend.scanQuestionDuplicates(request)

    expect(initial.exactGroups.some((group) => {
      const ids = group.members.map((member) => member.id)
      return ids.includes(source!.id) && ids.includes(exactCopy.id)
    })).toBe(true)
    const suspected = initial.suspectedGroups.find((group) => {
      const ids = group.members.map((member) => member.id)
      return ids.includes(first.id) && ids.includes(second.id)
    })
    expect(suspected?.similarityPercent).toBeGreaterThanOrEqual(initial.suspectedThresholdPercent)

    await expect(mockBackend.ignoreQuestionDuplicate({
      firstQuestionId: source!.id,
      firstContentVersion: source!.contentVersion,
      secondQuestionId: exactCopy.id,
      secondContentVersion: exactCopy.contentVersion,
    })).rejects.toMatchObject({ code: 'DUPLICATE_PAIR_EXACT' })

    await mockBackend.ignoreQuestionDuplicate({
      firstQuestionId: first.id,
      firstContentVersion: first.contentVersion,
      secondQuestionId: second.id,
      secondContentVersion: second.contentVersion,
    })
    const ignored = await mockBackend.scanQuestionDuplicates(request)
    expect(ignored.suspectedGroups.some((group) => {
      const ids = group.members.map((member) => member.id)
      return ids.includes(first.id) && ids.includes(second.id)
    })).toBe(false)

    await mockBackend.saveQuestion({
      ...structuredClone(secondDraft),
      questionId: second.id,
      baseContentVersion: second.contentVersion,
      stem: rich('某学校网络管理员需要为教学楼配置访问控制策略并阻止访客访问服务器'),
    })
    const afterEdit = await mockBackend.scanQuestionDuplicates(request)
    expect(afterEdit.suspectedGroups.some((group) => {
      const ids = group.members.map((member) => member.id)
      return ids.includes(first.id) && ids.includes(second.id)
    })).toBe(true)

    await expect(mockBackend.ignoreQuestionDuplicate({
      firstQuestionId: first.id,
      firstContentVersion: first.contentVersion,
      secondQuestionId: second.id,
      secondContentVersion: second.contentVersion,
    })).rejects.toMatchObject({ code: 'DUPLICATE_PAIR_STALE' })

    await mockBackend.moveQuestionsToRecycle([second.id])
    await mockBackend.permanentlyDeleteQuestions([second.id])
    const storedIgnores = JSON.parse(localStorage.getItem('zhitiku-preview-duplicate-ignores') ?? '[]') as Array<{
      lowId: string
      highId: string
    }>
    expect(storedIgnores.some((ignored) => ignored.lowId === second.id || ignored.highId === second.id)).toBe(false)
  })

  it('persists create drafts through the backend facade and updates the pending count', async () => {
    const { backend } = await import('./backend')
    const empty = { schemaVersion: 1 as const, html: '', plainText: '' }
    const draft: QuestionDraft = {
      type: 'short_answer',
      stem: empty,
      options: [],
      answer: empty,
      explanation: empty,
      subjectId: 'subject-education',
      chapterId: 'chapter-edu-1',
      tagIds: [],
    }

    const saved = await backend.saveQuestionDraft(draft)
    expect(saved).toMatchObject({
      draftKey: 'question:create',
      draftKind: 'question_create',
      payloadSchemaVersion: 1,
      stale: false,
    })
    expect((await backend.initialize()).pendingDraftCount).toBe(1)

    vi.resetModules()
    const { backend: restoredBackend } = await import('./backend')
    expect((await restoredBackend.getQuestionDraft(null))?.id).toBe(saved.id)
    await restoredBackend.deleteQuestionDraft(null)
    expect(await restoredBackend.listQuestionDrafts()).toEqual([])
    expect((await restoredBackend.initialize()).pendingDraftCount).toBe(0)
  })

  it('round-trips the Word import draft and counts both pending workflows', async () => {
    const { backend } = await import('./backend')
    const empty = { schemaVersion: 1 as const, html: '', plainText: '' }
    await backend.saveQuestionDraft({
      type: 'short_answer',
      stem: empty,
      options: [],
      answer: empty,
      explanation: empty,
      subjectId: '',
      chapterId: '',
      tagIds: [],
    })

    const saved = await backend.saveWordImportDraft(wordImportDraftPayload())
    expect(saved.payload).toMatchObject({
      sourceFileName: 'Lesson.docx',
      parserVersion: 'w1-ooxml-1',
      sourceItemCount: 1,
      omittedItemCount: 0,
      activeItemId: saved.payload.items[0]?.id,
    })
    expect(JSON.parse(localStorage.getItem('zhitiku-preview-word-import-draft') ?? 'null')).toMatchObject({
      id: saved.id,
      payload: { sourceFileName: 'Lesson.docx' },
    })
    expect((await backend.initialize()).pendingDraftCount).toBe(2)

    vi.resetModules()
    const { backend: restoredBackend } = await import('./backend')
    expect(await restoredBackend.getWordImportDraft()).toEqual(saved)
    expect((await restoredBackend.initialize()).pendingDraftCount).toBe(2)

    await restoredBackend.deleteWordImportDraft()
    expect(await restoredBackend.getWordImportDraft()).toBeNull()
    expect(localStorage.getItem('zhitiku-preview-word-import-draft')).toBeNull()
    expect((await restoredBackend.initialize()).pendingDraftCount).toBe(1)
  })

  it('rejects unsafe Word import draft identity and duplicate state combinations', async () => {
    const { mockBackend } = await import('./mockBackend')
    const withPath = wordImportDraftPayload()
    withPath.sourceFileName = 'C:\\Teacher\\Lesson.docx'
    await expect(mockBackend.saveWordImportDraft(withPath)).rejects.toMatchObject({
      code: 'VALIDATION_ERROR',
    })

    const missingCandidate = wordImportDraftPayload()
    missingCandidate.items[0]!.duplicateKind = 'exact'
    await expect(mockBackend.saveWordImportDraft(missingCandidate)).rejects.toMatchObject({
      code: 'VALIDATION_ERROR',
    })

    const invalidOrdinal = wordImportDraftPayload()
    invalidOrdinal.items[0]!.ordinal = 0
    await expect(mockBackend.saveWordImportDraft(invalidOrdinal)).rejects.toMatchObject({
      code: 'VALIDATION_ERROR',
    })

    const invalidTruncation = wordImportDraftPayload()
    invalidTruncation.sourceItemCount = 1
    invalidTruncation.omittedItemCount = 1
    await expect(mockBackend.saveWordImportDraft(invalidTruncation)).rejects.toMatchObject({
      code: 'VALIDATION_ERROR',
    })
  })

  it('marks an edit draft stale after the question content version changes', async () => {
    const { mockBackend } = await import('./mockBackend')
    const question = await mockBackend.getQuestion('question-1')
    expect(question).not.toBeNull()
    const editDraft: QuestionDraft = {
      questionId: question!.id,
      type: question!.type,
      stem: structuredClone(question!.stem),
      options: structuredClone(question!.options),
      answer: structuredClone(question!.answer),
      explanation: structuredClone(question!.explanation),
      subjectId: question!.subjectId,
      chapterId: question!.chapterId,
      tagIds: question!.tags.map((tag) => tag.id),
      baseContentVersion: question!.contentVersion,
    }

    const savedDraft = await mockBackend.saveQuestionDraft(editDraft)
    expect(savedDraft.stale).toBe(false)
    await mockBackend.saveQuestion({
      ...structuredClone(editDraft),
      stem: { ...editDraft.stem, html: '<p>其他窗口更新</p>', plainText: '其他窗口更新' },
    })

    const staleDraft = await mockBackend.getQuestionDraft(question!.id)
    expect(staleDraft?.stale).toBe(true)
    await expect(mockBackend.saveQuestion(staleDraft!.payload)).rejects.toMatchObject({
      code: 'QUESTION_VERSION_CONFLICT',
    })
  })

  it('persists taxonomy CRUD, NFKC names and transactional-style reordering', async () => {
    const { mockBackend } = await import('./mockBackend')
    const subjectId = await mockBackend.createSubject('  Ｔｅｓｔ  ')
    let bootstrap = await mockBackend.initialize()
    expect(bootstrap.subjects.find((subject) => subject.id === subjectId)?.name).toBe('Test')

    await expect(mockBackend.createSubject('test')).rejects.toMatchObject({
      code: 'VALIDATION_ERROR',
    })

    const chapterId = await mockBackend.createChapter(subjectId, '  第一章  ')
    await expect(mockBackend.deleteSubject(subjectId)).rejects.toMatchObject({
      code: 'VALIDATION_ERROR',
    })
    await mockBackend.deleteChapter(chapterId)

    bootstrap = await mockBackend.initialize()
    const order = [...bootstrap.subjects]
      .reverse()
      .map((subject, index) => ({ id: subject.id, sortOrder: (index + 1) * 1024 }))
    await mockBackend.saveSubjectOrder(order)
    bootstrap = await mockBackend.initialize()
    expect([...bootstrap.subjects].sort((left, right) => left.sortOrder - right.sortOrder)[0]?.id).toBe(order[0]?.id)

    await mockBackend.deleteSubject(subjectId)
    expect((await mockBackend.initialize()).subjects.some((subject) => subject.id === subjectId)).toBe(false)
  })

  it('deleting a tag removes only relationships and keeps questions', async () => {
    const { mockBackend } = await import('./mockBackend')
    const before = await mockBackend.getQuestion('question-3')
    expect(before?.tags.some((tag) => tag.id === 'tag-thinking')).toBe(true)

    await mockBackend.deleteTag('tag-thinking')
    const after = await mockBackend.getQuestion('question-3')
    expect(after?.stem.plainText).toBe(before?.stem.plainText)
    expect(after?.tags.some((tag) => tag.id === 'tag-thinking')).toBe(false)
  })

  it('saves, lists, copies and version-checks paper snapshots', async () => {
    const { mockBackend } = await import('./mockBackend')
    const question = await mockBackend.getQuestion('question-1')
    expect(question).not.toBeNull()
    const timestamp = Date.now()
    const paper: Paper = {
      id: crypto.randomUUID(),
      title: '  Ｔｅｓｔ试卷  ',
      compositionMode: 'automatic',
      generationConfig: {
        subjectId: 'subject-education',
        chapterIds: ['chapter-edu-1'],
        questionTypeCounts: {
          single_choice: 1,
          multiple_choice: 0,
          fill_blank: 0,
          true_false: 0,
          short_answer: 0,
        },
      },
      status: 'draft',
      items: [{
        id: crypto.randomUUID(),
        sourceQuestionId: question!.id,
        position: 0,
        snapshot: structuredClone(question!),
      }],
      exportContentMode: 'paper_only',
      subjectSummaryText: '',
      rowVersion: 0,
      createdAt: timestamp,
      updatedAt: timestamp,
      savedAt: null,
      lastSavedAt: null,
    }

    const draft = await mockBackend.savePaper(paper)
    expect(draft.title).toBe('Test试卷')
    expect(draft.rowVersion).toBe(1)
    expect(draft.generationConfig).toEqual(paper.generationConfig)
    const saved = await mockBackend.savePaper({ ...draft, status: 'saved' })
    expect(saved.rowVersion).toBe(2)
    expect((await mockBackend.listPapers({ keyword: 'Test', status: 'saved', page: 1, pageSize: 20 })).total).toBe(1)

    await expect(mockBackend.savePaper({ ...draft, title: '过期修改' })).rejects.toMatchObject({
      code: 'PAPER_VERSION_CONFLICT',
    })
    const copied = await mockBackend.copyPaper(saved.id, saved.rowVersion)
    expect(copied.status).toBe('draft')
    expect(copied.generationConfig).toEqual(paper.generationConfig)
    expect(copied.items[0]?.snapshot.stem.plainText).toBe(question?.stem.plainText)
    await mockBackend.deletePaper(copied.id, copied.rowVersion)
    expect(await mockBackend.getPaper(copied.id)).toBeNull()

    const firstBatchCopy = await mockBackend.copyPaper(saved.id, saved.rowVersion)
    const secondBatchCopy = await mockBackend.copyPaper(saved.id, saved.rowVersion)
    await mockBackend.deletePapers([
      { id: firstBatchCopy.id, baseRowVersion: firstBatchCopy.rowVersion },
      { id: secondBatchCopy.id, baseRowVersion: secondBatchCopy.rowVersion },
    ])
    expect(await mockBackend.getPaper(firstBatchCopy.id)).toBeNull()
    expect(await mockBackend.getPaper(secondBatchCopy.id)).toBeNull()

    const retainedCopy = await mockBackend.copyPaper(saved.id, saved.rowVersion)
    const conflictedCopy = await mockBackend.copyPaper(saved.id, saved.rowVersion)
    await expect(mockBackend.deletePapers([
      { id: retainedCopy.id, baseRowVersion: retainedCopy.rowVersion },
      { id: conflictedCopy.id, baseRowVersion: conflictedCopy.rowVersion + 1 },
    ])).rejects.toMatchObject({ code: 'PAPER_VERSION_CONFLICT' })
    expect(await mockBackend.getPaper(retainedCopy.id)).not.toBeNull()
    expect(await mockBackend.getPaper(conflictedCopy.id)).not.toBeNull()
  })

  it('imports a template with the Rust DTO shape and restores it from localStorage', async () => {
    const { mockBackend } = await import('./mockBackend')
    const sourcePath = await mockBackend.pickDocxFile()
    const result = await mockBackend.importTemplate(sourcePath, undefined, {
      fileName: '教师试卷模板.dotx',
      size: 12_345,
    })

    expect(result.imported).toBe(true)
    expect(result.diagnostics).toEqual([])
    expect(result.template).toMatchObject({
      name: '教师试卷模板',
      fileName: '教师试卷模板.dotx',
      fileByteSize: 12_345,
      packageKind: 'template',
      analysisStatus: 'ready',
      rowVersion: 1,
      fileAvailable: true,
      regionConfigured: true,
    })
    expect(result.template?.fileSha256Hex).toMatch(/^[0-9a-f]{64}$/)
    expect(result.template?.anchors[0]).toMatchObject({
      name: 'ZT_QUESTIONS',
      kind: 'content_control',
      partName: 'word/document.xml',
    })
    expect(JSON.parse(localStorage.getItem('zhitiku-preview-templates') ?? '[]')).toHaveLength(1)

    vi.resetModules()
    const { mockBackend: restoredBackend } = await import('./mockBackend')
    expect(await restoredBackend.listTemplates()).toEqual([result.template])
  })

  it('returns import diagnostics without storing unsupported or oversized files', async () => {
    const { mockBackend } = await import('./mockBackend')
    const unsupported = await mockBackend.importTemplate('C:\\Templates\\template.pdf')
    const oversized = await mockBackend.importTemplate(
      'C:\\Templates\\template.docx',
      undefined,
      { size: 100 * 1024 * 1024 + 1 },
    )

    expect(unsupported).toMatchObject({
      imported: false,
      template: null,
      diagnostics: [{ severity: 'error', code: 'DOCX_TEMPLATE_EXTENSION_UNSUPPORTED' }],
    })
    expect(oversized).toMatchObject({
      imported: false,
      template: null,
      diagnostics: [{ severity: 'error', code: 'DOCX_ARCHIVE_SIZE_LIMIT' }],
    })
    expect(await mockBackend.listTemplates()).toEqual([])
  })

  it('renames templates with normalized unique names and optimistic version checks', async () => {
    const { mockBackend } = await import('./mockBackend')
    const first = (await mockBackend.importTemplate('C:\\Templates\\first.docx', '第一份')).template!
    await mockBackend.importTemplate('C:\\Templates\\second.docx', '第二份')

    const renamed = await mockBackend.renameTemplate(first.id, '  Ｎｅｗ　模板  ', first.rowVersion)
    expect(renamed.name).toBe('New 模板')
    expect(renamed.rowVersion).toBe(2)

    await expect(mockBackend.renameTemplate(first.id, '过期修改', first.rowVersion)).rejects.toMatchObject({
      code: 'TEMPLATE_VERSION_CONFLICT',
    })
    await expect(mockBackend.renameTemplate(renamed.id, '第二份', renamed.rowVersion)).rejects.toMatchObject({
      code: 'VALIDATION_ERROR',
    })
  })

  it('configures an anchorless template before allowing it to become the default', async () => {
    const { mockBackend } = await import('./mockBackend')
    const imported = (await mockBackend.importTemplate(
      'C:\\Templates\\待配置模板.docx',
      undefined,
      { configured: false },
    )).template!

    expect(imported.analysisStatus).toBe('needs_configuration')
    await expect(mockBackend.setDefaultTemplate(imported.id, imported.rowVersion)).rejects.toMatchObject({
      code: 'VALIDATION_ERROR',
    })

    const preview = await mockBackend.getTemplateConfigurationPreview(imported.id)
    expect(preview).toMatchObject({
      templateId: imported.id,
      rowVersion: imported.rowVersion,
      hasQuestionsAnchor: false,
    })
    expect(preview.paragraphs.length).toBeGreaterThan(0)

    const configured = await mockBackend.configureTemplateRegions({
      templateId: imported.id,
      baseRowVersion: preview.rowVersion,
      questions: { mode: 'document_end' },
      title: { mode: 'replace_paragraph', paragraphIndex: 1 },
    })
    expect(configured).toMatchObject({
      analysisStatus: 'ready',
      regionConfigured: true,
      rowVersion: imported.rowVersion + 1,
    })
    expect(configured.anchors.map((anchor) => anchor.name)).toEqual(['ZT_QUESTIONS', 'ZT_TITLE'])

    await mockBackend.setDefaultTemplate(configured.id, configured.rowVersion)
    expect((await mockBackend.getSettings()).defaultTemplateId).toBe(configured.id)
  })

  it('configures a complete sample-paper range with separate style samples', async () => {
    const { mockBackend } = await import('./mockBackend')
    const imported = (await mockBackend.importTemplate(
      'C:\\Templates\\样卷.docx',
      undefined,
      { configured: false },
    )).template!

    const configured = await mockBackend.configureTemplateRegions({
      templateId: imported.id,
      baseRowVersion: imported.rowVersion,
      questions: {
        mode: 'replace_range',
        paragraphIndex: 1,
        endParagraphIndex: 3,
      },
      styleSamples: {
        sectionHeadingParagraphIndex: 1,
        questionParagraphIndex: 2,
        optionParagraphIndex: 3,
      },
      title: { mode: 'replace_paragraph', paragraphIndex: 0 },
    })

    expect(configured.anchors[0]).toMatchObject({
      name: 'ZT_QUESTIONS',
      kind: 'content_control',
    })
    expect(configured.regionConfigured).toBe(true)
  })

  it('keeps the default template, settings and deletion state synchronized', async () => {
    const { mockBackend } = await import('./mockBackend')
    const first = (await mockBackend.importTemplate('C:\\Templates\\first.docx')).template!
    const second = (await mockBackend.importTemplate('C:\\Templates\\second.docx')).template!

    await mockBackend.setDefaultTemplate(first.id, first.rowVersion)
    expect((await mockBackend.listTemplates()).find((item) => item.id === first.id)?.isDefault).toBe(true)
    expect((await mockBackend.getSettings()).defaultTemplateId).toBe(first.id)

    await mockBackend.setDefaultTemplate(second.id, second.rowVersion)
    const switched = await mockBackend.listTemplates()
    expect(switched.filter((item) => item.isDefault).map((item) => item.id)).toEqual([second.id])
    expect((await mockBackend.getSettings()).defaultTemplateId).toBe(second.id)

    await mockBackend.deleteTemplate(second.id, second.rowVersion)
    expect((await mockBackend.listTemplates()).map((item) => item.id)).toEqual([first.id])
    expect((await mockBackend.getSettings()).defaultTemplateId).toBeNull()
  })

  it('routes template operations through the browser backend facade', async () => {
    const { backend } = await import('./backend')
    const sourcePath = await backend.pickDocxFile()
    expect(sourcePath).toMatch(/\.docx$/i)

    const imported = await backend.importTemplate(sourcePath!, '浏览器预览模板')
    expect(imported.imported).toBe(true)
    expect((await backend.listTemplates())[0]?.name).toBe('浏览器预览模板')

    await backend.setDefaultTemplate(imported.template!.id, imported.template!.rowVersion)
    expect((await backend.listTemplates())[0]?.isDefault).toBe(true)
  })

  it('creates, lists and validates browser-preview backup records', async () => {
    const { mockBackend } = await import('./mockBackend')
    const outputPath = await mockBackend.pickBackupSavePath()
    const created = await mockBackend.createBackup(outputPath)

    expect(created.record.status).toBe('ready')
    expect(created.record.archiveSha256Hex).toMatch(/^[0-9a-f]{64}$/)
    expect((await mockBackend.listBackups())[0]?.id).toBe(created.record.id)

    const inspection = await mockBackend.inspectBackup(outputPath)
    expect(inspection.archiveSha256Hex).toBe(created.record.archiveSha256Hex)
    expect(inspection.payloadFileCount).toBeGreaterThan(0)
  })

  it('prepares, validates and cancels a restore plan without changing data', async () => {
    const { mockBackend } = await import('./mockBackend')
    const path = await mockBackend.pickBackupFile()
    const plan = await mockBackend.prepareRestore(path)

    expect(plan.restoreToken).toMatch(/^[0-9a-f-]{36}$/)
    expect(plan.archiveInspection.archiveSha256Hex).toMatch(/^[0-9a-f]{64}$/)
    expect(plan.incomingSummary.questionCount).toBeGreaterThanOrEqual(plan.currentSummary!.questionCount)
    await expect(mockBackend.scheduleRestore({
      restoreToken: plan.restoreToken,
      expectedArchiveSha256Hex: plan.archiveInspection.archiveSha256Hex,
      confirmedCurrentDataReplacement: false,
    })).rejects.toMatchObject({ code: 'VALIDATION_ERROR' })
    await expect(mockBackend.scheduleRestore({
      restoreToken: plan.restoreToken,
      expectedArchiveSha256Hex: '00'.repeat(32),
      confirmedCurrentDataReplacement: true,
    })).rejects.toMatchObject({ code: 'RESTORE_CONFIRMATION_MISMATCH' })

    await mockBackend.cancelRestore(plan.restoreToken)
    await expect(mockBackend.scheduleRestore({
      restoreToken: plan.restoreToken,
      expectedArchiveSha256Hex: plan.archiveInspection.archiveSha256Hex,
      confirmedCurrentDataReplacement: true,
    })).rejects.toMatchObject({ code: 'RESTORE_PLAN_INVALID' })
  })

  it('persists the restart-style restore result until the user acknowledges it', async () => {
    const { backend } = await import('./backend')
    const path = await backend.pickBackupFile()
    const plan = await backend.prepareRestore(path!)
    const scheduled = await backend.scheduleRestore({
      restoreToken: plan.restoreToken,
      expectedArchiveSha256Hex: plan.archiveInspection.archiveSha256Hex,
      confirmedCurrentDataReplacement: true,
    })
    expect(scheduled.requiresRestart).toBe(false)
    expect((await backend.getLastRestoreResult())?.operationId).toBe(scheduled.operationId)

    vi.resetModules()
    const { backend: restartedBackend } = await import('./backend')
    const restored = await restartedBackend.getLastRestoreResult()
    expect(restored).toMatchObject({ operationId: scheduled.operationId, outcome: 'success' })
    await expect(restartedBackend.acknowledgeRestoreResult(crypto.randomUUID()))
      .rejects.toMatchObject({ code: 'VALIDATION_ERROR' })
    await restartedBackend.acknowledgeRestoreResult(scheduled.operationId)
    expect(await restartedBackend.getLastRestoreResult()).toBeNull()
    expect(localStorage.getItem('zhitiku-preview-last-restore-result')).toBeNull()
  })

  it('rejects an expired browser-preview restore plan', async () => {
    vi.useFakeTimers()
    try {
      vi.setSystemTime(new Date('2026-07-13T08:00:00+08:00'))
      const { mockBackend } = await import('./mockBackend')
      const path = await mockBackend.pickBackupFile()
      const plan = await mockBackend.prepareRestore(path)
      vi.setSystemTime(new Date(plan.expiresAt + 1))
      await expect(mockBackend.scheduleRestore({
        restoreToken: plan.restoreToken,
        expectedArchiveSha256Hex: plan.archiveInspection.archiveSha256Hex,
        confirmedCurrentDataReplacement: true,
      })).rejects.toMatchObject({ code: 'RESTORE_PLAN_EXPIRED' })
    } finally {
      vi.useRealTimers()
    }
  })

  it('prepares, cancels and validates a data-directory move plan', async () => {
    const { backend } = await import('./backend')
    const target = await backend.pickDataDirectory()
    const plan = await backend.prepareDataMove(target!)
    expect(plan).toMatchObject({
      sourceDataRoot: 'C:\\Users\\Teacher\\Documents\\教师题库',
      targetDataRoot: target,
      requiresRestart: true,
    })
    expect(plan.estimatedFileCount).toBeGreaterThan(0)
    await expect(backend.scheduleDataMove({
      moveToken: plan.moveToken,
      expectedSourceDataRoot: plan.sourceDataRoot,
      expectedTargetDataRoot: plan.targetDataRoot,
      confirmedKeepOldData: false,
    })).rejects.toMatchObject({ code: 'VALIDATION_ERROR' })

    await backend.cancelDataMove(plan.moveToken)
    await expect(backend.scheduleDataMove({
      moveToken: plan.moveToken,
      expectedSourceDataRoot: plan.sourceDataRoot,
      expectedTargetDataRoot: plan.targetDataRoot,
      confirmedKeepOldData: true,
    })).rejects.toMatchObject({ code: 'DATA_MOVE_PLAN_INVALID' })
  })

  it('persists a simulated data-directory move through a preview restart', async () => {
    const { backend } = await import('./backend')
    const target = await backend.pickDataDirectory()
    const plan = await backend.prepareDataMove(target!)
    const scheduled = await backend.scheduleDataMove({
      moveToken: plan.moveToken,
      expectedSourceDataRoot: plan.sourceDataRoot,
      expectedTargetDataRoot: plan.targetDataRoot,
      confirmedKeepOldData: true,
    })
    expect(scheduled.requiresRestart).toBe(false)
    expect((await backend.initialize()).dataRoot).toBe(target)

    vi.resetModules()
    const { backend: restartedBackend } = await import('./backend')
    const result = await restartedBackend.getLastDataMoveResult()
    expect(result).toMatchObject({
      operationId: scheduled.operationId,
      outcome: 'success',
      activeDataRoot: target,
      retainedDataRoot: plan.sourceDataRoot,
    })
    expect((await restartedBackend.initialize()).dataRoot).toBe(target)
    await restartedBackend.acknowledgeDataMoveResult(scheduled.operationId)
    expect(await restartedBackend.getLastDataMoveResult()).toBeNull()
  })

  it('creates automatic backups only when due and never prunes manual backups', async () => {
    vi.useFakeTimers()
    try {
      vi.setSystemTime(new Date('2026-08-01T08:00:00+08:00'))
      const { mockBackend } = await import('./mockBackend')
      const current = await mockBackend.getSettings()
      await mockBackend.saveSettings({
        ...current,
        automaticBackupEnabled: true,
        automaticBackupIntervalDays: 1,
        automaticBackupRetentionCount: 1,
      })
      await mockBackend.createBackup('C:\\backup\\manual.tqb')

      const first = await mockBackend.runAutomaticBackup()
      expect(first.outcome).toBe('created')
      expect(first.record?.backupKind).toBe('automatic')
      expect((await mockBackend.runAutomaticBackup()).outcome).toBe('not_due')

      vi.setSystemTime(new Date('2026-08-02T08:00:01+08:00'))
      const second = await mockBackend.runAutomaticBackup()
      expect(second).toMatchObject({ outcome: 'created', prunedCount: 1 })
      const backups = await mockBackend.listBackups()
      expect(backups.filter((backup) => backup.backupKind === 'automatic')).toHaveLength(1)
      expect(backups.filter((backup) => backup.backupKind === 'manual')).toHaveLength(1)
    } finally {
      vi.useRealTimers()
    }
  })
})
