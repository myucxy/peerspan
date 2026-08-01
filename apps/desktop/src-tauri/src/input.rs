use peerspan_core::{Capability, PeerSpanCore};
use peerspan_protocol::{InputEvent, PointerButton};
use std::collections::HashSet;

pub fn probe_input_capability(core: &PeerSpanCore) {
    let capability = if cfg!(windows) {
        Capability::ready(
            "Windows SendInput adapter is available and accepts events only from an authenticated active session",
        )
    } else {
        Capability::required("Windows SendInput is unavailable on this platform")
    };
    let _ = core.set_input_injection_capability(capability);
}

pub struct InputInjector {
    #[cfg(windows)]
    target: platform::DisplayRect,
    pressed_keys: HashSet<(u16, bool)>,
    pressed_buttons: HashSet<PointerButton>,
}

impl InputInjector {
    pub fn open() -> Result<Self, String> {
        #[cfg(windows)]
        let target = platform::find_peerspan_display()?;
        #[cfg(not(windows))]
        return Err("Windows SendInput is unavailable on this platform".into());

        #[allow(unreachable_code)]
        Ok(Self {
            #[cfg(windows)]
            target,
            pressed_keys: HashSet::new(),
            pressed_buttons: HashSet::new(),
        })
    }

    pub fn inject(&mut self, event: InputEvent) -> Result<(), String> {
        match event {
            InputEvent::PointerMove {
                normalized_x,
                normalized_y,
            } => {
                if !normalized_x.is_finite()
                    || !normalized_y.is_finite()
                    || !(0.0..=1.0).contains(&normalized_x)
                    || !(0.0..=1.0).contains(&normalized_y)
                {
                    return Err(
                        "remote pointer coordinates are outside the normalized display".into(),
                    );
                }
                #[cfg(windows)]
                platform::pointer_move(self.target, normalized_x, normalized_y)?;
            }
            InputEvent::PointerButton { button, pressed } => {
                #[cfg(windows)]
                platform::pointer_button(button, pressed)?;
                if pressed {
                    self.pressed_buttons.insert(button);
                } else {
                    self.pressed_buttons.remove(&button);
                }
            }
            InputEvent::Wheel { delta_x, delta_y } => {
                #[cfg(windows)]
                platform::wheel(delta_x, delta_y)?;
            }
            InputEvent::Key {
                scan_code,
                pressed,
                extended,
            } => {
                if scan_code == 0 || scan_code > 0xff {
                    return Err("remote keyboard scan code is invalid".into());
                }
                #[cfg(windows)]
                platform::key(scan_code, pressed, extended)?;
                if pressed {
                    self.pressed_keys.insert((scan_code, extended));
                } else {
                    self.pressed_keys.remove(&(scan_code, extended));
                }
            }
            InputEvent::ReleaseAll => self.release_all(),
        }
        Ok(())
    }

    pub fn release_all(&mut self) {
        for (scan_code, extended) in self.pressed_keys.drain() {
            #[cfg(windows)]
            let _ = platform::key(scan_code, false, extended);
            #[cfg(not(windows))]
            let _ = (scan_code, extended);
        }
        for button in self.pressed_buttons.drain() {
            #[cfg(windows)]
            let _ = platform::pointer_button(button, false);
            #[cfg(not(windows))]
            let _ = button;
        }
    }

    pub fn recover_windows(&self) -> Result<usize, String> {
        #[cfg(windows)]
        {
            platform::recover_windows(self.target)
        }
        #[cfg(not(windows))]
        {
            Ok(0)
        }
    }
}

impl Drop for InputInjector {
    fn drop(&mut self) {
        self.release_all();
    }
}

#[cfg(windows)]
mod platform {
    use peerspan_protocol::PointerButton;
    use std::{io, mem::size_of, ptr};
    use windows_sys::Win32::{
        Foundation::{LPARAM, POINT, RECT},
        Graphics::Gdi::{
            DEVMODEW, DISPLAY_DEVICE_ACTIVE, DISPLAY_DEVICE_ATTACHED_TO_DESKTOP, DISPLAY_DEVICEW,
            ENUM_CURRENT_SETTINGS, EnumDisplayDevicesW, EnumDisplaySettingsExW, GetMonitorInfoW,
            MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint,
        },
        UI::{
            Input::KeyboardAndMouse::{
                INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
                KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_HWHEEL,
                MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN,
                MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
                MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL, MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP,
                MOUSEINPUT, SendInput,
            },
            WindowsAndMessaging::{
                EnumWindows, GetSystemMetrics, GetWindowPlacement, IsWindowVisible,
                SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
                SetWindowPlacement, WINDOWPLACEMENT,
            },
        },
    };

