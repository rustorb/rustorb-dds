# Builds abi.c with MSVC (Windows) against the DLL's import library of the
# native archive of a release (release.yml, job native).
#
#   cmake -S tools/release-check -B build/check -DRDDS_PREFIX=<archive dir>
#
# Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.
cmake_minimum_required(VERSION 3.16)
project(rustorb_dds_release_check C)
add_executable(abi abi.c)
target_include_directories(abi PRIVATE "${RDDS_PREFIX}/include")
target_link_libraries(abi PRIVATE "${RDDS_PREFIX}/lib/rustorb_dds_ffi.dll.lib")
