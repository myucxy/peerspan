use crate::{
    AppSnapshot, ApplicationCatalog, ApplicationSource, Capability, DisplayLayout, LocalDevice,
    PeerDevice, Preferences, PublishedApplication,
};
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::RwLock,
    time::{SystemTime, UNIX_EPOCH},
};
use thiserror::Error;

pub struct PeerSpanCore {
    snapshot: RwLock<AppSnapshot>,
    preferences_path: PathBuf,
    trusted_devices_path: PathBuf,
    applications_path: PathBuf,
    application_catalog_revision_path: PathBuf,
    display_layouts_path: PathBuf,
}

pub const MAX_ACTIVE_DISPLAY_SESSIONS: usize = 8;
pub const MAX_PUBLISHED_APPLICATIONS: usize = 512;

impl PeerSpanCore {
    pub fn load(
        local_device: LocalDevice,
        data_dir: impl Into<PathBuf>,
    ) -> Result<Self, CoreError> {
        let data_dir = data_dir.into();
        fs::create_dir_all(&data_dir)?;
        let preferences_path = data_dir.join("preferences.json");
        let trusted_devices_path = data_dir.join("trusted-devices.json");
        let applications_path = data_dir.join("published-applications.json");
        let application_catalog_revision_path = data_dir.join("application-catalog-revision.json");
        let display_layouts_path = data_dir.join("display-layouts.json");
        let mut snapshot = AppSnapshot::new(local_device);
        snapshot.preferences = load_preferences(&preferences_path)?;
        snapshot.trusted_devices = load_trusted_devices(&trusted_devices_path)?;
        snapshot.local_applications = load_applications(&applications_path)?;
        snapshot.local_catalog_revision = load_catalog_revision(
            &application_catalog_revision_path,
            &snapshot.local_applications,
        )?;
        snapshot.display_layouts = load_display_layouts(&display_layouts_path)?;
        Ok(Self {
            snapshot: RwLock::new(snapshot),
            preferences_path,
            trusted_devices_path,
            applications_path,
            application_catalog_revision_path,
            display_layouts_path,
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

    pub fn set_streaming_backend_capability(
        &self,
        capability: Capability,
    ) -> Result<(), CoreError> {
        let mut snapshot = self
            .snapshot
            .write()
            .map_err(|_| CoreError::StatePoisoned)?;
        snapshot.capabilities.streaming_backend = capability;
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
        if snapshot.display_sessions.len() >= MAX_ACTIVE_DISPLAY_SESSIONS {
            return Err(CoreError::SessionCapacityReached);
        }
        if snapshot
            .display_sessions
            .iter()
            .any(|active| active.peer_id == session.peer_id || active.id == session.id)
        {
            return Err(CoreError::ActiveSessionExists);
        }
        if !snapshot
            .trusted_devices
            .iter()
            .any(|peer| peer.id == session.peer_id)
        {
            return Err(CoreError::PeerNotTrusted);
        }
        snapshot.display_sessions.push(session);
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
            .display_sessions
            .iter_mut()
            .find(|session| session.id == session_id)
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
        if let Some(index) = snapshot
            .display_sessions
            .iter()
            .position(|session| session.id == session_id)
        {
            snapshot.display_sessions.remove(index);
            Ok(())
        } else {
            Err(CoreError::SessionNotFound)
        }
    }

    pub fn set_display_layout(
        &self,
        peer_id: uuid::Uuid,
        x: i32,
        y: i32,
    ) -> Result<AppSnapshot, CoreError> {
        if !(-4096..=4096).contains(&x) || !(-4096..=4096).contains(&y) {
            return Err(CoreError::Validation(
                "display layout coordinates must stay within -4096..4096".into(),
            ));
        }
        let mut snapshot = self
            .snapshot
            .write()
            .map_err(|_| CoreError::StatePoisoned)?;
        if let Some(layout) = snapshot
            .display_layouts
            .iter_mut()
            .find(|layout| layout.peer_id == peer_id)
        {
            layout.x = x;
            layout.y = y;
        } else {
            snapshot
                .display_layouts
                .push(DisplayLayout { peer_id, x, y });
        }
        persist_json(&self.display_layouts_path, &snapshot.display_layouts)?;
        Ok(snapshot.clone())
    }

    pub fn save_application(
        &self,
        application: PublishedApplication,
    ) -> Result<AppSnapshot, CoreError> {
        validate_application(&application)?;
        let mut snapshot = self
            .snapshot
            .write()
            .map_err(|_| CoreError::StatePoisoned)?;
        if let Some(existing) = snapshot
            .local_applications
            .iter_mut()
            .find(|existing| existing.id == application.id)
        {
            *existing = application;
        } else {
            if snapshot.local_applications.len() >= MAX_PUBLISHED_APPLICATIONS {
                return Err(CoreError::ApplicationCapacityReached);
            }
            snapshot.local_applications.push(application);
        }
        sort_applications(&mut snapshot.local_applications);
        persist_json(&self.applications_path, &snapshot.local_applications)?;
        bump_catalog_revision(&mut snapshot, &self.application_catalog_revision_path)?;
        Ok(snapshot.clone())
    }

    pub fn remove_application(&self, application_id: uuid::Uuid) -> Result<AppSnapshot, CoreError> {
        let mut snapshot = self
            .snapshot
            .write()
            .map_err(|_| CoreError::StatePoisoned)?;
        let original_len = snapshot.local_applications.len();
        snapshot
            .local_applications
            .retain(|application| application.id != application_id);
        if snapshot.local_applications.len() == original_len {
            return Err(CoreError::ApplicationNotFound);
        }
        persist_json(&self.applications_path, &snapshot.local_applications)?;
        bump_catalog_revision(&mut snapshot, &self.application_catalog_revision_path)?;
        Ok(snapshot.clone())
    }

    pub fn replace_scanned_applications(
        &self,
        mut scanned: Vec<PublishedApplication>,
    ) -> Result<AppSnapshot, CoreError> {
        if scanned.len() > MAX_PUBLISHED_APPLICATIONS {
            scanned.truncate(MAX_PUBLISHED_APPLICATIONS);
        }
        for application in &scanned {
            validate_application(application)?;
            if application.source == ApplicationSource::Manual {
                return Err(CoreError::Validation(
                    "automatic scan results cannot use the manual source".into(),
                ));
            }
        }
        let mut snapshot = self
            .snapshot
            .write()
            .map_err(|_| CoreError::StatePoisoned)?;
        let manual: Vec<_> = snapshot
            .local_applications
            .iter()
            .filter(|application| application.source == ApplicationSource::Manual)
            .cloned()
            .collect();
        scanned.retain(|application| !manual.iter().any(|manual| manual.id == application.id));
        for application in &mut scanned {
            if let Some(existing) = snapshot
                .local_applications
                .iter()
                .find(|existing| existing.id == application.id)
            {
                application.enabled = existing.enabled;
            }
        }
        let remaining = MAX_PUBLISHED_APPLICATIONS.saturating_sub(manual.len());
        scanned.truncate(remaining);
        scanned.extend(manual);
        sort_applications(&mut scanned);
        snapshot.local_applications = scanned;
        persist_json(&self.applications_path, &snapshot.local_applications)?;
        bump_catalog_revision(&mut snapshot, &self.application_catalog_revision_path)?;
        Ok(snapshot.clone())
    }

    pub fn upsert_application_catalog(
        &self,
        mut catalog: ApplicationCatalog,
    ) -> Result<(), CoreError> {
        if catalog.device_id == self.snapshot()?.local_device.id {
            return Ok(());
        }
        if catalog.applications.len() > MAX_PUBLISHED_APPLICATIONS {
            return Err(CoreError::Validation(
                "remote application catalog is too large".into(),
            ));
        }
        catalog
            .applications
            .sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
        let mut snapshot = self
            .snapshot
            .write()
            .map_err(|_| CoreError::StatePoisoned)?;
        if !snapshot
            .trusted_devices
            .iter()
            .any(|peer| peer.id == catalog.device_id)
        {
            return Err(CoreError::PeerNotTrusted);
        }
        if let Some(existing) = snapshot
            .application_catalogs
            .iter_mut()
            .find(|existing| existing.device_id == catalog.device_id)
        {
            if catalog.revision >= existing.revision {
                *existing = catalog;
            }
        } else {
            snapshot.application_catalogs.push(catalog);
        }
        snapshot
            .application_catalogs
            .sort_by(|left, right| left.device_name.cmp(&right.device_name));
        Ok(())
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

fn load_applications(path: &Path) -> Result<Vec<PublishedApplication>, CoreError> {
    load_json_collection(path, CoreError::InvalidApplications)
}

fn load_display_layouts(path: &Path) -> Result<Vec<DisplayLayout>, CoreError> {
    load_json_collection(path, CoreError::InvalidDisplayLayouts)
}

fn load_catalog_revision(
    path: &Path,
    applications: &[PublishedApplication],
) -> Result<u64, CoreError> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(CoreError::InvalidCatalogRevision),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(applications
            .iter()
            .map(|application| application.updated_at_unix_ms)
            .max()
            .unwrap_or(0)),
        Err(error) => Err(error.into()),
    }
}

fn bump_catalog_revision(snapshot: &mut AppSnapshot, path: &Path) -> Result<(), CoreError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    snapshot.local_catalog_revision = snapshot.local_catalog_revision.saturating_add(1).max(now);
    persist_json(path, &snapshot.local_catalog_revision)
}

fn load_json_collection<T>(
    path: &Path,
    invalid: fn(serde_json::Error) -> CoreError,
) -> Result<Vec<T>, CoreError>
where
    T: serde::de::DeserializeOwned,
{
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(invalid),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error.into()),
    }
}

