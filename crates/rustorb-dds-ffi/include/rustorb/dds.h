/*
 * rustorb/dds.h: the C ABI of rustorb-dds (crate rustorb-dds-ffi).
 *
 * SPDX-License-Identifier: MIT OR Apache-2.0
 *
 * The C++ binding (cpp/include) and the Java binding are built on these
 * functions; they can be used from C too.
 *
 * Rules every caller follows:
 *
 * - Handles. Every rdds_* object (rdds_participant, rdds_topic, ...) is
 *   opaque and made by a function of this header that stores it in an
 *   output pointer. It is freed with its rdds_*_release function, once;
 *   releasing NULL does nothing. Releasing a writer or reader deletes it;
 *   releasing a participant deletes it and its entities stop working.
 *   A handle may be used from any thread.
 * - Errors. Functions that can fail return rdds_status, a DDS
 *   ReturnCode_t (RDDS_RETCODE_OK and the others below). After a failure
 *   rdds_last_error_message() tells why, on the same thread. Pointers
 *   the caller passes must be valid for the call; output pointers must
 *   not be NULL.
 * - Strings are UTF-8 and NUL-terminated, except in sample streams
 *   (rdds_out_string, rdds_in_string), where they carry a length. Wide
 *   strings and wide characters are UTF-16 code units.
 * - Samples. The caller writes the values of a sample in the order of its
 *   type into an rdds_out, and reads them in the same order from an
 *   rdds_in (see "Samples" below). The library encodes them.
 * - Listeners are called on a thread of the library, or, for a reader of
 *   the same participant as the writer, inside the rdds_writer_* call that
 *   wrote, on the caller's thread. No lock of the library is held: they
 *   may call any function, but must not block for long.
 */
#ifndef RUSTORB_DDS_H
#define RUSTORB_DDS_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* The version of this ABI; compare with rdds_abi_version(). */
#define RDDS_ABI_VERSION 1u

uint32_t rdds_abi_version(void);

/* ------------------------------------------------------------------ */
/* Return codes (DDS 1.4, 2.2.1.1)                                    */

typedef int32_t rdds_status;

#define RDDS_RETCODE_OK 0
#define RDDS_RETCODE_ERROR 1
#define RDDS_RETCODE_UNSUPPORTED 2
#define RDDS_RETCODE_BAD_PARAMETER 3
#define RDDS_RETCODE_PRECONDITION_NOT_MET 4
#define RDDS_RETCODE_OUT_OF_RESOURCES 5
#define RDDS_RETCODE_NOT_ENABLED 6
#define RDDS_RETCODE_IMMUTABLE_POLICY 7
#define RDDS_RETCODE_INCONSISTENT_POLICY 8
#define RDDS_RETCODE_ALREADY_DELETED 9
#define RDDS_RETCODE_TIMEOUT 10
#define RDDS_RETCODE_NO_DATA 11
#define RDDS_RETCODE_ILLEGAL_OPERATION 12

/* The message of the last failure on this thread; valid until the next. */
const char *rdds_last_error_message(void);

/* ------------------------------------------------------------------ */
/* Plain values                                                       */

typedef struct rdds_duration {
  int32_t sec;
  uint32_t nanosec;
} rdds_duration;

#define RDDS_DURATION_INFINITE_SEC 0x7fffffff
#define RDDS_DURATION_INFINITE_NSEC 0x7fffffffu

/* Since 1970-01-01T00:00:00Z. */
typedef struct rdds_time {
  int32_t sec;
  uint32_t nanosec;
} rdds_time;

#define RDDS_TIME_INVALID_SEC (-1)
#define RDDS_TIME_INVALID_NSEC 0xffffffffu

typedef struct rdds_guid {
  uint8_t value[16];
} rdds_guid;

/* All zeros is HANDLE_NIL. */
typedef struct rdds_instance_handle {
  uint8_t value[16];
} rdds_instance_handle;

/* ------------------------------------------------------------------ */
/* Types (DDS-XTypes 1.3, 7.5.2.2)                                    */

typedef struct rdds_type rdds_type;
typedef struct rdds_enum_builder rdds_enum_builder;
typedef struct rdds_bitmask_builder rdds_bitmask_builder;
typedef struct rdds_struct_builder rdds_struct_builder;
typedef struct rdds_union_builder rdds_union_builder;

/* Type kinds of the primitive types (DDS-XTypes 1.3, 7.3.4.6). */
#define RDDS_TK_BOOLEAN 0x01
#define RDDS_TK_BYTE 0x02
#define RDDS_TK_INT16 0x03
#define RDDS_TK_INT32 0x04
#define RDDS_TK_INT64 0x05
#define RDDS_TK_UINT16 0x06
#define RDDS_TK_UINT32 0x07
#define RDDS_TK_UINT64 0x08
#define RDDS_TK_FLOAT32 0x09
#define RDDS_TK_FLOAT64 0x0A
#define RDDS_TK_FLOAT128 0x0B
#define RDDS_TK_INT8 0x0C
#define RDDS_TK_UINT8 0x0D
#define RDDS_TK_CHAR8 0x10
#define RDDS_TK_CHAR16 0x11

/* Extensibility. */
#define RDDS_FINAL 0
#define RDDS_APPENDABLE 1
#define RDDS_MUTABLE 2

/* Flags of a struct or union. */
#define RDDS_TYPE_NESTED 1u
#define RDDS_TYPE_AUTOID_HASH 2u

