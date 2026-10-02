use crate::identity::DeviceIdentity;
use crate::protocol::*;
use crate::security::{decrypt_data, encrypt_data, SecureChannel};
use anyhow::Result;
use log::{error, info, warn};
use ring::{agreement, hkdf};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tauri::State;

// Utility function to write data to a file at a specific offset
fn write_file_at_offset(path: &std::path::Path, offset: u64, data: &[u8]) -> std::io::Result<()> {
    use std::io::{Seek, SeekFrom, Write};

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(path)?;

    file.seek(SeekFrom::Start(offset))?;
    file.write_all(data)?;
    Ok(())
}

// Utility function to get or create a file for a transfer
fn get_transfer_file_path(
    transfer_id: &str,
    file_info: &crate::protocol::FileInfo,
    base_dir: &std::path::Path,
) -> std::path::PathBuf {
    // Create a subdirectory for this transfer
    let transfer_dir = base_dir.join(transfer_id);
    let _ = std::fs::create_dir_all(&transfer_dir);

    // Preserve the relative path structure
    transfer_dir.join(&file_info.path)
}

// Utility function to ensure parent directory exists for a file path
fn ensure_parent_dir_exists(path: &std::path::Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

pub struct Connection {
    pub peer_id: String,
    pub stream: Option<std::net::TcpStream>,
    pub secure_channel: Option<SecureChannel>,
}

impl Clone for Connection {
    fn clone(&self) -> Self {
        Self {
            peer_id: self.peer_id.clone(),
            stream: None, // TcpStream is not cloneable, so we reset it
            secure_channel: self.secure_channel.clone(),
        }
    }
}

impl std::fmt::Debug for Connection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Connection")
            .field("peer_id", &self.peer_id)
            .field("stream", &self.stream.as_ref().map(|_| "<TcpStream>"))
            .field("secure_channel", &self.secure_channel)
            .finish()
    }
}

#[derive(Debug)]
pub struct NetworkState {
    pub listener: Arc<Mutex<Option<Arc<std::net::TcpListener>>>>,
    pub connections: Arc<Mutex<HashMap<String, Arc<Mutex<Connection>>>>>,
    pub running: Arc<Mutex<bool>>,
    pub port: u16,
    pub identity: Arc<Mutex<Option<crate::identity::DeviceIdentity>>>,
    pub transfer_manager: Arc<Mutex<crate::transfer::TransferManager>>,
}

// Initialize network state
pub fn init_network_state(
    port: u16,
    identity: Arc<Mutex<Option<crate::identity::DeviceIdentity>>>,
    transfer_manager: Arc<Mutex<crate::transfer::TransferManager>>,
) -> NetworkState {
    NetworkState {
        listener: Arc::new(Mutex::new(None)),
        connections: Arc::new(Mutex::new(HashMap::new())),
        running: Arc::new(Mutex::new(true)),
        port,
        identity,
        transfer_manager,
    }
}

