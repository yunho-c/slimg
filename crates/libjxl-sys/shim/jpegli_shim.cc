#include "jpegli_shim.h"

#include <setjmp.h>
#include <stdlib.h>
#include <string.h>

#include "lib/jpegli/common.h"
#include "lib/jpegli/decode.h"
#include "lib/jpegli/encode.h"

namespace {

struct SlimgErrorManager {
  jpeg_error_mgr base;
  jmp_buf env;
  char message[JMSG_LENGTH_MAX];
};

void ClearResult(slimg_jpegli_result* out) {
  if (out == nullptr) return;
  out->data = nullptr;
  out->len = 0;
  out->width = 0;
  out->height = 0;
  out->status = SLIMG_JPEGLI_STATUS_OK;
  out->error_message = nullptr;
}

char* DuplicateCString(const char* message) {
  if (message == nullptr) return nullptr;
  const size_t len = strlen(message);
  char* copy = static_cast<char*>(malloc(len + 1));
  if (copy == nullptr) return nullptr;
  memcpy(copy, message, len + 1);
  return copy;
}

int Fail(slimg_jpegli_result* out, int status, const char* message) {
  if (out != nullptr) {
    out->status = status;
    out->error_message = DuplicateCString(message);
  }
  return status;
}

void SlimgErrorExit(j_common_ptr cinfo) {
  auto* err = reinterpret_cast<SlimgErrorManager*>(cinfo->err);
  err->message[0] = '\0';
  if (err->base.format_message != nullptr) {
    err->base.format_message(cinfo, err->message);
  }
  longjmp(err->env, 1);
}

}  // namespace

extern "C" int slimg_jpegli_encode_rgb(const uint8_t* rgb, uint32_t width,
                                        uint32_t height, uint8_t quality,
                                        slimg_jpegli_result* out) {
  if (out == nullptr) return SLIMG_JPEGLI_STATUS_INVALID_ARGUMENT;
  ClearResult(out);

  if (rgb == nullptr || width == 0 || height == 0) {
    return Fail(out, SLIMG_JPEGLI_STATUS_INVALID_ARGUMENT,
                "invalid RGB buffer or dimensions");
  }

  jpeg_compress_struct cinfo;
  SlimgErrorManager jerr;
  bool created = false;
  unsigned char* encoded = nullptr;
  unsigned long encoded_size = 0;  // NOLINT

  cinfo.err = jpegli_std_error(&jerr.base);
  jerr.base.error_exit = SlimgErrorExit;

  if (setjmp(jerr.env)) {
    if (created) jpegli_destroy_compress(&cinfo);
    if (encoded != nullptr) free(encoded);
    return Fail(out, SLIMG_JPEGLI_STATUS_ENCODE_ERROR,
                jerr.message[0] != '\0' ? jerr.message : "jpegli encode failed");
  }

  jpegli_create_compress(&cinfo);
  created = true;
  jpegli_mem_dest(&cinfo, &encoded, &encoded_size);

  cinfo.image_width = width;
  cinfo.image_height = height;
  cinfo.input_components = 3;
  cinfo.in_color_space = JCS_RGB;

  jpegli_set_defaults(&cinfo);
  jpegli_set_quality(&cinfo, quality, TRUE);
  jpegli_set_progressive_level(&cinfo, 2);
  cinfo.optimize_coding = TRUE;

  jpegli_start_compress(&cinfo, TRUE);

  const size_t row_stride = static_cast<size_t>(width) * 3;
  while (cinfo.next_scanline < cinfo.image_height) {
    JSAMPROW row[] = {
        const_cast<JSAMPROW>(rgb + static_cast<size_t>(cinfo.next_scanline) * row_stride)};
    jpegli_write_scanlines(&cinfo, row, 1);
  }

  jpegli_finish_compress(&cinfo);
  jpegli_destroy_compress(&cinfo);

  out->data = encoded;
  out->len = static_cast<size_t>(encoded_size);
  out->width = width;
  out->height = height;
  out->status = SLIMG_JPEGLI_STATUS_OK;
  return SLIMG_JPEGLI_STATUS_OK;
}

extern "C" int slimg_jpegli_decode_rgba(const uint8_t* data, size_t len,
                                        slimg_jpegli_result* out) {
  if (out == nullptr) return SLIMG_JPEGLI_STATUS_INVALID_ARGUMENT;
  ClearResult(out);

  if (data == nullptr || len == 0) {
    return Fail(out, SLIMG_JPEGLI_STATUS_INVALID_ARGUMENT,
                "invalid JPEG buffer");
  }

  jpeg_decompress_struct cinfo;
  SlimgErrorManager jerr;
  bool created = false;
  uint8_t* decoded = nullptr;

  cinfo.err = jpegli_std_error(&jerr.base);
  jerr.base.error_exit = SlimgErrorExit;

  if (setjmp(jerr.env)) {
    if (created) jpegli_destroy_decompress(&cinfo);
    if (decoded != nullptr) free(decoded);
    return Fail(out, SLIMG_JPEGLI_STATUS_DECODE_ERROR,
                jerr.message[0] != '\0' ? jerr.message : "jpegli decode failed");
  }

  jpegli_create_decompress(&cinfo);
  created = true;
  jpegli_mem_src(&cinfo, reinterpret_cast<const unsigned char*>(data),
                 static_cast<unsigned long>(len));  // NOLINT
  jpegli_read_header(&cinfo, TRUE);

  cinfo.out_color_space = JCS_EXT_RGBA;
  jpegli_set_output_format(&cinfo, JPEGLI_TYPE_UINT8, JPEGLI_NATIVE_ENDIAN);
  jpegli_start_decompress(&cinfo);

  if (cinfo.output_components != 4) {
    jpegli_destroy_decompress(&cinfo);
    return Fail(out, SLIMG_JPEGLI_STATUS_DECODE_ERROR,
                "jpegli decode did not produce RGBA output");
  }

  const size_t row_stride = static_cast<size_t>(cinfo.output_width) * 4;
  const size_t total_size = row_stride * static_cast<size_t>(cinfo.output_height);
  decoded = static_cast<uint8_t*>(malloc(total_size));
  if (decoded == nullptr) {
    jpegli_destroy_decompress(&cinfo);
    return Fail(out, SLIMG_JPEGLI_STATUS_DECODE_ERROR,
                "failed to allocate RGBA decode buffer");
  }

  while (cinfo.output_scanline < cinfo.output_height) {
    JSAMPROW row[] = {decoded +
                      static_cast<size_t>(cinfo.output_scanline) * row_stride};
    jpegli_read_scanlines(&cinfo, row, 1);
  }

  const uint32_t out_width = cinfo.output_width;
  const uint32_t out_height = cinfo.output_height;
  jpegli_finish_decompress(&cinfo);
  jpegli_destroy_decompress(&cinfo);

  out->data = decoded;
  out->len = total_size;
  out->width = out_width;
  out->height = out_height;
  out->status = SLIMG_JPEGLI_STATUS_OK;
  return SLIMG_JPEGLI_STATUS_OK;
}

extern "C" void slimg_jpegli_free_result(slimg_jpegli_result* result) {
  if (result == nullptr) return;
  if (result->data != nullptr) free(result->data);
  if (result->error_message != nullptr) free(result->error_message);
  ClearResult(result);
}
