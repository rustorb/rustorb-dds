/*
 * The measurements of rustorb-dds-perf (crates/rustorb-dds-perf) for
 * Cyclone DDS, on its public C API only, with the same command line,
 * protocol and JSON output, so that bench/run.py measures both the same
 * way and they can talk to each other (rustorb-dds#103).
 *
 *   perf ping|pong|pub|sub [--domain N] [--size N | --sizes A,B,...]
 *        [--reliability reliable|best-effort] [--transport udp|shm]
 *        [--warmup S] [--duration S] [--count N] [--rate N] [--depth N]
 *        [--reply-timeout MS] [--timeout S] [--label TEXT] [--json]
 *
 * --transport only goes into the output: Cyclone DDS's shared memory is
 * turned on by its configuration (CYCLONEDDS_URI, which bench/run.py
 * sets) and needs iceoryx's iox-roudi to run.
 */
#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include "dds/dds.h"
#include "dds/version.h"
#include "Perf.h"

#define MAX_SIZES 64
#define MAX_TAKE 256
/* The samples a best-effort sub keeps not yet taken (as rustorb-dds-perf). */
#define READER_MAX_SAMPLES 1000

enum mode { PING, PONG, PUB, SUB };

static struct {
  enum mode mode;
  uint32_t domain;
  size_t sizes[MAX_SIZES];
  int nsizes;
  int reliable;
  const char *transport;
  double warmup, duration, rate, timeout;
  long long count;
  int depth;
  double reply_timeout;
  const char *label;
  int json;
} o;

static const char *mode_name[] = {"ping", "pong", "pub", "sub"};

static double now_s(void)
{
  struct timespec t;
  clock_gettime(CLOCK_MONOTONIC, &t);
  return (double)t.tv_sec + (double)t.tv_nsec * 1e-9;
}

static void sleep_s(double s)
{
  if (s <= 0)
    return;
  struct timespec t = {(time_t)s, (long)((s - (double)(time_t)s) * 1e9)};
  while (nanosleep(&t, &t) == -1 && errno == EINTR) {
  }
}

static void die(const char *what, dds_return_t rc)
{
  fprintf(stderr, "%s: %s\n", what, rc < 0 ? dds_strretcode(rc) : "failed");
  exit(1);
}

static void usage(const char *why)
{
  fprintf(stderr, "%s\nusage: perf ping|pong|pub|sub [options] (see the comment of bench/cyclonedds/perf.c)\n", why);
  exit(2);
}

static void parse(int argc, char **argv)
{
  if (argc < 2)
    usage("no mode");
  if (!strcmp(argv[1], "ping")) o.mode = PING;
  else if (!strcmp(argv[1], "pong")) o.mode = PONG;
  else if (!strcmp(argv[1], "pub")) o.mode = PUB;
  else if (!strcmp(argv[1], "sub")) o.mode = SUB;
  else usage("unknown mode");
  o.reliable = 1;
  o.transport = "udp";
  o.warmup = 1;
  o.duration = 5;
  o.timeout = 60;
  o.reply_timeout = 1.0;
  o.count = -1;
  o.depth = -1;
  for (int i = 2; i < argc; i++) {
    const char *a = argv[i];
    const char *v = i + 1 < argc ? argv[i + 1] : NULL;
#define VALUE() do { if (!v) usage("an option needs a value"); i++; } while (0)
    if (!strcmp(a, "--json")) { o.json = 1; continue; }
    VALUE();
    if (!strcmp(a, "--domain")) o.domain = (uint32_t)atoi(v);
    else if (!strcmp(a, "--size")) { o.sizes[0] = (size_t)atol(v); o.nsizes = 1; }
    else if (!strcmp(a, "--sizes")) {
      char *s = strdup(v), *tok, *save = NULL;
      o.nsizes = 0;
      for (tok = strtok_r(s, ",", &save); tok && o.nsizes < MAX_SIZES; tok = strtok_r(NULL, ",", &save))
        o.sizes[o.nsizes++] = (size_t)atol(tok);
      free(s);
    }
    else if (!strcmp(a, "--reliability")) o.reliable = !strcmp(v, "reliable");
    else if (!strcmp(a, "--transport")) o.transport = v;
    else if (!strcmp(a, "--warmup")) o.warmup = atof(v);
    else if (!strcmp(a, "--duration")) o.duration = atof(v);
    else if (!strcmp(a, "--count")) o.count = atoll(v);
    else if (!strcmp(a, "--rate")) o.rate = atof(v);
    else if (!strcmp(a, "--depth")) o.depth = atoi(v);
    else if (!strcmp(a, "--reply-timeout")) o.reply_timeout = atof(v) / 1000.0;
    else if (!strcmp(a, "--timeout")) o.timeout = atof(v);
    else if (!strcmp(a, "--label")) o.label = v;
    else usage("unknown option");
#undef VALUE
  }
  if (o.nsizes == 0) {
    if (o.mode == PING || o.mode == PONG)
      for (int e = 5; e <= 20; e++)
        o.sizes[o.nsizes++] = (size_t)1 << e;
    else
      o.sizes[o.nsizes++] = 1024;
  }
}