/* Flags of a member. */
#define RDDS_MEMBER_KEY 1u
#define RDDS_MEMBER_OPTIONAL 2u
#define RDDS_MEMBER_MUST_UNDERSTAND 4u
#define RDDS_MEMBER_EXTERNAL 8u
#define RDDS_MEMBER_TRY_CONSTRUCT_USE_DEFAULT 16u
#define RDDS_MEMBER_TRY_CONSTRUCT_TRIM 32u
#define RDDS_MEMBER_DEFAULT_LABEL 64u

/* Every function that makes a type copies the types it is given. A type
 * cannot refer to itself. */
rdds_status rdds_type_primitive(uint8_t kind, rdds_type **out);
/* bound 0: unbounded. */
rdds_status rdds_type_string(bool wide, uint32_t bound, rdds_type **out);
rdds_status rdds_type_sequence(const rdds_type *element, uint32_t bound,
                               rdds_type **out);
rdds_status rdds_type_array(const rdds_type *element, const uint32_t *dimensions,
                            size_t dimensions_len, rdds_type **out);
rdds_status rdds_type_map(const rdds_type *key, const rdds_type *element,
                          uint32_t bound, rdds_type **out);
/* A typedef; the name is fully scoped ("A::B::T"). */
rdds_status rdds_type_alias(const char *name, const rdds_type *related,
                            rdds_type **out);

rdds_status rdds_enum_builder_new(const char *name, uint16_t bit_bound,
                                  int32_t extensibility,
                                  rdds_enum_builder **out);
rdds_status rdds_enum_builder_add_literal(rdds_enum_builder *b, const char *name,
                                          int32_t value, bool is_default);
rdds_status rdds_enum_builder_build(const rdds_enum_builder *b, rdds_type **out);
void rdds_enum_builder_release(rdds_enum_builder *b);

rdds_status rdds_bitmask_builder_new(const char *name, uint16_t bit_bound,
                                     int32_t extensibility,
                                     rdds_bitmask_builder **out);
rdds_status rdds_bitmask_builder_add_flag(rdds_bitmask_builder *b,
                                          const char *name, uint16_t position);
rdds_status rdds_bitmask_builder_build(const rdds_bitmask_builder *b,
                                       rdds_type **out);
void rdds_bitmask_builder_release(rdds_bitmask_builder *b);

/* base: NULL for none. */
rdds_status rdds_struct_builder_new(const char *name, int32_t extensibility,
                                    uint32_t flags, const rdds_type *base,
                                    rdds_struct_builder **out);
/* hash_id: the string of @hashid ("" when none is given), or NULL. */
rdds_status rdds_struct_builder_add_member(rdds_struct_builder *b,
                                           const char *name, uint32_t id,
                                           const rdds_type *type, uint32_t flags,
                                           const char *hash_id);
rdds_status rdds_struct_builder_build(const rdds_struct_builder *b,
                                      rdds_type **out);
void rdds_struct_builder_release(rdds_struct_builder *b);

/* discriminator_flags: RDDS_MEMBER_KEY and the try-construct flags. */
rdds_status rdds_union_builder_new(const char *name, int32_t extensibility,
                                   uint32_t flags, const rdds_type *discriminator,
                                   uint32_t discriminator_flags,
                                   rdds_union_builder **out);
rdds_status rdds_union_builder_add_member(rdds_union_builder *b,
                                          const char *name, uint32_t id,
                                          const rdds_type *type,
                                          const int32_t *labels,
                                          size_t labels_len, uint32_t flags,
                                          const char *hash_id);
rdds_status rdds_union_builder_build(const rdds_union_builder *b,
                                     rdds_type **out);
void rdds_union_builder_release(rdds_union_builder *b);

rdds_status rdds_type_clone(const rdds_type *t, rdds_type **out);
void rdds_type_release(rdds_type *t);

/* The 14-octet equivalence hashes of the minimal and complete TypeObjects
 * of a topic type (DDS-XTypes 1.3, 7.3.4.9). */
rdds_status rdds_type_hashes(const rdds_type *t, uint8_t minimal[14],
                             uint8_t complete[14]);

/* ------------------------------------------------------------------ */
/* Samples                                                            */
/*
 * The values of a sample, in the order of its type:
 *
 * - a struct: its members in order, those of its base first; before an
 *   optional member, rdds_out_present(true or false), then its value
 *   only if present;
 * - a union: rdds_out_discriminator, then the value of the member it
 *   selects, if any;
 * - a sequence: rdds_out_length, then its elements; a map: its length,
 *   then key, value, key, value...;
 * - an array: its elements, every dimension row by row;
 * - the elements of a sequence or array of a primitive type other than
 *   float128 go in one call, rdds_out_<primitive>_array (after the length
 *   of a sequence); every element of an array of several dimensions;
 * - an enum: rdds_out_enum with the value of the literal; a bitmask:
 *   rdds_out_bitmask with its bits;
 * - typedefs and @external are transparent.
 *
 * An rdds_out does not know the type: a writer checks the values against
 * the type of its topic and fails with RDDS_RETCODE_BAD_PARAMETER when
 * they do not fit. An rdds_in gives the values in the same order; a read
 * that does not fit the sample returns zero and sets an error that sticks
 * (rdds_in_status).
 */

typedef struct rdds_out rdds_out;
typedef struct rdds_in rdds_in;

rdds_status rdds_out_new(rdds_out **out);
/* Empties the stream, to write the next sample. */
void rdds_out_clear(rdds_out *s);
void rdds_out_release(rdds_out *s);

