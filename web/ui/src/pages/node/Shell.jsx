import { useEffect, useRef, useState } from 'react'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import '@xterm/xterm/css/xterm.css'
import { getToken } from '../../api'
import { Btn, Icon } from '../../components/Icons'
import { useTheme } from '../../ThemeContext'
import { useNode } from '../NodeView'

function cellSize(term) {
  return term?._core?._renderService?.dimensions?.css?.cell || null
}

function fitFill(fit, term, host, socket) {
  try {
    fit.fit()
    const cell = cellSize(term)
    if (host && cell?.width > 0 && cell?.height > 0) {
      const cols = Math.max(2, Math.floor(host.clientWidth / cell.width))
      const rows = Math.max(1, Math.ceil(host.clientHeight / cell.height - 1e-6))
      if (term.cols !== cols || term.rows !== rows) {
        term.resize(cols, rows)
      }
    }
    if (socket?.readyState === 1 && term) {
      socket.send(JSON.stringify({ type: 'resize', cols: term.cols, rows: term.rows }))
    }
  } catch {
    /* host may be hidden briefly */
  }
}

function scheduleFit(fit, term, host, socket) {
  const run = () => fitFill(fit, term, host, socket)
  requestAnimationFrame(() => {
    run()
    requestAnimationFrame(run)
  })
}

export default function NodeShell() {
  const { node, nodeId, inv } = useNode()
  const { terminalTheme } = useTheme()
  const members = inv.cluster?.members || []
  const self = !inv.cluster?.self_id || inv.cluster.self_id === nodeId
  const [connected, setConnected] = useState(false)
  const [connecting, setConnecting] = useState(true)
  const [wsError, setWsError] = useState('')
  const termRef = useRef(null)
  const termHostRef = useRef(null)
  const fitRef = useRef(null)
  const wsRef = useRef(null)
  const everConnectedRef = useRef(false)

  useEffect(() => {
    if (!self) {
      setConnecting(false)
      setConnected(false)
      return
    }

    let cancelled = false
    let term
    let fit
    let socket
    let ro
    const host = termHostRef.current
    if (!host) return

    term = new Terminal({
      cursorBlink: true,
      fontFamily:
        '"Geist Mono", "MesloLGS NF", ui-monospace, SFMono-Regular, Menlo, Consolas, monospace',
      fontSize: 13,
      lineHeight: 1,
      theme: terminalTheme,
      convertEol: false,
      allowProposedApi: true,
      scrollback: 1000,
      disableStdin: false,
    })
    fit = new FitAddon()
    term.loadAddon(fit)
    term.open(host)
    termRef.current = term
    fitRef.current = fit

    const focusTerm = () => {
      try {
        term.focus()
      } catch {
        /* ignore */
      }
    }
    focusTerm()
    host.addEventListener('mousedown', focusTerm)

    const proto = location.protocol === 'https:' ? 'wss:' : 'ws:'
    socket = new WebSocket(
      `${proto}//${location.host}/v1/node/shell/ws?token=${encodeURIComponent(getToken())}`,
    )
    socket.binaryType = 'arraybuffer'
    wsRef.current = socket
    const timers = [
      window.setTimeout(() => {
        if (!cancelled) scheduleFit(fit, term, host, socket)
      }, 50),
      window.setTimeout(() => {
        if (!cancelled) scheduleFit(fit, term, host, socket)
      }, 200),
    ]
    socket.onopen = () => {
      if (!cancelled) {
        everConnectedRef.current = true
        setConnected(true)
        setConnecting(false)
        setWsError('')
        scheduleFit(fit, term, host, socket)
        focusTerm()
      }
    }
    socket.onclose = (ev) => {
      if (!cancelled) {
        setConnected(false)
        setConnecting(false)
        if (!ev.wasClean && ev.code !== 1000) {
          setWsError(`WebSocket closed (${ev.code})`)
        }
      }
    }
    socket.onerror = () => {
      if (!cancelled) {
        setConnecting(false)
        setWsError('WebSocket failed')
      }
    }
    socket.onmessage = (e) => {
      let text = ''
      if (typeof e.data === 'string') text = e.data
      else if (e.data instanceof ArrayBuffer) text = new TextDecoder().decode(e.data)
      else return
      try {
        term.write(text)
      } catch {
        /* disposed */
      }
    }
    term.onData((data) => {
      if (socket.readyState === 1) socket.send(data)
    })
    term.onResize(({ cols, rows }) => {
      if (socket.readyState === 1) {
        socket.send(JSON.stringify({ type: 'resize', cols, rows }))
      }
    })

    const onResize = () => scheduleFit(fit, term, host, socket)
    window.addEventListener('resize', onResize)
    if (typeof ResizeObserver !== 'undefined') {
      ro = new ResizeObserver(onResize)
      ro.observe(host)
      const stack = host.parentElement
      if (stack) ro.observe(stack)
    }
    if (document.fonts?.ready) {
      document.fonts.ready.then(() => {
        if (!cancelled) scheduleFit(fit, term, host, socket)
      })
    }

    return () => {
      cancelled = true
      timers.forEach((id) => window.clearTimeout(id))
      host.removeEventListener('mousedown', focusTerm)
      window.removeEventListener('resize', onResize)
      ro?.disconnect()
      socket.onclose = null
      socket.close()
      wsRef.current = null
      term.dispose()
      termRef.current = null
      fitRef.current = null
      setConnected(false)
      setConnecting(false)
    }
  }, [self, nodeId])

  useEffect(() => {
    if (!self) return
    const term = termRef.current
    const host = termHostRef.current
    if (!term || !host) return
    term.options.theme = terminalTheme
    host.style.background = terminalTheme.background
    try {
      term.refresh(0, Math.max(0, term.rows - 1))
    } catch {
      /* ignore */
    }
    scheduleFit(fitRef.current, term, host, wsRef.current)
  }, [self, terminalTheme])

  if (!self) {
    const name = node?.name || 'this node'
    const peer = members.find((m) => m.id === nodeId)
    const hint = peer?.peer_url || peer?.ipv4?.[0] || ''
    return (
      <div className="dash-empty card">
        <strong>Shell runs on the node itself</strong>
        <p className="muted">
          Open the UI on {name}
          {hint ? ` (${hint})` : ''} to use its host shell.
        </p>
      </div>
    )
  }

  const showConnecting = connecting && !everConnectedRef.current
  const statusLabel = connecting ? 'connecting' : connected ? 'connected' : 'disconnected'
  const statusClass = connecting ? 'pending' : connected ? 'ready' : 'unknown'

  return (
    <div className="pve-console-wrap">
      <div className="pve-console-bar">
        <span className="console-traffic" aria-hidden>
          <span />
          <span />
          <span />
        </span>
        <span className={`badge ${statusClass}`}>{statusLabel}</span>
        <span className="muted">
          {wsError || 'Root zsh on this hypervisor (Oh My Zsh + Powerlevel10k). Guests are not affected.'}
        </span>
        <span className="pve-header-spacer" />
        <Btn icon="trash" variant="secondary" onClick={() => termRef.current?.clear()}>
          Clear
        </Btn>
      </div>
      <div
        className="console-pane-stack"
        onMouseDown={() => {
          try {
            termRef.current?.focus()
          } catch {
            /* ignore */
          }
        }}
      >
        <div ref={termHostRef} className="console-pane pve-console console-xterm" />
        {showConnecting && (
          <div className="console-loading" aria-live="polite">
            <Icon name="refresh" size={18} />
            Connecting to host shell…
          </div>
        )}
      </div>
    </div>
  )
}
