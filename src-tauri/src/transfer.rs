use crate::identity::DeviceIdentity;
use crate::network::{send_message_to_peer, Connection, NetworkState};
use crate::protocol::{
    BridgeMessage, FileInfo, MessageType, TransferCompletePayload, TransferDataPayload,
    TransferRequestPayload,
};
use crate::security::SecureChannel;
use anyhow::Result;
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tauri::{Manager, State};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferState {
    pub id: String,
    pub sender_id: String,
    pub receiver_id: String,
    pub files: Vec<FileInfo>,
    pub total_size: u64,
    pub transferred_size: u64,
    pub status: TransferStatus,
    pub direction: TransferDirection,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TransferStatus {
    Queued,
    Connecting,
    Authenticating,
    WaitingForAccept,
    Transferring,
    Verifying,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TransferDirection {
    Sending,
    Receiving,
}

#[derive(Debug)]
pub struct TransferManager {
    pub active_transfers: Arc<Mutex<HashMap<String, Arc<Mutex<TransferState>>>>>,
    pub transfer_handler: Arc<Mutex<Option<std::thread::JoinHandle<()>>>>,
    pub running: Arc<Mutex<bool>>,
}

#[tauri::command]
pub fn request_file_transfer(
    transfer_state: State<'_, TransferManager>,
    network_state: State<'_, NetworkState>,
    identity_state: State<'_, crate::identity::IdentityState>,
    files: Vec<String>,
    device_id: String,
) -> Result<String, String> {
    // Get local device identity
    let local_identity = {
        let identity_guard = identity_state.0.lock().map_err(|e| e.to_string())?;
        identity_guard
            .as_ref()
            .expect("Identity should be initialized")
            .clone()
    };

    // Create file info list
    let mut file_infos = Vec::new();
    let mut total_size = 0u64;

    for file_path in files {
        let path = Path::new(&file_path);
        if let Ok(metadata) = fs::metadata(path) {
            let size = metadata.len();
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();

            file_infos.push(FileInfo {
                name: name.clone(),
                size,
                is_directory: metadata.is_dir(),
                path: name.clone(), // Simplified - would preserve directory structure
            });

            total_size += size;
        }
    }

    // Create transfer ID
    let transfer_id = Uuid::new_v4().to_string();

    // Create transfer state
    let transfer_state_inner = TransferState {
        id: transfer_id.clone(),
        sender_id: local_identity.id.clone(),
        receiver_id: device_id.clone(),
        files: file_infos.clone(),
        total_size,
        transferred_size: 0,
        status: TransferStatus::Queued,
        direction: TransferDirection::Sending,
    };

    // Store transfer state
    let mut transfers = transfer_state
        .active_transfers
        .lock()
        .expect("Failed to acquire lock on active_transfers");
    transfers.insert(
        transfer_id.clone(),
        Arc::new(Mutex::new(transfer_state_inner)),
    );

    // Create and send TransferRequest message
    let transfer_request = TransferRequestPayload {
        transfer_id: transfer_id.clone(),
        sender_id: local_identity.id.clone(),
        files: file_infos,
        total_size,
    };

    let message = BridgeMessage::new(MessageType::TransferRequest, &transfer_request)
        .map_err(|e| e.to_string())?;

    // Send to target device
    if let Err(e) = send_message_to_peer(&network_state.inner(), &device_id, &message) {
        warn!("Failed to send transfer request to {}: {}", device_id, e);
        // Update transfer status to failed
        if let Some(transfer) = transfers.get(&transfer_id) {
            if let Ok(mut state) = transfer.lock() {
                state.status = TransferStatus::Failed;
            }
        }
        return Err(format!("Failed to send transfer request: {}", e));
    }

    // Update transfer status to Connecting
    if let Some(transfer) = transfers.get(&transfer_id) {
        if let Ok(mut state) = transfer.lock() {
            state.status = TransferStatus::Connecting;
        }
    }

    Ok(transfer_id)
}

#[tauri::command]
pub fn get_transfer_status(
    state: State<'_, TransferManager>,
    transfer_id: String,
) -> Result<String, String> {
    let transfers = state
        .active_transfers
        .lock()
        .expect("Failed to acquire lock on active_transfers");
    if let Some(transfer) = transfers.get(&transfer_id) {
        let state = transfer.lock().expect("Failed to acquire lock on transfer");
        Ok(serde_json::to_string(&*state).expect("Failed to serialize transfer state"))
    } else {
        Err("Transfer not found".to_string())
    }
}

#[tauri::command]
pub fn cancel_transfer(
    state: State<'_, TransferManager>,
    transfer_id: String,
) -> Result<(), String> {
    let mut transfers = state
        .active_transfers
        .lock()
        .expect("Failed to acquire lock on active_transfers");
    if let Some(transfer) = transfers.get(&transfer_id) {
        let mut state = transfer.lock().expect("Failed to acquire lock on transfer");
        state.status = TransferStatus::Cancelled;
        Ok(())
    } else {
        Err("Transfer not found".to_string())
    }
}

#[tauri::command]
pub fn accept_transfer(
    transfer_state: State<'_, TransferManager>,
    network_state: State<'_, NetworkState>,
    identity_state: State<'_, crate::identity::IdentityState>,
    transfer_id: String,
) -> Result<(), String> {
    let mut transfers = transfer_state
        .active_transfers
        .lock()
        .expect("Failed to acquire lock on active_transfers");
    if let Some(transfer) = transfers.get(&transfer_id) {
        let mut state = transfer.lock().expect("Failed to acquire lock on transfer");

        // Get local device identity
        let local_identity = {
            let identity_guard = identity_state.0.lock().map_err(|e| e.to_string())?;
            identity_guard
                .as_ref()
                .expect("Identity should be initialized")
                .clone()
        };

        // Get transfer info to know who to send response to
        let sender_id = state.sender_id.clone();

        // Update transfer status
        state.status = TransferStatus::Transferring;

        // Create and send TransferAccept message
        let transfer_accept = crate::protocol::TransferAcceptPayload {
            transfer_id: transfer_id.clone(),
        };

        let message = BridgeMessage::new(MessageType::TransferAccept, &transfer_accept)
            .map_err(|e| e.to_string())?;

        // Send to the device that requested the transfer
        if let Err(e) = send_message_to_peer(&network_state.inner(), &sender_id, &message) {
            warn!("Failed to send transfer accept to {}: {}", sender_id, e);
            // Note: We don't change the transfer status here as the transfer might still proceed
            // depending on how the initiator handles the failure
        }

        Ok(())
    } else {
        Err("Transfer not found".to_string())
    }
}

#[tauri::command]
pub fn reject_transfer(
    transfer_state: State<'_, TransferManager>,
    network_state: State<'_, NetworkState>,
    identity_state: State<'_, crate::identity::IdentityState>,
    transfer_id: String,
) -> Result<(), String> {
    let mut transfers = transfer_state
        .active_transfers
        .lock()
        .expect("Failed to acquire lock on active_transfers");
    if let Some(transfer) = transfers.get(&transfer_id) {
        let mut state = transfer.lock().expect("Failed to acquire lock on transfer");

        // Get local device identity
        let local_identity = {
            let identity_guard = identity_state.0.lock().map_err(|e| e.to_string())?;
            identity_guard
                .as_ref()
                .expect("Identity should be initialized")
                .clone()
        };

        // Get transfer info to know who to send response to
        let sender_id = state.sender_id.clone();

        // Update transfer status
        state.status = TransferStatus::Failed;

        // Create and send TransferReject message
        let transfer_reject = crate::protocol::TransferRejectPayload {
            transfer_id: transfer_id.clone(),
        };

        let message = BridgeMessage::new(MessageType::TransferReject, &transfer_reject)
            .map_err(|e| e.to_string())?;

        // Send to the device that requested the transfer
        if let Err(e) = send_message_to_peer(&network_state.inner(), &sender_id, &message) {
            warn!("Failed to send transfer reject to {}: {}", sender_id, e);
            // Note: We don't change the transfer status here as the transfer might still proceed
            // depending on how the initiator handles the failure
        }

        Ok(())
    } else {
        Err("Transfer not found".to_string())
    }
}

// Simulate transfer progress (in real implementation, this would be driven by actual network I/O)
fn simulate_transfer_progress(transfer_id: String, state: Arc<Mutex<TransferState>>) {
    thread::spawn(move || {
        let total_size = {
            let guard = state.lock().unwrap();
            guard.total_size
        };

        let mut transferred = 0u64;
        while transferred < total_size {
            {
                let mut guard = state.lock().unwrap();
                if guard.status == TransferStatus::Cancelled
                    || guard.status == TransferStatus::Failed
                {
                    break;
                }

                // Simulate transferring chunks
                let chunk_size = std::cmp::min(1024 * 1024, total_size - transferred); // 1MB chunks
                transferred += chunk_size;
                guard.transferred_size = transferred;

                // Check if we're done
                if transferred >= total_size {
                    guard.status = TransferStatus::Verifying;
                }
            }

            thread::sleep(Duration::from_millis(100)); // Simulate network delay
        }

        // Simulate verification
        {
            let mut guard = state.lock().unwrap();
            if guard.status == TransferStatus::Verifying {
                // Simulate verification delay
                thread::sleep(Duration::from_secs(2));
                guard.status = TransferStatus::Completed;
            }
        }
    });
}

// Initialize transfer manager
pub fn init_transfer_manager() -> TransferManager {
    TransferManager {
        active_transfers: Arc::new(Mutex::new(HashMap::new())),
        transfer_handler: Arc::new(Mutex::new(None)),
        running: Arc::new(Mutex::new(true)),
    }
}
