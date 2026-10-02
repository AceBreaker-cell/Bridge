import React, { useState, useEffect } from 'react';
import { useTauri } from '../../hooks/useTauri';

interface ActiveTransfer {
  id: string;
  fileName: string;
  size: string;
  toDevice: string;
  progress: number;
  speed: string;
  eta: string;
  paused: boolean;
}

interface CompletedTransfer {
  id: string;
  fileName: string;
  size: string;
  toOrFromDevice: string;
  direction: 'upload' | 'download';
  timestamp: string;
  status: string;
}

const Transfers: React.FC = () => {
  const [activeTransfers, setActiveTransfers] = useState<ActiveTransfer[]>([]);
  const [completedTransfers, setCompletedTransfers] = useState<CompletedTransfer[]>([]);
  const [loading, setLoading] = useState<boolean>(true);
  const { invoke } = useTauri();

  useEffect(() => {
    const loadTransfers = async () => {
      setLoading(true);
      try {
        // In a real implementation, we would invoke Tauri commands to get transfer data
        // For now, we'll keep the mock data but note that this should be replaced
        // with actual Tauri invocations when the backend transfer system is fully implemented
        setActiveTransfers([
          {
            id: 'txn-1',
            fileName: 'project.zip',
            size: '2.4 GB',
            toDevice: 'Study-Laptop',
            progress: 76,
            speed: '48.2 MB/s',
            eta: '~12 seconds remaining',
            paused: false
          }
        ]);

        setCompletedTransfers([
          {
            id: 'txn-2',
            fileName: 'notes.pdf',
            size: '4.2 MB',
            toOrFromDevice: 'Frangky-PC',
            direction: 'download',
            timestamp: 'Today, 9:15 AM',
            status: 'Completed'
          }
        ]);
      } catch (error) {
        console.error('Failed to load transfers:', error);
      } finally {
        setLoading(false);
      }
    };

    loadTransfers();
  }, [invoke]);

  return (
    <div className="transfers-page">
      <h2>Transfers</h2>

      <div className="transfer-filters">
        <button className="btn-active">Active</button>
        <button className="btn-inactive">Completed</button>
      </div>

      {loading ? (
        <div className="empty-state">
          <h3>Loading transfers...</h3>
          <p>Please wait while we load your transfer data.</p>
        </div>
      ) : (
        <>
          {activeTransfers.length > 0 ? (
            <div className="transfer-cards">
              {activeTransfers.map((transfer) => (
                <div key={transfer.id} className="transfer-card">
                  <div className="transfer-header">
                    <h3>{transfer.fileName}</h3>
                    <p className="transfer-size">{transfer.size}</p>
                  </div>
                  <div className="transfer-info">
                    <p>To: {transfer.toDevice}</p>
                    <div className="transfer-progress">
                      <div className="progress-bar">
                        <div className="progress-fill" style={{ width: transfer.progress + '%' }}></div>
                      </div>
                    </div>
                    <p className="progress-text">{transfer.progress}% • {transfer.speed} • {transfer.eta}</p>
                  </div>
                  <div className="transfer-actions">
                    <button className="btn-sm btn-secondary">{transfer.paused ? 'Resume' : 'Pause'}</button>
                    <button className="btn-sm btn-danger">Cancel</button>
                  </div>
                </div>
              ))}
            </div>
          ) : (
            <div className="empty-state">
              <h3>No active transfers</h3>
              <p>Transfers you initiate or receive will appear here.</p>
            </div>
          )}

          {completedTransfers.length > 0 && (
            <div className="transfer-history">
              <h3>Recent Transfers</h3>
              <div className="history-list">
                {completedTransfers.map((transfer) => (
                  <div key={transfer.id} className="history-item">
                    <div className="history-info">
                      <h4>{transfer.fileName}</h4>
                      <p>
                        {transfer.direction === 'upload' ? '↑' : '↓'}
                        {transfer.toOrFromDevice}
                      </p>
                      <p className="history-size">{transfer.size}</p>
                      <p className="history-time">{transfer.timestamp}</p>
                      <span className={`status-${transfer.status.toLowerCase()}`}>
                        {transfer.status}
                      </span>
                    </div>
                  </div>
                ))}
              </div>
            </div>
          )}
        </>
      )}
    </div>
  );
};

export default Transfers;