    #[derive(Clone, Copy)]
    pub struct DisplayRect {
        pub(super) x: i32,
        pub(super) y: i32,
        pub(super) width: u32,
        pub(super) height: u32,
    }

    pub fn find_peerspan_display() -> Result<DisplayRect, String> {
        for adapter_index in 0..32 {
            let mut adapter = DISPLAY_DEVICEW {
                cb: size_of::<DISPLAY_DEVICEW>() as u32,
                ..Default::default()
            };
            if unsafe { EnumDisplayDevicesW(ptr::null(), adapter_index, &mut adapter, 0) } == 0 {
                break;
            }
            if adapter.StateFlags & (DISPLAY_DEVICE_ACTIVE | DISPLAY_DEVICE_ATTACHED_TO_DESKTOP)
                == 0
            {
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
                matches |= contains_peerspan(&monitor.DeviceString)
                    || contains_peerspan(&monitor.DeviceID);
            }
            if !matches {
                continue;
            }
            let mut mode = DEVMODEW {
                dmSize: size_of::<DEVMODEW>() as u16,
                ..Default::default()
            };
            if unsafe {
                EnumDisplaySettingsExW(
                    adapter.DeviceName.as_ptr(),
                    ENUM_CURRENT_SETTINGS,
                    &mut mode,
                    0,
                )
            } == 0
            {
                continue;
            }
            let position = unsafe { mode.Anonymous1.Anonymous2.dmPosition };
            if mode.dmPelsWidth != 0 && mode.dmPelsHeight != 0 {
                return Ok(DisplayRect {
                    x: position.x,
                    y: position.y,
                    width: mode.dmPelsWidth,
                    height: mode.dmPelsHeight,
                });
            }
        }
        Err(
            "PeerSpan virtual display is active but its desktop bounds could not be resolved"
                .into(),
        )
    }

