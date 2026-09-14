import { Navigate, useParams } from 'react-router-dom'
import { isTemplate } from '../../api'
import { useGuest } from '../GuestView'
import GuestSsh from './Ssh'

export default function GuestSshRoute() {
  const { vmId } = useParams()
  const { vm } = useGuest()
  if (isTemplate(vm)) {
    return <Navigate to={`/vm/${vmId}/summary`} replace />
  }
  return <GuestSsh key={vmId} />
}
