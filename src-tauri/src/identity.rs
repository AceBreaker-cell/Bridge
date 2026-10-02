use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::{Manager, State};
use uuid::Uuid;
// Logging is handled by tauri-plugin-log
use rand::rngs::OsRng;
use ring::agreement::{EphemeralPrivateKey, PublicKey as AgreementPublicKey, UnparsedPublicKey, X25519};
use x25519_dalek::{PublicKey as X25519PublicKey};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceIdentity {
    pub id: String,
    pub name: String,
    pub public_key: Vec<u8>,
    pub private_key: Vec<u8>,
}

impl DeviceIdentity {
    pub fn new(name: String) -> Result<Self> {
        // Generate a new key pair for the device
        let our_private_key = EphemeralPrivateKey::generate(
            &agreement::X25519,
            &OsRng::new(),
        );
        let our_public_key = our_private_key.compute_public_key();

        Ok(Self {
            id: Uuid::new_v4().to_string(),
            name,
            public_key: our_public_key.as_ref().to_vec(),
            private_key: our_private_key.to_bytes().to_vec(),
        })
    }

    pub fn load_or_create<P: AsRef<Path>>(path: P, name: String) -> Result<Self> {
        let identity_path = path.as_ref().join("identity.json");

        if let Ok(contents) = fs::read_to_string(&identity_path) {
            // Load existing identity
            let identity: DeviceIdentity = serde_json::from_str(&contents)?;
            Ok(identity)
        } else {
            // Create new identity
            let identity = DeviceIdentity::new(name)?;
            // Save it
            fs::create_dir_all(path.as_ref())?;
            let json = serde_json::to_string_pretty(&identity)?;
            fs::write(identity_path, json)?;
            Ok(identity)
        }
    }
}

// State type for Tauri
pub struct IdentityState(pub Arc<Mutex<Option<DeviceIdentity>>>);

#[tauri::command]
pub fn get_device_identity(state: State<'_, IdentityState>) -> Result<String, String> {
    let identity = state.0.lock().map_err(|e| e.to_string())?;
    match identity.as_ref() {
        Some(id) => Ok(serde_json::to_string(id).map_err(|e| e.to_string())?),
        None => Err("Identity not initialized".to_string()),
    }
}

#[tauri::command]
pub fn set_device_name(state: State<'_, IdentityState>, name: String) -> Result<(), String> {
    let mut identity = state.0.lock().map_err(|e| e.to_string())?;
    if let Some(ref mut id) = *identity {
        id.name = name;
        // Save to disk would go here
        Ok(())
    } else {
        Err("Identity not initialized".to_string())
    }
}
