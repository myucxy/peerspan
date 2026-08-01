fn main() {
    match run() {
        Ok(()) => {}
        Err(error) => {
            eprintln!("PeerSpan video probe failed: {error}");
            std::process::exit(1);
        }
    }
}

fn run() -> Result<(), peerspan_video::VideoError> {
    let capability = peerspan_video::probe_hardware_h264()?;
    println!(
        "{}",
        serde_json::to_string_pretty(&capability).expect("video capability should serialize")
    );

    let config = peerspan_video::EncoderConfig {
        width: 640,
        height: 360,
        frames_per_second: 60,
        bitrate: 4_000_000,
    };
    let y_plane_bytes = (config.width * config.height) as usize;
    let mut nv12 = vec![16_u8; y_plane_bytes + y_plane_bytes / 2];
    nv12[y_plane_bytes..].fill(128);
    let mut encoder = peerspan_video::HardwareH264Encoder::new(config)?;
    let access_unit = encoder.encode_nv12(&nv12, 0)?;
    println!(
        "encodedAccessUnitBytes={} keyframe={}",
        access_unit.bytes.len(),
        access_unit.keyframe
    );
    let mut decoder = peerspan_video::HardwareH264Decoder::new(peerspan_video::DecoderConfig {
        width: config.width,
        height: config.height,
        frames_per_second: config.frames_per_second,
    })?;
    let decoded = decoder
        .decode(&access_unit.bytes, access_unit.timestamp_micros)?
        .ok_or_else(|| {
            peerspan_video::VideoError::Codec(
                "decoder buffered the first low-latency access unit".into(),
            )
        })?;
    println!(
        "decodedNv12Bytes={} dimensions={}x{}",
        decoded.bytes.len(),
        decoded.width,
        decoded.height
    );
    Ok(())
}
