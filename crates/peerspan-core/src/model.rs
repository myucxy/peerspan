use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    pub local_device: LocalDevice,
    pub nearby_devices: Vec<PeerDevice>,
    pub trusted_devices: Vec<PeerDevice>,
    pub active_session: Option<DisplaySession>,
    pub preferences: Preferences,
    pub capabilities: RuntimeCapabilities,
}

impl AppSnapshot {
    pub fn new(local_device: LocalDevice) -> Self {
        Self {
            local_device,
            nearby_devices: Vec::new(),
            trusted_devices: Vec::new(),
            active_session: None,
            preferences: Preferences::default(),
            capabilities: RuntimeCapabilities::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LocalDevice {
    pub id: Uuid,
    pub name: String,
    pub platform: String,
    pub fingerprint: String,
    pub public_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PeerDevice {
    pub id: Uuid,
    pub name: String,
    pub platform: String,
    pub fingerprint: String,
    pub public_key: String,
    pub status: DeviceStatus,
    pub trusted: bool,
    pub latency_ms: Option<u16>,
    pub last_seen_unix_ms: u64,
    pub addresses: Vec<String>,
    pub control_port: u16,
    #[serde(default = "default_pairing_port")]
    pub pairing_port: u16,
    pub protocol_version: u16,
}

fn default_pairing_port() -> u16 {
    37_621
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DeviceStatus {
    Online,
    Busy,
    Offline,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DisplaySession {
    pub id: Uuid,
    pub peer_id: Uuid,
    pub direction: SessionDirection,
    pub state: SessionState,
    pub width_px: u32,
    pub height_px: u32,
    pub refresh_hz: u16,
    pub latency_ms: Option<u16>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SessionDirection {
    Sending,
    Receiving,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SessionState {
    Negotiating,
    Streaming,
    Recovering,
    Ending,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Preferences {
    pub launch_at_startup: bool,
    pub auto_reconnect: bool,
    pub clipboard_sync: bool,
    pub screen_edge: ScreenEdge,
    pub quality: QualityMode,
    pub release_shortcut: String,
    #[serde(default)]
    pub streaming_backend: StreamingBackend,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            launch_at_startup: false,
            auto_reconnect: true,
            clipboard_sync: true,
            screen_edge: ScreenEdge::Right,
            quality: QualityMode::Balanced,
            release_shortcut: "Ctrl+Alt+Shift+Esc".into(),
            streaming_backend: StreamingBackend::SunshineMoonlight,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StreamingBackend {
    #[default]
    SunshineMoonlight,
    Native,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReleaseShortcut {
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
    pub windows: bool,
    pub virtual_key: u16,
}

pub fn parse_release_shortcut(value: &str) -> Option<ReleaseShortcut> {
    let mut shortcut = ReleaseShortcut {
        control: false,
        alt: false,
        shift: false,
        windows: false,
        virtual_key: 0,
    };
    for token in value
        .split('+')
        .map(str::trim)
        .filter(|token| !token.is_empty())
    {
        match token.to_ascii_lowercase().as_str() {
            "ctrl" | "control" if !shortcut.control => shortcut.control = true,
            "alt" if !shortcut.alt => shortcut.alt = true,
            "shift" if !shortcut.shift => shortcut.shift = true,
            "win" | "windows" if !shortcut.windows => shortcut.windows = true,
            key if shortcut.virtual_key == 0 => {
                shortcut.virtual_key = match key {
                    "esc" | "escape" => 0x1b,
                    "tab" => 0x09,
                    "backspace" => 0x08,
                    "delete" | "del" => 0x2e,
                    key if key.len() == 1 && key.as_bytes()[0].is_ascii_alphanumeric() => {
                        u16::from(key.as_bytes()[0].to_ascii_uppercase())
                    }
                    key if key.starts_with('f') => key[1..]
                        .parse::<u16>()
                        .ok()
                        .filter(|number| (1..=12).contains(number))
                        .map(|number| 0x6f + number)?,
                    _ => return None,
                };
            }
            _ => return None,
        }
    }
    let modifier_count = [
        shortcut.control,
        shortcut.alt,
        shortcut.shift,
        shortcut.windows,
    ]
    .into_iter()
    .filter(|enabled| *enabled)
    .count();
    (modifier_count >= 2 && shortcut.virtual_key != 0).then_some(shortcut)
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ScreenEdge {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum QualityMode {
    Clarity,
    Balanced,
    Responsive,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCapabilities {
    pub control_bridge: Capability,
    pub discovery: Capability,
    pub secure_pairing: Capability,
    pub secure_control: Capability,
    pub virtual_display: Capability,
    pub streaming_backend: Capability,
    pub media_pipeline: Capability,
    pub input_injection: Capability,
}

impl Default for RuntimeCapabilities {
    fn default() -> Self {
        Self {
            control_bridge: Capability::ready("Tauri command bridge is active"),
            discovery: Capability::planned("LAN discovery adapter is not connected yet"),
            secure_pairing: Capability::planned("PAKE pairing listener is not active yet"),
            secure_control: Capability::planned(
                "Mutually authenticated TLS control listener is not active yet",
            ),
            virtual_display: Capability::required(
                "PeerSpan virtual display driver is not installed",
            ),
            streaming_backend: Capability::required(
                "Sunshine and Moonlight runtimes have not been located",
            ),
            media_pipeline: Capability::planned("D3D11 capture and hardware codec spike pending"),
            input_injection: Capability::planned(
                "Input adapter is disabled until an authenticated session exists",
            ),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Capability {
    pub state: CapabilityState,
    pub detail: String,
}

impl Capability {
    pub fn ready(detail: impl Into<String>) -> Self {
        Self {
            state: CapabilityState::Ready,
            detail: detail.into(),
        }
    }

    pub fn planned(detail: impl Into<String>) -> Self {
        Self {
            state: CapabilityState::Planned,
            detail: detail.into(),
        }
    }

    pub fn required(detail: impl Into<String>) -> Self {
        Self {
            state: CapabilityState::RequiresSetup,
            detail: detail.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CapabilityState {
    Ready,
    RequiresSetup,
    Planned,
}
