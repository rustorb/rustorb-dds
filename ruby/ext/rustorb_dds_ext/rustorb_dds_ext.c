/*
 * rustorb_dds_ext: the native part of the rustorb-dds gem, on the C ABI of
 * librustorb_dds_ffi (rustorb/dds.h). It wraps the handles (types,
 * entities, conditions), the sample streams (rdds_out, rdds_in), the QoS
 * and the listeners for lib/rustorb/dds, which does the rest
 * (docs/ruby.md).
 *
 * Threads, as in rustorb's gem. Ruby code runs only on Ruby threads holding
 * the GVL. Calls that can wait (writes, waits, deleting entities, setting
 * listeners) release the GVL. Listeners are called on rustorb-dds's
 * threads, which cannot take the GVL: they hand the call to a pool of Ruby
 * dispatcher threads (DDS::Native.dispatcher_loop) and wait for it; a
 * listener called inside a call of a Ruby thread (a write to a reader of
 * the same participant) runs on that thread. The pool grows by one
 * whenever a dispatcher takes the last idle slot.
 *
 * Handles the garbage collector frees are released on a thread of this
 * extension, without the GVL, since deleting an entity can wait for a
 * listener that waits for the GVL.
 *
 * Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.
 */

#include <ruby.h>
#include <ruby/encoding.h>
#include <ruby/thread.h>

#include <pthread.h>
#include <stdlib.h>
#include <string.h>

#include <rustorb/dds.h>

static VALUE mDDS, mNative, cHandle, cOut, cIn;
static VALUE errors; /* the exception classes, by return code */
static ID id_dispatch, id_spawn_dispatcher, id_negative_p;

/* --- errors ----------------------------------------------------------------- */

static VALUE error_class(rdds_status st) {
  VALUE e = Qnil;
  if (errors != Qnil && st >= 0 && st < RARRAY_LEN(errors)) e = rb_ary_entry(errors, st);
  return NIL_P(e) ? rb_eRuntimeError : e;
}

static void raise_status(rdds_status st) {
  const char* msg = rdds_last_error_message();
  rb_raise(error_class(st), "%s", msg && *msg ? msg : "rustorb-dds call failed");
}

#define CHECK(call)                       \
  do {                                    \
    rdds_status st_ = (call);             \
    if (st_ != RDDS_RETCODE_OK) raise_status(st_); \
  } while (0)

static void invalid(const char* fmt, VALUE v) {
  rb_raise(error_class(RDDS_RETCODE_BAD_PARAMETER), fmt, rb_obj_classname(v));
}

/* --- the GVL and the dispatchers --------------------------------------------- */

/* Whether this thread is a Ruby thread that released the GVL with
   without_gvl below (then listeners use rb_thread_call_with_gvl). */
static __thread int tls_released;

typedef void* (*gvl_fn)(void*);

struct nogvl_call {
  gvl_fn fn;
  void* arg;
};

static void* nogvl_trampoline(void* p) {
  struct nogvl_call* c = p;
  void* r;
  tls_released = 1;
  r = c->fn(c->arg);
  tls_released = 0;
  return r;
}

static void* without_gvl(gvl_fn fn, void* arg) {
  struct nogvl_call c;
  c.fn = fn;
  c.arg = arg;
  return rb_thread_call_without_gvl(nogvl_trampoline, &c, NULL, NULL);
}

typedef struct job {
  gvl_fn fn;
  void* arg;
  void* result;
  int done;
  pthread_cond_t cond;
  struct job* next;
} job;

static pthread_mutex_t queue_lock = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t queue_cond = PTHREAD_COND_INITIALIZER;
static job *queue_head, *queue_tail;
static int idle_dispatchers, dispatchers, interrupt_dispatchers, shutting_down;

static void* with_gvl_trampoline(void* p) {
  struct nogvl_call* c = p;
  void* r;
  tls_released = 0;
  r = c->fn(c->arg);
  tls_released = 1;
  return r;
}

/* Runs fn(arg) with the GVL, from any thread: on a Ruby thread that released
   it, by taking it back; on another thread, on a dispatcher thread. Returns
   NULL without running fn when that cannot be done (shutting down, or no
   dispatcher). */
static void* with_gvl(gvl_fn fn, void* arg) {
  job j;
  if (tls_released) {
    struct nogvl_call c;
    c.fn = fn;
    c.arg = arg;
    return rb_thread_call_with_gvl(with_gvl_trampoline, &c);
  }
  if (ruby_native_thread_p()) return fn(arg); /* holds the GVL */
  memset(&j, 0, sizeof j);
  j.fn = fn;
  j.arg = arg;
  pthread_cond_init(&j.cond, NULL);
  pthread_mutex_lock(&queue_lock);
  if (shutting_down || dispatchers == 0) {
    pthread_mutex_unlock(&queue_lock);
    pthread_cond_destroy(&j.cond);
    return NULL;
  }
  if (queue_tail)
    queue_tail->next = &j;
  else
    queue_head = &j;
  queue_tail = &j;
  pthread_cond_signal(&queue_cond);
  while (!j.done) pthread_cond_wait(&j.cond, &queue_lock);
  pthread_mutex_unlock(&queue_lock);
  pthread_cond_destroy(&j.cond);
  return j.result;
}

static void* wait_job(void* unused) {
  job* j = NULL;
  (void)unused;
  pthread_mutex_lock(&queue_lock);
  ++idle_dispatchers;
  while (!queue_head && !interrupt_dispatchers && !shutting_down)
    pthread_cond_wait(&queue_cond, &queue_lock);
  if (queue_head) {
    j = queue_head;
    queue_head = j->next;
    if (!queue_head) queue_tail = NULL;
  }
  interrupt_dispatchers = 0;
  --idle_dispatchers;
  pthread_mutex_unlock(&queue_lock);
  return j;
}

static void wake_dispatchers(void* unused) {
  (void)unused;
  pthread_mutex_lock(&queue_lock);
  interrupt_dispatchers = 1;
  pthread_cond_broadcast(&queue_cond);
  pthread_mutex_unlock(&queue_lock);
}

/* DDS::Native.dispatcher_loop: runs listener calls until the process ends. */
static VALUE native_dispatcher_loop(VALUE self) {
  (void)self;
  pthread_mutex_lock(&queue_lock);
  ++dispatchers;
  pthread_mutex_unlock(&queue_lock);
  for (;;) {
    job* j = rb_thread_call_without_gvl(wait_job, NULL, wake_dispatchers, NULL);
    int spawn;
    if (!j) {
      rb_thread_check_ints();
      pthread_mutex_lock(&queue_lock);
      if (shutting_down) {
        --dispatchers;
        pthread_mutex_unlock(&queue_lock);
        return Qnil;
      }
      pthread_mutex_unlock(&queue_lock);
      continue;
    }
    pthread_mutex_lock(&queue_lock);
    spawn = idle_dispatchers == 0;
    pthread_mutex_unlock(&queue_lock);
    if (spawn) rb_funcall(mNative, id_spawn_dispatcher, 0);
    j->result = j->fn(j->arg);
    pthread_mutex_lock(&queue_lock);
    j->done = 1;
    pthread_cond_signal(&j->cond);
    pthread_mutex_unlock(&queue_lock);
  }
  return Qnil;
}

static VALUE native_dispatchers(VALUE self) {
  int n;
  (void)self;
  pthread_mutex_lock(&queue_lock);
  n = dispatchers;
  pthread_mutex_unlock(&queue_lock);
  return INT2NUM(n);
}

static void at_exit_dispatchers(VALUE unused) {
  job* j;
  (void)unused;
  pthread_mutex_lock(&queue_lock);
  shutting_down = 1;
  for (j = queue_head; j; j = j->next) {
    j->result = NULL;
    j->done = 1;
    pthread_cond_signal(&j->cond);
  }
  queue_head = queue_tail = NULL;
  pthread_cond_broadcast(&queue_cond);
  pthread_mutex_unlock(&queue_lock);
}

/* --- calls without the GVL ---------------------------------------------------- */

typedef struct call {
  rdds_status (*f)(struct call*);
  const void *a, *b, *c, *d, *e;
  void* out;
  uint32_t u;
  int32_t i;
  size_t n, cap;
  size_t* len;
  rdds_duration dur;
  rdds_status r;
} call;

static void* run_call(void* p) {
  call* c = p;
  c->r = c->f(c);
  return NULL;
}

/* Runs c->f without the GVL and raises its failure. */
static void nogvl(call* c) {
  without_gvl(run_call, c);
  if (c->r != RDDS_RETCODE_OK) raise_status(c->r);
}

/* --- releasing handles ---------------------------------------------------------- */

enum kind {
  K_TYPE,
  K_PARTICIPANT,
  K_TOPIC,
  K_CFTOPIC,
  K_PUBLISHER,
  K_SUBSCRIBER,
  K_WRITER,
  K_READER,
  K_CONDITION,
  K_WAITSET,
  K_COUNT
};

static const char* const kind_names[K_COUNT] = {"type",   "participant", "topic",     "content filtered topic",
                                                "publisher", "subscriber", "writer", "reader",
                                                "condition", "waitset"};

static void rel_type(void* p) { rdds_type_release(p); }
static void rel_participant(void* p) { rdds_participant_release(p); }
static void rel_topic(void* p) { rdds_topic_release(p); }
static void rel_cftopic(void* p) { rdds_cftopic_release(p); }
static void rel_publisher(void* p) { rdds_publisher_release(p); }
static void rel_subscriber(void* p) { rdds_subscriber_release(p); }
static void rel_writer(void* p) { rdds_writer_release(p); }
static void rel_reader(void* p) { rdds_reader_release(p); }
static void rel_condition(void* p) { rdds_condition_release(p); }
static void rel_waitset(void* p) { rdds_waitset_release(p); }

static void (*const releases[K_COUNT])(void*) = {rel_type,      rel_participant, rel_topic,  rel_cftopic,
                                                 rel_publisher, rel_subscriber,  rel_writer, rel_reader,
                                                 rel_condition, rel_waitset};

/* The reaper: a thread that releases what the garbage collector freed. */
typedef struct reap {
  void (*release)(void*);
  void* p;
  struct reap* next;
} reap;

static pthread_mutex_t reap_lock = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t reap_cond = PTHREAD_COND_INITIALIZER;
static reap* reap_head;
static int reaper_started;

static void* reaper(void* unused) {
  (void)unused;
  for (;;) {
    reap* r;
    pthread_mutex_lock(&reap_lock);
    while (!reap_head) pthread_cond_wait(&reap_cond, &reap_lock);
    r = reap_head;
    reap_head = r->next;
    pthread_mutex_unlock(&reap_lock);
    r->release(r->p);
    free(r);
  }
  return NULL;
}

static void reap_later(void (*release)(void*), void* p) {
  reap* r = malloc(sizeof *r);
  if (!r) return; /* leaks the handle */
  r->release = release;
  r->p = p;
  pthread_mutex_lock(&reap_lock);
  if (!reaper_started) {
    pthread_t t;
    pthread_attr_t attr;
    pthread_attr_init(&attr);
    pthread_attr_setdetachstate(&attr, PTHREAD_CREATE_DETACHED);
    if (pthread_create(&t, &attr, reaper, NULL) == 0) reaper_started = 1;
    pthread_attr_destroy(&attr);
    if (!reaper_started) {
      pthread_mutex_unlock(&reap_lock);
      free(r);
      return;
    }
  }
  r->next = reap_head;
  reap_head = r;
  pthread_cond_signal(&reap_cond);
  pthread_mutex_unlock(&reap_lock);
}

/* --- handles ---------------------------------------------------------------- */

typedef struct handle {
  void* p;
  int kind;
} handle;

static void handle_free(void* ptr) {
  handle* h = ptr;
  if (h->p) {
    if (h->kind == K_TYPE)
      releases[h->kind](h->p);
    else
      reap_later(releases[h->kind], h->p);
  }
  xfree(h);
}

static size_t handle_size(const void* ptr) {
  (void)ptr;
  return sizeof(handle);
}

static const rb_data_type_t handle_type = {
    "rustorb_dds_handle", {NULL, handle_free, handle_size, NULL, {0}}, NULL, NULL, RUBY_TYPED_FREE_IMMEDIATELY};

