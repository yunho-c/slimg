#ifndef SLIMG_JPEGLI_SHIM_H_
#define SLIMG_JPEGLI_SHIM_H_

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

enum slimg_jpegli_status {
  SLIMG_JPEGLI_STATUS_OK = 0,
  SLIMG_JPEGLI_STATUS_INVALID_ARGUMENT = 1,
  SLIMG_JPEGLI_STATUS_ENCODE_ERROR = 2,
  SLIMG_JPEGLI_STATUS_DECODE_ERROR = 3,
};

typedef struct slimg_jpegli_result {
  uint8_t* data;
  size_t len;
  uint32_t width;
  uint32_t height;
  int status;
  char* error_message;
} slimg_jpegli_result;

// Preserves the default encoding settings (equivalent to effort 100).
int slimg_jpegli_encode_rgb(const uint8_t* rgb, uint32_t width, uint32_t height,
                            uint8_t quality, slimg_jpegli_result* out);

// Effort 0..24: sequential, fixed Huffman tables; 25..49: sequential,
// optimized Huffman tables; 50..74: progressive level 1; 75..100: level 2.
// Progressive modes use optimized Huffman tables. Values above 100 behave as
// 100. Quality, chroma subsampling and adaptive quantization are unchanged.
int slimg_jpegli_encode_rgb_with_effort(const uint8_t* rgb, uint32_t width,
                                       uint32_t height, uint8_t quality,
                                       uint8_t effort, slimg_jpegli_result* out);

int slimg_jpegli_decode_rgba(const uint8_t* data, size_t len,
                             slimg_jpegli_result* out);

void slimg_jpegli_free_result(slimg_jpegli_result* result);

#ifdef __cplusplus
}  // extern "C"
#endif

#endif  // SLIMG_JPEGLI_SHIM_H_