void rdds_out_boolean(rdds_out *s, bool v);
void rdds_out_octet(rdds_out *s, uint8_t v);
void rdds_out_int8(rdds_out *s, int8_t v);
void rdds_out_uint8(rdds_out *s, uint8_t v);
void rdds_out_int16(rdds_out *s, int16_t v);
void rdds_out_uint16(rdds_out *s, uint16_t v);
void rdds_out_int32(rdds_out *s, int32_t v);
void rdds_out_uint32(rdds_out *s, uint32_t v);
void rdds_out_int64(rdds_out *s, int64_t v);
void rdds_out_uint64(rdds_out *s, uint64_t v);
void rdds_out_float32(rdds_out *s, float v);
void rdds_out_float64(rdds_out *s, double v);
/* The IEEE 754 binary128 bits. */
void rdds_out_float128(rdds_out *s, uint64_t high, uint64_t low);
/* char8 as one octet (ISO-8859-1), char16 as one UTF-16 code unit. */
void rdds_out_char8(rdds_out *s, char v);
void rdds_out_char16(rdds_out *s, uint16_t v);
void rdds_out_string(rdds_out *s, const char *v, size_t len);
void rdds_out_wstring(rdds_out *s, const uint16_t *v, size_t len);
void rdds_out_length(rdds_out *s, uint32_t v);
void rdds_out_present(rdds_out *s, bool v);
void rdds_out_discriminator(rdds_out *s, int64_t v);
void rdds_out_enum(rdds_out *s, int32_t v);
void rdds_out_bitmask(rdds_out *s, uint64_t v);

void rdds_out_boolean_array(rdds_out *s, const bool *v, size_t n);
void rdds_out_octet_array(rdds_out *s, const uint8_t *v, size_t n);
void rdds_out_int8_array(rdds_out *s, const int8_t *v, size_t n);
void rdds_out_uint8_array(rdds_out *s, const uint8_t *v, size_t n);
void rdds_out_int16_array(rdds_out *s, const int16_t *v, size_t n);
void rdds_out_uint16_array(rdds_out *s, const uint16_t *v, size_t n);
void rdds_out_int32_array(rdds_out *s, const int32_t *v, size_t n);
void rdds_out_uint32_array(rdds_out *s, const uint32_t *v, size_t n);
void rdds_out_int64_array(rdds_out *s, const int64_t *v, size_t n);
void rdds_out_uint64_array(rdds_out *s, const uint64_t *v, size_t n);
void rdds_out_float32_array(rdds_out *s, const float *v, size_t n);
void rdds_out_float64_array(rdds_out *s, const double *v, size_t n);
void rdds_out_char8_array(rdds_out *s, const char *v, size_t n);
void rdds_out_char16_array(rdds_out *s, const uint16_t *v, size_t n);

/* RDDS_RETCODE_OK, or RDDS_RETCODE_BAD_PARAMETER after a read that did
 * not fit (the message set as for any failure). */
rdds_status rdds_in_status(const rdds_in *s);
/* Whether every value was read. */
bool rdds_in_at_end(const rdds_in *s);
void rdds_in_release(rdds_in *s);

bool rdds_in_boolean(rdds_in *s);
uint8_t rdds_in_octet(rdds_in *s);
int8_t rdds_in_int8(rdds_in *s);
uint8_t rdds_in_uint8(rdds_in *s);
int16_t rdds_in_int16(rdds_in *s);
uint16_t rdds_in_uint16(rdds_in *s);
int32_t rdds_in_int32(rdds_in *s);
uint32_t rdds_in_uint32(rdds_in *s);
int64_t rdds_in_int64(rdds_in *s);
uint64_t rdds_in_uint64(rdds_in *s);
float rdds_in_float32(rdds_in *s);
double rdds_in_float64(rdds_in *s);
void rdds_in_float128(rdds_in *s, uint64_t *high, uint64_t *low);
char rdds_in_char8(rdds_in *s);
uint16_t rdds_in_char16(rdds_in *s);
/* Not NUL-terminated; valid until the stream is released. */
void rdds_in_string(rdds_in *s, const char **v, size_t *len);
void rdds_in_wstring(rdds_in *s, const uint16_t **v, size_t *len);
uint32_t rdds_in_length(rdds_in *s);
bool rdds_in_present(rdds_in *s);
int64_t rdds_in_discriminator(rdds_in *s);
int32_t rdds_in_enum(rdds_in *s);
uint64_t rdds_in_bitmask(rdds_in *s);

/* Exactly n elements (the length read before, or the array's size). */
void rdds_in_boolean_array(rdds_in *s, bool *v, size_t n);
void rdds_in_octet_array(rdds_in *s, uint8_t *v, size_t n);
void rdds_in_int8_array(rdds_in *s, int8_t *v, size_t n);
void rdds_in_uint8_array(rdds_in *s, uint8_t *v, size_t n);
void rdds_in_int16_array(rdds_in *s, int16_t *v, size_t n);
void rdds_in_uint16_array(rdds_in *s, uint16_t *v, size_t n);
void rdds_in_int32_array(rdds_in *s, int32_t *v, size_t n);
void rdds_in_uint32_array(rdds_in *s, uint32_t *v, size_t n);
void rdds_in_int64_array(rdds_in *s, int64_t *v, size_t n);
void rdds_in_uint64_array(rdds_in *s, uint64_t *v, size_t n);
void rdds_in_float32_array(rdds_in *s, float *v, size_t n);
void rdds_in_float64_array(rdds_in *s, double *v, size_t n);
void rdds_in_char8_array(rdds_in *s, char *v, size_t n);
void rdds_in_char16_array(rdds_in *s, uint16_t *v, size_t n);

/* A sample's serialized payload with its encapsulation header (XCDR1, or
 * XCDR2 with xcdr2, little endian), freed with rdds_bytes_free; and back.
 * For tests and tools. */
