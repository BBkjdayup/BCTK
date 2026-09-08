import type {
  Question,
  QuestionBatchEditRequest,
  QuestionBatchTagMode,
  QuestionBatchUsageOperation,
} from '../types/domain'

export interface QuestionBatchEditForm {
  classificationMode: 'keep' | 'change'
  subjectId: string
  chapterId: string
  usageOperation: QuestionBatchUsageOperation
  tagMode: QuestionBatchTagMode
  tagIds: string[]
}

export function createQuestionBatchEditForm(): QuestionBatchEditForm {
  return {
    classificationMode: 'keep',
    subjectId: '',
    chapterId: '',
    usageOperation: 'keep',
    tagMode: 'keep',
    tagIds: [],
  }
}

export function questionBatchEditValidation(
  questions: readonly Question[],
  form: QuestionBatchEditForm,
): string | null {
  if (!questions.length) return '请先选择至少一道要修改的题目。'
  if (form.classificationMode === 'change' && (!form.subjectId || !form.chapterId)) {
    return '修改分类时必须同时选择学科和章节。'
  }
  if ((form.tagMode === 'append' || form.tagMode === 'remove') && !form.tagIds.length) {
    return form.tagMode === 'append' ? '请选择要追加的标签。' : '请选择要删除的标签。'
  }
  if (form.tagMode === 'keep' && form.tagIds.length) {
    return '保持标签不变时不能选择标签。'
  }
  if (
    form.classificationMode === 'keep'
    && form.usageOperation === 'keep'
    && form.tagMode === 'keep'
  ) {
    return '尚未选择任何要修改的项目。'
  }
  return null
}

export function buildQuestionBatchEditRequest(
  questions: readonly Question[],
  form: QuestionBatchEditForm,
): QuestionBatchEditRequest {
  const validation = questionBatchEditValidation(questions, form)
  if (validation) throw new Error(validation)
  return {
    questions: questions.map((question) => ({
      id: question.id,
      expectedContentVersion: question.contentVersion,
    })),
    classification: form.classificationMode === 'change'
      ? { subjectId: form.subjectId, chapterId: form.chapterId }
      : null,
    usageOperation: form.usageOperation,
    tagOperation: {
      mode: form.tagMode,
      tagIds: form.tagMode === 'keep' ? [] : [...form.tagIds],
    },
  }
}

export function questionBatchEditSummary(form: QuestionBatchEditForm): string[] {
  const items: string[] = []
  if (form.classificationMode === 'change') items.push('修改所属学科和章节')
  if (form.tagMode === 'append') items.push(`追加 ${form.tagIds.length} 个标签`)
  if (form.tagMode === 'replace') {
    items.push(form.tagIds.length ? `替换为 ${form.tagIds.length} 个标签` : '清空全部标签')
  }
  if (form.tagMode === 'remove') items.push(`删除 ${form.tagIds.length} 个标签`)
  if (form.usageOperation === 'reset_never') items.push('将最近使用状态重置为“从未使用”')
  return items
}
