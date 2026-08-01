use crate::{
    DecodedNv12Frame, DecoderConfig, EncodedAccessUnit, EncoderConfig, TransformCapability,
    VideoCapability, VideoError,
};
use std::{
    mem::ManuallyDrop,
    ptr, slice, thread,
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{HMODULE, RPC_E_CHANGED_MODE},
        Graphics::{
            Direct3D::{
                D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_11_0,
                D3D_FEATURE_LEVEL_11_1,
            },
            Direct3D11::{
                D3D11_BIND_SHADER_RESOURCE, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                D3D11_CREATE_DEVICE_FLAG, D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_SDK_VERSION,
                D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11CreateDevice, ID3D11Device,
                ID3D11DeviceContext, ID3D11Texture2D,
            },
            Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_SAMPLE_DESC},
        },
        Media::MediaFoundation::{
            IMFActivate, IMFDXGIDeviceManager, IMFMediaEventGenerator, IMFSample, IMFTransform,
            METransformHaveOutput, METransformNeedInput, MF_E_NO_MORE_TYPES,
            MF_E_TRANSFORM_NEED_MORE_INPUT, MF_E_TRANSFORM_STREAM_CHANGE, MF_EVENT_FLAG_NO_WAIT,
            MF_LOW_LATENCY, MF_MT_AVG_BITRATE, MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_RATE,
            MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE, MF_MT_MPEG2_PROFILE,
            MF_MT_PIXEL_ASPECT_RATIO, MF_MT_SUBTYPE, MF_SA_D3D11_AWARE, MF_TRANSFORM_ASYNC,
            MF_TRANSFORM_ASYNC_UNLOCK, MF_VERSION, MFCreateDXGIDeviceManager,
            MFCreateDXGISurfaceBuffer, MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample,
            MFMediaType_Video, MFSTARTUP_FULL, MFSampleExtension_CleanPoint, MFShutdown, MFStartup,
            MFT_CATEGORY_VIDEO_DECODER, MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_FLAG,
            MFT_ENUM_FLAG_ALL, MFT_ENUM_FLAG_HARDWARE, MFT_ENUM_FLAG_SORTANDFILTER,
            MFT_FRIENDLY_NAME_Attribute, MFT_MESSAGE_NOTIFY_BEGIN_STREAMING,
            MFT_MESSAGE_NOTIFY_END_OF_STREAM, MFT_MESSAGE_NOTIFY_END_STREAMING,
            MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_MESSAGE_SET_D3D_MANAGER,
            MFT_OUTPUT_DATA_BUFFER, MFT_OUTPUT_STREAM_PROVIDES_SAMPLES, MFT_REGISTER_TYPE_INFO,
            MFTEnumEx, MFVideoFormat_H264, MFVideoFormat_NV12, MFVideoInterlace_Progressive,
            eAVEncH264VProfile_Main,
        },
        System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize},
    },
    core::{Error as WindowsError, GUID, Interface, PWSTR},
};

const CODEC_EVENT_TIMEOUT: Duration = Duration::from_secs(2);
const FALLBACK_OUTPUT_BUFFER_BYTES: u32 = 4 * 1024 * 1024;

impl From<WindowsError> for VideoError {
    fn from(error: WindowsError) -> Self {
        Self::Codec(error.to_string())
    }
}

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
    let d3d = create_video_device()?;
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
        d3d11_feature_level: feature_level_name(d3d.feature_level).into(),
        encoder,
        decoder,
    })
}

struct D3dContext {
    feature_level: D3D_FEATURE_LEVEL,
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    manager: IMFDXGIDeviceManager,
}

fn create_video_device() -> Result<D3dContext, VideoError> {
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
    let context = context
        .ok_or_else(|| VideoError::D3d11("device creation returned a null context".into()))?;
    Ok(D3dContext {
        feature_level: selected,
        device,
        context,
        manager,
    })
}

pub struct HardwareH264Encoder {
    transform: Option<IMFTransform>,
    activation: IMFActivate,
    event_generator: Option<IMFMediaEventGenerator>,
    need_input: bool,
    have_output: bool,
    input_texture: ID3D11Texture2D,
    width: u32,
    height: u32,
    frame_duration_hns: i64,
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    manager: IMFDXGIDeviceManager,
    _runtime: RuntimeGuard,
}