static VALUE wrap(int kind, void* p) {
  handle* h;
  VALUE v = TypedData_Make_Struct(cHandle, handle, &handle_type, h);
  h->p = p;
  h->kind = kind;
  return v;
}

static void* unwrap(VALUE v, int kind) {
  handle* h = rb_check_typeddata(v, &handle_type);
  if (h->kind != kind)
    rb_raise(rb_eTypeError, "a %s expected, got a %s", kind_names[kind], kind_names[h->kind]);
  if (!h->p) rb_raise(error_class(RDDS_RETCODE_ALREADY_DELETED), "the %s is closed", kind_names[kind]);
  return h->p;
}

static int handle_kind(VALUE v) {
  handle* h = rb_check_typeddata(v, &handle_type);
  return h->kind;
}

static rdds_status op_release(call* c) {
  releases[c->i]((void*)c->a);
  return RDDS_RETCODE_OK;
}

/* DDS::Native.release(handle): releases it now (deletes the entity). */
static VALUE native_release(VALUE self, VALUE v) {
  handle* h = rb_check_typeddata(v, &handle_type);
  void* p = h->p;
  call c;
  (void)self;
  if (!p) return Qnil;
  h->p = NULL;
  memset(&c, 0, sizeof c);
  c.f = op_release;
  c.a = p;
  c.i = h->kind;
  nogvl(&c);
  return Qnil;
}

static VALUE handle_closed_p(VALUE self) {
  handle* h = rb_check_typeddata(self, &handle_type);
  return h->p ? Qfalse : Qtrue;
}

/* --- values ------------------------------------------------------------------- */

static void out_of_range(VALUE v) {
  rb_raise(error_class(RDDS_RETCODE_BAD_PARAMETER), "%" PRIsVALUE " is out of range", v);
}

static long long int_in(VALUE v, long long lo, long long hi) {
  long long x;
  if (!RB_INTEGER_TYPE_P(v)) invalid("an Integer expected, got %s", v);
  if (!FIXNUM_P(v) &&
      (rb_big_cmp(v, LL2NUM(lo)) == INT2FIX(-1) || rb_big_cmp(v, LL2NUM(hi)) == INT2FIX(1)))
    out_of_range(v);
  x = NUM2LL(v);
  if (x < lo || x > hi) out_of_range(v);
  return x;
}

static uint64_t uint64_in(VALUE v) {
  if (!RB_INTEGER_TYPE_P(v)) invalid("an Integer expected, got %s", v);
  if (RTEST(rb_funcall(v, id_negative_p, 0))) out_of_range(v);
  if (!FIXNUM_P(v) && rb_big_cmp(v, rb_ull2inum(UINT64_MAX)) == INT2FIX(1)) out_of_range(v);
  return NUM2ULL(v);
}

static double float_in(VALUE v) {
  if (!RB_FLOAT_TYPE_P(v) && !RB_INTEGER_TYPE_P(v) && !rb_obj_is_kind_of(v, rb_cNumeric))
    invalid("a Numeric expected, got %s", v);
  return NUM2DBL(v);
}

static bool bool_in(VALUE v) {
  if (v == Qtrue) return true;
  if (v == Qfalse) return false;
  invalid("true or false expected, got %s", v);
  return false;
}

/* A character: an Integer, or a String of one character, up to max. */
static uint32_t char_in(VALUE v, uint32_t max) {
  long long code;
  if (RB_TYPE_P(v, T_STRING)) {
    if (rb_str_strlen(v) != 1) rb_raise(error_class(RDDS_RETCODE_BAD_PARAMETER), "one character expected");
    code = NUM2LL(rb_funcall(v, rb_intern("ord"), 0));
  } else {
    code = int_in(v, 0, max);
  }
  if (code < 0 || code > (long long)max)
    rb_raise(error_class(RDDS_RETCODE_BAD_PARAMETER), "the character %lld does not fit", code);
  return (uint32_t)code;
}

static const char* cstr(VALUE v) {
  StringValue(v);
  return StringValueCStr(v);
}

static rdds_duration duration_in(VALUE v) {
  rdds_duration d;
  Check_Type(v, T_ARRAY);
  if (RARRAY_LEN(v) != 2) rb_raise(rb_eArgError, "a duration is [sec, nanosec]");
  d.sec = (int32_t)int_in(rb_ary_entry(v, 0), INT32_MIN, INT32_MAX);
  d.nanosec = (uint32_t)int_in(rb_ary_entry(v, 1), 0, UINT32_MAX);
  return d;
}

static VALUE duration_out(rdds_duration d) { return rb_ary_new_from_args(2, INT2NUM(d.sec), UINT2NUM(d.nanosec)); }

static VALUE bytes16(const uint8_t* p) { return rb_str_new((const char*)p, 16); }

static void bytes16_in(VALUE v, uint8_t* out) {
  StringValue(v);
  if (RSTRING_LEN(v) != 16) rb_raise(rb_eArgError, "16 octets expected");
  memcpy(out, RSTRING_PTR(v), 16);
}

/* --- types ------------------------------------------------------------------ */

static int ext_in(VALUE v) {
  ID id = SYM2ID(v);
  if (id == rb_intern("final")) return RDDS_FINAL;
  if (id == rb_intern("appendable")) return RDDS_APPENDABLE;
  if (id == rb_intern("mutable")) return RDDS_MUTABLE;
  rb_raise(rb_eArgError, "unknown extensibility %" PRIsVALUE, v);
  return 0;
}

static VALUE native_type_primitive(VALUE self, VALUE kind) {
  rdds_type* t;
  (void)self;
  CHECK(rdds_type_primitive((uint8_t)NUM2UINT(kind), &t));
  return wrap(K_TYPE, t);
}

static VALUE native_type_string(VALUE self, VALUE wide, VALUE bound) {
  rdds_type* t;
  (void)self;
  CHECK(rdds_type_string(RTEST(wide), NUM2UINT(bound), &t));
  return wrap(K_TYPE, t);
}

static VALUE native_type_sequence(VALUE self, VALUE element, VALUE bound) {
  rdds_type* t;
  (void)self;
  CHECK(rdds_type_sequence(unwrap(element, K_TYPE), NUM2UINT(bound), &t));
  return wrap(K_TYPE, t);
}

static VALUE native_type_array(VALUE self, VALUE element, VALUE dims) {
  rdds_type* t;
  uint32_t d[16];
  long i, n;
  (void)self;
  Check_Type(dims, T_ARRAY);
  n = RARRAY_LEN(dims);
  if (n < 1 || n > 16) rb_raise(rb_eArgError, "1 to 16 dimensions");
  for (i = 0; i < n; i++) d[i] = NUM2UINT(rb_ary_entry(dims, i));
  CHECK(rdds_type_array(unwrap(element, K_TYPE), d, (size_t)n, &t));
  return wrap(K_TYPE, t);
}

static VALUE native_type_alias(VALUE self, VALUE name, VALUE related) {
  rdds_type* t;
  (void)self;
  CHECK(rdds_type_alias(cstr(name), unwrap(related, K_TYPE), &t));
  return wrap(K_TYPE, t);
}

/* literals: [[name, value, default?], ...] */
static VALUE native_type_enum(VALUE self, VALUE name, VALUE bits, VALUE ext, VALUE literals) {
  rdds_enum_builder* b;
  rdds_type* t;
  rdds_status st = RDDS_RETCODE_OK;
  long i;
  (void)self;
  Check_Type(literals, T_ARRAY);
  for (i = 0; i < RARRAY_LEN(literals); i++) Check_Type(rb_ary_entry(rb_ary_entry(literals, i), 0), T_STRING);
  CHECK(rdds_enum_builder_new(cstr(name), (uint16_t)NUM2UINT(bits), ext_in(ext), &b));
  for (i = 0; st == RDDS_RETCODE_OK && i < RARRAY_LEN(literals); i++) {
    VALUE l = rb_ary_entry(literals, i);
    st = rdds_enum_builder_add_literal(b, RSTRING_PTR(rb_ary_entry(l, 0)), NUM2INT(rb_ary_entry(l, 1)),
                                       RTEST(rb_ary_entry(l, 2)));
  }
  if (st == RDDS_RETCODE_OK) st = rdds_enum_builder_build(b, &t);
  rdds_enum_builder_release(b);
  if (st != RDDS_RETCODE_OK) raise_status(st);
  return wrap(K_TYPE, t);
}

/* flags: [[name, position], ...] */
static VALUE native_type_bitmask(VALUE self, VALUE name, VALUE bits, VALUE ext, VALUE flags) {
  rdds_bitmask_builder* b;
  rdds_type* t;
  rdds_status st = RDDS_RETCODE_OK;
  long i;
  (void)self;
  Check_Type(flags, T_ARRAY);
  for (i = 0; i < RARRAY_LEN(flags); i++) Check_Type(rb_ary_entry(rb_ary_entry(flags, i), 0), T_STRING);
  CHECK(rdds_bitmask_builder_new(cstr(name), (uint16_t)NUM2UINT(bits), ext_in(ext), &b));
  for (i = 0; st == RDDS_RETCODE_OK && i < RARRAY_LEN(flags); i++) {
    VALUE f = rb_ary_entry(flags, i);
    st = rdds_bitmask_builder_add_flag(b, RSTRING_PTR(rb_ary_entry(f, 0)), (uint16_t)NUM2UINT(rb_ary_entry(f, 1)));
  }
  if (st == RDDS_RETCODE_OK) st = rdds_bitmask_builder_build(b, &t);
  rdds_bitmask_builder_release(b);
  if (st != RDDS_RETCODE_OK) raise_status(st);
  return wrap(K_TYPE, t);
}

static const char* hash_id_in(VALUE v) { return NIL_P(v) ? NULL : RSTRING_PTR(v); }

/* members: [[name, id, type handle, flags, hash_id or nil], ...] */
static VALUE native_type_struct(VALUE self, VALUE name, VALUE ext, VALUE flags, VALUE members) {
  rdds_struct_builder* b;
  rdds_type* t;
  rdds_status st = RDDS_RETCODE_OK;
  long i;
  (void)self;
  Check_Type(members, T_ARRAY);
  for (i = 0; i < RARRAY_LEN(members); i++) {
    VALUE m = rb_ary_entry(members, i);
    Check_Type(m, T_ARRAY);
    Check_Type(rb_ary_entry(m, 0), T_STRING);
    unwrap(rb_ary_entry(m, 2), K_TYPE);
    if (!NIL_P(rb_ary_entry(m, 4))) Check_Type(rb_ary_entry(m, 4), T_STRING);
  }
  CHECK(rdds_struct_builder_new(cstr(name), ext_in(ext), NUM2UINT(flags), NULL, &b));
  for (i = 0; st == RDDS_RETCODE_OK && i < RARRAY_LEN(members); i++) {
    VALUE m = rb_ary_entry(members, i);
    st = rdds_struct_builder_add_member(b, RSTRING_PTR(rb_ary_entry(m, 0)), NUM2UINT(rb_ary_entry(m, 1)),
                                        unwrap(rb_ary_entry(m, 2), K_TYPE), NUM2UINT(rb_ary_entry(m, 3)),
                                        hash_id_in(rb_ary_entry(m, 4)));
  }
  if (st == RDDS_RETCODE_OK) st = rdds_struct_builder_build(b, &t);
  rdds_struct_builder_release(b);
  if (st != RDDS_RETCODE_OK) raise_status(st);
  return wrap(K_TYPE, t);
}

