import { errorMessage, isCommandErrorPayload } from '../services/errors'

export interface ImportErrorExplanation {
  title: string
  action: string
  detail: string
}

export function explainImportError(reason: unknown, kind: 'docx' | 'xlsx'): ImportErrorExplanation {
  const code = isCommandErrorPayload(reason) ? reason.code : ''
  const message = errorMessage(reason, `${kind === 'docx' ? 'Word' : 'Excel'} 文件读取失败`)
  const detail = code ? `${code}：${message}` : message

  if (code === 'WORD_IMPORT_DRAFT_EXISTS' || /未完成的.*导入草稿/u.test(message)) {
    return {
      title: '还有上次未完成的导入草稿',
      action: '先在当前页面恢复或放弃旧草稿，再重新选择文件。',
      detail,
    }
  }
  if (/超过.*(?:安全上限|大小限制|100 MB)|FILE_TOO_LARGE|LIMIT_EXCEEDED/u.test(`${code} ${message}`)) {
    return {
      title: '文件内容超过导入限制',
      action: '请把文档分成几份较小的文件，分别导入。',
      detail,
    }
  }
  if (code === 'DOCX_IMAGE_EXTRACTION_FAILED' || /图片.*(?:无法导入|格式暂不支持)/u.test(message)) {
    const image = message.match(/第\s*\d+\s*张图片/u)?.[0]
    return {
      title: image ? `${image}无法读取` : '文档中有图片无法读取',
      action: '请在 Word 或 WPS 中找到这张图片，将其替换为 PNG 或 JPEG 图片后重新导入。',
      detail,
    }
  }
  if (code === 'DOCX_IMPORT_ANALYSIS_REJECTED' || code === 'DOCX_ZIP_INVALID'
    || /文件不是有效的 DOCX|未通过安全与结构检查/u.test(message)) {
    return {
      title: 'Word 文件结构无法读取',
      action: '请用 Word 或 WPS 打开原文件，另存为新的 .docx 后再导入；若仍失败，请复制错误详情。',
      detail,
    }
  }
  if (code === 'DOCX_MATHTYPE_CONVERSION_FAILED' || code === 'DOCX_OMML_CONVERSION_FAILED') {
    return {
      title: '文档中有公式无法转换',
      action: '请在 Word 或 WPS 中检查公式，另存为新的 .docx 后重试；若仍失败，请复制错误详情。',
      detail,
    }
  }
  if (/(?:database is locked|SQLITE_BUSY|数据库.*锁定)/iu.test(`${code} ${message}`)) {
    return {
      title: '本地题库暂时被占用',
      action: '请关闭其他正在使用题库的窗口，稍等片刻后重试。',
      detail,
    }
  }
  if (/(?:permission denied|access is denied|拒绝访问|无权访问)/iu.test(message)) {
    return {
      title: '没有读取该文件的权限',
      action: '请将文件复制到“文档”文件夹，关闭 Word 或 WPS 后重新选择。',
      detail,
    }
  }
  return {
    title: `${kind === 'docx' ? 'Word' : 'Excel'} 导入没有完成`,
    action: '请查看下方错误详情；如果重试仍失败，可复制详情发给客服排查。',
    detail,
  }
}
