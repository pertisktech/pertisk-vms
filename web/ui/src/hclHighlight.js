const KEYWORDS = new Set([
  'terraform',
  'required_providers',
  'provider',
  'resource',
  'variable',
  'output',
  'locals',
  'module',
  'data',
  'lifecycle',
  'dynamic',
  'moved',
  'import',
  'check',
  'provider_installation',
  'dev_overrides',
  'direct',
])

const BLOCK_LABEL = new Set(['resource', 'data', 'provider', 'module'])
const LITERALS = new Set(['true', 'false', 'null'])
const TYPES = new Set(['string', 'number', 'bool', 'list', 'map', 'object', 'any', 'set', 'tuple'])

function isIdentStart(c) {
  return (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z') || c === '_'
}

function isIdentPart(c) {
  return isIdentStart(c) || (c >= '0' && c <= '9') || c === '-'
}

function isWs(c) {
  return c === ' ' || c === '\t' || c === '\n' || c === '\r'
}

function peekWord(src, i) {
  if (!isIdentStart(src[i] || '')) return ''
  let j = i + 1
  while (j < src.length && isIdentPart(src[j])) j += 1
  return src.slice(i, j)
}

function skipQuoted(src, i) {
  // i points at opening quote
  i += 1
  while (i < src.length) {
    if (src[i] === '\\') {
      i += src[i + 1] ? 2 : 1
      continue
    }
    if (src[i] === '"') return i + 1
    i += 1
  }
  return i
}

function findInterpEnd(src, start) {
  let i = start
  let depth = 1
  while (i < src.length && depth > 0) {
    if (src[i] === '"') {
      i = skipQuoted(src, i)
      continue
    }
    if (src[i] === '{') depth += 1
    else if (src[i] === '}') {
      depth -= 1
      if (depth === 0) return i
    }
    i += 1
  }
  return i
}

function consumeString(src, start, push, kind, state) {
  push(kind, '"')
  let i = start + 1
  let buf = i
  const n = src.length
  while (i < n) {
    if (src[i] === '\\') {
      i += src[i + 1] ? 2 : 1
      continue
    }
    if (src[i] === '$' && src[i + 1] === '{') {
      if (i > buf) push(kind, src.slice(buf, i))
      push('interp', '${')
      const end = findInterpEnd(src, i + 2)
      tokenizeChunk(src.slice(i + 2, end), push, state)
      if (src[end] === '}') {
        push('interp', '}')
        i = end + 1
      } else {
        i = end
      }
      buf = i
      continue
    }
    if (src[i] === '"') {
      if (i > buf) push(kind, src.slice(buf, i))
      push(kind, '"')
      return i + 1
    }
    i += 1
  }
  if (i > buf) push(kind, src.slice(buf, i))
  return i
}

function consumeHeredoc(src, start, push) {
  let i = start
  const strip = src.startsWith('<<-', start)
  i += strip ? 3 : 2
  while (i < src.length && isWs(src[i]) && src[i] !== '\n') i += 1
  const marker = peekWord(src, i)
  if (!marker) {
    push('punct', src.slice(start, start + 2))
    return start + 2
  }
  i += marker.length
  push('string', src.slice(start, i))
  const close = new RegExp(`\\n[ \\t]*${marker}\\b`)
  const rest = src.slice(i)
  const m = close.exec(rest)
  if (!m) {
    push('string', rest)
    return src.length
  }
  push('string', rest.slice(0, m.index + m[0].length))
  return i + m.index + m[0].length
}

function tokenizeChunk(src, push, state) {
  let i = 0
  const n = src.length
  let typeStringsLeft = state?.typeStringsLeft || 0

  while (i < n) {
    const c = src[i]

    if (isWs(c)) {
      let j = i + 1
      while (j < n && isWs(src[j])) j += 1
      push('ws', src.slice(i, j))
      i = j
      continue
    }

    if (c === '#') {
      let j = i + 1
      while (j < n && src[j] !== '\n') j += 1
      push('comment', src.slice(i, j))
      i = j
      continue
    }

    if (c === '/' && src[i + 1] === '/') {
      let j = i + 2
      while (j < n && src[j] !== '\n') j += 1
      push('comment', src.slice(i, j))
      i = j
      continue
    }

    if (c === '/' && src[i + 1] === '*') {
      let j = src.indexOf('*/', i + 2)
      j = j === -1 ? n : j + 2
      push('comment', src.slice(i, j))
      i = j
      continue
    }

    if (c === '<' && src[i + 1] === '<') {
      i = consumeHeredoc(src, i, push)
      continue
    }

    if (c === '"') {
      const kind = typeStringsLeft > 0 ? 'type' : 'string'
      if (typeStringsLeft > 0) typeStringsLeft -= 1
      i = consumeString(src, i, push, kind, { typeStringsLeft: 0 })
      continue
    }

    if ((c >= '0' && c <= '9') || (c === '-' && src[i + 1] >= '0' && src[i + 1] <= '9')) {
      let j = i + 1
      while (j < n && ((src[j] >= '0' && src[j] <= '9') || src[j] === '.' || src[j] === '_')) j += 1
      push('number', src.slice(i, j))
      i = j
      continue
    }

    if (isIdentStart(c)) {
      let j = i + 1
      while (j < n && isIdentPart(src[j])) j += 1
      const word = src.slice(i, j)
      let k = j
      while (k < n && (src[k] === ' ' || src[k] === '\t')) k += 1
      if (KEYWORDS.has(word)) {
        push('keyword', word)
        if (BLOCK_LABEL.has(word)) typeStringsLeft = 1
      } else if (LITERALS.has(word)) {
        push('literal', word)
      } else if (TYPES.has(word)) {
        push('type', word)
      } else if (src[k] === '(') {
        push('function', word)
      } else {
        push('ident', word)
      }
      i = j
      continue
    }

    if (c === '$' && src[i + 1] === '{') {
      push('interp', '${')
      i += 2
      continue
    }

    push('punct', c)
    i += 1
  }

  if (state) state.typeStringsLeft = typeStringsLeft
}

export function tokenizeHcl(src) {
  const tokens = []
  const state = { typeStringsLeft: 0 }
  function push(type, text) {
    if (!text) return
    const last = tokens[tokens.length - 1]
    if (last && last.type === type) last.text += text
    else tokens.push({ type, text })
  }
  tokenizeChunk(String(src ?? ''), push, state)
  return tokens
}

export function hclLines(src) {
  const tokens = tokenizeHcl(src)
  const lines = [[]]
  for (const tok of tokens) {
    const parts = tok.text.split('\n')
    parts.forEach((part, idx) => {
      if (idx) lines.push([])
      if (part) lines[lines.length - 1].push({ type: tok.type, text: part })
    })
  }
  if (lines.length > 1 && lines[lines.length - 1].length === 0) lines.pop()
  return lines
}
