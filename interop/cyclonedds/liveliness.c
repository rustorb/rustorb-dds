/* LIVELINESS across participants, for interop/run.sh: the same as the
 * liveliness example of rustorb-dds (see its documentation).
 *
 *   liveliness pub <domain> <automatic|participant|topic> <lease_ms>
 *   liveliness sub <domain> <automatic|participant|topic> <lease_ms> <timeout_s>
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "dds/dds.h"
#include "HelloWorld.h"

static dds_liveliness_kind_t kind;
static dds_duration_t lease;

static dds_qos_t *make_qos(void)
{
  dds_qos_t *qos = dds_create_qos();
  dds_qset_reliability(qos, DDS_RELIABILITY_RELIABLE, DDS_SECS(1));
  dds_qset_history(qos, DDS_HISTORY_KEEP_ALL, 0);
  dds_qset_liveliness(qos, kind, lease);
  return qos;
}

static int write_one(dds_entity_t writer, uint32_t index)
{
  HelloWorld_Greeting g = { .index = index, .message = "Hello, world!" };
  dds_return_t rc = dds_write(writer, &g);
  if (rc != DDS_RETCODE_OK) {
    fprintf(stderr, "cyclonedds: write: %s\n", dds_strretcode(-rc));
    return 1;
  }
  printf("cyclonedds: sent %u\n", (unsigned)index);
  fflush(stdout);
  return 0;
}

static int publish(dds_entity_t pp, dds_entity_t topic)
{
  dds_qos_t *qos = make_qos();
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
  if (write_one(writer, 1))
    return 1;
  dds_time_t until = dds_time() + 3 * lease;
  while (dds_time() < until) {
    if (kind == DDS_LIVELINESS_MANUAL_BY_PARTICIPANT)
      dds_assert_liveliness(pp);
    else if (kind == DDS_LIVELINESS_MANUAL_BY_TOPIC)
      dds_assert_liveliness(writer);
    dds_sleepfor(lease / 4);
  }
  if (write_one(writer, 2))
    return 1;
  dds_sleepfor(4 * lease);
  if (write_one(writer, 3))
    return 1;
  dds_return_t rc = dds_wait_for_acks(writer, DDS_SECS(30));
  if (rc != DDS_RETCODE_OK) {
    fprintf(stderr, "cyclonedds: wait_for_acks: %s\n", dds_strretcode(-rc));
    return 1;
  }
  dds_sleepfor(DDS_SECS(1));
  dds_liveliness_lost_status_t lost;
  dds_get_liveliness_lost_status(writer, &lost);
  printf("cyclonedds: liveliness lost %u times\n", (unsigned)lost.total_count);
  return 0;
}

static int subscribe(dds_entity_t pp, dds_entity_t topic, int timeout_s)
{
  dds_qos_t *qos = make_qos();
  dds_entity_t reader = dds_create_reader(pp, topic, qos, NULL);
  dds_delete_qos(qos);
  if (reader < 0) {
    fprintf(stderr, "cyclonedds: create_reader: %s\n", dds_strretcode(-reader));
    return 1;
  }
  dds_time_t deadline = dds_time() + DDS_SECS(timeout_s);
  void *samples[1] = { NULL };
  dds_sample_info_t infos[1];
  samples[0] = HelloWorld_Greeting__alloc();
  uint32_t last = 0;
  int not_alive_seen = 0;
  int rc = 1;
  while (dds_time() < deadline) {
    dds_liveliness_changed_status_t s;
    dds_get_liveliness_changed_status(reader, &s);
    if (last >= 1 && s.not_alive_count > 0 && !not_alive_seen) {
      not_alive_seen = 1;
      printf("cyclonedds: writer not alive after sample %u\n", (unsigned)last);
      if (last != 2 || kind == DDS_LIVELINESS_AUTOMATIC) {
        fprintf(stderr, "cyclonedds: writer not alive after sample %u\n", (unsigned)last);
        goto out;
      }
    }
    if (last == 3) {
      if (s.alive_count == 1 && s.not_alive_count == 0) {
        printf("cyclonedds: writer alive again\n");
        rc = 0;
        goto out;
      }
    } else {
      while (dds_take(reader, samples, infos, 1, 1) > 0) {
        if (!infos[0].valid_data)
          continue;
        HelloWorld_Greeting *g = samples[0];
        if (g->index != last + 1) {
          fprintf(stderr, "cyclonedds: expected %u, got %u\n", (unsigned)last + 1, (unsigned)g->index);
          goto out;
        }
        last = g->index;
        printf("cyclonedds: received %u\n", (unsigned)last);
        fflush(stdout);
        if (last == 3 && kind != DDS_LIVELINESS_AUTOMATIC && !not_alive_seen) {
          fprintf(stderr, "cyclonedds: the writer was never seen not alive\n");
          goto out;
        }
      }
    }
    dds_sleepfor(DDS_MSECS(20));
  }
  fprintf(stderr, "cyclonedds: timed out after sample %u\n", (unsigned)last);
out:
  HelloWorld_Greeting_free(samples[0], DDS_FREE_ALL);
  return rc;
}

int main(int argc, char **argv)
{
  if (argc < 5) {
    fprintf(stderr, "usage: %s pub|sub <domain> <automatic|participant|topic> <lease_ms> [timeout_s]\n", argv[0]);
    return 2;
  }
  if (strcmp(argv[3], "automatic") == 0)
    kind = DDS_LIVELINESS_AUTOMATIC;
  else if (strcmp(argv[3], "participant") == 0)
    kind = DDS_LIVELINESS_MANUAL_BY_PARTICIPANT;
  else if (strcmp(argv[3], "topic") == 0)
    kind = DDS_LIVELINESS_MANUAL_BY_TOPIC;
  else {
    fprintf(stderr, "unknown liveliness kind %s\n", argv[3]);
    return 2;
  }
  lease = DDS_MSECS(atoi(argv[4]));
  dds_entity_t pp = dds_create_participant((dds_domainid_t)atoi(argv[2]), NULL, NULL);
  if (pp < 0) {
    fprintf(stderr, "cyclonedds: create_participant: %s\n", dds_strretcode(-pp));
    return 1;
  }
  dds_entity_t topic = dds_create_topic(pp, &HelloWorld_Greeting_desc, "liveliness", NULL, NULL);
  if (topic < 0) {
    fprintf(stderr, "cyclonedds: create_topic: %s\n", dds_strretcode(-topic));
    return 1;
  }
  int rc = strcmp(argv[1], "pub") == 0 ? publish(pp, topic)
                                      : subscribe(pp, topic, argc > 5 ? atoi(argv[5]) : 60);
  dds_delete(pp);
  return rc;
}
