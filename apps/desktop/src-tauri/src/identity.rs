use ed25519_dalek::SigningKey;
use rand_core::OsRng;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{self, Write},
    os::windows::ffi::OsStrExt,
    path::Path,
};
use thiserror::Error;
use uuid::Uuid;
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    },
    Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW},
};
use zeroize::Zeroize;

const IDENTITY_FILE: &str = "identity.json";
const DPAPI_ENTROPY: &[u8] = b"PeerSpan device identity v1";

pub struct DeviceIdentity {
    pub device_id: Uuid,
    pub signing_key: SigningKey,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IdentityDocument {
    device_id: Uuid,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    protected_signing_key_hex: String,
    // Read the original plaintext field only long enough to migrate it. It is
    // deliberately impossible to serialize this field again.
    #[serde(default, skip_serializing)]
    signing_key_hex: String,
}

pub fn load_or_create_identity(data_dir: &Path) -> Result<DeviceIdentity, IdentityError> {
    let path = data_dir.join(IDENTITY_FILE);
    match fs::read(&path) {
        Ok(mut bytes) => {
            let identity = load_identity_document(&path, &bytes);
            bytes.zeroize();
            identity
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let identity = DeviceIdentity {
                device_id: Uuid::new_v4(),
                signing_key: SigningKey::generate(&mut OsRng),
            };
            persist_identity(&path, &identity)?;
            Ok(identity)
        }
        Err(error) => Err(error.into()),
    }
}

fn load_identity_document(path: &Path, bytes: &[u8]) -> Result<DeviceIdentity, IdentityError> {
    let mut document: IdentityDocument = serde_json::from_slice(bytes)?;
    let has_legacy_key = !document.signing_key_hex.is_empty();

    let signing_key = if !document.protected_signing_key_hex.is_empty() {
        decode_protected_signing_key(&document.protected_signing_key_hex)
    } else if has_legacy_key {
        decode_plaintext_signing_key(&document.signing_key_hex)
    } else {
        return Err(IdentityError::MissingSigningKey);
    };
    document.signing_key_hex.zeroize();
    let signing_key = signing_key?;

    let identity = DeviceIdentity {
        device_id: document.device_id,
        signing_key,
    };
    if has_legacy_key || document.protected_signing_key_hex.is_empty() {
        persist_identity(path, &identity)?;
    }
    Ok(identity)
}

fn persist_identity(path: &Path, identity: &DeviceIdentity) -> Result<(), IdentityError> {
    let protected_signing_key_hex = protect_signing_key(&identity.signing_key)?;
    let document = IdentityDocument {
        device_id: identity.device_id,
        protected_signing_key_hex,
        signing_key_hex: String::new(),
    };
    let bytes = serde_json::to_vec_pretty(&document)?;
    atomic_write(path, &bytes)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), IdentityError> {
    let temp_path = path.with_extension(format!("{}.tmp", Uuid::new_v4()));
    let result = (|| -> Result<(), IdentityError> {
        let mut file = File::create(&temp_path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);

        let source = wide_path(&temp_path);
        let destination = wide_path(path);
        // SAFETY: Both paths are NUL-terminated UTF-16 buffers that remain live
        // for the call. The temporary file is on the same volume as the target.
        let succeeded = unsafe {
            MoveFileExW(
                source.as_ptr(),
                destination.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if succeeded == 0 {
            return Err(io::Error::last_os_error().into());
        }
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

fn protect_signing_key(signing_key: &SigningKey) -> Result<String, IdentityError> {
    let mut plaintext = signing_key.to_bytes();
    let protected = protect_data(&mut plaintext);
    plaintext.zeroize();
    Ok(hex::encode(protected?))
}

fn decode_protected_signing_key(value: &str) -> Result<SigningKey, IdentityError> {
    let mut protected = hex::decode(value).map_err(|_| IdentityError::InvalidProtectedKey)?;
    let plaintext = unprotect_data(&mut protected);
    protected.zeroize();
    let mut plaintext = plaintext?;
    decode_signing_key_bytes(&mut plaintext)
}

fn decode_plaintext_signing_key(value: &str) -> Result<SigningKey, IdentityError> {
    let mut plaintext = hex::decode(value).map_err(|_| IdentityError::InvalidLegacyKey)?;
    decode_signing_key_bytes(&mut plaintext)
}

fn decode_signing_key_bytes(bytes: &mut Vec<u8>) -> Result<SigningKey, IdentityError> {
    if bytes.len() != 32 {
        bytes.zeroize();
        return Err(IdentityError::WrongKeyLength);
    }
    let mut key_bytes = [0_u8; 32];
    key_bytes.copy_from_slice(bytes);
    bytes.zeroize();
    let signing_key = SigningKey::from_bytes(&key_bytes);
    key_bytes.zeroize();
    Ok(signing_key)
}

fn protect_data(plaintext: &mut [u8]) -> Result<Vec<u8>, IdentityError> {
    let mut entropy = DPAPI_ENTROPY.to_vec();
    let input = blob(plaintext)?;
    let entropy_blob = blob(&mut entropy)?;
    let mut output = empty_blob();
    // SAFETY: The input and entropy blobs borrow live mutable slices for the
    // duration of the call. DPAPI initializes output with LocalAlloc memory,
    // which is copied and released below.
    let succeeded = unsafe {
        CryptProtectData(
            &input,
            std::ptr::null(),
            &entropy_blob,
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    entropy.zeroize();
    if succeeded == 0 {
        return Err(io::Error::last_os_error().into());
    }
    take_local_blob(output)
}

fn unprotect_data(protected: &mut [u8]) -> Result<Vec<u8>, IdentityError> {
    let mut entropy = DPAPI_ENTROPY.to_vec();
    let input = blob(protected)?;
    let entropy_blob = blob(&mut entropy)?;
    let mut output = empty_blob();
    // SAFETY: The blob pointers remain valid for the call. UI is forbidden and
    // no description pointer is requested. Output is released with LocalFree.
    let succeeded = unsafe {
        CryptUnprotectData(
            &input,
            std::ptr::null_mut(),
            &entropy_blob,
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    entropy.zeroize();
    if succeeded == 0 {
        return Err(io::Error::last_os_error().into());
    }
    take_local_blob(output)
}

fn blob(bytes: &mut [u8]) -> Result<CRYPT_INTEGER_BLOB, IdentityError> {
    Ok(CRYPT_INTEGER_BLOB {
        cbData: bytes
            .len()
            .try_into()
            .map_err(|_| IdentityError::DataTooLarge)?,
        pbData: bytes.as_mut_ptr(),
    })
}

fn empty_blob() -> CRYPT_INTEGER_BLOB {
    CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    }
}

fn take_local_blob(output: CRYPT_INTEGER_BLOB) -> Result<Vec<u8>, IdentityError> {
    if output.pbData.is_null() || output.cbData == 0 {
        if !output.pbData.is_null() {
            // SAFETY: DPAPI allocated this pointer with LocalAlloc.
            let _ = unsafe { LocalFree(output.pbData.cast()) };
        }
        return Err(IdentityError::EmptyDpapiOutput);
    }
    // SAFETY: DPAPI returned output.cbData initialized bytes at output.pbData.
    let local_bytes =
        unsafe { std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize) };
    let bytes = local_bytes.to_vec();
    local_bytes.zeroize();
    // SAFETY: DPAPI allocated this pointer with LocalAlloc and ownership is
    // transferred to the caller on a successful API call.
    let _ = unsafe { LocalFree(output.pbData.cast()) };
    Ok(bytes)
}

#[derive(Debug, Error)]
pub enum IdentityError {
    #[error("identity storage I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("identity document is invalid: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("identity document does not contain a signing key")]
    MissingSigningKey,
    #[error("protected identity key is not valid hexadecimal data")]
    InvalidProtectedKey,
    #[error("legacy identity key is not valid hexadecimal data")]
    InvalidLegacyKey,
    #[error("identity signing key has the wrong length")]
    WrongKeyLength,
    #[error("identity data is too large for Windows DPAPI")]
    DataTooLarge,
    #[error("Windows DPAPI returned an empty identity key")]
    EmptyDpapiOutput,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_directory(label: &str) -> std::path::PathBuf {
        let directory = std::env::temp_dir().join(format!("peerspan-{label}-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[test]
    fn dpapi_round_trip_is_bound_to_the_current_user() {
        let mut plaintext = [0x5a_u8; 32];
        let mut protected = protect_data(&mut plaintext).unwrap();
        assert_ne!(protected.as_slice(), plaintext.as_slice());
        let mut recovered = unprotect_data(&mut protected).unwrap();
        assert_eq!(recovered, plaintext);
        recovered.zeroize();
        protected.zeroize();
        plaintext.zeroize();
    }

    #[test]
    fn new_identity_never_serializes_the_plaintext_key() {
        let directory = temporary_directory("identity-new");
        let identity = load_or_create_identity(&directory).unwrap();
        let signing_key_hex = hex::encode(identity.signing_key.to_bytes());
        let bytes = fs::read(directory.join(IDENTITY_FILE)).unwrap();
        let document: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        assert!(document.get("protectedSigningKeyHex").is_some());
        assert!(document.get("signingKeyHex").is_none());
        assert!(!String::from_utf8(bytes).unwrap().contains(&signing_key_hex));

        let reloaded = load_or_create_identity(&directory).unwrap();
        assert_eq!(reloaded.device_id, identity.device_id);
        assert_eq!(
            reloaded.signing_key.to_bytes(),
            identity.signing_key.to_bytes()
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn legacy_plaintext_identity_is_migrated_without_rotation() {
        let directory = temporary_directory("identity-migrate");
        let path = directory.join(IDENTITY_FILE);
        let device_id = Uuid::new_v4();
        let legacy_key = [7_u8; 32];
        let legacy_key_hex = hex::encode(legacy_key);
        fs::write(
            &path,
            serde_json::json!({
                "deviceId": device_id,
                "signingKeyHex": legacy_key_hex,
            })
            .to_string(),
        )
        .unwrap();

        let identity = load_or_create_identity(&directory).unwrap();
        assert_eq!(identity.device_id, device_id);
        assert_eq!(identity.signing_key.to_bytes(), legacy_key);

        let migrated = fs::read_to_string(path).unwrap();
        let document: serde_json::Value = serde_json::from_str(&migrated).unwrap();
        assert!(document.get("protectedSigningKeyHex").is_some());
        assert!(document.get("signingKeyHex").is_none());
        assert!(!migrated.contains(&legacy_key_hex));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn corrupt_protected_key_fails_instead_of_rotating_identity() {
        let directory = temporary_directory("identity-corrupt");
        let path = directory.join(IDENTITY_FILE);
        let original = serde_json::json!({
            "deviceId": Uuid::new_v4(),
            "protectedSigningKeyHex": "00",
        })
        .to_string();
        fs::write(&path, &original).unwrap();

        assert!(load_or_create_identity(&directory).is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), original);
        fs::remove_dir_all(directory).unwrap();
    }
}
