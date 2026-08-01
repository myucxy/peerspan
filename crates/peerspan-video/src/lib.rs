//! Windows video capability discovery and the native media pipeline boundary.
//!
//! PeerSpan only reports the pipeline as available when a real D3D11 video
//! device and D3D11-aware hardware H.264 encoder and decoder can all be opened.
//! Software transforms are deliberately not accepted by this probe.

use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

pub const IDD_SHARED_TEXTURE_PREFIX: &str = "Global\\PeerSpan.Idd.Frame.v1";

pub fn idd_shared_texture_name(width: u32, height: u32) -> String {
    format!("{IDD_SHARED_TEXTURE_PREFIX}.{width}x{height}")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoCapability {
    pub d3d11_feature_level: String,
    pub encoder: TransformCapability,
    pub decoder: TransformCapability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransformCapability {
    pub name: String,
    pub d3d11_aware: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncoderConfig {
    pub width: u32,
    pub height: u32,
    pub frames_per_second: u32,
    pub bitrate: u32,
}

impl Default for EncoderConfig {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            frames_per_second: 60,
            bitrate: 12_000_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedAccessUnit {
    pub timestamp_micros: u64,
    pub keyframe: bool,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecoderConfig {
    pub width: u32,
    pub height: u32,
    pub frames_per_second: u32,
}

impl Default for DecoderConfig {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            frames_per_second: 60,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedNv12Frame {
    pub timestamp_micros: u64,
    pub width: u32,
    pub height: u32,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Error)]
pub enum VideoError {
    #[error("Windows Media Foundation video is only available on Windows")]
    UnsupportedPlatform,
    #[error("could not initialize COM: {0}")]
    Com(String),
    #[error("could not initialize Media Foundation: {0}")]
    MediaFoundation(String),
    #[error("could not create a D3D11 hardware video device: {0}")]
    D3d11(String),
    #[error("no usable D3D11-aware H.264 {kind} is available")]
    MissingTransform { kind: &'static str },
    #[error("could not enumerate the hardware H.264 {kind}: {detail}")]
    TransformEnumeration { kind: &'static str, detail: String },
    #[error("invalid H.264 encoder configuration: {0}")]
    InvalidConfiguration(String),
    #[error("NV12 frame length is {actual} bytes; expected {expected}")]
    InvalidNv12Frame { expected: usize, actual: usize },
    #[error("hardware H.264 codec operation failed: {0}")]
    Codec(String),
    #[error("hardware H.264 codec did not produce output within {0} ms")]
    CodecTimeout(u64),
    #[error("could not open the IddCx shared D3D11 frame texture: {0}")]
    SharedTexture(String),
}

#[cfg(windows)]
mod windows_impl;

/// Probes the hardware path PeerSpan requires for a zero-copy H.264 session.
pub fn probe_hardware_h264() -> Result<VideoCapability, VideoError> {
    #[cfg(windows)]
    {
        windows_impl::probe_hardware_h264()
    }
    #[cfg(not(windows))]
    {
        Err(VideoError::UnsupportedPlatform)
    }
}

/// A Media Foundation hardware encoder fed through an NV12 D3D11 surface.
pub struct HardwareH264Encoder {
    #[cfg(windows)]
    inner: windows_impl::HardwareH264Encoder,
}

pub struct HardwareH264Decoder {
    #[cfg(windows)]
    inner: windows_impl::HardwareH264Decoder,
}

/// Consumes the IddCx driver's keyed-mutex BGRA texture, converts it to NV12
/// on the GPU, and emits H.264 access units through the hardware encoder.
pub struct SharedIddFrameEncoder {
    #[cfg(windows)]
    inner: windows_impl::SharedIddFrameEncoder,
}

impl HardwareH264Encoder {
    pub fn new(config: EncoderConfig) -> Result<Self, VideoError> {
        #[cfg(windows)]
        {
            Ok(Self {
                inner: windows_impl::HardwareH264Encoder::new(config)?,
            })
        }
        #[cfg(not(windows))]
        {
            let _ = config;
            Err(VideoError::UnsupportedPlatform)
        }
    }

    pub fn encode_nv12(
        &mut self,
        frame: &[u8],
        timestamp_micros: u64,
    ) -> Result<EncodedAccessUnit, VideoError> {
        #[cfg(windows)]
        {
            self.inner.encode_nv12(frame, timestamp_micros)
        }
        #[cfg(not(windows))]
        {
            let _ = (frame, timestamp_micros);
            Err(VideoError::UnsupportedPlatform)
        }
    }
}

impl HardwareH264Decoder {
    pub fn new(config: DecoderConfig) -> Result<Self, VideoError> {
        #[cfg(windows)]
        {
            Ok(Self {
                inner: windows_impl::HardwareH264Decoder::new(config)?,
            })
        }
        #[cfg(not(windows))]
        {
            let _ = config;
            Err(VideoError::UnsupportedPlatform)
        }
    }

    pub fn decode(
        &mut self,
        access_unit: &[u8],
        timestamp_micros: u64,
    ) -> Result<Option<DecodedNv12Frame>, VideoError> {
        #[cfg(windows)]
        {
            self.inner.decode(access_unit, timestamp_micros)
        }
        #[cfg(not(windows))]
        {
            let _ = (access_unit, timestamp_micros);
            Err(VideoError::UnsupportedPlatform)
        }
    }
}

impl SharedIddFrameEncoder {
    pub fn open(config: EncoderConfig) -> Result<Self, VideoError> {
        #[cfg(windows)]
        {
            let texture_name = idd_shared_texture_name(config.width, config.height);
            Ok(Self {
                inner: windows_impl::SharedIddFrameEncoder::open(config, &texture_name)?,
            })
        }
        #[cfg(not(windows))]
        {
            let _ = config;
            Err(VideoError::UnsupportedPlatform)
        }
    }

    pub fn encode_next(
        &mut self,
        timestamp_micros: u64,
        timeout: Duration,
    ) -> Result<Option<EncodedAccessUnit>, VideoError> {
        #[cfg(windows)]
        {
            self.inner.encode_next(timestamp_micros, timeout)
        }
        #[cfg(not(windows))]
        {
            let _ = (timestamp_micros, timeout);
            Err(VideoError::UnsupportedPlatform)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_serializes_with_stable_field_names() {
        let capability = VideoCapability {
            d3d11_feature_level: "11.1".into(),
            encoder: TransformCapability {
                name: "hardware encoder".into(),
                d3d11_aware: true,
            },
            decoder: TransformCapability {
                name: "hardware decoder".into(),
                d3d11_aware: true,
            },
        };

        let value = serde_json::to_value(&capability).expect("capability should serialize");
        assert_eq!(value["d3d11FeatureLevel"], "11.1");
        assert_eq!(value["encoder"]["d3d11Aware"], true);
    }

    #[test]
    fn default_encoder_configuration_matches_the_mvp_mode() {
        let config = EncoderConfig::default();
        assert_eq!((config.width, config.height), (1920, 1080));
        assert_eq!(config.frames_per_second, 60);
        assert_eq!(config.bitrate, 12_000_000);
    }

    #[test]
    fn default_decoder_configuration_matches_the_mvp_mode() {
        let config = DecoderConfig::default();
        assert_eq!((config.width, config.height), (1920, 1080));
        assert_eq!(config.frames_per_second, 60);
    }

    #[test]
    fn shared_texture_name_is_bound_to_the_negotiated_mode() {
        assert_eq!(
            idd_shared_texture_name(1920, 1080),
            "Global\\PeerSpan.Idd.Frame.v1.1920x1080"
        );
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires a D3D11-aware hardware H.264 encoder and decoder"]
    fn hardware_h264_round_trip_produces_a_real_decoded_surface() {
        let encoder_config = EncoderConfig {
            width: 640,
            height: 360,
            frames_per_second: 60,
            bitrate: 4_000_000,
        };
        let y_plane_bytes = (encoder_config.width * encoder_config.height) as usize;
        let mut input = vec![16_u8; y_plane_bytes + y_plane_bytes / 2];
        input[y_plane_bytes..].fill(128);
        let mut encoder =
            HardwareH264Encoder::new(encoder_config).expect("hardware encoder should initialize");
        let access_unit = encoder
            .encode_nv12(&input, 0)
            .expect("hardware encoder should emit an access unit");
        assert!(access_unit.keyframe);
        assert!(!access_unit.bytes.is_empty());

        let mut decoder = HardwareH264Decoder::new(DecoderConfig {
            width: encoder_config.width,
            height: encoder_config.height,
            frames_per_second: encoder_config.frames_per_second,
        })
        .expect("hardware decoder should initialize");
        let decoded = decoder
            .decode(&access_unit.bytes, access_unit.timestamp_micros)
            .expect("hardware decoder should accept the access unit")
            .expect("low-latency decoder should emit the first frame");
        assert_eq!((decoded.width, decoded.height), (640, 360));
        assert_eq!(decoded.bytes.len(), input.len());
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires D3D11 video processing and hardware H.264 codecs"]
    fn gpu_bgra_to_h264_to_nv12_round_trip() {
        windows_impl::test_gpu_bgra_h264_round_trip()
            .expect("GPU BGRA conversion and hardware codec round trip should succeed");
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires D3D11 shared textures, GPU conversion, and hardware H.264 encoding"]
    fn keyed_shared_bgra_texture_encodes_without_a_cpu_readback() {
        windows_impl::test_shared_texture_to_h264_round_trip()
            .expect("shared keyed texture should encode through the GPU pipeline");
    }
}