static void json_head(size_t size)
{
  printf("{\"tool\":\"rustorb-dds-perf\",\"impl\":\"cyclonedds\",\"version\":\"%s\","
         "\"mode\":\"%s\",\"size\":%zu,\"reliability\":\"%s\",\"transport\":\"%s\"",
         DDS_VERSION, mode_name[o.mode], size, o.reliable ? "reliable" : "best-effort", o.transport);
  if (o.label)
    printf(",\"label\":\"%s\"", o.label);
}

static int cmp_double(const void *a, const void *b)
{
  double x = *(const double *)a, y = *(const double *)b;
  return x < y ? -1 : x > y;
}

/* The nearest-rank percentile of sorted v. */
static double pct(const double *v, size_t n, double p)
{
  size_t k = (size_t)(p * (double)n + 0.999999999);
  if (k < 1) k = 1;
  if (k > n) k = n;
  return v[k - 1];
}

static dds_entity_t participant, wtopic, rtopic, writer, reader, waitset;

static void setup(void)
{
  int throughput = o.mode == PUB || o.mode == SUB;
  int depth = o.depth > 0 ? o.depth : (throughput ? 100 : 1);
  const char *w = NULL, *r = NULL;
  switch (o.mode) {
    case PING: w = "RustorbPerf_ping"; r = "RustorbPerf_pong"; break;
    case PONG: w = "RustorbPerf_pong"; r = "RustorbPerf_ping"; break;
    case PUB: w = "RustorbPerf_data"; break;
    case SUB: r = "RustorbPerf_data"; break;
  }
  participant = dds_create_participant(o.domain, NULL, NULL);
  if (participant < 0)
    die("dds_create_participant", participant);
  dds_qos_t *wq = dds_create_qos(), *rq = dds_create_qos();
  if (o.reliable) {
    dds_qset_reliability(wq, DDS_RELIABILITY_RELIABLE, DDS_MSECS(100));
    dds_qset_reliability(rq, DDS_RELIABILITY_RELIABLE, DDS_MSECS(100));
  } else {
    dds_qset_reliability(wq, DDS_RELIABILITY_BEST_EFFORT, 0);
    dds_qset_reliability(rq, DDS_RELIABILITY_BEST_EFFORT, 0);
  }
  if (throughput && o.reliable) {
    /* Flow control: a write blocks once depth samples are unacknowledged. */
    dds_qset_history(wq, DDS_HISTORY_KEEP_ALL, 0);
    dds_qset_resource_limits(wq, depth, DDS_LENGTH_UNLIMITED, DDS_LENGTH_UNLIMITED);
  } else {
    dds_qset_history(wq, DDS_HISTORY_KEEP_LAST, depth);
  }
  if (throughput) {
    dds_qset_history(rq, DDS_HISTORY_KEEP_ALL, 0);
    /* A best-effort sub slower than its pub drops what does not fit. */
    if (!o.reliable)
      dds_qset_resource_limits(rq, READER_MAX_SAMPLES, DDS_LENGTH_UNLIMITED, DDS_LENGTH_UNLIMITED);
  } else
    dds_qset_history(rq, DDS_HISTORY_KEEP_LAST, depth);
  if (w) {
    wtopic = dds_create_topic(participant, &RustorbPerf_Sample_desc, w, NULL, NULL);
    if (wtopic < 0) die("dds_create_topic", wtopic);
    writer = dds_create_writer(participant, wtopic, wq, NULL);
    if (writer < 0) die("dds_create_writer", writer);
  }
  if (r) {
    rtopic = dds_create_topic(participant, &RustorbPerf_Sample_desc, r, NULL, NULL);
    if (rtopic < 0) die("dds_create_topic", rtopic);
    reader = dds_create_reader(participant, rtopic, rq, NULL);
    if (reader < 0) die("dds_create_reader", reader);
    dds_set_status_mask(reader, DDS_DATA_AVAILABLE_STATUS);
    waitset = dds_create_waitset(participant);
    dds_waitset_attach(waitset, reader, reader);
  }
  dds_delete_qos(wq);
  dds_delete_qos(rq);
}

