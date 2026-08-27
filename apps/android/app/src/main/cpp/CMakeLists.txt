cmake_minimum_required(VERSION 3.22.1)
project(picoo_jni)

# Xiaomi 15 / Android 15+ devices often use 16 KB memory pages. ELF PT_LOAD
# alignment must be ≥ 16384 or dlopen fails at cold start.
set(CMAKE_SHARED_LINKER_FLAGS
    "${CMAKE_SHARED_LINKER_FLAGS} -Wl,-z,max-page-size=16384")

set(PICOO_FFI_HEADER "${CMAKE_SOURCE_DIR}/../../../../../../crates/picoo-ffi")
set(PICOO_FFI_DIR "${CMAKE_SOURCE_DIR}/../jniLibs/${ANDROID_ABI}")

add_library(picoo_jni SHARED picoo_jni.cpp)
target_include_directories(picoo_jni PRIVATE "${PICOO_FFI_HEADER}")
# Link by -L/-l so DT_NEEDED is "libpicoo_ffi.so" (requires SONAME on the Rust cdylib),
# not the absolute CI/host path baked by IMPORTED_LOCATION.
target_link_directories(picoo_jni PRIVATE "${PICOO_FFI_DIR}")
target_link_libraries(picoo_jni PRIVATE picoo_ffi log)