/* members: [[name, id, type handle, labels, flags, hash_id or nil], ...] */
static VALUE native_type_union(VALUE self, VALUE name, VALUE ext, VALUE flags, VALUE disc, VALUE members) {
  rdds_union_builder* b;
  rdds_type* t;
  rdds_status st = RDDS_RETCODE_OK;
  long i, j;
  (void)self;
  Check_Type(members, T_ARRAY);
  for (i = 0; i < RARRAY_LEN(members); i++) {
    VALUE m = rb_ary_entry(members, i);
    Check_Type(m, T_ARRAY);
    Check_Type(rb_ary_entry(m, 0), T_STRING);
    unwrap(rb_ary_entry(m, 2), K_TYPE);
    Check_Type(rb_ary_entry(m, 3), T_ARRAY);
    for (j = 0; j < RARRAY_LEN(rb_ary_entry(m, 3)); j++) NUM2INT(rb_ary_entry(rb_ary_entry(m, 3), j));
    if (!NIL_P(rb_ary_entry(m, 5))) Check_Type(rb_ary_entry(m, 5), T_STRING);
  }
  CHECK(rdds_union_builder_new(cstr(name), ext_in(ext), NUM2UINT(flags), unwrap(disc, K_TYPE), 0, &b));
  for (i = 0; st == RDDS_RETCODE_OK && i < RARRAY_LEN(members); i++) {
    VALUE m = rb_ary_entry(members, i);
    VALUE ls = rb_ary_entry(m, 3);
    long n = RARRAY_LEN(ls);
    int32_t* labels = ALLOC_N(int32_t, n > 0 ? n : 1);
    for (j = 0; j < n; j++) labels[j] = NUM2INT(rb_ary_entry(ls, j));
    st = rdds_union_builder_add_member(b, RSTRING_PTR(rb_ary_entry(m, 0)), NUM2UINT(rb_ary_entry(m, 1)),
                                       unwrap(rb_ary_entry(m, 2), K_TYPE), labels, (size_t)n,
                                       NUM2UINT(rb_ary_entry(m, 4)), hash_id_in(rb_ary_entry(m, 5)));
    xfree(labels);
  }
  if (st == RDDS_RETCODE_OK) st = rdds_union_builder_build(b, &t);
  rdds_union_builder_release(b);
  if (st != RDDS_RETCODE_OK) raise_status(st);
  return wrap(K_TYPE, t);
}

/* [minimal, complete]: the 14-octet hashes. */
static VALUE native_type_hashes(VALUE self, VALUE type) {
  uint8_t m[14], c[14];
  (void)self;
  CHECK(rdds_type_hashes(unwrap(type, K_TYPE), m, c));
  return rb_ary_new_from_args(2, rb_str_new((const char*)m, 14), rb_str_new((const char*)c, 14));
}

/* --- sample streams ----------------------------------------------------------- */

static void out_free(void* p) {
  if (p) rdds_out_release(p);
}

static size_t ptr_size(const void* p) {
  (void)p;
  return sizeof(void*);
}

static const rb_data_type_t out_type = {"rustorb_dds_out", {NULL, out_free, ptr_size, NULL, {0}}, NULL, NULL,
                                        RUBY_TYPED_FREE_IMMEDIATELY};

static void in_free(void* p) {
  if (p) rdds_in_release(p);
}

static const rb_data_type_t in_type = {"rustorb_dds_in", {NULL, in_free, ptr_size, NULL, {0}}, NULL, NULL,
                                       RUBY_TYPED_FREE_IMMEDIATELY};

static VALUE out_alloc(VALUE klass) { return TypedData_Wrap_Struct(klass, &out_type, NULL); }

static VALUE out_initialize(VALUE self) {
  rdds_out* o;
  CHECK(rdds_out_new(&o));
  DATA_PTR(self) = o;
  return self;
}

static rdds_out* out_of(VALUE v) {
  rdds_out* o = rb_check_typeddata(v, &out_type);
  if (!o) rb_raise(rb_eRuntimeError, "uninitialized DDS::Native::Out");
  return o;
}

static VALUE wrap_in(rdds_in* in) { return TypedData_Wrap_Struct(cIn, &in_type, in); }

static rdds_in* in_of(VALUE v) {
  rdds_in* i = rb_check_typeddata(v, &in_type);
  if (!i) rb_raise(rb_eRuntimeError, "a released DDS::Native::In");
  return i;
}

static VALUE out_clear(VALUE self) {
  rdds_out_clear(out_of(self));
  return self;
}

#define OUT_INT(name, ctype, lo, hi)                              \
  static VALUE out_##name(VALUE self, VALUE v) {                  \
    rdds_out_##name(out_of(self), (ctype)int_in(v, lo, hi));      \
    return self;                                                  \
  }

OUT_INT(octet, uint8_t, 0, UINT8_MAX)
OUT_INT(int8, int8_t, INT8_MIN, INT8_MAX)
OUT_INT(uint8, uint8_t, 0, UINT8_MAX)
OUT_INT(int16, int16_t, INT16_MIN, INT16_MAX)
OUT_INT(uint16, uint16_t, 0, UINT16_MAX)
OUT_INT(int32, int32_t, INT32_MIN, INT32_MAX)
OUT_INT(uint32, uint32_t, 0, UINT32_MAX)
OUT_INT(int64, int64_t, INT64_MIN, INT64_MAX)
OUT_INT(enum, int32_t, INT32_MIN, INT32_MAX)
OUT_INT(discriminator, int64_t, INT64_MIN, INT64_MAX)

static VALUE out_uint64(VALUE self, VALUE v) {
  rdds_out_uint64(out_of(self), uint64_in(v));
  return self;
}

static VALUE out_bitmask(VALUE self, VALUE v) {
  rdds_out_bitmask(out_of(self), uint64_in(v));
  return self;
}

static VALUE out_boolean(VALUE self, VALUE v) {
  rdds_out_boolean(out_of(self), bool_in(v));
  return self;
}

static VALUE out_present(VALUE self, VALUE v) {
  rdds_out_present(out_of(self), RTEST(v));
  return self;
}

static VALUE out_float32(VALUE self, VALUE v) {
  rdds_out_float32(out_of(self), (float)float_in(v));
  return self;
}

static VALUE out_float64(VALUE self, VALUE v) {
  rdds_out_float64(out_of(self), float_in(v));
  return self;
}

static VALUE out_float128(VALUE self, VALUE hi, VALUE lo) {
  rdds_out_float128(out_of(self), uint64_in(hi), uint64_in(lo));
  return self;
}

static VALUE out_char8(VALUE self, VALUE v) {
  rdds_out_char8(out_of(self), (char)(unsigned char)char_in(v, 0xFF));
  return self;
}

static VALUE out_char16(VALUE self, VALUE v) {
  rdds_out_char16(out_of(self), (uint16_t)char_in(v, 0xFFFF));
  return self;
}

static VALUE out_string(VALUE self, VALUE v) {
  if (!RB_TYPE_P(v, T_STRING)) invalid("a String expected, got %s", v);
  if (ENCODING_GET(v) != rb_utf8_encindex() && ENCODING_GET(v) != rb_usascii_encindex() &&
      ENCODING_GET(v) != rb_ascii8bit_encindex())
    v = rb_str_export_to_enc(v, rb_utf8_encoding());
  if (memchr(RSTRING_PTR(v), 0, (size_t)RSTRING_LEN(v)))
    rb_raise(error_class(RDDS_RETCODE_BAD_PARAMETER), "a string with a NUL character");
  rdds_out_string(out_of(self), RSTRING_PTR(v), (size_t)RSTRING_LEN(v));
  RB_GC_GUARD(v);
  return self;
}

/* A wide string: an Array of UTF-16 code units. */
static VALUE out_wstring(VALUE self, VALUE v) {
  long i, n;
  uint16_t* u;
  if (!RB_TYPE_P(v, T_ARRAY)) invalid("an Array of UTF-16 code units expected, got %s", v);
  n = RARRAY_LEN(v);
  u = ALLOC_N(uint16_t, n > 0 ? n : 1);
  for (i = 0; i < n; i++) {
    VALUE e = rb_ary_entry(v, i);
    if (!RB_INTEGER_TYPE_P(e) || NUM2LL(e) < 0 || NUM2LL(e) > 0xFFFF) {
      xfree(u);
      rb_raise(error_class(RDDS_RETCODE_BAD_PARAMETER), "a UTF-16 code unit expected");
    }
    u[i] = (uint16_t)NUM2UINT(e);
  }
  rdds_out_wstring(out_of(self), u, (size_t)n);
  xfree(u);
  return self;
}

static VALUE out_length(VALUE self, VALUE v) {
  rdds_out_length(out_of(self), (uint32_t)int_in(v, 0, UINT32_MAX));
  return self;
}

/* Out#array(kind, values): the elements of a sequence or array of a
   primitive type (RDDS_TK_* but float128), from an Array (or, for octets,
   a String). */
static VALUE out_array(VALUE self, VALUE kind, VALUE v) {
  rdds_out* o = out_of(self);
  int k = NUM2INT(kind);
  long i, n;
  if (k == RDDS_TK_BYTE && RB_TYPE_P(v, T_STRING)) {
    rdds_out_octet_array(o, (const uint8_t*)RSTRING_PTR(v), (size_t)RSTRING_LEN(v));
    return self;
  }
  if (!RB_TYPE_P(v, T_ARRAY)) invalid("an Array expected, got %s", v);
  n = RARRAY_LEN(v);
  {
    size_t size;
    VALUE buf;
    char* p;
    switch (k) {
      case RDDS_TK_BOOLEAN: size = sizeof(bool); break;
      case RDDS_TK_BYTE: case RDDS_TK_INT8: case RDDS_TK_UINT8: case RDDS_TK_CHAR8: size = 1; break;
      case RDDS_TK_INT16: case RDDS_TK_UINT16: case RDDS_TK_CHAR16: size = 2; break;
      case RDDS_TK_INT32: case RDDS_TK_UINT32: case RDDS_TK_FLOAT32: size = 4; break;
      case RDDS_TK_INT64: case RDDS_TK_UINT64: case RDDS_TK_FLOAT64: size = 8; break;
      default: rb_raise(rb_eArgError, "no arrays of the kind %d", k);
    }
    buf = rb_str_buf_new((long)size * (n > 0 ? n : 1));
    p = RSTRING_PTR(buf);
    for (i = 0; i < n; i++) {
      VALUE e = rb_ary_entry(v, i);
      void* at = p + (size_t)i * size;
      switch (k) {
        case RDDS_TK_BOOLEAN: *(bool*)at = bool_in(e); break;
        case RDDS_TK_BYTE: case RDDS_TK_UINT8: *(uint8_t*)at = (uint8_t)int_in(e, 0, UINT8_MAX); break;
        case RDDS_TK_INT8: *(int8_t*)at = (int8_t)int_in(e, INT8_MIN, INT8_MAX); break;
        case RDDS_TK_CHAR8: *(char*)at = (char)(unsigned char)char_in(e, 0xFF); break;
        case RDDS_TK_INT16: *(int16_t*)at = (int16_t)int_in(e, INT16_MIN, INT16_MAX); break;
        case RDDS_TK_UINT16: *(uint16_t*)at = (uint16_t)int_in(e, 0, UINT16_MAX); break;
        case RDDS_TK_CHAR16: *(uint16_t*)at = (uint16_t)char_in(e, 0xFFFF); break;
        case RDDS_TK_INT32: *(int32_t*)at = (int32_t)int_in(e, INT32_MIN, INT32_MAX); break;
        case RDDS_TK_UINT32: *(uint32_t*)at = (uint32_t)int_in(e, 0, UINT32_MAX); break;
        case RDDS_TK_FLOAT32: *(float*)at = (float)float_in(e); break;
        case RDDS_TK_INT64: *(int64_t*)at = (int64_t)int_in(e, INT64_MIN, INT64_MAX); break;
        case RDDS_TK_UINT64: *(uint64_t*)at = uint64_in(e); break;
        case RDDS_TK_FLOAT64: *(double*)at = float_in(e); break;
      }
      p = RSTRING_PTR(buf); /* the conversions may call Ruby */
    }
    switch (k) {
      case RDDS_TK_BOOLEAN: rdds_out_boolean_array(o, (const bool*)p, (size_t)n); break;
      case RDDS_TK_BYTE: rdds_out_octet_array(o, (const uint8_t*)p, (size_t)n); break;
      case RDDS_TK_UINT8: rdds_out_uint8_array(o, (const uint8_t*)p, (size_t)n); break;
      case RDDS_TK_INT8: rdds_out_int8_array(o, (const int8_t*)p, (size_t)n); break;
      case RDDS_TK_CHAR8: rdds_out_char8_array(o, (const char*)p, (size_t)n); break;
      case RDDS_TK_INT16: rdds_out_int16_array(o, (const int16_t*)p, (size_t)n); break;
      case RDDS_TK_UINT16: rdds_out_uint16_array(o, (const uint16_t*)p, (size_t)n); break;
      case RDDS_TK_CHAR16: rdds_out_char16_array(o, (const uint16_t*)p, (size_t)n); break;
      case RDDS_TK_INT32: rdds_out_int32_array(o, (const int32_t*)p, (size_t)n); break;
      case RDDS_TK_UINT32: rdds_out_uint32_array(o, (const uint32_t*)p, (size_t)n); break;
      case RDDS_TK_FLOAT32: rdds_out_float32_array(o, (const float*)p, (size_t)n); break;
      case RDDS_TK_INT64: rdds_out_int64_array(o, (const int64_t*)p, (size_t)n); break;
      case RDDS_TK_UINT64: rdds_out_uint64_array(o, (const uint64_t*)p, (size_t)n); break;
      case RDDS_TK_FLOAT64: rdds_out_float64_array(o, (const double*)p, (size_t)n); break;
    }
    RB_GC_GUARD(buf);
  }
  return self;
}

