use peerspan_core::{Capability, PeerSpanCore, ScreenEdge};
use std::sync::{Arc, Mutex};

const INACTIVE_DETAIL: &str = "VirtualDrivers VDD is inactive; install the bundled signed driver and enable the virtual screen when needed";

trait DisplayLease: Send {
    fn instance_id(&self) -> &str;
}

trait VirtualDisplayBackend: Send + Sync {
    fn activate(&self) -> Result<Box<dyn DisplayLease>, String>;
    fn position(&self, edge: ScreenEdge) -> Result<(), String>;
}

pub struct VirtualDisplayRuntime {
    core: Arc<PeerSpanCore>,
    backend: Box<dyn VirtualDisplayBackend>,
    active: Mutex<Option<Box<dyn DisplayLease>>>,
}

impl VirtualDisplayRuntime {
    pub fn new(core: Arc<PeerSpanCore>) -> Self {
        let runtime = Self::with_backend(core, platform::backend());
        let _ = runtime
            .core
            .set_virtual_display_capability(Capability::required(INACTIVE_DETAIL));
        runtime
    }

    fn with_backend(core: Arc<PeerSpanCore>, backend: Box<dyn VirtualDisplayBackend>) -> Self {
        Self {
            core,
            backend,
            active: Mutex::new(None),
        }
    }

    pub fn start(&self) -> Result<(), String> {
        let edge = self
            .core
            .snapshot()
            .map_err(|error| error.to_string())?
            .preferences
            .screen_edge;
        let mut active = self
            .active
            .lock()
            .map_err(|_| "virtual display state lock is poisoned")?;
        if let Some(lease) = active.as_ref() {
            if let Err(error) = self.backend.position(edge) {
                let _ = self
                    .core
                    .set_virtual_display_capability(Capability::required(format!(
                        "VirtualDrivers VDD remains active, but its layout could not be applied: {error}"
                    )));
                return Err(error);
            }
            self.mark_ready(lease.instance_id())?;
            return Ok(());
        }

        match self.backend.activate() {
            Ok(lease) => {
                let instance_id = lease.instance_id().to_owned();
                // Keep the software-device lease as soon as Windows starts the device node.
                // RDP can temporarily veto display-topology refreshes; dropping the lease here
                // would turn a healthy device into CM_PROB_PHANTOM and make retry impossible.
                *active = Some(lease);
                if let Err(error) = self.backend.position(edge) {
                    let _ = self
                        .core
                        .set_virtual_display_capability(Capability::required(format!(
                            "VirtualDrivers VDD is started and retained, but its layout could not be applied: {error}. End the RDP display session if it owns the display topology, then retry"
                        )));
                    return Err(error);
                }
                self.mark_ready(&instance_id)
            }
            Err(error) => {
                let detail = format!(
                    "VirtualDrivers VDD could not start: {error}. Install or repair the bundled signed driver, then retry"
                );
                let _ = self
                    .core
                    .set_virtual_display_capability(Capability::required(detail));
                Err(error)
            }
        }
    }

    pub fn apply_layout(&self, edge: ScreenEdge) -> Result<(), String> {
        if self
            .active
            .lock()
            .map_err(|_| "virtual display state lock is poisoned")?
            .is_some()
        {
            self.backend.position(edge)?;
        }
        Ok(())
    }

    pub fn stop(&self) -> Result<(), String> {
        if !self
            .core
            .snapshot()
            .map_err(|error| error.to_string())?
            .display_sessions
            .is_empty()
        {
            return Err(
                "End all active display sessions before removing the virtual display".into(),
            );
        }

        let lease = self
            .active
            .lock()
            .map_err(|_| "virtual display state lock is poisoned")?
            .take();
        drop(lease);
        self.core
            .set_virtual_display_capability(Capability::required(INACTIVE_DETAIL))
            .map_err(|error| error.to_string())
    }