impl HardwareH264Encoder {
    pub fn new(config: EncoderConfig) -> Result<Self, VideoError> {
        validate_encoder_config(config)?;
        let runtime = RuntimeGuard::start()?;
        let d3d = create_video_device()?;
        let (activation, transform) = activate_hardware_encoder()?;
        // SAFETY: the transform-owned attribute store remains valid while the
        // transform is alive.
        let attributes = unsafe { transform.GetAttributes() }
            .map_err(|error| VideoError::Codec(error.to_string()))?;
        let is_async = unsafe { attributes.GetUINT32(&MF_TRANSFORM_ASYNC) }.unwrap_or(0) != 0;
        if is_async {
            // SAFETY: asynchronous hardware MFTs require this opt-in before use.
            unsafe { attributes.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1) }
                .map_err(|error| VideoError::Codec(error.to_string()))?;
        }
        // Every selected encoder already advertised MF_SA_D3D11_AWARE.
        // MF_LOW_LATENCY is a standard attribute and is safe to set even when a
        // vendor ignores the hint.
        let _ = unsafe { attributes.SetUINT32(&MF_LOW_LATENCY, 1) };
        // SAFETY: the device manager pointer remains owned by this encoder for
        // the entire transform lifetime.
        unsafe {
            transform.ProcessMessage(
                MFT_MESSAGE_SET_D3D_MANAGER,
                Interface::as_raw(&d3d.manager) as usize,
            )
        }
        .map_err(|error| VideoError::Codec(error.to_string()))?;

        configure_encoder_types(&transform, config)?;
        let input_texture = create_nv12_texture(&d3d.device, config.width, config.height)?;
        let event_generator = if is_async {
            Some(
                transform
                    .cast::<IMFMediaEventGenerator>()
                    .map_err(|error| VideoError::Codec(error.to_string()))?,
            )
        } else {
            None
        };
        // SAFETY: type negotiation and the D3D manager are complete.
        unsafe {
            transform.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?;
            transform.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?;
        }

