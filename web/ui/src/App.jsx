import { HashRouter, Navigate, Route, Routes } from 'react-router-dom'
import { ConfirmProvider } from './components/Confirm'
import { ThemeProvider } from './ThemeContext'
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
import NodeUpdates from './pages/node/Updates'
import NodeRepositories from './pages/node/Repositories'
import NodeShell from './pages/node/Shell'
import GuestSummary from './pages/guest/Summary'
import GuestConsoleRoute from './pages/guest/ConsoleRoute'
import GuestHardware from './pages/guest/Hardware'
import GuestOptions from './pages/guest/Options'
import Templates from './pages/Templates'

export default function App() {
  return (
    <ThemeProvider>
      <ConfirmProvider>
        <HashRouter>
          <Routes>
            <Route path="/login" element={<Login />} />
            <Route element={<Layout />}>
              <Route path="/dc" element={<Datacenter />}>
                <Route index element={<Navigate to="summary" replace />} />
                <Route path="summary" element={<Overview />} />
                <Route path="storage" element={<Storage />} />
                <Route path="templates" element={<Templates />} />
                <Route path="networks" element={<Networks />} />
                <Route path="cluster" element={<Cluster />} />
                <Route path="tasks" element={<Activity />} />
                <Route path="users" element={<Users />} />
              </Route>
              <Route path="/node/:nodeId" element={<NodeView />}>
                <Route index element={<Navigate to="summary" replace />} />
                <Route path="summary" element={<NodeSummary />} />
                <Route path="guests" element={<NodeGuests />} />
                <Route path="updates" element={<NodeUpdates />} />
                <Route path="repositories" element={<NodeRepositories />} />
                <Route path="shell" element={<NodeShell />} />
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
    </ThemeProvider>
  )
}
