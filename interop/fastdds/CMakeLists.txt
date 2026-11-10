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
add_executable(keyed keyed.cpp ${GEN_DIR}/Keyed.cxx ${GEN_DIR}/KeyedPubSubTypes.cxx)
target_include_directories(keyed PRIVATE ${GEN_DIR})
target_link_libraries(keyed fastrtps fastcdr)
add_executable(liveliness liveliness.cpp ${GEN_DIR}/HelloWorld.cxx ${GEN_DIR}/HelloWorldPubSubTypes.cxx)
target_include_directories(liveliness PRIVATE ${GEN_DIR})
target_link_libraries(liveliness fastrtps fastcdr)
# The same hello world, with the TypeObject code of fastddsgen
# -typeobject in ${GEN_DIR}-typeobject: Fast DDS 2.11 makes a
# ContentFilteredTopic only of a type whose TypeObject it has. The other
# programs are built without it, so that Fast DDS announces no TypeObject.
add_executable(hello_filtered hello.cpp ${GEN_DIR}-typeobject/HelloWorld.cxx
  ${GEN_DIR}-typeobject/HelloWorldPubSubTypes.cxx ${GEN_DIR}-typeobject/HelloWorldTypeObject.cxx)
target_include_directories(hello_filtered PRIVATE ${GEN_DIR}-typeobject)
target_compile_definitions(hello_filtered PRIVATE HELLO_TYPEOBJECT)
target_link_libraries(hello_filtered fastrtps fastcdr)
