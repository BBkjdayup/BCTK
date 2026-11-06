import type { Paper, QuestionTypeDefinition, RichContent } from '../types/domain'
import { questionTypeBehavior } from './questionTypes'
import { richContentSearchText } from './richContent'
import type { PreparedImage } from './miniProgramMedia'

// Keep this publication limit aligned with cloud/src/mini/assets.rs.
const MAX_TABLE_SPAN = 1000

function escape(text: string) {
  return text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;')
}
function content(value: RichContent, label: string, media?: Map<string, PreparedImage>, preflight = false) {
  const doc = new DOMParser().parseFromString(value.html || '', 'text/html')
  if (doc.querySelector('video,audio,iframe') || /"type"\s*:\s*"(?:mathNode|image)"/u.test(JSON.stringify(value.document ?? {})) && !doc.querySelector('img,[data-latex]')) {
    throw new Error(`${label}含不支持的附件或缺失的媒体内容，请先检查原题。`)
  }
  // Whitelist formatting supported by native rich-text; no links, event handlers, or remote resources.
  const tags = new Set(['p', 'div', 'span', 'strong', 'b', 'em', 'i', 'u', 's', 'sub', 'sup', 'br', 'ul', 'ol', 'li', 'table', 'thead', 'tbody', 'tfoot', 'tr', 'td', 'th'])
  function render(node: Node): string {
    if (node.nodeType === Node.TEXT_NODE) return escape(node.textContent || '')
    if (!(node instanceof Element)) return ''
    if (node.hasAttribute('data-latex') || node.tagName === 'IMG') {
      const key = node.hasAttribute('data-latex') ? 'formula:' + node.getAttribute('data-latex') : node.hasAttribute('data-resource-id') ? 'image:' + node.getAttribute('data-resource-id') : 'inline:' + node.getAttribute('src')
      const prepared = media?.get(key) ?? (preflight ? { id: 'preflight', width: 1 } : undefined)
      if (!prepared) throw new Error(`${label}图片或公式资源尚未就绪，暂不能发布。`)
      const requested = Number(node.getAttribute('width'))
      const width = node.tagName === 'IMG' && !node.hasAttribute('data-latex') && Number.isFinite(requested) && requested > 0
        ? Math.min(prepared.width, requested) : prepared.width
      return `<img src="tkasset:${escape(prepared.id)}" data-width="${Math.ceil(width)}"${prepared.height ? ` data-height="${Math.max(1, Math.ceil(prepared.height * width / prepared.width))}"` : ''} alt="${escape(node.getAttribute('data-latex') || node.getAttribute('alt') || '题目图片')}"/>`
    }
    if (['SVG', 'MATH'].includes(node.tagName)) throw new Error(`${label}包含未转换的公式，请在公式编辑器重新保存。`)
    if (['SCRIPT', 'STYLE', 'IFRAME', 'OBJECT'].includes(node.tagName)) return ''
    const inner = [...node.childNodes].map(render).join(''), tag = node.tagName.toLowerCase()
    if (!tags.has(tag)) return inner
    if (tag === 'br') return '<br/>'
    let attributes = ''
    if (tag === 'td' || tag === 'th') {
      for (const name of ['colspan', 'rowspan']) {
        const value = node.getAttribute(name)
        if (value === null) continue
        if (!/^[1-9]\d{0,3}$/.test(value) || Number(value) > MAX_TABLE_SPAN) {
          throw new Error(`${label}的表格合并行列数无效，仅支持 1–${MAX_TABLE_SPAN} 的整数，请检查原题。`)
        }
        attributes += ` ${name}="${value}"`
      }
    }
    return `<${tag}${attributes}>${inner}</${tag}>`
  }
  return value.html ? [...doc.body.childNodes].map(render).join('') : escape(value.plainText || '').replace(/\n/g, '<br/>')
}

export function miniProgramPaper(paper: Paper, definitions?: readonly QuestionTypeDefinition[], media?: Map<string, PreparedImage>, preflight = false) {
  if (paper.status !== 'saved' || !paper.items.length) throw new Error('请先保存包含题目的试卷')
  const questions = [...paper.items].sort((a, b) => a.position - b.position).map((item, n) => {
    const q = item.snapshot, label = `第 ${n + 1} 题`
    const behavior = questionTypeBehavior(q.type, definitions)
    const choice = behavior === 'single_choice' || behavior === 'multiple_choice'
    const options = choice ? [...q.options].sort((a, b) => a.position - b.position) : []
    const answerText = richContentSearchText(q.answer).normalize('NFKC').trim()
    let labels = answerText.toUpperCase()
    if (choice && q.type === 'true_false' && !/^[A-Z]$/u.test(labels)) {
      const positives = ['正确', '对', '是', '√', '✓', 'TRUE'], negatives = ['错误', '错', '否', '×', '✕', 'FALSE']
      const normalized = (value: string) => positives.includes(value.toUpperCase()) ? '正确' : negatives.includes(value.toUpperCase()) ? '错误' : value
      const index = options.findIndex(o => normalized(richContentSearchText(o.content).trim()) === normalized(labels))
      if (index >= 0) labels = String.fromCharCode(65 + index)
    }
    const letters = [...labels].filter(s => /[A-Z]/u.test(s))
    if (choice && (!/^[A-Z](?:[\s、,，;；]*[A-Z])*$/u.test(labels) || !letters.length || new Set(letters).size !== letters.length || letters.some(s => !options[s.charCodeAt(0) - 65]) || (behavior === 'single_choice' && letters.length !== 1))) {
      throw new Error(`${label}的正确选项无法确定，请先将答案整理为 A 或 A、C 等选项字母。`)
    }
    return {
      metadata: { sourceId: q.id, revision: q.contentVersion, subjectId:q.subjectId, subjectName:q.subjectName,
        chapterId:q.chapterId, chapterName:q.chapterName, tags:(q.tags || []).map(tag => tag.name), tagIds:(q.tags || []).map(tag => tag.id) },
      id: item.id, kind: choice ? (behavior === 'single_choice' ? 'single' : 'multiple') : 'text',
      stem: content(q.stem, label + '题干', media, preflight),
      options: options.map(o => ({ id: o.id, content: content(o.content, label + '选项', media, preflight) })),
      correctChoices: choice ? letters.map(s => options[s.charCodeAt(0) - 65]!.id) : [],
      answer: content(q.answer, label + '答案', media, preflight), explanation: content(q.explanation, label + '解析', media, preflight),
    }
  })
  return { title: paper.title, questions }
}
