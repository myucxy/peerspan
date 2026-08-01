//! Authenticated, low-latency datagrams for PeerSpan encoded video access units.
//!
//! TLS authenticates the peers and exports the per-session key material. This
//! crate handles only the bounded UDP packetization layer; it never persists a
//! key and it deliberately drops incomplete or stale frames instead of building
//! an unbounded latency queue.

use chacha20poly1305::{
    ChaCha20Poly1305, KeyInit,
    aead::{Aead, Payload},
};
use socket2::{Domain, Protocol, Socket, Type};
use std::{
    collections::HashMap,
    fmt, io,
    net::{SocketAddr, UdpSocket},
    time::{Duration, Instant},
};
use thiserror::Error;
use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop};

const MAGIC: [u8; 4] = *b"PSM3";
const FORMAT_VERSION: u8 = 1;
const KEYFRAME_FLAG: u8 = 1;
const HEADER_BYTES: usize = 50;
const AUTH_TAG_BYTES: usize = 16;
const NONCE_PREFIX_BYTES: usize = 4;
const MAX_IN_FLIGHT_FRAMES: usize = 4;
const MAX_FRAGMENT_COUNT: usize = 8192;
const REPLAY_WINDOW_BITS: u64 = 128;
const FRAME_TTL: Duration = Duration::from_millis(80);
const SOCKET_BUFFER_BYTES: usize = 4 * 1024 * 1024;

/// Fits IPv6's minimum MTU after UDP and IP headers without relying on fragmentation.
pub const MAX_DATAGRAM_BYTES: usize = 1200;
pub const MAX_FRAGMENT_PAYLOAD: usize = MAX_DATAGRAM_BYTES - HEADER_BYTES - AUTH_TAG_BYTES;
pub const MAX_ENCODED_FRAME_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct MediaKeyMaterial {
    key: [u8; 32],
    nonce_prefix: [u8; NONCE_PREFIX_BYTES],
}

impl MediaKeyMaterial {
    pub const EXPORTER_BYTES: usize = 32 + NONCE_PREFIX_BYTES;

    pub fn from_exporter(mut exporter: [u8; Self::EXPORTER_BYTES]) -> Self {
        let mut key = [0_u8; 32];
        key.copy_from_slice(&exporter[..32]);
        let mut nonce_prefix = [0_u8; NONCE_PREFIX_BYTES];
        nonce_prefix.copy_from_slice(&exporter[32..]);
        exporter.zeroize();
        Self { key, nonce_prefix }
    }

    fn nonce(&self, packet_sequence: u64) -> [u8; 12] {
        let mut nonce = [0_u8; 12];
        nonce[..NONCE_PREFIX_BYTES].copy_from_slice(&self.nonce_prefix);
        nonce[NONCE_PREFIX_BYTES..].copy_from_slice(&packet_sequence.to_be_bytes());
        nonce
    }
}

