# The rustorb-dds C++ binding: the headers in include/ (the ISO/IEC C++ PSM
# for DDS, header only) on the C ABI of crates/rustorb-dds-ffi, its tests
# and its examples.
#
#   cmake -S cpp -B build/cpp
#   cmake --build build/cpp && ctest --test-dir build/cpp
#
# The build runs cargo for librustorb_dds_ffi (static). Set
# RUSTORB_DDS_FFI_LIBRARY to use one built elsewhere instead. The tests use
# the code rustorb-idl wrote for their IDL, checked in under tests/generated.
#
# Installing: with -DRUSTORB_DDS_BUILD_TESTS=OFF only librustorb_dds_ffi is
# built, and `cmake --install` puts the headers, the library and a CMake
# package (find_package(rustorb_dds), the rustorb_dds::rustorb_dds target)
# under CMAKE_INSTALL_PREFIX.

cmake_minimum_required(VERSION 3.16)
project(rustorb_dds_cpp VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

get_filename_component(RUSTORB_DDS_ROOT "${CMAKE_CURRENT_SOURCE_DIR}/.." ABSOLUTE)
set(RUSTORB_DDS_FFI_LIBRARY "" CACHE FILEPATH "librustorb_dds_ffi to link (default: built with cargo)")
option(RUSTORB_DDS_BUILD_TESTS "build the tests and the examples" ON)

find_package(Threads REQUIRED)

if(RUSTORB_DDS_FFI_LIBRARY)
  add_custom_target(rustorb_dds_ffi_build)
else()
  find_program(CARGO cargo REQUIRED)
  set(RUSTORB_DDS_FFI_LIBRARY
      "${RUSTORB_DDS_ROOT}/target/release/${CMAKE_STATIC_LIBRARY_PREFIX}rustorb_dds_ffi${CMAKE_STATIC_LIBRARY_SUFFIX}")
  # Always run: cargo knows whether it is up to date.
  add_custom_target(rustorb_dds_ffi_build ALL
    COMMAND "${CARGO}" build --release --locked -p rustorb-dds-ffi
            --manifest-path "${RUSTORB_DDS_ROOT}/Cargo.toml"
    BYPRODUCTS "${RUSTORB_DDS_FFI_LIBRARY}"
    COMMENT "Building librustorb_dds_ffi with cargo"
    VERBATIM)
endif()

add_library(rustorb_dds INTERFACE)
add_library(rustorb_dds::rustorb_dds ALIAS rustorb_dds)
add_dependencies(rustorb_dds rustorb_dds_ffi_build)
target_include_directories(rustorb_dds INTERFACE
  "$<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>"
  "$<BUILD_INTERFACE:${RUSTORB_DDS_ROOT}/crates/rustorb-dds-ffi/include>")
# What the static librustorb_dds_ffi needs from the system (also written
# into the installed package).
set(RUSTORB_DDS_SYSTEM_LIBS ${CMAKE_DL_LIBS})
if(UNIX AND NOT APPLE)
  list(APPEND RUSTORB_DDS_SYSTEM_LIBS m)
elseif(APPLE)
  list(APPEND RUSTORB_DDS_SYSTEM_LIBS "-framework CoreFoundation" "-framework Security")
elseif(WIN32)
  list(APPEND RUSTORB_DDS_SYSTEM_LIBS ws2_32 userenv ntdll bcrypt iphlpapi)
endif()
target_link_libraries(rustorb_dds INTERFACE
  "$<BUILD_INTERFACE:${RUSTORB_DDS_FFI_LIBRARY}>" Threads::Threads ${RUSTORB_DDS_SYSTEM_LIBS})

include(GNUInstallDirs)
include(CMakePackageConfigHelpers)
get_filename_component(RUSTORB_DDS_FFI_LIBRARY_NAME "${RUSTORB_DDS_FFI_LIBRARY}" NAME)
install(DIRECTORY include/dds include/rustorb DESTINATION "${CMAKE_INSTALL_INCLUDEDIR}")
install(FILES "${RUSTORB_DDS_ROOT}/crates/rustorb-dds-ffi/include/rustorb/dds.h"
        DESTINATION "${CMAKE_INSTALL_INCLUDEDIR}/rustorb")
install(FILES "${RUSTORB_DDS_FFI_LIBRARY}" DESTINATION "${CMAKE_INSTALL_LIBDIR}")
set(RUSTORB_DDS_CMAKE_DIR "${CMAKE_INSTALL_LIBDIR}/cmake/rustorb_dds")
configure_package_config_file(cmake/rustorb_ddsConfig.cmake.in
  "${CMAKE_CURRENT_BINARY_DIR}/rustorb_ddsConfig.cmake"
  INSTALL_DESTINATION "${RUSTORB_DDS_CMAKE_DIR}"
  PATH_VARS CMAKE_INSTALL_INCLUDEDIR CMAKE_INSTALL_LIBDIR)
# 0.x: a minor version may break the API.
write_basic_package_version_file("${CMAKE_CURRENT_BINARY_DIR}/rustorb_ddsConfigVersion.cmake"
  COMPATIBILITY SameMinorVersion)
install(FILES "${CMAKE_CURRENT_BINARY_DIR}/rustorb_ddsConfig.cmake"
              "${CMAKE_CURRENT_BINARY_DIR}/rustorb_ddsConfigVersion.cmake"
        DESTINATION "${RUSTORB_DDS_CMAKE_DIR}")

if(RUSTORB_DDS_BUILD_TESTS)
  enable_testing()
  set(GENERATED "${CMAKE_CURRENT_SOURCE_DIR}/tests/generated")
  set(FIXTURES "${RUSTORB_DDS_ROOT}/crates/rustorb-xtypes/tests/fixtures/cyclonedds")
  if(MSVC)
    set(WARNINGS /W4)
  else()
    set(WARNINGS -Wall -Wextra -Wpedantic)
  endif()

  foreach(t test_types test_entities)
    add_executable(${t} tests/${t}.cc)
    target_include_directories(${t} PRIVATE "${GENERATED}")
    target_compile_definitions(${t} PRIVATE "RUSTORB_DDS_FIXTURES=\"${FIXTURES}\"")
    target_compile_options(${t} PRIVATE ${WARNINGS})
    target_link_libraries(${t} PRIVATE rustorb_dds)
    add_test(NAME ${t} COMMAND ${t})
  endforeach()

  # The examples, also used by interop/run.sh.
  foreach(e hello_world keyed openrtm)
    add_executable(${e} examples/${e}.cc)
    target_include_directories(${e} PRIVATE "${GENERATED}")
    target_compile_options(${e} PRIVATE ${WARNINGS})
    target_link_libraries(${e} PRIVATE rustorb_dds)
  endforeach()
  add_test(NAME hello_world COMMAND "${CMAKE_COMMAND}"
    -DPROGRAM=$<TARGET_FILE:hello_world> -DDOMAIN=46 -P "${CMAKE_CURRENT_SOURCE_DIR}/tests/pub_sub.cmake")
  add_test(NAME keyed COMMAND "${CMAKE_COMMAND}"
    -DPROGRAM=$<TARGET_FILE:keyed> -DDOMAIN=47 -P "${CMAKE_CURRENT_SOURCE_DIR}/tests/pub_sub.cmake")
endif()
