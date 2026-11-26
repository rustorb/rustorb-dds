cmake_minimum_required(VERSION 3.16)
project(rustorb_bench_fastdds CXX)
set(CMAKE_CXX_STANDARD 11)
find_package(fastcdr REQUIRED)
find_package(fastrtps REQUIRED)
# The type code comes from fastddsgen (bench/peers.py runs it into
# ${GEN_DIR} before configuring).
set(GEN_DIR "${CMAKE_BINARY_DIR}/gen" CACHE PATH "fastddsgen output")
add_executable(perf perf.cpp ${GEN_DIR}/Perf.cxx ${GEN_DIR}/PerfPubSubTypes.cxx)
target_include_directories(perf PRIVATE ${GEN_DIR})
target_link_libraries(perf fastrtps fastcdr)