rdds_status rdds_type_serialize(const rdds_type *t, const rdds_out *sample,
                                bool xcdr2, uint8_t **data, size_t *len);
rdds_status rdds_type_deserialize(const rdds_type *t, const uint8_t *data,
                                  size_t len, rdds_in **out);
void rdds_bytes_free(uint8_t *data, size_t len);

/* ------------------------------------------------------------------ */
/* QoS                                                                */

#define RDDS_LENGTH_UNLIMITED (-1)

#define RDDS_BEST_EFFORT_RELIABILITY_QOS 0
#define RDDS_RELIABLE_RELIABILITY_QOS 1
#define RDDS_KEEP_LAST_HISTORY_QOS 0
#define RDDS_KEEP_ALL_HISTORY_QOS 1
#define RDDS_VOLATILE_DURABILITY_QOS 0
#define RDDS_TRANSIENT_LOCAL_DURABILITY_QOS 1
#define RDDS_TRANSIENT_DURABILITY_QOS 2
#define RDDS_PERSISTENT_DURABILITY_QOS 3
#define RDDS_AUTOMATIC_LIVELINESS_QOS 0
#define RDDS_MANUAL_BY_PARTICIPANT_LIVELINESS_QOS 1
#define RDDS_MANUAL_BY_TOPIC_LIVELINESS_QOS 2
#define RDDS_DISALLOW_TYPE_COERCION 0
#define RDDS_ALLOW_TYPE_COERCION 1
#define RDDS_XCDR_DATA_REPRESENTATION 0
#define RDDS_XCDR2_DATA_REPRESENTATION 2

/* Start from rdds_writer_qos_default. The arrays are read during the
 * call that takes the QoS. An empty data_representation: XCDR. */
typedef struct rdds_writer_qos {
  int32_t reliability_kind;
  rdds_duration max_blocking_time;
  int32_t history_kind;
  int32_t history_depth;
  int32_t max_samples;
  int32_t max_instances;
  int32_t max_samples_per_instance;
  int32_t durability_kind;
  rdds_duration deadline_period;
  int32_t liveliness_kind;
  rdds_duration lease_duration;
  const uint8_t *user_data;
  size_t user_data_len;
  const int16_t *data_representation;
  size_t data_representation_len;
  bool autodispose_unregistered_instances;
} rdds_writer_qos;

typedef struct rdds_reader_qos {
  int32_t reliability_kind;
  rdds_duration max_blocking_time;
  int32_t history_kind;
  int32_t history_depth;
  int32_t max_samples;
  int32_t max_instances;
  int32_t max_samples_per_instance;
  int32_t durability_kind;
  rdds_duration deadline_period;
  int32_t liveliness_kind;
  rdds_duration lease_duration;
  const uint8_t *user_data;
  size_t user_data_len;
  const int16_t *data_representation;
  size_t data_representation_len;
  int32_t type_consistency_kind;
  bool ignore_sequence_bounds;
  bool ignore_string_bounds;
  bool ignore_member_names;
  bool prevent_type_widening;
  bool force_type_validation;
} rdds_reader_qos;

/* The PARTITION of a publisher or subscriber. */
typedef struct rdds_group_qos {
  const char *const *partition;
  size_t partition_len;
} rdds_group_qos;

/* A property of the PROPERTY QoS (DDS Security 1.2, 7.2.5). */
typedef struct rdds_property {
  const char *name;
  const char *value;
} rdds_property;

/* property: the dds.sec.* properties of the builtin plugins
   (dds.sec.auth.identity_ca, dds.sec.auth.identity_certificate,
   dds.sec.auth.private_key, dds.sec.access.permissions_ca,
   dds.sec.access.governance, dds.sec.access.permissions; values are
   file: or data:, URIs) turn on DDS Security; all six are needed. Other
   properties are ignored. */
typedef struct rdds_participant_qos {
  const uint8_t *user_data;
  size_t user_data_len;
  const rdds_property *property;
  size_t property_len;
} rdds_participant_qos;

rdds_status rdds_writer_qos_default(rdds_writer_qos *out);
/* data_representation points to static storage. */
rdds_status rdds_reader_qos_default(rdds_reader_qos *out);

/* ------------------------------------------------------------------ */
/* Statuses                                                           */

/* StatusKind bits (DDS 1.4, 2.2.4.1). */
#define RDDS_INCONSISTENT_TOPIC_STATUS (1u << 0)
#define RDDS_OFFERED_DEADLINE_MISSED_STATUS (1u << 1)
#define RDDS_REQUESTED_DEADLINE_MISSED_STATUS (1u << 2)
#define RDDS_OFFERED_INCOMPATIBLE_QOS_STATUS (1u << 5)
#define RDDS_REQUESTED_INCOMPATIBLE_QOS_STATUS (1u << 6)
#define RDDS_SAMPLE_LOST_STATUS (1u << 7)
#define RDDS_SAMPLE_REJECTED_STATUS (1u << 8)
#define RDDS_DATA_ON_READERS_STATUS (1u << 9)
#define RDDS_DATA_AVAILABLE_STATUS (1u << 10)
#define RDDS_LIVELINESS_LOST_STATUS (1u << 11)
#define RDDS_LIVELINESS_CHANGED_STATUS (1u << 12)
#define RDDS_PUBLICATION_MATCHED_STATUS (1u << 13)
#define RDDS_SUBSCRIPTION_MATCHED_STATUS (1u << 14)
#define RDDS_ALL_STATUS 0xffffffffu

typedef struct rdds_matched_status {
  int32_t total_count;
  int32_t total_count_change;
  int32_t current_count;
  int32_t current_count_change;
  rdds_guid last_handle;
} rdds_matched_status;

