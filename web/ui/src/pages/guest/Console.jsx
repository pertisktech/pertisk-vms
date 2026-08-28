import { useEffect, useRef, useState } from 'react'
import { getToken } from '../../api'
import { Btn, Icon } from '../../components/Icons'
import { useGuest } from '../GuestView'

export default function GuestConsole() {
  const { vm, vmId } = useGuest()
  const [text, setText] = useState('')
  const [connected, setConnected] = useState(false)
  const wsRef = useRef(null)
  const preRef = useRef(null)

  useEffect(() => {
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
  }, [vmId])

  useEffect(() => {
    if (preRef.current) preRef.current.scrollTop = preRef.current.scrollHeight
  }, [text])

  function sendKey(e) {
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
          {vm.state === 'running'
            ? 'Click the pane and type. Enter, Tab, and Backspace are forwarded.'
            : 'Guest is not running; output is the last serial log.'}
        </span>
        <span className="pve-header-spacer" />
        <Btn icon="trash" variant="secondary" onClick={() => setText('')}>
          Clear
        </Btn>
      </div>
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
    </div>
  )
}
