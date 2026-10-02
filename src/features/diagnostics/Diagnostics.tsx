import './Diagnostics.css';
import { useState, useEffect } from 'react';
import { useTauri } from '../../hooks/useTauri';

const Diagnostics: React.FC = () => {
  const [diagnostics, setDiagnostics] = useState({
    version: '0.1.0',
    platform: '',
    distribution: '',
    cpuUsage: '0%',
    memoryUsage: '0 MB / 0 GB',
    networkInterface: '',
    localAddress: '',
    discoveryStatus: 'Stopped',
    listeningPort: 42137,
    connectedDevices: 0,
    activeTransfers: 0,
    uptime: '0h 0m'
  });

  const [refreshing, setRefreshing] = useState(false);
  const [lastUpdated, setLastUpdated] = useState<string | null>(null);
  const { invoke } = useTauri();

  useEffect(() => {
    const loadDiagnostics = async () => {
      setRefreshing(true);
      try {
        const result = await invoke<string>('get_diagnostics');
        const parsed = JSON.parse(result);
        setDiagnostics(parsed);
        setLastUpdated(new Date().toLocaleTimeString());
      } catch (error) {
        console.error('Failed to load diagnostics:', error);
      } finally {
        setRefreshing(false);
      }
    };

    loadDiagnostics();
  }, [invoke]);

  const refreshDiagnostics = async () => {
    setRefreshing(true);
    try {
      const result = await invoke<string>('get_diagnostics');
      const parsed = JSON.parse(result);
      setDiagnostics(parsed);
      setLastUpdated(new Date().toLocaleTimeString());
    } catch (error) {
      console.error('Failed to refresh diagnostics:', error);
    } finally {
      setRefreshing(false);
    }
  };

  const copyDiagnostics = () => {
    const text = Object.entries(diagnostics)
      .map(([key, value]) => `${key}: ${value}`)
      .join('\n');
    navigator.clipboard.writeText(text).then(() => {
      alert('Diagnostics copied to clipboard!');
    });
  };

  return (
    <div className="diagnostics-page">
      <h2>Bridge Diagnostics</h2>
      
      <div className="diagnostics-header">
        <p className="diagnostics-updated">
          Last updated: {lastUpdated || 'Never'}
          {!refreshing && (
            <button 
              className="btn-sm btn-secondary" 
              onClick={refreshDiagnostics}
            >
              Refresh
            </button>
          )}
          {refreshing && <span className="refreshing">Refreshing...</span>}
        </p>
        <button className="btn-primary" onClick={copyDiagnostics}>
          [ Copy Diagnostics ]
        </button>
      </div>
      
      <div className="diagnostics-grid">
        <div className="diagnostics-card">
          <h3>System Information</h3>
          <div className="diagnostics-item">
            <span className="label">Version:</span>
            <span className="value">{diagnostics.version}</span>
          </div>
          <div className="diagnostics-item">
            <span className="label">Platform:</span>
            <span className="value">{diagnostics.platform}</span>
          </div>
          <div className="diagnostics-item">
            <span className="label">Distribution:</span>
            <span className="value">{diagnostics.distribution}</span>
          </div>
          <div className="diagnostics-item">
            <span className="label">CPU Usage:</span>
            <span className="value">{diagnostics.cpuUsage}</span>
          </div>
          <div className="diagnostics-item">
            <span className="label">Memory Usage:</span>
            <span className="value">{diagnostics.memoryUsage}</span>
          </div>
          <div className="diagnostics-item">
            <span className="label">Uptime:</span>
            <span className="value">{diagnostics.uptime}</span>
          </div>
        </div>
        
        <div className="diagnostics-card">
          <h3>Network Information</h3>
          <div className="diagnostics-item">
            <span className="label">Network Interface:</span>
            <span className="value">{diagnostics.networkInterface}</span>
          </div>
          <div className="diagnostics-item">
            <span className="label">Local Address:</span>
            <span className="value">{diagnostics.localAddress}</span>
          </div>
          <div className="diagnostics-item">
            <span className="label">Discovery Status:</span>
            <span className={`status-${diagnostics.discoveryStatus.toLowerCase()}`}>
              {diagnostics.discoveryStatus}
            </span>
          </div>
          <div className="diagnostics-item">
            <span className="label">Listening Port:</span>
            <span className="value">{diagnostics.listeningPort}</span>
          </div>
          <div className="diagnostics-item">
            <span className="label">Connected Devices:</span>
            <span className="value">{diagnostics.connectedDevices}</span>
          </div>
          <div className="diagnostics-item">
            <span className="label">Active Transfers:</span>
            <span className="value">{diagnostics.activeTransfers}</span>
          </div>
        </div>
      </div>
      
      <div className="diagnostics-footer">
        <p className="diagnostics-note">
          This diagnostic information is gathered locally from your system.
          No data is sent to external servers.
        </p>
      </div>
    </div>
  );
};

export default Diagnostics;
