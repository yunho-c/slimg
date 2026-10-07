use slimg_core::codec::{Codec, EncodeOptions, ImageData, jpeg::JpegCodec, jxl::JxlCodec};

// Exercise both SIMD implementations in one process; isolated codec builds do
// not catch accidental sharing of different Highway versions at link time.
#[test]
fn jpeg_and_jxl_work_in_the_same_process() {
    let image = ImageData::new(
        16,
        16,
        (0..16 * 16).flat_map(|i| [i as u8, 80, 140, 255]).collect(),
    );
    let options = EncodeOptions {
        quality: 90,
        effort: Some(10),
        threads: Some(2),
        ..Default::default()
    };
    for codec in [
        &JpegCodec as &dyn Codec,
        &JxlCodec as &dyn Codec,
        &JpegCodec as &dyn Codec,
    ] {
        let encoded = codec.encode(&image, &options).unwrap();
        let decoded = codec.decode(&encoded).unwrap();
        assert_eq!(
            (decoded.width, decoded.height, decoded.data.len()),
            (16, 16, 1024)
        );
    }
}
