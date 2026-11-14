cmake_minimum_required(VERSION 3.16)
project(rustorb_interop_opendds CXX)
# No CMAKE_CXX_STANDARD: the OpenDDS targets carry the standard OpenDDS
# was built with, and a different one changes its ABI.
find_package(OpenDDS REQUIRED)
set(IDL ${CMAKE_CURRENT_SOURCE_DIR}/../idl)
# HelloWorld.idl and Keyed.idl have no extensibility annotation, and the
# other implementations make them @final; OpenDDS's default is
# @appendable. The IDL files have no @topic either: --no-default-nested
# makes each struct a topic type. -Gxtypes-complete adds the COMPLETE
# TypeObjects to the type information, which the dynamic cases look up.
set(TOPIC --no-default-nested -Gxtypes-complete)
set(FINAL ${TOPIC} --default-extensibility final)
set(LIBS OpenDDS::Dcps OpenDDS::Rtps OpenDDS::Rtps_Udp)
foreach(t hello keyed evolve_v1 evolve_v2)
  if(t STREQUAL keyed)
    add_executable(${t} keyed.cpp)
  elseif(t STREQUAL hello)
    add_executable(${t} hello.cpp)
  else()
    add_executable(${t} evolve.cpp)
  endif()
  target_include_directories(${t} PRIVATE ${CMAKE_CURRENT_SOURCE_DIR})
  target_link_libraries(${t} ${LIBS})
endforeach()
opendds_target_sources(hello ${IDL}/HelloWorld.idl OPENDDS_IDL_OPTIONS ${FINAL})
opendds_target_sources(keyed ${IDL}/Keyed.idl OPENDDS_IDL_OPTIONS ${FINAL})
opendds_target_sources(evolve_v1 ${IDL}/EvolveV1.idl OPENDDS_IDL_OPTIONS ${TOPIC})
opendds_target_sources(evolve_v2 ${IDL}/EvolveV2.idl OPENDDS_IDL_OPTIONS ${TOPIC})
target_compile_definitions(evolve_v2 PRIVATE EVOLVE_V2)
