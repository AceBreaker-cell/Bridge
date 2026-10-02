#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            // Initialize identity
            let app_dir = app
                .path()
                .app_data_dir()
                .expect("failed to get app data directory");
            std::fs::create_dir_all(&app_dir)?;

            // Get or create device identity
            let identity = crate::identity::DeviceIdentity::load_or_create(
                &app_dir,
                hostname::get()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
            )?;

            // Store identity in state
            let identity_state = crate::identity::IdentityState(std::sync::Arc::new(
                std::sync::Mutex::new(Some(identity)),
            ));
            app.manage(identity_state);

            // Initialize discovery
            let identity_state = app.state::<crate::identity::IdentityState>();
            let discovery_state =
                crate::discovery::init_discovery_state(42137, identity_state.0.clone());
            app.manage(discovery_state);

            // Initialize network
            let identity_state = app.state::<crate::identity::IdentityState>();
            let transfer_manager = app.state::<crate::transfer::TransferManager>();
            let network_state = crate::network::init_network_state(
                42138,
                identity_state.0.clone(),
                transfer_manager.0.clone(),
            );
            app.manage(network_state);

            // Initialize transfer manager
            let transfer_manager = crate::transfer::init_transfer_manager();
            app.manage(transfer_manager);

            // Initialize database
            let database = crate::database::init_database()?;
            app.manage(database);

            // Initialize diagnostics
            let diagnostics_state = crate::diagnostics::init_diagnostics_state(42137);
            app.manage(diagnostics_state);

            // Initialize verification code state
            let verification_code_state =
                crate::security::VerificationCodeState(Arc::new(Mutex::new(None)));
            app.manage(verification_code_state);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Identity commands
            crate::identity::get_device_identity,
            crate::identity::set_device_name,
            // Discovery commands
            crate::discovery::start_discovery,
            crate::discovery::stop_discovery,
            crate::discovery::get_discovered_devices,
            // Network commands
            crate::network::start_network_listener,
            crate::network::stop_network_listener,
            crate::network::get_connections,
            // Transfer commands
            crate::transfer::request_file_transfer,
            crate::transfer::get_transfer_status,
            crate::transfer::cancel_transfer,
            crate::transfer::accept_transfer,
            crate::transfer::reject_transfer,
            // Database commands
            crate::database::get_transfer_history,
            crate::database::add_transfer_record,
            crate::database::get_trusted_devices,
            crate::database::set_setting,
            crate::database::get_setting,
            // Diagnostics commands
            crate::diagnostics::get_diagnostics,
            // Security commands
            crate::security::generate_verification_code,
            crate::security::verify_code,
        ])
        .run(tauri::generate_context!())
        .expect("error while building tauri application");
}

// Declare modules
mod database;
mod diagnostics;
mod discovery;
mod identity;
mod network;
mod protocol;
mod security;
mod transfer;

// Add necessary imports
use hostname::get;
use std::fs;
use std::sync::{Arc, Mutex};
use tauri::Manager;
use uuid::Uuid;
// Logging is handled by tauri-plugin-log
