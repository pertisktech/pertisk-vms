import { useEffect, useRef, useState } from 'react'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import RFB from '@novnc/novnc'
import '@xterm/xterm/css/xterm.css'
import { getToken } from '../../api'
import { Btn, Icon } from '../../components/Icons'
import { useTheme } from '../../ThemeContext'
import { useGuest } from '../GuestView'

function scheduleFit(fit) {
  if (!fit) return
  requestAnimationFrame(() => {
    try {
      fit.fit()
    } catch {
      /* host may be hidden briefly */
    }
  })
}

export default function GuestConsole() {
  const { vm, vmId } = useGuest()
  const { terminalTheme } = useTheme()
  const [connected, setConnected] = useState(false)
  const [connecting, setConnecting] = useState(true)
  const [wsError, setWsError] = useState('')
  const [gotOutput, setGotOutput] = useState(false)
  // VGA login needs Display; prefer it whenever a graphics socket exists.
  const [tab, setTab] = useState(() => (vm?.graphics_socket ? 'display' : 'serial'))
  const hasGraphics = Boolean(vm?.graphics_socket)
  const graphicsPathRef = useRef(vm?.graphics_socket || '')
  const termRef = useRef(null)
  const termHostRef = useRef(null)
  const fitRef = useRef(null)
  const wsRef = useRef(null)
  const screenRef = useRef(null)
  const rfbRef = useRef(null)
  const tabRef = useRef(tab)
  const everConnectedRef = useRef(false)

  if (vm?.graphics_socket) {
    graphicsPathRef.current = vm.graphics_socket
  }
  tabRef.current = tab

  useEffect(() => {
    setConnected(false)
    setConnecting(true)
    setWsError('')
    setGotOutput(false)
    everConnectedRef.current = false
    setTab(graphicsPathRef.current ? 'display' : 'serial')
  }, [vmId])

  // Serial: xterm.js over websocket
  useEffect(() => {
    if (tab !== 'serial') return

    let cancelled = false
    let term
    let fit
    let socket
    let ro

    const host = termHostRef.current
    if (!host) return

    term = new Terminal({
      cursorBlink: true,
      fontFamily: '"Geist Mono", ui-monospace, SFMono-Regular, Menlo, Consolas, monospace',
      fontSize: 13,
      theme: terminalTheme,
      convertEol: true,
      disableStdin: false,
    })
    fit = new FitAddon()
    term.loadAddon(fit)
    term.open(host)
    termRef.current = term
    fitRef.current = fit
    scheduleFit(fit)

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
      `${proto}//${location.host}/v1/vms/${vmId}/console/ws?token=${encodeURIComponent(getToken())}`,
    )
    socket.binaryType = 'arraybuffer'
    wsRef.current = socket
    socket.onopen = () => {
      if (!cancelled) {
        everConnectedRef.current = true
        setConnected(true)
        setConnecting(false)
        setWsError('')
        scheduleFit(fit)
        focusTerm()
      }
    }
    socket.onclose = (ev) => {
      if (!cancelled) {
        setConnected(false)
        setConnecting(false)
        if (!ev.wasClean && ev.code !== 1000) {
          setWsError(
            `WebSocket closed (${ev.code}). Use http://${location.hostname}:7480/ in Chrome or Safari.`,
          )
        }
      }
    }
    socket.onerror = () => {
      if (!cancelled) {
        setConnecting(false)
        setWsError('WebSocket failed. Open the UI at http://' + location.hostname + ':7480/')
      }
    }
    socket.onmessage = (e) => {
      let text = ''
      if (typeof e.data === 'string') text = e.data
      else if (e.data instanceof ArrayBuffer) text = new TextDecoder().decode(e.data)
      else return
      if (text) setGotOutput(true)
      try {
        term.write(text)
      } catch {
        /* disposed */
      }
    }
    term.onData((data) => {
      if (socket.readyState === 1) socket.send(data)
    })

    const onResize = () => scheduleFit(fit)
    window.addEventListener('resize', onResize)
    if (typeof ResizeObserver !== 'undefined') {
      ro = new ResizeObserver(onResize)
      ro.observe(host)
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
  }, [vmId, tab])

  useEffect(() => {
    if (tab !== 'serial') return
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
    scheduleFit(fitRef.current)
  }, [tab, terminalTheme])

  useEffect(() => {
    if (tab === 'serial') scheduleFit(fitRef.current)
  }, [tab])

  // Display: noVNC
  useEffect(() => {
    if (tab !== 'display') return

    let cancelled = false
    const host = screenRef.current
    if (!host) return

    if (!graphicsPathRef.current) {
      setConnecting(false)
      setConnected(false)
      setWsError('No graphics socket for this guest yet.')
      return
    }

    host.innerHTML = ''

    const proto = location.protocol === 'https:' ? 'wss:' : 'ws:'
    const url = `${proto}//${location.host}/v1/vms/${vmId}/graphics/ws?token=${encodeURIComponent(getToken())}`

    const focusRfb = () => {
      try {
        rfbRef.current?.focus?.({ preventScroll: true })
      } catch {
        /* ignore */
      }
    }
    host.tabIndex = 0

    let rfb
    try {
      rfb = new RFB(host, url, { shared: true })
      rfb.scaleViewport = true
      rfb.clipViewport = false
      rfb.resizeSession = false
      rfb.focusOnClick = true
      rfb.viewOnly = false
      rfbRef.current = rfb
      rfb.addEventListener('connect', () => {
        if (!cancelled) {
          everConnectedRef.current = true
          setConnected(true)
          setConnecting(false)
          setWsError('')
          requestAnimationFrame(focusRfb)
        }
      })
      rfb.addEventListener('disconnect', (ev) => {
        if (!cancelled) {
          setConnected(false)
          setConnecting(false)
          if (ev?.detail?.clean === false) {
            setWsError('Display disconnected. Click Display to reconnect.')
          }
        }
      })
    } catch (err) {
      console.error('VNC init error', err)
      if (!cancelled) {
        setConnected(false)
        setConnecting(false)
        setWsError(String(err?.message || err))
      }
    }

    return () => {
      cancelled = true
      if (rfbRef.current) {
        try {
          rfbRef.current.disconnect()
        } catch {
          /* ignore */
        }
        rfbRef.current = null
      }
      host.innerHTML = ''
      setConnected(false)
      setConnecting(false)
    }
  }, [vmId, tab])

  // Enter on a focused header button (Refresh / Restart) steals login.
  // While Display is open, keep VNC focused and block that.
  useEffect(() => {
    if (tab !== 'display') return

    const onKeyDown = (e) => {
      const target = e.target
      if (!(target instanceof Element)) return

      // Allow real UI forms / modals.
      if (target.closest('.modal-backdrop, .modal-card, .user-menu')) return
      if (
        (target.tagName === 'INPUT' || target.tagName === 'SELECT' || target.tagName === 'TEXTAREA') &&
        !target.closest('.console-vnc, .console-pane-stack')
      ) {
        return
      }

      // Stop Enter/Space from activating toolbar buttons while logging in.
      if (
        (e.key === 'Enter' || e.key === ' ' || e.key === 'Spacebar') &&
        target.closest('button, a.btn, a.pve-header-btn, .pve-toolbar, .pve-header')
      ) {
        e.preventDefault()
        e.stopPropagation()
        try {
          rfbRef.current?.focus?.({ preventScroll: true })
        } catch {
          /* ignore */
        }
        return
      }

      // If focus left the canvas (inventory re-render), pull it back for typing.
      if (!target.closest('.console-vnc')) {
        try {
          rfbRef.current?.focus?.({ preventScroll: true })
        } catch {
          /* ignore */
        }
      }
    }

    window.addEventListener('keydown', onKeyDown, true)
    return () => window.removeEventListener('keydown', onKeyDown, true)
  }, [tab])

  function switchTab(next) {
    if (next === tab) return
    setConnected(false)
    setConnecting(true)
    setWsError('')
    setTab(next)
  }

  function focusConsole(e) {
    if (e.target.closest?.('button, a, input, select')) return
    if (tabRef.current === 'serial') {
      try {
        termRef.current?.focus()
      } catch {
        /* ignore */
      }
    } else {
      try {
        rfbRef.current?.focus?.({ preventScroll: true })
      } catch {
        /* ignore */
      }
    }
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
        <div className="console-tabs">
          <button
            type="button"
            className={`console-tab${tab === 'serial' ? ' active' : ''}`}
            onClick={() => switchTab('serial')}
            onMouseDown={(e) => e.preventDefault()}
          >
            Serial
          </button>
          <button
            type="button"
            className={`console-tab${tab === 'display' ? ' active' : ''}`}
            onClick={() => switchTab('display')}
            onMouseDown={(e) => e.preventDefault()}
            disabled={!hasGraphics && !graphicsPathRef.current}
            title={hasGraphics || graphicsPathRef.current ? 'VGA / VNC' : 'Needs QEMU driver'}
          >
            Display
          </button>
        </div>
        <span className={`badge ${statusClass}`}>{statusLabel}</span>
        <span className="muted">
          {wsError
            ? wsError
            : tab === 'display'
              ? 'Click the screen, type username, Enter, then password. Do not press Enter on header buttons.'
              : connected && !gotOutput && vm?.state === 'running'
                ? 'Connected; waiting for guest serial.'
                : vm?.state === 'running'
                  ? 'Serial login: type user, Enter (password stays hidden).'
                  : 'Guest is not running; serial shows the last log.'}
        </span>
        <span className="pve-header-spacer" />
        {tab === 'serial' && (
          <Btn icon="trash" variant="secondary" onClick={() => termRef.current?.clear()}>
            Clear
          </Btn>
        )}
      </div>

      <div className="console-pane-stack" onMouseDown={focusConsole}>
        <div
          ref={screenRef}
          className={`console-pane pve-console console-vnc${tab === 'display' ? '' : ' console-pane-hidden'}`}
          aria-hidden={tab !== 'display'}
        />
        <div
          ref={termHostRef}
          className={`console-pane pve-console console-xterm${tab === 'serial' ? '' : ' console-pane-hidden'}`}
          aria-hidden={tab !== 'serial'}
        />
        {showConnecting && (
          <div className="console-loading" aria-live="polite">
            <Icon name="refresh" size={18} />
            Connecting to {vm?.spec?.name || vmId}…
          </div>
        )}
      </div>
    </div>
  )
}
