use crate::{TransformCapability, VideoCapability, VideoError};
use std::{ptr, slice};
use windows::{
    Win32::{
        Foundation::{HMODULE, RPC_E_CHANGED_MODE},
        Graphics::{
            Direct3D::{
                D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_11_0,
                D3D_FEATURE_LEVEL_11_1,
            },
            Direct3D11::{
                D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_FLAG,
                D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice,
                ID3D11Device, ID3D11DeviceContext,
            },
        },
        Media::MediaFoundation::{
            IMFActivate, IMFTransform, MF_SA_D3D11_AWARE, MF_VERSION, MFCreateDXGIDeviceManager,
            MFMediaType_Video, MFSTARTUP_FULL, MFShutdown, MFStartup, MFT_CATEGORY_VIDEO_DECODER,
            MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_FLAG, MFT_ENUM_FLAG_ALL, MFT_ENUM_FLAG_HARDWARE,
            MFT_ENUM_FLAG_SORTANDFILTER, MFT_FRIENDLY_NAME_Attribute, MFT_REGISTER_TYPE_INFO,
            MFTEnumEx, MFVideoFormat_H264, MFVideoFormat_NV12,
        },
        System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize},
    },
    core::{Error as WindowsError, GUID, PWSTR},
};

struct RuntimeGuard {
    uninitialize_com: bool,
    shutdown_media_foundation: bool,
}

impl RuntimeGuard {
    fn start() -> Result<Self, VideoError> {
        let mut guard = Self {
            uninitialize_com: false,
            shutdown_media_foundation: false,
        };
        // SAFETY: COM is initialized once for this calling thread and balanced by Drop.
        let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if result.is_ok() {
            guard.uninitialize_com = true;
        } else if result == RPC_E_CHANGED_MODE {
            // Tauri may already own this thread as an STA. Media Foundation is
            // still usable; only the matching CoUninitialize must be skipped.
        } else {
            return Err(VideoError::Com(
                WindowsError::from_hresult(result).to_string(),
            ));
        }
        // SAFETY: MFStartup is paired with MFShutdown in Drop.
        unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) }
            .map_err(|error| VideoError::MediaFoundation(error.to_string()))?;
        guard.shutdown_media_foundation = true;
        Ok(guard)
    }
}

impl Drop for RuntimeGuard {
    fn drop(&mut self) {
        if self.shutdown_media_foundation {
            // SAFETY: this guard successfully called MFStartup on this thread.
            let _ = unsafe { MFShutdown() };
        }
        if self.uninitialize_com {
            // SAFETY: this guard successfully initialized COM on this thread.
            unsafe { CoUninitialize() };
        }
    }
}

struct ActivationArray {
    pointer: *mut Option<IMFActivate>,
    count: usize,
}

impl ActivationArray {
    fn as_slice(&self) -> &[Option<IMFActivate>] {
        if self.pointer.is_null() || self.count == 0 {
            return &[];
        }
        // SAFETY: MFTEnumEx returns `count` contiguous COM interface pointers.
        unsafe { slice::from_raw_parts(self.pointer, self.count) }
    }
}

impl Drop for ActivationArray {
    fn drop(&mut self) {
        if !self.pointer.is_null() {
            // SAFETY: every slot is an owning COM interface pointer returned by
            // MFTEnumEx. Release each one before freeing the enclosing array.
            unsafe {
                for index in 0..self.count {
                    ptr::drop_in_place(self.pointer.add(index));
                }
                CoTaskMemFree(Some(self.pointer.cast()));
            }
        }
    }
}

pub fn probe_hardware_h264() -> Result<VideoCapability, VideoError> {
    let _runtime = RuntimeGuard::start()?;
    let feature_level = create_video_device()?;
    let encoder = enumerate_transform(
        MFT_CATEGORY_VIDEO_ENCODER,
        MFVideoFormat_NV12,
        Some(MFVideoFormat_H264),
        true,
        "encoder",
    )?;
    let decoder = enumerate_transform(
        MFT_CATEGORY_VIDEO_DECODER,
        MFVideoFormat_H264,
        None,
        false,
        "decoder",
    )?;
    Ok(VideoCapability {
        d3d11_feature_level: feature_level_name(feature_level).into(),
        encoder,
        decoder,
    })
}

