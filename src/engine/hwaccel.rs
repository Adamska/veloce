use std::ptr;

use anyhow::{Result, bail};
use ffmpeg_next::{self as ffmpeg, ffi};

#[cfg(target_os = "macos")]
const CANDIDATES: &[(ffi::AVHWDeviceType, &str)] = &[(
    ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_VIDEOTOOLBOX,
    "VideoToolbox",
)];

#[cfg(target_os = "windows")]
const CANDIDATES: &[(ffi::AVHWDeviceType, &str)] = &[
    (ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA, "D3D11VA"),
    (ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_CUDA, "NVDEC"),
];

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const CANDIDATES: &[(ffi::AVHWDeviceType, &str)] = &[
    (ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI, "VAAPI"),
    (ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_CUDA, "NVDEC"),
];

pub struct HardwareDevice {
    context: *mut ffi::AVBufferRef,
    pixel_format: ffi::AVPixelFormat,
    name: &'static str,
}

unsafe impl Send for HardwareDevice {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preference {
    Automatic,
    Hardware,
    Software,
}

pub fn preference() -> Preference {
    let value = std::env::var("VELOCE_HWACCEL").unwrap_or_default();
    match value.to_ascii_lowercase().as_str() {
        "0" | "off" | "false" | "none" | "software" => Preference::Software,
        "1" | "on" | "true" | "force" | "hardware" => Preference::Hardware,
        _ => Preference::Automatic,
    }
}

fn hardware_format(
    codec: *const ffi::AVCodec,
    device_type: ffi::AVHWDeviceType,
) -> Option<ffi::AVPixelFormat> {
    let method = ffi::AV_CODEC_HW_CONFIG_METHOD_HW_DEVICE_CTX as i32;
    (0..)
        .map_while(|index| {
            // SAFETY: `codec` is a valid codec descriptor and the call returns null past the last entry.
            unsafe { ffi::avcodec_get_hw_config(codec, index).as_ref() }
        })
        .find(|config| config.device_type == device_type && config.methods & method != 0)
        .map(|config| config.pix_fmt)
}

impl HardwareDevice {
    pub fn open_for(codec: &ffmpeg::Codec) -> Option<Self> {
        CANDIDATES.iter().find_map(|(device_type, name)| {
            // SAFETY: the codec pointer comes from a live `Codec` handle.
            let pixel_format = hardware_format(unsafe { codec.as_ptr() }, *device_type)?;
            let mut context = ptr::null_mut();
            // SAFETY: `context` receives a new reference owned by the returned device on success.
            let status = unsafe {
                ffi::av_hwdevice_ctx_create(
                    &mut context,
                    *device_type,
                    ptr::null(),
                    ptr::null_mut(),
                    0,
                )
            };
            (status >= 0 && !context.is_null()).then_some(Self {
                context,
                pixel_format,
                name,
            })
        })
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    pub fn produces(&self, frame: &ffmpeg::frame::Video) -> bool {
        // SAFETY: the frame pointer is valid for the lifetime of `frame`.
        unsafe { (*frame.as_ptr()).format == self.pixel_format as i32 }
    }

    pub fn attach(&self, context: &mut ffmpeg::codec::Context) {
        // SAFETY: the codec context is not yet opened; it takes its own device reference and the
        // opaque slot only carries the hardware pixel format as an integer.
        unsafe {
            let raw = context.as_mut_ptr();
            (*raw).hw_device_ctx = ffi::av_buffer_ref(self.context);
            (*raw).opaque = self.pixel_format as usize as *mut std::ffi::c_void;
            (*raw).get_format = Some(select_format);
        }
    }
}

impl Drop for HardwareDevice {
    fn drop(&mut self) {
        // SAFETY: `context` is the reference created in `open_for` and is released exactly once.
        unsafe { ffi::av_buffer_unref(&mut self.context) };
    }
}

unsafe extern "C" fn select_format(
    context: *mut ffi::AVCodecContext,
    formats: *const ffi::AVPixelFormat,
) -> ffi::AVPixelFormat {
    // SAFETY: FFmpeg passes a valid context and a list terminated by AV_PIX_FMT_NONE.
    unsafe {
        let wanted = (*context).opaque as usize as i32;
        let mut cursor = formats;
        while *cursor != ffi::AVPixelFormat::AV_PIX_FMT_NONE {
            if *cursor as i32 == wanted {
                return *cursor;
            }
            cursor = cursor.add(1);
        }
        ffi::avcodec_default_get_format(context, formats)
    }
}

pub fn download(surface: &ffmpeg::frame::Video) -> Result<ffmpeg::frame::Video> {
    let mut output = ffmpeg::frame::Video::empty();
    // SAFETY: both frames are valid; FFmpeg allocates the destination buffers.
    let status = unsafe {
        let status = ffi::av_hwframe_transfer_data(output.as_mut_ptr(), surface.as_ptr(), 0);
        if status >= 0 {
            ffi::av_frame_copy_props(output.as_mut_ptr(), surface.as_ptr());
        }
        status
    };
    if status < 0 {
        bail!(
            "hardware frame download failed: {}",
            ffmpeg::Error::from(status)
        );
    }
    Ok(output)
}
