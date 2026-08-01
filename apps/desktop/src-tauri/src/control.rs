use crate::{clipboard::ClipboardSync, input::InputInjector, pairing::DeviceCredentials};
use ed25519_dalek::{Signer as _, SigningKey};
use peerspan_core::{
    Capability, CapabilityState, DeviceStatus, DisplaySession, PeerDevice, PeerSpanCore,
    QualityMode, SessionDirection, SessionState, parse_release_shortcut,
};
use peerspan_media::{MediaError, MediaKeyMaterial, UdpMediaReceiver, UdpMediaSender};
use peerspan_protocol::{
    ControlMessage, DisplayDecision, DisplayOffer, Heartbeat, Hello, InputEvent, PROTOCOL_VERSION,
    PointerButton, SessionEnd, StreamReady, VideoCodec,
};
use peerspan_video::{
    DecoderConfig, EncoderConfig, NativeVideoReceiver, ReceiverInputEvent, ReceiverPointerButton,
    ReceiverReleaseShortcut, SharedIddFrameEncoder, VideoError,
};
use rcgen::{
    CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, KeyPair,
    KeyUsagePurpose, PKCS_ED25519, RemoteKeyPair, SerialNumber, date_time_ymd,
};
use rustls::{
    ClientConfig, ClientConnection, DigitallySignedStruct,
    DistinguishedName as TlsDistinguishedName, Error as TlsError, ServerConfig, ServerConnection,
    SignatureAlgorithm, SignatureScheme, StreamOwned,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::{WebPkiSupportedAlgorithms, ring, verify_tls12_signature, verify_tls13_signature},
    pki_types::{CertificateDer, ServerName, SubjectPublicKeyInfoDer, UnixTime},
    server::danger::{ClientCertVerified, ClientCertVerifier},
    sign::{CertifiedKey, SingleCertAndKey},
    version::TLS13,
};
use serde::{Serialize, de::DeserializeOwned};
#[cfg(test)]
use std::time::{SystemTime, UNIX_EPOCH};
use std::{
    collections::HashMap,
    fmt,
    io::{self, Read, Write},
    net::{IpAddr, SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;
use x509_parser::{oid_registry::OID_SIG_ED25519, parse_x509_certificate};

pub const CONTROL_PORT: u16 = 37_622;
const ALPN_PROTOCOL: &[u8] = b"peerspan-control/4";
const MEDIA_EXPORTER_LABEL: &[u8] = b"EXPORTER-PeerSpan-media-v4";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const SESSION_IO_TIMEOUT: Duration = Duration::from_millis(25);
const HEARTBEAT_INTERVAL: Duration = Duration::from_millis(500);
const SESSION_LIVENESS_TIMEOUT: Duration = Duration::from_secs(1);
const MEDIA_START_TIMEOUT: Duration = Duration::from_secs(8);
const FRAME_ACQUIRE_TIMEOUT: Duration = Duration::from_millis(4);
const MAX_FRAME_BYTES: usize = 1024 * 1024 + 64 * 1024;
const MAX_SERVER_WORKERS: usize = 32;

type ClientTlsStream = StreamOwned<ClientConnection, TcpStream>;
type ServerTlsStream = StreamOwned<ServerConnection, TcpStream>;
type ActiveMediaMap = Arc<Mutex<HashMap<Uuid, Arc<Mutex<ActiveMediaEndpoint>>>>>;

enum ActiveMediaEndpoint {
    Sender(Box<UdpMediaSender>),
    Receiver(Box<UdpMediaReceiver>),
}

enum ServerSessionEvent {
    Input(InputEvent),
    StreamReady,
}

impl ActiveMediaEndpoint {
    fn local_addr(&self) -> Result<SocketAddr, String> {
        match self {
            Self::Sender(sender) => sender.local_addr().map_err(|error| error.to_string()),
            Self::Receiver(receiver) => receiver.local_addr().map_err(|error| error.to_string()),
        }
    }
}

struct IncomingContext<'a> {
    credentials: &'a DeviceCredentials,
    core: Arc<PeerSpanCore>,
    config: Arc<ServerConfig>,
    runtime_shutdown: &'a AtomicBool,
    active_signals: &'a Mutex<HashMap<Uuid, Arc<AtomicBool>>>,
    active_media: &'a ActiveMediaMap,
    real_media_workers: bool,
}

pub struct ControlRuntime {
    credentials: DeviceCredentials,
    core: Arc<PeerSpanCore>,
    tls_identity: TlsIdentity,
    shutdown: Arc<AtomicBool>,
    active_signals: Arc<Mutex<HashMap<Uuid, Arc<AtomicBool>>>>,
    active_media: ActiveMediaMap,
    listener_worker: Mutex<Option<thread::JoinHandle<()>>>,
    server_workers: Arc<Mutex<Vec<thread::JoinHandle<()>>>>,
    client_workers: Mutex<Vec<thread::JoinHandle<()>>>,
    real_media_workers: bool,
}

#[derive(Clone)]
struct TlsIdentity {
    certificate: CertificateDer<'static>,
    signing_key: Arc<dyn rustls::sign::SigningKey>,
}

struct RcgenEd25519Key {
    signing_key: SigningKey,
    public_key: [u8; 32],
}

impl RemoteKeyPair for RcgenEd25519Key {
    fn public_key(&self) -> &[u8] {
        &self.public_key
    }

    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, rcgen::Error> {
        Ok(self.signing_key.sign(message).to_bytes().to_vec())
    }

    fn algorithm(&self) -> &'static rcgen::SignatureAlgorithm {
        &PKCS_ED25519
    }
}

struct DalekTlsSigningKey {
    signing_key: SigningKey,
    public_key_der: Vec<u8>,
}

impl fmt::Debug for DalekTlsSigningKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DalekTlsSigningKey")
            .finish_non_exhaustive()
    }
}

impl rustls::sign::SigningKey for DalekTlsSigningKey {
    fn choose_scheme(&self, offered: &[SignatureScheme]) -> Option<Box<dyn rustls::sign::Signer>> {
        offered.contains(&SignatureScheme::ED25519).then(|| {
            Box::new(DalekTlsSigner(self.signing_key.clone())) as Box<dyn rustls::sign::Signer>
        })
    }

    fn public_key(&self) -> Option<SubjectPublicKeyInfoDer<'_>> {
        Some(SubjectPublicKeyInfoDer::from(
            self.public_key_der.as_slice(),
        ))
    }

    fn algorithm(&self) -> SignatureAlgorithm {
        SignatureAlgorithm::ED25519
    }
}

struct DalekTlsSigner(SigningKey);

impl fmt::Debug for DalekTlsSigner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DalekTlsSigner")
            .finish_non_exhaustive()
    }
}

impl rustls::sign::Signer for DalekTlsSigner {
    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, TlsError> {
        Ok(self.0.sign(message).to_bytes().to_vec())
    }

    fn scheme(&self) -> SignatureScheme {
        SignatureScheme::ED25519
    }
}

impl TlsIdentity {
    fn from_credentials(credentials: &DeviceCredentials) -> Result<Self, String> {
        let public_key = credentials.signing_key.verifying_key().to_bytes();
        let key_pair = KeyPair::from_remote(Box::new(RcgenEd25519Key {
            signing_key: credentials.signing_key.clone(),
            public_key,
        }))
        .map_err(|error| format!("could not create the TLS signing key: {error}"))?;

        let dns_name = tls_server_name(credentials.device.id);
        let mut parameters = CertificateParams::new(vec![dns_name])
            .map_err(|error| format!("could not create TLS certificate parameters: {error}"))?;
        parameters.not_before = date_time_ymd(2025, 1, 1);
        parameters.not_after = date_time_ymd(2045, 1, 1);
        parameters.serial_number = Some(SerialNumber::from_slice(credentials.device.id.as_bytes()));
        let mut distinguished_name = DistinguishedName::new();
        distinguished_name.push(
            DnType::CommonName,
            format!("PeerSpan {}", credentials.device.id),
        );
        parameters.distinguished_name = distinguished_name;
        parameters.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        parameters.extended_key_usages = vec![
            ExtendedKeyUsagePurpose::ClientAuth,
            ExtendedKeyUsagePurpose::ServerAuth,
        ];
        let certificate = parameters
            .self_signed(&key_pair)
            .map_err(|error| format!("could not self-sign the TLS certificate: {error}"))?;
        let signing_key = Arc::new(DalekTlsSigningKey {
            signing_key: credentials.signing_key.clone(),
            public_key_der: key_pair.public_key_der(),
        });

        Ok(Self {
            certificate: certificate.der().clone(),
            signing_key,
        })
    }