typedef struct rdds_qos_policy_count {
  int32_t policy_id;
  int32_t count;
} rdds_qos_policy_count;

#define RDDS_MAX_POLICIES 32

typedef struct rdds_incompatible_qos_status {
  int32_t total_count;
  int32_t total_count_change;
  int32_t last_policy_id;
  uint32_t policies_len;
  rdds_qos_policy_count policies[RDDS_MAX_POLICIES];
} rdds_incompatible_qos_status;

typedef struct rdds_liveliness_changed_status {
  int32_t alive_count;
  int32_t not_alive_count;
  int32_t alive_count_change;
  int32_t not_alive_count_change;
  rdds_guid last_publication_handle;
} rdds_liveliness_changed_status;

/* LIVELINESS_LOST, SAMPLE_LOST and INCONSISTENT_TOPIC. */
typedef struct rdds_count_status {
  int32_t total_count;
  int32_t total_count_change;
} rdds_count_status;

typedef struct rdds_deadline_missed_status {
  int32_t total_count;
  int32_t total_count_change;
  rdds_instance_handle last_instance_handle;
} rdds_deadline_missed_status;

#define RDDS_NOT_REJECTED 0
#define RDDS_REJECTED_BY_INSTANCES_LIMIT 1
#define RDDS_REJECTED_BY_SAMPLES_LIMIT 2
#define RDDS_REJECTED_BY_SAMPLES_PER_INSTANCE_LIMIT 3

typedef struct rdds_sample_rejected_status {
  int32_t total_count;
  int32_t total_count_change;
  int32_t last_reason;
  rdds_instance_handle last_instance_handle;
} rdds_sample_rejected_status;

/* Any function may be NULL; release, if not NULL, is called once when the
 * listener is replaced or cleared or its entity is deleted. */
typedef struct rdds_writer_listener {
  void *self;
  void (*on_offered_deadline_missed)(void *self,
                                     const rdds_deadline_missed_status *s);
  void (*on_offered_incompatible_qos)(void *self,
                                      const rdds_incompatible_qos_status *s);
  void (*on_liveliness_lost)(void *self, const rdds_count_status *s);
  void (*on_publication_matched)(void *self, const rdds_matched_status *s);
  void (*release)(void *self);
} rdds_writer_listener;

typedef struct rdds_reader_listener {
  void *self;
  void (*on_requested_deadline_missed)(void *self,
                                       const rdds_deadline_missed_status *s);
  void (*on_requested_incompatible_qos)(void *self,
                                        const rdds_incompatible_qos_status *s);
  void (*on_sample_rejected)(void *self, const rdds_sample_rejected_status *s);
  void (*on_liveliness_changed)(void *self,
                                const rdds_liveliness_changed_status *s);
  void (*on_data_available)(void *self);
  void (*on_subscription_matched)(void *self, const rdds_matched_status *s);
  void (*on_sample_lost)(void *self, const rdds_count_status *s);
  void (*release)(void *self);
} rdds_reader_listener;

typedef struct rdds_subscriber rdds_subscriber;

/* The writer or reader a status is of, given to an rdds_listener; the
 * strings are valid for the call. */
typedef struct rdds_endpoint_info {
  rdds_guid guid;
  const char *topic_name;
  const char *type_name;
} rdds_endpoint_info;

/* The listener of a participant, publisher, subscriber or topic. It takes
 * the statuses of the writers and readers that their own listeners do
 * not (DDS 1.4, 2.2.4.3.1): a writer's go to its publisher's listener,
 * else the participant's; a reader's to its subscriber's, else the
 * participant's; INCONSISTENT_TOPIC to the topic's, else the
 * participant's. New data goes to on_data_on_readers of the subscriber's
 * listener, else the participant's, and only when neither takes
 * DATA_ON_READERS to on_data_available (2.2.4.3.2); the subscriber passed
 * to on_data_on_readers is valid for the call (rdds_subscriber_clone
 * keeps it). Functions that do not fit the entity are not called; any
 * may be NULL; release as for rdds_writer_listener. */
typedef struct rdds_listener {
  void *self;
  void (*on_offered_deadline_missed)(void *self, const rdds_endpoint_info *w,
                                     const rdds_deadline_missed_status *s);
  void (*on_offered_incompatible_qos)(void *self, const rdds_endpoint_info *w,
                                      const rdds_incompatible_qos_status *s);
  void (*on_liveliness_lost)(void *self, const rdds_endpoint_info *w,
                             const rdds_count_status *s);
  void (*on_publication_matched)(void *self, const rdds_endpoint_info *w,
                                 const rdds_matched_status *s);
  void (*on_data_on_readers)(void *self, const rdds_subscriber *s);
  void (*on_requested_deadline_missed)(void *self, const rdds_endpoint_info *r,
                                       const rdds_deadline_missed_status *s);
  void (*on_requested_incompatible_qos)(void *self,
                                        const rdds_endpoint_info *r,
                                        const rdds_incompatible_qos_status *s);
  void (*on_sample_rejected)(void *self, const rdds_endpoint_info *r,
                             const rdds_sample_rejected_status *s);
  void (*on_liveliness_changed)(void *self, const rdds_endpoint_info *r,
                                const rdds_liveliness_changed_status *s);
  void (*on_data_available)(void *self, const rdds_endpoint_info *r);
  void (*on_subscription_matched)(void *self, const rdds_endpoint_info *r,
                                  const rdds_matched_status *s);
  void (*on_sample_lost)(void *self, const rdds_endpoint_info *r,
                         const rdds_count_status *s);
  void (*on_inconsistent_topic)(void *self, const char *topic_name,
                                const rdds_count_status *s);
  void (*release)(void *self);
} rdds_listener;

