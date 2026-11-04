/*
 * Type evolution for Cyclone DDS, written against its public C API only.
 * Used by interop/run.sh (rustorb-dds#33). Built twice: evolve_v1 with
 * the types of EvolveV1.idl, evolve_v2 with those of EvolveV2.idl, which
 * have one more member.
 *
 *   evolve_vN pub <domain> <count> <appendable|mutable>
 *   evolve_vN sub <domain> <count> <timeout_s> <appendable|mutable>
 *
 * The topic is "evolve_appendable" or "evolve_mutable", RELIABLE and
 * KEEP_ALL, written in XCDR2. The publisher waits for a reader, writes
 * <count> samples (number 1, 2, 3..., text "evolve <number>", extra 100 + number in
 * version 2) and waits for acknowledgments. The subscriber exits with 0
 * once it has <count> samples with the expected number and text in order;
 * with 1 on timeout or a wrong sample.
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "dds/dds.h"
#ifdef EVOLVE_V2
#include "EvolveV2.h"
#else
#include "EvolveV1.h"
#endif

static dds_qos_t *make_qos(void)
{
  dds_qos_t *qos = dds_create_qos();
  dds_qset_reliability(qos, DDS_RELIABILITY_RELIABLE, DDS_SECS(1));
  dds_qset_history(qos, DDS_HISTORY_KEEP_ALL, 0);
  dds_data_representation_id_t xcdr2 = DDS_DATA_REPRESENTATION_XCDR2;
  dds_qset_data_representation(qos, 1, &xcdr2);
  return qos;
}

static int publish(dds_entity_t pp, dds_entity_t topic, int count, int mutable_)
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
  for (int i = 1; i <= count; i++) {
    char text[32];
    snprintf(text, sizeof text, "evolve %d", i);
    dds_return_t rc;
    if (mutable_) {
      Evolve_Mutable m = { .number = i, .text = text };
#ifdef EVOLVE_V2
      m.extra = 100 + i;
#endif
      rc = dds_write(writer, &m);
    } else {
      Evolve_Appendable a = { .number = i, .text = text };
#ifdef EVOLVE_V2
      a.extra = 100 + i;
#endif
      rc = dds_write(writer, &a);
    }
    if (rc != DDS_RETCODE_OK) {
      fprintf(stderr, "cyclonedds: write: %s\n", dds_strretcode(-rc));
      return 1;
    }
    printf("cyclonedds: sent %d\n", i);
    fflush(stdout);
    dds_sleepfor(DDS_MSECS(20));
  }
  if (dds_wait_for_acks(writer, DDS_SECS(30)) != DDS_RETCODE_OK) {
    fprintf(stderr, "cyclonedds: not acknowledged\n");
    return 1;
  }
  return 0;
}

static int subscribe(dds_entity_t pp, dds_entity_t topic, int count, int timeout_s, int mutable_)
{
  dds_qos_t *qos = make_qos();
  dds_entity_t reader = dds_create_reader(pp, topic, qos, NULL);
  dds_delete_qos(qos);
  if (reader < 0) {
    fprintf(stderr, "cyclonedds: create_reader: %s\n", dds_strretcode(-reader));
    return 1;
  }
  int got = 0;
  dds_time_t deadline = dds_time() + DDS_SECS(timeout_s);
  while (got < count && dds_time() < deadline) {
    void *samples[1] = { NULL };
    dds_sample_info_t info[1];
    int n = dds_take(reader, samples, info, 1, 1);
    if (n <= 0) {
      dds_sleepfor(DDS_MSECS(20));
      continue;
    }
    if (info[0].valid_data) {
      int id;
      const char *text;
      if (mutable_) {
        Evolve_Mutable *m = samples[0];
        id = m->number;
        text = m->text;
      } else {
        Evolve_Appendable *a = samples[0];
        id = a->number;
        text = a->text;
      }
      char want[32];
      snprintf(want, sizeof want, "evolve %d", got + 1);
      if (id != got + 1 || strcmp(text, want) != 0) {
        fprintf(stderr, "cyclonedds: got id %d text \"%s\", want %d \"%s\"\n", id, text, got + 1, want);
        dds_return_loan(reader, samples, n);
        return 1;
      }
      got++;
      printf("cyclonedds: received %d\n", id);
      fflush(stdout);
    }
    dds_return_loan(reader, samples, n);
  }
  if (got < count) {
    fprintf(stderr, "cyclonedds: got %d of %d samples\n", got, count);
    return 1;
  }
  return 0;
}

int main(int argc, char **argv)
{
  if (argc < 5) {
    fprintf(stderr, "usage: %s pub <domain> <count> <appendable|mutable>\n"
                    "       %s sub <domain> <count> <timeout_s> <appendable|mutable>\n", argv[0], argv[0]);
    return 2;
  }
  int sub = strcmp(argv[1], "sub") == 0;
  int domain = atoi(argv[2]);
  int count = atoi(argv[3]);
  const char *kind = argv[sub ? 5 : 4];
  if (sub && argc < 6)
    return 2;
  int mutable_ = strcmp(kind, "mutable") == 0;
  dds_entity_t pp = dds_create_participant((dds_domainid_t)domain, NULL, NULL);
  if (pp < 0) {
    fprintf(stderr, "cyclonedds: create_participant: %s\n", dds_strretcode(-pp));
    return 1;
  }
  dds_entity_t topic = dds_create_topic(pp, mutable_ ? &Evolve_Mutable_desc : &Evolve_Appendable_desc,
                                        mutable_ ? "evolve_mutable" : "evolve_appendable", NULL, NULL);
  if (topic < 0) {
    fprintf(stderr, "cyclonedds: create_topic: %s\n", dds_strretcode(-topic));
    return 1;
  }
  int rc = sub ? subscribe(pp, topic, count, atoi(argv[4]), mutable_) : publish(pp, topic, count, mutable_);
  dds_delete(pp);
  return rc;
}
