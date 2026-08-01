use crate::{AppSnapshot, Capability, LocalDevice, PeerDevice, Preferences};
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::RwLock,
};
use thiserror::Error;

pub struct PeerSpanCore {
    snapshot: RwLock<AppSnapshot>,
    preferences_path: PathBuf,
    trusted_devices_path: PathBuf,
}

impl PeerSpanCore {
    pub fn load(
        local_device: LocalDevice,
        data_dir: impl Into<PathBuf>,
    ) -> Result<Self, CoreError> {
        let data_dir = data_dir.into();
        fs::create_dir_all(&data_dir)?;
        let preferences_path = data_dir.join("preferences.json");
        let trusted_devices_path = data_dir.join("trusted-devices.json");
        let mut snapshot = AppSnapshot::new(local_device);
        snapshot.preferences = load_preferences(&preferences_path)?;
        snapshot.trusted_devices = load_trusted_devices(&trusted_devices_path)?;
        Ok(Self {
            snapshot: RwLock::new(snapshot),
            preferences_path,
            trusted_devices_path,
        })
    }

    pub fn snapshot(&self) -> Result<AppSnapshot, CoreError> {
        self.snapshot
            .read()
            .map(|snapshot| snapshot.clone())
            .map_err(|_| CoreError::StatePoisoned)
    }

    pub fn update_preferences(&self, preferences: Preferences) -> Result<AppSnapshot, CoreError> {
        validate_preferences(&preferences)?;
        persist_preferences(&self.preferences_path, &preferences)?;
        let mut snapshot = self
            .snapshot
            .write()
            .map_err(|_| CoreError::StatePoisoned)?;
        snapshot.preferences = preferences;
        Ok(snapshot.clone())
    }

    pub fn set_discovery_capability(&self, capability: Capability) -> Result<(), CoreError> {
        let mut snapshot = self
            .snapshot
            .write()
            .map_err(|_| CoreError::StatePoisoned)?;
        snapshot.capabilities.discovery = capability;
        Ok(())
    }

    pub fn set_secure_pairing_capability(&self, capability: Capability) -> Result<(), CoreError> {
        let mut snapshot = self
            .snapshot
            .write()
            .map_err(|_| CoreError::StatePoisoned)?;
        snapshot.capabilities.secure_pairing = capability;
        Ok(())
    }

    pub fn set_secure_control_capability(&self, capability: Capability) -> Result<(), CoreError> {
        let mut snapshot = self
            .snapshot
            .write()
            .map_err(|_| CoreError::StatePoisoned)?;
        snapshot.capabilities.secure_control = capability;
        Ok(())
    }

    pub fn set_virtual_display_capability(&self, capability: Capability) -> Result<(), CoreError> {
        let mut snapshot = self
            .snapshot
            .write()
            .map_err(|_| CoreError::StatePoisoned)?;
        snapshot.capabilities.virtual_display = capability;
        Ok(())
    }

    pub fn set_media_pipeline_capability(&self, capability: Capability) -> Result<(), CoreError> {
        let mut snapshot = self
            .snapshot
            .write()
            .map_err(|_| CoreError::StatePoisoned)?;
        snapshot.capabilities.media_pipeline = capability;
        Ok(())
    }

    pub fn set_input_injection_capability(&self, capability: Capability) -> Result<(), CoreError> {
        let mut snapshot = self
            .snapshot
            .write()
            .map_err(|_| CoreError::StatePoisoned)?;
        snapshot.capabilities.input_injection = capability;
        Ok(())
    }

    pub fn start_display_session(&self, session: crate::DisplaySession) -> Result<(), CoreError> {
        if session.width_px == 0 || session.height_px == 0 || session.refresh_hz == 0 {
            return Err(CoreError::Validation(
                "display session dimensions and refresh rate must be non-zero".into(),
            ));
        }
        let mut snapshot = self
            .snapshot
            .write()
            .map_err(|_| CoreError::StatePoisoned)?;
        if snapshot.active_session.is_some() {
            return Err(CoreError::ActiveSessionExists);
        }
        if !snapshot
            .trusted_devices
            .iter()
            .any(|peer| peer.id == session.peer_id)
        {
            return Err(CoreError::PeerNotTrusted);
        }
        snapshot.active_session = Some(session);
        Ok(())
    }

