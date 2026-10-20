// Type clipboard text into a noVNC session. QEMU does not inject the VNC
// clipboard into a guest that has no agent, so each character is sent as a key.

const XK_TAB = 0xff09
const XK_RETURN = 0xff0d
const MAX_CHARS = 4096

/** X11 keysyms for the characters QEMU can turn into keystrokes. */
export function keysymsFor(text) {
  const out = []
  const chars = Array.from(text ?? '').slice(0, MAX_CHARS)
  for (let i = 0; i < chars.length; i++) {
    const ch = chars[i]
    if (ch === '\r') {
      if (chars[i + 1] !== '\n') out.push(XK_RETURN)
      continue
    }
    if (ch === '\n') {
      out.push(XK_RETURN)
      continue
    }
    if (ch === '\t') {
      out.push(XK_TAB)
      continue
    }
    const code = ch.codePointAt(0)
    if (code >= 0x20 && code <= 0xff) out.push(code)
  }
  return out
}

export async function typeKeysyms(rfb, keysyms, gapMs = 8) {
  if (!rfb || typeof rfb.sendKey !== 'function') return
  for (const keysym of keysyms) {
    rfb.sendKey(keysym, null, true)
    rfb.sendKey(keysym, null, false)
    if (gapMs > 0) {
      await new Promise((resolve) => setTimeout(resolve, gapMs))
    }
  }
}
