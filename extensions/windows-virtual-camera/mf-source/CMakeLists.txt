cmake_minimum_required(VERSION 3.20)
project(PicooVirtualCameraSource LANGUAGES CXX)

add_library(PicooVirtualCameraSource SHARED
    src/dllmain.cpp
    src/ring_reader.cpp
    src/picoo_frame_provider.cpp
    src/picoo_media_stream.cpp
    src/picoo_media_source.cpp
    src/picoo_activator.cpp
)

target_include_directories(PicooVirtualCameraSource PRIVATE include)
target_compile_features(PicooVirtualCameraSource PRIVATE cxx_std_17)

target_compile_definitions(PicooVirtualCameraSource PRIVATE
    WIN32_LEAN_AND_MEAN
    NOMINMAX
    UNICODE
    _UNICODE
    _WIN32_WINNT=0x0A00
)

set_target_properties(PicooVirtualCameraSource PROPERTIES
    OUTPUT_NAME "PicooVirtualCameraSource"
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/bin"
    LIBRARY_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/bin"
)

if(MSVC)
    target_compile_options(PicooVirtualCameraSource PRIVATE /W4 /EHsc)
    target_link_libraries(PicooVirtualCameraSource PRIVATE
        mf
        mfplat
        mfuuid
        ole32
        uuid
    )
endif()