fn create_video_device() -> Result<D3D_FEATURE_LEVEL, VideoError> {
    let feature_levels = [D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0];
    let flags = D3D11_CREATE_DEVICE_FLAG(
        D3D11_CREATE_DEVICE_BGRA_SUPPORT.0 | D3D11_CREATE_DEVICE_VIDEO_SUPPORT.0,
    );
    let mut device: Option<ID3D11Device> = None;
    let mut context: Option<ID3D11DeviceContext> = None;
    let mut selected = D3D_FEATURE_LEVEL_11_0;
    // SAFETY: all output pointers are valid for the duration of the call.
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            flags,
            Some(&feature_levels),
            D3D11_SDK_VERSION,
            Some(&mut device),
            Some(&mut selected),
            Some(&mut context),
        )
    }
    .map_err(|error| VideoError::D3d11(error.to_string()))?;
    let device = device.ok_or_else(|| VideoError::D3d11("device creation returned null".into()))?;

    let mut reset_token = 0;
    let mut manager = None;
    // SAFETY: the manager and token output pointers are valid.
    unsafe { MFCreateDXGIDeviceManager(&mut reset_token, &mut manager) }
        .map_err(|error| VideoError::D3d11(error.to_string()))?;
    let manager = manager.ok_or_else(|| {
        VideoError::D3d11("Media Foundation returned a null DXGI device manager".into())
    })?;
    // SAFETY: `device` is a live D3D11 device and reset_token belongs to this manager.
    unsafe { manager.ResetDevice(&device, reset_token) }
        .map_err(|error| VideoError::D3d11(error.to_string()))?;
    drop(context);
    Ok(selected)
}

fn enumerate_transform(
    category: GUID,
    input_subtype: GUID,
    output_subtype: Option<GUID>,
    hardware_only: bool,
    kind: &'static str,
) -> Result<TransformCapability, VideoError> {
    let input = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: input_subtype,
    };
    let output = output_subtype.map(|subtype| MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: subtype,
    });
    let base_flags = if hardware_only {
        MFT_ENUM_FLAG_HARDWARE.0
    } else {
        MFT_ENUM_FLAG_ALL.0
    };
    let flags = MFT_ENUM_FLAG(base_flags | MFT_ENUM_FLAG_SORTANDFILTER.0);
    let mut pointer: *mut Option<IMFActivate> = ptr::null_mut();
    let mut count = 0_u32;
    // SAFETY: output pointers are valid and MFTEnumEx owns the returned array allocation.
    unsafe {
        MFTEnumEx(
            category,
            flags,
            Some(&input),
            output.as_ref().map(ptr::from_ref),
            &mut pointer,
            &mut count,
        )
    }
    .map_err(|error| VideoError::TransformEnumeration {
        kind,
        detail: error.to_string(),
    })?;
    let activations = ActivationArray {
        pointer,
        count: count as usize,
    };
    let mut rejected = Vec::new();
    for activation in activations.as_slice().iter().flatten() {
        let name =
            activation_name(activation).unwrap_or_else(|| "Unnamed hardware transform".into());
        // SAFETY: the activation is a live IMFActivate returned by MFTEnumEx.
        let transform = unsafe { activation.ActivateObject::<IMFTransform>() };
        let Ok(transform) = transform else {
            continue;
        };
        // SAFETY: GetAttributes returns the transform-owned attribute store.
        let d3d11_aware = unsafe { transform.GetAttributes() }
            .ok()
            .and_then(|attributes| unsafe { attributes.GetUINT32(&MF_SA_D3D11_AWARE) }.ok())
            .is_some_and(|value| value != 0);
        // SAFETY: deactivation is allowed after the activated transform is released.
        drop(transform);
        let _ = unsafe { activation.ShutdownObject() };
        if d3d11_aware {
            return Ok(TransformCapability {
                name,
                d3d11_aware: true,
            });
        }
        rejected.push(format!("{name} (D3D11-aware=false)"));
    }
    if !rejected.is_empty() {
        return Err(VideoError::TransformEnumeration {
            kind,
            detail: format!(
                "hardware candidates did not expose MF_SA_D3D11_AWARE: {}",
                rejected.join(", ")
            ),
        });
    }
    Err(VideoError::MissingTransform { kind })
}

fn activation_name(activation: &IMFActivate) -> Option<String> {
    let mut value = PWSTR::null();
    let mut length = 0_u32;
    // SAFETY: Media Foundation allocates `value`; it is freed below with CoTaskMemFree.
    unsafe { activation.GetAllocatedString(&MFT_FRIENDLY_NAME_Attribute, &mut value, &mut length) }
        .ok()?;
    if value.is_null() {
        return None;
    }
    // SAFETY: GetAllocatedString returned `length` UTF-16 code units.
    let name = String::from_utf16_lossy(unsafe { slice::from_raw_parts(value.0, length as usize) });
    // SAFETY: the string is allocated with CoTaskMemAlloc.
    unsafe { CoTaskMemFree(Some(value.0.cast())) };
    Some(name)
}

fn feature_level_name(level: D3D_FEATURE_LEVEL) -> &'static str {
    match level {
        D3D_FEATURE_LEVEL_11_1 => "11.1",
        D3D_FEATURE_LEVEL_11_0 => "11.0",
        _ => "unknown",
    }
}
