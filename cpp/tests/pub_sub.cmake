# Runs an example's subscriber and publisher at the same time, on the
# loopback interface (RUSTORB_DDS_LOCALHOST): cmake -DPROGRAM=<example>
# -DDOMAIN=<domain> -P pub_sub.cmake. Fails unless both exit with 0.
set(ENV{RUSTORB_DDS_LOCALHOST} 1)
# The commands of one execute_process run together (as a pipeline).
execute_process(
  COMMAND "${PROGRAM}" sub ${DOMAIN} 12 30 reliable
  COMMAND "${PROGRAM}" pub ${DOMAIN} 12 20 reliable
  RESULTS_VARIABLE results
  TIMEOUT 90)
message(STATUS "subscriber, publisher: ${results}")
if(NOT results STREQUAL "0;0")
  message(FATAL_ERROR "the subscriber or the publisher failed: ${results}")
endif()
