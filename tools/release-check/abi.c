/* A program against the native archive of a release (release.yml, job
 * native): links librustorb_dds_ffi, checks the ABI version and makes a
 * participant on the loopback interface.
 *
 * Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds. */

#include <stdio.h>

#include <rustorb/dds.h>

int main(void) {
  if (rdds_abi_version() != RDDS_ABI_VERSION) {
    fprintf(stderr, "ABI version %u, the header has %u\n", rdds_abi_version(), RDDS_ABI_VERSION);
    return 1;
  }
  rdds_participant *p = NULL;
  if (rdds_participant_new(7, NULL, RDDS_PARTICIPANT_LOCALHOST, &p) != RDDS_RETCODE_OK) {
    fprintf(stderr, "rdds_participant_new: %s\n", rdds_last_error_message());
    return 1;
  }
  printf("rustorb-dds: ABI %u, domain %u\n", rdds_abi_version(), rdds_participant_domain_id(p));
  rdds_participant_release(p);
  return 0;
}