/* In: the readers raise nothing; In#check raises the stream's failure. */
static VALUE in_check(VALUE self) {
  rdds_status st = rdds_in_status(in_of(self));
  if (st != RDDS_RETCODE_OK) raise_status(st);
  return self;
}

static VALUE in_at_end_p(VALUE self) { return rdds_in_at_end(in_of(self)) ? Qtrue : Qfalse; }

static VALUE in_release(VALUE self) {
  rdds_in* i = rb_check_typeddata(self, &in_type);
  if (i) {
    DATA_PTR(self) = NULL;
    rdds_in_release(i);
  }
  return Qnil;
}

static VALUE in_boolean(VALUE self) { return rdds_in_boolean(in_of(self)) ? Qtrue : Qfalse; }
static VALUE in_present(VALUE self) { return rdds_in_present(in_of(self)) ? Qtrue : Qfalse; }
static VALUE in_octet(VALUE self) { return UINT2NUM(rdds_in_octet(in_of(self))); }
static VALUE in_int8(VALUE self) { return INT2NUM(rdds_in_int8(in_of(self))); }
static VALUE in_uint8(VALUE self) { return UINT2NUM(rdds_in_uint8(in_of(self))); }
static VALUE in_int16(VALUE self) { return INT2NUM(rdds_in_int16(in_of(self))); }
static VALUE in_uint16(VALUE self) { return UINT2NUM(rdds_in_uint16(in_of(self))); }
static VALUE in_int32(VALUE self) { return INT2NUM(rdds_in_int32(in_of(self))); }
static VALUE in_uint32(VALUE self) { return UINT2NUM(rdds_in_uint32(in_of(self))); }
static VALUE in_int64(VALUE self) { return LL2NUM(rdds_in_int64(in_of(self))); }
static VALUE in_uint64(VALUE self) { return ULL2NUM(rdds_in_uint64(in_of(self))); }
static VALUE in_float32(VALUE self) { return DBL2NUM(rdds_in_float32(in_of(self))); }
static VALUE in_float64(VALUE self) { return DBL2NUM(rdds_in_float64(in_of(self))); }
static VALUE in_char8(VALUE self) { return UINT2NUM((unsigned char)rdds_in_char8(in_of(self))); }
static VALUE in_char16(VALUE self) { return UINT2NUM(rdds_in_char16(in_of(self))); }
static VALUE in_length(VALUE self) { return UINT2NUM(rdds_in_length(in_of(self))); }
static VALUE in_discriminator(VALUE self) { return LL2NUM(rdds_in_discriminator(in_of(self))); }
static VALUE in_enum(VALUE self) { return INT2NUM(rdds_in_enum(in_of(self))); }
static VALUE in_bitmask(VALUE self) { return ULL2NUM(rdds_in_bitmask(in_of(self))); }

static VALUE in_float128(VALUE self) {
  uint64_t hi = 0, lo = 0;
  rdds_in_float128(in_of(self), &hi, &lo);
  return rb_ary_new_from_args(2, ULL2NUM(hi), ULL2NUM(lo));
}

static VALUE in_string(VALUE self) {
  const char* p = NULL;
  size_t n = 0;
  rdds_in_string(in_of(self), &p, &n);
  return rb_utf8_str_new(p ? p : "", (long)n);
}

static VALUE in_wstring(VALUE self) {
  const uint16_t* p = NULL;
  size_t n = 0, i;
  VALUE a;
  rdds_in_wstring(in_of(self), &p, &n);
  a = rb_ary_new_capa((long)n);
  for (i = 0; i < n; i++) rb_ary_push(a, UINT2NUM(p[i]));
  return a;
}

/* In#array(kind, n): n elements of a primitive kind, as an Array. */
static VALUE in_array(VALUE self, VALUE kind, VALUE count) {
  rdds_in* in = in_of(self);
  int k = NUM2INT(kind);
  long n = NUM2LONG(count), i;
  VALUE a;
  if (n < 0) rb_raise(rb_eArgError, "a negative count");
  a = rb_ary_new_capa(n);
#define READ_ARR(ctype, fn, conv)                    \
  do {                                               \
    VALUE buf = rb_str_buf_new((long)sizeof(ctype) * (n > 0 ? n : 1)); \
    ctype* p = (ctype*)RSTRING_PTR(buf);             \
    fn(in, p, (size_t)n);                            \
    for (i = 0; i < n; i++) rb_ary_push(a, conv(p[i])); \
    RB_GC_GUARD(buf);                                \
  } while (0)
#define BOOLV(x) ((x) ? Qtrue : Qfalse)
#define UCHARV(x) UINT2NUM((unsigned char)(x))
  switch (k) {
    case RDDS_TK_BOOLEAN: READ_ARR(bool, rdds_in_boolean_array, BOOLV); break;
    case RDDS_TK_BYTE: READ_ARR(uint8_t, rdds_in_octet_array, UINT2NUM); break;
    case RDDS_TK_UINT8: READ_ARR(uint8_t, rdds_in_uint8_array, UINT2NUM); break;
    case RDDS_TK_INT8: READ_ARR(int8_t, rdds_in_int8_array, INT2NUM); break;
    case RDDS_TK_CHAR8: READ_ARR(char, rdds_in_char8_array, UCHARV); break;
    case RDDS_TK_INT16: READ_ARR(int16_t, rdds_in_int16_array, INT2NUM); break;
    case RDDS_TK_UINT16: READ_ARR(uint16_t, rdds_in_uint16_array, UINT2NUM); break;
    case RDDS_TK_CHAR16: READ_ARR(uint16_t, rdds_in_char16_array, UINT2NUM); break;
    case RDDS_TK_INT32: READ_ARR(int32_t, rdds_in_int32_array, INT2NUM); break;
    case RDDS_TK_UINT32: READ_ARR(uint32_t, rdds_in_uint32_array, UINT2NUM); break;
    case RDDS_TK_FLOAT32: READ_ARR(float, rdds_in_float32_array, DBL2NUM); break;
    case RDDS_TK_INT64: READ_ARR(int64_t, rdds_in_int64_array, LL2NUM); break;
    case RDDS_TK_UINT64: READ_ARR(uint64_t, rdds_in_uint64_array, ULL2NUM); break;
    case RDDS_TK_FLOAT64: READ_ARR(double, rdds_in_float64_array, DBL2NUM); break;
    default: rb_raise(rb_eArgError, "no arrays of the kind %d", k);
  }
#undef READ_ARR
#undef BOOLV
#undef UCHARV
  return a;
}

/* DDS::Native.serialize(type, out, xcdr2): the payload, with its
   encapsulation header. */
static VALUE native_serialize(VALUE self, VALUE type, VALUE out, VALUE xcdr2) {
  uint8_t* data = NULL;
  size_t len = 0;
  VALUE s;
  (void)self;
  CHECK(rdds_type_serialize(unwrap(type, K_TYPE), out_of(out), RTEST(xcdr2), &data, &len));
  s = rb_str_new((const char*)data, (long)len);
  rdds_bytes_free(data, len);
  return s;
}

static VALUE native_deserialize(VALUE self, VALUE type, VALUE data) {
  rdds_in* in;
  (void)self;
  StringValue(data);
  CHECK(rdds_type_deserialize(unwrap(type, K_TYPE), (const uint8_t*)RSTRING_PTR(data), (size_t)RSTRING_LEN(data),
                              &in));
  return wrap_in(in);
}

/* --- QoS ------------------------------------------------------------------------ */

static VALUE qget(VALUE h, const char* key) {
  VALUE v = rb_hash_lookup2(h, ID2SYM(rb_intern(key)), Qundef);
  if (v == Qundef) rb_raise(rb_eArgError, "the QoS has no %s", key);
  return v;
}

#define QI(q, h, f) (q)->f = (int32_t)int_in(qget(h, #f), INT32_MIN, INT32_MAX)
#define QD(q, h, f) (q)->f = duration_in(qget(h, #f))
#define QB(q, h, f) (q)->f = RTEST(qget(h, #f))

/* The arrays of a QoS, kept alive while it is used. */
typedef struct qos_arrays {
  VALUE user_data;
  VALUE repr;
} qos_arrays;

static void common_qos_in(VALUE h, rdds_writer_qos* q, qos_arrays* keep) {
  VALUE ud, dr;
  long i, n;
  int16_t* reps;
  QI(q, h, reliability_kind);
  QD(q, h, max_blocking_time);
  QI(q, h, history_kind);
  QI(q, h, history_depth);
  QI(q, h, max_samples);
  QI(q, h, max_instances);
  QI(q, h, max_samples_per_instance);
  QI(q, h, durability_kind);
  QD(q, h, deadline_period);
  QI(q, h, liveliness_kind);
  QD(q, h, lease_duration);
  ud = qget(h, "user_data");
  StringValue(ud);
  keep->user_data = ud;
  q->user_data = (const uint8_t*)RSTRING_PTR(ud);
  q->user_data_len = (size_t)RSTRING_LEN(ud);
  dr = qget(h, "data_representation");
  Check_Type(dr, T_ARRAY);
  n = RARRAY_LEN(dr);
  keep->repr = rb_str_buf_new((long)sizeof(int16_t) * (n > 0 ? n : 1));
  reps = (int16_t*)RSTRING_PTR(keep->repr);
  for (i = 0; i < n; i++) reps[i] = (int16_t)int_in(rb_ary_entry(dr, i), INT16_MIN, INT16_MAX);
  reps = (int16_t*)RSTRING_PTR(keep->repr);
  q->data_representation = reps;
  q->data_representation_len = (size_t)n;
}

static void writer_qos_in(VALUE h, rdds_writer_qos* q, qos_arrays* keep) {
  Check_Type(h, T_HASH);
  memset(q, 0, sizeof *q);
  common_qos_in(h, q, keep);
  QB(q, h, autodispose_unregistered_instances);
  QD(q, h, service_cleanup_delay);
  QI(q, h, service_history_kind);
  QI(q, h, service_history_depth);
  QI(q, h, service_max_samples);
  QI(q, h, service_max_instances);
  QI(q, h, service_max_samples_per_instance);
}

static void reader_qos_in(VALUE h, rdds_reader_qos* q, qos_arrays* keep) {
  rdds_writer_qos w;
  Check_Type(h, T_HASH);
  memset(q, 0, sizeof *q);
  memset(&w, 0, sizeof w);
  common_qos_in(h, &w, keep);
  q->reliability_kind = w.reliability_kind;
  q->max_blocking_time = w.max_blocking_time;
  q->history_kind = w.history_kind;
  q->history_depth = w.history_depth;
  q->max_samples = w.max_samples;
  q->max_instances = w.max_instances;
  q->max_samples_per_instance = w.max_samples_per_instance;
  q->durability_kind = w.durability_kind;
  q->deadline_period = w.deadline_period;
  q->liveliness_kind = w.liveliness_kind;
  q->lease_duration = w.lease_duration;
  q->user_data = w.user_data;
  q->user_data_len = w.user_data_len;
  q->data_representation = w.data_representation;
  q->data_representation_len = w.data_representation_len;
  QI(q, h, type_consistency_kind);
  QB(q, h, ignore_sequence_bounds);
  QB(q, h, ignore_string_bounds);
  QB(q, h, ignore_member_names);
  QB(q, h, prevent_type_widening);
  QB(q, h, force_type_validation);
}