static uint32_t writer_matched(void)
{
  dds_publication_matched_status_t st = {0};
  if (writer > 0)
    dds_get_publication_matched_status(writer, &st);
  return st.current_count;
}

static uint32_t reader_matched(void)
{
  dds_subscription_matched_status_t st = {0};
  if (reader > 0)
    dds_get_subscription_matched_status(reader, &st);
  return st.current_count;
}

static void wait_for_match(void)
{
  double deadline = now_s() + o.timeout;
  for (;;) {
    int ok = (!writer || writer_matched() > 0) && (!reader || reader_matched() > 0);
    if (ok) {
      sleep_s(0.3);
      return;
    }
    if (now_s() > deadline) {
      fprintf(stderr, "%s: nothing matched in %.0f s\n", mode_name[o.mode], o.timeout);
      exit(1);
    }
    sleep_s(0.02);
  }
}

/* Waits up to s seconds for data. */
static int wait_data(double s)
{
  if (s <= 0)
    return 0;
  dds_attach_t x;
  return dds_waitset_wait(waitset, &x, 1, (dds_duration_t)(s * 1e9)) > 0;
}

static RustorbPerf_Sample out_sample;

static dds_return_t send(uint64_t seq, uint64_t stamp, size_t len)
{
  if (out_sample.payload._maximum < len) {
    dds_free(out_sample.payload._buffer);
    out_sample.payload._buffer = dds_alloc(len ? len : 1);
    memset(out_sample.payload._buffer, 0x5a, len);
    out_sample.payload._maximum = (uint32_t)len;
  }
  out_sample.payload._length = (uint32_t)len;
  out_sample.seq = seq;
  out_sample.stamp = stamp;
  return dds_write(writer, &out_sample);
}

