import { useEffect, useRef, useState } from 'react'
import { getToken } from '../../api'
import { Btn, Icon } from '../../components/Icons'
import { useGuest } from '../GuestView'

export default function GuestConsole() {
  const { vm, vmId } = useGuest()
  const [text, setText] = useState('')
  const [connected, setConnected] = useState(false)
  const [consoleType, setConsoleType] = useState('serial')
  const wsRef = useRef(null)
  const preRef = useRef(null)
  const canvasRef = useRef(null)
  const vncRef = useRef(null)

  // Detect console type from API
  useEffect(() => {
    const type = vm?.spec?.console_type || 'serial'
    setConsoleType(type)
  }, [vm])

  // Serial console WebSocket
  useEffect(() => {
    if (consoleType !== 'serial') return

    const proto = location.protocol === 'https:' ? 'wss:' : 'ws:'
    const socket = new WebSocket(
      `${proto}//${location.host}/v1/vms/${vmId}/console/ws?token=${encodeURIComponent(getToken())}`,
    )
    wsRef.current = socket
    socket.onopen = () => setConnected(true)
    socket.onclose = () => setConnected(false)
    socket.onmessage = (e) => setText((t) => t + e.data)
    return () => {
      socket.onclose = null
      socket.close()
      wsRef.current = null
    }
  }, [vmId, consoleType])

  // Graphics console VNC
  useEffect(() => {
    if (consoleType !== 'graphics') return

    // Dynamically load noVNC if not already loaded
    if (typeof RFB === 'undefined') {
      const script = document.createElement('script')
      script.src = 'https://cdn.jsdelivr.net/npm/novnc/core/rfb.js'
      script.onload = initVNC
      document.head.appendChild(script)
    } else {
      initVNC()
    }

    function initVNC() {
      try {
        const proto = location.protocol === 'https:' ? 'wss:' : 'ws:'
        const rfb = new RFB(canvasRef.current, 
          `${proto}//${location.host}/v1/vms/${vmId}/graphics/ws?token=${encodeURIComponent(getToken())}`,
          { shared: true }
        )
        vncRef.current = rfb
        rfb.addEventListener('connect', () => setConnected(true))
        rfb.addEventListener('disconnect', () => setConnected(false))
        rfb.addEventListener('securityfailure', (e) => console.error('VNC security error', e))
        setConnected(true)
      } catch (e) {
        console.error('VNC init error', e)
      }
    }

    return () => {
      if (vncRef.current) {
        try {
          vncRef.current.disconnect()
        } catch (e) {
          console.error('VNC disconnect error', e)
        }
        vncRef.current = null
      }
    }
  }, [vmId, consoleType])

  // Scroll serial console
  useEffect(() => {
    if (preRef.current && consoleType === 'serial') {
      preRef.current.scrollTop = preRef.current.scrollHeight
    }
  }, [text, consoleType])

  function sendKey(e) {
    if (consoleType !== 'serial') return

    const ws = wsRef.current
    if (!ws || ws.readyState !== 1) return
    if (e.key === 'Enter') {
      ws.send('\n')
      e.preventDefault()
    } else if (e.key === 'Backspace') {
      ws.send('\x7f')
      e.preventDefault()
    } else if (e.key === 'Tab') {
      ws.send('\t')
      e.preventDefault()
    } else if (e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey) {
      ws.send(e.key)
      e.preventDefault()
    }
  }

  return (
    <div className="pve-console-wrap">
      <div className="pve-console-bar">
        <span className={`badge ${connected ? 'ready' : 'unknown'}`}>
          {connected ? 'connected' : 'disconnected'}
        </span>
        <span className="muted">
          {consoleType === 'graphics' 
            ? 'Graphics console. Use VNC client or keyboard/mouse in the canvas below.'
            : vm.state === 'running'
            ? 'Click the pane and type. Enter, Tab, and Backspace are forwarded.'
            : 'Guest is not running; output is the last serial log.'}
        </span>
        <span className="pve-header-spacer" />
        {consoleType === 'serial' && (
          <Btn icon="trash" variant="secondary" onClick={() => setText('')}>
            Clear
          </Btn>
        )}
      </div>

      {consoleType === 'graphics' ? (
        <>
          <canvas
            ref={canvasRef}
            className="console-pane pve-console"
            style={{ width: '100%', height: '600px', cursor: 'none' }}
          />
          <p className="muted pve-console-hint">
            <Icon name="tv" size={13} /> Graphics (VNC) console over websocket. Requires noVNC support.
          </p>
        </>
      ) : (
        <>
          <pre
            ref={preRef}
            className="console-pane pve-console"
            tabIndex={0}
            onKeyDown={sendKey}
            onClick={() => preRef.current?.focus()}
          >
            {text || 'Waiting for serial output…'}
          </pre>
          <p className="muted pve-console-hint">
            <Icon name="terminal" size={13} /> Serial console over websocket.
          </p>
        </>
      )}
    </div>
  )
}
