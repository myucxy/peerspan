use peerspan_core::{Capability, PeerSpanCore};

pub fn probe_video_capability(core: &PeerSpanCore) {
    let capability = match peerspan_video::probe_hardware_h264() {
        Ok(video) => Capability::ready(format!(
            "D3D11 feature level {} detected with {} and {}; the keyed IDD encoder, authenticated UDP receiver, hardware decoder, and native D3D11 swap-chain presenter are available",
            video.d3d11_feature_level, video.encoder.name, video.decoder.name
        )),
        Err(error) => Capability::required(format!(
            "The required D3D11-aware H.264 hardware path is unavailable: {error}"
        )),
    };
    let _ = core.set_media_pipeline_capability(capability);
}