fn handle_bridge_message(
    conn: &mut Connection,
    message: BridgeMessage,
    identity_state: Arc<Mutex<Option<crate::identity::DeviceIdentity>>>,
    state: &NetworkState,
) -> Result<()> {
    info!(
        "Received message: {:?} from {}",
        message.msg_type, conn.peer_id
    );

    // Handle different message types
    match message.msg_type {
        MessageType::Ping => {
            // Respond with pong
            if let Some(stream) = &mut conn.stream {
                let pong =
                    BridgeMessage::new(MessageType::Pong, &()).map_err(|e| anyhow::anyhow!(e))?;
                let bytes = pong.to_bytes();
                let _ = stream.write_all(&bytes);
            }
        }
        MessageType::HandshakeRequest => {
            // Handle handshake (involves crypto key exchange)
            info!("Handling handshake request from {}", conn.peer_id);

            // Deserialize the handshake payload
            let payload = message
                .payload_as::<HandshakePayload>()
                .map_err(|e| anyhow::anyhow!(e))?;

            // Get our identity
            let identity_guard = match identity_state.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            let identity = identity_guard
                .as_ref()
                .expect("Identity should be initialized");

            // Generate our ephemeral key pair for X25519 key exchange
            let our_private_key = agreement::EphemeralPrivateKey::generate(
                &agreement::X25519,
                &ring::rand::SystemRandom::new(),
            )
            .map_err(|e| anyhow::anyhow!("Failed to generate ephemeral private key: {}", e))?;
            let our_public_key = our_private_key
                .compute_public_key()
                .map_err(|e| anyhow::anyhow!("Failed to compute public key: {}", e))?;

            // Perform X25519 key agreement
            let their_public_key =
                agreement::UnparsedPublicKey::new(&agreement::X25519, &payload.public_key);
            let mut shared_secret = Some([0u8; 32]);
            agreement::agree_ephemeral(our_private_key, &their_public_key, |shared_secret_bytes| {
                if let Some(ref mut secret) = shared_secret {
                    secret.copy_from_slice(shared_secret_bytes);
                }
                0u8 // return value ignored
            })
            .map_err(|e| anyhow::anyhow!("X25519 key agreement failed: {}", e))?;
            let shared_secret = shared_secret.expect("Shared secret should be set");

            // Generate nonces for AES-GCM (using part of shared secret)
            let mut init_nonce = [0u8; 12];
            let mut response_nonce = [0u8; 12];
            init_nonce.copy_from_slice(&shared_secret[..12]);
            response_nonce.copy_from_slice(&shared_secret[12..24]);

            // Create encryption keys from shared secret (using HKDF)
            let salt = b"BRIDGE_SECURE_CHANNEL_V1";
            let ik = hkdf::Prk::new_less_safe(hkdf::HKDF_SHA256, &shared_secret);

            // Derive init key (for encrypting data we send)
            let mut init_key_input = Vec::new();
            init_key_input.extend_from_slice(&init_nonce);
            init_key_input.extend_from_slice(b"init");
            pub(crate) struct EmptyKeyType1;
            impl hkdf::KeyType for EmptyKeyType1 {
                fn len(&self) -> usize {
                    0
                }
            };
            let init_key_input_refs = [init_key_input.as_slice()];
            let okm_result = ik
                .expand(&init_key_input_refs, EmptyKeyType1)
                .map_err(|e| anyhow::anyhow!("HKDF expand for init key failed: {}", e))?;
            let mut init_key = [0u8; 32];
            okm_result.fill(&mut init_key);

            // Derive response key (for encrypting data we receive)
            let mut response_key_input = Vec::new();
            response_key_input.extend_from_slice(&response_nonce);
            response_key_input.extend_from_slice(b"response");
            pub(crate) struct EmptyKeyType2;
            impl hkdf::KeyType for EmptyKeyType2 {
                fn len(&self) -> usize {
                    0
                }
            };
            let response_key_input_refs = [response_key_input.as_slice()];
            let okm2_result = ik
                .expand(&response_key_input_refs, EmptyKeyType2)
                .map_err(|e| anyhow::anyhow!("HKDF expand for response key failed: {}", e))?;
            let mut response_key = [0u8; 32];
            okm2_result.fill(&mut response_key);

            // Create secure channel
            let secure_channel = SecureChannel {
                init_key: init_key.to_vec(),
                response_key: response_key.to_vec(),
                init_nonce: init_nonce.to_vec(),
                response_nonce: response_nonce.to_vec(),
                init_mac_key: vec![0; 32], // Simplified - would derive properly
                response_mac_key: vec![0; 32], // Simplified - would derive properly
                sequence_num: 0,
            };

            // Store secure channel in connection
            conn.secure_channel = Some(secure_channel);

            // Send handshake response with our public key and nonce
            let response_payload = HandshakePayload {
                device_id: identity.id.clone(),
                public_key: our_public_key.as_ref().to_vec(),
                nonce: response_nonce.to_vec(), // Using our response nonce as nonce for now
            };

            if let Some(stream) = &mut conn.stream {
                let response =
                    BridgeMessage::new(MessageType::HandshakeResponse, &response_payload)
                        .map_err(|e| anyhow::anyhow!(e))?;
                let bytes = response.to_bytes();
                let _ = stream.write_all(&bytes);
            }
        }
        MessageType::HandshakeResponse => {
            // Handle handshake response (complete our side of key exchange)
            info!("Handling handshake response from {}", conn.peer_id);

            // Deserialize the handshake payload
            let payload = message
                .payload_as::<HandshakePayload>()
                .map_err(|e| anyhow::anyhow!(e))?;

            // Get our identity
            let identity_guard = match identity_state.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            let identity = identity_guard
                .as_ref()
                .expect("Identity should be initialized");

            // Generate our ephemeral key pair for X25519 key exchange
            let our_private_key = agreement::EphemeralPrivateKey::generate(
                &agreement::X25519,
                &ring::rand::SystemRandom::new(),
            )
            .map_err(|e| anyhow::anyhow!("Failed to generate ephemeral private key: {}", e))?;
            let our_public_key = our_private_key
                .compute_public_key()
                .map_err(|e| anyhow::anyhow!("Failed to compute public key: {}", e))?;

            // Perform X25519 key agreement
            let their_public_key =
                agreement::UnparsedPublicKey::new(&agreement::X25519, &payload.public_key);
            let mut shared_secret = Some([0u8; 32]);
            agreement::agree_ephemeral(our_private_key, &their_public_key, |shared_secret_bytes| {
                if let Some(ref mut secret) = shared_secret {
                    secret.copy_from_slice(shared_secret_bytes);
                }
                0u8 // return value ignored
            })
            .map_err(|e| anyhow::anyhow!("X25519 key agreement failed: {}", e))?;
            let shared_secret = shared_secret.expect("Shared secret should be set");

            // Generate nonces for AES-GCM (using part of shared secret)
            // Note: We need to use the nonces in the opposite order compared to the initiator
            // The initiator used: init_nonce = shared_secret[0..12], response_nonce = shared_secret[12..24]
            // As the responder, we should use: init_nonce = shared_secret[12..24], response_nonce = shared_secret[0..12]
            // So that we use the same keys for the same purposes
            let mut init_nonce = [0u8; 12];
            let mut response_nonce = [0u8; 12];
            init_nonce.copy_from_slice(&shared_secret[12..24]); // Opposite of initiator
            response_nonce.copy_from_slice(&shared_secret[0..12]); // Opposite of initiator

            // Create encryption keys from shared secret (using HKDF)
            let salt = b"BRIDGE_SECURE_CHANNEL_V1";
            let ik = hkdf::Prk::new_less_safe(hkdf::HKDF_SHA256, &shared_secret);

            // Derive init key (for encrypting data we send)
            // Note: As responder, we use what the initiator called "response key" for sending
            let mut init_key_input = Vec::new();
            init_key_input.extend_from_slice(&init_nonce);
            init_key_input.extend_from_slice(b"response"); // Opposite of initiator
            pub(crate) struct EmptyKeyType1;
            impl hkdf::KeyType for EmptyKeyType1 {
                fn len(&self) -> usize {
                    0
                }
            };
            let init_key_input_refs = [init_key_input.as_slice()];
            let okm_result = ik
                .expand(&init_key_input_refs, EmptyKeyType1)
                .map_err(|e| anyhow::anyhow!("HKDF expand for init key failed: {}", e))?;
            let mut init_key = [0u8; 32];
            okm_result.fill(&mut init_key);

            // Derive response key (for encrypting data we receive)
            // Note: As responder, we use what the initiator called "init key" for receiving
            let mut response_key_input = Vec::new();
            response_key_input.extend_from_slice(&response_nonce);
            response_key_input.extend_from_slice(b"init"); // Opposite of initiator
            pub(crate) struct EmptyKeyType2;
            impl hkdf::KeyType for EmptyKeyType2 {
                fn len(&self) -> usize {
                    0
                }
            };
            let response_key_input_refs = [response_key_input.as_slice()];
            let okm2_result = ik
                .expand(&response_key_input_refs, EmptyKeyType2)
                .map_err(|e| anyhow::anyhow!("HKDF expand for response key failed: {}", e))?;
            let mut response_key = [0u8; 32];
            okm2_result.fill(&mut response_key);

            // Create secure channel
            let secure_channel = SecureChannel {
                init_key: init_key.to_vec(),
                response_key: response_key.to_vec(),
                init_nonce: init_nonce.to_vec(),
                response_nonce: response_nonce.to_vec(),
                init_mac_key: vec![0; 32], // Simplified - would derive properly
                response_mac_key: vec![0; 32], // Simplified - would derive properly
                sequence_num: 0,
            };

            // Store secure channel in connection
            conn.secure_channel = Some(secure_channel);
        }
        MessageType::TransferRequest => {
            // Handle transfer request
            info!("Handling transfer request from {}", conn.peer_id);

            // Deserialize the transfer request payload
            let payload = message
                .payload_as::<TransferRequestPayload>()
                .map_err(|e| anyhow::anyhow!(e))?;

            // In a real implementation, we would:
            // 1. Check if we accept the transfer (based on settings, permissions, etc.)
            // 2. Create a transfer record in our transfer manager
            // 3. Send a TransferAccept or TransferReject response

            // For now, we'll automatically accept the transfer and create a basic response
            // TODO: Implement proper transfer acceptance logic based on user settings

            // Send transfer accept response
            let response_payload = crate::protocol::TransferAcceptPayload {
                transfer_id: payload.transfer_id.clone(),
            };

            if let Some(stream) = &mut conn.stream {
                let response = BridgeMessage::new(MessageType::TransferAccept, &response_payload)
                    .map_err(|e| anyhow::anyhow!(e))?;
                let bytes = response.to_bytes();
                let _ = stream.write_all(&bytes);
            }
        }
        MessageType::TransferAccept => {
            // Handle transfer accept - start sending file data
            info!("Handling transfer accept from {}", conn.peer_id);

            // Deserialize the transfer accept payload
            let payload = message
                .payload_as::<crate::protocol::TransferAcceptPayload>()
                .map_err(|e| anyhow::anyhow!(e))?;

            // Look up the transfer in our transfer manager and start sending data
            let transfer_found = {
                let transfer_manager_guard = match state.transfer_manager.lock() {
                    Ok(guard) => guard,
                    Err(e) => {
                        error!("Failed to lock transfer manager: {}", e);
                        return Err(anyhow::anyhow!("Failed to lock transfer manager: {}", e));
                    }
                };
                let active_transfers = &transfer_manager_guard.active_transfers;
                let transfers_guard = match active_transfers.lock() {
                    Ok(guard) => guard,
                    Err(e) => {
                        error!("Failed to lock active transfers: {}", e);
                        return Err(anyhow::anyhow!("Failed to lock active transfers: {}", e));
                    }
                };
                transfers_guard.contains_key(&payload.transfer_id)
            };

            if transfer_found {
                info!("Found transfer {}, starting file send", payload.transfer_id);

                // Spawn a thread to handle the file sending
                let state_clone = NetworkState {
                    listener: state.listener.clone(),
                    connections: state.connections.clone(),
                    running: state.running.clone(),
                    port: state.port,
                    identity: state.identity.clone(),
                    transfer_manager: state.transfer_manager.clone(),
                };
                let transfer_id_clone = payload.transfer_id.clone();
                let peer_id_clone = conn.peer_id.clone();

                std::thread::spawn(move || {
                    // Get the connection from the map using peer ID
                    let connection = {
                        if let Ok(connections) = state_clone.connections.lock() {
                            connections.get(&peer_id_clone).cloned()
                        } else {
                            None
                        }
                    };

                    if let Some(connection) = connection {
                        if let Ok(conn_guard) = connection.lock() {
                            // Extract secure channel while we hold the lock
                            let secure_channel = conn_guard.secure_channel.clone();
                            drop(conn_guard); // Drop the lock before spawning the transfer

                            if let Some(channel) = secure_channel {
                                let _ = send_file_transfer(
                                    state_clone,
                                    channel,
                                    &transfer_id_clone,
                                    &peer_id_clone,
                                );
                            }
                        }
                    }
                });
            } else {
                warn!(
                    "Transfer {} not found in transfer manager",
                    payload.transfer_id
                );
            }
        }
        MessageType::TransferData => {
            // Handle transfer data - receive and write file data
            info!("Handling transfer data from {}", conn.peer_id);

            // Deserialize the transfer data payload
            let payload = message
                .payload_as::<crate::protocol::TransferDataPayload>()
                .map_err(|e| anyhow::anyhow!(e))?;

            // Decrypt data using secure channel, write to file, update transfer progress
            if let Some(ref channel) = conn.secure_channel {
                // Decrypt the data
                let key_array: [u8; 32] = channel
                    .response_key
                    .clone()
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("Invalid key length"))?;
                let decrypted_data = crate::security::decrypt_data(&payload.data, &key_array)?;

                // Handle file reception
                let transfer_id = &payload.transfer_id;
                let chunk_index = payload.chunk_index;
                let is_last = payload.is_last;

                // Look up the transfer in our transfer manager
                let transfer_state_opt = {
                    let transfer_manager_guard = match state.transfer_manager.lock() {
                        Ok(guard) => guard,
                        Err(e) => {
                            error!("Failed to lock transfer manager: {}", e);
                            return Err(anyhow::anyhow!(e.to_string()));
                        }
                    };
                    let active_transfers = &transfer_manager_guard.active_transfers;
                    let transfers_guard = match active_transfers.lock() {
                        Ok(guard) => guard,
                        Err(e) => {
                            error!("Failed to lock active transfers: {}", e);
                            return Err(anyhow::anyhow!(e.to_string()));
                        }
                    };
                    transfers_guard.get(transfer_id).cloned()
                };

                if let Some(transfer_state) = transfer_state_opt {
                    // Get transfer details
                    let (receiver_id, files) = {
                        let state_guard = match transfer_state.lock() {
                            Ok(guard) => guard,
                            Err(e) => {
                                error!("Failed to lock transfer state: {}", e);
                                return Err(anyhow::anyhow!(e.to_string()));
                            }
                        };
                        (state_guard.receiver_id.clone(), state_guard.files.clone())
                    };

                    // Get secure channel for encryption info (we need to know if we're the receiver)
                    let is_receiver = {
                        let state_guard = match transfer_state.lock() {
                            Ok(guard) => guard,
                            Err(e) => {
                                error!("Failed to lock transfer state: {}", e);
                                return Err(anyhow::anyhow!(e.to_string()));
                            }
                        };
                        state_guard.direction == crate::transfer::TransferDirection::Receiving
                    };

                    if is_receiver {
                        // We are the receiver, so we should write the data to file
                        // For simplicity, we'll assume a single file transfer for now
                        // In a full implementation, we'd need to map chunk_index to the correct file and offset
                        if let Some(file_info) = files.first() {
                            // Determine base directory for received files
                            // In a real implementation, this would come from user settings
                            let base_dir = std::path::Path::new("./received");

                            // Get file path
                            let file_path =
                                get_transfer_file_path(transfer_id, file_info, base_dir);

                            // Ensure parent directory exists
                            let _ = ensure_parent_dir_exists(&file_path);

                            // Calculate offset
                            let offset = (chunk_index as u64) * 32768;

                            // Write decrypted data to file
                            if let Err(e) =
                                write_file_at_offset(&file_path, offset, &decrypted_data)
                            {
                                error!("Failed to write to file {}: {}", file_path.display(), e);
                            } else {
                                info!(
                                    "Wrote {} bytes to {} at offset {}",
                                    decrypted_data.len(),
                                    file_path.display(),
                                    offset
                                );
                            }

                            // Update transfer progress
                            let mut state_guard = match transfer_state.lock() {
                                Ok(guard) => guard,
                                Err(e) => {
                                    error!("Failed to lock transfer state: {}", e);
                                    return Err(anyhow::anyhow!(e.to_string()));
                                }
                            };
                            let bytes_written = decrypted_data.len() as u64;
                            state_guard.transferred_size += bytes_written;

                            // If this is the last chunk, send transfer complete
                            if is_last {
                                info!("Received last chunk for transfer {}, sending transfer complete", transfer_id);

                                // Update status to Verifying
                                state_guard.status = crate::transfer::TransferStatus::Verifying;

                                // In a full implementation, we would calculate the hash of received files
                                // and send it in the TransferComplete message
                                // For now, we'll just update the status
                                info!("Transfer {} marked as verifying", transfer_id);
                            }
                        }
                    } else {
                        info!("Received transfer data but we are the sender, ignoring");
                    }
                } else {
                    warn!("Transfer {} not found in transfer manager", transfer_id);
                }
            } else {
                warn!("Received transfer data but no secure channel established");
            }
        }
        MessageType::TransferComplete => {
            // Handle transfer complete - verify transfer
            info!("Handling transfer complete from {}", conn.peer_id);

            // Deserialize the transfer complete payload
            let payload = message
                .payload_as::<crate::protocol::TransferCompletePayload>()
                .map_err(|e| anyhow::anyhow!(e))?;

            // Look up the transfer in our transfer manager
            let transfer_state_opt = {
                let transfer_manager_guard = match state.transfer_manager.lock() {
                    Ok(guard) => guard,
                    Err(e) => {
                        error!("Failed to lock transfer manager: {}", e);
                        // Even if the mutex is poisoned, we can still use the guard
                        let guard = e.into_inner();
                        guard
                    }
                };
                let active_transfers = &transfer_manager_guard.active_transfers;
                let transfers_guard = match active_transfers.lock() {
                    Ok(guard) => guard,
                    Err(e) => {
                        error!("Failed to lock active transfers: {}", e);
                        // Even if the mutex is poisoned, we can still use the guard
                        let guard = e.into_inner();
                        guard
                    }
                };
                transfers_guard.get(&payload.transfer_id).cloned()
            };

            if let Some(transfer_state) = transfer_state_opt {
                // Get transfer details
                let is_sender = {
                    let state_guard = match transfer_state.lock() {
                        Ok(guard) => guard,
                        Err(e) => {
                            error!("Failed to lock transfer state: {}", e);
                            // Even if the mutex is poisoned, we can still use the guard
                            let guard = e.into_inner();
                            guard
                        }
                    };
                    state_guard.direction == crate::transfer::TransferDirection::Sending
                };

                if is_sender {
                    // We are the sender, so we received confirmation that the transfer completed
                    if payload.success {
                        info!("Transfer {} completed successfully", payload.transfer_id);
                        // Update transfer status to Completed
                        let mut state_guard = match transfer_state.lock() {
                            Ok(guard) => guard,
                            Err(e) => {
                                error!("Failed to lock transfer state: {}", e);
                                // Even if the mutex is poisoned, we can still use the guard
                                let guard = e.into_inner();
                                guard
                            }
                        };
                        state_guard.status = crate::transfer::TransferStatus::Completed;
                    } else {
                        error!("Transfer {} failed according to peer", payload.transfer_id);
                        // Update transfer status to Failed
                        let mut state_guard = match transfer_state.lock() {
                            Ok(guard) => guard,
                            Err(e) => {
                                error!("Failed to lock transfer state: {}", e);
                                // Even if the mutex is poisoned, we can still use the guard
                                let guard = e.into_inner();
                                guard
                            }
                        };
                        state_guard.status = crate::transfer::TransferStatus::Failed;
                    }
                } else {
                    // We are the receiver, so we need to send verification
                    info!(
                        "We are the receiver for transfer {}, sending verification",
                        payload.transfer_id
                    );
                    // Update status to Completed (we've received all data)
                    let mut state_guard = match transfer_state.lock() {
                        Ok(guard) => guard,
                        Err(e) => {
                            error!("Failed to lock transfer state: {}", e);
                            // Even if the mutex is poisoned, we can still use the guard
                            let guard = e.into_inner();
                            guard
                        }
                    };
                    state_guard.status = crate::transfer::TransferStatus::Completed;

                    // In a full implementation, we would calculate the hash of received files
                    // and send it back to the sender for verification
                    // For now, we'll just consider the transfer complete
                    info!(
                        "Transfer {} marked as completed (receiver side)",
                        payload.transfer_id
                    );
                }
            } else {
                warn!(
                    "Transfer {} not found in transfer manager",
                    payload.transfer_id
                );
            }
        }
        MessageType::TransferVerify => {
            // Handle transfer verify - send verification
            info!("Handling transfer verify from {}", conn.peer_id);

            // Deserialize the transfer verify payload
            let payload = message
                .payload_as::<crate::protocol::TransferCompletePayload>()
                .map_err(|e| anyhow::anyhow!(e))?;

            // Look up the transfer in our transfer manager
            let transfer_state_opt = {
                let transfer_manager_guard = match state.transfer_manager.lock() {
                    Ok(guard) => guard,
                    Err(e) => {
                        error!("Failed to lock transfer manager: {}", e);
                        // Even if the mutex is poisoned, we can still use the guard
                        let guard = e.into_inner();
                        guard
                    }
                };
                let active_transfers = &transfer_manager_guard.active_transfers;
                let transfers_guard = match active_transfers.lock() {
                    Ok(guard) => guard,
                    Err(e) => {
                        error!("Failed to lock active transfers: {}", e);
                        // Even if the mutex is poisoned, we can still use the guard
                        let guard = e.into_inner();
                        guard
                    }
                };
                transfers_guard.get(&payload.transfer_id).cloned()
            };

            if let Some(transfer_state) = transfer_state_opt {
                // Get transfer details
                let is_sender = {
                    let state_guard = match transfer_state.lock() {
                        Ok(guard) => guard,
                        Err(e) => {
                            error!("Failed to lock transfer state: {}", e);
                            // Even if the mutex is poisoned, we can still use the guard
                            let guard = e.into_inner();
                            guard
                        }
                    };
                    state_guard.direction == crate::transfer::TransferDirection::Sending
                };

                if is_sender {
                    // We are the sender, so we received verification from the receiver
                    info!(
                        "Received verification for transfer {} from receiver",
                        payload.transfer_id
                    );
                    // In a full implementation, we would compare the hash
                    // For now, we'll just consider the transfer verified
                    info!("Transfer {} verified", payload.transfer_id);
                } else {
                    // We are the receiver, so we sent verification
                    info!(
                        "We are the receiver for transfer {}, we sent verification",
                        payload.transfer_id
                    );
                    // Status should already be Completed
                }
            } else {
                warn!(
                    "Transfer {} not found in transfer manager",
                    payload.transfer_id
                );
            }
        }
        MessageType::TransferCancel => {
            // Handle transfer cancel
            info!("Handling transfer cancel from {}", conn.peer_id);

            // Deserialize the transfer cancel payload
            let payload = message
                .payload_as::<crate::protocol::TransferCompletePayload>()
                .map_err(|e| anyhow::anyhow!(e))?;

            // Look up the transfer in our transfer manager
            let transfer_state_opt = {
                let transfer_manager_guard = match state.transfer_manager.lock() {
                    Ok(guard) => guard,
                    Err(e) => {
                        error!("Failed to lock transfer manager: {}", e);
                        return Err(anyhow::anyhow!(e.to_string()));
                    }
                };
                let active_transfers = &transfer_manager_guard.active_transfers;
                let transfers_guard = match active_transfers.lock() {
                    Ok(guard) => guard,
                    Err(e) => {
                        error!("Failed to lock active transfers: {}", e);
                        // Even if the mutex is poisoned, we can still use the guard
                        let guard = e.into_inner();
                        guard
                    }
                };
                transfers_guard.get(&payload.transfer_id).cloned()
            };

            if let Some(transfer_state) = transfer_state_opt {
                info!(
                    "Cancelling transfer {} at peer's request",
                    payload.transfer_id
                );
                // Update transfer status to Cancelled
                let mut state_guard = match transfer_state.lock() {
                    Ok(guard) => guard,
                    Err(e) => {
                        error!("Failed to lock transfer state: {}", e);
                        // Even if the mutex is poisoned, we can still use the guard
                        let guard = e.into_inner();
                        guard
                    }
                };
                state_guard.status = crate::transfer::TransferStatus::Cancelled;
            } else {
                warn!(
                    "Transfer {} not found in transfer manager",
                    payload.transfer_id
                );
            }
        }
        MessageType::PairingRequest => {
            // Handle pairing request (for verification codes)
            info!("Handling pairing request from {}", conn.peer_id);

            // Deserialize the pairing payload
            let payload = message
                .payload_as::<crate::protocol::PairingPayload>()
                .map_err(|e| anyhow::anyhow!(e))?;

            // Look up our identity
            let identity_guard = match state.identity.lock() {
                Ok(guard) => guard,
                Err(e) => {
                    error!("Failed to lock identity state: {}", e);
                    // Even if the mutex is poisoned, we can still use the guard
                    let guard = e.into_inner();
                    guard
                }
            };
            let identity = identity_guard
                .as_ref()
                .expect("Identity should be initialized");

            info!(
                "Pairing request received: code {} from device {}",
                payload.verification_code, payload.device_id
            );

            // For now, automatically accept pairing requests
            // In a real implementation, we would show the verification code to the user
            // and wait for them to confirm or deny the pairing
            let response_payload = crate::protocol::PairingResponsePayload {
                device_id: identity.id.clone(),
                device_name: identity.name.clone(),
                verification_code: payload.verification_code,
                accepted: true, // Automatically accept for now
            };

            if let Some(stream) = &mut conn.stream {
                let response = BridgeMessage::new(MessageType::PairingResponse, &response_payload)
                    .map_err(|e| anyhow::anyhow!(e))?;
                let bytes = response.to_bytes();
                let _ = stream.write_all(&bytes);
            }
        }
        MessageType::PairingResponse => {
            // Handle pairing response
            info!("Handling pairing response from {}", conn.peer_id);

            // Deserialize the pairing payload
            let payload = message
                .payload_as::<crate::protocol::PairingPayload>()
                .map_err(|e| anyhow::anyhow!(e))?;

            // Update pairing state based on response
            // In a full implementation, we would update the verification code state
            // based on whether the pairing was accepted or rejected
            if let Ok(response_payload) =
                message.payload_as::<crate::protocol::PairingResponsePayload>()
            {
                info!(
                    "Pairing response received: accepted={} from device {} ({})",
                    response_payload.accepted,
                    response_payload.device_id,
                    response_payload.device_name
                );

                // If pairing was accepted, we could store the peer's identity for future use
                if response_payload.accepted {
                    info!(
                        "Pairing with {} ({}) accepted",
                        response_payload.device_id, response_payload.device_name
                    );
                } else {
                    info!(
                        "Pairing with {} ({}) rejected",
                        response_payload.device_id, response_payload.device_name
                    );
                }
            } else {
                warn!("Failed to parse pairing response payload");
            }
        }
        // Other message types would be handled here
        _ => {
            info!("Unhandled message type: {:?}", message.msg_type);
        }
    }

    Ok(())
}

