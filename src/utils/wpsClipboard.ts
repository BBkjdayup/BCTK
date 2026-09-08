import type { ManagedImagePayload } from '../types/domain'

export interface ManagedWpsClipboardImagePayload extends ManagedImagePayload {
  path: string
}

function fileUrlToWindowsPath(source: string) {
  if (!/^file:\/+/iu.test(source)) return null
  let path = source.replace(/^file:\/+/iu, '')
  try {
    path = decodeURIComponent(path)
  } catch {
    return null
  }
  path = path.replace(/\//g, '\\')
  return /^[a-z]:\\/iu.test(path) ? path : null
}

function parseHtml(html: string) {
  return new DOMParser().parseFromString(html, 'text/html')
}

export function wpsClipboardImagePaths(html: string) {
  const document = parseHtml(html)
  const paths = [...document.querySelectorAll<HTMLImageElement>('img[src]')]
    .map((image) => fileUrlToWindowsPath(image.getAttribute('src') ?? ''))
    .filter((path): path is string => Boolean(path))
    .filter((path) => /\\ksohtml\d+\\wps\d+\.(?:png|jpe?g)$/iu.test(path))
  return [...new Set(paths)]
}

export function embedWpsClipboardImages(
  html: string,
  payloads: readonly ManagedWpsClipboardImagePayload[],
) {
  const document = parseHtml(html)
  const byPath = new Map(payloads.map((payload) => [payload.path.toLowerCase(), payload]))

  document.querySelectorAll('script,style,link,meta,object,embed,iframe').forEach((node) => node.remove())
  document.querySelectorAll<HTMLElement>('*').forEach((element) => {
    for (const attribute of [...element.attributes]) {
      if (attribute.name.toLowerCase().startsWith('on')) {
        element.removeAttribute(attribute.name)
      }
    }
    if ((element.getAttribute('style') ?? '').toLowerCase().includes('mso-tab-count')) {
      element.textContent = '\u2003\u2003'
    }
  })

  for (const image of document.querySelectorAll<HTMLImageElement>('img[src]')) {
    const path = fileUrlToWindowsPath(image.getAttribute('src') ?? '')
    const payload = path ? byPath.get(path.toLowerCase()) : null
    if (!payload) {
      if (path) {
        const replacement = document.createElement('span')
        replacement.textContent = image.alt || '[图片已失效]'
        image.replaceWith(replacement)
      }
      continue
    }
    image.removeAttribute('src')
    image.dataset.resourceId = payload.resourceId
    image.dataset.nodeId = crypto.randomUUID()
    if (!image.getAttribute('width') && payload.widthPx) image.width = payload.widthPx
    if (!image.getAttribute('height') && payload.heightPx) image.height = payload.heightPx
    if (!image.alt) image.alt = 'WPS 粘贴图片'
  }
  return document.body.innerHTML
}
