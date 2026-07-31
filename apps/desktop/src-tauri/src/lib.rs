mod discovery;
mod pairing;

use discovery::{DiscoveryRuntime, mark_discovery_ready, mark_discovery_unavailable};
use ed25519_dalek::SigningKey;
use pairing::{
    DeviceCredentials, PairingOffer, PairingRuntime, fingerprint_public_key, mark_pairing_ready,
    mark_pairing_unavailable,
};
use peerspan_core::{AppSnapshot, LocalDevice, PeerSpanCore, Preferences};
use rand_core::OsRng;
use serde::{Deserialize, Serialize};
use std::{fs, io, path::Path, sync::Arc};
use tauri::{Manager, State};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredIdentity {
    device_id: Uuid,
    #[serde(default)]
    signing_key_hex: String,
}

#[tauri::command]
fn get_app_snapshot(core: State<'_, Arc<PeerSpanCore>>) -> Result<AppSnapshot, String> {
    core.snapshot().map_err(|error| error.to_string())
}

#[tauri::command]
fn refresh_devices(core: State<'_, Arc<PeerSpanCore>>) -> Result<AppSnapshot, String> {
    // mDNS browsing is continuous. Refresh returns its latest real cache without
    // injecting UI samples or restarting the daemon.
    core.snapshot().map_err(|error| error.to_string())
}

#[tauri::command]
fn update_preferences(
    core: State<'_, Arc<PeerSpanCore>>,
    preferences: Preferences,
) -> Result<AppSnapshot, String> {
    core.update_preferences(preferences)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn request_display_session(peer_id: String) -> Result<(), String> {
    Uuid::parse_str(&peer_id).map_err(|_| "Invalid peer device identifier".to_owned())?;
    Err("The virtual display and authenticated media adapters are not installed yet".into())
}

#[tauri::command]
fn create_pairing_offer(runtime: State<'_, PairingRuntime>) -> PairingOffer {
    runtime.create_offer()
}

#[tauri::command]
fn pair_device(
    runtime: State<'_, PairingRuntime>,
    peer_id: String,
    code: String,
) -> Result<peerspan_core::PeerDevice, String> {
    let peer_id = Uuid::parse_str(&peer_id).map_err(|_| "Invalid peer device identifier")?;
    runtime.pair_device(peer_id, &code)
}

fn load_or_create_identity(data_dir: &Path) -> Result<StoredIdentity, Box<dyn std::error::Error>> {
    let path = data_dir.join("identity.json");
    match fs::read(&path) {
        Ok(bytes) => {
            let mut identity: StoredIdentity = serde_json::from_slice(&bytes)?;
            if decode_signing_key(&identity.signing_key_hex).is_err() {
                identity.signing_key_hex = generate_signing_key_hex();
                persist_identity(&path, &identity)?;
            }
            Ok(identity)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let identity = StoredIdentity {
                device_id: Uuid::new_v4(),
                signing_key_hex: generate_signing_key_hex(),
            };
            persist_identity(&path, &identity)?;
            Ok(identity)
        }
        Err(error) => Err(error.into()),
    }
}

fn generate_signing_key_hex() -> String {
    hex::encode(SigningKey::generate(&mut OsRng).to_bytes())
}

fn decode_signing_key(value: &str) -> Result<SigningKey, io::Error> {
    let bytes = hex::decode(value)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "identity key is not hex"))?;
    let bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "identity key has wrong length"))?;
    Ok(SigningKey::from_bytes(&bytes))
}

fn persist_identity(
    path: &Path,
    identity: &StoredIdentity,
) -> Result<(), Box<dyn std::error::Error>> {
    fs::write(path, serde_json::to_vec_pretty(identity)?)?;
    Ok(())
}

fn fingerprint(identity: &StoredIdentity) -> Result<String, io::Error> {
    let signing_key = decode_signing_key(&identity.signing_key_hex)?;
    Ok(fingerprint_public_key(
        &signing_key.verifying_key().to_bytes(),
    ))
}

fn public_key(identity: &StoredIdentity) -> Result<String, io::Error> {
    Ok(hex::encode(
        decode_signing_key(&identity.signing_key_hex)?
            .verifying_key()
            .to_bytes(),
    ))
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            fs::create_dir_all(&data_dir)?;
            let identity = load_or_create_identity(&data_dir)?;
            let name = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "Windows PC".into());
            let local_device = LocalDevice {
                id: identity.device_id,
                name,
                platform: format!("Windows · {}", std::env::consts::ARCH),
                fingerprint: fingerprint(&identity)?,
                public_key: public_key(&identity)?,
            };
            let core = Arc::new(PeerSpanCore::load(local_device.clone(), &data_dir)?);
            let credentials = DeviceCredentials {
                device: local_device.clone(),
                signing_key: decode_signing_key(&identity.signing_key_hex)?,
            };
            match PairingRuntime::start(credentials, Arc::clone(&core)) {
                Ok(runtime) => {
                    mark_pairing_ready(&core);
                    app.manage(runtime);
                }
                Err(error) => mark_pairing_unavailable(&core, &error),
            }
            match DiscoveryRuntime::start(&local_device, Arc::clone(&core)) {
                Ok(runtime) => {
                    mark_discovery_ready(&core);
                    app.manage(runtime);
                }
                Err(error) => mark_discovery_unavailable(&core, &error),
            }
            app.manage(core);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_app_snapshot,
            refresh_devices,
            update_preferences,
            create_pairing_offer,
            pair_device,
            request_display_session,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run PeerSpan desktop application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_derived_from_public_key_and_grouped() {
        let identity = StoredIdentity {
            device_id: Uuid::nil(),
            signing_key_hex: hex::encode([7_u8; 32]),
        };
        let value = fingerprint(&identity).unwrap();
        assert_eq!(value.len(), 29);
        assert_eq!(value.split(' ').count(), 6);
        assert_eq!(value, fingerprint(&identity).unwrap());
    }
}