        Ok(Self {
            transform: Some(transform),
            activation,
            event_generator,
            need_input: false,
            have_output: false,
            input_texture,
            width: config.width,
            height: config.height,
            frame_duration_hns: 10_000_000_i64 / i64::from(config.frames_per_second),
            device: d3d.device,
            context: d3d.context,
            manager: d3d.manager,
            _runtime: runtime,
        })
    }

    pub fn encode_nv12(
        &mut self,
        frame: &[u8],
        timestamp_micros: u64,
    ) -> Result<EncodedAccessUnit, VideoError> {
        let expected = nv12_frame_bytes(self.width, self.height)?;
        if frame.len() != expected {
            return Err(VideoError::InvalidNv12Frame {
                expected,
                actual: frame.len(),
            });
        }
        if self.event_generator.is_some() {
            self.wait_for_need_input()?;
            self.need_input = false;
        }
        // SAFETY: `frame` contains one tightly packed NV12 surface. The texture
        // stays alive until the MFT has emitted this frame's output.
        unsafe {
            self.context.UpdateSubresource(
                &self.input_texture,
                0,
                None,
                frame.as_ptr().cast(),
                self.width,
                0,
            );
        }
        let sample = create_input_sample(
            &self.input_texture,
            timestamp_micros,
            self.frame_duration_hns,
        )?;
        let transform = self
            .transform
            .as_ref()
            .ok_or_else(|| VideoError::Codec("encoder is already shut down".into()))?;
        // SAFETY: stream zero is configured for NV12 and sample owns a matching
        // DXGI surface buffer.
        unsafe { transform.ProcessInput(0, &sample, 0) }
            .map_err(|error| VideoError::Codec(error.to_string()))?;
        if self.event_generator.is_some() {
            self.wait_for_have_output()?;
            self.have_output = false;
        }
        self.read_output(timestamp_micros)
    }

    fn wait_for_need_input(&mut self) -> Result<(), VideoError> {
        if self.need_input {
            return Ok(());
        }
        self.pump_events_until(true)
    }

    fn wait_for_have_output(&mut self) -> Result<(), VideoError> {
        if self.have_output {
            return Ok(());
        }
        self.pump_events_until(false)
    }

    fn pump_events_until(&mut self, need_input: bool) -> Result<(), VideoError> {
        let generator = self
            .event_generator
            .as_ref()
            .ok_or_else(|| VideoError::Codec("encoder is not asynchronous".into()))?;
        let deadline = Instant::now() + CODEC_EVENT_TIMEOUT;
        loop {
            // SAFETY: polling with NO_WAIT never blocks the UI or codec thread.
            match unsafe { generator.GetEvent(MF_EVENT_FLAG_NO_WAIT) } {
                Ok(event) => {
                    // SAFETY: the event is a live IMFMediaEvent.
                    let event_status = unsafe { event.GetStatus() }
                        .map_err(|error| VideoError::Codec(error.to_string()))?;
                    if event_status.is_err() {
                        return Err(VideoError::Codec(
                            WindowsError::from_hresult(event_status).to_string(),
                        ));
                    }
                    let event_type = unsafe { event.GetType() }
                        .map_err(|error| VideoError::Codec(error.to_string()))?;
                    if event_type == METransformNeedInput.0 as u32 {
                        self.need_input = true;
                    } else if event_type == METransformHaveOutput.0 as u32 {
                        self.have_output = true;
                    }
                    if (need_input && self.need_input) || (!need_input && self.have_output) {
                        return Ok(());
                    }
                }
                Err(error)
                    if error.code()
                        == windows::Win32::Media::MediaFoundation::MF_E_NO_EVENTS_AVAILABLE =>
                {
                    if Instant::now() >= deadline {
                        return Err(VideoError::CodecTimeout(
                            CODEC_EVENT_TIMEOUT.as_millis() as u64
                        ));
                    }
                    thread::sleep(Duration::from_millis(1));
                }
                Err(error) => return Err(VideoError::Codec(error.to_string())),
            }
        }
    }

    fn read_output(&self, timestamp_micros: u64) -> Result<EncodedAccessUnit, VideoError> {
        let transform = self
            .transform
            .as_ref()
            .ok_or_else(|| VideoError::Codec("encoder is already shut down".into()))?;
        // SAFETY: output stream zero was configured during construction.
        let stream_info = unsafe { transform.GetOutputStreamInfo(0) }
            .map_err(|error| VideoError::Codec(error.to_string()))?;
        let provides_sample =
            stream_info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32 != 0;
        let supplied_sample = if provides_sample {
            None
        } else {
            let sample = unsafe { MFCreateSample() }
                .map_err(|error| VideoError::Codec(error.to_string()))?;
            let capacity = stream_info.cbSize.max(FALLBACK_OUTPUT_BUFFER_BYTES);
            let buffer = unsafe { MFCreateMemoryBuffer(capacity) }
                .map_err(|error| VideoError::Codec(error.to_string()))?;
            unsafe { sample.AddBuffer(&buffer) }
                .map_err(|error| VideoError::Codec(error.to_string()))?;
            Some(sample)
        };
        let mut output = MFT_OUTPUT_DATA_BUFFER {
            dwStreamID: 0,
            pSample: ManuallyDrop::new(supplied_sample),
            dwStatus: 0,
            pEvents: ManuallyDrop::new(None),
        };
        let mut status = 0;
        let process_result =
            unsafe { transform.ProcessOutput(0, slice::from_mut(&mut output), &mut status) };
        // SAFETY: these two ManuallyDrop fields contain owning COM references
        // after ProcessOutput returns, regardless of success or failure.
        let sample = unsafe { ManuallyDrop::take(&mut output.pSample) };
        let events = unsafe { ManuallyDrop::take(&mut output.pEvents) };
        drop(events);
        process_result.map_err(|error| VideoError::Codec(error.to_string()))?;
        let sample =
            sample.ok_or_else(|| VideoError::Codec("encoder produced no output sample".into()))?;
        let keyframe = unsafe { sample.GetUINT32(&MFSampleExtension_CleanPoint) }.unwrap_or(0) != 0;
        let buffer = unsafe { sample.ConvertToContiguousBuffer() }
            .map_err(|error| VideoError::Codec(error.to_string()))?;
        let mut data = ptr::null_mut();
        let mut current_length = 0_u32;
        unsafe { buffer.Lock(&mut data, None, Some(&mut current_length)) }
            .map_err(|error| VideoError::Codec(error.to_string()))?;
        let bytes = if data.is_null() || current_length == 0 {
            Vec::new()
        } else {
            // SAFETY: IMFMediaBuffer::Lock guarantees `current_length` readable bytes.
            unsafe { slice::from_raw_parts(data, current_length as usize) }.to_vec()
        };
        let unlock_result = unsafe { buffer.Unlock() };
        unlock_result.map_err(|error| VideoError::Codec(error.to_string()))?;
        if bytes.is_empty() {
            return Err(VideoError::Codec(
                "encoder returned an empty H.264 access unit".into(),
            ));
        }
        Ok(EncodedAccessUnit {
            timestamp_micros,
            keyframe,
            bytes,
        })
    }
}