#define HSET(h, f, v) rb_hash_aset(h, ID2SYM(rb_intern(f)), v)

static VALUE common_qos_out(int32_t reliability_kind, rdds_duration max_blocking_time, int32_t history_kind,
                            int32_t history_depth, int32_t max_samples, int32_t max_instances,
                            int32_t max_samples_per_instance, int32_t durability_kind, rdds_duration deadline,
                            int32_t liveliness_kind, rdds_duration lease, const uint8_t* ud, size_t ud_len,
                            const int16_t* dr, size_t dr_len) {
  VALUE h = rb_hash_new(), reps = rb_ary_new();
  size_t i;
  HSET(h, "reliability_kind", INT2NUM(reliability_kind));
  HSET(h, "max_blocking_time", duration_out(max_blocking_time));
  HSET(h, "history_kind", INT2NUM(history_kind));
  HSET(h, "history_depth", INT2NUM(history_depth));
  HSET(h, "max_samples", INT2NUM(max_samples));
  HSET(h, "max_instances", INT2NUM(max_instances));
  HSET(h, "max_samples_per_instance", INT2NUM(max_samples_per_instance));
  HSET(h, "durability_kind", INT2NUM(durability_kind));
  HSET(h, "deadline_period", duration_out(deadline));
  HSET(h, "liveliness_kind", INT2NUM(liveliness_kind));
  HSET(h, "lease_duration", duration_out(lease));
  HSET(h, "user_data", rb_str_new((const char*)ud, (long)ud_len));
  for (i = 0; i < dr_len; i++) rb_ary_push(reps, INT2NUM(dr[i]));
  HSET(h, "data_representation", reps);
  return h;
}

static VALUE native_writer_qos_default(VALUE self) {
  rdds_writer_qos q;
  VALUE h;
  (void)self;
  CHECK(rdds_writer_qos_default(&q));
  h = common_qos_out(q.reliability_kind, q.max_blocking_time, q.history_kind, q.history_depth, q.max_samples,
                     q.max_instances, q.max_samples_per_instance, q.durability_kind, q.deadline_period,
                     q.liveliness_kind, q.lease_duration, q.user_data, q.user_data_len, q.data_representation,
                     q.data_representation_len);
  HSET(h, "autodispose_unregistered_instances", q.autodispose_unregistered_instances ? Qtrue : Qfalse);
  HSET(h, "service_cleanup_delay", duration_out(q.service_cleanup_delay));
  HSET(h, "service_history_kind", INT2NUM(q.service_history_kind));
  HSET(h, "service_history_depth", INT2NUM(q.service_history_depth));
  HSET(h, "service_max_samples", INT2NUM(q.service_max_samples));
  HSET(h, "service_max_instances", INT2NUM(q.service_max_instances));
  HSET(h, "service_max_samples_per_instance", INT2NUM(q.service_max_samples_per_instance));
  return h;
}

static VALUE native_reader_qos_default(VALUE self) {
  rdds_reader_qos q;
  VALUE h;
  (void)self;
  CHECK(rdds_reader_qos_default(&q));
  h = common_qos_out(q.reliability_kind, q.max_blocking_time, q.history_kind, q.history_depth, q.max_samples,
                     q.max_instances, q.max_samples_per_instance, q.durability_kind, q.deadline_period,
                     q.liveliness_kind, q.lease_duration, q.user_data, q.user_data_len, q.data_representation,
                     q.data_representation_len);
  HSET(h, "type_consistency_kind", INT2NUM(q.type_consistency_kind));
  HSET(h, "ignore_sequence_bounds", q.ignore_sequence_bounds ? Qtrue : Qfalse);
  HSET(h, "ignore_string_bounds", q.ignore_string_bounds ? Qtrue : Qfalse);
  HSET(h, "ignore_member_names", q.ignore_member_names ? Qtrue : Qfalse);
  HSET(h, "prevent_type_widening", q.prevent_type_widening ? Qtrue : Qfalse);
  HSET(h, "force_type_validation", q.force_type_validation ? Qtrue : Qfalse);
  return h;
}

/* An Array of Strings as NUL-terminated pointers, in a Ruby String (kept
   alive with the Strings by the caller's keep Array). */
static const char* const* strings_in(VALUE ary, VALUE keep, size_t* n) {
  long i, len;
  VALUE buf;
  const char** p;
  Check_Type(ary, T_ARRAY);
  len = RARRAY_LEN(ary);
  buf = rb_str_buf_new((long)sizeof(char*) * (len > 0 ? len : 1));
  rb_ary_push(keep, buf);
  for (i = 0; i < len; i++) {
    VALUE s = rb_str_new_frozen(rb_ary_entry(ary, i));
    StringValueCStr(s);
    rb_ary_push(keep, s);
  }
  p = (const char**)RSTRING_PTR(buf);
  for (i = 0; i < len; i++) p[i] = RSTRING_PTR(rb_ary_entry(keep, RARRAY_LEN(keep) - len + i));
  *n = (size_t)len;
  return p;
}

/* --- listeners -------------------------------------------------------------------- */

/* A listener's self is its id in DDS::Native's registry of listeners; an
   event is delivered with DDS::Native.dispatch(id, event, endpoint, arg). */

enum ev_kind {
  EV_DEADLINE,
  EV_INCOMPATIBLE,
  EV_COUNT,
  EV_MATCHED,
  EV_LIVELINESS_CHANGED,
  EV_REJECTED,
  EV_NONE,
  EV_DATA_ON_READERS,
  EV_RELEASE
};

typedef struct event {
  uintptr_t id;
  const char* name;
  int kind;
  const rdds_endpoint_info* endpoint;
  const char* topic_name;
  const void* status;
} event;

static VALUE status_value(const event* e) {
  switch (e->kind) {
    case EV_DEADLINE: {
      const rdds_deadline_missed_status* s = e->status;
      return rb_ary_new_from_args(3, INT2NUM(s->total_count), INT2NUM(s->total_count_change),
                                  bytes16(s->last_instance_handle.value));
    }
    case EV_INCOMPATIBLE: {
      const rdds_incompatible_qos_status* s = e->status;
      VALUE ps = rb_ary_new();
      uint32_t i;
      for (i = 0; i < s->policies_len && i < RDDS_MAX_POLICIES; i++)
        rb_ary_push(ps, rb_ary_new_from_args(2, INT2NUM(s->policies[i].policy_id), INT2NUM(s->policies[i].count)));
      return rb_ary_new_from_args(4, INT2NUM(s->total_count), INT2NUM(s->total_count_change),
                                  INT2NUM(s->last_policy_id), ps);
    }
    case EV_COUNT: {
      const rdds_count_status* s = e->status;
      return rb_ary_new_from_args(2, INT2NUM(s->total_count), INT2NUM(s->total_count_change));
    }
    case EV_MATCHED: {
      const rdds_matched_status* s = e->status;
      return rb_ary_new_from_args(5, INT2NUM(s->total_count), INT2NUM(s->total_count_change),
                                  INT2NUM(s->current_count), INT2NUM(s->current_count_change),
                                  bytes16(s->last_handle.value));
    }
    case EV_LIVELINESS_CHANGED: {
      const rdds_liveliness_changed_status* s = e->status;
      return rb_ary_new_from_args(5, INT2NUM(s->alive_count), INT2NUM(s->not_alive_count),
                                  INT2NUM(s->alive_count_change), INT2NUM(s->not_alive_count_change),
                                  bytes16(s->last_publication_handle.value));
    }
    case EV_REJECTED: {
      const rdds_sample_rejected_status* s = e->status;
      return rb_ary_new_from_args(4, INT2NUM(s->total_count), INT2NUM(s->total_count_change),
                                  INT2NUM(s->last_reason), bytes16(s->last_instance_handle.value));
    }
    case EV_DATA_ON_READERS: {
      rdds_subscriber* c = NULL;
      if (rdds_subscriber_clone(e->status, &c) != RDDS_RETCODE_OK) return Qnil;
      return wrap(K_SUBSCRIBER, c);
    }
    default:
      return Qnil;
  }
}

static VALUE deliver_body(VALUE p) {
  const event* e = (const event*)p;
  VALUE endpoint = Qnil;
  if (e->endpoint) endpoint = bytes16(e->endpoint->guid.value);
  if (e->topic_name) endpoint = rb_utf8_str_new_cstr(e->topic_name);
  return rb_funcall(mNative, id_dispatch, 4, ULONG2NUM((unsigned long)e->id), ID2SYM(rb_intern(e->name)), endpoint,
                    status_value(e));
}

static void* deliver(void* p) {
  int state = 0;
  rb_protect(deliver_body, (VALUE)p, &state);
  if (state) rb_set_errinfo(Qnil); /* DDS::Native.dispatch reports what it can */
  return NULL;
}

static void fire(void* self, const char* name, int kind, const rdds_endpoint_info* endpoint, const void* status) {
  event e;
  e.id = (uintptr_t)self;
  e.name = name;
  e.kind = kind;
  e.endpoint = endpoint;
  e.topic_name = NULL;
  e.status = status;
  with_gvl(deliver, &e);
}

#define L_STATUS(fn, ev, stype)                                                         \
  static void l_##fn(void* self, const rdds_endpoint_info* who, const stype* s) {       \
    fire(self, #fn, ev, who, s);                                                        \
  }                                                                                     \
  static void w_##fn(void* self, const stype* s) { fire(self, #fn, ev, NULL, s); }

L_STATUS(on_offered_deadline_missed, EV_DEADLINE, rdds_deadline_missed_status)
L_STATUS(on_offered_incompatible_qos, EV_INCOMPATIBLE, rdds_incompatible_qos_status)
L_STATUS(on_liveliness_lost, EV_COUNT, rdds_count_status)
L_STATUS(on_publication_matched, EV_MATCHED, rdds_matched_status)
L_STATUS(on_requested_deadline_missed, EV_DEADLINE, rdds_deadline_missed_status)
L_STATUS(on_requested_incompatible_qos, EV_INCOMPATIBLE, rdds_incompatible_qos_status)
L_STATUS(on_sample_rejected, EV_REJECTED, rdds_sample_rejected_status)
L_STATUS(on_liveliness_changed, EV_LIVELINESS_CHANGED, rdds_liveliness_changed_status)
L_STATUS(on_subscription_matched, EV_MATCHED, rdds_matched_status)
L_STATUS(on_sample_lost, EV_COUNT, rdds_count_status)

static void l_on_data_available(void* self, const rdds_endpoint_info* who) {
  fire(self, "on_data_available", EV_NONE, who, NULL);
}

static void w_on_data_available(void* self) { fire(self, "on_data_available", EV_NONE, NULL, NULL); }

static void l_on_data_on_readers(void* self, const rdds_subscriber* s) {
  fire(self, "on_data_on_readers", EV_DATA_ON_READERS, NULL, s);
}

static void l_on_inconsistent_topic(void* self, const char* topic, const rdds_count_status* s) {
  event e;
  e.id = (uintptr_t)self;
  e.name = "on_inconsistent_topic";
  e.kind = EV_COUNT;
  e.endpoint = NULL;
  e.topic_name = topic;
  e.status = s;
  with_gvl(deliver, &e);
}

static void l_release(void* self) { fire(self, "release", EV_RELEASE, NULL, NULL); }

static void group_listener(rdds_listener* l, VALUE id) {
  memset(l, 0, sizeof *l);
  l->self = (void*)(uintptr_t)NUM2ULONG(id);
  l->on_offered_deadline_missed = l_on_offered_deadline_missed;
  l->on_offered_incompatible_qos = l_on_offered_incompatible_qos;
  l->on_liveliness_lost = l_on_liveliness_lost;
  l->on_publication_matched = l_on_publication_matched;
  l->on_data_on_readers = l_on_data_on_readers;
  l->on_requested_deadline_missed = l_on_requested_deadline_missed;
  l->on_requested_incompatible_qos = l_on_requested_incompatible_qos;
  l->on_sample_rejected = l_on_sample_rejected;
  l->on_liveliness_changed = l_on_liveliness_changed;
  l->on_data_available = l_on_data_available;
  l->on_subscription_matched = l_on_subscription_matched;
  l->on_sample_lost = l_on_sample_lost;
  l->on_inconsistent_topic = l_on_inconsistent_topic;
  l->release = l_release;
}