/* ------------------------------------------------------------------ */
/* Entities                                                           */

typedef struct rdds_participant rdds_participant;
typedef struct rdds_topic rdds_topic;
typedef struct rdds_cftopic rdds_cftopic;
typedef struct rdds_publisher rdds_publisher;
typedef struct rdds_writer rdds_writer;
typedef struct rdds_reader rdds_reader;
typedef struct rdds_samples rdds_samples;
typedef struct rdds_condition rdds_condition;
typedef struct rdds_waitset rdds_waitset;

/* Only the loopback interface, for tests. */
#define RDDS_PARTICIPANT_LOCALHOST 1u

/* qos: NULL for the default. */
rdds_status rdds_participant_new(uint32_t domain_id,
                                 const rdds_participant_qos *qos,
                                 uint32_t flags, rdds_participant **out);
void rdds_participant_release(rdds_participant *p);
uint32_t rdds_participant_domain_id(const rdds_participant *p);
rdds_status rdds_participant_guid(const rdds_participant *p, rdds_guid *out);
rdds_status rdds_participant_assert_liveliness(const rdds_participant *p);
/* How many other participants are known. */
size_t rdds_participant_discovered_participants(const rdds_participant *p);

/* A topic of the type, registered under the type's name. */
rdds_status rdds_participant_create_topic(const rdds_participant *p,
                                          const char *name,
                                          const rdds_type *type,
                                          rdds_topic **out);
void rdds_topic_release(rdds_topic *t);
rdds_status rdds_topic_inconsistent_topic_status(const rdds_topic *t,
                                                 rdds_count_status *out);

/* A content filtered topic of related (DDS 1.4, 2.2.2.3.3): its readers
 * get the samples that pass the filter expression (DDS 1.4, Annex B),
 * with n parameters for its %0 .. %99. RDDS_RETCODE_BAD_PARAMETER for an
 * expression not valid for the type. */
rdds_status rdds_participant_create_contentfilteredtopic(
    const rdds_participant *p, const char *name, const rdds_topic *related,
    const char *expression, const char *const *parameters, size_t n,
    rdds_cftopic **out);
void rdds_cftopic_release(rdds_cftopic *t);
rdds_status rdds_cftopic_set_expression_parameters(
    const rdds_cftopic *t, const char *const *parameters, size_t n);

/* qos: NULL for the default. */
rdds_status rdds_participant_create_publisher(const rdds_participant *p,
                                              const rdds_group_qos *qos,
                                              rdds_publisher **out);
void rdds_publisher_release(rdds_publisher *p);
rdds_status rdds_participant_create_subscriber(const rdds_participant *p,
                                               const rdds_group_qos *qos,
                                               rdds_subscriber **out);
void rdds_subscriber_release(rdds_subscriber *s);

/* Listeners of the participant, publisher, subscriber and topic (see
 * rdds_listener); listener NULL clears it. */
rdds_status rdds_participant_set_listener(const rdds_participant *p,
                                          const rdds_listener *listener,
                                          uint32_t mask);
rdds_status rdds_publisher_set_listener(const rdds_publisher *p,
                                        const rdds_listener *listener,
                                        uint32_t mask);
rdds_status rdds_subscriber_set_listener(const rdds_subscriber *s,
                                         const rdds_listener *listener,
                                         uint32_t mask);
rdds_status rdds_topic_set_listener(const rdds_topic *t,
                                    const rdds_listener *listener,
                                    uint32_t mask);
/* Calls on_data_available of the listeners of the subscriber's readers
 * that have new data (notify_datareaders). */
rdds_status rdds_subscriber_notify_datareaders(const rdds_subscriber *s);
/* Another handle of the same subscriber, to release on its own. */
rdds_status rdds_subscriber_clone(const rdds_subscriber *s,
                                  rdds_subscriber **out);
/* Whether two handles are of the same subscriber. */
bool rdds_subscriber_same(const rdds_subscriber *a, const rdds_subscriber *b);

/* Writers. qos and listener may be NULL; the listener is set before the
 * writer is matched. timestamp NULL: now. */
rdds_status rdds_publisher_create_writer(const rdds_publisher *p,
                                         const rdds_topic *topic,
                                         const rdds_writer_qos *qos,
                                         const rdds_writer_listener *listener,
                                         uint32_t mask, rdds_writer **out);
void rdds_writer_release(rdds_writer *w);
rdds_status rdds_writer_guid(const rdds_writer *w, rdds_guid *out);
rdds_status rdds_writer_write(const rdds_writer *w, const rdds_out *sample,
                              const rdds_time *timestamp);
rdds_status rdds_writer_register_instance(const rdds_writer *w,
                                          const rdds_out *sample,
                                          rdds_instance_handle *out);
rdds_status rdds_writer_unregister_instance(const rdds_writer *w,
                                            const rdds_out *sample,
                                            const rdds_time *timestamp);
rdds_status rdds_writer_dispose(const rdds_writer *w, const rdds_out *sample,
                                const rdds_time *timestamp);
/* HANDLE_NIL when the instance is not known. */
rdds_status rdds_writer_lookup_instance(const rdds_writer *w,
                                        const rdds_out *sample,
                                        rdds_instance_handle *out);
rdds_status rdds_writer_wait_for_acknowledgments(const rdds_writer *w,
                                                 rdds_duration max_wait);