impl Drop for HardwareH264Encoder {
    fn drop(&mut self) {
        if let Some(transform) = self.transform.take() {
            // SAFETY: best-effort orderly shutdown; no error can be surfaced in Drop.
            let _ = unsafe { transform.ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0) };
            let _ = unsafe { transform.ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0) };
            drop(transform);
        }
        let _ = unsafe { self.activation.ShutdownObject() };
        // Keep these ownership relationships explicit until all COM objects above
        // have been shut down.
        let _ = (&self.device, &self.manager);
    }
}

pub struct HardwareH264Decoder {
    transform: Option<IMFTransform>,
    activation: IMFActivate,
    event_generator: Option<IMFMediaEventGenerator>,
    need_input: bool,
    have_output: bool,
    width: u32,
    height: u32,
    frame_duration_hns: i64,
    output_renegotiations: u8,
    device: ID3D11Device,
    manager: IMFDXGIDeviceManager,
    _runtime: RuntimeGuard,
}

impl HardwareH264Decoder {
    pub fn new(config: DecoderConfig) -> Result<Self, VideoError> {
        validate_decoder_config(config)?;
        let runtime = RuntimeGuard::start()?;
        let d3d = create_video_device()?;
        let (activation, transform) = activate_h264_decoder()?;
        let attributes = unsafe { transform.GetAttributes() }
            .map_err(|error| VideoError::Codec(error.to_string()))?;
        let is_async = unsafe { attributes.GetUINT32(&MF_TRANSFORM_ASYNC) }.unwrap_or(0) != 0;
        if is_async {
            unsafe { attributes.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1) }
                .map_err(|error| VideoError::Codec(error.to_string()))?;
        }
        let _ = unsafe { attributes.SetUINT32(&MF_LOW_LATENCY, 1) };
        unsafe {
            transform.ProcessMessage(
                MFT_MESSAGE_SET_D3D_MANAGER,
                Interface::as_raw(&d3d.manager) as usize,
            )
        }
        .map_err(|error| VideoError::Codec(error.to_string()))?;
        configure_decoder_types(&transform, config)?;
        let event_generator = if is_async {
            Some(
                transform
                    .cast::<IMFMediaEventGenerator>()
                    .map_err(|error| VideoError::Codec(error.to_string()))?,
            )
        } else {
            None
        };
        unsafe {
            transform.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?;
            transform.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?;
        }
        Ok(Self {
            transform: Some(transform),
            activation,
            event_generator,
            need_input: false,
            have_output: false,
            width: config.width,
            height: config.height,
            frame_duration_hns: 10_000_000_i64 / i64::from(config.frames_per_second),
            output_renegotiations: 0,
            device: d3d.device,
            manager: d3d.manager,
            _runtime: runtime,
        })
    }

    pub fn decode(
        &mut self,
        access_unit: &[u8],
        timestamp_micros: u64,
    ) -> Result<Option<DecodedNv12Frame>, VideoError> {
        if access_unit.is_empty() {
            return Err(VideoError::Codec("H.264 access unit is empty".into()));
        }
        if self.event_generator.is_some() {
            self.wait_for_need_input()?;
            self.need_input = false;
        }
        let sample =
            create_compressed_sample(access_unit, timestamp_micros, self.frame_duration_hns)?;
        let transform = self
            .transform
            .as_ref()
            .ok_or_else(|| VideoError::Codec("decoder is already shut down".into()))?;
        unsafe { transform.ProcessInput(0, &sample, 0) }
            .map_err(|error| VideoError::Codec(error.to_string()))?;
        if self.event_generator.is_some() {
            self.wait_for_have_output()?;
            self.have_output = false;
        }
        self.read_output(timestamp_micros)
    }

    fn wait_for_need_input(&mut self) -> Result<(), VideoError> {
        if self.need_input {
            return Ok(());
        }
        self.pump_events_until(true)
    }

    fn wait_for_have_output(&mut self) -> Result<(), VideoError> {
        if self.have_output {
            return Ok(());
        }
        self.pump_events_until(false)
    }

    fn pump_events_until(&mut self, need_input: bool) -> Result<(), VideoError> {
        let generator = self
            .event_generator
            .as_ref()
            .ok_or_else(|| VideoError::Codec("decoder is not asynchronous".into()))?;
        let deadline = Instant::now() + CODEC_EVENT_TIMEOUT;
        loop {
            match unsafe { generator.GetEvent(MF_EVENT_FLAG_NO_WAIT) } {
                Ok(event) => {
                    let event_status = unsafe { event.GetStatus() }
                        .map_err(|error| VideoError::Codec(error.to_string()))?;
                    if event_status.is_err() {
                        return Err(VideoError::Codec(
                            WindowsError::from_hresult(event_status).to_string(),
                        ));
                    }
                    let event_type = unsafe { event.GetType() }
                        .map_err(|error| VideoError::Codec(error.to_string()))?;
                    if event_type == METransformNeedInput.0 as u32 {
                        self.need_input = true;
                    } else if event_type == METransformHaveOutput.0 as u32 {
                        self.have_output = true;
                    }
                    if (need_input && self.need_input) || (!need_input && self.have_output) {
                        return Ok(());
                    }
                }
                Err(error)
                    if error.code()
                        == windows::Win32::Media::MediaFoundation::MF_E_NO_EVENTS_AVAILABLE =>
                {
                    if Instant::now() >= deadline {
                        return Err(VideoError::CodecTimeout(
                            CODEC_EVENT_TIMEOUT.as_millis() as u64
                        ));
                    }
                    thread::sleep(Duration::from_millis(1));
                }
                Err(error) => return Err(VideoError::Codec(error.to_string())),
            }
        }
    }

    fn read_output(
        &mut self,
        timestamp_micros: u64,
    ) -> Result<Option<DecodedNv12Frame>, VideoError> {
        let transform = self
            .transform
            .as_ref()
            .ok_or_else(|| VideoError::Codec("decoder is already shut down".into()))?;
        let stream_info = unsafe { transform.GetOutputStreamInfo(0) }
            .map_err(|error| VideoError::Codec(error.to_string()))?;
        let provides_sample =
            stream_info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32 != 0;
        let supplied_sample = if provides_sample {
            None
        } else {
            let sample = unsafe { MFCreateSample() }
                .map_err(|error| VideoError::Codec(error.to_string()))?;
            let expected = u32::try_from(nv12_frame_bytes(self.width, self.height)?)
                .map_err(|_| VideoError::Codec("decoded frame is too large".into()))?;
            let capacity = stream_info.cbSize.max(expected);
            let buffer = unsafe { MFCreateMemoryBuffer(capacity) }
                .map_err(|error| VideoError::Codec(error.to_string()))?;
            unsafe { sample.AddBuffer(&buffer) }
                .map_err(|error| VideoError::Codec(error.to_string()))?;
            Some(sample)
        };
        let mut output = MFT_OUTPUT_DATA_BUFFER {
            dwStreamID: 0,
            pSample: ManuallyDrop::new(supplied_sample),
            dwStatus: 0,
            pEvents: ManuallyDrop::new(None),
        };
        let mut status = 0;
        let process_result =
            unsafe { transform.ProcessOutput(0, slice::from_mut(&mut output), &mut status) };
        let sample = unsafe { ManuallyDrop::take(&mut output.pSample) };
        let events = unsafe { ManuallyDrop::take(&mut output.pEvents) };
        drop(events);
        if let Err(error) = process_result {
            if error.code() == MF_E_TRANSFORM_NEED_MORE_INPUT {
                return Ok(None);
            }
            if error.code() == MF_E_TRANSFORM_STREAM_CHANGE && self.output_renegotiations < 4 {
                select_decoder_output_type(transform)?;
                self.output_renegotiations += 1;
                return self.read_output(timestamp_micros);
            }
            return Err(VideoError::Codec(error.to_string()));
        }
        let sample =
            sample.ok_or_else(|| VideoError::Codec("decoder produced no output sample".into()))?;
        let buffer = unsafe { sample.ConvertToContiguousBuffer() }
            .map_err(|error| VideoError::Codec(error.to_string()))?;
        let mut data = ptr::null_mut();
        let mut current_length = 0_u32;
        unsafe { buffer.Lock(&mut data, None, Some(&mut current_length)) }
            .map_err(|error| VideoError::Codec(error.to_string()))?;
        let bytes = if data.is_null() || current_length == 0 {
            Vec::new()
        } else {
            unsafe { slice::from_raw_parts(data, current_length as usize) }.to_vec()
        };
        let unlock_result = unsafe { buffer.Unlock() };
        unlock_result.map_err(|error| VideoError::Codec(error.to_string()))?;
        let expected = nv12_frame_bytes(self.width, self.height)?;
        if bytes.len() < expected {
            return Err(VideoError::Codec(format!(
                "decoder returned {} NV12 bytes; expected at least {expected}",
                bytes.len()
            )));
        }
        Ok(Some(DecodedNv12Frame {
            timestamp_micros,
            width: self.width,
            height: self.height,
            bytes: bytes[..expected].to_vec(),
        }))
    }
}

