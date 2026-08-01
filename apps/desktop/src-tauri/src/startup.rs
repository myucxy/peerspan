#[cfg(windows)]
pub fn set_launch_at_startup(enabled: bool) -> Result<(), String> {
    platform::set(enabled)
}

#[cfg(not(windows))]
pub fn set_launch_at_startup(enabled: bool) -> Result<(), String> {
    if enabled {
        Err("launch at startup is only supported on Windows".into())
    } else {
        Ok(())
    }
}

#[cfg(windows)]
mod platform {
    use std::{io, mem::size_of, path::Path};
    use windows_sys::Win32::{
        Foundation::ERROR_SUCCESS,
        System::Registry::{
            HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
            RegCreateKeyExW, RegDeleteValueW, RegSetValueExW,
        },
    };

    const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
    const VALUE_NAME: &str = "PeerSpan";

    pub fn set(enabled: bool) -> Result<(), String> {
        let key_name = wide(RUN_KEY);
        let value_name = wide(VALUE_NAME);
        let mut key: HKEY = std::ptr::null_mut();
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                key_name.as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                std::ptr::null(),
                &mut key,
                std::ptr::null_mut(),
            )
        };
        if status != ERROR_SUCCESS {
            return Err(registry_error("open the per-user startup key", status));
        }
        let key = RegistryKey(key);
        if enabled {
            let executable = std::env::current_exe()
                .map_err(|error| format!("could not resolve the PeerSpan executable: {error}"))?;
            let command = wide(&quoted_path(&executable));
            let byte_count = command
                .len()
                .checked_mul(size_of::<u16>())
                .and_then(|bytes| u32::try_from(bytes).ok())
                .ok_or_else(|| "startup command is too long".to_owned())?;
            let status = unsafe {
                RegSetValueExW(
                    key.0,
                    value_name.as_ptr(),
                    0,
                    REG_SZ,
                    command.as_ptr().cast(),
                    byte_count,
                )
            };
            if status != ERROR_SUCCESS {
                return Err(registry_error("write the PeerSpan startup value", status));
            }
        } else {
            let status = unsafe { RegDeleteValueW(key.0, value_name.as_ptr()) };
            if status != ERROR_SUCCESS && status != 2 {
                return Err(registry_error("remove the PeerSpan startup value", status));
            }
        }
        Ok(())
    }

    struct RegistryKey(HKEY);

    impl Drop for RegistryKey {
        fn drop(&mut self) {
            if !self.0.is_null() {
                let _ = unsafe { RegCloseKey(self.0) };
            }
        }
    }

    fn quoted_path(path: &Path) -> String {
        format!("\"{}\"", path.display())
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(Some(0)).collect()
    }

    fn registry_error(action: &str, status: u32) -> String {
        format!(
            "could not {action}: {}",
            io::Error::from_raw_os_error(status as i32)
        )
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn startup_paths_are_quoted() {
            assert_eq!(
                quoted_path(Path::new("C:\\Program Files\\PeerSpan\\peerspan.exe")),
                "\"C:\\Program Files\\PeerSpan\\peerspan.exe\""
            );
        }
    }
}
