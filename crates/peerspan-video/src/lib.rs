//! Windows video capability discovery and the native media pipeline boundary.
//!
//! PeerSpan only reports the pipeline as available when a real D3D11 video
//! device and D3D11-aware hardware H.264 encoder and decoder can all be opened.
//! Software transforms are deliberately not accepted by this probe.

use serde::{Deserialize, Serialize};
use thiserror::Error;

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
}