    fn certificate_chain(&self) -> Vec<CertificateDer<'static>> {
        vec![self.certificate.clone()]
    }

    fn certified_key(&self) -> CertifiedKey {
        CertifiedKey::new(self.certificate_chain(), Arc::clone(&self.signing_key))
    }
}

impl ControlRuntime {
    pub fn start(credentials: DeviceCredentials, core: Arc<PeerSpanCore>) -> Result<Self, String> {
        let listener = TcpListener::bind(("0.0.0.0", CONTROL_PORT))
            .map_err(|error| format!("cannot bind TLS control port {CONTROL_PORT}: {error}"))?;
        Self::start_with_listener(credentials, core, listener)
    }

    fn start_with_listener(
        credentials: DeviceCredentials,
        core: Arc<PeerSpanCore>,
        listener: TcpListener,
    ) -> Result<Self, String> {
        Self::start_with_listener_mode(credentials, core, listener, cfg!(not(test)))
    }

    fn start_with_listener_mode(
        credentials: DeviceCredentials,
        core: Arc<PeerSpanCore>,
        listener: TcpListener,
        real_media_workers: bool,
    ) -> Result<Self, String> {
        listener
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        let tls_identity = TlsIdentity::from_credentials(&credentials)?;
        let server_config = Arc::new(server_config(&tls_identity, Arc::clone(&core))?);
        let shutdown = Arc::new(AtomicBool::new(false));
        let active_signals = Arc::new(Mutex::new(HashMap::new()));
        let active_media = Arc::new(Mutex::new(HashMap::new()));
        let server_workers = Arc::new(Mutex::new(Vec::new()));

        let listener_shutdown = Arc::clone(&shutdown);
        let listener_signals = Arc::clone(&active_signals);
        let listener_media = Arc::clone(&active_media);
        let listener_workers = Arc::clone(&server_workers);
        let listener_credentials = credentials.clone();
        let listener_core = Arc::clone(&core);
        let listener_config = Arc::clone(&server_config);
        let listener_worker = thread::Builder::new()
            .name("peerspan-control-listener".into())
            .spawn(move || {
                while !listener_shutdown.load(Ordering::Relaxed) {
                    reap_finished_workers(&listener_workers);
                    match listener.accept() {
                        Ok((stream, remote)) => {
                            let can_accept = listener_workers
                                .lock()
                                .map(|workers| workers.len() < MAX_SERVER_WORKERS)
                                .unwrap_or(false);
                            if !can_accept {
                                continue;
                            }
                            let worker_shutdown = Arc::clone(&listener_shutdown);
                            let worker_signals = Arc::clone(&listener_signals);
                            let worker_media = Arc::clone(&listener_media);
                            let worker_credentials = listener_credentials.clone();
                            let worker_core = Arc::clone(&listener_core);
                            let worker_config = Arc::clone(&listener_config);
                            let worker = thread::Builder::new()
                                .name("peerspan-control-peer".into())
                                .spawn(move || {
                                    if let Err(_error) = handle_incoming(
                                        stream,
                                        remote,
                                        IncomingContext {
                                            credentials: &worker_credentials,
                                            core: worker_core,
                                            config: worker_config,
                                            runtime_shutdown: &worker_shutdown,
                                            active_signals: &worker_signals,
                                            active_media: &worker_media,
                                            real_media_workers,
                                        },
                                    ) {
                                        #[cfg(test)]
                                        eprintln!("TLS control connection failed: {_error}");
                                    }
                                });
                            if let Ok(worker) = worker
                                && let Ok(mut workers) = listener_workers.lock()
                            {
                                workers.push(worker);
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
            tls_identity,
            shutdown,
            active_signals,
            active_media,
            listener_worker: Mutex::new(Some(listener_worker)),
            server_workers,
            client_workers: Mutex::new(Vec::new()),
            real_media_workers,
        })
    }

    pub fn request_display_session(&self, peer_id: Uuid) -> Result<DisplaySession, String> {
        reap_finished_client_workers(&self.client_workers);
        let snapshot = self.core.snapshot().map_err(|error| error.to_string())?;
        require_ready(
            "virtual display",
            &snapshot.capabilities.virtual_display.state,
            &snapshot.capabilities.virtual_display.detail,
        )?;
        require_ready(
            "media pipeline",
            &snapshot.capabilities.media_pipeline.state,
            &snapshot.capabilities.media_pipeline.detail,
        )?;
        require_ready(
            "input injection",
            &snapshot.capabilities.input_injection.state,
            &snapshot.capabilities.input_injection.detail,
        )?;
        if snapshot.active_session.is_some() {
            return Err("Another PeerSpan display session is already active".into());
        }
        let peer = resolve_online_trusted_peer(&snapshot, peer_id)?;
        let stream = connect_to_peer(&peer)?;
        configure_stream(&stream, HANDSHAKE_TIMEOUT)?;
        let mut channel = connect_tls(stream, &self.credentials, &self.tls_identity, &peer)?;

        let session = DisplaySession {
            id: Uuid::new_v4(),
            peer_id,
            direction: SessionDirection::Sending,
            state: SessionState::Negotiating,
            width_px: 1920,
            height_px: 1080,
            refresh_hz: 60,
            latency_ms: None,
        };
        let encoder_config = EncoderConfig {
            width: session.width_px,
            height: session.height_px,
            frames_per_second: u32::from(session.refresh_hz),
            bitrate: bitrate_for_quality(snapshot.preferences.quality),
        };
        let input_injector = if self.real_media_workers {
            Some(InputInjector::open()?)
        } else {
            None
        };
        write_control_message(
            &mut channel,
            &ControlMessage::DisplayOffer(DisplayOffer {
                session_id: session.id,
                width_px: session.width_px,
                height_px: session.height_px,
                refresh_hz: session.refresh_hz,
                dpi_x: 96,
                dpi_y: 96,
                rotation_degrees: 0,
                codec: VideoCodec::H264,
            }),
        )?;
        let decision = match read_control_message(&mut channel)? {
            ControlMessage::DisplayDecision(decision) if decision.session_id == session.id => {
                decision
            }
            _ => return Err("The peer returned an invalid display decision".into()),
        };
        if !decision.accepted {
            return Err(decision
                .reason
                .unwrap_or_else(|| "The peer declined the display session".into()));
        }
        let media_port = decision
            .media_port
            .filter(|port| *port != 0)
            .ok_or_else(|| {
                "The peer accepted the session without a UDP media endpoint".to_owned()
            })?;
        let peer_media_address = SocketAddr::new(
            channel
                .sock
                .peer_addr()
                .map_err(|error| error.to_string())?
                .ip(),
            media_port,
        );
        let media_key = export_client_media_key(&channel.conn, session.id)?;
        let media_sender = UdpMediaSender::connect(
            unspecified_address(peer_media_address),
            peer_media_address,
            session.id,
            media_key,
        )
        .map_err(|error| error.to_string())?;
        let media_endpoint = Arc::new(Mutex::new(ActiveMediaEndpoint::Sender(Box::new(
            media_sender,
        ))));
        media_endpoint
            .lock()
            .map_err(|_| "media endpoint lock is poisoned")?
            .local_addr()?;

        channel
            .sock
            .set_read_timeout(Some(SESSION_IO_TIMEOUT))
            .map_err(|error| error.to_string())?;
        channel
            .sock
            .set_write_timeout(Some(SESSION_IO_TIMEOUT))
            .map_err(|error| error.to_string())?;
        self.core
            .start_display_session(session.clone())
            .map_err(|error| error.to_string())?;
        let signal = Arc::new(AtomicBool::new(false));
        match self.active_signals.lock() {
            Ok(mut signals) => {
                signals.insert(session.id, Arc::clone(&signal));
            }
            Err(_) => {
                let _ = self.core.end_display_session(session.id);
                return Err("control session lock is poisoned".into());
            }
        }
        match self.active_media.lock() {
            Ok(mut media) => {
                media.insert(session.id, media_endpoint);
            }
            Err(_) => {
                remove_session(&self.active_signals, session.id);
                let _ = self.core.end_display_session(session.id);
                return Err("media session lock is poisoned".into());
            }
        }
        let worker_core = Arc::clone(&self.core);
        let worker_signals = Arc::clone(&self.active_signals);
        let worker_media = Arc::clone(&self.active_media);
        let worker_shutdown = Arc::clone(&self.shutdown);
        let worker_media_endpoint = self
            .active_media
            .lock()
            .map_err(|_| "media session lock is poisoned")?
            .get(&session.id)
            .cloned()
            .ok_or_else(|| "media session is not active".to_owned())?;
        let real_media_workers = self.real_media_workers;
        let session_id = session.id;
        let (startup_sender, startup_receiver) = mpsc::sync_channel(1);
        let worker_signal = Arc::clone(&signal);
        let worker = thread::Builder::new()
            .name("peerspan-control-session".into())
            .spawn(move || {
                let media_worker = real_media_workers.then(|| {
                    let media_shutdown = Arc::clone(&worker_shutdown);
                    let media_signal = Arc::clone(&worker_signal);
                    let media_startup_sender = startup_sender.clone();
                    thread::Builder::new()
                        .name("peerspan-media-sender".into())
                        .spawn(move || {
                            run_media_sender(
                                worker_media_endpoint,
                                encoder_config,
                                &media_shutdown,
                                &media_signal,
                                media_startup_sender,
                            );
                        })
                });
                let media_worker = match media_worker {
                    Some(Ok(worker)) => Some(worker),
                    Some(Err(error)) => {
                        let _ = startup_sender.send(Err(error.to_string()));
                        None
                    }
                    None => {
                        let _ = startup_sender.send(Ok(()));
                        None
                    }
                };
                run_client_session(
                    channel,
                    &worker_core,
                    session_id,
                    &worker_shutdown,
                    &worker_signal,
                    input_injector,
                );
                worker_signal.store(true, Ordering::Relaxed);
                if let Some(media_worker) = media_worker {
                    let _ = media_worker.join();
                }
                remove_session(&worker_signals, session_id);
                remove_media(&worker_media, session_id);
            });
        let worker = match worker {
            Ok(worker) => worker,
            Err(error) => {
                remove_session(&self.active_signals, session.id);
                remove_media(&self.active_media, session.id);
                let _ = self.core.end_display_session(session.id);
                return Err(error.to_string());
            }
        };
        match startup_receiver.recv_timeout(MEDIA_START_TIMEOUT) {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                signal.store(true, Ordering::Relaxed);
                let _ = worker.join();
                return Err(format!("could not start the real media sender: {error}"));
            }
            Err(error) => {
                signal.store(true, Ordering::Relaxed);
                let _ = worker.join();
                return Err(format!("real media sender startup timed out: {error}"));
            }
        }
        self.client_workers
            .lock()
            .map_err(|_| "control worker lock is poisoned")?
            .push(worker);
        Ok(session)
    }

    #[cfg(test)]
    fn send_test_media(
        &self,
        session_id: Uuid,
        frame_id: u64,
        payload: &[u8],
    ) -> Result<peerspan_media::SendStats, String> {
        let endpoint = self
            .active_media
            .lock()
            .map_err(|_| "media session lock is poisoned")?
            .get(&session_id)
            .cloned()
            .ok_or_else(|| "media session is not active".to_owned())?;
        let mut endpoint = endpoint
            .lock()
            .map_err(|_| "media endpoint lock is poisoned")?;
        match &mut *endpoint {
            ActiveMediaEndpoint::Sender(sender) => sender
                .send_frame(frame_id, frame_id * 16_667, frame_id == 0, payload)
                .map_err(|error| error.to_string()),
            ActiveMediaEndpoint::Receiver(_) => {
                Err("the receiving side cannot send encoded video".into())
            }
        }
    }

    #[cfg(test)]
    fn receive_test_media(
        &self,
        session_id: Uuid,
    ) -> Result<Option<peerspan_media::EncodedFrame>, String> {
        let endpoint = self
            .active_media
            .lock()
            .map_err(|_| "media session lock is poisoned")?
            .get(&session_id)
            .cloned()
            .ok_or_else(|| "media session is not active".to_owned())?;
        let mut endpoint = endpoint
            .lock()
            .map_err(|_| "media endpoint lock is poisoned")?;
        match &mut *endpoint {
            ActiveMediaEndpoint::Receiver(receiver) => receiver
                .receive_once(Instant::now())
                .map_err(|error| error.to_string()),
            ActiveMediaEndpoint::Sender(_) => {
                Err("the sending side cannot receive encoded video".into())
            }
        }
    }

    pub fn end_display_session(&self, session_id: Uuid) -> Result<(), String> {
        let signal = self
            .active_signals
            .lock()
            .map_err(|_| "control session lock is poisoned")?
            .get(&session_id)
            .cloned()
            .ok_or_else(|| "The display session is no longer active".to_owned())?;
        if let Ok(snapshot) = self.core.snapshot()
            && let Some(session) = snapshot.active_session
            && session.id == session_id
        {
            let _ = self.core.update_display_session(
                session_id,
                SessionState::Ending,
                session.latency_ms,
            );
        }
        signal.store(true, Ordering::Relaxed);
        for _ in 0..40 {
            let active = self
                .core
                .snapshot()
                .map_err(|error| error.to_string())?
                .active_session;
            if active
                .as_ref()
                .is_none_or(|session| session.id != session_id)
            {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(50));
        }
        Err("The session shutdown did not finish within two seconds".into())
    }
}

impl Drop for ControlRuntime {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Ok(signals) = self.active_signals.lock() {
            for signal in signals.values() {
                signal.store(true, Ordering::Relaxed);
            }
        }
        if let Ok(mut worker) = self.listener_worker.lock()
            && let Some(worker) = worker.take()
        {
            let _ = worker.join();
        }
        if let Ok(mut workers) = self.client_workers.lock() {
            for worker in workers.drain(..) {
                let _ = worker.join();
            }
        }
        if let Ok(mut workers) = self.server_workers.lock() {
            for worker in workers.drain(..) {
                let _ = worker.join();
            }
        }
    }
}

pub fn mark_control_ready(core: &PeerSpanCore) {
    let _ = core.set_secure_control_capability(Capability::ready(
        "TLS 1.3 listener uses mutual authentication pinned to paired Ed25519 identities",
    ));
}

pub fn mark_control_unavailable(core: &PeerSpanCore, error: &str) {
    let _ = core.set_secure_control_capability(Capability::required(format!(
        "Authenticated TLS control listener could not start: {error}"
    )));
}

fn server_config(identity: &TlsIdentity, core: Arc<PeerSpanCore>) -> Result<ServerConfig, String> {
    let provider = Arc::new(ring::default_provider());
    let verifier = Arc::new(PinnedClientVerifier {
        core,
        algorithms: provider.signature_verification_algorithms,
        root_hints: Vec::new(),
    });
    let mut config = ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&TLS13])
        .map_err(|error| error.to_string())?
        .with_client_cert_verifier(verifier)
        .with_cert_resolver(Arc::new(SingleCertAndKey::from(identity.certified_key())));
    config.alpn_protocols = vec![ALPN_PROTOCOL.to_vec()];
    Ok(config)
}

