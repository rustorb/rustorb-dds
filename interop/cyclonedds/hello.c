/*
 * Hello world publisher and subscriber for Cyclone DDS, written against
 * its public C API only. Used by interop/run.sh.
 *
 *   hello pub <domain> <count> <interval_ms> [best_effort|reliable [size]]
 *   hello sub <domain> <count> <timeout_s> [best_effort|reliable [size]]
 *
 * The publisher waits for a matched reader, then writes <count> samples.
 * The subscriber exits with 0 after <count> valid samples, with 1 on
 * timeout.
 *
 * Best effort by default. When reliable, both sides keep every sample
 * (KEEP_ALL), the publisher waits for acknowledgments before it exits and
 * the subscriber checks that it gets 1, 2, 3... with none missing.
 *
 * The message is "Hello, world!", or with <size>, <size> letters that
 * depend on the index ('a' + (i + index) % 26 for the i-th), so that the
 * sample is too large for a datagram.
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "dds/dds.h"
#include "HelloWorld.h"

#define MESSAGE "Hello, world!"

static int reliable = 0;
static long size = -1;

/* The message of sample <index>, to be freed. */
static char *message(uint32_t index)
{
  if (size < 0)
    return strdup(MESSAGE);
  char *m = malloc((size_t)size + 1);
  for (long i = 0; i < size; i++)
    m[i] = (char)('a' + (i + index) % 26);
  m[size] = 0;
  return m;
}

static void set_reliability(dds_qos_t *qos)
{
  if (reliable) {
    dds_qset_reliability(qos, DDS_RELIABILITY_RELIABLE, DDS_SECS(1));
    dds_qset_history(qos, DDS_HISTORY_KEEP_ALL, 0);
  } else {
    dds_qset_reliability(qos, DDS_RELIABILITY_BEST_EFFORT, 0);
  }
}

static int publish(dds_entity_t pp, dds_entity_t topic, int count, int interval_ms)
{
  dds_qos_t *qos = dds_create_qos();
  set_reliability(qos);
  dds_entity_t writer = dds_create_writer(pp, topic, qos, NULL);
  dds_delete_qos(qos);
  if (writer < 0) {
    fprintf(stderr, "cyclonedds: create_writer: %s\n", dds_strretcode(-writer));
    return 1;
  }
  dds_set_status_mask(writer, DDS_PUBLICATION_MATCHED_STATUS);
  for (int i = 0; i < 300; i++) {
    dds_publication_matched_status_t st;
    dds_get_publication_matched_status(writer, &st);
    if (st.current_count > 0)
      break;
    dds_sleepfor(DDS_MSECS(100));
  }
  /* Give the reader time to match this writer too. */
  dds_sleepfor(DDS_MSECS(500));
  for (int i = 1; i <= count; i++) {
    HelloWorld_Greeting g = { .index = (uint32_t)i, .message = message((uint32_t)i) };
    dds_return_t rc = dds_write(writer, &g);
    free(g.message);
    if (rc != DDS_RETCODE_OK) {
      fprintf(stderr, "cyclonedds: write: %s\n", dds_strretcode(-rc));
      return 1;
    }
    printf("cyclonedds: sent %d\n", i);
    fflush(stdout);
    dds_sleepfor(DDS_MSECS(interval_ms));
  }
  if (reliable) {
    dds_return_t rc = dds_wait_for_acks(writer, DDS_SECS(30));
    if (rc != DDS_RETCODE_OK) {
      fprintf(stderr, "cyclonedds: wait_for_acks: %s\n", dds_strretcode(-rc));
      return 1;
    }
    printf("cyclonedds: all acknowledged\n");
  }
  return 0;
}

static int subscribe(dds_entity_t pp, dds_entity_t topic, int count, int timeout_s)
{
  dds_qos_t *qos = dds_create_qos();
  set_reliability(qos);
  dds_qset_history(qos, DDS_HISTORY_KEEP_ALL, 0);
  dds_entity_t reader = dds_create_reader(pp, topic, qos, NULL);
  dds_delete_qos(qos);
  if (reader < 0) {
    fprintf(stderr, "cyclonedds: create_reader: %s\n", dds_strretcode(-reader));
    return 1;
  }
  int got = 0;
  dds_time_t deadline = dds_time() + DDS_SECS(timeout_s);
  void *samples[1] = { NULL };
  dds_sample_info_t infos[1];
  samples[0] = HelloWorld_Greeting__alloc();
  while (got < count && dds_time() < deadline) {
    dds_return_t n = dds_take(reader, samples, infos, 1, 1);
    if (n > 0 && infos[0].valid_data) {
      HelloWorld_Greeting *g = samples[0];
      char *want = message(g->index);
      int ok = strcmp(g->message, want) == 0;
      free(want);
      if (!ok) {
        fprintf(stderr, "cyclonedds: bad message \"%.40s\" (%zu octets) for %u\n",
                g->message, strlen(g->message), (unsigned)g->index);
        return 1;
      }
      printf("cyclonedds: received %u\n", (unsigned)g->index);
      fflush(stdout);
      got++;
      if (reliable && g->index != (uint32_t)got) {
        fprintf(stderr, "cyclonedds: expected %d, got %u\n", got, (unsigned)g->index);
        return 1;
      }
    } else {
      dds_sleepfor(DDS_MSECS(20));
    }
  }
  HelloWorld_Greeting_free(samples[0], DDS_FREE_ALL);
  if (got < count) {
    fprintf(stderr, "cyclonedds: got %d of %d samples\n", got, count);
    return 1;
  }
  return 0;
}

int main(int argc, char **argv)
{
  if (argc < 5 || argc > 7) {
    fprintf(stderr, "usage: %s pub|sub <domain> <count> <interval_ms|timeout_s> [best_effort|reliable [size]]\n", argv[0]);
    return 2;
  }
  reliable = argc >= 6 && strcmp(argv[5], "reliable") == 0;
  if (argc == 7)
    size = atol(argv[6]);
  dds_domainid_t domain = (dds_domainid_t)atoi(argv[2]);
  int count = atoi(argv[3]);
  int arg = atoi(argv[4]);
  dds_entity_t pp = dds_create_participant(domain, NULL, NULL);
  if (pp < 0) {
    fprintf(stderr, "cyclonedds: create_participant: %s\n", dds_strretcode(-pp));
    return 1;
  }
  dds_entity_t topic = dds_create_topic(pp, &HelloWorld_Greeting_desc, "hello_world", NULL, NULL);
  if (topic < 0) {
    fprintf(stderr, "cyclonedds: create_topic: %s\n", dds_strretcode(-topic));
    return 1;
  }
  int rc = strcmp(argv[1], "pub") == 0 ? publish(pp, topic, count, arg)
                                      : subscribe(pp, topic, count, arg);
  dds_delete(pp);
  return rc;
}
