use anyhow::Result;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::env;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use sysinfo::{Components, Disks, Networks, Process, System};
use tauri::{Manager, State};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    pub version: String,
    pub platform: String,
    pub distribution: String,
    pub cpu_usage: String,
    pub memory_usage: String,
    pub network_interface: String,
    pub local_address: String,
    pub discovery_status: String,
    pub listening_port: u16,
    pub connected_devices: usize,
    pub active_transfers: usize,
    pub uptime: String,
}

pub struct DiagnosticsState {
    pub system: Arc<Mutex<System>>,
    pub start_time: Arc<Mutex<Instant>>,
    pub listening_port: Arc<Mutex<u16>>,
}

#[tauri::command]
pub fn get_diagnostics(
    state: State<'_, DiagnosticsState>,
    discovery_state: State<'_, crate::discovery::DiscoveryState>,
    network_state: State<'_, crate::network::NetworkState>,
    transfer_manager: State<'_, crate::transfer::TransferManager>,
    db_state: State<'_, crate::database::Database>,
) -> Result<String, String> {
    let mut system = state.system.lock().map_err(|e| e.to_string())?;
    system.refresh_all();
    // Let's try to use the Networks struct directly
    let mut networks = Networks::new();
    networks.refresh();

    let uptime = sysinfo::System::uptime();
    let hours = uptime / 3600;
    let minutes = (uptime % 3600) / 60;

    // Get primary network interface and IP address
    let network_interface = networks
        .iter()
        .find(|(name, data)| {
            // Let's just try to access some basic properties to see what's available
            let _ = data.received();
            let _ = data.transmitted();
            // Skip loopback interfaces by name (common convention)
            !name.starts_with("lo") && !name.contains("loopback")
        })
        .map(|(name, _)| name.clone())
        .unwrap_or_else(|| "unknown".to_string());

    let local_address = networks
        .get(&network_interface)
        .map(|data| {
            // Try to get IP address - using addresses() method
            let mut ip_address = "0.0.0.0".to_string();
            for addr in data.addresses() {
                // Prefer IPv4 addresses that are not loopback
                if let IpAddr::V4(ipv4) = addr {
                    if !ipv4.is_loopback() {
                        ip_address = ipv4.to_string();
                        break;
                    }
                }
                // If we have an IPv6 address that's not loopback, use it as fallback
                if let IpAddr::V6(ipv6) = addr {
                    if !ipv6.is_loopback() && ip_address == "0.0.0.0" {
                        ip_address = ipv6.to_string();
                    }
                }
            }
            ip_address
        })
        .unwrap_or_else(|| "0.0.0.0".to_string());

    // Get discovery status
    let discovery_status = {
        let running = *discovery_state.running.lock().map_err(|e| e.to_string())?;
        if running { "Running" } else { "Stopped" }.to_string()
    };

    // Get listening port
    let listening_port = *network_state.port.lock().map_err(|e| e.to_string())?;

    // Get connected devices count
    let connected_devices = {
        let devices = discovery_state
            .discovered_devices
            .lock()
            .map_err(|e| e.to_string())?;
        devices.len()
    };

    // Get active transfers count
    let active_transfers = {
        let transfers = transfer_manager
            .active_transfers
            .lock()
            .map_err(|e| e.to_string())?;
        transfers
            .iter()
            .filter(|(_, t)| {
                let state = t.lock().unwrap_or_else(|e| e.into_inner());
                matches!(
                    state.status,
                    crate::transfer::TransferStatus::Connecting
                        | crate::transfer::TransferStatus::Authenticating
                        | crate::transfer::TransferStatus::WaitingForAccept
                        | crate::transfer::TransferStatus::Transferring
                        | crate::transfer::TransferStatus::Verifying
                )
            })
            .count()
    };

    // System info
    let info = SystemInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        platform: std::env::consts::OS.to_string(),
        distribution: {
            // Try to get Linux distribution info
            let mut dist = "Unknown".to_string();
            if std::env::consts::OS == "linux" {
                if let Ok(release) = std::fs::read_to_string("/etc/os-release") {
                    if let Some(line) = release.lines().find(|l| l.starts_with("PRETTY_NAME=")) {
                        if let Some(value) = line.strip_prefix("PRETTY_NAME=") {
                            dist = value.trim_matches('"').to_string();
                        }
                    }
                }
            }
            dist
        },
        cpu_usage: format!("{}%", system.global_cpu_usage()),
        memory_usage: {
            let used = system.used_memory();
            let total = system.total_memory();
            format!(
                "{} MB / {} GB",
                used / 1024 / 1024,
                total / 1024 / 1024 / 1024
            )
        },
        network_interface,
        local_address,
        discovery_status,
        listening_port,
        connected_devices,
        active_transfers,
        uptime: format!("{}h {}m", hours, minutes),
    };

    Ok(serde_json::to_string(&info).map_err(|e| e.to_string())?)
}

// Initialize diagnostics state
pub fn init_diagnostics_state(port: u16) -> DiagnosticsState {
    DiagnosticsState {
        system: Arc::new(Mutex::new(System::new_all())),
        start_time: Arc::new(Mutex::new(Instant::now())),
        listening_port: Arc::new(Mutex::new(port)),
    }
}