    fn mark_ready(&self, instance_id: &str) -> Result<(), String> {
        self.core
            .set_virtual_display_capability(Capability::ready(format!(
                "VirtualDrivers VDD is active and its device node is started ({instance_id})"
            )))
            .map_err(|error| error.to_string())
    }
}

#[cfg(windows)]
mod platform {
    use super::{DisplayLease, VirtualDisplayBackend};
    use peerspan_core::ScreenEdge;
    use std::{
        ffi::c_void,
        ptr,
        sync::{Arc, Condvar, Mutex},
        thread,
        time::{Duration, Instant},
    };
    use windows_sys::{
        Win32::Devices::{
            DeviceAndDriverInstallation::{
                CM_Get_DevNode_Status, CM_LOCATE_DEVNODE_NORMAL, CM_Locate_DevNodeW, CR_SUCCESS,
                DN_STARTED,
            },
            Enumeration::Pnp::{
                HSWDEVICE, SW_DEVICE_CREATE_INFO, SWDeviceCapabilitiesDriverRequired,
                SWDeviceCapabilitiesRemovable, SWDeviceCapabilitiesSilentInstall, SwDeviceClose,
                SwDeviceCreate,
            },
        },
        Win32::Graphics::Gdi::{
            CDS_UPDATEREGISTRY, ChangeDisplaySettingsExW, DEVMODEW, DISP_CHANGE_SUCCESSFUL,
            DISPLAY_DEVICE_ACTIVE, DISPLAY_DEVICE_ATTACHED_TO_DESKTOP,
            DISPLAY_DEVICE_PRIMARY_DEVICE, DISPLAY_DEVICEW, DM_DISPLAYFREQUENCY, DM_PELSHEIGHT,
            DM_PELSWIDTH, DM_POSITION, ENUM_CURRENT_SETTINGS, EnumDisplayDevicesW,
            EnumDisplaySettingsExW,
        },
        core::{HRESULT, PCWSTR},
    };

    const CREATE_TIMEOUT: Duration = Duration::from_secs(10);
    const START_TIMEOUT: Duration = Duration::from_secs(10);
    const MAX_INSTANCE_ID_UNITS: usize = 4096;

    pub(super) fn backend() -> Box<dyn VirtualDisplayBackend> {
        Box::new(WindowsVirtualDisplayBackend)
    }

    struct WindowsVirtualDisplayBackend;

    struct WindowsDisplayLease {
        handle: usize,
        instance_id: String,
    }

    impl DisplayLease for WindowsDisplayLease {
        fn instance_id(&self) -> &str {
            &self.instance_id
        }
    }

    impl Drop for WindowsDisplayLease {
        fn drop(&mut self) {
            if self.handle != 0 {
                // SAFETY: This handle was returned by SwDeviceCreate and is closed exactly once.
                unsafe { SwDeviceClose(self.handle as HSWDEVICE) };
                self.handle = 0;
            }
        }
    }

    #[derive(Clone)]
    struct CreationResult {
        hresult: HRESULT,
        instance_id: Option<String>,
    }

    struct CallbackContext {
        result: Mutex<Option<CreationResult>>,
        ready: Condvar,
    }

    unsafe extern "system" fn creation_callback(
        _device: HSWDEVICE,
        create_result: HRESULT,
        context: *const c_void,
        instance_id: PCWSTR,
    ) {
        if context.is_null() {
            return;
        }

        // SAFETY: activate reserved one Arc strong reference specifically for this single callback.
        let context = unsafe { Arc::from_raw(context.cast::<CallbackContext>()) };
        let instance_id = unsafe { wide_ptr_to_string(instance_id) };
        if let Ok(mut result) = context.result.lock() {
            *result = Some(CreationResult {
                hresult: create_result,
                instance_id,
            });
            context.ready.notify_all();
        }
    }

