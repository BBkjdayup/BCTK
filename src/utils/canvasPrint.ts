const PRINT_FRAME_ATTRIBUTE = 'data-paper-canvas-print-frame'

export interface CanvasPrintOptions {
  width: number
  height: number
}

function pageSizeMillimetres(options: CanvasPrintOptions) {
  const shortEdge = Math.min(options.width, options.height)
  const longEdge = Math.max(options.width, options.height)
  const landscape = options.width > options.height
  const knownSize = [
    { width: 565, height: 796, widthMm: 148, heightMm: 210 },
    { width: 794, height: 1123, widthMm: 210, heightMm: 297 },
    { width: 1125, height: 1593, widthMm: 297, heightMm: 420 },
  ].find((size) => (
    Math.abs(size.width - shortEdge) <= 2
    && Math.abs(size.height - longEdge) <= 2
  ))
  const portraitWidth = knownSize?.widthMm ?? shortEdge * (25.4 / 96)
  const portraitHeight = knownSize?.heightMm ?? longEdge * (25.4 / 96)
  return landscape
    ? { width: portraitHeight, height: portraitWidth }
    : { width: portraitWidth, height: portraitHeight }
}

function removeExistingPrintFrame() {
  document.querySelector(`[${PRINT_FRAME_ATTRIBUTE}]`)?.remove()
}

export async function printCanvasPages(pageImages: readonly string[], options: CanvasPrintOptions) {
  if (!pageImages.length) throw new Error('没有可打印的试卷页面')
  if (!Number.isFinite(options.width) || options.width <= 0
    || !Number.isFinite(options.height) || options.height <= 0) {
    throw new Error('打印纸张尺寸无效')
  }

  removeExistingPrintFrame()

  const printFrame = document.createElement('iframe')
  printFrame.setAttribute(PRINT_FRAME_ATTRIBUTE, '')
  printFrame.title = '试卷打印'
  printFrame.style.position = 'fixed'
  printFrame.style.right = '0'
  printFrame.style.bottom = '0'
  printFrame.style.width = '0'
  printFrame.style.height = '0'
  printFrame.style.border = '0'
  printFrame.style.visibility = 'hidden'
  document.body.append(printFrame)

  const printWindow = printFrame.contentWindow
  if (!printWindow) {
    printFrame.remove()
    throw new Error('无法创建打印窗口')
  }

  const printDocument = printWindow.document
  printDocument.open()
  printDocument.write('<!doctype html><html><head><meta charset="UTF-8"></head><body></body></html>')
  printDocument.close()

  const pageSize = pageSizeMillimetres(options)
  const printStyle = printDocument.createElement('style')
  printStyle.textContent = `
    * { box-sizing: border-box; margin: 0; padding: 0; }
    html, body {
      width: auto !important;
      height: auto !important;
      min-width: 0 !important;
      min-height: 0 !important;
      overflow: visible !important;
      background: #fff !important;
    }
    @page {
      size: ${pageSize.width}mm ${pageSize.height}mm;
      margin: 0;
    }
    .paper-print-page {
      position: relative;
      width: ${pageSize.width}mm;
      height: ${pageSize.height}mm;
      overflow: hidden;
      break-after: page;
      page-break-after: always;
    }
    .paper-print-page:last-child {
      break-after: auto;
      page-break-after: auto;
    }
    .paper-print-page img {
      display: block;
      width: 100%;
      height: 100%;
      object-fit: fill;
    }
  `
  printDocument.head.append(printStyle)

  const imageElements = pageImages.map((source, index) => {
    const page = printDocument.createElement('section')
    page.className = 'paper-print-page'
    page.setAttribute('aria-label', `试卷第 ${index + 1} 页`)
    const image = printDocument.createElement('img')
    image.src = source
    image.alt = `试卷第 ${index + 1} 页`
    page.append(image)
    printDocument.body.append(page)
    return image
  })

  await Promise.all(imageElements.map(async (image) => {
    if (typeof image.decode !== 'function') return
    try {
      await image.decode()
    } catch {
      // The WebView can still print a data URL if eager decoding is unavailable.
    }
  }))

  let cleanupTimer = 0
  const cleanup = () => {
    printWindow.clearTimeout(cleanupTimer)
    printWindow.removeEventListener('afterprint', cleanup)
    printFrame.remove()
  }

  printWindow.addEventListener('afterprint', cleanup, { once: true })
  cleanupTimer = printWindow.setTimeout(cleanup, 120_000)

  try {
    printWindow.print()
  } catch (error) {
    cleanup()
    throw error
  }
}
