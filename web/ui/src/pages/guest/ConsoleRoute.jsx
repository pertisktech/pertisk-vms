import { useParams } from 'react-router-dom'
import GuestConsole from './Console'

/** Remount console when switching guests so websockets and VNC reset cleanly. */
export default function GuestConsoleRoute() {
  const { vmId } = useParams()
  return <GuestConsole key={vmId} />
}
