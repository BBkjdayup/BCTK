import katex from 'katex'
import { toPng } from 'html-to-image'
import katexCss from 'katex/dist/katex.css?inline'
import type { ManagedImagePayload, Paper, RichContent } from '../types/domain'

const fonts = import.meta.glob('../../node_modules/katex/dist/fonts/*.woff2', { eager: true, query: '?inline', import: 'default' }) as Record<string, string>
const embeddedCss = katexCss.replace(/src:[^;]+;/g, source => {
  const name = source.match(/KaTeX_[\w-]+\.woff2/)?.[0]
  const entry = name && Object.entries(fonts).find(([path]) => path.endsWith('/' + name))
  return entry ? `src:url("${entry[1]}") format("woff2");` : source
})
let fontStyle: HTMLStyleElement | undefined
function ensureFonts() {
  if (fontStyle?.isConnected) return
  fontStyle = document.createElement('style'); fontStyle.textContent = embeddedCss; document.head.append(fontStyle)
}
export interface PreparedImage { id: string; width: number }
export interface MediaDependencies {
  getImage: (id: string) => Promise<ManagedImagePayload>
  upload: (image: { mimeType: string; dataBase64: string }) => Promise<{ id: string; width: number; height: number }>
  formula?: typeof formulaPng
}
export async function formulaPng(latex: string) {
  if (!latex.trim() || latex.length > 4000) throw new Error('公式为空或过长，无法发布')
  ensureFonts()
  const holder = document.createElement('div')
  holder.style.cssText = 'position:fixed;left:-10000px;top:0;pointer-events:none;'
  const node = document.createElement('div')
  node.style.cssText = 'display:inline-block;padding:4px 6px;font-size:20px;line-height:1.4;color:#111827;background:#fff;white-space:nowrap;'
  holder.append(node); document.body.append(holder)
  try {
    katex.render(latex, node, { output: 'html', displayMode: false, throwOnError: true, trust: false, maxExpand: 1000, maxSize: 20, strict: 'error' })
    await document.fonts.ready
    const bounds = node.getBoundingClientRect()
    if (bounds.width > 2048 || bounds.height > 2048) throw new Error('公式尺寸过大，请拆分公式')
    const dataUrl = await toPng(node, { pixelRatio: 2, fontEmbedCSS: embeddedCss, backgroundColor: '#fff', width: Math.ceil(bounds.width), height: Math.ceil(bounds.height) })
    const dataBase64 = dataUrl.split(',')[1]!
    if (dataBase64.length > 2796204) throw new Error('公式图片超过 2 MB，请简化后重试')
    return { mimeType: 'image/png', dataBase64, width: Math.ceil(bounds.width) }
  } finally { holder.remove() }
}
export function mediaKey(element: Element) {
  const latex = element.getAttribute('data-latex')
  if (latex !== null) return 'formula:' + latex
  const id = element.getAttribute('data-resource-id')
  if (id) return 'image:' + id
  const source = element.getAttribute('src') || ''
  if (/^data:image\/(png|jpeg);base64,[A-Za-z0-9+/]+=*$/u.test(source)) return 'inline:' + source
  throw new Error('图片没有本地资源编号，请先重新导入图片；不能发布外部图片链接。')
}
export async function prepareMedia(paper: Paper, dependencies: MediaDependencies) {
  const result = new Map<string, PreparedImage>()
  const fields: RichContent[] = paper.items.flatMap(i => [i.snapshot.stem, i.snapshot.answer, i.snapshot.explanation, ...i.snapshot.options.map(o => o.content)])
  for (const field of fields) {
    const doc = new DOMParser().parseFromString(field.html || '', 'text/html')
    for (const element of doc.querySelectorAll('[data-latex],img')) {
      if (element.parentElement?.closest('[data-latex]')) continue
      const key = mediaKey(element)
      if (result.has(key)) continue
      if (key.startsWith('formula:')) {
        const image = await (dependencies.formula ?? formulaPng)(key.slice(8))
        const uploaded = await dependencies.upload({ mimeType: image.mimeType, dataBase64: image.dataBase64 })
        result.set(key, { id: uploaded.id, width: image.width })
      } else {
        let image: { mimeType: string; dataBase64: string }
        if (key.startsWith('image:')) image = await dependencies.getImage(key.slice(6))
        else {
          const [, mimeType, dataBase64] = key.slice(7).match(/^data:(image\/(?:png|jpeg));base64,(.*)$/)!
          image = { mimeType: mimeType!, dataBase64: dataBase64! }
        }
        const uploaded = await dependencies.upload(image)
        const requested = Number(element.getAttribute('width'))
        result.set(key, { id: uploaded.id, width: Math.min(uploaded.width, requested > 0 ? requested : uploaded.width) })
      }
    }
  }
  return result
}
