use crate::control::CONTROL_PORT;
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use peerspan_core::{Capability, DeviceStatus, LocalDevice, PairingCode, PeerDevice, PeerSpanCore};
use peerspan_protocol::PROTOCOL_VERSION;
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use spake2::{Ed25519Group, Identity, Password, Spake2};
use std::{
    io::{self, Read, Write},
    net::{IpAddr, SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

pub const PAIRING_PORT: u16 = 37_621;
const OFFER_LIFETIME: Duration = Duration::from_secs(120);
const MAX_ATTEMPTS: u8 = 5;
const MAX_FRAME_BYTES: usize = 64 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct DeviceCredentials {
    pub device: LocalDevice,
    pub signing_key: SigningKey,
}

pub struct PairingRuntime {
    credentials: DeviceCredentials,
    core: Arc<PeerSpanCore>,
    offer: Arc<Mutex<Option<ActiveOffer>>>,
    shutdown: Arc<AtomicBool>,
    worker: Mutex<Option<thread::JoinHandle<()>>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingOffer {
    pub code: String,
    pub expires_at_unix_ms: u64,
    pub attempts_remaining: u8,
}

struct ActiveOffer {
    code: String,
    expires_at: Instant,
    attempts_remaining: u8,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClientHello {
    device_id: Uuid,
    spake_message: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ServerHello {
    spake_message: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EncryptedEnvelope {
    nonce: Vec<u8>,
    ciphertext: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireIdentity {
    device_id: Uuid,
    name: String,
    platform: String,
    fingerprint: String,
    public_key: String,
    control_port: u16,
    pairing_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignedIdentityBody {
    identity: WireIdentity,
    transcript_hash: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignedIdentity {
    body: SignedIdentityBody,
    signature: Vec<u8>,
}

impl PairingRuntime {
    pub fn start(credentials: DeviceCredentials, core: Arc<PeerSpanCore>) -> Result<Self, String> {
        let listener = TcpListener::bind(("0.0.0.0", PAIRING_PORT))
            .map_err(|error| format!("cannot bind pairing port {PAIRING_PORT}: {error}"))?;
        Self::start_with_listener(credentials, core, listener)
    }

    fn start_with_listener(
        credentials: DeviceCredentials,
        core: Arc<PeerSpanCore>,
        listener: TcpListener,
    ) -> Result<Self, String> {
        listener
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        let offer = Arc::new(Mutex::new(None));
        let shutdown = Arc::new(AtomicBool::new(false));
        let thread_offer = Arc::clone(&offer);
        let thread_shutdown = Arc::clone(&shutdown);
        let thread_credentials = credentials.clone();
        let thread_core = Arc::clone(&core);
        let worker = thread::Builder::new()
            .name("peerspan-pairing".into())
            .spawn(move || {
                while !thread_shutdown.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, remote)) => {
                            if let Err(_error) = handle_incoming(
                                stream,
                                remote,
                                &thread_credentials,
                                &thread_core,
                                &thread_offer,
                            ) {
                                #[cfg(test)]
                                eprintln!("pairing server rejected connection: {_error}");
                            }
                        }
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(40));
                        }
                        Err(_) => thread::sleep(Duration::from_millis(100)),
                    }
                }
            })
            .map_err(|error| error.to_string())?;

        Ok(Self {
            credentials,
            core,
            offer,
            shutdown,
            worker: Mutex::new(Some(worker)),
        })
    }

    pub fn create_offer(&self) -> PairingOffer {
        let code = generate_pairing_code();
        let expires_at_unix_ms = unix_millis() + OFFER_LIFETIME.as_millis() as u64;
        let active = ActiveOffer {
            code: code.clone(),
            expires_at: Instant::now() + OFFER_LIFETIME,
            attempts_remaining: MAX_ATTEMPTS,
        };
        *self.offer.lock().expect("pairing offer lock poisoned") = Some(active);
        PairingOffer {
            code,
            expires_at_unix_ms,
            attempts_remaining: MAX_ATTEMPTS,
        }
    }

    pub fn pair_device(&self, peer_id: Uuid, code: &str) -> Result<PeerDevice, String> {
        PairingCode::parse(code).map_err(|error| error.to_string())?;
        let peer = self
            .core
            .snapshot()
            .map_err(|error| error.to_string())?
            .nearby_devices
            .into_iter()
            .find(|device| device.id == peer_id)
            .ok_or_else(|| "The selected device is no longer visible on the LAN".to_owned())?;
        pair_peer(&self.credentials, &self.core, peer, code)
    }
}

impl Drop for PairingRuntime {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Ok(mut worker) = self.worker.lock()
            && let Some(worker) = worker.take()
        {
            let _ = worker.join();
        }
    }
}

pub fn mark_pairing_ready(core: &PeerSpanCore) {
    let _ = core.set_secure_pairing_capability(Capability::ready(
        "SPAKE2 listener is active with signed Ed25519 device identities",
    ));
}

pub fn mark_pairing_unavailable(core: &PeerSpanCore, error: &str) {
    let _ = core.set_secure_pairing_capability(Capability::required(format!(
        "Secure pairing listener could not start: {error}"
    )));
}

pub fn fingerprint_public_key(public_key: &[u8; 32]) -> String {
    Sha256::digest(public_key)[..12]
        .chunks_exact(2)
        .map(hex::encode_upper)
        .collect::<Vec<_>>()
        .join(" ")
}

fn handle_incoming(
    mut stream: TcpStream,
    remote: SocketAddr,
    credentials: &DeviceCredentials,
    core: &PeerSpanCore,
    offer: &Mutex<Option<ActiveOffer>>,
) -> Result<(), String> {
    configure_stream(&stream)?;
    let hello: ClientHello = read_frame(&mut stream)?;
    if hello.device_id == credentials.device.id {
        return Err("self-pairing is not allowed".into());
    }
    let code = consume_offer_attempt(offer)?;
    let (spake, server_message) = Spake2::<Ed25519Group>::start_b(
        &Password::new(code.as_bytes()),
        &Identity::new(hello.device_id.as_bytes()),
        &Identity::new(credentials.device.id.as_bytes()),
    );
    write_frame(
        &mut stream,
        &ServerHello {
            spake_message: server_message.clone(),
        },
    )?;
    let shared_key = spake
        .finish(&hello.spake_message)
        .map_err(|_| "invalid SPAKE2 client message".to_owned())?;
    let transcript = transcript_hash(&hello.spake_message, &server_message);
    let cipher_key = derive_cipher_key(&shared_key);
    let client_aad = pairing_aad(hello.device_id, credentials.device.id, "client");
    let client_envelope: EncryptedEnvelope = read_frame(&mut stream)?;
    let client_signed: SignedIdentity = decrypt_json(&cipher_key, &client_aad, &client_envelope)?;
    verify_signed_identity(&client_signed, hello.device_id, &transcript)?;
    verify_discovered_identity(core, &client_signed.body.identity)?;

    let peer = wire_identity_to_peer(&client_signed.body.identity, vec![remote.ip().to_string()]);
    core.trust_device(peer.clone())
        .map_err(|error| error.to_string())?;

    let response = sign_identity(credentials, transcript.clone())?;
    let server_aad = pairing_aad(hello.device_id, credentials.device.id, "server");
    let envelope = encrypt_json(&cipher_key, &server_aad, &response)?;
    write_frame(&mut stream, &envelope)?;
    *offer.lock().map_err(|_| "pairing offer lock poisoned")? = None;
    Ok(())
}

fn pair_peer(
    credentials: &DeviceCredentials,
    core: &PeerSpanCore,
    peer: PeerDevice,
    code: &str,
) -> Result<PeerDevice, String> {
    let mut stream = connect_to_peer(&peer)?;
    configure_stream(&stream)?;
    let (spake, client_message) = Spake2::<Ed25519Group>::start_a(
        &Password::new(code.as_bytes()),
        &Identity::new(credentials.device.id.as_bytes()),
        &Identity::new(peer.id.as_bytes()),
    );
    write_frame(
        &mut stream,
        &ClientHello {
            device_id: credentials.device.id,
            spake_message: client_message.clone(),
        },
    )?;
    let server_hello: ServerHello = read_frame(&mut stream)?;
    let shared_key = spake
        .finish(&server_hello.spake_message)
        .map_err(|_| "invalid SPAKE2 server message".to_owned())?;
    let transcript = transcript_hash(&client_message, &server_hello.spake_message);
    let cipher_key = derive_cipher_key(&shared_key);
    let signed = sign_identity(credentials, transcript.clone())?;
    let client_aad = pairing_aad(credentials.device.id, peer.id, "client");
    let envelope = encrypt_json(&cipher_key, &client_aad, &signed)?;
    write_frame(&mut stream, &envelope)?;

    let response_envelope: EncryptedEnvelope = read_frame(&mut stream)
        .map_err(|_| "Pairing failed: the code may be incorrect or expired".to_owned())?;
    let server_aad = pairing_aad(credentials.device.id, peer.id, "server");
    let response: SignedIdentity = decrypt_json(&cipher_key, &server_aad, &response_envelope)
        .map_err(|_| "Pairing failed: the code may be incorrect or expired".to_owned())?;
    verify_signed_identity(&response, peer.id, &transcript)?;
    if response.body.identity.public_key != peer.public_key
        || response.body.identity.fingerprint != peer.fingerprint
    {
        return Err("The peer identity changed after discovery; pairing was aborted".into());
    }

    let trusted = wire_identity_to_peer(&response.body.identity, peer.addresses.clone());
    core.trust_device(trusted.clone())
        .map_err(|error| error.to_string())?;
    Ok(trusted)
}

fn sign_identity(
    credentials: &DeviceCredentials,
    transcript_hash: Vec<u8>,
) -> Result<SignedIdentity, String> {
    let body = SignedIdentityBody {
        identity: WireIdentity {
            device_id: credentials.device.id,
            name: credentials.device.name.clone(),
            platform: credentials.device.platform.clone(),
            fingerprint: credentials.device.fingerprint.clone(),
            public_key: credentials.device.public_key.clone(),
            control_port: CONTROL_PORT,
            pairing_port: PAIRING_PORT,
        },
        transcript_hash,
    };
    let encoded = serde_json::to_vec(&body).map_err(|error| error.to_string())?;
    Ok(SignedIdentity {
        signature: credentials.signing_key.sign(&encoded).to_bytes().to_vec(),
        body,
    })
}

fn verify_signed_identity(
    signed: &SignedIdentity,
    expected_id: Uuid,
    expected_transcript: &[u8],
) -> Result<(), String> {
    if signed.body.identity.device_id != expected_id
        || signed.body.transcript_hash != expected_transcript
    {
        return Err("pairing identity is not bound to this handshake".into());
    }
    let public_key = decode_public_key(&signed.body.identity.public_key)?;
    if fingerprint_public_key(&public_key) != signed.body.identity.fingerprint {
        return Err("pairing identity fingerprint does not match its public key".into());
    }
    let verifying_key =
        VerifyingKey::from_bytes(&public_key).map_err(|_| "invalid Ed25519 public key")?;
    let signature = Signature::from_slice(&signed.signature).map_err(|_| "invalid signature")?;
    let encoded = serde_json::to_vec(&signed.body).map_err(|error| error.to_string())?;
    verifying_key
        .verify(&encoded, &signature)
        .map_err(|_| "device identity signature verification failed".into())
}

fn verify_discovered_identity(core: &PeerSpanCore, identity: &WireIdentity) -> Result<(), String> {
    if let Some(discovered) = core
        .snapshot()
        .map_err(|error| error.to_string())?
        .nearby_devices
        .iter()
        .find(|device| device.id == identity.device_id)
        && (discovered.public_key != identity.public_key
            || discovered.fingerprint != identity.fingerprint)
    {
        return Err("incoming pairing identity differs from its mDNS advertisement".into());
    }
    Ok(())
}

fn wire_identity_to_peer(identity: &WireIdentity, addresses: Vec<String>) -> PeerDevice {
    PeerDevice {
        id: identity.device_id,
        name: identity.name.clone(),
        platform: identity.platform.clone(),
        fingerprint: identity.fingerprint.clone(),
        public_key: identity.public_key.clone(),
        status: DeviceStatus::Online,
        trusted: true,
        latency_ms: None,
        last_seen_unix_ms: unix_millis(),
        addresses,
        control_port: identity.control_port,
        pairing_port: identity.pairing_port,
        protocol_version: PROTOCOL_VERSION,
    }
}

fn connect_to_peer(peer: &PeerDevice) -> Result<TcpStream, String> {
    let mut failures = Vec::new();
    for address in &peer.addresses {
        let Ok(ip) = address.parse::<IpAddr>() else {
            continue;
        };
        let endpoint = SocketAddr::new(ip, peer.pairing_port);
        match TcpStream::connect_timeout(&endpoint, Duration::from_secs(2)) {
            Ok(stream) => return Ok(stream),
            Err(error) => failures.push(format!("{endpoint}: {error}")),
        }
    }
    Err(format!(
        "Could not reach the peer pairing service{}",
        if failures.is_empty() {
            String::new()
        } else {
            format!(": {}", failures.join("; "))
        }
    ))
}

fn configure_stream(stream: &TcpStream) -> Result<(), String> {
    stream
        .set_nonblocking(false)
        .map_err(|error| error.to_string())?;
    stream
        .set_nodelay(true)
        .map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(IO_TIMEOUT))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(IO_TIMEOUT))
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn consume_offer_attempt(offer: &Mutex<Option<ActiveOffer>>) -> Result<String, String> {
    let mut guard = offer.lock().map_err(|_| "pairing offer lock poisoned")?;
    let active = guard
        .as_mut()
        .ok_or_else(|| "No local pairing offer is active".to_owned())?;
    if Instant::now() >= active.expires_at || active.attempts_remaining == 0 {
        *guard = None;
        return Err("The local pairing offer expired".into());
    }
    active.attempts_remaining -= 1;
    Ok(active.code.clone())
}

fn generate_pairing_code() -> String {
    const RANGE: u32 = 1_000_000;
    const ZONE: u32 = u32::MAX - (u32::MAX % RANGE);
    let value = loop {
        let candidate = OsRng.next_u32();
        if candidate < ZONE {
            break candidate % RANGE;
        }
    };
    format!("{value:06}")
}

fn derive_cipher_key(shared_key: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"PeerSpan pairing cipher v1\0");
    hasher.update(shared_key);
    hasher.finalize().into()
}

