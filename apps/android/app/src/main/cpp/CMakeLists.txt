cmake_minimum_required(VERSION 3.22.1)
project(picoo_jni)

set(PICOO_FFI_HEADER "${CMAKE_SOURCE_DIR}/../../../../../../crates/picoo-ffi")

add_library(picoo_ffi SHARED IMPORTED)
set_target_properties(
    picoo_ffi
    PROPERTIES
    IMPORTED_LOCATION "${CMAKE_SOURCE_DIR}/../jniLibs/${ANDROID_ABI}/libpicoo_ffi.so")

add_library(picoo_jni SHARED picoo_jni.cpp)
target_include_directories(picoo_jni PRIVATE "${PICOO_FFI_HEADER}")
target_link_libraries(picoo_jni picoo_ffi log)
