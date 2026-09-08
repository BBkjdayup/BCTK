import { afterEach, describe, expect, it, vi } from 'vitest'
import { printCanvasPages } from './canvasPrint'

afterEach(() => {
  document.querySelector('[data-paper-canvas-print-frame]')?.remove()
  vi.restoreAllMocks()
})

describe('printCanvasPages', () => {
  it('prints every generated page from an isolated document and cleans up afterward', async () => {
    const printJob = printCanvasPages([
      'data:image/png;base64,one',
      'data:image/png;base64,two',
      'data:image/png;base64,three',
    ], {
      width: 1123,
      height: 794,
    })

    const frame = document.querySelector<HTMLIFrameElement>('[data-paper-canvas-print-frame]')
    const printWindow = frame?.contentWindow
    expect(frame).not.toBeNull()
    expect(printWindow).not.toBeNull()
    const print = vi.spyOn(printWindow!, 'print').mockImplementation(() => undefined)

    await printJob

    expect(print).toHaveBeenCalledOnce()
    expect(printWindow?.document.querySelectorAll('.paper-print-page')).toHaveLength(3)
    expect(printWindow?.document.querySelectorAll('.paper-print-page img')).toHaveLength(3)
    expect(printWindow?.document.body.style.overflow).not.toBe('hidden')
    expect(printWindow?.document.querySelector('style')?.textContent)
      .toContain('size: 297mm 210mm')

    printWindow?.dispatchEvent(new Event('afterprint'))
    expect(document.querySelector('[data-paper-canvas-print-frame]')).toBeNull()
  })

  it('refuses to open an empty print job', async () => {
    await expect(printCanvasPages([], { width: 794, height: 1123 }))
      .rejects.toThrow('没有可打印的试卷页面')
    expect(document.querySelector('[data-paper-canvas-print-frame]')).toBeNull()
  })

  it('refuses an invalid paper size', async () => {
    await expect(printCanvasPages(['data:image/png;base64,one'], { width: 0, height: 1123 }))
      .rejects.toThrow('打印纸张尺寸无效')
    expect(document.querySelector('[data-paper-canvas-print-frame]')).toBeNull()
  })
})
