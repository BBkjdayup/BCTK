import katex from 'katex'
import { toPng } from 'html-to-image'
import katexCss from 'katex/dist/katex.css?inline'
import type { ManagedImagePayload, Paper, RichContent } from '../types/domain'
import type { MediaPreparationCache } from './mediaCache'

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
export interface PreparedImage { id: string; width: number; height?: number }
export interface MediaProgress { completed: number; total: number; phase: 'preparing' | 'uploading' | 'waiting'; retryAfterSeconds?: number }
function checkMediaCancellation(signal?: AbortSignal) {
  if (signal?.aborted) throw new Error('已取消发布，可稍后重试')
}
function waitForRetry(seconds: number, signal?: AbortSignal) {
  checkMediaCancellation(signal)
  return new Promise<void>((resolve, reject) => {
    const done = () => { signal?.removeEventListener('abort', abort); resolve() }
    const timer = setTimeout(done, seconds * 1000)
    const abort = () => {
      clearTimeout(timer); signal?.removeEventListener('abort', abort)
      try { checkMediaCancellation(signal) } catch (error) { reject(error) }
    }
    signal?.addEventListener('abort', abort, { once: true })
  })
}
export interface MediaDependencies {
  getImage: (id: string) => Promise<ManagedImagePayload>
  upload: (image: { mimeType: string; dataBase64: string }) => Promise<{ id: string; width: number; height: number }>
  formula?: typeof formulaPng
  cache?: MediaPreparationCache
  checkAssets?: (ids: string[]) => Promise<string[]>
  signal?: AbortSignal
  onProgress?: (progress: MediaProgress) => void
}
async function formulaPng(latex: string) {
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
function mediaKey(element: Element) {
  const latex = element.getAttribute('data-latex')
  if (latex !== null) return 'formula:' + latex
  const id = element.getAttribute('data-resource-id')
  if (id) return 'image:' + id
  const source = element.getAttribute('src') || ''
  if (/^data:image\/(png|jpeg);base64,[A-Za-z0-9+/]+=*$/u.test(source)) return 'inline:' + source
  throw new Error('图片没有本地资源编号，请先重新导入图片；不能发布外部图片链接。')
}
export async function prepareMedia(paper: Paper, dependencies: MediaDependencies) {
  checkMediaCancellation(dependencies.signal)
  const result = new Map<string, PreparedImage>()
  const fields: RichContent[] = paper.items.flatMap(i => [i.snapshot.stem, i.snapshot.answer, i.snapshot.explanation, ...i.snapshot.options.map(o => o.content)])
  const keys = new Set<string>()
  for (const field of fields) {
    const doc = new DOMParser().parseFromString(field.html || '', 'text/html')
    for (const element of doc.querySelectorAll('[data-latex],img')) {
      if (element.parentElement?.closest('[data-latex]')) continue
      keys.add(mediaKey(element))
    }
  }
  if (keys.size > 1000) throw new Error('单份试卷最多引用 1000 个图片资源')
  const report = (phase: MediaProgress['phase'], retryAfterSeconds?: number) => dependencies.onProgress?.({ completed: result.size, total: keys.size, phase, retryAfterSeconds })
  report('preparing')
  let retries = 0
  async function upload(image: { mimeType: string; dataBase64: string }) {
    for (let attempt = 0; ; attempt++) {
      checkMediaCancellation(dependencies.signal)
      report('uploading')
      try { return await dependencies.upload(image) }
      catch (reason) {
        checkMediaCancellation(dependencies.signal)
        const error = reason as { status?: number; code?: string; retryAfterSeconds?: number } | null
        if (error?.status !== 429 || error.code !== 'RATE_LIMITED') throw reason
        if (attempt >= 3 || retries >= 30) throw new Error('服务持续繁忙，本次发布已停止，请稍后重试。')
        retries++
        const seconds = Number(error.retryAfterSeconds)
        const delay = Number.isFinite(seconds) ? Math.max(1, Math.min(120, Math.ceil(seconds))) : 60
        report('waiting', delay)
        await waitForRetry(delay, dependencies.signal)
      }
    }
  }
  for (const key of keys) {
    if (!key.startsWith('formula:')) continue
    const latex = key.slice(8)
    if (!latex.trim() || latex.length > 4000) throw new Error('公式为空或过长，无法发布')
    katex.renderToString(latex, { output: 'html', displayMode: false, throwOnError: true, trust: false, maxExpand: 1000, maxSize: 20, strict: 'error' })
  }
  const cacheKey = (key: string) => `${katex.version}:png-v1:${key}`
  // Validate receipts in one small request before reusing an asset that might
  // have been removed from the server since the previous publication.
  if (dependencies.cache && dependencies.checkAssets) {
    const cached = [...keys].flatMap(key => {
      const item = dependencies.cache!.uploads.get(cacheKey(key))
      return item ? [{ key, item }] : []
    })
    const ids = [...new Set(cached.map(({ item }) => item.id))]
    const valid = new Set<string>()
    for (let offset = 0; offset < ids.length; offset += 256) {
      checkMediaCancellation(dependencies.signal)
      for (const id of await dependencies.checkAssets(ids.slice(offset, offset + 256))) valid.add(id)
    }
    for (const { key, item } of cached) {
      if (valid.has(item.id)) result.set(key, item)
      else dependencies.cache.uploads.delete(cacheKey(key))
    }
  }
  for (const key of keys) {
    checkMediaCancellation(dependencies.signal)
    if (result.has(key)) continue
    if (key.startsWith('formula:')) {
      const image = dependencies.cache?.formulas.get(cacheKey(key))
        ?? await (dependencies.formula ?? formulaPng)(key.slice(8))
      dependencies.cache?.formulas.set(cacheKey(key), image)
      const uploaded = await upload({ mimeType: image.mimeType, dataBase64: image.dataBase64 })
      result.set(key, { id: uploaded.id, width: image.width, height: Math.max(1, Math.round(uploaded.height * image.width / uploaded.width)) })
    } else {
      let image: { mimeType: string; dataBase64: string }
      if (key.startsWith('image:')) image = await dependencies.getImage(key.slice(6))
      else {
        const [, mimeType, dataBase64] = key.slice(7).match(/^data:(image\/(?:png|jpeg));base64,(.*)$/)!
        image = { mimeType: mimeType!, dataBase64: dataBase64! }
      }
      const uploaded = await upload(image)
      result.set(key, { id: uploaded.id, width: uploaded.width, height: uploaded.height })
    }
    if (dependencies.checkAssets) dependencies.cache?.uploads.set(cacheKey(key), result.get(key)!)
    report('uploading')
  }
  checkMediaCancellation(dependencies.signal)
  report('uploading')
  return result
}
