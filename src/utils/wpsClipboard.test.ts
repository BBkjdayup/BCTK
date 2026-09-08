import { describe, expect, it } from 'vitest'
import { embedWpsClipboardImages, wpsClipboardImagePaths } from './wpsClipboard'

describe('WPS clipboard HTML', () => {
  const html = `
    <p>1.<span><img width="30" height="16"
      src="file:///C:\\Users\\Teacher\\AppData\\Local\\Temp\\ksohtml19104\\wps1.png"></span>题目</p>
    <p>A.<img src="file:///C:/Users/Teacher/AppData/Local/Temp/ksohtml19104/wps2.jpg">
      <span style="mso-tab-count:1"> </span>B.</p>
  `

  it('extracts only numbered WPS temporary images', () => {
    expect(wpsClipboardImagePaths(`${html}<img src="file:///C:/outside.png">`)).toEqual([
      'C:\\Users\\Teacher\\AppData\\Local\\Temp\\ksohtml19104\\wps1.png',
      'C:\\Users\\Teacher\\AppData\\Local\\Temp\\ksohtml19104\\wps2.jpg',
    ])
  })

  it('inserts managed image references, preserves dimensions and removes executable markup', () => {
    const paths = wpsClipboardImagePaths(html)
    const embedded = embedWpsClipboardImages(
      `${html}<script>alert(1)</script><span onclick="alert(2)">安全文字</span>`,
      paths.map((path, index) => ({
        path,
        resourceId: `019fa7fc-2c03-7111-a8b6-4166afe943b${index}`,
        mimeType: index ? 'image/jpeg' : 'image/png',
        dataBase64: index ? 'jpeg-bytes' : 'png-bytes',
        widthPx: 40,
        heightPx: 20,
      })),
    )

    expect(embedded).toContain('data-resource-id="019fa7fc-2c03-7111-a8b6-4166afe943b0"')
    expect(embedded).toContain('data-node-id=')
    expect(embedded).toContain('width="30"')
    expect(embedded).toContain('height="16"')
    expect(embedded).toContain('data-resource-id="019fa7fc-2c03-7111-a8b6-4166afe943b1"')
    expect(embedded).toContain('width="40"')
    expect(embedded).toContain('  ')
    expect(embedded).not.toContain('<script')
    expect(embedded).not.toContain('onclick')
    expect(embedded).not.toContain('file:')
    expect(embedded).not.toContain('data:image/')
  })
})