fn transcript_hash(client_message: &[u8], server_message: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(b"PeerSpan pairing transcript v1\0");
    hasher.update((client_message.len() as u32).to_be_bytes());
    hasher.update(client_message);
    hasher.update((server_message.len() as u32).to_be_bytes());
    hasher.update(server_message);
    hasher.finalize().to_vec()
}

fn pairing_aad(client_id: Uuid, server_id: Uuid, direction: &str) -> Vec<u8> {
    format!("PeerSpan/v1/{client_id}/{server_id}/{direction}").into_bytes()
}

fn encrypt_json<T: Serialize>(
    key: &[u8; 32],
    aad: &[u8],
    value: &T,
) -> Result<EncryptedEnvelope, String> {
    let plaintext = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    let mut nonce = [0_u8; 24];
    OsRng.fill_bytes(&mut nonce);
    let nonce_value: XNonce = nonce.into();
    let cipher = XChaCha20Poly1305::new(key.into());
    let ciphertext = cipher
        .encrypt(
            &nonce_value,
            Payload {
                msg: &plaintext,
                aad,
            },
        )
        .map_err(|_| "pairing payload encryption failed".to_owned())?;
    Ok(EncryptedEnvelope {
        nonce: nonce.to_vec(),
        ciphertext,
    })
}

