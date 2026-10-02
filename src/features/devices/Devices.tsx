import React, { useState, useEffect } from 'react';
import { useTauri } from '../../hooks/useTauri';

interface DiscoveredDevice {
  id: string;
  name: string;
  platform: string;
  status: string;
  lastSeen: number;
}

const Devices: React.FC = () => {
  const [devices, setDevices] = useState<DiscoveredDevice[]>([]);
  const [searching, setSearching] = useState(false);
  const [selectedDeviceId, setSelectedDeviceId] = useState<string | null>(null);
  const { invoke } = useTauri();

  useEffect(() => {
    const startDiscovery = async () => {
      setSearching(true);
      try {
        // Start discovery via Tauri command
        await invoke('start_discovery');
        
        // Poll for discovered devices
        const pollDevices = async () => {
          try {
            const discovered = await invoke<Array<{id: string; name: string; platform: string}>>('get_discovered_devices');
            setDevices(discovered.map(device => ({
              ...device,
              status: 'Available', // Default status
              lastSeen: Date.now()
            })));
            
            if (searching) {
              setTimeout(pollDevices, 3000);
            }
          } catch (error) {
            console.error('Error polling devices:', error);
            if (searching) {
              setTimeout(pollDevices, 5000); // Retry longer on error
            }
          }
        };
        
        pollDevices();
      } catch (error) {
        console.error('Discovery error:', error);
        setSearching(false);
      }
    };

    startDiscovery();

    return () => {
      // Cleanup
      setSearching(false);
      invoke('stop_discovery');
    };
  }, [invoke]);

  const handlePair = async (deviceId: string) => {
    try {
      setSelectedDeviceId(deviceId);
      await invoke('pair_device', { deviceId });
      // Update device status
      setDevices(prev => prev.map(device => 
        device.id === deviceId ? { ...device, status: 'Trusted' } : device
      ));
      setSelectedDeviceId(null);
    } catch (error) {
      console.error('Pairing error:', error);
      setSelectedDeviceId(null);
    }
  };

  const handleSend = async (deviceId: string) => {
    // This would open file picker and initiate transfer
    console.log('Sending to device:', deviceId);
    // In a real implementation, this would show a file picker
    alert(`File transfer to ${devices.find(d => d.id === deviceId)?.name || 'device'} initiated.`);
  };

  const handleRefresh = async () => {
    setSearching(true);
    try {
      // Restart discovery
      await invoke('stop_discovery');
      await invoke('start_discovery');
    } catch (error) {
      console.error('Error refreshing discovery:', error);
    }
    
    // Resume polling in 2 seconds
    setTimeout(() => {
      setSearching(false);
    }, 2000);
  };

  if (searching && devices.length === 0) {
    return (
      <div className="devices-page">
        <h2>Nearby Devices</h2>
        <div className="searching">
          <div className="spinner"></div>
          <p>Searching for nearby devices...</p>
        </div>
      </div>
    );
  }

  return (
    <div className="devices-page">
      <h2>Nearby Devices</h2>
      
      <div className="devices-toolbar">
        <button 
          className="btn-sm btn-secondary" 
          onClick={handleRefresh}
          disabled={searching}
        >
          {searching ? 'Searching...' : 'Refresh'}
        </button>
      </div>
      
      {devices.length === 0 ? (
        <div className="empty-state">
          <h3>No Bridge devices found</h3>
          <p>Make sure another computer is running Bridge on the same local network.</p>
          <div className="empty-actions">
            <button 
              className="btn-secondary" 
              onClick={handleRefresh}
              disabled={searching}
            >
              {searching ? 'Searching...' : 'Search Again'}
            </button>
            <button className="btn-primary">Add Manually</button>
          </div>
        </div>
      ) : (
        <div className="devices-list">
          {devices.map(device => (
            <div 
              key={device.id} 
              className={`device-card${selectedDeviceId === device.id ? ' selected' : ''}`}
            >
              <div className="device-info">
                <div className="device-header">
                  <span className="device-icon">💻</span>
                  <div>
                    <h3>{device.name}</h3>
                    <p className="device-platform">{device.platform}</p>
                  </div>
                </div>
                <div className="device-status">
                  <span className={`status-${device.status.toLowerCase()}`}>
                    {device.status === 'Trusted' ? '✓ Trusted' : 
                     device.status === 'Available' ? '● Available' : 
                     device.status === 'Pairing' ? '○ Pairing' : 
                     '● Discovered'}
                  </span>
                  <small className="last-seen">
                    Last seen: {new Date(device.lastSeen).toLocaleTimeString()}
                  </small>
                </div>
              </div>
              <div className="device-actions">
                {device.status === 'Available' && (
                  <button 
                    className="btn-primary" 
                    onClick={() => handlePair(device.id)}
                    disabled={selectedDeviceId !== null}
                  >
                    {selectedDeviceId === device.id ? 'Pairing...' : 'Pair'}
                  </button>
                )}
                {(device.status === 'Trusted' || device.status === 'Available') && (
                  <button 
                    className="btn-secondary" 
                    onClick={() => handleSend(device.id)}
                    disabled={selectedDeviceId !== null}
                  >
                    Send
                  </button>
                )}
              </div>
            </div>
          ))}
        </div>
      )}
      
      <div className="drop-zone" 
        onDragOver={(e) => e.preventDefault()} 
        onDrop={async (e) => {
          e.preventDefault();
          const files = Array.from(e.dataTransfer.files);
          if (files.length > 0) {
            // Show device selection modal
            console.log('Files dropped:', files.map(f => f.name));
            // In a real implementation, this would show a device selection dialog
            alert(`${files.length} file${files.length > 1 ? 's' : ''} dropped. Select a device to transfer to.`);
          }
        }}
      >
        <h3>DROP FILES HERE</h3>
        <p>Drag files or folders to transfer</p>
        <p className="drop-hint">Release to select destination device</p>
      </div>
    </div>
  );
};

export default Devices;