    pub fn update_display_session(
        &self,
        session_id: uuid::Uuid,
        state: crate::SessionState,
        latency_ms: Option<u16>,
    ) -> Result<(), CoreError> {
        let mut snapshot = self
            .snapshot
            .write()
            .map_err(|_| CoreError::StatePoisoned)?;
        let session = snapshot
            .active_session
            .as_mut()
            .filter(|session| session.id == session_id)
            .ok_or(CoreError::SessionNotFound)?;
        if !valid_session_transition(session.state, state) {
            return Err(CoreError::InvalidSessionTransition {
                from: session.state,
                to: state,
            });
        }
        session.state = state;
        session.latency_ms = latency_ms;
        Ok(())
    }

    pub fn end_display_session(&self, session_id: uuid::Uuid) -> Result<(), CoreError> {
        let mut snapshot = self
            .snapshot
            .write()
            .map_err(|_| CoreError::StatePoisoned)?;
        match snapshot.active_session.as_ref() {
            Some(session) if session.id == session_id => {
                snapshot.active_session = None;
                Ok(())
            }
            _ => Err(CoreError::SessionNotFound),
        }
    }

    pub fn upsert_nearby_device(&self, mut device: PeerDevice) -> Result<(), CoreError> {
        let mut snapshot = self
            .snapshot
            .write()
            .map_err(|_| CoreError::StatePoisoned)?;
        if device.id == snapshot.local_device.id {
            return Ok(());
        }

        device.trusted = snapshot.trusted_devices.iter().any(|peer| {
            peer.id == device.id
                && peer.public_key == device.public_key
                && peer.fingerprint == device.fingerprint
        });
        if let Some(existing) = snapshot
            .nearby_devices
            .iter_mut()
            .find(|peer| peer.id == device.id)
        {
            *existing = device;
        } else {
            snapshot.nearby_devices.push(device);
            snapshot
                .nearby_devices
                .sort_by(|left, right| left.name.cmp(&right.name));
        }
        Ok(())
    }

    pub fn remove_nearby_device(&self, device_id: uuid::Uuid) -> Result<(), CoreError> {
        let mut snapshot = self
            .snapshot
            .write()
            .map_err(|_| CoreError::StatePoisoned)?;
        snapshot.nearby_devices.retain(|peer| peer.id != device_id);
        Ok(())
    }

    pub fn trust_device(&self, mut device: PeerDevice) -> Result<AppSnapshot, CoreError> {
        if device.public_key.trim().is_empty() || device.fingerprint.trim().is_empty() {
            return Err(CoreError::Validation(
                "trusted device must include a public key and fingerprint".into(),
            ));
        }
        device.trusted = true;
        let mut snapshot = self
            .snapshot
            .write()
            .map_err(|_| CoreError::StatePoisoned)?;
        if let Some(existing) = snapshot
            .trusted_devices
            .iter_mut()
            .find(|peer| peer.id == device.id)
        {
            *existing = device.clone();
        } else {
            snapshot.trusted_devices.push(device.clone());
        }
        if let Some(nearby) = snapshot
            .nearby_devices
            .iter_mut()
            .find(|peer| peer.id == device.id)
        {
            nearby.trusted = true;
        }
        persist_trusted_devices(&self.trusted_devices_path, &snapshot.trusted_devices)?;
        Ok(snapshot.clone())
    }

    pub fn revoke_trusted_device(&self, device_id: uuid::Uuid) -> Result<AppSnapshot, CoreError> {
        let mut snapshot = self
            .snapshot
            .write()
            .map_err(|_| CoreError::StatePoisoned)?;
        snapshot.trusted_devices.retain(|peer| peer.id != device_id);
        if let Some(nearby) = snapshot
            .nearby_devices
            .iter_mut()
            .find(|peer| peer.id == device_id)
        {
            nearby.trusted = false;
        }
        persist_trusted_devices(&self.trusted_devices_path, &snapshot.trusted_devices)?;
        Ok(snapshot.clone())
    }
}

