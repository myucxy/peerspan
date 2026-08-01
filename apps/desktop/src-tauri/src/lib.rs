// MSVC's localized "creating library" progress line is emitted on stdout.
// Rust 1.97 classifies any linker stdout as `linker_messages`, even though this
// line is informational rather than an LNK warning. Keep older MSRV compilers
// compatible while suppressing only this crate-level false positive.
#![allow(unknown_lints)]
#![allow(linker_messages)]

mod applications;
mod clipboard;
mod control;
mod discovery;
mod gamestream;
mod identity;
mod input;
mod pairing;
mod startup;
mod video;
mod virtual_display;

use control::{ControlRuntime, mark_control_ready, mark_control_unavailable};
use discovery::{DiscoveryRuntime, mark_discovery_ready, mark_discovery_unavailable};
use gamestream::GameStreamRuntime;
use identity::{DeviceIdentity, load_or_create_identity};
use input::probe_input_capability;
use pairing::{
    DeviceCredentials, PairingOffer, PairingRuntime, fingerprint_public_key, mark_pairing_ready,
    mark_pairing_unavailable,
};
use peerspan_core::{
    AppSnapshot, LocalDevice, PeerSpanCore, Preferences, PublishedApplication, StreamingBackend,
};
use std::{fs, sync::Arc};
use tauri::{Manager, State};
use uuid::Uuid;
use video::probe_video_capability;
use virtual_display::VirtualDisplayRuntime;

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
    virtual_display: State<'_, VirtualDisplayRuntime>,
    gamestream: State<'_, Arc<GameStreamRuntime>>,
    preferences: Preferences,
) -> Result<AppSnapshot, String> {
    if preferences.streaming_backend != StreamingBackend::SunshineMoonlight {
        return Err(
            "VirtualDrivers VDD is supported through the Sunshine + Moonlight backend only".into(),
        );
    }
    let previous = core.snapshot().map_err(|error| error.to_string())?;
    if previous.preferences.launch_at_startup != preferences.launch_at_startup {
        startup::set_launch_at_startup(preferences.launch_at_startup)?;
    }
    if previous.preferences.screen_edge != preferences.screen_edge {
        virtual_display.apply_layout(preferences.screen_edge)?;
    }
    if previous.preferences.streaming_backend != preferences.streaming_backend
        && !previous.display_sessions.is_empty()
    {
        return Err("End the active display session before changing the streaming backend".into());
    }
    core.update_preferences(preferences)
        .map_err(|error| error.to_string())?;
    probe_video_capability(&core);
    gamestream.apply_capability(&core);
    core.snapshot().map_err(|error| error.to_string())
}

