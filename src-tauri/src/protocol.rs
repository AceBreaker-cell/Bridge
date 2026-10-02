use serde::{Deserialize, Serialize};
use std::convert::TryFrom;
use std::fmt;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MessageType {
    Discovery = 1,
    HandshakeRequest = 2,
    HandshakeResponse = 3,
    PairingRequest = 4,
    PairingResponse = 5,
    TransferRequest = 6,
    TransferAccept = 7,
    TransferReject = 8,
    TransferData = 9,
    TransferCancel = 10,
    TransferComplete = 11,
    TransferVerify = 12,
    Ping = 13,
    Pong = 14,
    Error = 15,
}

impl TryFrom<u32> for MessageType {
    type Error = String;

    fn try_from(value: u32) -> Result<Self, <Self as TryFrom<u32>>::Error> {
        match value {
            1 => Ok(MessageType::Discovery),
            2 => Ok(MessageType::HandshakeRequest),
            3 => Ok(MessageType::HandshakeResponse),
            4 => Ok(MessageType::PairingRequest),
            5 => Ok(MessageType::PairingResponse),
            6 => Ok(MessageType::TransferRequest),
            7 => Ok(MessageType::TransferAccept),
            8 => Ok(MessageType::TransferReject),
            9 => Ok(MessageType::TransferData),
            10 => Ok(MessageType::TransferCancel),
            11 => Ok(MessageType::TransferComplete),
            12 => Ok(MessageType::TransferVerify),
            13 => Ok(MessageType::Ping),
            14 => Ok(MessageType::Pong),
            15 => Ok(MessageType::Error),
            _ => Err(format!("Unknown message type: {}", value)),
        }
    }
}

impl fmt::Display for MessageType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            MessageType::Discovery => "Discovery",
            MessageType::HandshakeRequest => "HandshakeRequest",
            MessageType::HandshakeResponse => "HandshakeResponse",
            MessageType::PairingRequest => "PairingRequest",
            MessageType::PairingResponse => "PairingResponse",
            MessageType::TransferRequest => "TransferRequest",
            MessageType::TransferAccept => "TransferAccept",
            MessageType::TransferReject => "TransferReject",
            MessageType::TransferData => "TransferData",
            MessageType::TransferCancel => "TransferCancel",
            MessageType::TransferComplete => "TransferComplete",
            MessageType::TransferVerify => "TransferVerify",
            MessageType::Ping => "Ping",
            MessageType::Pong => "Pong",
            MessageType::Error => "Error",
        };
        write!(f, "{}", s)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeMessage {
    pub msg_type: MessageType,
    pub version: u32,
    pub payload: Vec<u8>,
}

impl BridgeMessage {
    pub fn new<T: Serialize>(msg_type: MessageType, payload: &T) -> Result<Self, String> {
        let payload_bytes = serde_json::to_string(payload)
            .map_err(|e| e.to_string())?
            .into_bytes();

        Ok(Self {
            msg_type,
            version: 1,
            payload: payload_bytes,
        })
    }

    pub fn payload_as<T: for<'de> Deserialize<'de>>(&self) -> Result<T, String> {
        serde_json::from_slice(&self.payload).map_err(|e| e.to_string())
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let header = format!("{:02}{:08}", self.msg_type.clone() as u32, self.version);
        let mut bytes = header.into_bytes();
        bytes.extend(self.payload.clone());
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < 10 {
            return Err("Message too short".to_string());
        }

        let header = String::from_utf8_lossy(&bytes[..10]);
        let msg_type_val = u32::from_str_radix(&header[0..2], 16)
            .map_err(|_| "Invalid message type".to_string())?;
        let version =
            u32::from_str_radix(&header[2..10], 16).map_err(|_| "Invalid version".to_string())?;

        let msg_type = MessageType::try_from(msg_type_val).map_err(|e| e)?;

        let payload = bytes[10..].to_vec();

        Ok(Self {
            msg_type,
            version,
            payload,
        })
    }
}

// Specific message payloads

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakePayload {
    pub device_id: String,
    pub public_key: Vec<u8>,
    pub nonce: Vec<u8>, // For preventing replay attacks
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingPayload {
    pub device_id: String,
    pub device_name: String,
    pub verification_code: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferRequestPayload {
    pub transfer_id: String,
    pub sender_id: String,
    pub files: Vec<FileInfo>,
    pub total_size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub name: String,
    pub size: u64,
    pub is_directory: bool,
    pub path: String, // Relative path within transfer
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferDataPayload {
    pub transfer_id: String,
    pub chunk_index: u32,
    pub data: Vec<u8>,
    pub is_last: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferCompletePayload {
    pub transfer_id: String,
    pub success: bool,
    pub hash: Option<String>, // SHA-256 hash of received data
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferAcceptPayload {
    pub transfer_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferRejectPayload {
    pub transfer_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingResponsePayload {
    pub device_id: String,
    pub device_name: String,
    pub verification_code: u32,
    pub accepted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorPayload {
    pub message: String,
    pub code: u32,
}