static void ping(void)
{
  wait_for_match();
  double start = now_s();
  uint64_t seq = 0;
  void *samples[MAX_TAKE];
  dds_sample_info_t infos[MAX_TAKE];
  if (!o.json)
    printf("%8s %8s %6s %9s %9s %9s %9s %9s  (round trip, us)\n", "size", "count", "lost", "p50", "p90", "p99", "p99.9", "max");
  for (int i = 0; i < o.nsizes; i++) {
    size_t size = o.sizes[i];
    double warm_end = now_s() + o.warmup, end = warm_end + o.duration;
    size_t cap = 1 << 16, n = 0;
    double *rtt = malloc(cap * sizeof *rtt);
    uint64_t lost = 0;
    samples[0] = NULL;
    int k = dds_take(reader, samples, infos, MAX_TAKE, MAX_TAKE);
    if (k > 0) dds_return_loan(reader, samples, k);
    for (;;) {
      double t = now_s();
      int measuring = t >= warm_end;
      if (t >= end || (measuring && o.count >= 0 && (long long)n >= o.count))
        break;
      seq++;
      double sent = now_s();
      dds_return_t rc = send(seq, (uint64_t)((sent - start) * 1e9), size);
      if (rc == DDS_RETCODE_TIMEOUT) {
        if (measuring) lost++;
        continue;
      }
      if (rc < 0) die("dds_write", rc);
      double deadline = sent + o.reply_timeout;
      for (;;) {
        double left = deadline - now_s();
        if (left <= 0 || !wait_data(left)) {
          if (measuring) lost++;
          break;
        }
        samples[0] = NULL;
        k = dds_take(reader, samples, infos, MAX_TAKE, MAX_TAKE);
        int found = 0;
        for (int j = 0; j < k; j++)
          if (infos[j].valid_data && ((RustorbPerf_Sample *)samples[j])->seq == seq)
            found = 1;
        if (k > 0) dds_return_loan(reader, samples, k);
        if (found) {
          if (measuring) {
            if (n == cap) rtt = realloc(rtt, (cap *= 2) * sizeof *rtt);
            rtt[n++] = (now_s() - sent) * 1e6;
          }
          break;
        }
      }
    }
    double mean = 0;
    for (size_t j = 0; j < n; j++) mean += rtt[j];
    mean = n ? mean / (double)n : 0;
    qsort(rtt, n, sizeof *rtt, cmp_double);
    double mn = n ? rtt[0] : 0, mx = n ? rtt[n - 1] : 0;
    double p50 = n ? pct(rtt, n, 0.5) : 0, p90 = n ? pct(rtt, n, 0.9) : 0;
    double p99 = n ? pct(rtt, n, 0.99) : 0, p999 = n ? pct(rtt, n, 0.999) : 0;
    if (o.json) {
      json_head(size);
      printf(",\"count\":%zu,\"lost\":%llu,\"seconds\":%.3f,\"rtt_min_us\":%.3f,\"rtt_mean_us\":%.3f,"
             "\"rtt_p50_us\":%.3f,\"rtt_p90_us\":%.3f,\"rtt_p99_us\":%.3f,\"rtt_p999_us\":%.3f,\"rtt_max_us\":%.3f}\n",
             n, (unsigned long long)lost, o.duration, mn, mean, p50, p90, p99, p999, mx);
    } else {
      printf("%8zu %8zu %6llu %9.1f %9.1f %9.1f %9.1f %9.1f\n", size, n, (unsigned long long)lost, p50, p90, p99, p999, mx);
    }
    fflush(stdout);
    free(rtt);
  }
}

static void pong(void)
{
  double deadline = now_s() + o.timeout;
  int matched = 0;
  uint64_t echoed = 0;
  void *samples[MAX_TAKE];
  dds_sample_info_t infos[MAX_TAKE];
  for (;;) {
    wait_data(0.2);
    samples[0] = NULL;
    int k = dds_take(reader, samples, infos, MAX_TAKE, MAX_TAKE);
    for (int j = 0; j < k; j++) {
      if (!infos[j].valid_data)
        continue;
      dds_return_t rc = dds_write(writer, samples[j]);
      if (rc < 0 && rc != DDS_RETCODE_TIMEOUT)
        die("dds_write", rc);
      echoed++;
    }
    if (k > 0)
      dds_return_loan(reader, samples, k);
    uint32_t w = writer_matched(), r = reader_matched();
    if (matched) {
      if (w == 0 && r == 0)
        break;
    } else if (w || r) {
      matched = 1;
    } else if (now_s() > deadline) {
      fprintf(stderr, "pong: no ping in %.0f s\n", o.timeout);
      exit(1);
    }
  }
  if (!o.json)
    printf("pong: sent back %llu samples\n", (unsigned long long)echoed);
}

