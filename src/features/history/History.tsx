import { useState, useEffect } from 'react';
import { useTauri } from '../../hooks/useTauri';

const History: React.FC = () => {
  const [history, setHistory] = useState<Array<{
    id: string;
    fileName: string;
    size: string;
    toOrFromDevice: string;
    direction: 'upload' | 'download';
    timestamp: string;
    status: string;
  }>>([]);

  const [searchTerm, setSearchTerm] = useState('');
  const [filteredHistory, setFilteredHistory] = useState<Array<typeof history[0]>>([]);
  const [loading, setLoading] = useState<boolean>(true);
  const { invoke } = useTauri();

  useEffect(() => {
    const loadHistory = async () => {
      setLoading(true);
      try {
        // Get history from database via Tauri command
        const result = await invoke<string>('get_transfer_history');
        const parsed = JSON.parse(result);

        // Transform the data to match our expected format
        const transformed = parsed.map((item: any) => ({
          id: item.id.toString(),
          fileName: item.fileName,
          size: `${(item.total_size / (1024 * 1024)).toFixed(2)} GB`,
          toOrFromDevice: item.device_name,
          direction: item.direction === 'sent' ? 'upload' as const : 'download' as const,
          timestamp: new Date(item.timestamp).toLocaleString(),
          status: item.status
        }));

        setHistory(transformed);
        setFilteredHistory(transformed);
      } catch (error) {
        console.error('Failed to load history:', error);
        // Fallback to empty array on error
        setHistory([]);
        setFilteredHistory([]);
      } finally {
        setLoading(false);
      }
    };

    loadHistory();
  }, [invoke]);

  useEffect(() => {
    if (searchTerm.trim() === '') {
      setFilteredHistory(history);
    } else {
      const filtered = history.filter(item =>
        item.fileName.toLowerCase().includes(searchTerm.toLowerCase()) ||
        item.toOrFromDevice.toLowerCase().includes(searchTerm.toLowerCase())
      );
      setFilteredHistory(filtered);
    }
  }, [searchTerm, history]);

  const handleClearHistory = () => {
    if (window.confirm('Are you sure you want to clear all transfer history? This cannot be undone.')) {
      // Invoke Tauri command to clear history
      invoke<void>('clear_transfer_history')
        .then(() => {
          setHistory([]);
          setFilteredHistory([]);
          alert('Transfer history has been cleared.');
        })
        .catch((error) => {
          console.error('Failed to clear history:', error);
          alert('Failed to clear transfer history.');
        });
    }
  };

  return (
    <div className="history-page">
      <h2>Transfer History</h2>

      <div className="history-controls">
        <input
          type="text"
          placeholder="Search history..."
          value={searchTerm}
          onChange={(e) => setSearchTerm(e.target.value)}
          className="history-search"
        />
        <button className="btn-sm btn-secondary" onClick={handleClearHistory}>
          Clear History
        </button>
      </div>

      {loading ? (
        <div className="empty-state">
          <h3>Loading history...</h3>
          <p>Please wait while we load your transfer history.</p>
        </div>
      ) : (
        filteredHistory.length === 0 ? (
          <div className="empty-state">
            <h3>No transfers yet</h3>
            <p>Files you send or receive will appear here.</p>
            {searchTerm.trim() !== '' && (
              <p className="search-hint">Try adjusting your search criteria.</p>
            )}
          </div>
        ) : (
          <div className="history-list">
            {filteredHistory.map(item => (
              <div key={item.id} className="history-item">
                <div className="history-content">
                  <div className="history-icon">
                    {item.direction === 'upload' ? '↑' : '↓'}
                  </div>
                  <div className="history-details">
                    <h4>{item.fileName}</h4>
                    <p>
                      {item.direction === 'upload' ? '→' : '←'}
                      {item.toOrFromDevice}
                    </p>
                    <p className="history-meta">
                      <span className="history-size">{item.size}</span>
                      <span className="history-date">{item.timestamp}</span>
                    </p>
                  </div>
                  <div className="history-status">
                    <span className={`status-${item.status.toLowerCase()}`}>
                      {item.status}
                    </span>
                  </div>
                </div>
              </div>
            ))}
          </div>
        )
      )}
    </div>
  );
};

export default History;