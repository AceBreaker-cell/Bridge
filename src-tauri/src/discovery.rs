use crate::identity::DeviceIdentity;
use anyhow::Result;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tauri::{Manager, State};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryInfo {
    pub device_id: String,
    pub device_name: String,
    pub platform: String,
    pub port: u16,
    pub last_seen: u64, // Unix timestamp in seconds
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryMessage {
    pub protocol: String,
    pub version: u32,
    pub device_id: String,
    pub device_name: String,
    pub platform: String,
    pub port: u16,
}

impl DiscoveryMessage {
    pub fn new(identity: &DeviceIdentity, port: u16) -> Self {
        Self {
            protocol: "BRIDGE".to_string(),
            version: 1,
            device_id: identity.id.clone(),
            device_name: identity.name.clone(),
            platform: std::env::consts::OS.to_string(),
            port: port,
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_string(self).unwrap().into_bytes()
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        Ok(serde_json::from_slice(bytes)?)
    }
}

pub struct DiscoveryState {
    pub running: Arc<Mutex<bool>>,
    pub socket: Arc<Mutex<Option<UdpSocket>>>,
    pub discovered_devices: Arc<Mutex<HashMap<String, DiscoveryInfo>>>,
    pub my_port: Arc<Mutex<u16>>,
    pub identity: Arc<Mutex<Option<crate::identity::DeviceIdentity>>>,
}

#[tauri::command]
pub fn start_discovery(state: State<'_, DiscoveryState>) -> Result<(), String> {
    let mut running = state.running.lock().map_err(|e| e.to_string())?;
    if *running {
        return Ok(());
    }

    *running = true;

    // Get my port
    let my_port = *state.my_port.lock().map_err(|e| e.to_string())?;

    // Clone state for the thread
    let state_clone = DiscoveryState {
        running: state.running.clone(),
        socket: state.socket.clone(),
        discovered_devices: state.discovered_devices.clone(),
        my_port: state.my_port.clone(),
        identity: state.identity.clone(),
    };

    // Spawn discovery thread
    thread::spawn(move || {
        let _ = discovery_loop(state_clone, my_port);
    });

    Ok(())
}

#[tauri::command]
pub fn stop_discovery(state: State<'_, DiscoveryState>) -> Result<(), String> {
    let mut running = state.running.lock().map_err(|e| e.to_string())?;
    *running = false;

    // Close socket
    if let Ok(mut socket) = state.socket.lock() {
        if let Some(_sock) = socket.take() {
            // Socket will be closed when dropped
        }
    }

    Ok(())
}

#[tauri::command]
pub fn get_discovered_devices(state: State<'_, DiscoveryState>) -> Result<String, String> {
    let devices = state.discovered_devices.lock().map_err(|e| e.to_string())?;
    let list: Vec<DiscoveryInfo> = devices.values().cloned().collect();
    Ok(serde_json::to_string(&list).map_err(|e| e.to_string())?)
}

fn discovery_loop(state: DiscoveryState, port: u16) -> Result<()> {
    // Create UDP socket for broadcasting and listening
    let socket = UdpSocket::bind(("0.0.0.0", port))?;
    socket.set_broadcast(true)?;

    // Store socket reference
    {
        let mut socket_guard = state.socket.lock().map_err(|e| e.into())?;
        *socket_guard = Some(socket.try_clone().map_err(|e| e.into())?);
    }

    let broadcast_addr = ("255.255.255.255", port);

    // Buffer for incoming packets
    let mut buf = [0u8; 1024];

    while *state.running.lock().map_err(|e| e.into())? {
        // Get current identity
        let identity_guard = state.identity.lock().map_err(|e| e.into())?;
        let identity = identity_guard
            .as_ref()
            .expect("Identity should be initialized");

        // Create discovery message from current identity
        let my_identity =
            DiscoveryMessage::new(identity, *state.my_port.lock().map_err(|e| e.into())?);

        // Send discovery broadcast
        let message = my_identity.to_bytes();
        socket.send_to(&message, broadcast_addr)?;

        // Listen for responses (with timeout)
        socket.set_read_timeout(Some(Duration::from_secs(1)))?;

        match socket.recv_from(&mut buf) {
            Ok((size, addr)) => {
                if size > 0 {
                    let message_bytes = &buf[..size];
                    if let Ok(message) = DiscoveryMessage::from_bytes(message_bytes) {
                        // Skip our own messages
                        if message.device_id != identity.id {
                            let info = DiscoveryInfo {
                                device_id: message.device_id,
                                device_name: message.device_name,
                                platform: message.platform,
                                port: message.port,
                                last_seen: std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs(),
                            };

                            let mut devices =
                                state.discovered_devices.lock().map_err(|e| e.into())?;
                            devices.insert(message.device_id.clone(), info);
                        }
                    }
                }
            }
            Err(_) => {
                // Timeout or error, continue loop
            }
        }

        // Wait before next broadcast
        thread::sleep(Duration::from_secs(3));

        // Clean up old entries (older than 10 seconds)
        let cutoff = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .saturating_sub(10);
        let mut devices = state.discovered_devices.lock().map_err(|e| e.into())?;
        devices.retain(|_, info| info.last_seen > cutoff);
    }

    Ok(())
}

// Initialize discovery state
pub fn init_discovery_state(
    port: u16,
    identity: Arc<Mutex<Option<crate::identity::DeviceIdentity>>>,
) -> DiscoveryState {
    DiscoveryState {
        running: Arc::new(Mutex::new(false)),
        socket: Arc::new(Mutex::new(None)),
        discovered_devices: Arc::new(Mutex::new(HashMap::new())),
        my_port: Arc::new(Mutex::new(port)),
        identity,
    }
}
