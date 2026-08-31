#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(dead_code)]

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

/// Whether this libjxl build includes the reference thread-pool runner.
///
/// The original 0.1.0 prebuilt archives predate threaded JXL support. Source
/// builds and regenerated prebuilts expose it; older prebuilts remain usable
/// for serial encoding.
pub const JXL_THREADS_AVAILABLE: bool = cfg!(slimg_libjxl_threads);

#[cfg(slimg_libjxl_threads)]
/// Creates and attaches libjxl's reference parallel runner to `encoder`.
///
/// # Safety
///
/// `encoder` must point to a live `JxlEncoder`. The returned runner must be
/// destroyed only after the encoder has finished using it.
pub unsafe fn create_encoder_parallel_runner(
    encoder: *mut JxlEncoder,
    worker_threads: usize,
) -> *mut std::ffi::c_void {
    let runner = unsafe { JxlThreadParallelRunnerCreate(std::ptr::null(), worker_threads) };
    if runner.is_null() {
        return runner;
    }

    let status =
        unsafe { JxlEncoderSetParallelRunner(encoder, Some(JxlThreadParallelRunner), runner) };
    if status != JxlEncoderStatus_JXL_ENC_SUCCESS {
        unsafe { JxlThreadParallelRunnerDestroy(runner) };
        return std::ptr::null_mut();
    }

    runner
}

#[cfg(not(slimg_libjxl_threads))]
/// Returns null because this libjxl build has no reference parallel runner.
///
/// # Safety
///
/// This compatibility stub does not dereference `encoder`.
pub unsafe fn create_encoder_parallel_runner(
    _encoder: *mut JxlEncoder,
    _worker_threads: usize,
) -> *mut std::ffi::c_void {
    std::ptr::null_mut()
}

#[cfg(slimg_libjxl_threads)]
/// Destroys a runner created by [`create_encoder_parallel_runner`].
///
/// # Safety
///
/// `runner` must be a live runner returned by this crate and must no longer be
/// in use by its encoder.
pub unsafe fn destroy_encoder_parallel_runner(runner: *mut std::ffi::c_void) {
    unsafe { JxlThreadParallelRunnerDestroy(runner) };
}

#[cfg(not(slimg_libjxl_threads))]
/// Compatibility no-op for builds without the reference parallel runner.
///
/// # Safety
///
/// This compatibility stub does not dereference `runner`.
pub unsafe fn destroy_encoder_parallel_runner(_runner: *mut std::ffi::c_void) {}

#[cfg(feature = "jpegli")]
pub mod jpegli;
