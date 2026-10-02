use aes_gcm::aead::AeadInPlace;
use aes_gcm::Aes256Gcm;
use aes_gcm::KeyInit;
use anyhow::Result;
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use generic_array::GenericArray;
use log::{info, warn};
use rand::SecureRandom;
use ring::{agreement, digest, hkdf, hmac, rand};
use serde::{Deserialize, Serialize};
use std::convert::TryFrom;
use std::fmt;
use std::sync::{Arc, Mutex};
use tauri::State;
use typenum::U16;
use x25519_dalek::PublicKey;

#[derive(Clone)]
pub struct SecureChannel {
    pub init_key: Vec<u8>,
    pub response_key: Vec<u8>,
    pub init_nonce: Vec<u8>,
    pub response_nonce: Vec<u8>,
    pub init_mac_key: Vec<u8>,
    pub response_mac_key: Vec<u8>,
    pub sequence_num: u64,
}

impl std::fmt::Debug for SecureChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecureChannel")
            .field("init_key", &"<hidden>")
            .field("response_key", &"<hidden>")
            .field("init_nonce", &"<hidden>")
            .field("response_nonce", &"<hidden>")
            .field("init_mac_key", &"<hidden>")
            .field("response_mac_key", &"<hidden>")
            .field("sequence_num", &self.sequence_num)
            .finish()
    }
}

impl SecureChannel {
    pub fn new(
        our_secret_param: &[u8; 32],
        their_public: &PublicKey,
        our_nonce: &[u8; 32],
        their_nonce: &[u8; 32],
    ) -> Result<Self> {
        // X25519 key exchange to establish secure channel
        // Perform X25519 key exchange using ring
        let our_private_key = agreement::EphemeralPrivateKey::generate(
            &agreement::X25519,
            &rand::SystemRandom::new(),
        )
        .map_err(|e| anyhow::anyhow!("Failed to generate ephemeral private key: {}", e))?;
        let our_public_key = our_private_key.compute_public_key();
        let their_public_key =
            agreement::UnparsedPublicKey::new(&agreement::X25519, their_public.as_ref());
        let mut shared_secret = [0u8; 32];
        agreement::agree_ephemeral(&our_private_key, &their_public_key, |shared_secret_bytes| {
            shared_secret.copy_from_slice(shared_secret_bytes);
            // Return a dummy value - the return value is ignored by agree_ephemeral
            0u8
        })
        .map_err(|e| anyhow::anyhow!("X25519 key agreement failed: {}", e))?;

        // Extract shared secret bytes
        let shared_secret_bytes = shared_secret;

        // Create HKFD to derive keys
        let salt = b"BRIDGE_SECURE_CHANNEL_V1";
        let ik = hkdf::Prk::new_less_safe(hkdf::HKDF_SHA256, &shared_secret_bytes);

        // Derive encryption keys (for AES-GCM)
        let mut input = Vec::new();
        input.extend_from_slice(our_nonce);
        input.extend_from_slice(their_nonce);
        struct EmptyKeyType;
        impl hkdf::KeyType for EmptyKeyType {
            fn len(&self) -> usize {
                0
            }
        };
        // Expand HKDF to generate keying material (for encryption keys and nonces)
        struct EmptyKeyType1;
        impl hkdf::KeyType for EmptyKeyType1 {
            fn len(&self) -> usize {
                0
            }
        };
        let input_slice = input.as_slice();
        let input_refs = [input_slice];
        let okm_result = ik
            .expand(&input_refs, EmptyKeyType1)
            .map_err(|e| anyhow::anyhow!("HKDF expand failed: {}", e))?;
        // Copy the keying material into our fixed-size array
        let mut okm_array = [0u8; 64];
        okm_result.fill(&mut okm_array);

        // Split the OKM into key material and nonce material
        let (key_material, nonce_material) = okm_array.split_at(32);

        // Split key material into init and response keys (16 bytes each for AES-128, but we'll use 32)
        // Actually, let's derive proper 32-byte keys for AES-256
        let mut init_key = [0u8; 32];
        let mut response_key = [0u8; 32];
        init_key[..16].copy_from_slice(&key_material[..16]);
        init_key[16..].copy_from_slice(&nonce_material[..16]);
        response_key[..16].copy_from_slice(&key_material[16..]);
        response_key[16..].copy_from_slice(&nonce_material[16..]);

        // Extract nonces (12 bytes each for AES-GCM)
        let mut init_nonce = [0u8; 12];
        let mut response_nonce = [0u8; 12];
        init_nonce.copy_from_slice(&nonce_material[32..44]);
        response_nonce.copy_from_slice(&nonce_material[44..56]);

        // Derive MAC keys (using HKDF again or split from remaining material)
        let mut mac_input = Vec::new();
        mac_input.extend_from_slice(our_nonce);
        mac_input.extend_from_slice(their_nonce);
        mac_input.extend_from_slice(b"MAC");

        struct EmptyKeyType2;
        impl hkdf::KeyType for EmptyKeyType2 {
            fn len(&self) -> usize {
                0
            }
        };
        // Expand HKDF to generate MAC keying material (for MAC keys)
        struct EmptyKeyType3;
        impl hkdf::KeyType for EmptyKeyType3 {
            fn len(&self) -> usize {
                0
            }
        };
        let mac_input_slice = mac_input.as_slice();
        let mac_input_refs = [mac_input_slice];
        let okm2_result = ik
            .expand(&mac_input_refs, EmptyKeyType3)
            .map_err(|e| anyhow::anyhow!("HKDF expand for MAC failed: {}", e))?;
        // Copy the keying material into our fixed-size array
        let mut okm2_array = [0u8; 64];
        okm2_result.fill(&mut okm2_array);

        let (init_mac_key, response_mac_key) = okm2_array.split_at(32);

        Ok(Self {
            init_key: init_key.to_vec(),
            response_key: response_key.to_vec(),
            init_nonce: init_nonce.to_vec(),
            response_nonce: response_nonce.to_vec(),
            init_mac_key: init_mac_key.to_vec(),
            response_mac_key: response_mac_key.to_vec(),
            sequence_num: 0,
        })
    }

    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>> {
        // Use AES-GCM for encryption
        let key = <[u8; 32]>::try_from(self.init_key.as_slice())
            .map_err(|_| anyhow::anyhow!("Invalid key length"))?;
        let nonce = <[u8; 12]>::try_from(self.init_nonce.as_slice())
            .map_err(|_| anyhow::anyhow!("Invalid nonce length"))?;

        let mut aead = aes_gcm::Aes256Gcm::new_from_slice(&key)
            .map_err(|e| anyhow::anyhow!("Failed to create AES-GCM instance: {}", e))?;

        let mut ciphertext = plaintext.to_vec();
        let mut tag = [0u8; 16]; // AES-GCM tag is 16 bytes
        aead.encrypt_in_place_detached(&nonce.into(), &mut ciphertext, &mut tag)
            .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;

        // Append the tag to the ciphertext
        ciphertext.extend_from_slice(&tag);
        self.sequence_num += 1;
        Ok(ciphertext)
    }