fn client_config(identity: &TlsIdentity, peer_key: [u8; 32]) -> Result<ClientConfig, String> {
    let provider = Arc::new(ring::default_provider());
    let verifier = Arc::new(PinnedServerVerifier {
        expected_key: peer_key,
        algorithms: provider.signature_verification_algorithms,
    });
    let mut config = ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&TLS13])
        .map_err(|error| error.to_string())?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_client_cert_resolver(Arc::new(SingleCertAndKey::from(identity.certified_key())));
    config.alpn_protocols = vec![ALPN_PROTOCOL.to_vec()];
    Ok(config)
}

#[derive(Debug)]
struct PinnedServerVerifier {
    expected_key: [u8; 32],
    algorithms: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for PinnedServerVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, TlsError> {
        verify_leaf_only(intermediates)?;
        if certificate_public_key(end_entity)? != self.expected_key {
            return Err(TlsError::General(
                "server certificate does not match the paired PeerSpan identity".into(),
            ));
        }
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        verify_tls12_signature(message, cert, dss, &self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        verify_tls13_signature(message, cert, dss, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

struct PinnedClientVerifier {
    core: Arc<PeerSpanCore>,
    algorithms: WebPkiSupportedAlgorithms,
    root_hints: Vec<TlsDistinguishedName>,
}

impl fmt::Debug for PinnedClientVerifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PinnedClientVerifier")
            .finish_non_exhaustive()
    }
}

impl ClientCertVerifier for PinnedClientVerifier {
    fn root_hint_subjects(&self) -> &[TlsDistinguishedName] {
        &self.root_hints
    }

    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, TlsError> {
        verify_leaf_only(intermediates)?;
        let presented_key = certificate_public_key(end_entity)?;
        let trusted = self
            .core
            .snapshot()
            .map_err(|error| TlsError::General(error.to_string()))?
            .trusted_devices
            .iter()
            .filter_map(|peer| decode_public_key(&peer.public_key).ok())
            .any(|key| key == presented_key);
        if !trusted {
            return Err(TlsError::General(
                "client certificate is not a paired PeerSpan identity".into(),
            ));
        }
        Ok(ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        verify_tls12_signature(message, cert, dss, &self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        verify_tls13_signature(message, cert, dss, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

fn connect_tls(
    stream: TcpStream,
    credentials: &DeviceCredentials,
    identity: &TlsIdentity,
    peer: &PeerDevice,
) -> Result<ClientTlsStream, String> {
    let peer_key = decode_public_key(&peer.public_key)?;
    let config = Arc::new(client_config(identity, peer_key)?);
    let server_name = ServerName::try_from(tls_server_name(peer.id))
        .map_err(|_| "invalid PeerSpan TLS server name")?;
    let connection =
        ClientConnection::new(config, server_name).map_err(|error| error.to_string())?;
    let mut channel = StreamOwned::new(connection, stream);
    write_control_message(
        &mut channel,
        &ControlMessage::Hello(local_hello(&credentials.device)),
    )?;
    match read_control_message(&mut channel)? {
        ControlMessage::Hello(hello)
            if hello.protocol_version == PROTOCOL_VERSION
                && hello.device_id == peer.id
                && hello.fingerprint == peer.fingerprint =>
        {
            Ok(channel)
        }
        _ => Err("TLS peer identity does not match the paired device".into()),
    }
}

fn export_client_media_key(
    connection: &ClientConnection,
    session_id: Uuid,
) -> Result<MediaKeyMaterial, String> {
    let exporter = connection
        .export_keying_material(
            [0_u8; MediaKeyMaterial::EXPORTER_BYTES],
            MEDIA_EXPORTER_LABEL,
            Some(session_id.as_bytes()),
        )
        .map_err(|error| format!("could not export the TLS media key: {error}"))?;
    Ok(MediaKeyMaterial::from_exporter(exporter))
}

fn export_server_media_key(
    connection: &ServerConnection,
    session_id: Uuid,
) -> Result<MediaKeyMaterial, String> {
    let exporter = connection
        .export_keying_material(
            [0_u8; MediaKeyMaterial::EXPORTER_BYTES],
            MEDIA_EXPORTER_LABEL,
            Some(session_id.as_bytes()),
        )
        .map_err(|error| format!("could not export the TLS media key: {error}"))?;
    Ok(MediaKeyMaterial::from_exporter(exporter))
}

fn unspecified_address(peer: SocketAddr) -> SocketAddr {
    if peer.is_ipv4() {
        SocketAddr::from(([0, 0, 0, 0], 0))
    } else {
        SocketAddr::from(([0_u16; 8], 0))
    }
}

fn handle_incoming(
    stream: TcpStream,
    remote: SocketAddr,
    context: IncomingContext<'_>,
) -> Result<(), String> {
    let IncomingContext {
        credentials,
        core,
        config,
        runtime_shutdown,
        active_signals,
        active_media,
        real_media_workers,
    } = context;
    configure_stream(&stream, HANDSHAKE_TIMEOUT)?;
    let connection = ServerConnection::new(config).map_err(|error| error.to_string())?;
    let mut channel = StreamOwned::new(connection, stream);
    let hello = match read_control_message(&mut channel)? {
        ControlMessage::Hello(hello) if hello.protocol_version == PROTOCOL_VERSION => hello,
        _ => return Err("TLS control connection did not begin with a compatible hello".into()),
    };
    let certificate = channel
        .conn
        .peer_certificates()
        .and_then(|certificates| certificates.first())
        .ok_or_else(|| "TLS client did not provide a certificate".to_owned())?;
    let certificate_key = certificate_public_key(certificate).map_err(|error| error.to_string())?;
    let peer = core
        .snapshot()
        .map_err(|error| error.to_string())?
        .trusted_devices
        .into_iter()
        .find(|peer| {
            peer.id == hello.device_id
                && peer.fingerprint == hello.fingerprint
                && decode_public_key(&peer.public_key).ok() == Some(certificate_key)
        })
        .ok_or_else(|| "TLS hello is not bound to the presented paired identity".to_owned())?;
    write_control_message(
        &mut channel,
        &ControlMessage::Hello(local_hello(&credentials.device)),
    )?;

    let offer = match read_control_message(&mut channel)? {
        ControlMessage::DisplayOffer(offer) => offer,
        ControlMessage::Heartbeat(heartbeat) => {
            write_control_message(&mut channel, &ControlMessage::Heartbeat(heartbeat))?;
            return Ok(());
        }
        _ => return Err("TLS control connection sent an unsupported first request".into()),
    };
    if let Err(reason) = validate_incoming_offer(&core, &offer) {
        write_control_message(
            &mut channel,
            &ControlMessage::DisplayDecision(DisplayDecision {
                session_id: offer.session_id,
                accepted: false,
                reason: Some(reason),
                media_port: None,
            }),
        )?;
        return Ok(());
    }

    channel
        .sock
        .set_read_timeout(Some(SESSION_IO_TIMEOUT))
        .map_err(|error| error.to_string())?;
    channel
        .sock
        .set_write_timeout(Some(SESSION_IO_TIMEOUT))
        .map_err(|error| error.to_string())?;
    let media_key = export_server_media_key(&channel.conn, offer.session_id)?;
    let media_receiver =
        match UdpMediaReceiver::bind(unspecified_address(remote), offer.session_id, media_key) {
            Ok(receiver) => receiver,
            Err(error) => {
                write_control_message(
                    &mut channel,
                    &ControlMessage::DisplayDecision(DisplayDecision {
                        session_id: offer.session_id,
                        accepted: false,
                        reason: Some(format!("could not bind the UDP media receiver: {error}")),
                        media_port: None,
                    }),
                )?;
                return Ok(());
            }
        };
    media_receiver
        .set_read_timeout(Some(SESSION_IO_TIMEOUT))
        .map_err(|error| error.to_string())?;
    let media_port = media_receiver
        .local_addr()
        .map_err(|error| error.to_string())?
        .port();
    let media_endpoint = Arc::new(Mutex::new(ActiveMediaEndpoint::Receiver(Box::new(
        media_receiver,
    ))));
    media_endpoint
        .lock()
        .map_err(|_| "media endpoint lock is poisoned")?
        .local_addr()?;
    let session = DisplaySession {
        id: offer.session_id,
        peer_id: peer.id,
        direction: SessionDirection::Receiving,
        state: SessionState::Negotiating,
        width_px: offer.width_px,
        height_px: offer.height_px,
        refresh_hz: offer.refresh_hz,
        latency_ms: None,
    };
    if let Err(error) = core.start_display_session(session) {
        write_control_message(
            &mut channel,
            &ControlMessage::DisplayDecision(DisplayDecision {
                session_id: offer.session_id,
                accepted: false,
                reason: Some(error.to_string()),
                media_port: None,
            }),
        )?;
        return Ok(());
    }
    let signal = Arc::new(AtomicBool::new(false));
    match active_signals.lock() {
        Ok(mut signals) => {
            signals.insert(offer.session_id, Arc::clone(&signal));
        }
        Err(_) => {
            let _ = core.end_display_session(offer.session_id);
            return Err("control session lock is poisoned".into());
        }
    }
    match active_media.lock() {
        Ok(mut media) => {
            media.insert(offer.session_id, Arc::clone(&media_endpoint));
        }
        Err(_) => {
            remove_session(active_signals, offer.session_id);
            let _ = core.end_display_session(offer.session_id);
            return Err("media session lock is poisoned".into());
        }
    }
    let (input_sender, input_receiver) = mpsc::channel::<ServerSessionEvent>();
    let (media_startup_sender, media_startup_receiver) = mpsc::sync_channel(1);
    let media_worker = if real_media_workers {
        let media_core = Arc::clone(&core);
        let media_signal = Arc::clone(&signal);
        let media_title = format!("PeerSpan · {}", peer.name);
        let decoder_config = DecoderConfig {
            width: offer.width_px,
            height: offer.height_px,
            frames_per_second: u32::from(offer.refresh_hz),
        };
        let release_shortcut = core
            .snapshot()
            .ok()
            .and_then(|snapshot| parse_release_shortcut(&snapshot.preferences.release_shortcut))
            .map(|shortcut| ReceiverReleaseShortcut {
                control: shortcut.control,
                alt: shortcut.alt,
                shift: shortcut.shift,
                windows: shortcut.windows,
                virtual_key: shortcut.virtual_key,
            })
            .unwrap_or_default();
        match thread::Builder::new()
            .name("peerspan-media-receiver".into())
            .spawn(move || {
                run_media_receiver(
                    media_endpoint,
                    decoder_config,
                    &media_title,
                    release_shortcut,
                    media_core,
                    offer.session_id,
                    media_signal,
                    media_startup_sender,
                    input_sender,
                );
            }) {
            Ok(worker) => Some(worker),
            Err(error) => {
                signal.store(true, Ordering::Relaxed);
                let _ = core.end_display_session(offer.session_id);
                remove_session(active_signals, offer.session_id);
                remove_media(active_media, offer.session_id);
                write_control_message(
                    &mut channel,
                    &ControlMessage::DisplayDecision(DisplayDecision {
                        session_id: offer.session_id,
                        accepted: false,
                        reason: Some(format!("could not start the media receiver: {error}")),
                        media_port: None,
                    }),
                )?;
                return Ok(());
            }
        }
    } else {
        let _ = media_startup_sender.send(Ok(()));
        None
    };
    match media_startup_receiver.recv_timeout(MEDIA_START_TIMEOUT) {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            signal.store(true, Ordering::Relaxed);
            if let Some(worker) = media_worker {
                let _ = worker.join();
            }
            let _ = core.end_display_session(offer.session_id);
            remove_session(active_signals, offer.session_id);
            remove_media(active_media, offer.session_id);
            write_control_message(
                &mut channel,
                &ControlMessage::DisplayDecision(DisplayDecision {
                    session_id: offer.session_id,
                    accepted: false,
                    reason: Some(format!("could not start the real media receiver: {error}")),
                    media_port: None,
                }),
            )?;
            return Ok(());
        }
        Err(error) => {
            signal.store(true, Ordering::Relaxed);
            if let Some(worker) = media_worker {
                let _ = worker.join();
            }
            let _ = core.end_display_session(offer.session_id);
            remove_session(active_signals, offer.session_id);
            remove_media(active_media, offer.session_id);
            write_control_message(
                &mut channel,
                &ControlMessage::DisplayDecision(DisplayDecision {
                    session_id: offer.session_id,
                    accepted: false,
                    reason: Some(format!("real media receiver startup timed out: {error}")),
                    media_port: None,
                }),
            )?;
            return Ok(());
        }
    }
    if let Err(error) = write_control_message(
        &mut channel,
        &ControlMessage::DisplayDecision(DisplayDecision {
            session_id: offer.session_id,
            accepted: true,
            reason: None,
            media_port: Some(media_port),
        }),
    ) {
        signal.store(true, Ordering::Relaxed);
        if let Some(worker) = media_worker {
            let _ = worker.join();
        }
        let _ = core.end_display_session(offer.session_id);
        remove_session(active_signals, offer.session_id);
        remove_media(active_media, offer.session_id);
        return Err(error);
    }
    run_server_session(
        channel,
        &core,
        offer.session_id,
        runtime_shutdown,
        &signal,
        &input_receiver,
    );
    signal.store(true, Ordering::Relaxed);
    if let Some(worker) = media_worker {
        let _ = worker.join();
    }
    remove_session(active_signals, offer.session_id);
    remove_media(active_media, offer.session_id);
    Ok(())
}

fn run_client_session(
    mut channel: ClientTlsStream,
    core: &PeerSpanCore,
    session_id: Uuid,
    runtime_shutdown: &AtomicBool,
    session_shutdown: &AtomicBool,
    mut input_injector: Option<InputInjector>,
) {
    let started = Instant::now();
    let mut sequence = 0_u64;
    let mut next_heartbeat = Instant::now();
    let mut pending_heartbeats = HashMap::new();
    let mut last_received = Instant::now();
    let clipboard_enabled = core
        .snapshot()
        .map(|snapshot| snapshot.preferences.clipboard_sync)
        .unwrap_or(false);
    let mut clipboard = ClipboardSync::new(clipboard_enabled);
    let mut reader = ControlMessageReader::default();
    while !runtime_shutdown.load(Ordering::Relaxed) && !session_shutdown.load(Ordering::Relaxed) {
        let now = Instant::now();
        if now >= next_heartbeat {
            let heartbeat = Heartbeat {
                sequence,
                monotonic_millis: started.elapsed().as_millis() as u64,
            };
            if write_control_message(&mut channel, &ControlMessage::Heartbeat(heartbeat)).is_err() {
                break;
            }
            pending_heartbeats.insert(sequence, now);
            sequence = sequence.wrapping_add(1);
            next_heartbeat = now + HEARTBEAT_INTERVAL;
        }
        if let Some(update) = clipboard.poll_local(now)
            && write_control_message(&mut channel, &ControlMessage::ClipboardText(update)).is_err()
        {
            break;
        }
        match reader.poll(&mut channel) {
            Ok(Some(ControlMessage::Heartbeat(response))) => {
                last_received = Instant::now();
                if let Some(sent) = pending_heartbeats.remove(&response.sequence) {
                    let latency = sent.elapsed().as_millis().min(u16::MAX as u128) as u16;
                    update_session_latency(core, session_id, latency);
                }
            }
            Ok(Some(ControlMessage::StreamReady(ready))) if ready.session_id == session_id => {
                last_received = Instant::now();
                let latency = core
                    .snapshot()
                    .ok()
                    .and_then(|snapshot| snapshot.active_session)
                    .and_then(|session| session.latency_ms);
                let _ = core.update_display_session(session_id, SessionState::Streaming, latency);
            }
            Ok(Some(ControlMessage::Input(event))) => {
                last_received = Instant::now();
                let Some(injector) = input_injector.as_mut() else {
                    break;
                };
                if injector.inject(event).is_err() {
                    break;
                }
            }
            Ok(Some(ControlMessage::ClipboardText(update))) => {
                last_received = Instant::now();
                let _ = clipboard.apply_remote(update);
            }
            Ok(Some(ControlMessage::SessionEnd(end))) if end.session_id == session_id => break,
            Ok(Some(_)) => break,
            Ok(None) => {
                if last_received.elapsed() > SESSION_LIVENESS_TIMEOUT {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    if let Some(injector) = input_injector.as_mut() {
        injector.release_all();
        let _ = injector.recover_windows();
    }
    let _ = write_control_message(
        &mut channel,
        &ControlMessage::SessionEnd(SessionEnd {
            session_id,
            reason: "local session ended".into(),
        }),
    );
    let _ = core.end_display_session(session_id);
}

fn run_server_session(
    mut channel: ServerTlsStream,
    core: &PeerSpanCore,
    session_id: Uuid,
    runtime_shutdown: &AtomicBool,
    session_shutdown: &AtomicBool,
    events: &mpsc::Receiver<ServerSessionEvent>,
) {
    let clipboard_enabled = core
        .snapshot()
        .map(|snapshot| snapshot.preferences.clipboard_sync)
        .unwrap_or(false);
    let mut clipboard = ClipboardSync::new(clipboard_enabled);
    let mut reader = ControlMessageReader::default();
    let mut last_received = Instant::now();
    while !runtime_shutdown.load(Ordering::Relaxed) && !session_shutdown.load(Ordering::Relaxed) {
        while let Ok(event) = events.try_recv() {
            let message = match event {
                ServerSessionEvent::Input(event) => ControlMessage::Input(event),
                ServerSessionEvent::StreamReady => {
                    ControlMessage::StreamReady(StreamReady { session_id })
                }
            };
            if write_control_message(&mut channel, &message).is_err() {
                session_shutdown.store(true, Ordering::Relaxed);
                break;
            }
        }
        if session_shutdown.load(Ordering::Relaxed) {
            break;
        }
        if let Some(update) = clipboard.poll_local(Instant::now())
            && write_control_message(&mut channel, &ControlMessage::ClipboardText(update)).is_err()
        {
            break;
        }
        match reader.poll(&mut channel) {
            Ok(Some(ControlMessage::Heartbeat(heartbeat))) => {
                last_received = Instant::now();
                if write_control_message(&mut channel, &ControlMessage::Heartbeat(heartbeat))
                    .is_err()
                {
                    break;
                }
            }
            Ok(Some(ControlMessage::ClipboardText(update))) => {
                last_received = Instant::now();
                let _ = clipboard.apply_remote(update);
            }
            Ok(Some(ControlMessage::SessionEnd(end))) if end.session_id == session_id => break,
            Ok(Some(_)) => break,
            Ok(None) => {
                if last_received.elapsed() > SESSION_LIVENESS_TIMEOUT {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let _ = write_control_message(
        &mut channel,
        &ControlMessage::SessionEnd(SessionEnd {
            session_id,
            reason: "remote session ended".into(),
        }),
    );
    let _ = core.end_display_session(session_id);
}

#[derive(Default)]
struct ControlMessageReader {
    buffer: Vec<u8>,
}

impl ControlMessageReader {
    fn poll<R: Read>(&mut self, reader: &mut R) -> Result<Option<ControlMessage>, String> {
        if let Some(message) = self.take_message()? {
            return Ok(Some(message));
        }
        let mut chunk = [0_u8; 8192];
        match reader.read(&mut chunk) {
            Ok(0) => Err("authenticated control channel closed".into()),
            Ok(read) => {
                self.buffer.extend_from_slice(&chunk[..read]);
                self.take_message()
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) =>
            {
                Ok(None)
            }
            Err(error) => Err(error.to_string()),
        }
    }

    fn take_message(&mut self) -> Result<Option<ControlMessage>, String> {
        if self.buffer.len() < 4 {
            return Ok(None);
        }
        let length =
            u32::from_be_bytes(self.buffer[..4].try_into().expect("four byte prefix")) as usize;
        if length == 0 || length > MAX_FRAME_BYTES {
            return Err("control frame length is outside the allowed range".into());
        }
        let total = 4 + length;
        if self.buffer.len() < total {
            return Ok(None);
        }
        let message = serde_json::from_slice(&self.buffer[4..total])
            .map_err(|error| format!("invalid control message: {error}"))?;
        self.buffer.drain(..total);
        Ok(Some(message))
    }
}

fn update_session_latency(core: &PeerSpanCore, session_id: Uuid, latency: u16) {
    if let Ok(snapshot) = core.snapshot()
        && let Some(session) = snapshot.active_session
        && session.id == session_id
    {
        let _ = core.update_display_session(session_id, session.state, Some(latency));
    }
}

fn run_media_sender(
    endpoint: Arc<Mutex<ActiveMediaEndpoint>>,
    config: EncoderConfig,
    runtime_shutdown: &AtomicBool,
    session_shutdown: &AtomicBool,
    startup: mpsc::SyncSender<Result<(), String>>,
) {
    let open_deadline = Instant::now() + Duration::from_secs(7);
    let mut encoder = loop {
        match SharedIddFrameEncoder::open(config) {
            Ok(encoder) => break encoder,
            Err(VideoError::SharedTexture(_)) if Instant::now() < open_deadline => {
                thread::sleep(Duration::from_millis(100));
            }
            Err(error) => {
                let _ = startup.send(Err(error.to_string()));
                session_shutdown.store(true, Ordering::Relaxed);
                return;
            }
        }
    };
    let _ = startup.send(Ok(()));
    let started = Instant::now();
    let mut frame_id = 0_u64;
    while !runtime_shutdown.load(Ordering::Relaxed) && !session_shutdown.load(Ordering::Relaxed) {
        let access_unit = match encoder.encode_next(
            started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64,
            FRAME_ACQUIRE_TIMEOUT,
        ) {
            Ok(Some(access_unit)) => access_unit,
            Ok(None) => continue,
            Err(_) => break,
        };
        let sent = endpoint
            .lock()
            .map_err(|_| ())
            .and_then(|mut endpoint| match &mut *endpoint {
                ActiveMediaEndpoint::Sender(sender) => sender
                    .send_frame(
                        frame_id,
                        access_unit.timestamp_micros,
                        access_unit.keyframe,
                        &access_unit.bytes,
                    )
                    .map(|_| ())
                    .map_err(|_| ()),
                ActiveMediaEndpoint::Receiver(_) => Err(()),
            });
        if sent.is_err() {
            break;
        }
        frame_id = frame_id.wrapping_add(1);
    }
    session_shutdown.store(true, Ordering::Relaxed);
}

#[allow(clippy::too_many_arguments)]
fn run_media_receiver(
    endpoint: Arc<Mutex<ActiveMediaEndpoint>>,
    config: DecoderConfig,
    title: &str,
    release_shortcut: ReceiverReleaseShortcut,
    core: Arc<PeerSpanCore>,
    session_id: Uuid,
    session_shutdown: Arc<AtomicBool>,
    startup: mpsc::SyncSender<Result<(), String>>,
    events: mpsc::Sender<ServerSessionEvent>,
) {
    let mut receiver =
        match NativeVideoReceiver::open_with_release_shortcut(config, title, release_shortcut) {
            Ok(receiver) => receiver,
            Err(error) => {
                let _ = startup.send(Err(error.to_string()));
                session_shutdown.store(true, Ordering::Relaxed);
                return;
            }
        };
    let _ = startup.send(Ok(()));
    let mut first_frame_presented = false;
    while !session_shutdown.load(Ordering::Relaxed) {
        if receiver.pump_events().is_err() {
            break;
        }
        forward_receiver_events(&mut receiver, &events);
        if receiver.close_requested() {
            break;
        }
        let frame = match endpoint.lock() {
            Ok(mut endpoint) => match &mut *endpoint {
                ActiveMediaEndpoint::Receiver(media) => media.receive_once(Instant::now()),
                ActiveMediaEndpoint::Sender(_) => break,
            },
            Err(_) => break,
        };
        match frame {
            Ok(Some(frame)) => {
                match receiver.decode_and_present(&frame.payload, frame.timestamp_micros) {
                    Ok(true) if !first_frame_presented => {
                        first_frame_presented = true;
                        if core
                            .update_display_session(session_id, SessionState::Streaming, None)
                            .is_err()
                        {
                            break;
                        }
                        if events.send(ServerSessionEvent::StreamReady).is_err() {
                            break;
                        }
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
            Ok(None) => {}
            Err(MediaError::Io(error))
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) => {}
            Err(MediaError::Io(_)) => break,
            Err(_) => {
                // Auth failures, replays and stale datagrams are bounded and
                // dropped without tearing down a healthy authenticated session.
            }
        }
        forward_receiver_events(&mut receiver, &events);
    }
    let _ = events.send(ServerSessionEvent::Input(InputEvent::ReleaseAll));
    session_shutdown.store(true, Ordering::Relaxed);
}

fn forward_receiver_events(
    receiver: &mut NativeVideoReceiver,
    sender: &mpsc::Sender<ServerSessionEvent>,
) {
    for event in receiver.take_input_events() {
        let event = match event {
            ReceiverInputEvent::PointerMove {
                normalized_x,
                normalized_y,
            } => InputEvent::PointerMove {
                normalized_x,
                normalized_y,
            },
            ReceiverInputEvent::PointerButton { button, pressed } => InputEvent::PointerButton {
                button: match button {
                    ReceiverPointerButton::Left => PointerButton::Left,
                    ReceiverPointerButton::Right => PointerButton::Right,
                    ReceiverPointerButton::Middle => PointerButton::Middle,
                    ReceiverPointerButton::Back => PointerButton::Back,
                    ReceiverPointerButton::Forward => PointerButton::Forward,
                },
                pressed,
            },
            ReceiverInputEvent::Wheel { delta_x, delta_y } => {
                InputEvent::Wheel { delta_x, delta_y }
            }
            ReceiverInputEvent::Key {
                scan_code,
                pressed,
                extended,
            } => InputEvent::Key {
                scan_code,
                pressed,
                extended,
            },
            ReceiverInputEvent::ReleaseAll => InputEvent::ReleaseAll,
        };
        if sender.send(ServerSessionEvent::Input(event)).is_err() {
            break;
        }
    }
}

fn bitrate_for_quality(quality: QualityMode) -> u32 {
    match quality {
        QualityMode::Clarity => 20_000_000,
        QualityMode::Balanced => 12_000_000,
        QualityMode::Responsive => 8_000_000,
    }
}

fn validate_incoming_offer(core: &PeerSpanCore, offer: &DisplayOffer) -> Result<(), String> {
    if offer.session_id.is_nil()
        || offer.width_px == 0
        || offer.height_px == 0
        || offer.refresh_hz == 0
        || offer.width_px > 7680
        || offer.height_px > 4320
        || offer.refresh_hz > 240
    {
        return Err("The requested display mode is outside PeerSpan safety limits".into());
    }
    if !(48..=960).contains(&offer.dpi_x) || !(48..=960).contains(&offer.dpi_y) {
        return Err("The requested display DPI is outside PeerSpan safety limits".into());
    }
    if offer.rotation_degrees != 0
        && offer.rotation_degrees != 90
        && offer.rotation_degrees != 180
        && offer.rotation_degrees != 270
    {
        return Err("The requested display rotation is invalid".into());
    }
    if offer.codec != VideoCodec::H264 {
        return Err("The receiver currently requires H.264".into());
    }
    let snapshot = core.snapshot().map_err(|error| error.to_string())?;
    require_ready(
        "media pipeline",
        &snapshot.capabilities.media_pipeline.state,
        &snapshot.capabilities.media_pipeline.detail,
    )?;
    Ok(())
}

fn require_ready(name: &str, state: &CapabilityState, detail: &str) -> Result<(), String> {
    if *state == CapabilityState::Ready {
        Ok(())
    } else {
        Err(format!("PeerSpan {name} is not ready: {detail}"))
    }
}

fn resolve_online_trusted_peer(
    snapshot: &peerspan_core::AppSnapshot,
    peer_id: Uuid,
) -> Result<PeerDevice, String> {
    let trusted = snapshot
        .trusted_devices
        .iter()
        .find(|peer| peer.id == peer_id)
        .ok_or_else(|| "Display sessions require a paired device".to_owned())?;
    let nearby = snapshot
        .nearby_devices
        .iter()
        .find(|peer| peer.id == peer_id && peer.status == DeviceStatus::Online)
        .ok_or_else(|| "The paired device is not currently visible on the LAN".to_owned())?;
    if trusted.public_key != nearby.public_key || trusted.fingerprint != nearby.fingerprint {
        return Err("The nearby device identity differs from the paired identity".into());
    }
    let mut resolved = nearby.clone();
    resolved.trusted = true;
    Ok(resolved)
}

fn connect_to_peer(peer: &PeerDevice) -> Result<TcpStream, String> {
    let mut failures = Vec::new();
    for address in &peer.addresses {
        let Ok(ip) = address.parse::<IpAddr>() else {
            continue;
        };
        let endpoint = SocketAddr::new(ip, peer.control_port);
        match TcpStream::connect_timeout(&endpoint, CONNECT_TIMEOUT) {
            Ok(stream) => return Ok(stream),
            Err(error) => failures.push(format!("{endpoint}: {error}")),
        }
    }
    Err(format!(
        "Could not reach the peer TLS control service{}",
        if failures.is_empty() {
            String::new()
        } else {
            format!(": {}", failures.join("; "))
        }
    ))
}

fn configure_stream(stream: &TcpStream, timeout: Duration) -> Result<(), String> {
    stream
        .set_nonblocking(false)
        .map_err(|error| error.to_string())?;
    stream
        .set_nodelay(true)
        .map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn local_hello(device: &peerspan_core::LocalDevice) -> Hello {
    Hello {
        protocol_version: PROTOCOL_VERSION,
        device_id: device.id,
        device_name: device.name.clone(),
        fingerprint: device.fingerprint.clone(),
    }
}

fn write_control_message<W: Write>(writer: &mut W, message: &ControlMessage) -> Result<(), String> {
    write_frame(writer, message).map_err(|error| error.to_string())
}

fn read_control_message<R: Read>(reader: &mut R) -> Result<ControlMessage, String> {
    read_frame(reader).map_err(|error| error.to_string())
}

fn write_frame<W: Write, T: Serialize>(writer: &mut W, value: &T) -> io::Result<()> {
    let bytes = serde_json::to_vec(value)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if bytes.is_empty() || bytes.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "control frame exceeds the size limit",
        ));
    }
    writer.write_all(&(bytes.len() as u32).to_be_bytes())?;
    writer.write_all(&bytes)?;
    writer.flush()
}

fn read_frame<R: Read, T: DeserializeOwned>(reader: &mut R) -> io::Result<T> {
    let mut length = [0_u8; 4];
    reader.read_exact(&mut length)?;
    let length = u32::from_be_bytes(length) as usize;
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid control frame length",
        ));
    }
    let mut bytes = vec![0_u8; length];
    reader.read_exact(&mut bytes)?;
    serde_json::from_slice(&bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn certificate_public_key(certificate: &CertificateDer<'_>) -> Result<[u8; 32], TlsError> {
    let (remaining, certificate) = parse_x509_certificate(certificate.as_ref())
        .map_err(|_| TlsError::General("invalid PeerSpan TLS certificate".into()))?;
    if !remaining.is_empty()
        || certificate.public_key().algorithm.algorithm != OID_SIG_ED25519
        || !certificate.validity().is_valid()
    {
        return Err(TlsError::General(
            "invalid or expired PeerSpan TLS certificate".into(),
        ));
    }
    certificate
        .public_key()
        .subject_public_key
        .data
        .as_ref()
        .try_into()
        .map_err(|_| TlsError::General("PeerSpan TLS key has the wrong length".into()))
}

fn verify_leaf_only(intermediates: &[CertificateDer<'_>]) -> Result<(), TlsError> {
    if intermediates.is_empty() {
        Ok(())
    } else {
        Err(TlsError::General(
            "PeerSpan TLS identities must not include certificate chains".into(),
        ))
    }
}

fn decode_public_key(value: &str) -> Result<[u8; 32], String> {
    hex::decode(value)
        .map_err(|_| "paired public key is not hexadecimal".to_owned())?
        .try_into()
        .map_err(|_| "paired public key has the wrong length".to_owned())
}

fn tls_server_name(device_id: Uuid) -> String {
    format!("{}.peerspan.local", device_id.simple())
}

#[cfg(test)]
fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn remove_session(active_signals: &Mutex<HashMap<Uuid, Arc<AtomicBool>>>, session_id: Uuid) {
    if let Ok(mut signals) = active_signals.lock() {
        signals.remove(&session_id);
    }
}

fn remove_media(active_media: &ActiveMediaMap, session_id: Uuid) {
    if let Ok(mut media) = active_media.lock() {
        media.remove(&session_id);
    }
}

fn reap_finished_workers(workers: &Mutex<Vec<thread::JoinHandle<()>>>) {
    if let Ok(mut workers) = workers.lock() {
        let mut index = 0;
        while index < workers.len() {
            if workers[index].is_finished() {
                let worker = workers.swap_remove(index);
                let _ = worker.join();
            } else {
                index += 1;
            }
        }
    }
}

fn reap_finished_client_workers(workers: &Mutex<Vec<thread::JoinHandle<()>>>) {
    reap_finished_workers(workers);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pairing::fingerprint_public_key;
    use ed25519_dalek::SigningKey;
    use std::fs;

    #[test]
    fn session_control_reader_preserves_fragmented_and_buffered_frames() {
        struct OneByteReader(std::io::Cursor<Vec<u8>>);
        impl Read for OneByteReader {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                let length = buffer.len().min(1);
                self.0.read(&mut buffer[..length])
            }
        }

        let first = ControlMessage::Heartbeat(Heartbeat {
            sequence: 7,
            monotonic_millis: 11,
        });
        let second = ControlMessage::StreamReady(StreamReady {
            session_id: Uuid::new_v4(),
        });
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &first).unwrap();
        write_frame(&mut bytes, &second).unwrap();
        let mut source = OneByteReader(std::io::Cursor::new(bytes));
        let mut reader = ControlMessageReader::default();
        let decoded_first = loop {
            if let Some(message) = reader.poll(&mut source).unwrap() {
                break message;
            }
        };
        let decoded_second = loop {
            if let Some(message) = reader.poll(&mut source).unwrap() {
                break message;
            }
        };
        assert_eq!(decoded_first, first);
        assert_eq!(decoded_second, second);
    }

    fn credentials(seed: u8, name: &str) -> DeviceCredentials {
        let signing_key = SigningKey::from_bytes(&[seed; 32]);
        let public_key = signing_key.verifying_key().to_bytes();
        DeviceCredentials {
            device: peerspan_core::LocalDevice {
                id: Uuid::new_v4(),
                name: name.into(),
                platform: "Windows test".into(),
                fingerprint: fingerprint_public_key(&public_key),
                public_key: hex::encode(public_key),
            },
            signing_key,
        }
    }

    fn peer(credentials: &DeviceCredentials, control_port: u16) -> PeerDevice {
        PeerDevice {
            id: credentials.device.id,
            name: credentials.device.name.clone(),
            platform: credentials.device.platform.clone(),
            fingerprint: credentials.device.fingerprint.clone(),
            public_key: credentials.device.public_key.clone(),
            status: DeviceStatus::Online,
            trusted: true,
            latency_ms: None,
            last_seen_unix_ms: unix_millis(),
            addresses: vec!["127.0.0.1".into()],
            control_port,
            pairing_port: 37_621,
            protocol_version: PROTOCOL_VERSION,
        }
    }

    fn ready_sender(core: &PeerSpanCore) {
        core.set_virtual_display_capability(Capability::ready("test virtual display"))
            .unwrap();
        core.set_media_pipeline_capability(Capability::ready("test media"))
            .unwrap();
        core.set_input_injection_capability(Capability::ready("test input"))
            .unwrap();
    }

    fn wait_until(mut condition: impl FnMut() -> bool) -> bool {
        (0..60).any(|_| {
            let ready = condition();
            if !ready {
                thread::sleep(Duration::from_millis(50));
            }
            ready
        })
    }

    #[test]
    fn mutual_tls_session_uses_paired_identity_and_cleans_up() {
        let credentials_a = credentials(31, "A");
        let credentials_b = credentials(47, "B");
        let data_a = std::env::temp_dir().join(format!("peerspan-control-a-{}", Uuid::new_v4()));
        let data_b = std::env::temp_dir().join(format!("peerspan-control-b-{}", Uuid::new_v4()));
        let core_a = Arc::new(PeerSpanCore::load(credentials_a.device.clone(), &data_a).unwrap());
        let core_b = Arc::new(PeerSpanCore::load(credentials_b.device.clone(), &data_b).unwrap());
        ready_sender(&core_a);
        core_b
            .set_media_pipeline_capability(Capability::ready("test media"))
            .unwrap();

        let listener_a = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port_a = listener_a.local_addr().unwrap().port();
        let listener_b = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port_b = listener_b.local_addr().unwrap().port();
        let peer_a = peer(&credentials_a, port_a);
        let peer_b = peer(&credentials_b, port_b);
        core_a.trust_device(peer_b.clone()).unwrap();
        core_a.upsert_nearby_device(peer_b).unwrap();
        core_b.trust_device(peer_a.clone()).unwrap();
        core_b.upsert_nearby_device(peer_a).unwrap();

        let runtime_a =
            ControlRuntime::start_with_listener(credentials_a, Arc::clone(&core_a), listener_a)
                .unwrap();
        let runtime_b = ControlRuntime::start_with_listener(
            credentials_b.clone(),
            Arc::clone(&core_b),
            listener_b,
        )
        .unwrap();

        let session = runtime_a
            .request_display_session(core_b.snapshot().unwrap().local_device.id)
            .unwrap();
        assert!(wait_until(|| {
            core_a
                .snapshot()
                .unwrap()
                .active_session
                .as_ref()
                .and_then(|session| session.latency_ms)
                .is_some()
                && core_b.snapshot().unwrap().active_session.is_some()
        }));
        assert_eq!(
            core_b.snapshot().unwrap().active_session.unwrap().direction,
            SessionDirection::Receiving
        );
        let encoded_access_unit = vec![0x65_u8; 70_000];
        let stats = runtime_a
            .send_test_media(session.id, 0, &encoded_access_unit)
            .unwrap();
        assert!(stats.datagrams > 1);
        let received = loop {
            if let Some(frame) = runtime_b.receive_test_media(session.id).unwrap() {
                break frame;
            }
        };
        assert_eq!(received.frame_id, 0);
        assert!(received.keyframe);
        assert_eq!(received.payload, encoded_access_unit);

        runtime_a.end_display_session(session.id).unwrap();
        assert!(wait_until(|| {
            core_a.snapshot().unwrap().active_session.is_none()
                && core_b.snapshot().unwrap().active_session.is_none()
        }));
        assert!(runtime_a.active_media.lock().unwrap().is_empty());
        assert!(runtime_b.active_media.lock().unwrap().is_empty());
        drop(runtime_a);
        drop(runtime_b);
        let _ = fs::remove_dir_all(data_a);
        let _ = fs::remove_dir_all(data_b);
    }

    #[test]
    fn tls_server_rejects_an_identity_it_has_not_paired() {
        let credentials_a = credentials(53, "A");
        let credentials_b = credentials(61, "B");
        let data_a = std::env::temp_dir().join(format!("peerspan-control-a-{}", Uuid::new_v4()));
        let data_b = std::env::temp_dir().join(format!("peerspan-control-b-{}", Uuid::new_v4()));
        let core_a = Arc::new(PeerSpanCore::load(credentials_a.device.clone(), &data_a).unwrap());
        let core_b = Arc::new(PeerSpanCore::load(credentials_b.device.clone(), &data_b).unwrap());
        ready_sender(&core_a);
        core_b
            .set_media_pipeline_capability(Capability::ready("test media"))
            .unwrap();
        let listener_b = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port_b = listener_b.local_addr().unwrap().port();
        let peer_b = peer(&credentials_b, port_b);
        core_a.trust_device(peer_b.clone()).unwrap();
        core_a.upsert_nearby_device(peer_b).unwrap();
        let _runtime_b = ControlRuntime::start_with_listener(
            credentials_b.clone(),
            Arc::clone(&core_b),
            listener_b,
        )
        .unwrap();

        let identity_a = TlsIdentity::from_credentials(&credentials_a).unwrap();
        let stream = TcpStream::connect(("127.0.0.1", port_b)).unwrap();
        configure_stream(&stream, HANDSHAKE_TIMEOUT).unwrap();
        assert!(
            connect_tls(
                stream,
                &credentials_a,
                &identity_a,
                &peer(&credentials_b, port_b)
            )
            .is_err()
        );
        assert!(core_b.snapshot().unwrap().active_session.is_none());
        let _ = fs::remove_dir_all(data_a);
        let _ = fs::remove_dir_all(data_b);
    }

    #[test]
    fn tls_client_rejects_a_server_with_a_changed_identity_key() {
        let credentials_a = credentials(67, "A");
        let credentials_b = credentials(71, "B");
        let credentials_changed = credentials(73, "Changed B");
        let data_changed =
            std::env::temp_dir().join(format!("peerspan-control-changed-{}", Uuid::new_v4()));
        let core_changed = Arc::new(
            PeerSpanCore::load(credentials_changed.device.clone(), &data_changed).unwrap(),
        );
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        core_changed
            .trust_device(peer(&credentials_a, port.saturating_add(1)))
            .unwrap();
        let _changed_runtime = ControlRuntime::start_with_listener(
            credentials_changed,
            Arc::clone(&core_changed),
            listener,
        )
        .unwrap();

        let identity_a = TlsIdentity::from_credentials(&credentials_a).unwrap();
        let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        configure_stream(&stream, HANDSHAKE_TIMEOUT).unwrap();
        assert!(
            connect_tls(
                stream,
                &credentials_a,
                &identity_a,
                &peer(&credentials_b, port),
            )
            .is_err()
        );
        let _ = fs::remove_dir_all(data_changed);
    }
}