/* Until count readers match, or RDDS_RETCODE_TIMEOUT. */
rdds_status rdds_writer_wait_for_subscriptions(const rdds_writer *w,
                                               size_t count,
                                               rdds_duration timeout);
/* Up to cap into buf; how many there are into *len. */
rdds_status rdds_writer_matched_subscriptions(const rdds_writer *w,
                                              rdds_guid *buf, size_t cap,
                                              size_t *len);
rdds_status rdds_writer_assert_liveliness(const rdds_writer *w);
/* The status getters reset the change counts. */
rdds_status rdds_writer_publication_matched_status(const rdds_writer *w,
                                                   rdds_matched_status *out);
rdds_status rdds_writer_offered_incompatible_qos_status(
    const rdds_writer *w, rdds_incompatible_qos_status *out);
rdds_status rdds_writer_liveliness_lost_status(const rdds_writer *w,
                                               rdds_count_status *out);
rdds_status rdds_writer_offered_deadline_missed_status(
    const rdds_writer *w, rdds_deadline_missed_status *out);
uint32_t rdds_writer_status_changes(const rdds_writer *w);
rdds_status rdds_writer_status_condition(const rdds_writer *w,
                                         rdds_condition **out);
/* listener NULL: clears it. */
rdds_status rdds_writer_set_listener(const rdds_writer *w,
                                     const rdds_writer_listener *listener,
                                     uint32_t mask);

/* Readers. */
rdds_status rdds_subscriber_create_reader(const rdds_subscriber *s,
                                          const rdds_topic *topic,
                                          const rdds_reader_qos *qos,
                                          const rdds_reader_listener *listener,
                                          uint32_t mask, rdds_reader **out);
/* A reader of a content filtered topic. */
rdds_status rdds_subscriber_create_filtered_reader(
    const rdds_subscriber *s, const rdds_cftopic *topic,
    const rdds_reader_qos *qos, const rdds_reader_listener *listener,
    uint32_t mask, rdds_reader **out);
void rdds_reader_release(rdds_reader *r);
rdds_status rdds_reader_guid(const rdds_reader *r, rdds_guid *out);

/* Sample, view and instance states (DDS 1.4, 2.2.2.5.1.8). */
#define RDDS_READ_SAMPLE_STATE 1u
#define RDDS_NOT_READ_SAMPLE_STATE 2u
#define RDDS_ANY_SAMPLE_STATE 3u
#define RDDS_NEW_VIEW_STATE 1u
#define RDDS_NOT_NEW_VIEW_STATE 2u
#define RDDS_ANY_VIEW_STATE 3u
#define RDDS_ALIVE_INSTANCE_STATE 1u
#define RDDS_NOT_ALIVE_DISPOSED_INSTANCE_STATE 2u
#define RDDS_NOT_ALIVE_NO_WRITERS_INSTANCE_STATE 4u
#define RDDS_NOT_ALIVE_INSTANCE_STATE 6u
#define RDDS_ANY_INSTANCE_STATE 7u

#define RDDS_SELECT_ANY 0
#define RDDS_SELECT_INSTANCE 1
#define RDDS_SELECT_NEXT_INSTANCE 2

typedef struct rdds_sample_info {
  uint32_t sample_state;
  uint32_t view_state;
  uint32_t instance_state;
  int32_t disposed_generation_count;
  int32_t no_writers_generation_count;
  int32_t sample_rank;
  int32_t generation_rank;
  int32_t absolute_generation_rank;
  rdds_time source_timestamp;
  rdds_instance_handle instance_handle;
  rdds_guid publication_handle;
  int64_t sequence_number;
  bool valid_data;
} rdds_sample_info;

/* Reads (or takes) at most max_samples (RDDS_LENGTH_UNLIMITED for any
 * number) of the samples in the states of the masks, the first to arrive.
 * With
 * RDDS_SELECT_INSTANCE, those of *instance (the masks ignored); with
 * RDDS_SELECT_NEXT_INSTANCE, those of the instance after *instance (NULL
 * or HANDLE_NIL for the first). No samples is RDDS_RETCODE_OK with an
 * empty rdds_samples. */
rdds_status rdds_reader_select(const rdds_reader *r, bool take,
                               int32_t max_samples, uint32_t sample_states, uint32_t view_states,
                               uint32_t instance_states, int32_t mode,
                               const rdds_instance_handle *instance,
                               rdds_samples **out);
rdds_status rdds_reader_select_w_condition(const rdds_reader *r, bool take,
                                           int32_t max_samples,
                                           const rdds_condition *condition,
                                           rdds_samples **out);
size_t rdds_samples_count(const rdds_samples *s);
rdds_status rdds_samples_info(const rdds_samples *s, size_t i,
                              rdds_sample_info *out);
/* A new stream of sample i's values; RDDS_RETCODE_NO_DATA for a sample
 * without data (valid_data false). */
rdds_status rdds_samples_data(const rdds_samples *s, size_t i, rdds_in **out);
void rdds_samples_release(rdds_samples *s);

rdds_status rdds_reader_lookup_instance(const rdds_reader *r,
                                        const rdds_out *sample,
                                        rdds_instance_handle *out);
rdds_status rdds_reader_get_key_value(const rdds_reader *r,
                                      rdds_instance_handle instance,
                                      rdds_in **out);
/* Until a sample is there, or RDDS_RETCODE_TIMEOUT. */
rdds_status rdds_reader_wait_for_data(const rdds_reader *r,
                                      rdds_duration timeout);
rdds_status rdds_reader_wait_for_publications(const rdds_reader *r,
                                              size_t count,
                                              rdds_duration timeout);
rdds_status rdds_reader_matched_publications(const rdds_reader *r,
                                             rdds_guid *buf, size_t cap,
                                             size_t *len);