impl Drop for HardwareH264Decoder {
    fn drop(&mut self) {
        if let Some(transform) = self.transform.take() {
            let _ = unsafe { transform.ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0) };
            let _ = unsafe { transform.ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0) };
            drop(transform);
        }
        let _ = unsafe { self.activation.ShutdownObject() };
        let _ = (&self.device, &self.manager);
    }
}

fn validate_encoder_config(config: EncoderConfig) -> Result<(), VideoError> {
    if config.width == 0 || config.height == 0 || config.width % 2 != 0 || config.height % 2 != 0 {
        return Err(VideoError::InvalidConfiguration(
            "NV12 dimensions must be non-zero even numbers".into(),
        ));
    }
    if config.frames_per_second == 0 || config.frames_per_second > 240 {
        return Err(VideoError::InvalidConfiguration(
            "frame rate must be between 1 and 240".into(),
        ));
    }
    if config.bitrate < 128_000 {
        return Err(VideoError::InvalidConfiguration(
            "bitrate must be at least 128000 bits per second".into(),
        ));
    }
    nv12_frame_bytes(config.width, config.height)?;
    Ok(())
}

fn validate_decoder_config(config: DecoderConfig) -> Result<(), VideoError> {
    if config.width == 0 || config.height == 0 || config.width % 2 != 0 || config.height % 2 != 0 {
        return Err(VideoError::InvalidConfiguration(
            "decoder dimensions must be non-zero even numbers".into(),
        ));
    }
    if config.frames_per_second == 0 || config.frames_per_second > 240 {
        return Err(VideoError::InvalidConfiguration(
            "decoder frame rate must be between 1 and 240".into(),
        ));
    }
    nv12_frame_bytes(config.width, config.height)?;
    Ok(())
}

