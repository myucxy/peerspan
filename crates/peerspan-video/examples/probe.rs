fn main() {
    match peerspan_video::probe_hardware_h264() {
        Ok(capability) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&capability)
                    .expect("video capability should serialize")
            );
        }
        Err(error) => {
            eprintln!("PeerSpan video probe failed: {error}");
            std::process::exit(1);
        }
    }
}
