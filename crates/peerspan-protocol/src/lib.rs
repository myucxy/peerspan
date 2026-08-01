//! Versioned control-plane messages shared by PeerSpan peers.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const PROTOCOL_VERSION: u16 = 3;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum ControlMessage {
    Hello(Hello),
    PairingRequest(PairingRequest),
    PairingDecision(PairingDecision),
    DisplayOffer(DisplayOffer),
    DisplayDecision(DisplayDecision),
    SessionEnd(SessionEnd),
    Input(InputEvent),
    ClipboardText(ClipboardText),
    Heartbeat(Heartbeat),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hello {
    pub protocol_version: u16,
    pub device_id: Uuid,
    pub device_name: String,
    pub fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PairingRequest {
    pub request_id: Uuid,
    pub device_id: Uuid,
    pub short_code_commitment: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PairingDecision {
    pub request_id: Uuid,
    pub accepted: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplayOffer {
    pub session_id: Uuid,
    pub width_px: u32,
    pub height_px: u32,
    pub refresh_hz: u16,
    pub dpi_x: u16,
    pub dpi_y: u16,
    pub rotation_degrees: u16,
    pub codec: VideoCodec,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplayDecision {
    pub session_id: Uuid,
    pub accepted: bool,
    pub reason: Option<String>,
    pub media_port: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionEnd {
    pub session_id: Uuid,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoCodec {
    H264,
    Hevc,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InputEvent {
    PointerMove {
        normalized_x: f32,
        normalized_y: f32,
    },
    PointerButton {
        button: PointerButton,
        pressed: bool,
    },
    Wheel {
        delta_x: i16,
        delta_y: i16,
    },
    Key {
        scan_code: u16,
        pressed: bool,
        extended: bool,
    },
    ReleaseAll,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PointerButton {
    Left,
    Right,
    Middle,
    Back,
    Forward,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClipboardText {
    pub revision: u64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Heartbeat {
    pub sequence: u64,
    pub monotonic_millis: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_message_round_trips_as_tagged_json() {
        let message = ControlMessage::Input(InputEvent::PointerMove {
            normalized_x: 0.25,
            normalized_y: 0.75,
        });

        let json = serde_json::to_string(&message).expect("message should serialize");
        let decoded: ControlMessage =
            serde_json::from_str(&json).expect("message should deserialize");

        assert_eq!(decoded, message);
        assert!(json.contains("pointer_move"));
    }

    #[test]
    fn display_decision_round_trips_with_session_identity() {
        let session_id = Uuid::new_v4();
        let message = ControlMessage::DisplayDecision(DisplayDecision {
            session_id,
            accepted: false,
            reason: Some("media pipeline unavailable".into()),
            media_port: None,
        });

        let json = serde_json::to_string(&message).expect("message should serialize");
        let decoded: ControlMessage =
            serde_json::from_str(&json).expect("message should deserialize");

        assert_eq!(decoded, message);
        assert!(json.contains(&session_id.to_string()));
    }

    #[test]
    fn accepted_display_decision_carries_the_media_endpoint() {
        let session_id = Uuid::new_v4();
        let message = ControlMessage::DisplayDecision(DisplayDecision {
            session_id,
            accepted: true,
            reason: None,
            media_port: Some(49_152),
        });

        let json = serde_json::to_string(&message).expect("message should serialize");
        let decoded: ControlMessage =
            serde_json::from_str(&json).expect("message should deserialize");

        assert_eq!(decoded, message);
        assert!(json.contains("49152"));
    }
}