fn nv12_frame_bytes(width: u32, height: u32) -> Result<usize, VideoError> {
    let pixels = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .ok_or_else(|| VideoError::InvalidConfiguration("frame dimensions overflow".into()))?;
    pixels
        .checked_add(pixels / 2)
        .ok_or_else(|| VideoError::InvalidConfiguration("NV12 frame size overflows".into()))
}

fn activate_hardware_encoder() -> Result<(IMFActivate, IMFTransform), VideoError> {
    let input = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_NV12,
    };
    let output = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_H264,
    };
    let flags = MFT_ENUM_FLAG(MFT_ENUM_FLAG_HARDWARE.0 | MFT_ENUM_FLAG_SORTANDFILTER.0);
    let mut pointer: *mut Option<IMFActivate> = ptr::null_mut();
    let mut count = 0_u32;
    unsafe {
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_ENCODER,
            flags,
            Some(&input),
            Some(&output),
            &mut pointer,
            &mut count,
        )
    }
    .map_err(|error| VideoError::TransformEnumeration {
        kind: "encoder",
        detail: error.to_string(),
    })?;
    let activations = ActivationArray {
        pointer,
        count: count as usize,
    };
    for activation in activations.as_slice().iter().flatten() {
        let Ok(transform) = (unsafe { activation.ActivateObject::<IMFTransform>() }) else {
            continue;
        };
        let aware = unsafe { transform.GetAttributes() }
            .ok()
            .and_then(|attributes| unsafe { attributes.GetUINT32(&MF_SA_D3D11_AWARE) }.ok())
            .is_some_and(|value| value != 0);
        if aware {
            return Ok((activation.clone(), transform));
        }
        drop(transform);
        let _ = unsafe { activation.ShutdownObject() };
    }
    Err(VideoError::MissingTransform { kind: "encoder" })
}

