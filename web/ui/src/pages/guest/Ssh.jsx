import { useEffect, useMemo, useRef, useState } from 'react'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import '@xterm/xterm/css/xterm.css'
import { defaultCloudUser, disksOf, getToken, netsOf, nicAddrs } from '../../api'
import { Btn, Icon } from '../../components/Icons'
import { useTheme } from '../../ThemeContext'
import { applyXtermTheme } from '../../termTheme'
import { useGuest } from '../GuestView'

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

function guestSshMeta(vm) {
  const nic = netsOf(vm)[0]
  const addrs = nicAddrs(nic)
  const host = addrs[0] || ''
  const hints = [vm?.spec?.name, ...(disksOf(vm) || []).map((d) => d.path || d.iso_name)].filter(Boolean)
  const user = (vm?.spec?.ssh_user || '').trim() || defaultCloudUser(...hints)
  return { host, user }
}

export default function GuestSsh({ popup = false }) {
  const { vm, vmId, canWrite } = useGuest()
  const { terminalTheme } = useTheme()
  const meta = useMemo(() => guestSshMeta(vm), [vm])
  const [user, setUser] = useState(meta.user)
  const [connected, setConnected] = useState(false)
  const [connecting, setConnecting] = useState(true)
  const [wsError, setWsError] = useState('')
  const [fullscreen, setFullscreen] = useState(false)
  const termRef = useRef(null)
  const termHostRef = useRef(null)
  const fitRef = useRef(null)
  const wsRef = useRef(null)
  const wrapRef = useRef(null)
  const everConnectedRef = useRef(false)
  const sessionKey = `${vmId}:${user}`

  useEffect(() => {
    setUser(meta.user)
  }, [meta.user, vmId])

  useEffect(() => {
    if (!canWrite) {
      setConnecting(false)
      setConnected(false)
      setWsError('Operator role required for guest SSH')
      return
    }
    if (!meta.host) {
      setConnecting(false)
      setConnected(false)
      setWsError('Guest has no IP yet — wait for DHCP/cloud-init')
      return
    }
    if (vm?.state !== 'running') {
      setConnecting(false)
      setConnected(false)
      setWsError('Guest is not running')
      return
    }

    let cancelled = false
    let term
    let fit
    let socket
    let ro
    const host = termHostRef.current
    if (!host) return

    setConnecting(true)
    setConnected(false)
    setWsError('')
    everConnectedRef.current = false

    term = new Terminal({
      cursorBlink: true,
      fontFamily:
        '"Geist Mono", "MesloLGS NF", ui-monospace, SFMono-Regular, Menlo, Consolas, monospace',
      fontSize: 13,
      lineHeight: 1,
      theme: terminalTheme,
      convertEol: false,
      allowProposedApi: true,
      scrollback: 2000,
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
    const q = new URLSearchParams({
      token: getToken(),
      user: user || meta.user,
    })
    socket = new WebSocket(`${proto}//${location.host}/v1/vms/${vmId}/ssh/ws?${q}`)
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
  }, [sessionKey, canWrite, meta.host, meta.user, user, vm?.state, vmId])

  useEffect(() => {
    const term = termRef.current
    const host = termHostRef.current
    if (!term || !host) return
    applyXtermTheme(term, host, terminalTheme)
    scheduleFit(fitRef.current, term, host, wsRef.current)
  }, [terminalTheme])

  useEffect(() => {
    function onFs() {
      setFullscreen(Boolean(document.fullscreenElement))
      scheduleFit(fitRef.current, termRef.current, termHostRef.current, wsRef.current)
    }
    document.addEventListener('fullscreenchange', onFs)
    return () => document.removeEventListener('fullscreenchange', onFs)
  }, [])

  async function toggleFullscreen() {
    const el = wrapRef.current
    if (!el) return
    try {
      if (document.fullscreenElement) {
        await document.exitFullscreen()
      } else {
        await el.requestFullscreen()
      }
    } catch (err) {
      setWsError(String(err?.message || err))
    }
  }

  function openNewWindow() {
    const url = `${location.origin}${location.pathname}#/vm/${vmId}/ssh-popup`
    window.open(url, `pertisk-ssh-${vmId}`, 'noopener,noreferrer,width=1100,height=720')
  }

  const showConnecting = connecting && !everConnectedRef.current
  const statusLabel = connecting ? 'connecting' : connected ? 'connected' : 'disconnected'
  const statusClass = connecting ? 'pending' : connected ? 'ready' : 'unknown'
  const target = meta.host ? `${user || meta.user}@${meta.host}` : 'no IP'

  return (
    <div ref={wrapRef} className={`pve-console-wrap${popup ? ' pve-ssh-popup' : ''}${fullscreen ? ' is-fullscreen' : ''}`}>
      <div className="pve-console-bar">
        <span className="console-traffic" aria-hidden>
          <span />
          <span />
          <span />
        </span>
        <span className={`badge ${statusClass}`}>{statusLabel}</span>
        <label className="ssh-user-field">
          <span className="muted">User</span>
          <input
            value={user}
            onChange={(e) => setUser(e.target.value.trim() || meta.user)}
            spellCheck={false}
            disabled={connecting}
          />
        </label>
        <span className="muted mono-inline">{target}</span>
        {wsError && <span className="muted">{wsError}</span>}
        <span className="pve-header-spacer" />
        {!popup && (
          <Btn icon="external" variant="secondary" onClick={openNewWindow} title="Open in new window">
            New window
          </Btn>
        )}
        <Btn
          icon={fullscreen ? 'minimize' : 'maximize'}
          variant="secondary"
          onClick={toggleFullscreen}
          title={fullscreen ? 'Exit fullscreen' : 'Fullscreen'}
        >
          {fullscreen ? 'Exit' : 'Fullscreen'}
        </Btn>
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
            Connecting SSH to {vm?.spec?.name || vmId}…
          </div>
        )}
      </div>
    </div>
  )
}