    pub fn pointer_move(
        target: DisplayRect,
        normalized_x: f32,
        normalized_y: f32,
    ) -> Result<(), String> {
        let virtual_x = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
        let virtual_y = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
        let virtual_width = unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) };
        let virtual_height = unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) };
        if virtual_width <= 1 || virtual_height <= 1 {
            return Err("Windows returned invalid virtual desktop bounds".into());
        }
        let pixel_x =
            target.x + (normalized_x * target.width.saturating_sub(1) as f32).round() as i32;
        let pixel_y =
            target.y + (normalized_y * target.height.saturating_sub(1) as f32).round() as i32;
        let absolute_x = ((i64::from(pixel_x - virtual_x) * 65_535) / i64::from(virtual_width - 1))
            .clamp(0, 65_535) as i32;
        let absolute_y = ((i64::from(pixel_y - virtual_y) * 65_535) / i64::from(virtual_height - 1))
            .clamp(0, 65_535) as i32;
        send_mouse(
            absolute_x,
            absolute_y,
            0,
            MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
        )
    }

    pub fn pointer_button(button: PointerButton, pressed: bool) -> Result<(), String> {
        let (flags, data) = match (button, pressed) {
            (PointerButton::Left, true) => (MOUSEEVENTF_LEFTDOWN, 0),
            (PointerButton::Left, false) => (MOUSEEVENTF_LEFTUP, 0),
            (PointerButton::Right, true) => (MOUSEEVENTF_RIGHTDOWN, 0),
            (PointerButton::Right, false) => (MOUSEEVENTF_RIGHTUP, 0),
            (PointerButton::Middle, true) => (MOUSEEVENTF_MIDDLEDOWN, 0),
            (PointerButton::Middle, false) => (MOUSEEVENTF_MIDDLEUP, 0),
            (PointerButton::Back, true) => (MOUSEEVENTF_XDOWN, 1),
            (PointerButton::Back, false) => (MOUSEEVENTF_XUP, 1),
            (PointerButton::Forward, true) => (MOUSEEVENTF_XDOWN, 2),
            (PointerButton::Forward, false) => (MOUSEEVENTF_XUP, 2),
        };
        send_mouse(0, 0, data, flags)
    }

    pub fn wheel(delta_x: i16, delta_y: i16) -> Result<(), String> {
        if delta_x != 0 {
            send_mouse(0, 0, i32::from(delta_x) as u32, MOUSEEVENTF_HWHEEL)?;
        }
        if delta_y != 0 {
            send_mouse(0, 0, i32::from(delta_y) as u32, MOUSEEVENTF_WHEEL)?;
        }
        Ok(())
    }

    pub fn key(scan_code: u16, pressed: bool, extended: bool) -> Result<(), String> {
        let mut flags = KEYEVENTF_SCANCODE;
        if extended {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        if !pressed {
            flags |= KEYEVENTF_KEYUP;
        }
        let input = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: 0,
                    wScan: scan_code,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        send(&input)
    }

    pub fn recover_windows(target: DisplayRect) -> Result<usize, String> {
        let monitor = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
        if monitor.is_null() {
            return Err("Windows did not return the primary monitor".into());
        }
        let mut primary = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if unsafe { GetMonitorInfoW(monitor, &mut primary) } == 0 {
            return Err(format!(
                "could not read primary monitor bounds: {}",
                io::Error::last_os_error()
            ));
        }
        let mut context = RecoveryContext {
            target,
            primary_work: primary.rcWork,
            moved: 0,
        };
        if unsafe {
            EnumWindows(
                Some(recover_window),
                &mut context as *mut RecoveryContext as LPARAM,
            )
        } == 0
        {
            return Err(format!(
                "could not enumerate desktop windows for recovery: {}",
                io::Error::last_os_error()
            ));
        }
        Ok(context.moved)
    }

    struct RecoveryContext {
        target: DisplayRect,
        primary_work: RECT,
        moved: usize,
    }

    unsafe extern "system" fn recover_window(
        window: windows_sys::Win32::Foundation::HWND,
        context: LPARAM,
    ) -> i32 {
        if unsafe { IsWindowVisible(window) } == 0 || context == 0 {
            return 1;
        }
        let context = unsafe { &mut *(context as *mut RecoveryContext) };
        let mut placement = WINDOWPLACEMENT {
            length: size_of::<WINDOWPLACEMENT>() as u32,
            ..Default::default()
        };
        if unsafe { GetWindowPlacement(window, &mut placement) } == 0 {
            return 1;
        }
        let rect = placement.rcNormalPosition;
        let center_x = i64::from(rect.left) + i64::from(rect.right - rect.left) / 2;
        let center_y = i64::from(rect.top) + i64::from(rect.bottom - rect.top) / 2;
        let target_right = i64::from(context.target.x) + i64::from(context.target.width);
        let target_bottom = i64::from(context.target.y) + i64::from(context.target.height);
        if center_x < i64::from(context.target.x)
            || center_x >= target_right
            || center_y < i64::from(context.target.y)
            || center_y >= target_bottom
        {
            return 1;
        }
        let width = (rect.right - rect.left).max(1);
        let height = (rect.bottom - rect.top).max(1);
        let work = context.primary_work;
        let max_x = (work.right - width).max(work.left);
        let max_y = (work.bottom - height).max(work.top);
        let left = (work.left + rect.left - context.target.x).clamp(work.left, max_x);
        let top = (work.top + rect.top - context.target.y).clamp(work.top, max_y);
        placement.rcNormalPosition = RECT {
            left,
            top,
            right: left + width,
            bottom: top + height,
        };
        if unsafe { SetWindowPlacement(window, &placement) } != 0 {
            context.moved += 1;
        }
        1
    }

    fn send_mouse(dx: i32, dy: i32, data: u32, flags: u32) -> Result<(), String> {
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx,
                    dy,
                    mouseData: data,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        send(&input)
    }

    fn send(input: &INPUT) -> Result<(), String> {
        let sent = unsafe { SendInput(1, input, size_of::<INPUT>() as i32) };
        if sent == 1 {
            Ok(())
        } else {
            Err(format!(
                "Windows SendInput failed: {}",
                io::Error::last_os_error()
            ))
        }
    }

    fn contains_peerspan(value: &[u16]) -> bool {
        let end = value
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(value.len());
        String::from_utf16_lossy(&value[..end])
            .to_ascii_lowercase()
            .contains("peerspan")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_remote_pointer_coordinates_are_rejected_before_platform_use() {
        let mut injector = InputInjector {
            #[cfg(windows)]
            target: platform::DisplayRect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
            pressed_keys: HashSet::new(),
            pressed_buttons: HashSet::new(),
        };
        assert!(
            injector
                .inject(InputEvent::PointerMove {
                    normalized_x: f32::NAN,
                    normalized_y: 0.5,
                })
                .is_err()
        );
    }
}