fn activate_h264_decoder() -> Result<(IMFActivate, IMFTransform), VideoError> {
    let input = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_H264,
    };
    let flags = MFT_ENUM_FLAG(MFT_ENUM_FLAG_ALL.0 | MFT_ENUM_FLAG_SORTANDFILTER.0);
    let mut pointer: *mut Option<IMFActivate> = ptr::null_mut();
    let mut count = 0_u32;
    unsafe {
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_DECODER,
            flags,
            Some(&input),
            None,
            &mut pointer,
            &mut count,
        )
    }
    .map_err(|error| VideoError::TransformEnumeration {
        kind: "decoder",
        detail: error.to_string(),
    })?;
    let activations = ActivationArray {
        pointer,
        count: count as usize,
    };
    for activation in activations.as_slice().iter().flatten() {
        let Ok(transform) = (unsafe { activation.ActivateObject::<IMFTransform>() }) else {
            continue;
        };
        let aware = unsafe { transform.GetAttributes() }
            .ok()
            .and_then(|attributes| unsafe { attributes.GetUINT32(&MF_SA_D3D11_AWARE) }.ok())
            .is_some_and(|value| value != 0);
        if aware {
            return Ok((activation.clone(), transform));
        }
        drop(transform);
        let _ = unsafe { activation.ShutdownObject() };
    }
    Err(VideoError::MissingTransform { kind: "decoder" })
}

fn configure_encoder_types(
    transform: &IMFTransform,
    config: EncoderConfig,
) -> Result<(), VideoError> {
    let output =
        unsafe { MFCreateMediaType() }.map_err(|error| VideoError::Codec(error.to_string()))?;
    unsafe {
        output.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        output.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
        output.SetUINT32(&MF_MT_AVG_BITRATE, config.bitrate)?;
        output.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
        output.SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_Main.0 as u32)?;
        output.SetUINT64(&MF_MT_FRAME_SIZE, pack_ratio(config.width, config.height))?;
        output.SetUINT64(&MF_MT_FRAME_RATE, pack_ratio(config.frames_per_second, 1))?;
        output.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack_ratio(1, 1))?;
        transform.SetOutputType(0, &output, 0)?;
    }

    let input =
        unsafe { MFCreateMediaType() }.map_err(|error| VideoError::Codec(error.to_string()))?;
    unsafe {
        input.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        input.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)?;
        input.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
        input.SetUINT32(&MF_MT_DEFAULT_STRIDE, config.width)?;
        input.SetUINT64(&MF_MT_FRAME_SIZE, pack_ratio(config.width, config.height))?;
        input.SetUINT64(&MF_MT_FRAME_RATE, pack_ratio(config.frames_per_second, 1))?;
        input.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack_ratio(1, 1))?;
        transform.SetInputType(0, &input, 0)?;
    }
    Ok(())
}

fn configure_decoder_types(
    transform: &IMFTransform,
    config: DecoderConfig,
) -> Result<(), VideoError> {
    let input =
        unsafe { MFCreateMediaType() }.map_err(|error| VideoError::Codec(error.to_string()))?;
    unsafe {
        input.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        input.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
        input.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
        input.SetUINT64(&MF_MT_FRAME_SIZE, pack_ratio(config.width, config.height))?;
        input.SetUINT64(&MF_MT_FRAME_RATE, pack_ratio(config.frames_per_second, 1))?;
        input.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack_ratio(1, 1))?;
        transform.SetInputType(0, &input, 0)?;
    }

    select_decoder_output_type(transform)
}

