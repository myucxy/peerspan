use peerspan_core::{Capability, PeerSpanCore, QualityMode, StreamingBackend};
use rand_core::{OsRng, RngCore};
use reqwest::blocking::Client;
use serde_json::json;
use std::{
    env,
    fs::{self, File, OpenOptions},
    net::{IpAddr, SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;

const SUNSHINE_BASE_PORT: u16 = 47_989;
const SUNSHINE_HTTPS_PORT: u16 = SUNSHINE_BASE_PORT + 1;
const SUNSHINE_APP_NAME: &str = "PeerSpan Desktop";
const PROCESS_START_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone)]
pub struct GameStreamBinaries {
    pub sunshine: PathBuf,
    pub moonlight: PathBuf,
}

#[derive(Debug, Clone)]
pub struct GameStreamLaunch {
    pub session_id: Uuid,
    pub host: IpAddr,
    pub width: u32,
    pub height: u32,
    pub fps: u16,
    pub quality: QualityMode,
    pub pin: Option<String>,
}

pub struct GameStreamRuntime {
    binaries: Option<GameStreamBinaries>,
    data_dir: PathBuf,
    sunshine: Mutex<Option<Child>>,
    controller_username: String,
    controller_password: String,
}

impl GameStreamRuntime {
    pub fn discover(data_dir: impl Into<PathBuf>, resource_dir: Option<&Path>) -> Arc<Self> {
        let data_dir = data_dir.into().join("gamestream");
        let binaries = discover_binaries(resource_dir);
        let mut random = [0_u8; 24];
        OsRng.fill_bytes(&mut random);
        let password = hex::encode(random);
        Arc::new(Self {
            binaries,
            data_dir,
            sunshine: Mutex::new(None),
            controller_username: "peerspan-controller".into(),
            controller_password: password,
        })
    }

    #[cfg(test)]
    pub fn unavailable_for_tests(data_dir: impl Into<PathBuf>) -> Arc<Self> {
        Arc::new(Self {
            binaries: None,
            data_dir: data_dir.into(),
            sunshine: Mutex::new(None),
            controller_username: "test".into(),
            controller_password: "test".into(),
        })
    }

    pub fn apply_capability(&self, core: &PeerSpanCore) {
        let backend = core
            .snapshot()
            .map(|snapshot| snapshot.preferences.streaming_backend)
            .unwrap_or_default();
        let (backend_capability, media_capability) = match (backend, &self.binaries) {
            (StreamingBackend::SunshineMoonlight, Some(binaries)) => (
                Capability::ready(format!(
                    "Sunshine + Moonlight ready ({}; {})",
                    binaries.sunshine.display(),
                    binaries.moonlight.display()
                )),
                Capability::ready(
                    "GameStream hardware codecs, RTP/FEC pacing and direct input are available",
                ),
            ),
            (StreamingBackend::SunshineMoonlight, None) => (
                Capability::required(
                    "Sunshine and Moonlight were not found; reinstall the full PeerSpan package or set PEERSPAN_SUNSHINE_PATH and PEERSPAN_MOONLIGHT_PATH",
                ),
                Capability::required("The selected Sunshine + Moonlight backend is unavailable"),
            ),
            (StreamingBackend::Native, _) => (
                Capability::ready("PeerSpan native D3D11 / Media Foundation backend selected"),
                core.snapshot()
                    .map(|snapshot| snapshot.capabilities.media_pipeline)
                    .unwrap_or_else(|_| Capability::required("Native media probe failed")),
            ),
        };
        let _ = core.set_streaming_backend_capability(backend_capability);
        if backend == StreamingBackend::SunshineMoonlight {
            let _ = core.set_media_pipeline_capability(media_capability);
        }
    }

    pub fn is_available(&self) -> bool {
        self.binaries.is_some()
    }

