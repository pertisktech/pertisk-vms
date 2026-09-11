import { useEffect, useRef, useState } from 'react'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import '@xterm/xterm/css/xterm.css'
import { getToken } from '../../api'
import { Btn, Icon } from '../../components/Icons'
import { useNode } from '../NodeView'

function scheduleFit(fit, term, socket) {
  const run = () => {
    try {
      fit.fit()
      if (socket?.readyState === 1 && term) {
        socket.send(JSON.stringify({ type: 'resize', cols: term.cols, rows: term.rows }))
      }
    } catch {
      /* host may be hidden briefly */
    }
  }
  requestAnimationFrame(() => {
    run()
    requestAnimationFrame(run)
  })
}

export default function NodeShell() {
  const { node, nodeId, inv } = useNode()
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
        '"MesloLGS NF", "JetBrainsMono Nerd Font", "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, Consolas, monospace',
      fontSize: 13,
      lineHeight: 1,
      theme: {
        background: '#0b0d12',
        foreground: '#c8c9de',
        cursor: '#c8c9de',
      },
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
    socket.onopen = () => {
      if (!cancelled) {
        everConnectedRef.current = true
        setConnected(true)
        setConnecting(false)
        setWsError('')
        scheduleFit(fit, term, socket)
        window.setTimeout(() => scheduleFit(fit, term, socket), 50)
        window.setTimeout(() => scheduleFit(fit, term, socket), 200)
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

    const onResize = () => scheduleFit(fit, term, socket)
    window.addEventListener('resize', onResize)
    if (typeof ResizeObserver !== 'undefined') {
      ro = new ResizeObserver(onResize)
      ro.observe(host)
    }
    if (document.fonts?.ready) {
      document.fonts.ready.then(() => {
        if (!cancelled) scheduleFit(fit, term, socket)
      })
    }

    return () => {
      cancelled = true
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