rdds_status rdds_reader_subscription_matched_status(const rdds_reader *r,
                                                    rdds_matched_status *out);
rdds_status rdds_reader_requested_incompatible_qos_status(
    const rdds_reader *r, rdds_incompatible_qos_status *out);
rdds_status rdds_reader_liveliness_changed_status(
    const rdds_reader *r, rdds_liveliness_changed_status *out);
rdds_status rdds_reader_requested_deadline_missed_status(
    const rdds_reader *r, rdds_deadline_missed_status *out);
rdds_status rdds_reader_sample_lost_status(const rdds_reader *r,
                                           rdds_count_status *out);
rdds_status rdds_reader_sample_rejected_status(
    const rdds_reader *r, rdds_sample_rejected_status *out);
uint32_t rdds_reader_status_changes(const rdds_reader *r);
rdds_status rdds_reader_status_condition(const rdds_reader *r,
                                         rdds_condition **out);
rdds_status rdds_reader_create_readcondition(const rdds_reader *r,
                                             uint32_t sample_states,
                                             uint32_t view_states,
                                             uint32_t instance_states,
                                             rdds_condition **out);
rdds_status rdds_reader_set_listener(const rdds_reader *r,
                                     const rdds_reader_listener *listener,
                                     uint32_t mask);

/* ------------------------------------------------------------------ */
/* Conditions and WaitSets                                            */

/* Requests and replies (DDS-RPC 1.0). A request is known by its
 * identity: the writer and sequence number of the request, which the
 * replies name. */
typedef struct rdds_sample_identity {
    rdds_guid writer;
    int64_t sequence_number;
} rdds_sample_identity;

/* The related sample identity of sample i (what a reply names), or
 * RDDS_RETCODE_NO_DATA. */
rdds_status rdds_samples_related_sample_identity(const rdds_samples *s,
                                                 size_t i,
                                                 rdds_sample_identity *out);

typedef struct rdds_requester rdds_requester;
typedef struct rdds_replier rdds_replier;

/* A requester sending requests on the topic request and getting the
 * replies on reply; the QoS NULL for the default (RELIABLE, KEEP_ALL). */
rdds_status rdds_requester_new(const rdds_participant *p,
                               const rdds_topic *request,
                               const rdds_topic *reply,
                               const rdds_writer_qos *writer_qos,
                               const rdds_reader_qos *reader_qos,
                               rdds_requester **out);
void rdds_requester_release(rdds_requester *r);
/* Until a replier is matched (both of its endpoints), or
 * RDDS_RETCODE_TIMEOUT. */
rdds_status rdds_requester_wait_for_service(const rdds_requester *r,
                                            rdds_duration timeout);
/* The request's identity into *out (NULL to ignore it). */
rdds_status rdds_requester_send_request(const rdds_requester *r,
                                        const rdds_out *sample,
                                        rdds_sample_identity *out);
/* Waits for a reply to request, or RDDS_RETCODE_TIMEOUT. */
rdds_status rdds_requester_receive_reply(const rdds_requester *r,
                                         const rdds_sample_identity *request,
                                         rdds_duration timeout,
                                         rdds_in **out);

/* A replier taking requests on the topic request and answering on reply;
 * the QoS NULL for the default (RELIABLE, KEEP_ALL), instance_name NULL
 * for none. */
rdds_status rdds_replier_new(const rdds_participant *p,
                             const rdds_topic *request,
                             const rdds_topic *reply,
                             const rdds_writer_qos *writer_qos,
                             const rdds_reader_qos *reader_qos,
                             const char *instance_name, rdds_replier **out);
void rdds_replier_release(rdds_replier *r);
/* Waits for a request, or RDDS_RETCODE_TIMEOUT: the request into *out,
 * what its replies name into *id. */
rdds_status rdds_replier_receive_request(const rdds_replier *r,
                                         rdds_duration timeout,
                                         rdds_in **out,
                                         rdds_sample_identity *id);
/* Sends sample as a reply to the request id. */
rdds_status rdds_replier_send_reply(const rdds_replier *r,
                                    const rdds_out *sample,
                                    const rdds_sample_identity *id);

rdds_status rdds_guard_condition_new(rdds_condition **out);
void rdds_condition_release(rdds_condition *c);
bool rdds_condition_trigger_value(const rdds_condition *c);
/* Whether two handles are the same condition. */
bool rdds_condition_equals(const rdds_condition *a, const rdds_condition *b);
rdds_status rdds_guard_condition_set_trigger_value(const rdds_condition *c,
                                                   bool value);
rdds_status rdds_status_condition_get_enabled_statuses(const rdds_condition *c,
                                                       uint32_t *out);
rdds_status rdds_status_condition_set_enabled_statuses(const rdds_condition *c,
                                                       uint32_t mask);

rdds_status rdds_waitset_new(rdds_waitset **out);
void rdds_waitset_release(rdds_waitset *w);
rdds_status rdds_waitset_attach_condition(const rdds_waitset *w,
                                          const rdds_condition *c);
rdds_status rdds_waitset_detach_condition(const rdds_waitset *w,
                                          const rdds_condition *c);
/* Until a condition is true, or RDDS_RETCODE_TIMEOUT. The indices of the
 * true ones, in the order the attached conditions were attached, go into
 * active (up to cap); how many there are into *len. */
rdds_status rdds_waitset_wait(const rdds_waitset *w, rdds_duration timeout,
                              size_t *active, size_t cap, size_t *len);

#ifdef __cplusplus
}
#endif

#endif /* RUSTORB_DDS_H */
