import { HashRouter, Navigate, Route, Routes } from 'react-router-dom'
import { ConfirmProvider } from './components/Confirm'
import Layout from './Layout'
import Login from './pages/Login'
import Datacenter from './pages/Datacenter'
import NodeView from './pages/NodeView'
import GuestView from './pages/GuestView'
import Overview from './pages/Overview'
import Storage from './pages/Storage'
import Networks from './pages/Networks'
import Cluster from './pages/Cluster'
import Activity from './pages/Activity'
import Users from './pages/Users'
import NodeSummary from './pages/node/Summary'
import NodeGuests from './pages/node/Guests'
import GuestSummary from './pages/guest/Summary'
import GuestConsoleRoute from './pages/guest/ConsoleRoute'
import GuestHardware from './pages/guest/Hardware'
import GuestOptions from './pages/guest/Options'

export default function App() {
  return (
    <ConfirmProvider>
      <HashRouter>
        <Routes>
          <Route path="/login" element={<Login />} />
          <Route element={<Layout />}>
            <Route path="/dc" element={<Datacenter />}>
              <Route index element={<Navigate to="summary" replace />} />
              <Route path="summary" element={<Overview />} />
              <Route path="storage" element={<Storage />} />
              <Route path="networks" element={<Networks />} />
              <Route path="cluster" element={<Cluster />} />
              <Route path="tasks" element={<Activity />} />
              <Route path="users" element={<Users />} />
            </Route>
            <Route path="/node/:nodeId" element={<NodeView />}>
              <Route index element={<Navigate to="summary" replace />} />
              <Route path="summary" element={<NodeSummary />} />
              <Route path="guests" element={<NodeGuests />} />
              <Route path="tasks" element={<Activity />} />
            </Route>
            <Route path="/vm/:vmId" element={<GuestView />}>
              <Route index element={<Navigate to="summary" replace />} />
              <Route path="summary" element={<GuestSummary />} />
              <Route path="console" element={<GuestConsoleRoute />} />
              <Route path="hardware" element={<GuestHardware />} />
              <Route path="options" element={<GuestOptions />} />
            </Route>
          </Route>
          <Route path="*" element={<Navigate to="/dc/summary" replace />} />
        </Routes>
      </HashRouter>
    </ConfirmProvider>
  )
}
