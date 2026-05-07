use std::ptr;

use libjxl_sys::*;

use crate::error::{Error, Result};

use super::types::EncodeConfig;

/// Safe wrapper around libjxl encoder.
pub(crate) struct Encoder {
    ptr: *mut JxlEncoder,
    runner_ptr: *mut std::ffi::c_void,
}

impl Encoder {
    /// Create a new JXL encoder instance.
    pub fn new() -> Result<Self> {
        let ptr = unsafe { JxlEncoderCreate(ptr::null()) };
        if ptr.is_null() {
            return Err(Error::Encode("failed to create JXL encoder".into()));
        }
        Ok(Self {
            ptr,
            runner_ptr: ptr::null_mut(),
        })
    }

    /// Encode RGBA pixel data into JXL format.
    pub fn encode_rgba(
        &mut self,
        pixels: &[u8],
        width: u32,
        height: u32,
        config: &EncodeConfig,
    ) -> Result<Vec<u8>> {
        unsafe { JxlEncoderReset(self.ptr) };
        self.configure_parallel_runner(config)?;

        self.set_basic_info(width, height, config)?;
        self.set_color_encoding()?;

        let frame_settings = unsafe { JxlEncoderFrameSettingsCreate(self.ptr, ptr::null()) };
        if frame_settings.is_null() {
            return Err(Error::Encode("failed to create frame settings".into()));
        }

        self.configure_frame(frame_settings, config)?;
        self.add_frame(frame_settings, pixels, width, height)?;

        unsafe { JxlEncoderCloseInput(self.ptr) };

        self.process_output()
    }

    fn configure_parallel_runner(&mut self, config: &EncodeConfig) -> Result<()> {
        if !self.runner_ptr.is_null() {
            unsafe {
                JxlThreadParallelRunnerDestroy(self.runner_ptr);
            }
            self.runner_ptr = ptr::null_mut();
        }

        let Some(thread_budget) = config.threads else {
            return Ok(());
        };

        let worker_threads = thread_budget.saturating_sub(1);
        let runner_ptr = unsafe { JxlThreadParallelRunnerCreate(ptr::null(), worker_threads) };
        if runner_ptr.is_null() {
            return Err(Error::Encode("failed to create JXL parallel runner".into()));
        }

        unsafe {
            check_status(
                JxlEncoderSetParallelRunner(self.ptr, Some(JxlThreadParallelRunner), runner_ptr),
                "set parallel runner",
            )?;
        }

        self.runner_ptr = runner_ptr;
        Ok(())
    }

    fn set_basic_info(&self, width: u32, height: u32, config: &EncodeConfig) -> Result<()> {
        unsafe {
            let mut info: JxlBasicInfo = std::mem::zeroed();
            info.xsize = width;
            info.ysize = height;
            info.bits_per_sample = 8;
            info.exponent_bits_per_sample = 0;
            info.num_color_channels = 3;
            info.num_extra_channels = 1;
            info.alpha_bits = 8;
            info.alpha_exponent_bits = 0;
            info.orientation = JxlOrientation_JXL_ORIENT_IDENTITY;
            info.uses_original_profile = if config.lossless { 1 } else { 0 };

            check_status(JxlEncoderSetBasicInfo(self.ptr, &info), "set basic info")
        }
    }

    fn set_color_encoding(&self) -> Result<()> {
        unsafe {
            let mut color: JxlColorEncoding = std::mem::zeroed();
            JxlColorEncodingSetToSRGB(&mut color, 0); // is_gray = false
            check_status(
                JxlEncoderSetColorEncoding(self.ptr, &color),
                "set color encoding",
            )
        }
    }

    fn configure_frame(
        &self,
        settings: *mut JxlEncoderFrameSettings,
        config: &EncodeConfig,
    ) -> Result<()> {
        if config.lossless {
            unsafe {
                check_status(JxlEncoderSetFrameLossless(settings, 1), "set lossless")?;
            }
        }
        if let Some(effort) = config.effort {
            unsafe {
                check_status(
                    JxlEncoderFrameSettingsSetOption(
                        settings,
                        JxlEncoderFrameSettingId_JXL_ENC_FRAME_SETTING_EFFORT,
                        effort_to_jxl_effort(effort) as i64,
                    ),
                    "set effort",
                )?;
            }
        }
        unsafe {
            check_status(
                JxlEncoderSetFrameDistance(settings, config.distance),
                "set distance",
            )
        }
    }

    fn add_frame(
        &self,
        settings: *mut JxlEncoderFrameSettings,
        pixels: &[u8],
        width: u32,
        height: u32,
    ) -> Result<()> {
        let format = JxlPixelFormat {
            num_channels: 4,
            data_type: JxlDataType_JXL_TYPE_UINT8,
            endianness: JxlEndianness_JXL_NATIVE_ENDIAN,
            align: 0,
        };

        let expected = (width as usize) * (height as usize) * 4;
        debug_assert_eq!(pixels.len(), expected);

        unsafe {
            check_status(
                JxlEncoderAddImageFrame(settings, &format, pixels.as_ptr().cast(), pixels.len()),
                "add image frame",
            )
        }
    }

    fn process_output(&self) -> Result<Vec<u8>> {
        let mut buffer = vec![0u8; 64 * 1024]; // 64 KB initial
        let mut all_output = Vec::new();

        loop {
            let mut next_out = buffer.as_mut_ptr();
            let mut avail_out = buffer.len();

            let status =
                unsafe { JxlEncoderProcessOutput(self.ptr, &mut next_out, &mut avail_out) };

            let written = buffer.len() - avail_out;
            all_output.extend_from_slice(&buffer[..written]);

            if status == JxlEncoderStatus_JXL_ENC_SUCCESS {
                return Ok(all_output);
            } else if status == JxlEncoderStatus_JXL_ENC_NEED_MORE_OUTPUT {
                continue;
            } else {
                return Err(Error::Encode("JXL encoding failed".into()));
            }
        }
    }
}

fn effort_to_jxl_effort(effort: u8) -> u8 {
    let effort = effort.min(100) as u16;
    if effort <= 50 {
        (1 + (effort * 6 + 25) / 50) as u8
    } else {
        (7 + ((effort - 50) * 3 + 25) / 50) as u8
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        unsafe {
            if !self.runner_ptr.is_null() {
                JxlThreadParallelRunnerDestroy(self.runner_ptr);
            }
            JxlEncoderDestroy(self.ptr);
        }
    }
}

/// Check a `JxlEncoderStatus` and convert to `Result`.
///
/// # Safety
/// The caller must ensure `status` was returned by a valid libjxl encoder call.
unsafe fn check_status(status: JxlEncoderStatus, context: &str) -> Result<()> {
    if status == JxlEncoderStatus_JXL_ENC_SUCCESS {
        Ok(())
    } else {
        Err(Error::Encode(format!("jxl {context}: status {status}")))
    }
}
