// The already published installer remains usable if the release service is unavailable.
const releasePattern = /^\d+\.\d+\.\d+$/

export function readRelease(manifest) {
  if (!manifest || !releasePattern.test(manifest.version ?? '')) return null
  const platform = manifest.platforms?.['windows-x86_64']
  if (!platform || typeof platform.url !== 'string') return null
  const expectedUrl = `https://api.tktiku.cn/updates/files/tktiku-desktop_${manifest.version}_windows_x86_64-setup.exe`
  if (platform.url !== expectedUrl) return null
  const published = new Date(manifest.pub_date)
  return {
    version: manifest.version,
    url: expectedUrl,
    date: Number.isNaN(published.getTime()) ? null : published.toLocaleDateString('sv-SE', { timeZone: 'Asia/Shanghai' }),
  }
}

async function refreshDownload() {
  try {
    const response = await fetch('/updates/latest.json', {
      cache: 'no-store',
      credentials: 'omit',
      signal: AbortSignal.timeout(6000),
    })
    if (!response.ok) return
    const release = readRelease(await response.json())
    if (!release) return
    document.querySelectorAll('[data-download]').forEach((link) => {
      link.href = release.url
    })
    document.querySelectorAll('.release-version').forEach((label) => {
      label.textContent = `v${release.version}`
    })
    document.querySelectorAll('.release-date').forEach((label) => {
      label.textContent = release.date ? `${release.date} 发布` : ''
    })
  } catch {
    // Keep the verified fallback link. Never block a download on a metadata request.
  }
}

if (typeof document !== 'undefined') {
  const year = document.getElementById('copyright-year')
  if (year) year.textContent = String(new Date().getFullYear())
  document.querySelectorAll('a[href="#trial-faq"]').forEach((link) => {
    link.addEventListener('click', () => {
      const details = document.getElementById('trial-faq')
      if (details) details.open = true
    })
  })
  if (window.location.hash === '#trial-faq') document.getElementById('trial-faq').open = true
  void refreshDownload()
}