fn persist_json<T: serde::Serialize + ?Sized>(path: &Path, value: &T) -> Result<(), CoreError> {
    let bytes = serde_json::to_vec_pretty(value)?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, bytes)?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(temporary, path)?;
    Ok(())
}

fn validate_application(application: &PublishedApplication) -> Result<(), CoreError> {
    let name = application.name.trim();
    let target = application.launch_target.trim();
    if name.is_empty() || name.chars().count() > 128 {
        return Err(CoreError::Validation(
            "application name must contain 1..128 characters".into(),
        ));
    }
    if target.is_empty() || target.chars().count() > 2048 {
        return Err(CoreError::Validation(
            "application launch target must contain 1..2048 characters".into(),
        ));
    }
    if application.arguments.chars().count() > 4096 {
        return Err(CoreError::Validation(
            "application arguments are longer than 4096 characters".into(),
        ));
    }
    Ok(())
}

fn sort_applications(applications: &mut [PublishedApplication]) {
    applications.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.id.cmp(&right.id))
    });
}

fn validate_preferences(preferences: &Preferences) -> Result<(), CoreError> {
    if crate::parse_release_shortcut(&preferences.release_shortcut).is_none() {
        return Err(CoreError::Validation(
            "release shortcut must contain at least two modifiers and one supported key".into(),
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
    #[error("stored published applications are invalid: {0}")]
    InvalidApplications(serde_json::Error),
    #[error("stored display layouts are invalid: {0}")]
    InvalidDisplayLayouts(serde_json::Error),
    #[error("stored application catalog revision is invalid: {0}")]
    InvalidCatalogRevision(serde_json::Error),
    #[error("invalid setting: {0}")]
    Validation(String),
    #[error("a display session with this peer is already active")]
    ActiveSessionExists,
    #[error("the maximum number of concurrent display sessions has been reached")]
    SessionCapacityReached,
    #[error("display sessions require a trusted peer")]
    PeerNotTrusted,
    #[error("display session was not found")]
    SessionNotFound,
    #[error("the published application was not found")]
    ApplicationNotFound,
    #[error("the published application limit has been reached")]
    ApplicationCapacityReached,
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
    use crate::{QualityMode, ScreenEdge, StreamingBackend};
    use uuid::Uuid;

    #[test]
    fn release_shortcut_parser_accepts_safe_combinations() {
        let shortcut = crate::parse_release_shortcut("Ctrl+Alt+Shift+Esc").unwrap();
        assert!(shortcut.control && shortcut.alt && shortcut.shift);
        assert_eq!(shortcut.virtual_key, 0x1b);
        assert!(crate::parse_release_shortcut("Ctrl+A").is_none());
        assert!(crate::parse_release_shortcut("Ctrl+Alt+Unknown").is_none());
    }

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
    fn legacy_preferences_default_to_sunshine_and_moonlight() {
        let directory = std::env::temp_dir().join(format!("peerspan-core-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("preferences.json"),
            br#"{
  "launchAtStartup": false,
  "autoReconnect": true,
  "clipboardSync": true,
  "screenEdge": "right",
  "quality": "balanced",
  "releaseShortcut": "Ctrl+Alt+Shift+Esc"
}"#,
        )
        .unwrap();

        let core = PeerSpanCore::load(local_device(), &directory).unwrap();
        assert_eq!(
            core.snapshot().unwrap().preferences.streaming_backend,
            StreamingBackend::SunshineMoonlight
        );
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
            protocol_version: 4,
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
            protocol_version: 4,
        };
        core.trust_device(device.clone()).unwrap();

        device.public_key = "08".repeat(32);
        device.fingerprint = "DDDD EEEE FFFF".into();
        core.upsert_nearby_device(device).unwrap();

        assert!(!core.snapshot().unwrap().nearby_devices[0].trusted);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn display_sessions_require_trust_and_are_isolated_per_peer() {
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
            protocol_version: 4,
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
            core.snapshot().unwrap().display_sessions[0].latency_ms,
            Some(8)
        );
        core.update_display_session(session.id, crate::SessionState::Ending, Some(8))
            .unwrap();
        assert!(matches!(
            core.update_display_session(session.id, crate::SessionState::Streaming, Some(8)),
            Err(CoreError::InvalidSessionTransition { .. })
        ));
        core.end_display_session(session.id).unwrap();
        assert!(core.snapshot().unwrap().display_sessions.is_empty());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn multiple_peers_can_have_concurrent_sessions_and_persisted_layouts() {
        let directory = std::env::temp_dir().join(format!("peerspan-core-{}", Uuid::new_v4()));
        let core = PeerSpanCore::load(local_device(), &directory).unwrap();
        let peers = [Uuid::new_v4(), Uuid::new_v4()];
        for (index, peer_id) in peers.into_iter().enumerate() {
            core.trust_device(PeerDevice {
                id: peer_id,
                name: format!("Peer {index}"),
                platform: "Windows".into(),
                fingerprint: format!("peer-{index}"),
                public_key: format!("{index:02}").repeat(32),
                status: crate::DeviceStatus::Online,
                trusted: true,
                latency_ms: None,
                last_seen_unix_ms: 0,
                addresses: Vec::new(),
                control_port: 37_622,
                pairing_port: 37_621,
                protocol_version: 6,
            })
            .unwrap();
            core.start_display_session(crate::DisplaySession {
                id: Uuid::new_v4(),
                peer_id,
                direction: crate::SessionDirection::Sending,
                state: crate::SessionState::Negotiating,
                width_px: 1920,
                height_px: 1080,
                refresh_hz: 60,
                latency_ms: None,
            })
            .unwrap();
        }
        assert_eq!(core.snapshot().unwrap().display_sessions.len(), 2);
        core.set_display_layout(peers[0], -240, 90).unwrap();
        let reloaded = PeerSpanCore::load(local_device(), &directory).unwrap();
        assert_eq!(
            reloaded.snapshot().unwrap().display_layouts,
            vec![DisplayLayout {
                peer_id: peers[0],
                x: -240,
                y: 90
            }]
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn manual_and_scanned_applications_merge_and_catalog_revision_survives_deletion() {
        let directory = std::env::temp_dir().join(format!("peerspan-apps-{}", Uuid::new_v4()));
        let core = PeerSpanCore::load(local_device(), &directory).unwrap();
        let manual_id = Uuid::new_v4();
        let scanned_id = Uuid::new_v4();
        core.save_application(PublishedApplication {
            id: manual_id,
            name: "Manual Editor".into(),
            launch_target: "editor.exe".into(),
            arguments: String::new(),
            working_directory: None,
            kind: crate::ApplicationKind::Gui,
            source: ApplicationSource::Manual,
            enabled: true,
            updated_at_unix_ms: 1,
        })
        .unwrap();
        core.replace_scanned_applications(vec![PublishedApplication {
            id: scanned_id,
            name: "Scanned Tool".into(),
            launch_target: "tool.lnk".into(),
            arguments: String::new(),
            working_directory: None,
            kind: crate::ApplicationKind::Gui,
            source: ApplicationSource::StartMenu,
            enabled: true,
            updated_at_unix_ms: 2,
        }])
        .unwrap();
        let mut scanned = core
            .snapshot()
            .unwrap()
            .local_applications
            .into_iter()
            .find(|application| application.id == scanned_id)
            .unwrap();
        scanned.enabled = false;
        core.save_application(scanned).unwrap();
        core.replace_scanned_applications(vec![PublishedApplication {
            id: scanned_id,
            name: "Scanned Tool".into(),
            launch_target: "tool.lnk".into(),
            arguments: String::new(),
            working_directory: None,
            kind: crate::ApplicationKind::Gui,
            source: ApplicationSource::StartMenu,
            enabled: true,
            updated_at_unix_ms: 3,
        }])
        .unwrap();
        let snapshot = core.snapshot().unwrap();
        assert_eq!(snapshot.local_applications.len(), 2);
        assert!(
            !snapshot
                .local_applications
                .iter()
                .find(|application| application.id == scanned_id)
                .unwrap()
                .enabled
        );
        let revision_before_delete = snapshot.local_catalog_revision;
        core.remove_application(manual_id).unwrap();
        let revision_after_delete = core.snapshot().unwrap().local_catalog_revision;
        assert!(revision_after_delete > revision_before_delete);
        let reloaded = PeerSpanCore::load(local_device(), &directory).unwrap();
        assert_eq!(
            reloaded.snapshot().unwrap().local_catalog_revision,
            revision_after_delete
        );
        assert_eq!(reloaded.snapshot().unwrap().local_applications.len(), 1);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn remote_catalog_requires_trust_and_ignores_stale_revisions() {
        let directory = std::env::temp_dir().join(format!("peerspan-catalog-{}", Uuid::new_v4()));
        let core = PeerSpanCore::load(local_device(), &directory).unwrap();
        let peer_id = Uuid::new_v4();
        let catalog = ApplicationCatalog {
            device_id: peer_id,
            device_name: "Remote".into(),
            revision: 5,
            updated_at_unix_ms: 10,
            applications: vec![crate::ApplicationSummary {
                id: Uuid::new_v4(),
                name: "Remote App".into(),
                kind: crate::ApplicationKind::Gui,
            }],
        };
        assert!(matches!(
            core.upsert_application_catalog(catalog.clone()),
            Err(CoreError::PeerNotTrusted)
        ));
        core.trust_device(PeerDevice {
            id: peer_id,
            name: "Remote".into(),
            platform: "Windows".into(),
            fingerprint: "remote".into(),
            public_key: "22".repeat(32),
            status: crate::DeviceStatus::Online,
            trusted: true,
            latency_ms: None,
            last_seen_unix_ms: 0,
            addresses: Vec::new(),
            control_port: 37_622,
            pairing_port: 37_621,
            protocol_version: 6,
        })
        .unwrap();
        core.upsert_application_catalog(catalog).unwrap();
        core.upsert_application_catalog(ApplicationCatalog {
            device_id: peer_id,
            device_name: "Remote".into(),
            revision: 4,
            updated_at_unix_ms: 11,
            applications: Vec::new(),
        })
        .unwrap();
        assert_eq!(
            core.snapshot().unwrap().application_catalogs[0]
                .applications
                .len(),
            1
        );
        fs::remove_dir_all(directory).unwrap();
    }
}