static void writer_listener(rdds_writer_listener* l, VALUE id) {
  memset(l, 0, sizeof *l);
  l->self = (void*)(uintptr_t)NUM2ULONG(id);
  l->on_offered_deadline_missed = w_on_offered_deadline_missed;
  l->on_offered_incompatible_qos = w_on_offered_incompatible_qos;
  l->on_liveliness_lost = w_on_liveliness_lost;
  l->on_publication_matched = w_on_publication_matched;
  l->release = l_release;
}

static void reader_listener(rdds_reader_listener* l, VALUE id) {
  memset(l, 0, sizeof *l);
  l->self = (void*)(uintptr_t)NUM2ULONG(id);
  l->on_requested_deadline_missed = w_on_requested_deadline_missed;
  l->on_requested_incompatible_qos = w_on_requested_incompatible_qos;
  l->on_sample_rejected = w_on_sample_rejected;
  l->on_liveliness_changed = w_on_liveliness_changed;
  l->on_data_available = w_on_data_available;
  l->on_subscription_matched = w_on_subscription_matched;
  l->on_sample_lost = w_on_sample_lost;
  l->release = l_release;
}

/* --- entities ------------------------------------------------------------------- */

static rdds_status op_participant_new(call* c) {
  return rdds_participant_new(c->u, c->a, (uint32_t)c->i, c->out);
}

/* participant_new(domain, user_data, [[name, value], ...], flags) */
static VALUE native_participant_new(VALUE self, VALUE domain, VALUE user_data, VALUE props, VALUE flags) {
  rdds_participant_qos q;
  rdds_participant* p = NULL;
  VALUE keep = rb_ary_new(), buf;
  rdds_property* ps;
  long i, n;
  call c;
  (void)self;
  StringValue(user_data);
  Check_Type(props, T_ARRAY);
  n = RARRAY_LEN(props);
  buf = rb_str_buf_new((long)sizeof(rdds_property) * (n > 0 ? n : 1));
  for (i = 0; i < n; i++) {
    VALUE kv = rb_ary_entry(props, i);
    VALUE k, v;
    Check_Type(kv, T_ARRAY);
    k = rb_str_new_frozen(rb_ary_entry(kv, 0));
    v = rb_str_new_frozen(rb_ary_entry(kv, 1));
    StringValueCStr(k);
    StringValueCStr(v);
    rb_ary_push(keep, k);
    rb_ary_push(keep, v);
  }
  ps = (rdds_property*)RSTRING_PTR(buf);
  for (i = 0; i < n; i++) {
    ps[i].name = RSTRING_PTR(rb_ary_entry(keep, 2 * i));
    ps[i].value = RSTRING_PTR(rb_ary_entry(keep, 2 * i + 1));
  }
  q.user_data = (const uint8_t*)RSTRING_PTR(user_data);
  q.user_data_len = (size_t)RSTRING_LEN(user_data);
  q.property = ps;
  q.property_len = (size_t)n;
  memset(&c, 0, sizeof c);
  c.f = op_participant_new;
  c.u = NUM2UINT(domain);
  c.a = &q;
  c.i = (int32_t)NUM2UINT(flags);
  c.out = &p;
  nogvl(&c);
  RB_GC_GUARD(keep);
  RB_GC_GUARD(buf);
  RB_GC_GUARD(user_data);
  return wrap(K_PARTICIPANT, p);
}

static VALUE native_participant_info(VALUE self, VALUE p) {
  rdds_participant* pp = unwrap(p, K_PARTICIPANT);
  rdds_guid g;
  (void)self;
  CHECK(rdds_participant_guid(pp, &g));
  return rb_ary_new_from_args(3, UINT2NUM(rdds_participant_domain_id(pp)), bytes16(g.value),
                              SIZET2NUM(rdds_participant_discovered_participants(pp)));
}

static VALUE native_participant_assert_liveliness(VALUE self, VALUE p) {
  (void)self;
  CHECK(rdds_participant_assert_liveliness(unwrap(p, K_PARTICIPANT)));
  return Qnil;
}

static rdds_status op_create_topic(call* c) { return rdds_participant_create_topic(c->a, c->b, c->c, c->out); }

static VALUE native_create_topic(VALUE self, VALUE p, VALUE name, VALUE type) {
  rdds_topic* t = NULL;
  call c;
  (void)self;
  memset(&c, 0, sizeof c);
  c.f = op_create_topic;
  c.a = unwrap(p, K_PARTICIPANT);
  c.b = cstr(name);
  c.c = unwrap(type, K_TYPE);
  c.out = &t;
  nogvl(&c);
  RB_GC_GUARD(name);
  return wrap(K_TOPIC, t);
}

static VALUE native_topic_inconsistent_status(VALUE self, VALUE t) {
  rdds_count_status s;
  (void)self;
  CHECK(rdds_topic_inconsistent_topic_status(unwrap(t, K_TOPIC), &s));
  return rb_ary_new_from_args(2, INT2NUM(s.total_count), INT2NUM(s.total_count_change));
}

static rdds_status op_create_cftopic(call* c) {
  return rdds_participant_create_contentfilteredtopic(c->a, c->b, c->c, c->d, c->e, c->n, c->out);
}

static VALUE native_create_cftopic(VALUE self, VALUE p, VALUE name, VALUE topic, VALUE expr, VALUE params) {
  rdds_cftopic* t = NULL;
  VALUE keep = rb_ary_new();
  call c;
  (void)self;
  memset(&c, 0, sizeof c);
  c.f = op_create_cftopic;
  c.a = unwrap(p, K_PARTICIPANT);
  c.b = cstr(name);
  c.c = unwrap(topic, K_TOPIC);
  c.d = cstr(expr);
  c.e = strings_in(params, keep, &c.n);
  c.out = &t;
  nogvl(&c);
  RB_GC_GUARD(keep);
  RB_GC_GUARD(name);
  RB_GC_GUARD(expr);
  return wrap(K_CFTOPIC, t);
}

static VALUE native_cftopic_set_parameters(VALUE self, VALUE t, VALUE params) {
  VALUE keep = rb_ary_new();
  size_t n;
  const char* const* p;
  (void)self;
  p = strings_in(params, keep, &n);
  CHECK(rdds_cftopic_set_expression_parameters(unwrap(t, K_CFTOPIC), p, n));
  RB_GC_GUARD(keep);
  return Qnil;
}

static rdds_status op_create_publisher(call* c) { return rdds_participant_create_publisher(c->a, c->b, c->out); }
static rdds_status op_create_subscriber(call* c) { return rdds_participant_create_subscriber(c->a, c->b, c->out); }

static VALUE create_group(VALUE p, VALUE partitions, int subscriber) {
  rdds_group_qos q;
  VALUE keep = rb_ary_new();
  void* out = NULL;
  call c;
  q.partition = strings_in(partitions, keep, &q.partition_len);
  memset(&c, 0, sizeof c);
  c.f = subscriber ? op_create_subscriber : op_create_publisher;
  c.a = unwrap(p, K_PARTICIPANT);
  c.b = &q;
  c.out = &out;
  nogvl(&c);
  RB_GC_GUARD(keep);
  return wrap(subscriber ? K_SUBSCRIBER : K_PUBLISHER, out);
}

static VALUE native_create_publisher(VALUE self, VALUE p, VALUE partitions) {
  (void)self;
  return create_group(p, partitions, 0);
}

static VALUE native_create_subscriber(VALUE self, VALUE p, VALUE partitions) {
  (void)self;
  return create_group(p, partitions, 1);
}

static rdds_status op_set_group_listener(call* c) {
  switch (c->i) {
    case K_PARTICIPANT: return rdds_participant_set_listener(c->a, c->b, c->u);
    case K_PUBLISHER: return rdds_publisher_set_listener(c->a, c->b, c->u);
    case K_SUBSCRIBER: return rdds_subscriber_set_listener(c->a, c->b, c->u);
    case K_TOPIC: return rdds_topic_set_listener(c->a, c->b, c->u);
    case K_WRITER: return rdds_writer_set_listener(c->a, c->c, c->u);
    case K_READER: return rdds_reader_set_listener(c->a, c->d, c->u);
  }
  return RDDS_RETCODE_BAD_PARAMETER;
}

/* set_listener(entity, listener id or nil, mask) */
static VALUE native_set_listener(VALUE self, VALUE entity, VALUE id, VALUE mask) {
  rdds_listener gl;
  rdds_writer_listener wl;
  rdds_reader_listener rl;
  int k = handle_kind(entity);
  call c;
  (void)self;
  memset(&c, 0, sizeof c);
  c.f = op_set_group_listener;
  c.a = unwrap(entity, k);
  c.i = k;
  c.u = NUM2UINT(mask);
  if (k == K_WRITER) {
    if (!NIL_P(id)) {
      writer_listener(&wl, id);
      c.c = &wl;
    }
  } else if (k == K_READER) {
    if (!NIL_P(id)) {
      reader_listener(&rl, id);
      c.d = &rl;
    }
  } else if (k == K_PARTICIPANT || k == K_PUBLISHER || k == K_SUBSCRIBER || k == K_TOPIC) {
    if (!NIL_P(id)) {
      group_listener(&gl, id);
      c.b = &gl;
    }
  } else {
    rb_raise(rb_eTypeError, "a %s has no listener", kind_names[k]);
  }
  nogvl(&c);
  return Qnil;
}

static rdds_status op_notify_datareaders(call* c) { return rdds_subscriber_notify_datareaders(c->a); }

static VALUE native_notify_datareaders(VALUE self, VALUE s) {
  call c;
  (void)self;
  memset(&c, 0, sizeof c);
  c.f = op_notify_datareaders;
  c.a = unwrap(s, K_SUBSCRIBER);
  nogvl(&c);
  return Qnil;
}

static VALUE native_subscriber_same(VALUE self, VALUE a, VALUE b) {
  (void)self;
  return rdds_subscriber_same(unwrap(a, K_SUBSCRIBER), unwrap(b, K_SUBSCRIBER)) ? Qtrue : Qfalse;
}

/* writers */

static rdds_status op_create_writer(call* c) {
  return rdds_publisher_create_writer(c->a, c->b, c->c, c->d, c->u, c->out);
}

/* create_writer(publisher, topic, qos hash, listener id or nil, mask) */
static VALUE native_create_writer(VALUE self, VALUE p, VALUE topic, VALUE qos, VALUE id, VALUE mask) {
  rdds_writer_qos q;
  rdds_writer_listener l;
  qos_arrays keep;
  rdds_writer* w = NULL;
  call c;
  (void)self;
  writer_qos_in(qos, &q, &keep);
  memset(&c, 0, sizeof c);
  c.f = op_create_writer;
  c.a = unwrap(p, K_PUBLISHER);
  c.b = unwrap(topic, K_TOPIC);
  c.c = &q;
  if (!NIL_P(id)) {
    writer_listener(&l, id);
    c.d = &l;
  }
  c.u = NUM2UINT(mask);
  c.out = &w;
  nogvl(&c);
  RB_GC_GUARD(keep.user_data);
  RB_GC_GUARD(keep.repr);
  return wrap(K_WRITER, w);
}

static VALUE native_writer_guid(VALUE self, VALUE w) {
  rdds_guid g;
  (void)self;
  CHECK(rdds_writer_guid(unwrap(w, K_WRITER), &g));
  return bytes16(g.value);
}

static rdds_status op_writer_sample(call* c) {
  rdds_time ts, *tp = NULL;
  if (c->d) {
    ts = *(const rdds_time*)c->d;
    tp = &ts;
  }
  switch (c->i) {
    case 0: return rdds_writer_write(c->a, c->b, tp);
    case 1: return rdds_writer_dispose(c->a, c->b, tp);
    case 2: return rdds_writer_unregister_instance(c->a, c->b, tp);
    case 3: return rdds_writer_register_instance(c->a, c->b, c->out);
    case 4: return rdds_writer_lookup_instance(c->a, c->b, c->out);
  }
  return RDDS_RETCODE_BAD_PARAMETER;
}

/* writer_op(writer, op, out, timestamp [sec, nanosec] or nil): op 0 write,
   1 dispose, 2 unregister_instance, 3 register_instance, 4
   lookup_instance (the last two return the instance handle). */
