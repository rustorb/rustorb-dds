# ping of bench/bindings.py in C++ (rustorb-dds#107), on the C++ binding
# of this repository (cpp/). Perf_dds.hh is what rustorb-idl -bc++11
# -Wbdds_only writes for bench/idl/Perf.idl (bench/bindings/generated).
cmake_minimum_required(VERSION 3.16)
project(rustorb_dds_bench_ping LANGUAGES CXX)
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
if(NOT CMAKE_BUILD_TYPE)
  set(CMAKE_BUILD_TYPE Release)
endif()
set(RUSTORB_DDS_BUILD_TESTS OFF CACHE BOOL "" FORCE)
add_subdirectory("${CMAKE_CURRENT_SOURCE_DIR}/../../../cpp" rustorb_dds)
add_executable(ping ping.cc)
target_include_directories(ping PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}/../generated/cpp")
target_link_libraries(ping PRIVATE rustorb_dds::rustorb_dds)
