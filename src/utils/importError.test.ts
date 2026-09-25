import { describe, expect, it } from 'vitest'
import { explainImportError } from './importError'

describe('explainImportError', () => {
  it('explains an unsupported document image without showing XML jargon as the title', () => {
    const result = explainImportError({
      code: 'DOCX_IMAGE_EXTRACTION_FAILED',
      message: '文档图片 第 7 张图片（word/media/image10.wmf）无法导入：图片格式暂不支持',
    }, 'docx')
    expect(result.title).toContain('第 7 张图片')
    expect(result.action).toContain('PNG')
    expect(result.detail).toContain('word/media/image10.wmf')
  })

  it('gives an actionable message for an unfinished draft', () => {
    const result = explainImportError({ code: 'WORD_IMPORT_DRAFT_EXISTS', message: '已有草稿' }, 'docx')
    expect(result.title).toContain('草稿')
    expect(result.action).toContain('恢复或放弃')
  })

  it('asks to split a file when the image extractor hits a size limit', () => {
    const result = explainImportError({
      code: 'DOCX_IMAGE_EXTRACTION_FAILED',
      message: 'DOCX image occurrences 超过安全上限：上限 128，实际 129。',
    }, 'docx')
    expect(result.title).toBe('文件内容超过导入限制')
    expect(result.action).toContain('分成几份')
  })

  it('keeps unknown technical details available instead of presenting them as instructions', () => {
    const result = explainImportError(new Error('invalid header row 3'), 'xlsx')
    expect(result.title).toBe('Excel 导入没有完成')
    expect(result.detail).toBe('invalid header row 3')
  })
})