static VALUE native_writer_op(VALUE self, VALUE w, VALUE op, VALUE out, VALUE ts) {
  rdds_time t;
  rdds_instance_handle h;
  call c;
  (void)self;
  memset(&c, 0, sizeof c);
  memset(&h, 0, sizeof h);
  c.f = op_writer_sample;
  c.a = unwrap(w, K_WRITER);
  c.b = out_of(out);
  c.i = NUM2INT(op);
  if (!NIL_P(ts)) {
    rdds_duration d = duration_in(ts);
    t.sec = d.sec;
    t.nanosec = d.nanosec;
    c.d = &t;
  }
  c.out = &h;
  nogvl(&c);
  RB_GC_GUARD(out);
  return c.i >= 3 ? bytes16(h.value) : Qnil;
}

static rdds_status op_writer_wait(call* c) {
  if (c->i == 0) return rdds_writer_wait_for_acknowledgments(c->a, c->dur);
  if (c->i == 1) return rdds_writer_wait_for_subscriptions(c->a, c->n, c->dur);
  if (c->i == 2) return rdds_reader_wait_for_data(c->a, c->dur);
  return rdds_reader_wait_for_publications(c->a, c->n, c->dur);
}

/* wait(entity, what, count, duration): what 0 acknowledgments, 1
   subscriptions (writers), 2 data, 3 publications (readers). */
static VALUE native_wait(VALUE self, VALUE e, VALUE what, VALUE count, VALUE dur) {
  call c;
  (void)self;
  memset(&c, 0, sizeof c);
  c.f = op_writer_wait;
  c.i = NUM2INT(what);
  c.a = unwrap(e, c.i < 2 ? K_WRITER : K_READER);
  c.n = NUM2SIZET(count);
  c.dur = duration_in(dur);
  nogvl(&c);
  return Qnil;
}

static VALUE guids_out(const rdds_guid* g, size_t n) {
  VALUE a = rb_ary_new_capa((long)n);
  size_t i;
  for (i = 0; i < n; i++) rb_ary_push(a, bytes16(g[i].value));
  return a;
}

static VALUE native_matched(VALUE self, VALUE e) {
  int k = handle_kind(e);
  void* p = unwrap(e, k);
  size_t n = 0, cap;
  rdds_guid* buf;
  VALUE r;
  (void)self;
  if (k == K_WRITER)
    CHECK(rdds_writer_matched_subscriptions(p, NULL, 0, &n));
  else
    CHECK(rdds_reader_matched_publications(unwrap(e, K_READER), NULL, 0, &n));
  cap = n + 8;
  buf = ALLOC_N(rdds_guid, cap);
  {
    rdds_status st = k == K_WRITER ? rdds_writer_matched_subscriptions(p, buf, cap, &n)
                                   : rdds_reader_matched_publications(p, buf, cap, &n);
    if (st != RDDS_RETCODE_OK) {
      xfree(buf);
      raise_status(st);
    }
  }
  r = guids_out(buf, n < cap ? n : cap);
  xfree(buf);
  return r;
}

static VALUE native_assert_liveliness(VALUE self, VALUE w) {
  (void)self;
  CHECK(rdds_writer_assert_liveliness(unwrap(w, K_WRITER)));
  return Qnil;
}

/* status(entity, name): a status as listeners get it. */
static VALUE native_status(VALUE self, VALUE e, VALUE name) {
  ID n = SYM2ID(name);
  int k = handle_kind(e);
  event ev;
  union {
    rdds_matched_status m;
    rdds_incompatible_qos_status q;
    rdds_count_status c;
    rdds_deadline_missed_status d;
    rdds_liveliness_changed_status l;
    rdds_sample_rejected_status r;
  } s;
  (void)self;
  memset(&ev, 0, sizeof ev);
  memset(&s, 0, sizeof s);
  ev.status = &s;
  if (k == K_WRITER) {
    rdds_writer* w = unwrap(e, K_WRITER);
    if (n == rb_intern("publication_matched")) {
      CHECK(rdds_writer_publication_matched_status(w, &s.m));
      ev.kind = EV_MATCHED;
    } else if (n == rb_intern("offered_incompatible_qos")) {
      CHECK(rdds_writer_offered_incompatible_qos_status(w, &s.q));
      ev.kind = EV_INCOMPATIBLE;
    } else if (n == rb_intern("liveliness_lost")) {
      CHECK(rdds_writer_liveliness_lost_status(w, &s.c));
      ev.kind = EV_COUNT;
    } else if (n == rb_intern("offered_deadline_missed")) {
      CHECK(rdds_writer_offered_deadline_missed_status(w, &s.d));
      ev.kind = EV_DEADLINE;
    } else {
      rb_raise(rb_eArgError, "no status %" PRIsVALUE, name);
    }
  } else {
    rdds_reader* r = unwrap(e, K_READER);
    if (n == rb_intern("subscription_matched")) {
      CHECK(rdds_reader_subscription_matched_status(r, &s.m));
      ev.kind = EV_MATCHED;
    } else if (n == rb_intern("requested_incompatible_qos")) {
      CHECK(rdds_reader_requested_incompatible_qos_status(r, &s.q));
      ev.kind = EV_INCOMPATIBLE;
    } else if (n == rb_intern("liveliness_changed")) {
      CHECK(rdds_reader_liveliness_changed_status(r, &s.l));
      ev.kind = EV_LIVELINESS_CHANGED;
    } else if (n == rb_intern("requested_deadline_missed")) {
      CHECK(rdds_reader_requested_deadline_missed_status(r, &s.d));
      ev.kind = EV_DEADLINE;
    } else if (n == rb_intern("sample_lost")) {
      CHECK(rdds_reader_sample_lost_status(r, &s.c));
      ev.kind = EV_COUNT;
    } else if (n == rb_intern("sample_rejected")) {
      CHECK(rdds_reader_sample_rejected_status(r, &s.r));
      ev.kind = EV_REJECTED;
    } else {
      rb_raise(rb_eArgError, "no status %" PRIsVALUE, name);
    }
  }
  return status_value(&ev);
}

static VALUE native_status_changes(VALUE self, VALUE e) {
  int k = handle_kind(e);
  (void)self;
  if (k == K_WRITER) return UINT2NUM(rdds_writer_status_changes(unwrap(e, K_WRITER)));
  return UINT2NUM(rdds_reader_status_changes(unwrap(e, K_READER)));
}

static VALUE native_status_condition(VALUE self, VALUE e) {
  int k = handle_kind(e);
  rdds_condition* c = NULL;
  (void)self;
  if (k == K_WRITER)
    CHECK(rdds_writer_status_condition(unwrap(e, K_WRITER), &c));
  else
    CHECK(rdds_reader_status_condition(unwrap(e, K_READER), &c));
  return wrap(K_CONDITION, c);
}

/* readers */

static rdds_status op_create_reader(call* c) {
  if (c->i) return rdds_subscriber_create_filtered_reader(c->a, c->b, c->c, c->d, c->u, c->out);
  return rdds_subscriber_create_reader(c->a, c->b, c->c, c->d, c->u, c->out);
}

/* create_reader(subscriber, topic or content filtered topic, qos hash,
   listener id or nil, mask) */
static VALUE native_create_reader(VALUE self, VALUE s, VALUE topic, VALUE qos, VALUE id, VALUE mask) {
  rdds_reader_qos q;
  rdds_reader_listener l;
  qos_arrays keep;
  rdds_reader* r = NULL;
  call c;
  int k = handle_kind(topic);
  (void)self;
  reader_qos_in(qos, &q, &keep);
  memset(&c, 0, sizeof c);
  c.f = op_create_reader;
  c.a = unwrap(s, K_SUBSCRIBER);
  c.i = k == K_CFTOPIC;
  c.b = unwrap(topic, c.i ? K_CFTOPIC : K_TOPIC);
  c.c = &q;
  if (!NIL_P(id)) {
    reader_listener(&l, id);
    c.d = &l;
  }
  c.u = NUM2UINT(mask);
  c.out = &r;
  nogvl(&c);
  RB_GC_GUARD(keep.user_data);
  RB_GC_GUARD(keep.repr);
  return wrap(K_READER, r);
}

static VALUE native_reader_guid(VALUE self, VALUE r) {
  rdds_guid g;
  (void)self;
  CHECK(rdds_reader_guid(unwrap(r, K_READER), &g));
  return bytes16(g.value);
}

static VALUE samples_out(rdds_samples* s) {
  size_t n = rdds_samples_count(s), i;
  VALUE a = rb_ary_new_capa((long)n);
  for (i = 0; i < n; i++) {
    rdds_sample_info info;
    rdds_in* in = NULL;
    rdds_status st = rdds_samples_info(s, i, &info);
    VALUE iv, data = Qnil;
    if (st != RDDS_RETCODE_OK) {
      rdds_samples_release(s);
      raise_status(st);
    }
    if (info.valid_data) {
      st = rdds_samples_data(s, i, &in);
      if (st == RDDS_RETCODE_OK) data = wrap_in(in);
    }
    iv = rb_ary_new_from_args(13, UINT2NUM(info.sample_state), UINT2NUM(info.view_state),
                              UINT2NUM(info.instance_state), INT2NUM(info.disposed_generation_count),
                              INT2NUM(info.no_writers_generation_count), INT2NUM(info.sample_rank),
                              INT2NUM(info.generation_rank), INT2NUM(info.absolute_generation_rank),
                              rb_ary_new_from_args(2, INT2NUM(info.source_timestamp.sec),
                                                   UINT2NUM(info.source_timestamp.nanosec)),
                              bytes16(info.instance_handle.value), bytes16(info.publication_handle.value),
                              LL2NUM(info.sequence_number), info.valid_data ? Qtrue : Qfalse);
    rb_ary_push(a, rb_ary_new_from_args(2, iv, data));
  }
  rdds_samples_release(s);
  return a;
}

/* select(reader, take, max, sample_states, view_states, instance_states,
   mode, instance or nil): [[info Array, In or nil], ...] */
static VALUE native_select(int argc, VALUE* argv, VALUE self) {
  rdds_samples* s = NULL;
  rdds_instance_handle h;
  (void)self;
  if (argc != 8) rb_raise(rb_eArgError, "8 arguments expected");
  if (!NIL_P(argv[7])) bytes16_in(argv[7], h.value);
  CHECK(rdds_reader_select(unwrap(argv[0], K_READER), RTEST(argv[1]), NUM2INT(argv[2]), NUM2UINT(argv[3]),
                           NUM2UINT(argv[4]), NUM2UINT(argv[5]), NUM2INT(argv[6]), NIL_P(argv[7]) ? NULL : &h,
                           &s));
  return samples_out(s);
}

static VALUE native_select_w_condition(VALUE self, VALUE r, VALUE take, VALUE max, VALUE cond) {
  rdds_samples* s = NULL;
  (void)self;
  CHECK(rdds_reader_select_w_condition(unwrap(r, K_READER), RTEST(take), NUM2INT(max), unwrap(cond, K_CONDITION),
                                       &s));
  return samples_out(s);
}

static VALUE native_reader_lookup_instance(VALUE self, VALUE r, VALUE out) {
  rdds_instance_handle h;
  (void)self;
  CHECK(rdds_reader_lookup_instance(unwrap(r, K_READER), out_of(out), &h));
  return bytes16(h.value);
}

static VALUE native_key_value(VALUE self, VALUE r, VALUE handle) {
  rdds_instance_handle h;
  rdds_in* in = NULL;
  (void)self;
  bytes16_in(handle, h.value);
  CHECK(rdds_reader_get_key_value(unwrap(r, K_READER), h, &in));
  return wrap_in(in);
}

static VALUE native_create_readcondition(VALUE self, VALUE r, VALUE ss, VALUE vs, VALUE is) {
  rdds_condition* c = NULL;
  (void)self;
  CHECK(rdds_reader_create_readcondition(unwrap(r, K_READER), NUM2UINT(ss), NUM2UINT(vs), NUM2UINT(is), &c));
  return wrap(K_CONDITION, c);
}

/* --- conditions and waitsets ----------------------------------------------------- */