// Start TCP listener for incoming connections
#[tauri::command]
pub fn start_network_listener(state: State<'_, NetworkState>) -> Result<(), String> {
    let mut running = state.running.lock().map_err(|e| e.to_string())?;
    if *running {
        return Ok(()); // Already running
    }

    *running = true;

    // Clone state for the listener thread
    let state_clone = NetworkState {
        listener: state.listener.clone(),
        connections: state.connections.clone(),
        running: state.running.clone(),
        port: state.port,
        identity: state.identity.clone(),
        transfer_manager: state.transfer_manager.clone(),
    };

    // Spawn listener thread
    std::thread::spawn(move || {
        let _ = network_listener_loop(state_clone);
    });

    Ok(())
}

// Stop TCP listener
#[tauri::command]
pub fn stop_network_listener(state: State<'_, NetworkState>) -> Result<(), String> {
    let mut running = state.running.lock().map_err(|e| e.to_string())?;
    *running = false;

    // Close listener socket
    if let Ok(mut listener) = state.listener.lock() {
        if let Some(listener_sock) = listener.take() {
            // Listener will be closed when dropped
            drop(listener_sock);
        }
    }

    // Close all connections
    if let Ok(connections) = state.connections.lock() {
        for (peer_id, conn) in connections.iter() {
            if let Ok(mut conn_guard) = conn.lock() {
                if let Some(stream) = &mut conn_guard.stream {
                    let _ = stream.shutdown(std::net::Shutdown::Both);
                }
            }
        }
    }

    Ok(())
}