    impl VirtualDisplayBackend for WindowsVirtualDisplayBackend {
        fn activate(&self) -> Result<Box<dyn DisplayLease>, String> {
            // The upstream INF intentionally exposes the unrooted `MttVDD` hardware ID for
            // software-device hosts. Keeping the HSWDEVICE handle gives PeerSpan a reversible
            // per-session lease without modifying the signed VirtualDrivers package.
            let enumerator = wide("MttVDD");
            let parent = wide("HTREE\\ROOT\\0");
            let instance = wide("PeerSpanVirtualDisplay");
            let hardware_ids = multi_sz(&["MttVDD"]);
            let compatible_ids = multi_sz(&["MttVDD"]);
            let description = wide("PeerSpan Virtual Display (VirtualDrivers VDD)");
            let create_info = SW_DEVICE_CREATE_INFO {
                cbSize: size_of::<SW_DEVICE_CREATE_INFO>() as u32,
                pszInstanceId: instance.as_ptr(),
                pszzHardwareIds: hardware_ids.as_ptr(),
                pszzCompatibleIds: compatible_ids.as_ptr(),
                pContainerId: ptr::null(),
                CapabilityFlags: (SWDeviceCapabilitiesRemovable
                    | SWDeviceCapabilitiesSilentInstall
                    | SWDeviceCapabilitiesDriverRequired) as u32,
                pszDeviceDescription: description.as_ptr(),
                pszDeviceLocation: ptr::null(),
                pSecurityDescriptor: ptr::null(),
            };
            let callback = Arc::new(CallbackContext {
                result: Mutex::new(None),
                ready: Condvar::new(),
            });
            let callback_pointer = Arc::into_raw(Arc::clone(&callback));
            let mut handle: HSWDEVICE = ptr::null_mut();
            // SAFETY: All UTF-16 buffers and create_info live through the call. The callback owns
            // a reserved Arc reference and SwDeviceCreate writes only the output handle.
            let hresult = unsafe {
                SwDeviceCreate(
                    enumerator.as_ptr(),
                    parent.as_ptr(),
                    &create_info,
                    0,
                    ptr::null(),
                    Some(creation_callback),
                    callback_pointer.cast(),
                    &mut handle,
                )
            };
            if failed(hresult) {
                // SAFETY: A synchronous SwDeviceCreate failure does not schedule the callback, so
                // the reserved Arc reference must be reclaimed here.
                unsafe { drop(Arc::from_raw(callback_pointer)) };
                return Err(format!(
                    "SwDeviceCreate failed with {}",
                    format_hresult(hresult)
                ));
            }

            let creation = wait_for_creation(&callback).inspect_err(|_| {
                if !handle.is_null() {
                    // SAFETY: Closing the successfully returned handle cancels this activation.
                    unsafe { SwDeviceClose(handle) };
                    handle = ptr::null_mut();
                }
            })?;
            if failed(creation.hresult) {
                if !handle.is_null() {
                    // SAFETY: The callback completed and the handle has not otherwise been closed.
                    unsafe { SwDeviceClose(handle) };
                }
                return Err(format!(
                    "software device creation completed with {}",
                    format_hresult(creation.hresult)
                ));
            }
            let Some(instance_id) = creation.instance_id else {
                if !handle.is_null() {
                    // SAFETY: Creation succeeded, but the callback result is unusable; close the
                    // still-owned handle before returning the diagnostic error.
                    unsafe { SwDeviceClose(handle) };
                }
                return Err("Windows did not return the virtual display instance ID".into());
            };
            if let Err(error) = wait_until_started(&instance_id) {
                if !handle.is_null() {
                    // SAFETY: The device node did not start and the handle is still owned here.
                    unsafe { SwDeviceClose(handle) };
                }
                return Err(error);
            }

            Ok(Box::new(WindowsDisplayLease {
                handle: handle as usize,
                instance_id,
            }))
        }

