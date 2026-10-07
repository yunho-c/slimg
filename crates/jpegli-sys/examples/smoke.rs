use slimg_jpegli_sys::*;

fn main() {
    unsafe {
        let mut encoded: slimg_jpegli_result = std::mem::zeroed();
        let rgb: Vec<u8> = (0..32 * 24 * 3).map(|i| (i % 251) as u8).collect();
        assert_eq!(
            slimg_jpegli_encode_rgb(rgb.as_ptr(), 32, 24, 90, &mut encoded),
            0
        );
        assert!(encoded.len > 3);
        assert_eq!(std::slice::from_raw_parts(encoded.data, 2), &[0xff, 0xd8]);
        let mut decoded = std::mem::zeroed();
        assert_eq!(
            slimg_jpegli_decode_rgba(encoded.data, encoded.len, &mut decoded),
            0
        );
        assert_eq!(
            (decoded.width, decoded.height, decoded.len),
            (32, 24, 32 * 24 * 4)
        );
        assert!(
            std::slice::from_raw_parts(decoded.data, decoded.len)
                .iter()
                .skip(3)
                .step_by(4)
                .all(|&alpha| alpha == 255)
        );
        // Exercise every effort tier when testing source and packaged builds.
        // Keeping the legacy entry point above also checks ABI compatibility.
        for effort in [0, 24, 25, 49, 50, 74, 75, 100, 255] {
            let mut with_effort: slimg_jpegli_result = std::mem::zeroed();
            assert_eq!(
                slimg_jpegli_encode_rgb_with_effort(
                    rgb.as_ptr(),
                    32,
                    24,
                    90,
                    effort,
                    &mut with_effort,
                ),
                0
            );
            if effort >= 75 {
                assert_eq!(
                    std::slice::from_raw_parts(with_effort.data, with_effort.len),
                    std::slice::from_raw_parts(encoded.data, encoded.len),
                );
            }
            let mut pixels: slimg_jpegli_result = std::mem::zeroed();
            assert_eq!(
                slimg_jpegli_decode_rgba(with_effort.data, with_effort.len, &mut pixels),
                0
            );
            assert_eq!(
                (pixels.width, pixels.height, pixels.len),
                (32, 24, 32 * 24 * 4)
            );
            slimg_jpegli_free_result(&mut pixels);
            slimg_jpegli_free_result(&mut with_effort);
        }
        slimg_jpegli_free_result(&mut encoded);
        slimg_jpegli_free_result(&mut decoded);
        assert_eq!(
            slimg_jpegli_decode_rgba(b"invalid".as_ptr(), 7, &mut decoded),
            3
        );
        assert!(!decoded.error_message.is_null());
        slimg_jpegli_free_result(&mut decoded);
        slimg_jpegli_free_result(&mut decoded);
        assert_eq!(
            slimg_jpegli_encode_rgb_with_effort(std::ptr::null(), 32, 24, 90, 0, &mut encoded),
            1
        );
        assert!(!encoded.error_message.is_null());
        slimg_jpegli_free_result(&mut encoded);
        assert_eq!(
            slimg_jpegli_encode_rgb_with_effort(
                rgb.as_ptr(),
                32,
                24,
                90,
                100,
                std::ptr::null_mut()
            ),
            1
        );
    }
}
