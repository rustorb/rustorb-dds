/*
 * Keyed instances for Cyclone DDS, written against its public C API only.
 * Used by interop/run.sh (rustorb-dds#17).
 *
 *   keyed pub <domain> <count> <interval_ms>
 *   keyed sub <domain> <count> <timeout_s>
 *
 * Both sides are RELIABLE and KEEP_ALL. The publisher waits for a matched
 * reader, writes <count> samples to the instances id 1, 2, 3 in turn
 * (index 1, 2, 3...), then disposes id 2 and unregisters id 3 (the writer
 * does not dispose unregistered instances), and waits for
 * acknowledgments.
 *
 * The subscriber exits with 0 once it has <count> valid samples, each
 * instance's in order, and has seen id 2 NOT_ALIVE_DISPOSED and id 3
 * NOT_ALIVE_NO_WRITERS; with 1 on timeout or a wrong sample.
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "dds/dds.h"
#include "Keyed.h"

#define MESSAGE "Hello, instance!"

static dds_qos_t *make_qos(void)
{
  dds_qos_t *qos = dds_create_qos();
  dds_qset_reliability(qos, DDS_RELIABILITY_RELIABLE, DDS_SECS(1));
  dds_qset_history(qos, DDS_HISTORY_KEEP_ALL, 0);
  return qos;
}

static int publish(dds_entity_t pp, dds_entity_t topic, int count, int interval_ms)
{
  dds_qos_t *qos = make_qos();
  dds_qset_writer_data_lifecycle(qos, false);
  dds_entity_t writer = dds_create_writer(pp, topic, qos, NULL);
  dds_delete_qos(qos);
  if (writer < 0) {
    fprintf(stderr, "cyclonedds: create_writer: %s\n", dds_strretcode(-writer));
    return 1;
  }
  for (int i = 0; i < 300; i++) {
    dds_publication_matched_status_t st;
    dds_get_publication_matched_status(writer, &st);
    if (st.current_count > 0)
      break;
    dds_sleepfor(DDS_MSECS(100));
  }
  dds_sleepfor(DDS_MSECS(500));
  for (int i = 1; i <= count; i++) {
    Keyed_Shape s = { .id = (uint32_t)((i - 1) % 3 + 1), .index = (uint32_t)i, .message = MESSAGE };
    if (dds_write(writer, &s) != DDS_RETCODE_OK) {
      fprintf(stderr, "cyclonedds: write failed\n");
      return 1;
    }
    printf("cyclonedds: sent id %u index %d\n", (unsigned)s.id, i);
    fflush(stdout);
    dds_sleepfor(DDS_MSECS(interval_ms));
  }
  Keyed_Shape two = { .id = 2, .index = 0, .message = "" };
  Keyed_Shape three = { .id = 3, .index = 0, .message = "" };
  if (dds_dispose(writer, &two) != DDS_RETCODE_OK || dds_unregister_instance(writer, &three) != DDS_RETCODE_OK) {
    fprintf(stderr, "cyclonedds: dispose or unregister failed\n");
    return 1;
  }
  printf("cyclonedds: disposed 2, unregistered 3\n");
  fflush(stdout);
  dds_return_t rc = dds_wait_for_acks(writer, DDS_SECS(30));
  if (rc != DDS_RETCODE_OK) {
    fprintf(stderr, "cyclonedds: wait_for_acks: %s\n", dds_strretcode(-rc));
    return 1;
  }
  printf("cyclonedds: all acknowledged\n");
  return 0;
}

static const char *state_name(dds_instance_state_t s)
{
  switch (s) {
    case DDS_IST_ALIVE: return "ALIVE";
    case DDS_IST_NOT_ALIVE_DISPOSED: return "NOT_ALIVE_DISPOSED";
    case DDS_IST_NOT_ALIVE_NO_WRITERS: return "NOT_ALIVE_NO_WRITERS";
  }
  return "?";
}

static int subscribe(dds_entity_t pp, dds_entity_t topic, int count, int timeout_s)
{
  dds_qos_t *qos = make_qos();
  dds_entity_t reader = dds_create_reader(pp, topic, qos, NULL);
  dds_delete_qos(qos);
  if (reader < 0) {
    fprintf(stderr, "cyclonedds: create_reader: %s\n", dds_strretcode(-reader));
    return 1;
  }
  int got = 0, disposed = 0, no_writers = 0;
  uint32_t last[4] = { 0, 0, 0, 0 };
  dds_time_t deadline = dds_time() + DDS_SECS(timeout_s);
  void *samples[1] = { NULL };
  dds_sample_info_t infos[1];
  samples[0] = Keyed_Shape__alloc();
  while (!(got >= count && disposed && no_writers) && dds_time() < deadline) {
    dds_return_t n = dds_take(reader, samples, infos, 1, 1);
    if (n <= 0) {
      dds_sleepfor(DDS_MSECS(20));
      continue;
    }
    Keyed_Shape *s = samples[0];
    if (s->id < 1 || s->id > 3) {
      fprintf(stderr, "cyclonedds: bad id %u\n", (unsigned)s->id);
      return 1;
    }
    if (infos[0].valid_data) {
      if (strcmp(s->message, MESSAGE) != 0 || s->index <= last[s->id] ||
          (s->index - 1) % 3 + 1 != s->id) {
        fprintf(stderr, "cyclonedds: bad sample id %u index %u\n", (unsigned)s->id, (unsigned)s->index);
        return 1;
      }
      last[s->id] = s->index;
      got++;
      printf("cyclonedds: received id %u index %u\n", (unsigned)s->id, (unsigned)s->index);
    } else {
      printf("cyclonedds: id %u is %s\n", (unsigned)s->id, state_name(infos[0].instance_state));
      if (s->id == 2 && infos[0].instance_state == DDS_IST_NOT_ALIVE_DISPOSED)
        disposed = 1;
      if (s->id == 3 && infos[0].instance_state == DDS_IST_NOT_ALIVE_NO_WRITERS)
        no_writers = 1;
    }
    fflush(stdout);
  }
  Keyed_Shape_free(samples[0], DDS_FREE_ALL);
  if (got < count || !disposed || !no_writers) {
    fprintf(stderr, "cyclonedds: got %d of %d samples, disposed %d, no writers %d\n", got, count, disposed, no_writers);
    return 1;
  }
  return 0;
}

int main(int argc, char **argv)
{
  if (argc != 5) {
    fprintf(stderr, "usage: %s pub|sub <domain> <count> <interval_ms|timeout_s>\n", argv[0]);
    return 2;
  }
  dds_entity_t pp = dds_create_participant((dds_domainid_t)atoi(argv[2]), NULL, NULL);
  if (pp < 0) {
    fprintf(stderr, "cyclonedds: create_participant: %s\n", dds_strretcode(-pp));
    return 1;
  }
  dds_entity_t topic = dds_create_topic(pp, &Keyed_Shape_desc, "keyed", NULL, NULL);
  if (topic < 0) {
    fprintf(stderr, "cyclonedds: create_topic: %s\n", dds_strretcode(-topic));
    return 1;
  }
  int count = atoi(argv[3]), arg = atoi(argv[4]);
  int rc = strcmp(argv[1], "pub") == 0 ? publish(pp, topic, count, arg)
                                      : subscribe(pp, topic, count, arg);
  dds_delete(pp);
  return rc;
}
