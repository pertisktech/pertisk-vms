import { Navigate, useParams } from 'react-router-dom'
import { isTemplate } from '../../api'
import { useGuest } from '../GuestView'
import GuestConsole from './Console'

/** Remount console when switching guests so websockets and VNC reset cleanly. */
export default function GuestConsoleRoute() {
  const { vmId } = useParams()
  const { vm } = useGuest()
  if (isTemplate(vm)) {
    return <Navigate to={`/vm/${vmId}/summary`} replace />
  }
  return <GuestConsole key={vmId} />
}
