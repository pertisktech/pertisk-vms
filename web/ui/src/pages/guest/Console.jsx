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
  const [tab, setTab] = useState('serial')
  const hasGraphics = Boolean(vm?.graphics_socket)
  const termRef = useRef(null)
  const termHostRef = useRef(null)
  const fitRef = useRef(null)
  const wsRef = useRef(null)
  const screenRef = useRef(null)
  const rfbRef = useRef(null)

  useEffect(() => {
    if (hasGraphics && vm?.spec?.console_type === 'graphics') {
      setTab('display')
    } else {
      setTab('serial')
    }
  }, [vmId, hasGraphics, vm?.spec?.console_type])

  // Serial: xterm.js over websocket
  useEffect(() => {
    if (tab !== 'serial' || !termHostRef.current) return

    const term = new Terminal({
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
    const fit = new FitAddon()
    term.loadAddon(fit)
    term.open(termHostRef.current)
    fit.fit()
    termRef.current = term
    fitRef.current = fit

    const proto = location.protocol === 'https:' ? 'wss:' : 'ws:'
    const socket = new WebSocket(
      `${proto}//${location.host}/v1/vms/${vmId}/console/ws?token=${encodeURIComponent(getToken())}`,
    )
    wsRef.current = socket
    socket.onopen = () => setConnected(true)
    socket.onclose = () => setConnected(false)
    socket.onmessage = (e) => term.write(typeof e.data === 'string' ? e.data : new TextDecoder().decode(e.data))
    term.onData((data) => {
      if (socket.readyState === 1) socket.send(data)
    })

    const onResize = () => fit.fit()
    window.addEventListener('resize', onResize)

    return () => {
      window.removeEventListener('resize', onResize)
      socket.onclose = null
      socket.close()
      wsRef.current = null
      term.dispose()
      termRef.current = null
      fitRef.current = null
      setConnected(false)
    }
  }, [vmId, tab])

  // Display: noVNC over graphics websocket
  useEffect(() => {
    if (tab !== 'display' || !hasGraphics || !screenRef.current) return

    const proto = location.protocol === 'https:' ? 'wss:' : 'ws:'
    const url = `${proto}//${location.host}/v1/vms/${vmId}/graphics/ws?token=${encodeURIComponent(getToken())}`
    let rfb
    try {
      rfb = new RFB(screenRef.current, url, { shared: true })
      rfb.scaleViewport = true
      rfb.resizeSession = false
      rfbRef.current = rfb
      rfb.addEventListener('connect', () => setConnected(true))
      rfb.addEventListener('disconnect', () => setConnected(false))
    } catch (err) {
      console.error('VNC init error', err)
      setConnected(false)
    }

    return () => {
      if (rfbRef.current) {
        try {
          rfbRef.current.disconnect()
        } catch {
          /* ignore */
        }
        rfbRef.current = null
      }
      setConnected(false)
    }
  }, [vmId, tab, hasGraphics])

  return (
    <div className="pve-console-wrap">
      <div className="pve-console-bar">
        <div className="console-tabs">
          <button
            type="button"
            className={`console-tab${tab === 'serial' ? ' active' : ''}`}
            onClick={() => setTab('serial')}
          >
            Serial
          </button>
          <button
            type="button"
            className={`console-tab${tab === 'display' ? ' active' : ''}`}
            onClick={() => setTab('display')}
            disabled={!hasGraphics}
            title={hasGraphics ? 'VGA / VNC' : 'Needs QEMU driver (vmm.driver = qemu)'}
          >
            Display
          </button>
        </div>
        <span className={`badge ${connected ? 'ready' : 'unknown'}`}>
          {connected ? 'connected' : 'disconnected'}
        </span>
        <span className="muted">
          {tab === 'display'
            ? 'Graphics (VNC). Click the screen to focus keyboard and mouse.'
            : vm.state === 'running'
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

      {tab === 'display' ? (
        <>
          <div ref={screenRef} className="console-pane pve-console console-vnc" />
          <p className="muted pve-console-hint">
            <Icon name="tv" size={13} /> Display over websocket (noVNC). Requires QEMU VMM.
          </p>
        </>
      ) : (
        <>
          <div ref={termHostRef} className="console-pane pve-console console-xterm" />
          <p className="muted pve-console-hint">
            <Icon name="terminal" size={13} /> Serial over websocket (xterm.js).
          </p>
        </>
      )}
    </div>
  )
}