    pub fn ensure_host(&self) -> Result<(), String> {
        let binaries = self
            .binaries
            .as_ref()
            .ok_or_else(|| "Sunshine + Moonlight runtime files are unavailable".to_owned())?;
        let mut sunshine = self
            .sunshine
            .lock()
            .map_err(|_| "Sunshine process lock is poisoned")?;
        if let Some(child) = sunshine.as_mut() {
            match child.try_wait() {
                Ok(None) => return Ok(()),
                Ok(Some(_)) => *sunshine = None,
                Err(error) => return Err(format!("could not inspect Sunshine: {error}")),
            }
        }

        let paths = self.prepare_host_files()?;
        let credential_status = hidden_command(&binaries.sunshine)
            .current_dir(binary_directory(&binaries.sunshine)?)
            .arg(&paths.config)
            .arg("--creds")
            .arg(&self.controller_username)
            .arg(&self.controller_password)
            .status()
            .map_err(|error| format!("could not configure Sunshine credentials: {error}"))?;
        if !credential_status.success() {
            return Err(format!(
                "Sunshine credential setup exited with {credential_status}"
            ));
        }

        let stdout = append_log(&paths.log)?;
        let stderr = stdout
            .try_clone()
            .map_err(|error| format!("could not clone Sunshine log handle: {error}"))?;
        let child = hidden_command(&binaries.sunshine)
            .current_dir(binary_directory(&binaries.sunshine)?)
            .arg(&paths.config)
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .spawn()
            .map_err(|error| format!("could not start Sunshine: {error}"))?;
        *sunshine = Some(child);

        let deadline = Instant::now() + PROCESS_START_TIMEOUT;
        loop {
            if TcpStream::connect_timeout(
                &SocketAddr::from(([127, 0, 0, 1], SUNSHINE_HTTPS_PORT)),
                Duration::from_millis(150),
            )
            .is_ok()
            {
                return Ok(());
            }
            if let Some(status) = sunshine
                .as_mut()
                .and_then(|child| child.try_wait().ok())
                .flatten()
            {
                *sunshine = None;
                return Err(format!("Sunshine exited during startup with {status}"));
            }
            if Instant::now() >= deadline {
                if let Some(mut child) = sunshine.take() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                return Err(format!(
                    "Sunshine did not open its local API port {SUNSHINE_HTTPS_PORT}"
                ));
            }
            thread::sleep(Duration::from_millis(150));
        }
    }

    pub fn submit_pairing_pin(&self, pin: &str, client_name: &str) -> Result<(), String> {
        validate_pin(pin)?;
        let client = Client::builder()
            .danger_accept_invalid_certs(true)
            .timeout(Duration::from_secs(2))
            .build()
            .map_err(|error| format!("could not create Sunshine API client: {error}"))?;
        let deadline = Instant::now() + Duration::from_secs(12);
        let mut last_error = String::new();
        while Instant::now() < deadline {
            match client
                .post(format!("https://127.0.0.1:{SUNSHINE_HTTPS_PORT}/api/pin"))
                .basic_auth(&self.controller_username, Some(&self.controller_password))
                .json(&json!({ "pin": pin, "name": client_name }))
                .send()
            {
                Ok(response) if response.status().is_success() => {
                    match response.json::<serde_json::Value>() {
                        Ok(body)
                            if body.get("status").and_then(|value| value.as_bool())
                                == Some(true) =>
                        {
                            return Ok(());
                        }
                        Ok(_) => last_error = "pairing request is not pending yet".into(),
                        Err(error) => last_error = format!("invalid API response: {error}"),
                    }
                }
                Ok(response) => last_error = format!("HTTP {}", response.status()),
                Err(error) => last_error = error.to_string(),
            }
            thread::sleep(Duration::from_millis(200));
        }
        Err(format!(
            "Sunshine rejected the Moonlight pairing PIN: {last_error}"
        ))
    }

    pub fn client_is_paired(&self, host: IpAddr) -> bool {
        let Some(binaries) = self.binaries.as_ref() else {
            return false;
        };
        let log_path = self.data_dir.join("moonlight-pairing-probe.log");
        let args = vec!["list".into(), host.to_string(), "--csv".into()];
        moonlight_command(&binaries.moonlight, &args, &log_path)
            .and_then(|command| run_with_timeout(command, Duration::from_secs(5)))
            .map(|status| status.success())
            .unwrap_or(false)
    }

    pub fn run_client(
        &self,
        launch: &GameStreamLaunch,
        runtime_shutdown: &AtomicBool,
        session_shutdown: &AtomicBool,
        startup: std::sync::mpsc::SyncSender<Result<(), String>>,
    ) {
        let result = self.run_client_inner(launch, runtime_shutdown, session_shutdown, &startup);
        if let Err(error) = result {
            let _ = startup.send(Err(error));
        }
    }