static void publish(void)
{
  wait_for_match();
  size_t size = o.sizes[0];
  double start = now_s(), give_up = start + o.warmup + o.duration + o.timeout;
  uint64_t seq = 0, blocked = 0;
  for (;;) {
    if (seq % 256 == 0 && (writer_matched() == 0 || now_s() >= give_up))
      break;
    dds_return_t rc = send(seq + 1, (uint64_t)((now_s() - start) * 1e9), size);
    if (rc == DDS_RETCODE_OK) {
      seq++;
    } else if (rc == DDS_RETCODE_TIMEOUT) {
      blocked++;
      if (writer_matched() == 0)
        break;
    } else {
      die("dds_write", rc);
    }
    if (o.rate > 0)
      sleep_s(start + (double)seq / o.rate - now_s());
  }
  double secs = now_s() - start;
  if (o.json) {
    json_head(size);
    printf(",\"samples\":%llu,\"seconds\":%.3f,\"samples_per_s\":%.3f,\"blocked\":%llu}\n",
           (unsigned long long)seq, secs, (double)seq / secs, (unsigned long long)blocked);
  } else {
    printf("pub: wrote %llu samples of %zu octets in %.1f s (%.0f/s), blocked %llu times\n",
           (unsigned long long)seq, size, secs, (double)seq / secs, (unsigned long long)blocked);
  }
}

static void subscribe(void)
{
  wait_for_match();
  size_t size = o.sizes[0];
  if (!wait_data(o.timeout)) {
    fprintf(stderr, "sub: no sample in %.0f s\n", o.timeout);
    exit(1);
  }
  double warm_end = now_s() + o.warmup, end = warm_end + o.duration;
  double second_end = warm_end + 1;
  uint64_t samples_n = 0, octets = 0, first = 0, last = 0, in_second = 0;
  uint64_t min_s = UINT64_MAX, max_s = 0;
  int have_first = 0, seconds = 0;
  void *samples[MAX_TAKE];
  dds_sample_info_t infos[MAX_TAKE];
  for (;;) {
    double t = now_s();
    if (t >= end)
      break;
    if (t >= second_end && t > warm_end) {
      if (in_second < min_s) min_s = in_second;
      if (in_second > max_s) max_s = in_second;
      seconds++;
      in_second = 0;
      second_end += 1;
    }
    double left = end - t;
    wait_data(left < 0.05 ? left : 0.05);
    for (;;) {
      samples[0] = NULL;
      int k = dds_take(reader, samples, infos, MAX_TAKE, MAX_TAKE);
      if (k <= 0)
        break;
      int counting = now_s() >= warm_end;
      for (int j = 0; j < k && counting; j++) {
        if (!infos[j].valid_data)
          continue;
        const RustorbPerf_Sample *s = samples[j];
        samples_n++;
        in_second++;
        octets += s->payload._length;
        if (!have_first) { first = s->seq; have_first = 1; }
        if (s->seq > last) last = s->seq;
      }
      dds_return_loan(reader, samples, k);
      if (k < MAX_TAKE)
        break;
    }
  }
  if (!seconds) min_s = 0;
  uint64_t lost = 0;
  if (have_first && last - first + 1 > samples_n)
    lost = last - first + 1 - samples_n;
  double secs = o.duration, mbit = (double)octets * 8 / secs / 1e6;
  if (o.json) {
    json_head(size);
    printf(",\"samples\":%llu,\"octets\":%llu,\"lost\":%llu,\"seconds\":%.3f,\"samples_per_s\":%.3f,"
           "\"mbit_per_s\":%.3f,\"samples_per_s_min\":%llu,\"samples_per_s_max\":%llu}\n",
           (unsigned long long)samples_n, (unsigned long long)octets, (unsigned long long)lost, secs,
           (double)samples_n / secs, mbit, (unsigned long long)min_s, (unsigned long long)max_s);
  } else {
    printf("sub: %llu samples of %zu octets in %.1f s: %.0f/s, %.1f Mbit/s, %llu lost\n",
           (unsigned long long)samples_n, size, secs, (double)samples_n / secs, mbit, (unsigned long long)lost);
  }
}

int main(int argc, char **argv)
{
  parse(argc, argv);
  setvbuf(stdout, NULL, _IOLBF, 0);
  setup();
  switch (o.mode) {
    case PING: ping(); break;
    case PONG: pong(); break;
    case PUB: publish(); break;
    case SUB: subscribe(); break;
  }
  dds_delete(participant);
  return 0;
}