#[tauri::command]
fn set_display_layout(
    core: State<'_, Arc<PeerSpanCore>>,
    peer_id: String,
    x: i32,
    y: i32,
) -> Result<AppSnapshot, String> {
    let peer_id = Uuid::parse_str(&peer_id).map_err(|_| "Invalid peer device identifier")?;
    core.set_display_layout(peer_id, x, y)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn scan_published_applications(core: State<'_, Arc<PeerSpanCore>>) -> Result<AppSnapshot, String> {
    let applications = applications::scan_installed_applications()?;
    core.replace_scanned_applications(applications)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn save_published_application(
    core: State<'_, Arc<PeerSpanCore>>,
    application: PublishedApplication,
) -> Result<AppSnapshot, String> {
    core.save_application(application)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn remove_published_application(
    core: State<'_, Arc<PeerSpanCore>>,
    application_id: String,
) -> Result<AppSnapshot, String> {
    let application_id =
        Uuid::parse_str(&application_id).map_err(|_| "Invalid application identifier")?;
    core.remove_application(application_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn sync_application_catalogs(runtime: State<'_, ControlRuntime>) -> Result<AppSnapshot, String> {
    runtime.sync_application_catalogs()
}

#[tauri::command]
fn request_display_session(
    runtime: State<'_, ControlRuntime>,
    peer_id: String,
) -> Result<peerspan_core::DisplaySession, String> {
    let peer_id =
        Uuid::parse_str(&peer_id).map_err(|_| "Invalid peer device identifier".to_owned())?;
    runtime.request_display_session(peer_id)
}

#[tauri::command]
fn end_display_session(
    runtime: State<'_, ControlRuntime>,
    session_id: String,
) -> Result<(), String> {
    let session_id = Uuid::parse_str(&session_id)
        .map_err(|_| "Invalid display session identifier".to_owned())?;
    runtime.end_display_session(session_id)
}

#[tauri::command]
fn start_virtual_display(
    runtime: State<'_, VirtualDisplayRuntime>,
    core: State<'_, Arc<PeerSpanCore>>,
) -> Result<AppSnapshot, String> {
    runtime.start()?;
    core.snapshot().map_err(|error| error.to_string())
}

#[tauri::command]
fn stop_virtual_display(
    runtime: State<'_, VirtualDisplayRuntime>,
    core: State<'_, Arc<PeerSpanCore>>,
) -> Result<AppSnapshot, String> {
    runtime.stop()?;
    core.snapshot().map_err(|error| error.to_string())
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

fn fingerprint(identity: &DeviceIdentity) -> String {
    fingerprint_public_key(&identity.signing_key.verifying_key().to_bytes())
}

fn public_key(identity: &DeviceIdentity) -> String {
    hex::encode(identity.signing_key.verifying_key().to_bytes())
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
                fingerprint: fingerprint(&identity),
                public_key: public_key(&identity),
            };
            let core = Arc::new(PeerSpanCore::load(local_device.clone(), &data_dir)?);
            if let Ok(applications) = applications::scan_installed_applications() {
                let _ = core.replace_scanned_applications(applications);
            }
            let mut preferences = core.snapshot()?.preferences;
            if preferences.streaming_backend != StreamingBackend::SunshineMoonlight {
                preferences.streaming_backend = StreamingBackend::SunshineMoonlight;
                core.update_preferences(preferences)?;
            }
            probe_video_capability(&core);
            probe_input_capability(&core);
            let resource_dir = app.path().resource_dir().ok();
            let gamestream = GameStreamRuntime::discover(&data_dir, resource_dir.as_deref());
            gamestream.apply_capability(&core);
            app.manage(VirtualDisplayRuntime::new(Arc::clone(&core)));
            let credentials = DeviceCredentials {
                device: local_device.clone(),
                signing_key: identity.signing_key,
            };
            match PairingRuntime::start(credentials.clone(), Arc::clone(&core)) {
                Ok(runtime) => {
                    mark_pairing_ready(&core);
                    app.manage(runtime);
                }
                Err(error) => mark_pairing_unavailable(&core, &error),
            }
            match ControlRuntime::start(credentials, Arc::clone(&core), Arc::clone(&gamestream)) {
                Ok(runtime) => {
                    mark_control_ready(&core);
                    app.manage(runtime);
                }
                Err(error) => mark_control_unavailable(&core, &error),
            }
            match DiscoveryRuntime::start(&local_device, Arc::clone(&core)) {
                Ok(runtime) => {
                    mark_discovery_ready(&core);
                    app.manage(runtime);
                }
                Err(error) => mark_discovery_unavailable(&core, &error),
            }
            app.manage(gamestream);
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
            end_display_session,
            start_virtual_display,
            stop_virtual_display,
            set_display_layout,
            scan_published_applications,
            save_published_application,
            remove_published_application,
            sync_application_catalogs,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run PeerSpan desktop application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_derived_from_public_key_and_grouped() {
        let identity = DeviceIdentity {
            device_id: Uuid::nil(),
            signing_key: ed25519_dalek::SigningKey::from_bytes(&[7_u8; 32]),
        };
        let value = fingerprint(&identity);
        assert_eq!(value.len(), 29);
        assert_eq!(value.split(' ').count(), 6);
        assert_eq!(value, fingerprint(&identity));
    }
}