static VALUE native_guard_condition(VALUE self) {
  rdds_condition* c = NULL;
  (void)self;
  CHECK(rdds_guard_condition_new(&c));
  return wrap(K_CONDITION, c);
}

static VALUE native_trigger_value(VALUE self, VALUE c) {
  (void)self;
  return rdds_condition_trigger_value(unwrap(c, K_CONDITION)) ? Qtrue : Qfalse;
}

static VALUE native_set_trigger_value(VALUE self, VALUE c, VALUE v) {
  (void)self;
  CHECK(rdds_guard_condition_set_trigger_value(unwrap(c, K_CONDITION), RTEST(v)));
  return Qnil;
}

static VALUE native_condition_equals(VALUE self, VALUE a, VALUE b) {
  (void)self;
  return rdds_condition_equals(unwrap(a, K_CONDITION), unwrap(b, K_CONDITION)) ? Qtrue : Qfalse;
}

static VALUE native_enabled_statuses(VALUE self, VALUE c) {
  uint32_t m = 0;
  (void)self;
  CHECK(rdds_status_condition_get_enabled_statuses(unwrap(c, K_CONDITION), &m));
  return UINT2NUM(m);
}

static VALUE native_set_enabled_statuses(VALUE self, VALUE c, VALUE m) {
  (void)self;
  CHECK(rdds_status_condition_set_enabled_statuses(unwrap(c, K_CONDITION), NUM2UINT(m)));
  return Qnil;
}

static VALUE native_waitset_new(VALUE self) {
  rdds_waitset* w = NULL;
  (void)self;
  CHECK(rdds_waitset_new(&w));
  return wrap(K_WAITSET, w);
}

static VALUE native_waitset_attach(VALUE self, VALUE w, VALUE c, VALUE attach) {
  (void)self;
  if (RTEST(attach))
    CHECK(rdds_waitset_attach_condition(unwrap(w, K_WAITSET), unwrap(c, K_CONDITION)));
  else
    CHECK(rdds_waitset_detach_condition(unwrap(w, K_WAITSET), unwrap(c, K_CONDITION)));
  return Qnil;
}

static rdds_status op_waitset_wait(call* c) { return rdds_waitset_wait(c->a, c->dur, c->out, c->cap, c->len); }

/* waitset_wait(waitset, duration, attached count): the indices of the
   conditions that are true. */
static VALUE native_waitset_wait(VALUE self, VALUE w, VALUE dur, VALUE count) {
  size_t cap = NUM2SIZET(count) + 1, n = 0, i;
  VALUE buf = rb_str_buf_new((long)(sizeof(size_t) * cap)), r;
  size_t* active;
  call c;
  (void)self;
  memset(&c, 0, sizeof c);
  c.f = op_waitset_wait;
  c.a = unwrap(w, K_WAITSET);
  c.dur = duration_in(dur);
  c.out = RSTRING_PTR(buf);
  c.cap = cap;
  c.len = &n;
  nogvl(&c);
  active = (size_t*)RSTRING_PTR(buf);
  r = rb_ary_new();
  for (i = 0; i < n && i < cap; i++) rb_ary_push(r, SIZET2NUM(active[i]));
  RB_GC_GUARD(buf);
  return r;
}

/* --- setup ------------------------------------------------------------------------ */

/* DDS::Native.setup(errors): the exception classes, by return code. */
static VALUE native_setup(VALUE self, VALUE errs) {
  (void)self;
  Check_Type(errs, T_ARRAY);
  errors = rb_ary_dup(errs);
  return Qnil;
}

static VALUE native_abi_version(VALUE self) {
  (void)self;
  return UINT2NUM(rdds_abi_version());
}

void Init_rustorb_dds_ext(void) {
  rb_ext_ractor_safe(false);
  mDDS = rb_define_module("DDS");
  mNative = rb_define_module_under(mDDS, "Native");
  errors = Qnil;
  rb_gc_register_address(&errors);
  id_dispatch = rb_intern("dispatch");
  id_spawn_dispatcher = rb_intern("spawn_dispatcher");
  id_negative_p = rb_intern("negative?");

  cHandle = rb_define_class_under(mNative, "Handle", rb_cObject);
  rb_undef_alloc_func(cHandle);
  rb_define_method(cHandle, "closed?", handle_closed_p, 0);

  rb_define_module_function(mNative, "setup", native_setup, 1);
  rb_define_module_function(mNative, "abi_version", native_abi_version, 0);
  rb_define_module_function(mNative, "dispatcher_loop", native_dispatcher_loop, 0);
  rb_define_module_function(mNative, "dispatchers", native_dispatchers, 0);
  rb_define_module_function(mNative, "release", native_release, 1);
  rb_set_end_proc(at_exit_dispatchers, Qnil);

  rb_define_module_function(mNative, "type_primitive", native_type_primitive, 1);
  rb_define_module_function(mNative, "type_string", native_type_string, 2);
  rb_define_module_function(mNative, "type_sequence", native_type_sequence, 2);
  rb_define_module_function(mNative, "type_array", native_type_array, 2);
  rb_define_module_function(mNative, "type_alias", native_type_alias, 2);
  rb_define_module_function(mNative, "type_enum", native_type_enum, 4);
  rb_define_module_function(mNative, "type_bitmask", native_type_bitmask, 4);
  rb_define_module_function(mNative, "type_struct", native_type_struct, 4);
  rb_define_module_function(mNative, "type_union", native_type_union, 5);
  rb_define_module_function(mNative, "type_hashes", native_type_hashes, 1);
  rb_define_module_function(mNative, "serialize", native_serialize, 3);
  rb_define_module_function(mNative, "deserialize", native_deserialize, 2);

  cOut = rb_define_class_under(mNative, "Out", rb_cObject);
  rb_define_alloc_func(cOut, out_alloc);
  rb_define_method(cOut, "initialize", out_initialize, 0);
  rb_define_method(cOut, "clear", out_clear, 0);
  rb_define_method(cOut, "boolean", out_boolean, 1);
  rb_define_method(cOut, "octet", out_octet, 1);
  rb_define_method(cOut, "int8", out_int8, 1);
  rb_define_method(cOut, "uint8", out_uint8, 1);
  rb_define_method(cOut, "int16", out_int16, 1);
  rb_define_method(cOut, "uint16", out_uint16, 1);
  rb_define_method(cOut, "int32", out_int32, 1);
  rb_define_method(cOut, "uint32", out_uint32, 1);
  rb_define_method(cOut, "int64", out_int64, 1);
  rb_define_method(cOut, "uint64", out_uint64, 1);
  rb_define_method(cOut, "float32", out_float32, 1);
  rb_define_method(cOut, "float64", out_float64, 1);
  rb_define_method(cOut, "float128", out_float128, 2);
  rb_define_method(cOut, "char8", out_char8, 1);
  rb_define_method(cOut, "char16", out_char16, 1);
  rb_define_method(cOut, "string", out_string, 1);
  rb_define_method(cOut, "wstring", out_wstring, 1);
  rb_define_method(cOut, "length", out_length, 1);
  rb_define_method(cOut, "present", out_present, 1);
  rb_define_method(cOut, "discriminator", out_discriminator, 1);
  rb_define_method(cOut, "enum", out_enum, 1);
  rb_define_method(cOut, "bitmask", out_bitmask, 1);
  rb_define_method(cOut, "array", out_array, 2);

  cIn = rb_define_class_under(mNative, "In", rb_cObject);
  rb_undef_alloc_func(cIn);
  rb_define_method(cIn, "check", in_check, 0);
  rb_define_method(cIn, "at_end?", in_at_end_p, 0);
  rb_define_method(cIn, "release", in_release, 0);
  rb_define_method(cIn, "boolean", in_boolean, 0);
  rb_define_method(cIn, "octet", in_octet, 0);
  rb_define_method(cIn, "int8", in_int8, 0);
  rb_define_method(cIn, "uint8", in_uint8, 0);
  rb_define_method(cIn, "int16", in_int16, 0);
  rb_define_method(cIn, "uint16", in_uint16, 0);
  rb_define_method(cIn, "int32", in_int32, 0);
  rb_define_method(cIn, "uint32", in_uint32, 0);
  rb_define_method(cIn, "int64", in_int64, 0);
  rb_define_method(cIn, "uint64", in_uint64, 0);
  rb_define_method(cIn, "float32", in_float32, 0);
  rb_define_method(cIn, "float64", in_float64, 0);
  rb_define_method(cIn, "float128", in_float128, 0);
  rb_define_method(cIn, "char8", in_char8, 0);
  rb_define_method(cIn, "char16", in_char16, 0);
  rb_define_method(cIn, "string", in_string, 0);
  rb_define_method(cIn, "wstring", in_wstring, 0);
  rb_define_method(cIn, "length", in_length, 0);
  rb_define_method(cIn, "present", in_present, 0);
  rb_define_method(cIn, "discriminator", in_discriminator, 0);
  rb_define_method(cIn, "enum", in_enum, 0);
  rb_define_method(cIn, "bitmask", in_bitmask, 0);
  rb_define_method(cIn, "array", in_array, 2);

  rb_define_module_function(mNative, "writer_qos_default", native_writer_qos_default, 0);
  rb_define_module_function(mNative, "reader_qos_default", native_reader_qos_default, 0);

  rb_define_module_function(mNative, "participant_new", native_participant_new, 4);
  rb_define_module_function(mNative, "participant_info", native_participant_info, 1);
  rb_define_module_function(mNative, "participant_assert_liveliness", native_participant_assert_liveliness, 1);
  rb_define_module_function(mNative, "create_topic", native_create_topic, 3);
  rb_define_module_function(mNative, "topic_inconsistent_status", native_topic_inconsistent_status, 1);
  rb_define_module_function(mNative, "create_cftopic", native_create_cftopic, 5);
  rb_define_module_function(mNative, "cftopic_set_parameters", native_cftopic_set_parameters, 2);
  rb_define_module_function(mNative, "create_publisher", native_create_publisher, 2);
  rb_define_module_function(mNative, "create_subscriber", native_create_subscriber, 2);
  rb_define_module_function(mNative, "set_listener", native_set_listener, 3);
  rb_define_module_function(mNative, "notify_datareaders", native_notify_datareaders, 1);
  rb_define_module_function(mNative, "subscriber_same", native_subscriber_same, 2);
  rb_define_module_function(mNative, "create_writer", native_create_writer, 5);
  rb_define_module_function(mNative, "writer_guid", native_writer_guid, 1);
  rb_define_module_function(mNative, "writer_op", native_writer_op, 4);
  rb_define_module_function(mNative, "wait", native_wait, 4);
  rb_define_module_function(mNative, "matched", native_matched, 1);
  rb_define_module_function(mNative, "assert_liveliness", native_assert_liveliness, 1);
  rb_define_module_function(mNative, "status", native_status, 2);
  rb_define_module_function(mNative, "status_changes", native_status_changes, 1);
  rb_define_module_function(mNative, "status_condition", native_status_condition, 1);
  rb_define_module_function(mNative, "create_reader", native_create_reader, 5);
  rb_define_module_function(mNative, "reader_guid", native_reader_guid, 1);
  rb_define_module_function(mNative, "select", native_select, -1);
  rb_define_module_function(mNative, "select_w_condition", native_select_w_condition, 4);
  rb_define_module_function(mNative, "reader_lookup_instance", native_reader_lookup_instance, 2);
  rb_define_module_function(mNative, "key_value", native_key_value, 2);
  rb_define_module_function(mNative, "create_readcondition", native_create_readcondition, 4);

  rb_define_module_function(mNative, "guard_condition", native_guard_condition, 0);
  rb_define_module_function(mNative, "trigger_value", native_trigger_value, 1);
  rb_define_module_function(mNative, "set_trigger_value", native_set_trigger_value, 2);
  rb_define_module_function(mNative, "condition_equals", native_condition_equals, 2);
  rb_define_module_function(mNative, "enabled_statuses", native_enabled_statuses, 1);
  rb_define_module_function(mNative, "set_enabled_statuses", native_set_enabled_statuses, 2);
  rb_define_module_function(mNative, "waitset_new", native_waitset_new, 0);
  rb_define_module_function(mNative, "waitset_attach", native_waitset_attach, 3);
  rb_define_module_function(mNative, "waitset_wait", native_waitset_wait, 3);
}
