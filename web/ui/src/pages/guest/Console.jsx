import { useEffect, useRef, useState } from 'react'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import RFB from '@novnc/novnc'
import '@xterm/xterm/css/xterm.css'
import { getToken } from '../../api'
import { Btn, Icon } from '../../components/Icons'
import { useGuest } from '../GuestView'

export default function GuestConsole() {
  const { vm, vmId } = useGuest()
  const [connected, setConnected] = useState(false)
  const [connecting, setConnecting] = useState(true)
  const [tab, setTab] = useState('serial')
  const hasGraphics = Boolean(vm?.graphics_socket)
  const termRef = useRef(null)
  const termHostRef = useRef(null)
  const fitRef = useRef(null)
  const wsRef = useRef(null)
  const screenRef = useRef(null)
  const rfbRef = useRef(null)

  useEffect(() => {
    setConnected(false)
    setConnecting(true)
    if (hasGraphics && vm?.spec?.console_type === 'graphics') {
      setTab('display')
    } else {
      setTab('serial')
    }
  }, [vmId, hasGraphics, vm?.spec?.console_type])

  // Serial: xterm.js over websocket
  useEffect(() => {
    if (tab !== 'serial') return

    let cancelled = false
    let term
    let fit
    let socket
    let onResize

    const host = termHostRef.current
    if (!host) return

    term = new Terminal({
      cursorBlink: true,
      fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Consolas, monospace',
      fontSize: 13,
      theme: {
        background: '#0b0d12',
        foreground: '#c8c9de',
        cursor: '#c8c9de',
      },
      convertEol: true,
    })
    fit = new FitAddon()
    term.loadAddon(fit)
    term.open(host)
    fit.fit()
    termRef.current = term
    fitRef.current = fit

    const proto = location.protocol === 'https:' ? 'wss:' : 'ws:'
    socket = new WebSocket(
      `${proto}//${location.host}/v1/vms/${vmId}/console/ws?token=${encodeURIComponent(getToken())}`,
    )
    wsRef.current = socket
    socket.onopen = () => {
      if (!cancelled) {
        setConnected(true)
        setConnecting(false)
      }
    }
    socket.onclose = () => {
      if (!cancelled) {
        setConnected(false)
        setConnecting(false)
      }
    }
    socket.onerror = () => {
      if (!cancelled) setConnecting(false)
    }
    socket.onmessage = (e) =>
      term.write(typeof e.data === 'string' ? e.data : new TextDecoder().decode(e.data))
    term.onData((data) => {
      if (socket.readyState === 1) socket.send(data)
    })

    onResize = () => fit.fit()
    window.addEventListener('resize', onResize)

    return () => {
      cancelled = true
      window.removeEventListener('resize', onResize)
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

  // Display: noVNC over graphics websocket
  useEffect(() => {
    if (tab !== 'display' || !hasGraphics) return

    let cancelled = false
    const host = screenRef.current
    if (!host) return

    host.innerHTML = ''

    const proto = location.protocol === 'https:' ? 'wss:' : 'ws:'
    const url = `${proto}//${location.host}/v1/vms/${vmId}/graphics/ws?token=${encodeURIComponent(getToken())}`
    let rfb
    try {
      rfb = new RFB(host, url, { shared: true })
      rfb.scaleViewport = true
      rfb.resizeSession = false
      rfbRef.current = rfb
      rfb.addEventListener('connect', () => {
        if (!cancelled) {
          setConnected(true)
          setConnecting(false)
        }
      })
      rfb.addEventListener('disconnect', () => {
        if (!cancelled) {
          setConnected(false)
          setConnecting(false)
        }
      })
    } catch (err) {
      console.error('VNC init error', err)
      if (!cancelled) {
        setConnected(false)
        setConnecting(false)
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
  }, [vmId, tab, hasGraphics])

  function switchTab(next) {
    if (next === tab) return
    setConnected(false)
    setConnecting(true)
    setTab(next)
  }

  const statusLabel = connecting ? 'connecting' : connected ? 'connected' : 'disconnected'
  const statusClass = connecting ? 'pending' : connected ? 'ready' : 'unknown'

  return (
    <div className="pve-console-wrap">
      <div className="pve-console-bar">
        <div className="console-tabs">
          <button
            type="button"
            className={`console-tab${tab === 'serial' ? ' active' : ''}`}
            onClick={() => switchTab('serial')}
          >
            Serial
          </button>
          <button
            type="button"
            className={`console-tab${tab === 'display' ? ' active' : ''}`}
            onClick={() => switchTab('display')}
            disabled={!hasGraphics}
            title={hasGraphics ? 'VGA / VNC' : 'Needs QEMU driver (vmm.driver = qemu)'}
          >
            Display
          </button>
        </div>
        <span className={`badge ${statusClass}`}>{statusLabel}</span>
        <span className="muted">
          {tab === 'display'
            ? 'Graphics (VNC). Click the screen to focus keyboard and mouse.'
            : vm?.state === 'running'
              ? 'Serial console (xterm). Anaconda text UI works here.'
              : 'Guest is not running; serial shows the last log.'}
        </span>
        <span className="pve-header-spacer" />
        {tab === 'serial' && (
          <Btn icon="trash" variant="secondary" onClick={() => termRef.current?.clear()}>
            Clear
          </Btn>
        )}
      </div>

      <div className="console-pane-stack">
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
        {connecting && (
          <div className="console-loading" aria-live="polite">
            <Icon name="refresh" size={18} />
            Connecting to {vm?.spec?.name || vmId}…
          </div>
        )}
      </div>
      <p className="muted pve-console-hint">
        <Icon name={tab === 'display' ? 'tv' : 'terminal'} size={13} />
        {tab === 'display'
          ? 'Display over websocket (noVNC). Requires QEMU VMM.'
          : 'Serial over websocket (xterm.js).'}
      </p>
    </div>
  )
}
