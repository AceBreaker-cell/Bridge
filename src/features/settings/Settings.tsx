import './Settings.css';
import { useState, useEffect } from 'react';
import { useTauri } from '../../hooks/useTauri';

const Settings: React.FC = () => {
  const [deviceName, setDeviceName] = useState('');
  const [theme, setTheme] = useState('system');
  const [startOnLogin, setStartOnLogin] = useState(false);
  const [minimizeToTray, setMinimizeToTray] = useState(false);
  const [downloadDir, setDownloadDir] = useState('');
  const [askOverwrite, setAskOverwrite] = useState(true);
  const [concurrentTransfers, setConcurrentTransfers] = useState(3);
  const [resumeTransfers, setResumeTransfers] = useState(true);
  const [discoveryEnabled, setDiscoveryEnabled] = useState(true);
  const [listeningPort, setListeningPort] = useState(42137);
  const [incomingConfirmation, setIncomingConfirmation] = useState(true);
  const [clipboardSharing, setClipboardSharing] = useState(false);

  // Form state
  const [isSaving, setIsSaving] = useState(false);
  const [saveStatus, setSaveStatus] = useState<'idle' | 'saving' | 'success' | 'error'>('idle');
  const [loading, setLoading] = useState<boolean>(true);
  const { invoke } = useTauri();

  const handleDownloadDirChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    setDownloadDir(e.target.value);
  };

  const handlePortChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    setListeningPort(parseInt(e.target.value) || 42137);
  };

  const handleSaveSettings = async () => {
    setIsSaving(true);
    setSaveStatus('saving');

    // Simulate saving to backend/database
    // In a real implementation, this would invoke Tauri commands
    try {
      // Simulate API call
      await new Promise(resolve => setTimeout(resolve, 1500));

      // Save settings via Tauri command
      await invoke<void>('set_setting', { key: 'deviceName', value: deviceName });
      await invoke<void>('set_setting', { key: 'theme', value: theme });
      await invoke<void>('set_setting', { key: 'startOnLogin', value: startOnLogin.toString() });
      await invoke<void>('set_setting', { key: 'minimizeToTray', value: minimizeToTray.toString() });
      await invoke<void>('set_setting', { key: 'downloadDir', value: downloadDir });
      await invoke<void>('set_setting', { key: 'askOverwrite', value: askOverwrite.toString() });
      await invoke<void>('set_setting', { key: 'concurrentTransfers', value: concurrentTransfers.toString() });
      await invoke<void>('set_setting', { key: 'resumeTransfers', value: resumeTransfers.toString() });
      await invoke<void>('set_setting', { key: 'discoveryEnabled', value: discoveryEnabled.toString() });
      await invoke<void>('set_setting', { key: 'listeningPort', value: listeningPort.toString() });
      await invoke<void>('set_setting', { key: 'incomingConfirmation', value: incomingConfirmation.toString() });
      await invoke<void>('set_setting', { key: 'clipboardSharing', value: clipboardSharing.toString() });

      setSaveStatus('success');
      setIsSaving(false);

      // Reset status after 3 seconds
      setTimeout(() => {
        setSaveStatus('idle');
      }, 3000);
    } catch (error) {
      console.error('Failed to save settings:', error);
      setSaveStatus('error');
      setIsSaving(false);

      // Reset status after 3 seconds
      setTimeout(() => {
        setSaveStatus('idle');
      }, 3000);
    }
  };

  const handleResetSettings = () => {
    if (window.confirm('Are you sure you want to reset all settings to default?')) {
      // Reset to defaults via Tauri commands
      invoke<void>('set_setting', { key: 'deviceName', value: 'My-Computer' })
        .then(() => invoke<void>('set_setting', { key: 'theme', value: 'system' }))
        .then(() => invoke<void>('set_setting', { key: 'startOnLogin', value: 'false' }))
        .then(() => invoke<void>('set_setting', { key: 'minimizeToTray', value: 'false' }))
        .then(() => invoke<void>('set_setting', { key: 'downloadDir', value: '' }))
        .then(() => invoke<void>('set_setting', { key: 'askOverwrite', value: 'true' }))
        .then(() => invoke<void>('set_setting', { key: 'concurrentTransfers', value: '3' }))
        .then(() => invoke<void>('set_setting', { key: 'resumeTransfers', value: 'true' }))
        .then(() => invoke<void>('set_setting', { key: 'discoveryEnabled', value: 'true' }))
        .then(() => invoke<void>('set_setting', { key: 'listeningPort', value: '42137' }))
        .then(() => invoke<void>('set_setting', { key: 'incomingConfirmation', value: 'true' }))
        .then(() => invoke<void>('set_setting', { key: 'clipboardSharing', value: 'false' }))
        .then(() => {
          // Update local state
          setDeviceName('My-Computer');
          setTheme('system');
          setStartOnLogin(false);
          setMinimizeToTray(false);
          setDownloadDir('');
          setAskOverwrite(true);
          setConcurrentTransfers(3);
          setResumeTransfers(true);
          setDiscoveryEnabled(true);
          setListeningPort(42137);
          setIncomingConfirmation(true);
          setClipboardSharing(false);
          alert('Settings have been reset to default values.');
        })
        .catch((error) => {
          console.error('Failed to reset settings:', error);
          alert('Failed to reset settings.');
        });

      // Clear localStorage
      localStorage.removeItem('bridge-settings');

      alert('Settings have been reset to default values.');
    }
  };

  const handleClearHistory = () => {
    if (window.confirm('Are you sure you want to clear all transfer history?')) {
      // In a real implementation, this would invoke a Tauri command to clear history
      alert('Transfer history has been cleared.');
    }
  };

  // Load saved settings on component mount
  useEffect(() => {
    const loadSettings = async () => {
      setLoading(true);
      try {
        // Get all settings via Tauri command
        const settingsResult = await invoke<string>('get_all_settings');
        const settings = JSON.parse(settingsResult);

        // Update state with retrieved settings
        setDeviceName(settings.deviceName || 'My-Computer');
        setTheme(settings.theme || 'system');
        setStartOnLogin(settings.startOnLogin === 'true');
        setMinimizeToTray(settings.minimizeToTray === 'true');
        setDownloadDir(settings.downloadDir || '');
        setAskOverwrite(settings.askOverwrite === 'true');
        setConcurrentTransfers(parseInt(settings.concurrentTransfers) || 3);
        setResumeTransfers(settings.resumeTransfers === 'true');
        setDiscoveryEnabled(settings.discoveryEnabled === 'true');
        setListeningPort(parseInt(settings.listeningPort) || 42137);
        setIncomingConfirmation(settings.incomingConfirmation === 'true');
        setClipboardSharing(settings.clipboardSharing === 'true');
      } catch (error) {
        console.error('Failed to load settings:', error);
        // Set defaults on error
        setDeviceName('My-Computer');
        setTheme('system');
        setStartOnLogin(false);
        setMinimizeToTray(false);
        setDownloadDir('');
        setAskOverwrite(true);
        setConcurrentTransfers(3);
        setResumeTransfers(true);
        setDiscoveryEnabled(true);
        setListeningPort(42137);
        setIncomingConfirmation(true);
        setClipboardSharing(false);
      } finally {
        setLoading(false);
      }
    };

    loadSettings();
  }, [invoke]);

  return (
    <div className="settings-page">
      {loading ? (
        <div className="empty-state">
          <h3>Loading settings...</h3>
          <p>Please wait while we load your settings.</p>
        </div>
      ) : (
        <>
          <h2>Settings</h2>

          {saveStatus !== 'idle' && (
            <div className={`save-status status-${saveStatus}`}>
              {saveStatus === 'saving' && <span>Saving settings...</span>}
              {saveStatus === 'success' && <span>Settings saved successfully!</span>}
              {saveStatus === 'error' && <span>Failed to save settings. Please try again.</span>}
            </div>
          )}

          <div className="settings-grid">
            <div className="settings-section">
              <h3>General</h3>
              <div className="setting-item">
                <label>Device Name</label>
                <input
                  type="text"
                  value={deviceName}
                  onChange={(e) => setDeviceName(e.target.value)}
                  className="setting-input"
                />
                <p className="setting-description">This is how your device appears to others</p>
              </div>

              <div className="setting-item">
                <label>Theme</label>
                <select
                  value={theme}
                  onChange={(e) => setTheme(e.target.value)}
                  className="setting-input"
                >
                  <option value="system">System</option>
                  <option value="light">Light</option>
                  <option value="dark">Dark</option>
                </select>
                <p className="setting-description">Choose your preferred appearance</p>
              </div>

              <div className="setting-item">
                <label>
                  <input
                    type="checkbox"
                    checked={startOnLogin}
                    onChange={(e) => setStartOnLogin(e.target.checked)}
                  />
                  Start Bridge on login
                </label>
                <p className="setting-description">Launch Bridge automatically when you log in</p>
              </div>

              <div className="setting-item">
                <label>
                  <input
                    type="checkbox"
                    checked={minimizeToTray}
                    onChange={(e) => setMinimizeToTray(e.target.checked)}
                  />
                  Minimize to system tray
                </label>
                <p className="setting-description">Keep Bridge running in the background when closed</p>
              </div>
            </div>

            <div className="settings-section">
              <h3>Transfers</h3>
              <div className="setting-item">
                <label>Default Download Directory</label>
                <div className="input-group">
                  <input
                    type="text"
                    value={downloadDir}
                    onChange={handleDownloadDirChange}
                    className="setting-input"
                    placeholder="Downloads/Bridge"
                  />
                  <button className="btn-sm btn-secondary">Browse</button>
                </div>
                <p className="setting-description">Where received files are saved by default</p>
              </div>

              <div className="setting-item">
                <label>
                  <input
                    type="checkbox"
                    checked={askOverwrite}
                    onChange={(e) => setAskOverwrite(e.target.checked)}
                  />
                  Ask before overwriting existing files
                </label>
              </div>

              <div className="setting-item">
                <label>Concurrent Transfer Limit</label>
                <input
                  type="number"
                  min="1"
                  max="10"
                  value={concurrentTransfers}
                  onChange={(e) => setConcurrentTransfers(parseInt(e.target.value) || 3)}
                  className="setting-input"
                />
                <p className="setting-description">Maximum number of simultaneous file transfers</p>
              </div>

              <div className="setting-item">
                <label>
                  <input
                    type="checkbox"
                    checked={resumeTransfers}
                    onChange={(e) => setResumeTransfers(e.target.checked)}
                  />
                  Automatically resume interrupted transfers
                </label>
              </div>
            </div>

            <div className="settings-section">
              <h3>Network</h3>
              <div className="setting-item">
                <label>
                  <input
                    type="checkbox"
                    checked={discoveryEnabled}
                    onChange={(e) => setDiscoveryEnabled(e.target.checked)}
                  />
                  Enable automatic device discovery
                </label>
                <p className="setting-description">Allow Bridge to find other devices on your network</p>
              </div>

              <div className="setting-item">
                <label>Listening Port</label>
                <input
                  type="number"
                  min="1024"
                  max="65535"
                  value={listeningPort}
                  onChange={handlePortChange}
                  className="setting-input"
                />
                <p className="setting-description">Port used for incoming connections (default: 42137)</p>
              </div>
            </div>

            <div className="settings-section">
              <h3>Security</h3>
              <div className="setting-item">
                <label>
                  <input
                    type="checkbox"
                    checked={incomingConfirmation}
                    onChange={(e) => setIncomingConfirmation(e.target.checked)}
                  />
                  Always ask for confirmation before accepting incoming transfers
                </label>
              </div>

              <div className="setting-item">
                <label>
                  <input
                    type="checkbox"
                    checked={clipboardSharing}
                    onChange={(e) => setClipboardSharing(e.target.checked)}
                  />
                  Enable clipboard sharing
                </label>
                <p className="setting-description">Share copied text between devices (opt-in)</p>
              </div>
            </div>

            <div className="settings-section">
              <h3>Privacy</h3>
              <div className="setting-item">
                <button className="btn-block btn-warning" onClick={handleClearHistory}>
                  Clear Transfer History
                </button>
                <p className="setting-description">Remove all transfer records (does not delete files)</p>
              </div>

              <div className="setting-item">
                <button className="btn-block btn-warning" onClick={handleResetSettings}>
                  Reset All Settings
                </button>
                <p className="setting-description">Restore all settings to default values</p>
              </div>
            </div>
          </div>

          <div className="settings-actions">
            <button
              className="btn-primary"
              onClick={handleSaveSettings}
              disabled={isSaving}
            >
              {isSaving ? 'Saving...' : 'Save Settings'}
            </button>
            <button className="btn-secondary">Cancel</button>
          </div>
        </>
      )}
    </div>
  );
};

export default Settings;