        fn position(&self, edge: ScreenEdge) -> Result<(), String> {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                match position_display(edge) {
                    Ok(()) => return Ok(()),
                    Err(error) if Instant::now() < deadline => {
                        let _ = error;
                        thread::sleep(Duration::from_millis(100));
                    }
                    Err(error) => return Err(error),
                }
            }
        }
    }

    fn position_display(edge: ScreenEdge) -> Result<(), String> {
        let mut peerspan = None;
        let mut primary = None;
        for index in 0..32 {
            let mut adapter = DISPLAY_DEVICEW {
                cb: size_of::<DISPLAY_DEVICEW>() as u32,
                ..Default::default()
            };
            if unsafe { EnumDisplayDevicesW(ptr::null(), index, &mut adapter, 0) } == 0 {
                break;
            }
            if adapter.StateFlags & (DISPLAY_DEVICE_ACTIVE | DISPLAY_DEVICE_ATTACHED_TO_DESKTOP)
                == 0
            {
                continue;
            }
            if adapter.StateFlags & DISPLAY_DEVICE_PRIMARY_DEVICE != 0 {
                primary = Some(adapter.DeviceName);
            }
            let mut matches = contains_vdd_identity(&adapter.DeviceString)
                || contains_vdd_identity(&adapter.DeviceID);
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
                matches |= contains_vdd_identity(&monitor.DeviceString)
                    || contains_vdd_identity(&monitor.DeviceID);
            }
            if matches {
                peerspan = Some(adapter.DeviceName);
            }
        }
        let peerspan = peerspan.ok_or_else(|| {
            "VirtualDrivers VDD has not appeared in the Windows desktop topology".to_owned()
        })?;
        let primary =
            primary.ok_or_else(|| "Windows primary display could not be resolved".to_owned())?;
        let primary_mode = current_mode(&primary)?;
        let mut peerspan_mode = current_mode(&peerspan)?;
        peerspan_mode.dmPelsWidth = 1920;
        peerspan_mode.dmPelsHeight = 1080;
        peerspan_mode.dmDisplayFrequency = 60;
        let primary_position = unsafe { primary_mode.Anonymous1.Anonymous2.dmPosition };
        let (x, y) = match edge {
            ScreenEdge::Left => (
                primary_position.x - peerspan_mode.dmPelsWidth as i32,
                primary_position.y,
            ),
            ScreenEdge::Right => (
                primary_position.x + primary_mode.dmPelsWidth as i32,
                primary_position.y,
            ),
            ScreenEdge::Top => (
                primary_position.x,
                primary_position.y - peerspan_mode.dmPelsHeight as i32,
            ),
            ScreenEdge::Bottom => (
                primary_position.x,
                primary_position.y + primary_mode.dmPelsHeight as i32,
            ),
        };
        peerspan_mode.dmFields |= DM_POSITION | DM_PELSWIDTH | DM_PELSHEIGHT | DM_DISPLAYFREQUENCY;
        peerspan_mode.Anonymous1.Anonymous2.dmPosition.x = x;
        peerspan_mode.Anonymous1.Anonymous2.dmPosition.y = y;
        let result = unsafe {
            ChangeDisplaySettingsExW(
                peerspan.as_ptr(),
                &peerspan_mode,
                ptr::null_mut(),
                CDS_UPDATEREGISTRY,
                ptr::null(),
            )
        };
        if result == DISP_CHANGE_SUCCESSFUL {
            Ok(())
        } else {
            Err(format!(
                "Windows rejected the VDD display position with status {result}"
            ))
        }
    }

    fn current_mode(device_name: &[u16; 32]) -> Result<DEVMODEW, String> {
        let mut mode = DEVMODEW {
            dmSize: size_of::<DEVMODEW>() as u16,
            ..Default::default()
        };
        if unsafe {
            EnumDisplaySettingsExW(device_name.as_ptr(), ENUM_CURRENT_SETTINGS, &mut mode, 0)
        } == 0
        {
            Err("Windows did not return the current display mode".into())
        } else {
            Ok(mode)
        }
    }

    fn contains_vdd_identity(value: &[u16]) -> bool {
        let end = value
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(value.len());
        let value = String::from_utf16_lossy(&value[..end]).to_ascii_lowercase();
        value.contains("mttvdd") || value.contains("virtual display driver")
    }

    #[cfg(test)]
    pub(super) fn active_vdd_mode() -> Result<(u32, u32, u32), String> {
        for index in 0..32 {
            let mut adapter = DISPLAY_DEVICEW {
                cb: size_of::<DISPLAY_DEVICEW>() as u32,
                ..Default::default()
            };
            if unsafe { EnumDisplayDevicesW(ptr::null(), index, &mut adapter, 0) } == 0 {
                break;
            }
            if adapter.StateFlags & (DISPLAY_DEVICE_ACTIVE | DISPLAY_DEVICE_ATTACHED_TO_DESKTOP)
                == 0
                || (!contains_vdd_identity(&adapter.DeviceString)
                    && !contains_vdd_identity(&adapter.DeviceID))
            {
                continue;
            }
            let mode = current_mode(&adapter.DeviceName)?;
            return Ok((mode.dmPelsWidth, mode.dmPelsHeight, mode.dmDisplayFrequency));
        }
        Err("an active VDD adapter was not found".into())
    }

    fn wait_for_creation(context: &CallbackContext) -> Result<CreationResult, String> {
        let result = context
            .result
            .lock()
            .map_err(|_| "virtual display creation callback lock is poisoned")?;
        let (result, timeout) = context
            .ready
            .wait_timeout_while(result, CREATE_TIMEOUT, |result| result.is_none())
            .map_err(|_| "virtual display creation callback lock is poisoned")?;
        if timeout.timed_out() && result.is_none() {
            return Err("Windows timed out while creating the virtual display device".into());
        }
        result
            .clone()
            .ok_or_else(|| "Windows did not complete virtual display creation".into())
    }

    fn wait_until_started(instance_id: &str) -> Result<(), String> {
        let instance_id_wide = wide(instance_id);
        let deadline = Instant::now() + START_TIMEOUT;
        let mut last_problem = None;
        loop {
            let mut device_instance = 0_u32;
            // SAFETY: device_instance points to valid storage and instance_id_wide is terminated.
            let locate_result = unsafe {
                CM_Locate_DevNodeW(
                    &mut device_instance,
                    instance_id_wide.as_ptr() as *mut u16,
                    CM_LOCATE_DEVNODE_NORMAL,
                )
            };
            if locate_result == CR_SUCCESS {
                let mut status = 0_u32;
                let mut problem = 0_u32;
                // SAFETY: The devnode was returned by Configuration Manager and outputs are valid.
                let status_result =
                    unsafe { CM_Get_DevNode_Status(&mut status, &mut problem, device_instance, 0) };
                if status_result == CR_SUCCESS {
                    if status & DN_STARTED != 0 && problem == 0 {
                        return Ok(());
                    }
                    last_problem = Some(problem);
                }
            }
            if Instant::now() >= deadline {
                return Err(match last_problem {
                    Some(problem) => format!(
                        "the virtual display device node did not start (Configuration Manager problem {problem})"
                    ),
                    None => {
                        "Windows could not locate the created virtual display device node".into()
                    }
                });
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    fn failed(hresult: HRESULT) -> bool {
        hresult < 0
    }

    fn format_hresult(hresult: HRESULT) -> String {
        format!("HRESULT 0x{:08X}", hresult as u32)
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain([0]).collect()
    }

    fn multi_sz(values: &[&str]) -> Vec<u16> {
        let mut result = Vec::new();
        for value in values {
            result.extend(value.encode_utf16());
            result.push(0);
        }
        result.push(0);
        result
    }

    unsafe fn wide_ptr_to_string(value: PCWSTR) -> Option<String> {
        if value.is_null() {
            return None;
        }
        let mut length = 0;
        // SAFETY: Windows owns this terminated callback string for the duration of the callback.
        while length < MAX_INSTANCE_ID_UNITS && unsafe { *value.add(length) } != 0 {
            length += 1;
        }
        if length == MAX_INSTANCE_ID_UNITS {
            return None;
        }
        // SAFETY: The preceding scan established a valid range before the terminator.
        let units = unsafe { std::slice::from_raw_parts(value, length) };
        Some(String::from_utf16_lossy(units))
    }
}

#[cfg(not(windows))]
mod platform {
    use super::{DisplayLease, VirtualDisplayBackend};
    use peerspan_core::ScreenEdge;

    pub(super) fn backend() -> Box<dyn VirtualDisplayBackend> {
        Box::new(UnsupportedBackend)
    }

    struct UnsupportedBackend;

    impl VirtualDisplayBackend for UnsupportedBackend {
        fn activate(&self) -> Result<Box<dyn DisplayLease>, String> {
            Err("PeerSpan virtual displays require Windows".into())
        }

        fn position(&self, _edge: ScreenEdge) -> Result<(), String> {
            Err("PeerSpan virtual displays require Windows".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use peerspan_core::{
        DeviceStatus, DisplaySession, LocalDevice, PeerDevice, SessionDirection, SessionState,
    };
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };
    use uuid::Uuid;

    struct FakeBackend {
        starts: Arc<AtomicUsize>,
        drops: Arc<AtomicUsize>,
        activation_error: Option<String>,
        position_error: Option<String>,
    }

    struct FakeLease {
        drops: Arc<AtomicUsize>,
    }

    impl DisplayLease for FakeLease {
        fn instance_id(&self) -> &str {
            "SWD\\MttVDD\\PeerSpanVirtualDisplay"
        }
    }

    impl Drop for FakeLease {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::Relaxed);
        }
    }

    impl VirtualDisplayBackend for FakeBackend {
        fn activate(&self) -> Result<Box<dyn DisplayLease>, String> {
            self.starts.fetch_add(1, Ordering::Relaxed);
            if let Some(error) = &self.activation_error {
                return Err(error.clone());
            }
            Ok(Box::new(FakeLease {
                drops: Arc::clone(&self.drops),
            }))
        }

        fn position(&self, _edge: ScreenEdge) -> Result<(), String> {
            self.position_error.clone().map_or(Ok(()), Err)
        }
    }

    fn local_device() -> LocalDevice {
        LocalDevice {
            id: Uuid::new_v4(),
            name: "test".into(),
            platform: "Windows".into(),
            fingerprint: "test".into(),
            public_key: "00".repeat(32),
        }
    }

    fn runtime(
        activation_error: Option<&str>,
        position_error: Option<&str>,
    ) -> (
        VirtualDisplayRuntime,
        Arc<AtomicUsize>,
        Arc<AtomicUsize>,
        PathBuf,
    ) {
        let directory = std::env::temp_dir().join(format!("peerspan-vdd-test-{}", Uuid::new_v4()));
        let core = Arc::new(PeerSpanCore::load(local_device(), &directory).unwrap());
        let starts = Arc::new(AtomicUsize::new(0));
        let drops = Arc::new(AtomicUsize::new(0));
        let runtime = VirtualDisplayRuntime::with_backend(
            core,
            Box::new(FakeBackend {
                starts: Arc::clone(&starts),
                drops: Arc::clone(&drops),
                activation_error: activation_error.map(str::to_owned),
                position_error: position_error.map(str::to_owned),
            }),
        );
        (runtime, starts, drops, directory)
    }

    #[test]
    fn start_is_idempotent_and_stop_releases_the_device() {
        let (runtime, starts, drops, directory) = runtime(None, None);
        runtime.start().unwrap();
        runtime.start().unwrap();
        assert_eq!(starts.load(Ordering::Relaxed), 1);
        assert_eq!(
            runtime
                .core
                .snapshot()
                .unwrap()
                .capabilities
                .virtual_display
                .state,
            peerspan_core::CapabilityState::Ready
        );

        runtime.stop().unwrap();
        runtime.stop().unwrap();
        assert_eq!(drops.load(Ordering::Relaxed), 1);
        assert_eq!(
            runtime
                .core
                .snapshot()
                .unwrap()
                .capabilities
                .virtual_display
                .state,
            peerspan_core::CapabilityState::RequiresSetup
        );
        drop(runtime);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn activation_failure_is_reported_as_required_setup() {
        let (runtime, starts, drops, directory) = runtime(Some("driver is missing"), None);
        assert_eq!(runtime.start(), Err("driver is missing".into()));
        assert_eq!(starts.load(Ordering::Relaxed), 1);
        assert_eq!(drops.load(Ordering::Relaxed), 0);
        let capability = runtime
            .core
            .snapshot()
            .unwrap()
            .capabilities
            .virtual_display;
        assert_eq!(
            capability.state,
            peerspan_core::CapabilityState::RequiresSetup
        );
        assert!(capability.detail.contains("driver is missing"));
        drop(runtime);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn layout_failure_retains_the_started_device_for_retry() {
        let (runtime, starts, drops, directory) = runtime(None, Some("RDP topology veto"));
        assert_eq!(runtime.start(), Err("RDP topology veto".into()));
        assert_eq!(starts.load(Ordering::Relaxed), 1);
        assert_eq!(drops.load(Ordering::Relaxed), 0);
        assert!(
            runtime
                .core
                .snapshot()
                .unwrap()
                .capabilities
                .virtual_display
                .detail
                .contains("started and retained")
        );
        assert_eq!(runtime.start(), Err("RDP topology veto".into()));
        assert_eq!(starts.load(Ordering::Relaxed), 1);
        runtime.stop().unwrap();
        assert_eq!(drops.load(Ordering::Relaxed), 1);
        drop(runtime);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn active_session_prevents_virtual_display_removal() {
        let (runtime, _, drops, directory) = runtime(None, None);
        runtime.start().unwrap();
        let peer_id = Uuid::new_v4();
        runtime
            .core
            .trust_device(PeerDevice {
                id: peer_id,
                name: "peer".into(),
                platform: "Windows".into(),
                fingerprint: "peer".into(),
                public_key: "11".repeat(32),
                status: DeviceStatus::Online,
                trusted: true,
                latency_ms: None,
                last_seen_unix_ms: 0,
                addresses: vec![],
                control_port: 37_622,
                pairing_port: 37_621,
                protocol_version: 5,
            })
            .unwrap();
        let session_id = Uuid::new_v4();
        runtime
            .core
            .start_display_session(DisplaySession {
                id: session_id,
                peer_id,
                direction: SessionDirection::Sending,
                state: SessionState::Negotiating,
                width_px: 1920,
                height_px: 1080,
                refresh_hz: 60,
                latency_ms: None,
            })
            .unwrap();

        assert!(runtime.stop().is_err());
        assert_eq!(drops.load(Ordering::Relaxed), 0);
        runtime.core.end_display_session(session_id).unwrap();
        runtime.stop().unwrap();
        assert_eq!(drops.load(Ordering::Relaxed), 1);
        drop(runtime);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "modifies the live Windows display topology and requires the signed VDD package"]
    fn signed_vdd_enters_the_windows_desktop_topology() {
        let backend = platform::backend();
        let lease = backend
            .activate()
            .expect("the signed VirtualDrivers VDD software device should start");
        assert!(
            lease
                .instance_id()
                .eq_ignore_ascii_case("SWD\\MttVDD\\PeerSpanVirtualDisplay")
        );
        backend
            .position(ScreenEdge::Right)
            .expect("the VDD monitor should enter the desktop topology and move right of primary");
        let mode = platform::active_vdd_mode().expect("the active VDD mode should be readable");
        assert_eq!(mode, (1920, 1080, 60));
        eprintln!(
            "verified {} at {}x{}@{} Hz",
            lease.instance_id(),
            mode.0,
            mode.1,
            mode.2
        );
        drop(lease);
    }
}