    fn run_client_inner(
        &self,
        launch: &GameStreamLaunch,
        runtime_shutdown: &AtomicBool,
        session_shutdown: &AtomicBool,
        startup: &std::sync::mpsc::SyncSender<Result<(), String>>,
    ) -> Result<(), String> {
        let binaries = self
            .binaries
            .as_ref()
            .ok_or_else(|| "Moonlight runtime files are unavailable".to_owned())?;
        if let Some(pin) = launch.pin.as_deref() {
            validate_pin(pin)?;
        }
        fs::create_dir_all(&self.data_dir)
            .map_err(|error| format!("could not create GameStream data directory: {error}"))?;
        let log_path = self
            .data_dir
            .join(format!("moonlight-{}.log", launch.session_id));

        if let Some(pin) = launch.pin.as_deref() {
            let pair_args = moonlight_pair_args(launch.host, pin);
            let pair_status = run_with_timeout(
                moonlight_command(&binaries.moonlight, &pair_args, &log_path)?,
                PROCESS_START_TIMEOUT,
            )?;
            if !pair_status.success() {
                return Err(format!("Moonlight pairing exited with {pair_status}"));
            }
        }

        let stream_args = moonlight_stream_args(launch);
        let mut child = moonlight_command(&binaries.moonlight, &stream_args, &log_path)?
            .spawn()
            .map_err(|error| format!("could not start Moonlight: {error}"))?;
        let ready_at = Instant::now() + Duration::from_secs(1);
        while Instant::now() < ready_at {
            if let Some(status) = child
                .try_wait()
                .map_err(|error| format!("could not inspect Moonlight: {error}"))?
            {
                return Err(format!("Moonlight exited during startup with {status}"));
            }
            thread::sleep(Duration::from_millis(100));
        }
        let _ = startup.send(Ok(()));

        while !runtime_shutdown.load(Ordering::Relaxed) && !session_shutdown.load(Ordering::Relaxed)
        {
            if child
                .try_wait()
                .map_err(|error| format!("could not inspect Moonlight: {error}"))?
                .is_some()
            {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(100));
        }
        let _ = child.kill();
        let _ = child.wait();
        Ok(())
    }

    fn prepare_host_files(&self) -> Result<HostPaths, String> {
        let host_dir = self.data_dir.join("sunshine");
        let credentials_dir = host_dir.join("credentials");
        fs::create_dir_all(&credentials_dir)
            .map_err(|error| format!("could not create Sunshine data directory: {error}"))?;
        let config = host_dir.join("sunshine.conf");
        let apps = host_dir.join("apps.json");
        let log = host_dir.join("sunshine.log");
        let state = host_dir.join("sunshine_state.json");
        let cert = credentials_dir.join("cacert.pem");
        let key = credentials_dir.join("cakey.pem");
        let output_name = peerspan_display_name().unwrap_or_default();
        let config_text = sunshine_config(&apps, &state, &log, &cert, &key, &output_name);
        fs::write(&config, config_text)
            .map_err(|error| format!("could not write Sunshine configuration: {error}"))?;
        fs::write(
            &apps,
            serde_json::to_vec_pretty(&json!({
                "env": {},
                "apps": [{
                    "name": SUNSHINE_APP_NAME,
                    "prep-cmd": [],
                    "detached": []
                }]
            }))
            .map_err(|error| format!("could not serialize Sunshine applications: {error}"))?,
        )
        .map_err(|error| format!("could not write Sunshine applications: {error}"))?;
        Ok(HostPaths { config, log })
    }
}