// Get current connections
#[tauri::command]
pub fn get_connections(state: State<'_, NetworkState>) -> Result<String, String> {
    if let Ok(connections) = state.connections.lock() {
        let conn_info: Vec<String> = connections.keys().cloned().collect();
        Ok(serde_json::to_string(&conn_info).map_err(|e| e.to_string())?)
    } else {
        Err("Failed to lock connections".to_string())
    }
}

// Network listener loop
fn network_listener_loop(state: NetworkState) -> Result<()> {
    // Create TCP listener
    let listener = std::net::TcpListener::bind(("0.0.0.0", state.port))?;
    listener.set_nonblocking(true)?;

    // Store listener reference
    {
        let mut listener_guard = match state.listener.lock() {
            Ok(guard) => guard,
            Err(e) => {
                error!("Failed to lock listener: {}", e);
                // Even if the mutex is poisoned, we can still use the guard
                let guard = e.into_inner();
                guard
            }
        };
        *listener_guard = Some(Arc::new(listener));
    }

    // Keep a local reference for accepting connections
    let listener_ref = match state.listener.lock() {
        Ok(guard) => guard,
        Err(e) => {
            error!("Failed to lock listener: {}", e);
            return Err(anyhow::anyhow!("Failed to lock listener: {}", e));
        }
    };
    let listener_arc = listener_ref.as_ref().unwrap().clone();

    info!("Network listener started on port {}", state.port);

    loop {
        let running = match state.running.lock() {
            Ok(guard) => *guard,
            Err(e) => {
                error!("Failed to lock running state: {}", e);
                break; // Exit the loop if we can't lock the running state
            }
        };
        if !running {
            break;
        }
        // Accept incoming connections
        match listener_arc.accept() {
            Ok((stream, addr)) => {
                info!("Accepted incoming connection from {}", addr);

                // Set stream to non-blocking
                stream.set_nonblocking(true)?;

                // Get peer ID (for now, we'll use address as peer ID, later we'll exchange identities)
                let peer_id = format!("{}:{}", addr.ip(), addr.port());

                // Create connection
                let connection = Connection {
                    peer_id: peer_id.clone(),
                    stream: Some(stream),
                    secure_channel: None,
                };

                // Store connection
                let peer_id_clone = peer_id.clone();
                if let Ok(mut connections) = state.connections.lock() {
                    connections.insert(peer_id, Arc::new(Mutex::new(connection)));
                }

                // Spawn thread to handle this connection
                let state_clone = NetworkState {
                    listener: state.listener.clone(),
                    connections: state.connections.clone(),
                    running: state.running.clone(),
                    port: state.port,
                    identity: state.identity.clone(),
                    transfer_manager: state.transfer_manager.clone(),
                };

                std::thread::spawn(move || {
                    let _ = connection_handler_loop(state_clone, peer_id_clone);
                });
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // No incoming connection, continue loop
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(e) => {
                error!("Error accepting connection: {}", e);
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        }

        // Check existing connections for data
        // In this implementation, each connection gets its own handler thread when accepted
        // so we don't need to periodically check existing connections

        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    Ok(())
}

// Connection handler loop
fn connection_handler_loop(state: NetworkState, peer_id: String) -> Result<()> {
    // Get connection
    let connection = {
        if let Ok(connections) = state.connections.lock() {
            if let Some(conn) = connections.get(&peer_id) {
                conn.clone()
            } else {
                // Connection no longer exists
                return Ok(());
            }
        } else {
            return Ok(());
        }
    };

    info!("Starting handler for connection {}", peer_id);

    // Buffer for incoming data
    let mut buf = [0u8; 4096];

    loop {
        // Check if we should still be running
        let running = match state.running.lock() {
            Ok(guard) => *guard,
            Err(e) => {
                error!("Failed to lock running state: {}", e);
                break; // Exit loop if we can't lock running state
            }
        };
        if !running {
            break;
        }

        // Get connection stream
        let mut stream_option = None;
        if let Ok(conn_guard) = connection.lock() {
            stream_option = conn_guard
                .stream
                .as_ref()
                .map(|s| s.try_clone().ok())
                .flatten();
        }

        let mut stream = match stream_option {
            Some(s) => s,
            None => {
                // Stream is gone, exit
                break;
            }
        };

        // Try to read data
        match stream.read(&mut buf) {
            Ok(0) => {
                // Connection closed
                info!("Connection {} closed by peer", peer_id);
                break;
            }
            Ok(size) => {
                // Process received data
                let message_bytes = &buf[..size];

                // Try to parse as a BridgeMessage
                if let Ok(message) = BridgeMessage::from_bytes(message_bytes) {
                    // Get identity state for message handling
                    let identity_state = state.identity.clone();

                    // Handle the message
                    let mut conn_guard = match connection.lock() {
                        Ok(g) => g,
                        Err(e) => {
                            error!("Failed to lock connection: {}", e);
                            // Even if the mutex is poisoned, we can still use the guard
                            let guard = e.into_inner();
                            guard
                        }
                    };
                    let _ =
                        handle_bridge_message(&mut *conn_guard, message, identity_state, &state);
                } else {
                    warn!("Failed to parse message from connection {}", peer_id);
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // No data available, sleep briefly
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(e) => {
                error!("Error reading from connection {}: {}", peer_id, e);
                break;
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    // Clean up connection
    if let Ok(mut connections) = state.connections.lock() {
        connections.remove(&peer_id);
    }

    info!("Connection handler for {} exited", peer_id);

    Ok(())
}

// Send file transfer logic
fn send_file_transfer(
    state: NetworkState,
    secure_channel: SecureChannel,
    transfer_id: &str,
    peer_id: &str,
) -> Result<()> {
    info!(
        "Starting file transfer for {} to peer {}",
        transfer_id, peer_id
    );

    // Get the transfer from transfer manager
    let transfer_state = {
        let transfer_manager_guard = match state.transfer_manager.lock() {
            Ok(guard) => guard,
            Err(e) => {
                error!("Failed to lock transfer manager: {}", e);
                // Even if the mutex is poisoned, we can still use the guard
                let guard = e.into_inner();
                return Err(anyhow::anyhow!("Failed to lock transfer manager: {}", guard));
            }
        };
        let active_transfers = &transfer_manager_guard.active_transfers;
        let transfers_guard = match active_transfers.lock() {
            Ok(guard) => guard,
            Err(e) => {
                error!("Failed to lock active transfers: {}", e);
                // Even if the mutex is poisoned, we can still use the guard
                let guard = e.into_inner();
                guard
            }
        };
        if let Some(transfer) = transfers_guard.get(transfer_id) {
            transfer.clone()
        } else {
            error!("Transfer {} not found", transfer_id);
            return Ok(());
        }
    };

    // Get transfer details
    let (files, total_size) = {
        let state_guard = match transfer_state.lock() {
            Ok(guard) => guard,
            Err(e) => {
                error!("Failed to lock transfer state: {}", e);
                // Even if the mutex is poisoned, we can still use the guard
                let guard = e.into_inner();
                guard
            }
        };
        let files_clone = state_guard.files.clone();
        let total_size = state_guard.total_size;
        drop(state_guard);
        (files_clone, total_size)
    };

    // Update transfer status to Transferring
    {
        let mut state_guard = match transfer_state.lock() {
            Ok(guard) => guard,
            Err(e) => {
                error!("Failed to lock transfer state: {}", e);
                // Even if the mutex is poisoned, we can still use the guard
                let guard = e.into_inner();
                guard
            }
        };
        state_guard.status = crate::transfer::TransferStatus::Transferring;
    }

    // Use the secure channel that was passed in
    let channel = secure_channel;

    // Calculate total bytes sent
    let mut total_sent: u64 = 0;

    // Process each file
    for file_info in files.iter() {
        info!(
            "Sending file: {} ({} bytes)",
            file_info.name, file_info.size
        );

        // Open file for reading
        let file_path = std::path::Path::new(&file_info.name);
        let mut file = match std::fs::File::open(file_path) {
            Ok(f) => f,
            Err(e) => {
                error!("Failed to open file {}: {}", file_info.name, e);
                // Update transfer status to Failed
                let mut state_guard = match transfer_state.lock() {
                    Ok(guard) => guard,
                    Err(e) => {
                        error!("Failed to lock transfer state: {}", e);
                        // Even if the mutex is poisoned, we can still use the guard
                        let guard = e.into_inner();
                        guard
                    }
                };
                state_guard.status = crate::transfer::TransferStatus::Failed;
                return Ok(());
            }
        };

        // Read and send file in chunks
        let mut buffer = [0u8; 32768]; // 32KB chunks
        let mut bytes_read = 0;

        loop {
            // Check if we should still be running
            let running = match state.running.lock() {
                Ok(guard) => *guard,
                Err(e) => {
                    error!("Failed to lock running state: {}", e);
                    // Even if the mutex is poisoned, we can still use the guard
                    let guard = e.into_inner();
                    *guard
                }
            };
            if !running {
                info!("Transfer cancelled for {}", transfer_id);
                let mut state_guard = match transfer_state.lock() {
                    Ok(guard) => guard,
                    Err(e) => {
                        error!("Failed to lock transfer state: {}", e);
                        // Even if the mutex is poisoned, we can still use the guard
                        let guard = e.into_inner();
                        guard
                    }
                };
                state_guard.status = crate::transfer::TransferStatus::Cancelled;
                return Ok(());
            }

            // Read chunk from file
            match file.read(&mut buffer) {
                Ok(0) => {
                    // End of file
                    break;
                }
                Ok(n) => {
                    bytes_read = n;
                    // Encrypt the data
                    let key_array: [u8; 32] = channel
                        .init_key
                        .clone()
                        .try_into()
                        .map_err(|_| anyhow::anyhow!("Invalid key length"))?;
                    let encrypted_data = crate::security::encrypt_data(&buffer[..n], &key_array)?;

                    // Create TransferData payload
                    let transfer_data = crate::protocol::TransferDataPayload {
                        transfer_id: transfer_id.to_string(),
                        chunk_index: (total_sent / 32768) as u32,
                        data: encrypted_data,
                        is_last: false, // Will be set to true for the last chunk of the last file
                    };

                    // Create and send TransferData message
                    let message = BridgeMessage::new(MessageType::TransferData, &transfer_data)
                        .map_err(|e| anyhow::anyhow!(e))?;

                    // Send to peer
                    if let Err(e) = send_message_to_peer(&state, peer_id, &message) {
                        error!("Failed to send transfer data to {}: {}", peer_id, e);
                        // Update transfer status to Failed
                        let mut state_guard = match transfer_state.lock() {
                            Ok(guard) => guard,
                            Err(e) => {
                                error!("Failed to lock transfer state: {}", e);
                                // Even if the mutex is poisoned, we can still use the guard
                                let guard = e.into_inner();
                                guard
                            }
                        };
                        state_guard.status = crate::transfer::TransferStatus::Failed;
                        return Ok(());
                    }

                    // Update progress
                    total_sent += n as u64;
                    {
                        let mut state_guard = match transfer_state.lock() {
                            Ok(guard) => guard,
                            Err(e) => {
                                error!("Failed to lock transfer state: {}", e);
                                // Even if the mutex is poisoned, we can still use the guard
                                let guard = e.into_inner();
                                guard
                            }
                        };
                        state_guard.transferred_size = total_sent;
                    }
                }
                Err(e) => {
                    error!("Failed to read from file {}: {}", file_info.name, e);
                    // Update transfer status to Failed
                    let mut state_guard = match transfer_state.lock() {
                        Ok(guard) => guard,
                        Err(e) => {
                            error!("Failed to lock transfer state: {}", e);
                            // Even if the mutex is poisoned, we can still use the guard
                            let guard = e.into_inner();
                            guard
                        }
                    };
                    state_guard.status = crate::transfer::TransferStatus::Failed;
                    return Ok(());
                }
            }
        }
    }

    // Send final chunk with is_last = true
    // We'll send an empty chunk with is_last = true to signal completion
    let transfer_data = crate::protocol::TransferDataPayload {
        transfer_id: transfer_id.to_string(),
        chunk_index: (total_sent / 32768) as u32,
        data: Vec::new(), // Empty data for final chunk
        is_last: true,
    };

    let message = BridgeMessage::new(MessageType::TransferData, &transfer_data)
        .map_err(|e| anyhow::anyhow!(e))?;

    if let Err(e) = send_message_to_peer(&state, peer_id, &message) {
        error!("Failed to send final transfer data to {}: {}", peer_id, e);
        // Update transfer status to Failed
        let mut state_guard = match transfer_state.lock() {
            Ok(guard) => guard,
            Err(e) => {
                error!("Failed to lock transfer state: {}", e);
                // Even if the mutex is poisoned, we can still use the guard
                let guard = e.into_inner();
                guard
            }
        };
        state_guard.status = crate::transfer::TransferStatus::Failed;
        return Ok(());
    }

    // Send TransferComplete message
    let transfer_complete = crate::protocol::TransferCompletePayload {
        transfer_id: transfer_id.to_string(),
        success: true,
        hash: None, // In a full implementation, we would calculate and include the hash
    };

    let message = BridgeMessage::new(MessageType::TransferComplete, &transfer_complete)
        .map_err(|e| anyhow::anyhow!(e))?;

    if let Err(e) = send_message_to_peer(&state, peer_id, &message) {
        error!("Failed to send transfer complete to {}: {}", peer_id, e);
        // Update transfer status to Failed
        let mut state_guard = match transfer_state.lock() {
            Ok(guard) => guard,
            Err(e) => {
                error!("Failed to lock transfer state: {}", e);
                // Even if the mutex is poisoned, we can still use the guard
                let guard = e.into_inner();
                guard
            }
        };
        state_guard.status = crate::transfer::TransferStatus::Failed;
        return Ok(());
    }

    // Update transfer status to Completed
    {
        let mut state_guard = match transfer_state.lock() {
            Ok(guard) => guard,
            Err(e) => {
                error!("Failed to lock transfer state: {}", e);
                // Even if the mutex is poisoned, we can still use the guard
                let guard = e.into_inner();
                guard
            }
        };
        state_guard.status = crate::transfer::TransferStatus::Completed;
    }

    info!(
        "File transfer completed for {} to peer {}",
        transfer_id, peer_id
    );

    Ok(())
}
pub fn send_message_to_peer(
    state: &NetworkState,
    peer_id: &str,
    message: &BridgeMessage,
) -> Result<(), String> {
    if let Ok(connections) = state.connections.lock() {
        if let Some(connection) = connections.get(peer_id) {
            if let Ok(mut conn_guard) = connection.lock() {
                if let Some(stream) = &mut conn_guard.stream {
                    let bytes = message.to_bytes();
                    stream.write_all(&bytes).map_err(|e| e.to_string())?;
                    return Ok(());
                }
            }
        }
    }
    Err(format!("Peer not found or not connected: {}", peer_id))
}

// Send a message to all connected peers
pub fn broadcast_message(state: &NetworkState, message: &BridgeMessage) -> Result<(), String> {
    if let Ok(connections) = state.connections.lock() {
        for (peer_id, connection) in connections.iter() {
            if let Ok(mut conn_guard) = connection.lock() {
                if let Some(stream) = &mut conn_guard.stream {
                    let bytes = message.to_bytes();
                    let _ = stream.write_all(&bytes);
                }
            }
        }
    }
    Ok(())
}