fn load_preferences(path: &Path) -> Result<Preferences, CoreError> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(CoreError::InvalidPreferences),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Preferences::default()),
        Err(error) => Err(error.into()),
    }
}

fn persist_preferences(path: &Path, preferences: &Preferences) -> Result<(), CoreError> {
    let bytes = serde_json::to_vec_pretty(preferences)?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, bytes)?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(temporary, path)?;
    Ok(())
}

fn load_trusted_devices(path: &Path) -> Result<Vec<PeerDevice>, CoreError> {
    match fs::read(path) {
        Ok(bytes) => {
            let mut devices: Vec<PeerDevice> =
                serde_json::from_slice(&bytes).map_err(CoreError::InvalidTrustedDevices)?;
            for device in &mut devices {
                device.trusted = true;
                device.status = crate::DeviceStatus::Offline;
                device.latency_ms = None;
                device.addresses.clear();
            }
            Ok(devices)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error.into()),
    }
}

fn persist_trusted_devices(path: &Path, devices: &[PeerDevice]) -> Result<(), CoreError> {
    let bytes = serde_json::to_vec_pretty(devices)?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, bytes)?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(temporary, path)?;
    Ok(())
}

fn validate_preferences(preferences: &Preferences) -> Result<(), CoreError> {
    if preferences.release_shortcut.trim().is_empty() {
        return Err(CoreError::Validation(
            "release shortcut cannot be empty".into(),
        ));
    }
    Ok(())
}

