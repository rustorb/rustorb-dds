/* A C program on the C ABI: a keyed topic, a writer and a reader in two
 * participants, samples through a WaitSet. Built and run by run.sh. */
#include <rustorb/dds.h>

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define CHECK(call)                                                       \
  do {                                                                    \
    rdds_status s_ = (call);                                              \
    if (s_ != RDDS_RETCODE_OK) {                                          \
      fprintf(stderr, "%s:%d: %s: %d %s\n", __FILE__, __LINE__, #call, s_, \
              rdds_last_error_message());                                 \
      exit(1);                                                            \
    }                                                                     \
  } while (0)

#define EXPECT(cond)                                                \
  do {                                                              \
    if (!(cond)) {                                                  \
      fprintf(stderr, "%s:%d: %s\n", __FILE__, __LINE__, #cond);    \
      exit(1);                                                      \
    }                                                               \
  } while (0)

/* struct Reading { @key string sensor; double value; sequence<long> raw; }; */
static rdds_type *reading_type(void) {
  rdds_type *str, *dbl, *lng, *seq, *t;
  rdds_struct_builder *b;
  CHECK(rdds_type_string(false, 0, &str));
  CHECK(rdds_type_primitive(RDDS_TK_FLOAT64, &dbl));
  CHECK(rdds_type_primitive(RDDS_TK_INT32, &lng));
  CHECK(rdds_type_sequence(lng, 0, &seq));
  CHECK(rdds_struct_builder_new("Smoke::Reading", RDDS_APPENDABLE, 0, NULL, &b));
  CHECK(rdds_struct_builder_add_member(b, "sensor", 0, str, RDDS_MEMBER_KEY, NULL));
  CHECK(rdds_struct_builder_add_member(b, "value", 1, dbl, 0, NULL));
  CHECK(rdds_struct_builder_add_member(b, "raw", 2, seq, 0, NULL));
  CHECK(rdds_struct_builder_build(b, &t));
  rdds_struct_builder_release(b);
  rdds_type_release(str);
  rdds_type_release(dbl);
  rdds_type_release(lng);
  rdds_type_release(seq);
  return t;
}

int main(void) {
  const rdds_duration wait = {10, 0};
  EXPECT(rdds_abi_version() == RDDS_ABI_VERSION);
  rdds_type *type = reading_type();

  rdds_participant *p1, *p2;
  CHECK(rdds_participant_new(41, NULL, RDDS_PARTICIPANT_LOCALHOST, &p1));
  CHECK(rdds_participant_new(41, NULL, RDDS_PARTICIPANT_LOCALHOST, &p2));
  rdds_topic *t1, *t2;
  CHECK(rdds_participant_create_topic(p1, "readings", type, &t1));
  CHECK(rdds_participant_create_topic(p2, "readings", type, &t2));
  rdds_publisher *pub;
  rdds_subscriber *sub;
  CHECK(rdds_participant_create_publisher(p1, NULL, &pub));
  CHECK(rdds_participant_create_subscriber(p2, NULL, &sub));

  rdds_writer_qos wq;
  CHECK(rdds_writer_qos_default(&wq));
  wq.history_kind = RDDS_KEEP_ALL_HISTORY_QOS;
  rdds_reader_qos rq;
  CHECK(rdds_reader_qos_default(&rq));
  rq.reliability_kind = RDDS_RELIABLE_RELIABILITY_QOS;
  rq.history_kind = RDDS_KEEP_ALL_HISTORY_QOS;
  rdds_writer *w;
  rdds_reader *r;
  CHECK(rdds_publisher_create_writer(pub, t1, &wq, NULL, 0, &w));
  CHECK(rdds_subscriber_create_reader(sub, t2, &rq, NULL, 0, &r));
  CHECK(rdds_writer_wait_for_subscriptions(w, 1, wait));

  rdds_condition *cond;
  CHECK(rdds_reader_status_condition(r, &cond));
  CHECK(rdds_status_condition_set_enabled_statuses(cond, RDDS_DATA_AVAILABLE_STATUS));
  rdds_waitset *ws;
  CHECK(rdds_waitset_new(&ws));
  CHECK(rdds_waitset_attach_condition(ws, cond));

  rdds_out *out;
  CHECK(rdds_out_new(&out));
  for (int i = 0; i < 3; i++) {
    const int32_t raw[] = {i, i * 10, i * 100};
    rdds_out_clear(out);
    rdds_out_string(out, "thermo", 6);
    rdds_out_float64(out, 20.5 + i);
    rdds_out_length(out, 3);
    rdds_out_int32_array(out, raw, 3);
    CHECK(rdds_writer_write(w, out, NULL));
  }
  rdds_out_release(out);
  CHECK(rdds_writer_wait_for_acknowledgments(w, wait));

  size_t active[1], n = 0, got = 0;
  CHECK(rdds_waitset_wait(ws, wait, active, 1, &n));
  EXPECT(n == 1 && active[0] == 0);
  rdds_samples *samples;
  CHECK(rdds_reader_select(r, true, RDDS_LENGTH_UNLIMITED, RDDS_ANY_SAMPLE_STATE, RDDS_ANY_VIEW_STATE,
                           RDDS_ANY_INSTANCE_STATE, RDDS_SELECT_ANY, NULL, &samples));
  for (size_t i = 0; i < rdds_samples_count(samples); i++) {
    rdds_sample_info info;
    CHECK(rdds_samples_info(samples, i, &info));
    EXPECT(info.valid_data);
    rdds_in *in;
    CHECK(rdds_samples_data(samples, i, &in));
    const char *sensor;
    size_t len;
    rdds_in_string(in, &sensor, &len);
    double value = rdds_in_float64(in);
    uint32_t count = rdds_in_length(in);
    int32_t raw[3] = {0};
    EXPECT(count == 3);
    rdds_in_int32_array(in, raw, count);
    CHECK(rdds_in_status(in));
    EXPECT(rdds_in_at_end(in));
    EXPECT(len == 6 && memcmp(sensor, "thermo", 6) == 0);
    EXPECT(value == 20.5 + (double)got && raw[2] == (int32_t)got * 100);
    rdds_in_release(in);
    got++;
  }
  rdds_samples_release(samples);
  EXPECT(got == 3);

  rdds_waitset_release(ws);
  rdds_condition_release(cond);
  rdds_writer_release(w);
  rdds_reader_release(r);
  rdds_publisher_release(pub);
  rdds_subscriber_release(sub);
  rdds_topic_release(t1);
  rdds_topic_release(t2);
  rdds_participant_release(p1);
  rdds_participant_release(p2);
  rdds_type_release(type);
  puts("ok");
  return 0;
}