fn decrypt_json<T: DeserializeOwned>(
    key: &[u8; 32],
    aad: &[u8],
    envelope: &EncryptedEnvelope,
) -> Result<T, String> {
    let nonce: [u8; 24] = envelope
        .nonce
        .as_slice()
        .try_into()
        .map_err(|_| "invalid pairing nonce")?;
    let nonce_value: XNonce = nonce.into();
    let cipher = XChaCha20Poly1305::new(key.into());
    let plaintext = cipher
        .decrypt(
            &nonce_value,
            Payload {
                msg: &envelope.ciphertext,
                aad,
            },
        )
        .map_err(|_| "pairing authentication failed".to_owned())?;
    serde_json::from_slice(&plaintext).map_err(|error| error.to_string())
}

fn write_frame<T: Serialize>(stream: &mut TcpStream, value: &T) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    if bytes.len() > MAX_FRAME_BYTES {
        return Err("pairing frame exceeds the size limit".into());
    }
    stream
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .and_then(|_| stream.write_all(&bytes))
        .map_err(|error| error.to_string())
}

fn read_frame<T: DeserializeOwned>(stream: &mut TcpStream) -> Result<T, String> {
    let mut length = [0_u8; 4];
    stream
        .read_exact(&mut length)
        .map_err(|error| error.to_string())?;
    let length = u32::from_be_bytes(length) as usize;
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err("invalid pairing frame length".into());
    }
    let mut bytes = vec![0_u8; length];
    stream
        .read_exact(&mut bytes)
        .map_err(|error| error.to_string())?;
    serde_json::from_slice(&bytes).map_err(|error| error.to_string())
}

