use crate::{control::CONTROL_PORT, pairing::PAIRING_PORT};
use mdns_sd::{ResolvedService, ServiceDaemon, ServiceEvent, ServiceInfo, TxtProperties};
use peerspan_core::{Capability, DeviceStatus, LocalDevice, PeerDevice, PeerSpanCore};
use peerspan_protocol::PROTOCOL_VERSION;
use std::{
    collections::HashMap,
    sync::Arc,
    thread,
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

const SERVICE_TYPE: &str = "_peerspan._tcp.local.";
pub struct DiscoveryRuntime {
    daemon: ServiceDaemon,
    service_fullname: String,
}

impl DiscoveryRuntime {
    pub fn start(local: &LocalDevice, core: Arc<PeerSpanCore>) -> Result<Self, String> {
        let daemon = ServiceDaemon::new().map_err(|error| error.to_string())?;
        let id = local.id.to_string();
        let protocol = PROTOCOL_VERSION.to_string();
        let pairing_port = PAIRING_PORT.to_string();
        let hostname = format!("peerspan-{}.local.", &id[..8]);
        let instance_name = format!("{} {}", local.name, &id[..6]);
        let properties = [
            ("id", id.as_str()),
            ("name", local.name.as_str()),
            ("platform", local.platform.as_str()),
            ("fingerprint", local.fingerprint.as_str()),
            ("public_key", local.public_key.as_str()),
            ("protocol", protocol.as_str()),
            ("pairing_port", pairing_port.as_str()),
        ];
        let service = ServiceInfo::new(
            SERVICE_TYPE,
            instance_name.as_str(),
            hostname.as_str(),
            "",
            CONTROL_PORT,
            &properties[..],
        )
        .map_err(|error| error.to_string())?
        .enable_addr_auto();
        let service_fullname = service.get_fullname().to_owned();
        daemon
            .register(service)
            .map_err(|error| error.to_string())?;
        let receiver = daemon
            .browse(SERVICE_TYPE)
            .map_err(|error| error.to_string())?;

        thread::Builder::new()
            .name("peerspan-mdns".into())
            .spawn(move || {
                let mut resolved_ids = HashMap::<String, Uuid>::new();
                while let Ok(event) = receiver.recv() {
                    match event {
                        ServiceEvent::ServiceResolved(info) => {
                            if let Some(device) = parse_service(&info) {
                                resolved_ids.insert(info.get_fullname().to_owned(), device.id);
                                let _ = core.upsert_nearby_device(device);
                            }
                        }
                        ServiceEvent::ServiceRemoved(_, fullname) => {
                            if let Some(device_id) = resolved_ids.remove(&fullname) {
                                let _ = core.remove_nearby_device(device_id);
                            }
                        }
                        _ => {}
                    }
                }
            })
            .map_err(|error| error.to_string())?;

        Ok(Self {
            daemon,
            service_fullname,
        })
    }
}

impl Drop for DiscoveryRuntime {
    fn drop(&mut self) {
        let _ = self.daemon.unregister(&self.service_fullname);
        let _ = self.daemon.stop_browse(SERVICE_TYPE);
        let _ = self.daemon.shutdown();
    }
}

pub fn mark_discovery_ready(core: &PeerSpanCore) {
    let _ = core.set_discovery_capability(Capability::ready(
        "mDNS advertisement and continuous LAN browsing are active",
    ));
}

pub fn mark_discovery_unavailable(core: &PeerSpanCore, error: &str) {
    let _ = core.set_discovery_capability(Capability::required(format!(
        "mDNS could not start: {error}"
    )));
}

fn parse_service(info: &ResolvedService) -> Option<PeerDevice> {
    parse_service_parts(
        info.get_properties(),
        info.get_addresses()
            .iter()
            .map(ToString::to_string)
            .collect(),
        info.get_port(),
    )
}

fn parse_service_parts(
    properties: &TxtProperties,
    addresses: Vec<String>,
    control_port: u16,
) -> Option<PeerDevice> {
    let id = Uuid::parse_str(properties.get_property_val_str("id")?).ok()?;
    let name = properties.get_property_val_str("name")?.trim();
    if name.is_empty() {
        return None;
    }
    let protocol_version = properties
        .get_property_val_str("protocol")?
        .parse::<u16>()
        .ok()?;
    if protocol_version != PROTOCOL_VERSION {
        return None;
    }

    Some(PeerDevice {
        id,
        name: name.to_owned(),
        platform: properties
            .get_property_val_str("platform")
            .unwrap_or("Windows")
            .to_owned(),
        fingerprint: properties
            .get_property_val_str("fingerprint")
            .unwrap_or("Unavailable")
            .to_owned(),
        public_key: properties.get_property_val_str("public_key")?.to_owned(),
        status: DeviceStatus::Online,
        trusted: false,
        latency_ms: None,
        last_seen_unix_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
        addresses,
        control_port,
        pairing_port: properties
            .get_property_val_str("pairing_port")
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or(PAIRING_PORT),
        protocol_version,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, time::Duration};

    #[test]
    fn parses_compatible_peer_advertisement() {
        let properties = [
            ("id", "12345678-90ab-cdef-1234-567890abcdef"),
            ("name", "Studio PC"),
            ("platform", "Windows 11"),
            ("fingerprint", "1234 5678 90AB"),
            (
                "public_key",
                "0707070707070707070707070707070707070707070707070707070707070707",
            ),
            ("protocol", "3"),
            ("pairing_port", "37621"),
        ];
        let info = ServiceInfo::new(
            SERVICE_TYPE,
            "Studio PC",
            "studio.local.",
            "192.168.1.20",
            CONTROL_PORT,
            &properties[..],
        )
        .unwrap();

        let device = parse_service_parts(
            info.get_properties(),
            info.get_addresses()
                .iter()
                .map(ToString::to_string)
                .collect(),
            info.get_port(),
        )
        .expect("valid advertisement");
        assert_eq!(device.name, "Studio PC");
        assert_eq!(device.control_port, CONTROL_PORT);
        assert_eq!(device.pairing_port, PAIRING_PORT);
        assert_eq!(device.addresses, ["192.168.1.20"]);
    }

    #[test]
    fn ignores_incompatible_protocol_versions() {
        let properties = [
            ("id", "12345678-90ab-cdef-1234-567890abcdef"),
            ("name", "Old PC"),
            ("protocol", "99"),
        ];
        let info = ServiceInfo::new(
            SERVICE_TYPE,
            "Old PC",
            "old.local.",
            "192.168.1.21",
            CONTROL_PORT,
            &properties[..],
        )
        .unwrap();
        assert!(
            parse_service_parts(
                info.get_properties(),
                info.get_addresses()
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
                info.get_port(),
            )
            .is_none()
        );
    }

    #[test]
    #[ignore = "requires a working local multicast socket"]
    fn two_local_daemons_discover_each_other() {
        let id_a = Uuid::new_v4();
        let id_b = Uuid::new_v4();
        let device_a = LocalDevice {
            id: id_a,
            name: format!("PeerSpan Test A {}", &id_a.simple().to_string()[..6]),
            platform: "Windows test".into(),
            fingerprint: "TEST A".into(),
            public_key: hex::encode([1_u8; 32]),
        };
        let device_b = LocalDevice {
            id: id_b,
            name: format!("PeerSpan Test B {}", &id_b.simple().to_string()[..6]),
            platform: "Windows test".into(),
            fingerprint: "TEST B".into(),
            public_key: hex::encode([2_u8; 32]),
        };
        let data_a = std::env::temp_dir().join(format!("peerspan-discovery-{id_a}"));
        let data_b = std::env::temp_dir().join(format!("peerspan-discovery-{id_b}"));
        let core_a = Arc::new(PeerSpanCore::load(device_a.clone(), &data_a).unwrap());
        let core_b = Arc::new(PeerSpanCore::load(device_b.clone(), &data_b).unwrap());
        let _runtime_a = DiscoveryRuntime::start(&device_a, Arc::clone(&core_a)).unwrap();
        let _runtime_b = DiscoveryRuntime::start(&device_b, Arc::clone(&core_b)).unwrap();

        let discovered = (0..50).any(|_| {
            let a_found = core_a
                .snapshot()
                .unwrap()
                .nearby_devices
                .iter()
                .any(|device| device.id == id_b);
            let b_found = core_b
                .snapshot()
                .unwrap()
                .nearby_devices
                .iter()
                .any(|device| device.id == id_a);
            if !(a_found && b_found) {
                thread::sleep(Duration::from_millis(100));
            }
            a_found && b_found
        });

        assert!(
            discovered,
            "both mDNS daemons should discover the other identity"
        );
        let _ = fs::remove_dir_all(data_a);
        let _ = fs::remove_dir_all(data_b);
    }
}
