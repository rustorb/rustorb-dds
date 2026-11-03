cmake_minimum_required(VERSION 3.16)
project(rustorb_interop_fastdds CXX)
set(CMAKE_CXX_STANDARD 11)
find_package(fastcdr REQUIRED)
find_package(fastrtps REQUIRED)
# The type code comes from fastddsgen; run.sh generates it into
# ${GEN_DIR} before configuring.
set(GEN_DIR "${CMAKE_BINARY_DIR}/gen" CACHE PATH "fastddsgen output")
add_executable(hello hello.cpp ${GEN_DIR}/HelloWorld.cxx ${GEN_DIR}/HelloWorldPubSubTypes.cxx)
target_include_directories(hello PRIVATE ${GEN_DIR})
target_link_libraries(hello fastrtps fastcdr)