fn decode_public_key(value: &str) -> Result<[u8; 32], String> {
    hex::decode(value)
        .map_err(|_| "public key is not hexadecimal".to_owned())?
        .try_into()
        .map_err(|_| "public key has the wrong length".to_owned())
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn credentials(seed: u8, name: &str) -> DeviceCredentials {
        let signing_key = SigningKey::from_bytes(&[seed; 32]);
        let public_key = signing_key.verifying_key().to_bytes();
        DeviceCredentials {
            device: LocalDevice {
                id: Uuid::new_v4(),
                name: name.into(),
                platform: "Windows test".into(),
                fingerprint: fingerprint_public_key(&public_key),
                public_key: hex::encode(public_key),
            },
            signing_key,
        }
    }

    #[test]
    fn generated_codes_are_always_six_digits() {
        for _ in 0..100 {
            let code = generate_pairing_code();
            assert!(PairingCode::parse(code).is_ok());
        }
    }

    #[test]
    fn signed_identity_detects_tampering() {
        let credentials = credentials(7, "A");
        let transcript = vec![9; 32];
        let mut signed = sign_identity(&credentials, transcript.clone()).unwrap();
        verify_signed_identity(&signed, credentials.device.id, &transcript).unwrap();
        signed.body.identity.name = "Mallory".into();
        assert!(verify_signed_identity(&signed, credentials.device.id, &transcript).is_err());
    }

    #[test]
    fn local_pairing_exchanges_and_persists_both_identities() {
        let credentials_a = credentials(11, "A");
        let credentials_b = credentials(22, "B");
        let data_a = std::env::temp_dir().join(format!("peerspan-pair-a-{}", Uuid::new_v4()));
        let data_b = std::env::temp_dir().join(format!("peerspan-pair-b-{}", Uuid::new_v4()));
        let core_a = Arc::new(
            PeerSpanCore::load(credentials_a.device.clone(), &data_a).expect("core A should load"),
        );
        let core_b = Arc::new(
            PeerSpanCore::load(credentials_b.device.clone(), &data_b).expect("core B should load"),
        );
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let runtime_b = PairingRuntime::start_with_listener(
            credentials_b.clone(),
            Arc::clone(&core_b),
            listener,
        )
        .unwrap();
        let offer = runtime_b.create_offer();
        let peer_b = PeerDevice {
            id: credentials_b.device.id,
            name: credentials_b.device.name.clone(),
            platform: credentials_b.device.platform.clone(),
            fingerprint: credentials_b.device.fingerprint.clone(),
            public_key: credentials_b.device.public_key.clone(),
            status: DeviceStatus::Online,
            trusted: false,
            latency_ms: None,
            last_seen_unix_ms: unix_millis(),
            addresses: vec!["127.0.0.1".into()],
            control_port: port.saturating_add(1),
            pairing_port: port,
            protocol_version: PROTOCOL_VERSION,
        };
        core_a.upsert_nearby_device(peer_b.clone()).unwrap();

        let wrong_value = (offer.code.parse::<u32>().unwrap() + 1) % 1_000_000;
        let wrong_code = format!("{wrong_value:06}");
        assert!(pair_peer(&credentials_a, &core_a, peer_b.clone(), &wrong_code).is_err());
        assert!(core_a.snapshot().unwrap().trusted_devices.is_empty());
        assert!(core_b.snapshot().unwrap().trusted_devices.is_empty());

        let trusted_b = pair_peer(&credentials_a, &core_a, peer_b, &offer.code).unwrap();
        assert_eq!(trusted_b.id, credentials_b.device.id);
        assert_eq!(core_a.snapshot().unwrap().trusted_devices.len(), 1);
        assert_eq!(core_b.snapshot().unwrap().trusted_devices.len(), 1);
        assert_eq!(
            core_b.snapshot().unwrap().trusted_devices[0].id,
            credentials_a.device.id
        );

        drop(runtime_b);
        let _ = fs::remove_dir_all(data_a);
        let _ = fs::remove_dir_all(data_b);
    }
}
