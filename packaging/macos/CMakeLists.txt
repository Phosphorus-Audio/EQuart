cmake_minimum_required(VERSION 3.27)

if(NOT CMAKE_HOST_SYSTEM_NAME STREQUAL "Darwin")
    message(FATAL_ERROR "Building the Audio Unit requires macOS and Xcode command line tools")
endif()
if(NOT DEFINED EQUART_VERSION)
    message(FATAL_ERROR "Pass -DEQUART_VERSION=<Cargo package version>")
endif()

set(CMAKE_OSX_DEPLOYMENT_TARGET "14.0" CACHE STRING "Minimum macOS version")
set(CMAKE_OSX_ARCHITECTURES "arm64;x86_64" CACHE STRING "Universal architectures")
project(equart-au VERSION "${EQUART_VERSION}" LANGUAGES C CXX OBJC OBJCXX)
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

get_filename_component(EQUART_ROOT "${CMAKE_CURRENT_SOURCE_DIR}/../.." ABSOLUTE)
set(EQUART_BUNDLES "${EQUART_ROOT}/target/bundled")
set(EQUART_CLAP "${EQUART_BUNDLES}/equart.clap")
if(NOT EXISTS "${EQUART_CLAP}/Contents/MacOS/equart")
    message(FATAL_ERROR "Build the Universal CLAP first: cargo xtask bundle-universal equart --release --locked")
endif()

include(FetchContent)
# Immutable revisions plus archive checksums keep release builds reproducible.
FetchContent_Declare(clap_sdk
    URL https://codeload.github.com/free-audio/clap/tar.gz/195b42a004144fab0b3cf95e9c067187d15365b7
    URL_HASH SHA256=30f68830ee462dbe86acf0cdb57743cfb9539c6f1fa0d3e2275aa1b06b179cca
    DOWNLOAD_EXTRACT_TIMESTAMP FALSE)
FetchContent_Declare(au_sdk
    URL https://codeload.github.com/apple/AudioUnitSDK/tar.gz/53a9a2008aae7fb1b0a9f093dd523b9b12f6c0d9
    URL_HASH SHA256=5d6ef0c2085e93d79da968695e2738784c19be84823901dfa2487e12ec75fdb9
    DOWNLOAD_EXTRACT_TIMESTAMP FALSE
    # clap-wrapper compiles the SDK sources itself; only download Apple's tree.
    SOURCE_SUBDIR download-only)
FetchContent_MakeAvailable(clap_sdk au_sdk)
set(CLAP_SDK_ROOT "${clap_sdk_SOURCE_DIR}")
set(AUDIOUNIT_SDK_ROOT "${au_sdk_SOURCE_DIR}")
set(CLAP_WRAPPER_DOWNLOAD_DEPENDENCIES OFF CACHE BOOL "Use the pinned SDKs")
set(CLAP_WRAPPER_COPY_AFTER_BUILD OFF CACHE BOOL "Install only through packaging")
FetchContent_Declare(clap_wrapper
    URL https://codeload.github.com/free-audio/clap-wrapper/tar.gz/806bba0a5f792e3966b99c1561ab9952d3316d7c
    URL_HASH SHA256=5a346f9a651db5ce24a3f1c09cd4b6d71c3e75ca2a6fa79a752db896a92f6cb3
    DOWNLOAD_EXTRACT_TIMESTAMP FALSE)
FetchContent_MakeAvailable(clap_wrapper)

add_library(equart-au MODULE)
set_target_properties(equart-au PROPERTIES LIBRARY_OUTPUT_DIRECTORY "${EQUART_BUNDLES}")
target_add_auv2_wrapper(
    TARGET equart-au
    OUTPUT_NAME equart
    BUNDLE_IDENTIFIER audio.equart.eq.component
    BUNDLE_VERSION "${EQUART_VERSION}"
    MANUFACTURER_NAME "Phosphorus Audio"
    MANUFACTURER_CODE PhAu
    SUBTYPE_CODE Eqrt
    INSTRUMENT_TYPE aufx
    MACOS_EMBEDDED_CLAP_LOCATION "${EQUART_CLAP}")

file(MAKE_DIRECTORY "${EQUART_BUNDLES}/au-licenses")
configure_file("${clap_wrapper_SOURCE_DIR}/LICENSE" "${EQUART_BUNDLES}/au-licenses/clap-wrapper-MIT.txt" COPYONLY)
configure_file("${clap_sdk_SOURCE_DIR}/LICENSE" "${EQUART_BUNDLES}/au-licenses/clap-MIT.txt" COPYONLY)
configure_file("${au_sdk_SOURCE_DIR}/LICENSE.txt" "${EQUART_BUNDLES}/au-licenses/AudioUnitSDK.txt" COPYONLY)
