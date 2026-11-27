cmake_minimum_required(VERSION 3.16)
project(rustorb_bench_opendds CXX)
# No CMAKE_CXX_STANDARD: the OpenDDS targets carry the standard OpenDDS
# was built with, and a different one changes its ABI.
find_package(OpenDDS REQUIRED)
add_executable(perf perf.cpp)
target_include_directories(perf PRIVATE ${CMAKE_CURRENT_SOURCE_DIR})
target_link_libraries(perf OpenDDS::Dcps OpenDDS::Rtps OpenDDS::Rtps_Udp)
# Perf.idl has no @topic: --no-default-nested makes Sample a topic type.
opendds_target_sources(perf ${CMAKE_CURRENT_SOURCE_DIR}/../idl/Perf.idl
  OPENDDS_IDL_OPTIONS --no-default-nested)
