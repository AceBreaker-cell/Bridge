import { BrowserRouter as Router, Routes, Route, NavLink } from 'react-router-dom';
import { useTheme } from './hooks/useTheme';
import './App.css';

import Devices from './features/devices/Devices';
import Transfers from './features/transfers/Transfers';
import History from './features/history/History';
import Settings from './features/settings/Settings';
import Diagnostics from './features/diagnostics/Diagnostics';

function App() {
  const { theme, toggleTheme } = useTheme();
  
  return (
    <Router>
      <div className={`app ${theme}`}>
        <header className="app-header">
          <h1>Bridge</h1>
          <div className="header-actions">
            <button onClick={toggleTheme} className="theme-toggle">
              {theme === 'dark' ? '☀️' : '🌙'}
            </button>
          </div>
        </header>
        
        <nav className="app-nav">
          <NavLink to="/devices" end className="nav-link">Devices</NavLink>
          <NavLink to="/transfers" end className="nav-link">Transfers</NavLink>
          <NavLink to="/history" end className="nav-link">History</NavLink>
          <NavLink to="/settings" end className="nav-link">Settings</NavLink>
          <NavLink to="/diagnostics" end className="nav-link">Diagnostics</NavLink>
        </nav>
        
        <main className="app-main">
          <Routes>
            <Route path="/devices" element={<Devices />} />
            <Route path="/transfers" element={<Transfers />} />
            <Route path="/history" element={<History />} />
            <Route path="/settings" element={<Settings />} />
            <Route path="/diagnostics" element={<Diagnostics />} />
            <Route path="/" element={<Devices />} />
            <Route path="*" element={<div>404 - Page Not Found</div> } />
          </Routes>
        </main>
      </div>
    </Router>
  );
}

export default App;
