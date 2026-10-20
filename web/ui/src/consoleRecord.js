/** Safe download name from a guest name. */
export function recordStem(name, id) {
  const raw = String(name || id || 'guest')
  const safe = raw.replace(/[^A-Za-z0-9._-]+/g, '-').replace(/^-+|-+$/g, '')
  return safe || 'guest'
}

/** Append serial pages until `from` reaches `size`. */
export function foldSerial(size, pages) {
  let text = ''
  let from = 0
  const end = Number(size) || 0
  for (const page of pages) {
    text += page?.text || ''
    const next = Number(page?.next)
    if (!Number.isFinite(next) || next <= from) break
    from = next
    if (from >= end) break
  }
  return text
}

/** First MediaRecorder mime the browser accepts. */
export function recorderMime(isSupported) {
  const types = ['video/webm;codecs=vp8', 'video/webm']
  return types.find((type) => isSupported(type)) || ''
}

export function saveBlob(blob, filename) {
  const url = URL.createObjectURL(blob)
  const link = document.createElement('a')
  link.href = url
  link.download = filename
  link.click()
  setTimeout(() => URL.revokeObjectURL(url), 1000)
}
