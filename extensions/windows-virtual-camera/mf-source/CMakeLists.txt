cmake_minimum_required(VERSION 3.20)
project(PicooVirtualCameraSource LANGUAGES CXX)

add_library(PicooVirtualCameraSource SHARED
    src/dllmain.cpp
)

target_compile_features(PicooVirtualCameraSource PRIVATE cxx_std_17)
set_target_properties(PicooVirtualCameraSource PROPERTIES
    OUTPUT_NAME "PicooVirtualCameraSource"
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/bin"
    LIBRARY_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/bin"
)

if(MSVC)
    target_compile_options(PicooVirtualCameraSource PRIVATE /W4)
endif()