fn select_decoder_output_type(transform: &IMFTransform) -> Result<(), VideoError> {
    let mut type_index = 0_u32;
    loop {
        let media_type = match unsafe { transform.GetOutputAvailableType(0, type_index) } {
            Ok(media_type) => media_type,
            Err(error) if error.code() == MF_E_NO_MORE_TYPES => {
                return Err(VideoError::Codec(
                    "decoder did not expose an NV12 output type".into(),
                ));
            }
            Err(error) => return Err(VideoError::Codec(error.to_string())),
        };
        let subtype = unsafe { media_type.GetGUID(&MF_MT_SUBTYPE) };
        if subtype.ok().as_ref() == Some(&MFVideoFormat_NV12) {
            unsafe { transform.SetOutputType(0, &media_type, 0) }
                .map_err(|error| VideoError::Codec(error.to_string()))?;
            return Ok(());
        }
        type_index = type_index
            .checked_add(1)
            .ok_or_else(|| VideoError::Codec("decoder output type index overflowed".into()))?;
    }
}

fn create_nv12_texture(
    device: &ID3D11Device,
    width: u32,
    height: u32,
) -> Result<ID3D11Texture2D, VideoError> {
    let description = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_NV12,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let mut texture = None;
    unsafe { device.CreateTexture2D(&description, None, Some(&mut texture)) }
        .map_err(|error| VideoError::D3d11(error.to_string()))?;
    texture.ok_or_else(|| VideoError::D3d11("texture creation returned null".into()))
}

fn create_input_sample(
    texture: &ID3D11Texture2D,
    timestamp_micros: u64,
    duration_hns: i64,
) -> Result<IMFSample, VideoError> {
    let buffer = unsafe { MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, texture, 0, false) }
        .map_err(|error| VideoError::Codec(error.to_string()))?;
    let sample =
        unsafe { MFCreateSample() }.map_err(|error| VideoError::Codec(error.to_string()))?;
    let timestamp_hns = timestamp_micros
        .checked_mul(10)
        .and_then(|value| i64::try_from(value).ok())
        .ok_or_else(|| VideoError::Codec("sample timestamp overflows Media Foundation".into()))?;
    unsafe {
        sample.AddBuffer(&buffer)?;
        sample.SetSampleTime(timestamp_hns)?;
        sample.SetSampleDuration(duration_hns)?;
    }
    Ok(sample)
}

fn create_compressed_sample(
    access_unit: &[u8],
    timestamp_micros: u64,
    duration_hns: i64,
) -> Result<IMFSample, VideoError> {
    let length = u32::try_from(access_unit.len())
        .map_err(|_| VideoError::Codec("H.264 access unit exceeds 4 GiB".into()))?;
    let buffer = unsafe { MFCreateMemoryBuffer(length) }
        .map_err(|error| VideoError::Codec(error.to_string()))?;
    let mut data = ptr::null_mut();
    unsafe { buffer.Lock(&mut data, None, None) }
        .map_err(|error| VideoError::Codec(error.to_string()))?;
    if data.is_null() {
        let _ = unsafe { buffer.Unlock() };
        return Err(VideoError::Codec(
            "Media Foundation returned a null input buffer".into(),
        ));
    }
    unsafe { ptr::copy_nonoverlapping(access_unit.as_ptr(), data, access_unit.len()) };
    let unlock_result = unsafe { buffer.Unlock() };
    unlock_result.map_err(|error| VideoError::Codec(error.to_string()))?;
    unsafe { buffer.SetCurrentLength(length) }
        .map_err(|error| VideoError::Codec(error.to_string()))?;
    let sample =
        unsafe { MFCreateSample() }.map_err(|error| VideoError::Codec(error.to_string()))?;
    let timestamp_hns = timestamp_micros
        .checked_mul(10)
        .and_then(|value| i64::try_from(value).ok())
        .ok_or_else(|| VideoError::Codec("sample timestamp overflows Media Foundation".into()))?;
    unsafe {
        sample.AddBuffer(&buffer)?;
        sample.SetSampleTime(timestamp_hns)?;
        sample.SetSampleDuration(duration_hns)?;
    }
    Ok(sample)
}

fn pack_ratio(numerator: u32, denominator: u32) -> u64 {
    (u64::from(numerator) << 32) | u64::from(denominator)
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
