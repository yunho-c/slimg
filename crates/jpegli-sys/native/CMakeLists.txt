cmake_minimum_required(VERSION 3.16)
cmake_policy(SET CMP0091 NEW)
project(slimg_jpegli LANGUAGES C CXX)

if(NOT IS_DIRECTORY "${SLIMG_JPEGLI_SOURCE}")
  message(FATAL_ERROR "SLIMG_JPEGLI_SOURCE must point to the pinned google/jpegli checkout")
endif()

set(BUILD_SHARED_LIBS OFF CACHE BOOL "" FORCE)
set(BUILD_TESTING OFF CACHE BOOL "" FORCE)
foreach(option TOOLS DEVTOOLS FUZZERS DOXYGEN MANPAGES BENCHMARK JNI SJPEG OPENEXR TCMALLOC JPEGLI_LIBJPEG)
  set(JPEGLI_ENABLE_${option} OFF CACHE BOOL "" FORCE)
endforeach()
set(JPEGLI_BUNDLE_LIBPNG OFF CACHE BOOL "" FORCE)
set(JPEGLI_ENABLE_SKCMS ON CACHE BOOL "" FORCE)
set(JPEGLI_FORCE_SYSTEM_HWY OFF CACHE BOOL "" FORCE)
set(HWY_ENABLE_INSTALL OFF CACHE BOOL "" FORCE)
set(CMAKE_POSITION_INDEPENDENT_CODE ON)

# EXCLUDE_FROM_ALL also excludes upstream's unrelated install rules. Only the
# narrow shim and its two dependencies are built and installed below.
add_subdirectory("${SLIMG_JPEGLI_SOURCE}" jpegli EXCLUDE_FROM_ALL)

# Both libjxl and jpegli use Highway, potentially at incompatible revisions.
# Rename its C++ namespace in BOTH the provider and every consumer, as well as
# the archive, so native linker search order cannot choose libjxl's copy.
foreach(target hwy jpegli-static)
  target_compile_definitions(${target} PRIVATE hwy=slimg_jpegli_hwy_v1)
endforeach()
set_target_properties(hwy PROPERTIES OUTPUT_NAME slimg_jpegli_hwy)
set_target_properties(jpegli-static PROPERTIES OUTPUT_NAME slimg_jpegli)

add_library(slimg_jpegli_shim STATIC ../shim/jpegli_shim.cc)
target_compile_features(slimg_jpegli_shim PRIVATE cxx_std_17)
target_include_directories(slimg_jpegli_shim PRIVATE "${SLIMG_JPEGLI_SOURCE}")
target_link_libraries(slimg_jpegli_shim PRIVATE jpegli-static)

install(TARGETS slimg_jpegli_shim jpegli-static hwy ARCHIVE DESTINATION lib)
install(FILES ../shim/jpegli_shim.h DESTINATION include)
install(FILES ../LICENSE DESTINATION licenses RENAME LICENSE.slimg)
install(FILES "${SLIMG_JPEGLI_SOURCE}/LICENSE" "${SLIMG_JPEGLI_SOURCE}/PATENTS" DESTINATION licenses)
install(FILES "${SLIMG_JPEGLI_SOURCE}/third_party/highway/LICENSE" DESTINATION licenses RENAME LICENSE.highway)
install(FILES "${SLIMG_JPEGLI_SOURCE}/third_party/libjpeg-turbo/LICENSE.md" DESTINATION licenses RENAME LICENSE.libjpeg-turbo)