fn valid_session_transition(current: crate::SessionState, next: crate::SessionState) -> bool {
    use crate::SessionState::{Ending, Negotiating, Recovering, Streaming};
    matches!(
        (current, next),
        (Negotiating, Negotiating | Streaming | Ending)
            | (Streaming, Streaming | Recovering | Ending)
            | (Recovering, Recovering | Streaming | Ending)
            | (Ending, Ending)
    )
}

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("configuration serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("stored preferences are invalid: {0}")]
    InvalidPreferences(serde_json::Error),
    #[error("stored trusted devices are invalid: {0}")]
    InvalidTrustedDevices(serde_json::Error),
    #[error("invalid setting: {0}")]
    Validation(String),
    #[error("another display session is already active")]
    ActiveSessionExists,
    #[error("display sessions require a trusted peer")]
    PeerNotTrusted,
    #[error("display session was not found")]
    SessionNotFound,
    #[error("invalid display session transition from {from:?} to {to:?}")]
    InvalidSessionTransition {
        from: crate::SessionState,
        to: crate::SessionState,
    },
    #[error("application state lock is poisoned")]
    StatePoisoned,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{QualityMode, ScreenEdge};
    use uuid::Uuid;

    fn local_device() -> LocalDevice {
        LocalDevice {
            id: Uuid::nil(),
            name: "test-device".into(),
            platform: "Windows".into(),
            fingerprint: "TEST 0000".into(),
            public_key: "test-public-key".into(),
        }
    }

    #[test]
    fn preferences_persist_across_core_instances() {
        let directory = std::env::temp_dir().join(format!("peerspan-core-{}", Uuid::new_v4()));
        let core = PeerSpanCore::load(local_device(), &directory).unwrap();
        let updated = Preferences {
            screen_edge: ScreenEdge::Left,
            quality: QualityMode::Clarity,
            ..Preferences::default()
        };
        core.update_preferences(updated.clone()).unwrap();

        let reloaded = PeerSpanCore::load(local_device(), &directory).unwrap();
        assert_eq!(reloaded.snapshot().unwrap().preferences, updated);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn trusted_devices_persist_without_stale_network_state() {
        let directory = std::env::temp_dir().join(format!("peerspan-core-{}", Uuid::new_v4()));
        let core = PeerSpanCore::load(local_device(), &directory).unwrap();
        let device = PeerDevice {
            id: Uuid::new_v4(),
            name: "Studio PC".into(),
            platform: "Windows 11".into(),
            fingerprint: "1234 5678 90AB".into(),
            public_key: "07".repeat(32),
            status: crate::DeviceStatus::Online,
            trusted: false,
            latency_ms: Some(8),
            last_seen_unix_ms: 123,
            addresses: vec!["192.168.1.20".into()],
            control_port: 37_622,
            pairing_port: 37_621,
            protocol_version: 2,
        };
        core.trust_device(device.clone()).unwrap();

        let reloaded = PeerSpanCore::load(local_device(), &directory).unwrap();
        let trusted = &reloaded.snapshot().unwrap().trusted_devices[0];
        assert_eq!(trusted.id, device.id);
        assert!(trusted.trusted);
        assert_eq!(trusted.status, crate::DeviceStatus::Offline);
        assert!(trusted.addresses.is_empty());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn changed_discovery_identity_is_not_marked_trusted() {
        let directory = std::env::temp_dir().join(format!("peerspan-core-{}", Uuid::new_v4()));
        let core = PeerSpanCore::load(local_device(), &directory).unwrap();
        let mut device = PeerDevice {
            id: Uuid::new_v4(),
            name: "Studio PC".into(),
            platform: "Windows 11".into(),
            fingerprint: "AAAA BBBB CCCC".into(),
            public_key: "07".repeat(32),
            status: crate::DeviceStatus::Online,
            trusted: false,
            latency_ms: None,
            last_seen_unix_ms: 123,
            addresses: vec!["192.168.1.20".into()],
            control_port: 37_622,
            pairing_port: 37_621,
            protocol_version: 2,
        };
        core.trust_device(device.clone()).unwrap();

        device.public_key = "08".repeat(32);
        device.fingerprint = "DDDD EEEE FFFF".into();
        core.upsert_nearby_device(device).unwrap();

        assert!(!core.snapshot().unwrap().nearby_devices[0].trusted);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn display_session_requires_trust_and_enforces_single_active_session() {
        let directory = std::env::temp_dir().join(format!("peerspan-core-{}", Uuid::new_v4()));
        let core = PeerSpanCore::load(local_device(), &directory).unwrap();
        let peer_id = Uuid::new_v4();
        let session = crate::DisplaySession {
            id: Uuid::new_v4(),
            peer_id,
            direction: crate::SessionDirection::Sending,
            state: crate::SessionState::Negotiating,
            width_px: 1920,
            height_px: 1080,
            refresh_hz: 60,
            latency_ms: None,
        };
        assert!(matches!(
            core.start_display_session(session.clone()),
            Err(CoreError::PeerNotTrusted)
        ));

        core.trust_device(PeerDevice {
            id: peer_id,
            name: "Peer".into(),
            platform: "Windows".into(),
            fingerprint: "AAAA BBBB CCCC".into(),
            public_key: "07".repeat(32),
            status: crate::DeviceStatus::Online,
            trusted: true,
            latency_ms: None,
            last_seen_unix_ms: 0,
            addresses: Vec::new(),
            control_port: 37_622,
            pairing_port: 37_621,
            protocol_version: 2,
        })
        .unwrap();
        core.start_display_session(session.clone()).unwrap();
        assert!(matches!(
            core.start_display_session(session.clone()),
            Err(CoreError::ActiveSessionExists)
        ));
        core.update_display_session(session.id, crate::SessionState::Streaming, Some(8))
            .unwrap();
        assert_eq!(
            core.snapshot().unwrap().active_session.unwrap().latency_ms,
            Some(8)
        );
        core.update_display_session(session.id, crate::SessionState::Ending, Some(8))
            .unwrap();
        assert!(matches!(
            core.update_display_session(session.id, crate::SessionState::Streaming, Some(8)),
            Err(CoreError::InvalidSessionTransition { .. })
        ));
        core.end_display_session(session.id).unwrap();
        assert!(core.snapshot().unwrap().active_session.is_none());
        fs::remove_dir_all(directory).unwrap();
    }
}
