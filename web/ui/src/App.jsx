import { HashRouter, Navigate, Route, Routes } from 'react-router-dom'
import { ConfirmProvider } from './components/Confirm'
import Layout from './Layout'
import Login from './pages/Login'
import Overview from './pages/Overview'
import Guests from './pages/Guests'
import Storage from './pages/Storage'
import Networks from './pages/Networks'
import Cluster from './pages/Cluster'
import Activity from './pages/Activity'
import Users from './pages/Users'

export default function App() {
  return (
    <ConfirmProvider>
      <HashRouter>
        <Routes>
          <Route path="/login" element={<Login />} />
          <Route element={<Layout />}>
            <Route path="/" element={<Overview />} />
            <Route path="/guests" element={<Guests />} />
            <Route path="/storage" element={<Storage />} />
            <Route path="/networks" element={<Networks />} />
            <Route path="/cluster" element={<Cluster />} />
            <Route path="/activity" element={<Activity />} />
            <Route path="/users" element={<Users />} />
          </Route>
          <Route path="*" element={<Navigate to="/" replace />} />
        </Routes>
      </HashRouter>
    </ConfirmProvider>
  )
}