    pub fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>> {
        // Use AES-GCM for decryption
        if ciphertext.len() < 16 {
            return Err(anyhow::anyhow!("Ciphertext too short for AES-GCM"));
        }

        let key = <[u8; 32]>::try_from(self.response_key.as_slice())
            .map_err(|_| anyhow::anyhow!("Invalid key length"))?;
        let nonce = <[u8; 12]>::try_from(self.response_nonce.as_slice())
            .map_err(|_| anyhow::anyhow!("Invalid nonce length"))?;

        // Split ciphertext and tag
        let (ciphertext_body, tag_bytes) = ciphertext.split_at(ciphertext.len() - 16);
        let tag =
            <[u8; 16]>::try_from(tag_bytes).map_err(|_| anyhow::anyhow!("Invalid tag length"))?;

        let mut aead = aes_gcm::Aes256Gcm::new_from_slice(&key)
            .map_err(|e| anyhow::anyhow!("Failed to create AES-GCM instance: {}", e))?;

        let mut plaintext = ciphertext_body.to_vec();
        let mut tag = generic_array::GenericArray::<u8, typenum::U16>::from_slice(tag_bytes);
        let mut aad = Vec::new();
        aead.decrypt_in_place_detached(&nonce.into(), &mut plaintext, &mut aad, &mut tag)
            .map_err(|e| anyhow::anyhow!("Decryption failed: {}", e))?;

        Ok(plaintext)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationCode {
    pub code: u32,
}

impl VerificationCode {
    pub fn generate() -> Self {
        let mut rng = rand::SystemRandom::new();
        let mut bytes = [0u8; 4];
        rng.fill(&mut bytes).ok();
        let code = u32::from_be_bytes(bytes) % 900000 + 100000; // 6-digit code
        Self { code }
    }

    pub fn validate(&self, input: u32) -> bool {
        self.code == input
    }
}

// State type for storing the current verification code
pub struct VerificationCodeState(pub Arc<Mutex<Option<VerificationCode>>>);

// Password-based key derivation for encrypting private keys at rest
pub fn derive_key_from_password(password: &str, salt: &[u8]) -> Result<[u8; 32]> {
    let mut key = [0u8; 32];
    ring::pbkdf2::derive(
        ring::pbkdf2::PBKDF2_HMAC_SHA256,
        std::num::NonZero::new(100_000).unwrap(), // iterations
        salt,
        password.as_bytes(),
        &mut key,
    );
    Ok(key)
}

// Simple symmetric encryption for local storage (would use proper AES-GCM in production)
pub fn encrypt_data(data: &[u8], key: &[u8; 32]) -> Result<Vec<u8>> {
    // In production, use AES-GCM or ChaCha20-Poly1305
    // For now, simple XOR (NOT SECURE - just for structure demonstration)
    let mut encrypted = data.to_vec();
    for (i, byte) in encrypted.iter_mut().enumerate() {
        *byte ^= key[i % key.len()];
    }
    Ok(encrypted)
}

pub fn decrypt_data(data: &[u8], key: &[u8; 32]) -> Result<Vec<u8>> {
    // Same as encrypt for XOR
    encrypt_data(data, key)
}

#[tauri::command]
pub fn generate_verification_code(state: State<'_, VerificationCodeState>) -> Result<u32, String> {
    let mut code_state = state.0.lock().map_err(|e| e.to_string())?;
    let verification_code = VerificationCode::generate();
    let code = verification_code.code;
    *code_state = Some(verification_code);
    Ok(code)
}

#[tauri::command]
pub fn verify_code(
    state: State<'_, VerificationCodeState>,
    input_code: u32,
) -> Result<bool, String> {
    let code_state = state.0.lock().map_err(|e| e.to_string())?;
    if let Some(ref verification_code) = *code_state {
        Ok(verification_code.validate(input_code))
    } else {
        Err("No verification code generated".to_string())
    }
}
