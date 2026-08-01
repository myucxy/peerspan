use peerspan_protocol::ClipboardText;
use std::time::{Duration, Instant};

pub const MAX_CLIPBOARD_TEXT_BYTES: usize = 1024 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(250);

pub struct ClipboardSync {
    enabled: bool,
    next_poll: Instant,
    local_revision: u64,
    remote_revision: Option<u64>,
    last_observed: Option<String>,
    suppress_text: Option<String>,
}

impl ClipboardSync {
    pub fn new(enabled: bool) -> Self {
        let initial_text = enabled
            .then(|| platform::read_text().ok().flatten())
            .flatten()
            .filter(|text| text.len() <= MAX_CLIPBOARD_TEXT_BYTES);
        Self {
            enabled,
            next_poll: Instant::now(),
            local_revision: 0,
            remote_revision: None,
            last_observed: initial_text,
            suppress_text: None,
        }
    }

    pub fn poll_local(&mut self, now: Instant) -> Option<ClipboardText> {
        if !self.enabled || now < self.next_poll {
            return None;
        }
        self.next_poll = now + POLL_INTERVAL;
        let text = platform::read_text().ok().flatten()?;
        if text.len() > MAX_CLIPBOARD_TEXT_BYTES {
            return None;
        }
        if self.suppress_text.as_deref() == Some(text.as_str()) {
            self.suppress_text = None;
            self.last_observed = Some(text);
            return None;
        }
        if self.last_observed.as_deref() == Some(text.as_str()) {
            return None;
        }
        self.last_observed = Some(text.clone());
        self.local_revision = self.local_revision.wrapping_add(1);
        if self.local_revision == 0 {
            self.local_revision = 1;
        }
        Some(ClipboardText {
            revision: self.local_revision,
            text,
        })
    }

    pub fn apply_remote(&mut self, update: ClipboardText) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        if update.text.len() > MAX_CLIPBOARD_TEXT_BYTES {
            return Err(format!(
                "remote clipboard text exceeds the {} byte limit",
                MAX_CLIPBOARD_TEXT_BYTES
            ));
        }
        if self
            .remote_revision
            .is_some_and(|revision| update.revision <= revision)
        {
            return Ok(());
        }
        self.remote_revision = Some(update.revision);
        if self.last_observed.as_deref() == Some(update.text.as_str()) {
            return Ok(());
        }
        platform::write_text(&update.text)?;
        self.suppress_text = Some(update.text.clone());
        self.last_observed = Some(update.text);
        Ok(())
    }
}

#[cfg(windows)]
mod platform {
    use std::{io, ptr, slice};
    use windows_sys::Win32::{
        Foundation::{GlobalFree, HANDLE, HGLOBAL},
        System::{
            DataExchange::{
                CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
                OpenClipboard, SetClipboardData,
            },
            Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock},
            Ole::CF_UNICODETEXT,
        },
    };

    struct ClipboardGuard;

    impl ClipboardGuard {
        fn open() -> Result<Self, String> {
            if unsafe { OpenClipboard(ptr::null_mut()) } == 0 {
                Err(format!(
                    "could not open the Windows clipboard: {}",
                    io::Error::last_os_error()
                ))
            } else {
                Ok(Self)
            }
        }
    }

    impl Drop for ClipboardGuard {
        fn drop(&mut self) {
            let _ = unsafe { CloseClipboard() };
        }
    }

    struct GlobalMemory(HGLOBAL);

    impl Drop for GlobalMemory {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { GlobalFree(self.0) };
            }
        }
    }

    pub fn read_text() -> Result<Option<String>, String> {
        if unsafe { IsClipboardFormatAvailable(u32::from(CF_UNICODETEXT)) } == 0 {
            return Ok(None);
        }
        let _clipboard = ClipboardGuard::open()?;
        let handle = unsafe { GetClipboardData(u32::from(CF_UNICODETEXT)) };
        if handle.is_null() {
            return Ok(None);
        }
        let size = unsafe { GlobalSize(handle as HGLOBAL) };
        if !(2..=(super::MAX_CLIPBOARD_TEXT_BYTES + 1) * 2).contains(&size) {
            return Ok(None);
        }
        let pointer = unsafe { GlobalLock(handle as HGLOBAL) } as *const u16;
        if pointer.is_null() {
            return Err(format!(
                "could not lock clipboard text: {}",
                io::Error::last_os_error()
            ));
        }
        let units = unsafe { slice::from_raw_parts(pointer, size / 2) };
        let end = units
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(units.len());
        let text = String::from_utf16(&units[..end])
            .map_err(|_| "Windows clipboard text is not valid UTF-16".to_owned());
        let _ = unsafe { GlobalUnlock(handle as HGLOBAL) };
        text.map(Some)
    }

    pub fn write_text(text: &str) -> Result<(), String> {
        let units: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
        let bytes = units
            .len()
            .checked_mul(2)
            .ok_or_else(|| "clipboard text allocation overflow".to_owned())?;
        let memory = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes) };
        if memory.is_null() {
            return Err(format!(
                "could not allocate clipboard text: {}",
                io::Error::last_os_error()
            ));
        }
        let mut memory = GlobalMemory(memory);
        let destination = unsafe { GlobalLock(memory.0) } as *mut u16;
        if destination.is_null() {
            return Err(format!(
                "could not lock clipboard allocation: {}",
                io::Error::last_os_error()
            ));
        }
        unsafe { destination.copy_from_nonoverlapping(units.as_ptr(), units.len()) };
        let _ = unsafe { GlobalUnlock(memory.0) };

        let _clipboard = ClipboardGuard::open()?;
        if unsafe { EmptyClipboard() } == 0 {
            return Err(format!(
                "could not clear the Windows clipboard: {}",
                io::Error::last_os_error()
            ));
        }
        let result = unsafe { SetClipboardData(u32::from(CF_UNICODETEXT), memory.0 as HANDLE) };
        if result.is_null() {
            return Err(format!(
                "could not set Windows clipboard text: {}",
                io::Error::last_os_error()
            ));
        }
        memory.0 = ptr::null_mut();
        Ok(())
    }
}

#[cfg(not(windows))]
mod platform {
    pub fn read_text() -> Result<Option<String>, String> {
        Ok(None)
    }

    pub fn write_text(_text: &str) -> Result<(), String> {
        Err("Windows clipboard synchronization is unavailable on this platform".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_remote_revisions_are_ignored_before_platform_access() {
        let mut sync = ClipboardSync::new(false);
        sync.apply_remote(ClipboardText {
            revision: 4,
            text: "first".into(),
        })
        .unwrap();
        sync.apply_remote(ClipboardText {
            revision: 4,
            text: "duplicate".into(),
        })
        .unwrap();
    }

    #[test]
    fn oversized_remote_text_is_rejected() {
        let mut sync = ClipboardSync::new(true);
        assert!(
            sync.apply_remote(ClipboardText {
                revision: 1,
                text: "x".repeat(MAX_CLIPBOARD_TEXT_BYTES + 1),
            })
            .is_err()
        );
    }
}