impl fmt::Debug for MediaKeyMaterial {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MediaKeyMaterial")
            .field("key", &"[REDACTED]")
            .field("nonce_prefix", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedFrame {
    pub frame_id: u64,
    pub timestamp_micros: u64,
    pub keyframe: bool,
    pub payload: Vec<u8>,
}

pub struct MediaPacketizer {
    session_id: Uuid,
    cipher: ChaCha20Poly1305,
    key_material: MediaKeyMaterial,
    next_packet_sequence: u64,
}

impl MediaPacketizer {
    pub fn new(session_id: Uuid, key_material: MediaKeyMaterial) -> Self {
        let cipher = ChaCha20Poly1305::new((&key_material.key).into());
        Self {
            session_id,
            cipher,
            key_material,
            next_packet_sequence: 0,
        }
    }

    pub fn packetize(
        &mut self,
        frame_id: u64,
        timestamp_micros: u64,
        keyframe: bool,
        encoded_frame: &[u8],
    ) -> Result<Vec<Vec<u8>>, MediaError> {
        if encoded_frame.is_empty() {
            return Err(MediaError::EmptyFrame);
        }
        if encoded_frame.len() > MAX_ENCODED_FRAME_BYTES {
            return Err(MediaError::FrameTooLarge(encoded_frame.len()));
        }
        let fragment_count = encoded_frame.len().div_ceil(MAX_FRAGMENT_PAYLOAD);
        if fragment_count == 0 || fragment_count > MAX_FRAGMENT_COUNT {
            return Err(MediaError::TooManyFragments(fragment_count));
        }
        let fragment_count_u16 = u16::try_from(fragment_count)
            .map_err(|_| MediaError::TooManyFragments(fragment_count))?;
        let mut datagrams = Vec::with_capacity(fragment_count);
        for (fragment_index, fragment) in encoded_frame.chunks(MAX_FRAGMENT_PAYLOAD).enumerate() {
            let packet_sequence = self.next_packet_sequence;
            self.next_packet_sequence = self
                .next_packet_sequence
                .checked_add(1)
                .ok_or(MediaError::SequenceExhausted)?;
            let header = encode_header(PacketHeader {
                flags: u8::from(keyframe) * KEYFRAME_FLAG,
                session_id: self.session_id,
                packet_sequence,
                frame_id,
                fragment_index: fragment_index as u16,
                fragment_count: fragment_count_u16,
                timestamp_micros,
            });
            let nonce = self.key_material.nonce(packet_sequence);
            let ciphertext = self
                .cipher
                .encrypt(
                    (&nonce).into(),
                    Payload {
                        msg: fragment,
                        aad: &header,
                    },
                )
                .map_err(|_| MediaError::Authentication)?;
            let mut datagram = Vec::with_capacity(header.len() + ciphertext.len());
            datagram.extend_from_slice(&header);
            datagram.extend_from_slice(&ciphertext);
            debug_assert!(datagram.len() <= MAX_DATAGRAM_BYTES);
            datagrams.push(datagram);
        }
        Ok(datagrams)
    }
}

pub struct MediaReassembler {
    session_id: Uuid,
    cipher: ChaCha20Poly1305,
    key_material: MediaKeyMaterial,
    replay: ReplayWindow,
    frames: HashMap<u64, IncompleteFrame>,
    last_completed_frame_id: Option<u64>,
}

impl MediaReassembler {
    pub fn new(session_id: Uuid, key_material: MediaKeyMaterial) -> Self {
        let cipher = ChaCha20Poly1305::new((&key_material.key).into());
        Self {
            session_id,
            cipher,
            key_material,
            replay: ReplayWindow::default(),
            frames: HashMap::new(),
            last_completed_frame_id: None,
        }
    }

    pub fn accept(
        &mut self,
        datagram: &[u8],
        now: Instant,
    ) -> Result<Option<EncodedFrame>, MediaError> {
        if datagram.len() <= HEADER_BYTES + AUTH_TAG_BYTES || datagram.len() > MAX_DATAGRAM_BYTES {
            return Err(MediaError::MalformedPacket);
        }
        let header = decode_header(&datagram[..HEADER_BYTES])?;
        if header.session_id != self.session_id {
            return Err(MediaError::WrongSession);
        }
        if header.fragment_count == 0
            || usize::from(header.fragment_count) > MAX_FRAGMENT_COUNT
            || header.fragment_index >= header.fragment_count
        {
            return Err(MediaError::InvalidFragment);
        }
        if self
            .last_completed_frame_id
            .is_some_and(|completed| header.frame_id <= completed)
        {
            return Err(MediaError::StaleFrame(header.frame_id));
        }

        let nonce = self.key_material.nonce(header.packet_sequence);
        let payload = self
            .cipher
            .decrypt(
                (&nonce).into(),
                Payload {
                    msg: &datagram[HEADER_BYTES..],
                    aad: &datagram[..HEADER_BYTES],
                },
            )
            .map_err(|_| MediaError::Authentication)?;
        if payload.is_empty() || payload.len() > MAX_FRAGMENT_PAYLOAD {
            return Err(MediaError::InvalidFragment);
        }
        if !self.replay.accept(header.packet_sequence) {
            return Err(MediaError::Replay(header.packet_sequence));
        }

        self.frames
            .retain(|_, frame| now.saturating_duration_since(frame.first_seen) <= FRAME_TTL);
        if !self.frames.contains_key(&header.frame_id)
            && self.frames.len() >= MAX_IN_FLIGHT_FRAMES
            && let Some(oldest) = self
                .frames
                .iter()
                .min_by_key(|(_, frame)| frame.first_seen)
                .map(|(frame_id, _)| *frame_id)
        {
            self.frames.remove(&oldest);
        }

        let frame = self
            .frames
            .entry(header.frame_id)
            .or_insert_with(|| IncompleteFrame::new(&header, now));
        if frame.fragment_count != header.fragment_count
            || frame.timestamp_micros != header.timestamp_micros
            || frame.keyframe != header.keyframe()
        {
            self.frames.remove(&header.frame_id);
            return Err(MediaError::InconsistentFrame(header.frame_id));
        }
        let fragment_slot = &mut frame.fragments[usize::from(header.fragment_index)];
        if fragment_slot.is_some() {
            return Err(MediaError::DuplicateFragment {
                frame_id: header.frame_id,
                fragment_index: header.fragment_index,
            });
        }
        frame.total_bytes = frame
            .total_bytes
            .checked_add(payload.len())
            .ok_or(MediaError::FrameTooLarge(usize::MAX))?;
        if frame.total_bytes > MAX_ENCODED_FRAME_BYTES {
            let total_bytes = frame.total_bytes;
            self.frames.remove(&header.frame_id);
            return Err(MediaError::FrameTooLarge(total_bytes));
        }
        *fragment_slot = Some(payload);
        frame.received_fragments += 1;
        if frame.received_fragments != usize::from(frame.fragment_count) {
            return Ok(None);
        }

        let completed = self
            .frames
            .remove(&header.frame_id)
            .ok_or(MediaError::InconsistentFrame(header.frame_id))?;
        let mut payload = Vec::with_capacity(completed.total_bytes);
        for fragment in completed.fragments {
            payload.extend(fragment.ok_or(MediaError::InconsistentFrame(header.frame_id))?);
        }
        self.frames
            .retain(|frame_id, _| *frame_id > header.frame_id);
        self.last_completed_frame_id = Some(header.frame_id);
        Ok(Some(EncodedFrame {
            frame_id: header.frame_id,
            timestamp_micros: completed.timestamp_micros,
            keyframe: completed.keyframe,
            payload,
        }))
    }

    pub fn pending_frames(&self) -> usize {
        self.frames.len()
    }
}

pub struct UdpMediaSender {
    socket: UdpSocket,
    packetizer: MediaPacketizer,
}

impl UdpMediaSender {
    pub fn connect(
        bind_address: SocketAddr,
        peer_address: SocketAddr,
        session_id: Uuid,
        key_material: MediaKeyMaterial,
    ) -> Result<Self, MediaError> {
        let socket = bind_udp(bind_address, false)?;
        socket.connect(peer_address)?;
        Ok(Self {
            socket,
            packetizer: MediaPacketizer::new(session_id, key_material),
        })
    }

    pub fn local_addr(&self) -> Result<SocketAddr, MediaError> {
        Ok(self.socket.local_addr()?)
    }

    pub fn send_frame(
        &mut self,
        frame_id: u64,
        timestamp_micros: u64,
        keyframe: bool,
        encoded_frame: &[u8],
    ) -> Result<SendStats, MediaError> {
        let datagrams =
            self.packetizer
                .packetize(frame_id, timestamp_micros, keyframe, encoded_frame)?;
        let mut bytes = 0;
        for datagram in &datagrams {
            let sent = self.socket.send(datagram)?;
            if sent != datagram.len() {
                return Err(MediaError::ShortDatagramWrite {
                    expected: datagram.len(),
                    actual: sent,
                });
            }
            bytes += sent;
        }
        Ok(SendStats {
            datagrams: datagrams.len(),
            bytes,
        })
    }
}

pub struct UdpMediaReceiver {
    socket: UdpSocket,
    reassembler: MediaReassembler,
    buffer: [u8; MAX_DATAGRAM_BYTES + 1],
}

impl UdpMediaReceiver {
    pub fn bind(
        bind_address: SocketAddr,
        session_id: Uuid,
        key_material: MediaKeyMaterial,
    ) -> Result<Self, MediaError> {
        Ok(Self {
            socket: bind_udp(bind_address, true)?,
            reassembler: MediaReassembler::new(session_id, key_material),
            buffer: [0_u8; MAX_DATAGRAM_BYTES + 1],
        })
    }

    pub fn local_addr(&self) -> Result<SocketAddr, MediaError> {
        Ok(self.socket.local_addr()?)
    }

    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> Result<(), MediaError> {
        Ok(self.socket.set_read_timeout(timeout)?)
    }

    pub fn receive_once(&mut self, now: Instant) -> Result<Option<EncodedFrame>, MediaError> {
        let (received, _source) = self.socket.recv_from(&mut self.buffer)?;
        if received > MAX_DATAGRAM_BYTES {
            return Err(MediaError::MalformedPacket);
        }
        self.reassembler.accept(&self.buffer[..received], now)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SendStats {
    pub datagrams: usize,
    pub bytes: usize,
}

#[derive(Debug, Error)]
pub enum MediaError {
    #[error("encoded frame is empty")]
    EmptyFrame,
    #[error("encoded frame is too large: {0} bytes")]
    FrameTooLarge(usize),
    #[error("encoded frame requires too many fragments: {0}")]
    TooManyFragments(usize),
    #[error("media packet sequence is exhausted")]
    SequenceExhausted,
    #[error("media datagram is malformed")]
    MalformedPacket,
    #[error("media datagram belongs to another session")]
    WrongSession,
    #[error("media datagram authentication failed")]
    Authentication,
    #[error("media datagram {0} was replayed or fell outside the replay window")]
    Replay(u64),
    #[error("media datagram has invalid fragment metadata")]
    InvalidFragment,
    #[error("frame {0} is older than the last completed frame")]
    StaleFrame(u64),
    #[error("frame {0} has inconsistent fragment metadata")]
    InconsistentFrame(u64),
    #[error("frame {frame_id} fragment {fragment_index} was received twice")]
    DuplicateFragment { frame_id: u64, fragment_index: u16 },
    #[error("UDP sent {actual} bytes instead of the expected {expected}")]
    ShortDatagramWrite { expected: usize, actual: usize },
    #[error(transparent)]
    Io(#[from] io::Error),
}

#[derive(Clone, Copy)]
struct PacketHeader {
    flags: u8,
    session_id: Uuid,
    packet_sequence: u64,
    frame_id: u64,
    fragment_index: u16,
    fragment_count: u16,
    timestamp_micros: u64,
}

impl PacketHeader {
    fn keyframe(&self) -> bool {
        self.flags & KEYFRAME_FLAG != 0
    }
}

struct IncompleteFrame {
    fragment_count: u16,
    timestamp_micros: u64,
    keyframe: bool,
    fragments: Vec<Option<Vec<u8>>>,
    received_fragments: usize,
    total_bytes: usize,
    first_seen: Instant,
}

impl IncompleteFrame {
    fn new(header: &PacketHeader, first_seen: Instant) -> Self {
        Self {
            fragment_count: header.fragment_count,
            timestamp_micros: header.timestamp_micros,
            keyframe: header.keyframe(),
            fragments: vec![None; usize::from(header.fragment_count)],
            received_fragments: 0,
            total_bytes: 0,
            first_seen,
        }
    }
}

#[derive(Default)]
struct ReplayWindow {
    highest: Option<u64>,
    bitmap: u128,
}

impl ReplayWindow {
    fn accept(&mut self, sequence: u64) -> bool {
        let Some(highest) = self.highest else {
            self.highest = Some(sequence);
            self.bitmap = 1;
            return true;
        };
        if sequence > highest {
            let shift = sequence - highest;
            self.bitmap = if shift >= REPLAY_WINDOW_BITS {
                1
            } else {
                (self.bitmap << shift) | 1
            };
            self.highest = Some(sequence);
            return true;
        }
        let distance = highest - sequence;
        if distance >= REPLAY_WINDOW_BITS {
            return false;
        }
        let bit = 1_u128 << distance;
        if self.bitmap & bit != 0 {
            return false;
        }
        self.bitmap |= bit;
        true
    }
}

fn encode_header(header: PacketHeader) -> [u8; HEADER_BYTES] {
    let mut bytes = [0_u8; HEADER_BYTES];
    bytes[..4].copy_from_slice(&MAGIC);
    bytes[4] = FORMAT_VERSION;
    bytes[5] = header.flags;
    bytes[6..22].copy_from_slice(header.session_id.as_bytes());
    bytes[22..30].copy_from_slice(&header.packet_sequence.to_be_bytes());
    bytes[30..38].copy_from_slice(&header.frame_id.to_be_bytes());
    bytes[38..40].copy_from_slice(&header.fragment_index.to_be_bytes());
    bytes[40..42].copy_from_slice(&header.fragment_count.to_be_bytes());
    bytes[42..50].copy_from_slice(&header.timestamp_micros.to_be_bytes());
    bytes
}

fn bind_udp(address: SocketAddr, receiver: bool) -> io::Result<UdpSocket> {
    let socket = Socket::new(
        Domain::for_address(address),
        Type::DGRAM,
        Some(Protocol::UDP),
    )?;
    if receiver {
        socket.set_recv_buffer_size(SOCKET_BUFFER_BYTES)?;
    } else {
        socket.set_send_buffer_size(SOCKET_BUFFER_BYTES)?;
    }
    socket.bind(&address.into())?;
    Ok(socket.into())
}

fn decode_header(bytes: &[u8]) -> Result<PacketHeader, MediaError> {
    if bytes.len() != HEADER_BYTES
        || bytes[..4] != MAGIC
        || bytes[4] != FORMAT_VERSION
        || bytes[5] & !KEYFRAME_FLAG != 0
    {
        return Err(MediaError::MalformedPacket);
    }
    Ok(PacketHeader {
        flags: bytes[5],
        session_id: Uuid::from_slice(&bytes[6..22]).map_err(|_| MediaError::MalformedPacket)?,
        packet_sequence: u64::from_be_bytes(
            bytes[22..30]
                .try_into()
                .map_err(|_| MediaError::MalformedPacket)?,
        ),
        frame_id: u64::from_be_bytes(
            bytes[30..38]
                .try_into()
                .map_err(|_| MediaError::MalformedPacket)?,
        ),
        fragment_index: u16::from_be_bytes(
            bytes[38..40]
                .try_into()
                .map_err(|_| MediaError::MalformedPacket)?,
        ),
        fragment_count: u16::from_be_bytes(
            bytes[40..42]
                .try_into()
                .map_err(|_| MediaError::MalformedPacket)?,
        ),
        timestamp_micros: u64::from_be_bytes(
            bytes[42..50]
                .try_into()
                .map_err(|_| MediaError::MalformedPacket)?,
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn material() -> MediaKeyMaterial {
        let mut bytes = [0_u8; MediaKeyMaterial::EXPORTER_BYTES];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = index as u8;
        }
        MediaKeyMaterial::from_exporter(bytes)
    }

    #[test]
    fn fragmented_frame_round_trips_with_bounded_reordering() {
        let session_id = Uuid::new_v4();
        let payload: Vec<u8> = (0..200_000).map(|index| (index % 251) as u8).collect();
        let mut sender = MediaPacketizer::new(session_id, material());
        let mut packets = sender.packetize(7, 123_456, true, &payload).unwrap();
        packets[..8].rotate_left(5);
        let mut receiver = MediaReassembler::new(session_id, material());
        let now = Instant::now();
        let mut completed = None;
        for packet in packets {
            if let Some(frame) = receiver.accept(&packet, now).unwrap() {
                completed = Some(frame);
            }
        }
        assert_eq!(
            completed,
            Some(EncodedFrame {
                frame_id: 7,
                timestamp_micros: 123_456,
                keyframe: true,
                payload,
            })
        );
        assert_eq!(receiver.pending_frames(), 0);
    }

    #[test]
    fn authentication_failure_does_not_consume_the_replay_sequence() {
        let session_id = Uuid::new_v4();
        let mut sender = MediaPacketizer::new(session_id, material());
        let original = sender.packetize(1, 1, false, b"frame").unwrap().remove(0);
        let mut tampered = original.clone();
        *tampered.last_mut().unwrap() ^= 0x80;
        let mut receiver = MediaReassembler::new(session_id, material());
        assert!(matches!(
            receiver.accept(&tampered, Instant::now()),
            Err(MediaError::Authentication)
        ));
        assert!(
            receiver
                .accept(&original, Instant::now())
                .unwrap()
                .is_some()
        );
        assert!(matches!(
            receiver.accept(&original, Instant::now()),
            Err(MediaError::StaleFrame(1) | MediaError::Replay(0))
        ));
    }

    #[test]
    fn wrong_session_and_stale_incomplete_frames_are_dropped() {
        let session_id = Uuid::new_v4();
        let mut sender = MediaPacketizer::new(session_id, material());
        let large = vec![7_u8; MAX_FRAGMENT_PAYLOAD + 1];
        let incomplete = sender.packetize(1, 1, false, &large).unwrap().remove(0);
        let mut receiver = MediaReassembler::new(Uuid::new_v4(), material());
        assert!(matches!(
            receiver.accept(&incomplete, Instant::now()),
            Err(MediaError::WrongSession)
        ));

        let mut receiver = MediaReassembler::new(session_id, material());
        let started = Instant::now();
        assert!(receiver.accept(&incomplete, started).unwrap().is_none());
        assert_eq!(receiver.pending_frames(), 1);
        let next = sender.packetize(2, 2, true, b"next").unwrap().remove(0);
        let completed = receiver
            .accept(&next, started + FRAME_TTL + Duration::from_millis(1))
            .unwrap()
            .unwrap();
        assert_eq!(completed.payload, b"next");
        assert_eq!(receiver.pending_frames(), 0);
    }

    #[test]
    fn udp_sender_and_receiver_exchange_an_encoded_access_unit() {
        let session_id = Uuid::new_v4();
        let loopback = "127.0.0.1:0".parse().unwrap();
        let mut receiver = UdpMediaReceiver::bind(loopback, session_id, material()).unwrap();
        receiver
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let receiver_address = receiver.local_addr().unwrap();
        let receiver_worker = std::thread::spawn(move || {
            let receive_batch_started = Instant::now();
            loop {
                if let Some(frame) = receiver.receive_once(receive_batch_started).unwrap() {
                    break frame;
                }
            }
        });
        let mut sender =
            UdpMediaSender::connect(loopback, receiver_address, session_id, material()).unwrap();
        let payload = vec![0x65_u8; 70_000];
        let stats = sender.send_frame(42, 9_000, true, &payload).unwrap();
        assert!(stats.datagrams > 1);

        let completed = receiver_worker.join().unwrap();
        assert_eq!(completed.frame_id, 42);
        assert!(completed.keyframe);
        assert_eq!(completed.payload, payload);
    }

    #[test]
    fn key_material_debug_output_is_redacted() {
        let debug = format!("{:?}", material());
        assert!(debug.contains("REDACTED"));
        assert!(!debug.contains("0, 1, 2"));
    }
}