impl Drop for GameStreamRuntime {
    fn drop(&mut self) {
        if let Ok(sunshine) = self.sunshine.get_mut()
            && let Some(mut child) = sunshine.take()
        {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

struct HostPaths {
    config: PathBuf,
    log: PathBuf,
}

fn discover_binaries(resource_dir: Option<&Path>) -> Option<GameStreamBinaries> {
    let sunshine_override = env::var_os("PEERSPAN_SUNSHINE_PATH").map(PathBuf::from);
    let moonlight_override = env::var_os("PEERSPAN_MOONLIGHT_PATH").map(PathBuf::from);
    if let (Some(sunshine), Some(moonlight)) = (sunshine_override, moonlight_override)
        && sunshine.is_file()
        && moonlight.is_file()
    {
        return Some(GameStreamBinaries {
            sunshine,
            moonlight,
        });
    }

    let mut roots = Vec::new();
    if let Some(resource_dir) = resource_dir {
        roots.push(resource_dir.to_path_buf());
    }
    if let Ok(executable) = env::current_exe()
        && let Some(parent) = executable.parent()
    {
        roots.push(parent.to_path_buf());
    }
    roots.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../target/installer-resources"),
    );
    roots.push(PathBuf::from(r"D:\Dev\Env\PeerSpan\runtimes"));

    roots.into_iter().find_map(|root| {
        let sunshine_candidates = [
            root.join("gamestream/sunshine/Sunshine/sunshine.exe"),
            root.join("sunshine/Sunshine/sunshine.exe"),
        ];
        let moonlight_candidates = [
            root.join("gamestream/moonlight/Moonlight.exe"),
            root.join("moonlight/Moonlight.exe"),
        ];
        let sunshine = sunshine_candidates
            .into_iter()
            .find(|path| path.is_file())?;
        let moonlight = moonlight_candidates
            .into_iter()
            .find(|path| path.is_file())?;
        Some(GameStreamBinaries {
            sunshine,
            moonlight,
        })
    })
}

fn moonlight_pair_args(host: IpAddr, pin: &str) -> Vec<String> {
    vec!["pair".into(), host.to_string(), "--pin".into(), pin.into()]
}

fn moonlight_stream_args(launch: &GameStreamLaunch) -> Vec<String> {
    vec![
        "stream".into(),
        launch.host.to_string(),
        SUNSHINE_APP_NAME.into(),
        "--resolution".into(),
        format!("{}x{}", launch.width, launch.height),
        "--fps".into(),
        launch.fps.to_string(),
        "--bitrate".into(),
        bitrate_kbps(launch.quality).to_string(),
        "--display-mode".into(),
        "windowed".into(),
        "--absolute-mouse".into(),
        "--video-decoder".into(),
        "hardware".into(),
        "--video-codec".into(),
        "H.264".into(),
        "--capture-system-keys".into(),
        "always".into(),
        "--quit-after".into(),
    ]
}

fn moonlight_command(
    executable: &Path,
    args: &[String],
    log_path: &Path,
) -> Result<Command, String> {
    let stdout = append_log(log_path)?;
    let stderr = stdout
        .try_clone()
        .map_err(|error| format!("could not clone Moonlight log handle: {error}"))?;
    let mut command = Command::new(executable);
    command
        .current_dir(binary_directory(executable)?)
        .args(args)
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    Ok(command)
}

fn run_with_timeout(mut command: Command, timeout: Duration) -> Result<ExitStatus, String> {
    let mut child = command
        .spawn()
        .map_err(|error| format!("could not start child process: {error}"))?;
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("could not inspect child process: {error}"))?
        {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("child process timed out".into());
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn append_log(path: &Path) -> Result<File, String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("could not create log directory: {error}"))?;
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| format!("could not open {}: {error}", path.display()))
}

fn binary_directory(executable: &Path) -> Result<&Path, String> {
    executable
        .parent()
        .ok_or_else(|| format!("binary path has no parent: {}", executable.display()))
}

fn sunshine_config(
    apps: &Path,
    state: &Path,
    log: &Path,
    cert: &Path,
    key: &Path,
    output_name: &str,
) -> String {
    let mut lines = vec![
        format!("port = {SUNSHINE_BASE_PORT}"),
        "upnp = disabled".into(),
        "origin_web_ui_allowed = pc".into(),
        format!("file_apps = {}", config_path(apps)),
        format!("credentials_file = {}", config_path(state)),
        format!("cert = {}", config_path(cert)),
        format!("pkey = {}", config_path(key)),
        format!("log_path = {}", config_path(log)),
    ];
    if !output_name.is_empty() {
        lines.push(format!("output_name = {output_name}"));
    }
    lines.join("\n") + "\n"
}

fn config_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn bitrate_kbps(quality: QualityMode) -> u32 {
    match quality {
        QualityMode::Clarity => 20_000,
        QualityMode::Balanced => 12_000,
        QualityMode::Responsive => 8_000,
    }
}

pub fn generate_pairing_pin() -> String {
    let value = OsRng.next_u32() % 10_000;
    format!("{value:04}")
}

