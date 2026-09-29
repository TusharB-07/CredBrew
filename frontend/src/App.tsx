import { Routes, Route, Navigate } from 'react-router-dom';
import { useAuth } from './hooks/useAuth';
import Layout from './components/Layout';
import Login from './pages/Login';
import Register from './pages/Register';
import EventList from './pages/EventList';
import EventCreate from './pages/EventCreate';
import EventDashboard from './pages/EventDashboard';
import Participants from './pages/Participants';
import Templates from './pages/Templates';
import Issue from './pages/Issue';

function PrivateRoute({ children }: { children: React.ReactNode }) {
  const { isAuthenticated } = useAuth();
  return isAuthenticated ? <>{children}</> : <Navigate to="/login" replace />;
}

function PublicRoute({ children }: { children: React.ReactNode }) {
  const { isAuthenticated } = useAuth();
  return !isAuthenticated ? <>{children}</> : <Navigate to="/events" replace />;
}

export default function App() {
  return (
    <Routes>
      <Route path="/login" element={<PublicRoute><Login /></PublicRoute>} />
      <Route path="/register" element={<PublicRoute><Register /></PublicRoute>} />
      <Route element={<PrivateRoute><Layout /></PrivateRoute>}>
        <Route path="/events" element={<EventList />} />
        <Route path="/events/new" element={<EventCreate />} />
        <Route path="/events/:id" element={<EventDashboard />} />
        <Route path="/events/:id/participants" element={<Participants />} />
        <Route path="/events/:id/templates" element={<Templates />} />
        <Route path="/events/:id/issue" element={<Issue />} />
      </Route>
      <Route path="*" element={<Navigate to="/events" replace />} />
    </Routes>
  );
}