fn validate_pin(pin: &str) -> Result<(), String> {
    if pin.len() == 4 && pin.bytes().all(|byte| byte.is_ascii_digit()) {
        Ok(())
    } else {
        Err("GameStream pairing PIN must contain exactly four digits".into())
    }
}

#[cfg(windows)]
fn hidden_command(program: &Path) -> Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut command = Command::new(program);
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

#[cfg(not(windows))]
fn hidden_command(program: &Path) -> Command {
    Command::new(program)
}

#[cfg(windows)]
fn peerspan_display_name() -> Option<String> {
    use std::{mem::size_of, ptr};
    use windows_sys::Win32::Graphics::Gdi::{
        DISPLAY_DEVICE_ACTIVE, DISPLAY_DEVICE_ATTACHED_TO_DESKTOP, DISPLAY_DEVICEW,
        EnumDisplayDevicesW,
    };
    for index in 0..32 {
        let mut adapter = DISPLAY_DEVICEW {
            cb: size_of::<DISPLAY_DEVICEW>() as u32,
            ..Default::default()
        };
        if unsafe { EnumDisplayDevicesW(ptr::null(), index, &mut adapter, 0) } == 0 {
            break;
        }
        if adapter.StateFlags & (DISPLAY_DEVICE_ACTIVE | DISPLAY_DEVICE_ATTACHED_TO_DESKTOP) == 0 {
            continue;
        }
        let mut matches =
            contains_peerspan(&adapter.DeviceString) || contains_peerspan(&adapter.DeviceID);
        for monitor_index in 0..16 {
            let mut monitor = DISPLAY_DEVICEW {
                cb: size_of::<DISPLAY_DEVICEW>() as u32,
                ..Default::default()
            };
            if unsafe {
                EnumDisplayDevicesW(adapter.DeviceName.as_ptr(), monitor_index, &mut monitor, 0)
            } == 0
            {
                break;
            }
            matches |=
                contains_peerspan(&monitor.DeviceString) || contains_peerspan(&monitor.DeviceID);
        }
        if matches {
            return Some(wide_string(&adapter.DeviceName));
        }
    }
    None
}

#[cfg(windows)]
fn contains_peerspan(value: &[u16]) -> bool {
    wide_string(value).to_ascii_lowercase().contains("peerspan")
}

#[cfg(windows)]
fn wide_string(value: &[u16]) -> String {
    let end = value
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(value.len());
    String::from_utf16_lossy(&value[..end])
}

#[cfg(not(windows))]
fn peerspan_display_name() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairing_pin_is_always_four_digits() {
        for _ in 0..64 {
            let pin = generate_pairing_pin();
            assert_eq!(pin.len(), 4);
            assert!(pin.bytes().all(|byte| byte.is_ascii_digit()));
        }
    }

    #[test]
    fn moonlight_arguments_are_discrete_and_performance_tuned() {
        let launch = GameStreamLaunch {
            session_id: Uuid::nil(),
            host: "192.168.9.26".parse().unwrap(),
            width: 1920,
            height: 1080,
            fps: 60,
            quality: QualityMode::Balanced,
            pin: Some("0042".into()),
        };
        assert_eq!(
            moonlight_pair_args(launch.host, launch.pin.as_deref().unwrap()),
            ["pair", "192.168.9.26", "--pin", "0042"]
        );
        let args = moonlight_stream_args(&launch);
        assert_eq!(args[0], "stream");
        assert!(args.windows(2).any(|pair| pair == ["--bitrate", "12000"]));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--video-decoder", "hardware"])
        );
        assert!(args.contains(&"--absolute-mouse".into()));
    }

    #[test]
    fn sunshine_configuration_uses_explicit_app_data_paths() {
        let text = sunshine_config(
            Path::new(r"C:\Users\Test User\apps.json"),
            Path::new(r"C:\Users\Test User\state.json"),
            Path::new(r"C:\Users\Test User\sunshine.log"),
            Path::new(r"C:\Users\Test User\cert.pem"),
            Path::new(r"C:\Users\Test User\key.pem"),
            r"\\.\DISPLAY7",
        );
        assert!(text.contains("file_apps = C:/Users/Test User/apps.json"));
        assert!(text.contains(r"output_name = \\.\DISPLAY7"));
        assert!(!text.contains("peerspan-controller"));
    }
}
