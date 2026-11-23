# frozen_string_literal: true

# The QoS policies (dds::core::policy of the C++ PSM) and the QoS of the
# entities. A policy is a frozen value; a QoS holds one of each of its
# policies: start from the default (DDS::Pub::Publisher#default_datawriter_qos,
# DDS::Pub::DataWriterQos.new...) and set policies with <<.
#
# Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

module DDS
  module Core
    module Policy
      # Freezes a policy once it is made.
      module Frozen
        def initialize(*)
          super
          freeze
        end
      end

      # A Struct whose values cannot change.
      def self.value(*members, &blk)
        cls = ::Struct.new(*members)
        cls.class_eval(&blk) if blk
        cls.prepend(Frozen)
        cls
      end

      module ReliabilityKind
        BEST_EFFORT = 0
        RELIABLE = 1
      end

      Reliability = value(:kind, :max_blocking_time) do
        def self.best_effort = new(ReliabilityKind::BEST_EFFORT, Duration.from_millisecs(100))
        def self.reliable(max_blocking_time = Duration.from_millisecs(100))
          new(ReliabilityKind::RELIABLE, Duration.of(max_blocking_time))
        end
      end

      module HistoryKind
        KEEP_LAST = 0
        KEEP_ALL = 1
      end

      History = value(:kind, :depth) do
        def self.keep_last(depth = 1) = new(HistoryKind::KEEP_LAST, depth)
        def self.keep_all = new(HistoryKind::KEEP_ALL, 1)
      end

      LENGTH_UNLIMITED = -1

      ResourceLimits = value(:max_samples, :max_instances, :max_samples_per_instance) do
        def self.unlimited = new(LENGTH_UNLIMITED, LENGTH_UNLIMITED, LENGTH_UNLIMITED)
      end

      module DurabilityKind
        VOLATILE = 0
        TRANSIENT_LOCAL = 1
        TRANSIENT = 2
        PERSISTENT = 3
      end

      Durability = value(:kind) do
        def self.volatile = new(DurabilityKind::VOLATILE)
        def self.transient_local = new(DurabilityKind::TRANSIENT_LOCAL)
        def self.transient = new(DurabilityKind::TRANSIENT)
        def self.persistent = new(DurabilityKind::PERSISTENT)
      end

      Deadline = value(:period)

      module LivelinessKind
        AUTOMATIC = 0
        MANUAL_BY_PARTICIPANT = 1
        MANUAL_BY_TOPIC = 2
      end

      Liveliness = value(:kind, :lease_duration) do
        def self.automatic(lease = Duration.infinite) = new(LivelinessKind::AUTOMATIC, Duration.of(lease))
        def self.manual_by_participant(lease = Duration.infinite)
          new(LivelinessKind::MANUAL_BY_PARTICIPANT, Duration.of(lease))
        end
        def self.manual_by_topic(lease = Duration.infinite) = new(LivelinessKind::MANUAL_BY_TOPIC, Duration.of(lease))
      end

      # The octets of USER_DATA, as a binary String.
      UserData = value(:value) do
        def initialize(value = "")
          super(value.b.freeze)
        end
      end

      module DataRepresentationId
        XCDR = 0
        XML = 1
        XCDR2 = 2
      end

      DataRepresentation = value(:value) do
        def initialize(value = [])
          super(Array(value).dup.freeze)
        end

        def self.xcdr = new([DataRepresentationId::XCDR])
        def self.xcdr2 = new([DataRepresentationId::XCDR2])
      end

      WriterDataLifecycle = value(:autodispose_unregistered_instances) do
        def self.auto_dispose_unregistered_instances = new(true)
        def self.manually_dispose_unregistered_instances = new(false)
      end

      # DURABILITY_SERVICE of a TRANSIENT or PERSISTENT writer.
      DurabilityService = value(:service_cleanup_delay, :history_kind, :history_depth, :max_samples, :max_instances,
                                :max_samples_per_instance)

      module TypeConsistencyKind
        DISALLOW_TYPE_COERCION = 0
        ALLOW_TYPE_COERCION = 1
      end

      TypeConsistencyEnforcement = value(:kind, :ignore_sequence_bounds, :ignore_string_bounds,
                                         :ignore_member_names, :prevent_type_widening, :force_type_validation)

      # The partitions of a publisher or subscriber.
      Partition = value(:name) do
        def initialize(name = [])
          super(Array(name).map { |n| n.to_s.dup.freeze }.freeze)
        end
      end

      # The PROPERTY QoS (DDS Security 1.2, 7.2.5): names and values. The
      # dds.sec.* properties turn on DDS Security.
      Property = value(:value) do
        def initialize(value = {})
          super(value.to_h { |k, v| [k.to_s.dup.freeze, v.to_s.dup.freeze] }.freeze)
        end
      end
    end

    # A QoS: one of each of its policies (Class => policy).
    class EntityQos
      include Policy

      def initialize(policies)
        @policies = policies
      end

      def self.policies(*names)
        names.each do |cls, name|
          define_method(name) { @policies.fetch(cls) }
          define_method("#{name}=") { |v| self << v }
        end
        const_set(:POLICIES, names.map(&:first).freeze)
      end

      # Sets a policy of this QoS; returns self.
      def <<(policy)
        unless self.class::POLICIES.include?(policy.class)
          raise ArgumentError, "a #{self.class.name.split('::').last} has no #{policy.class.name.split('::').last}"
        end

        @policies[policy.class] = policy
        self
      end

      def policy(cls)
        @policies.fetch(cls)
      end
      alias [] policy

      def initialize_copy(other)
        super
        @policies = other.instance_variable_get(:@policies).dup
      end

      def ==(other)
        other.class == self.class && other.instance_variable_get(:@policies) == @policies
      end
      alias eql? ==

      def hash
        @policies.hash
      end

      def inspect
        "#<#{self.class.name} #{@policies.values.map(&:inspect).join(', ')}>"
      end
    end

    # What the QoS of writers and readers share, from and to the C ABI's
    # struct (as a Hash).
    module EndpointQos
      include Policy

      def self.from_native(h)
        d = ->(v) { Duration.new(*v) }
        {
          Reliability => Reliability.new(h[:reliability_kind], d.call(h[:max_blocking_time])),
          History => History.new(h[:history_kind], h[:history_depth]),
          ResourceLimits => ResourceLimits.new(h[:max_samples], h[:max_instances], h[:max_samples_per_instance]),
          Durability => Durability.new(h[:durability_kind]),
          Deadline => Deadline.new(d.call(h[:deadline_period])),
          Liveliness => Liveliness.new(h[:liveliness_kind], d.call(h[:lease_duration])),
          UserData => UserData.new(h[:user_data]),
          DataRepresentation => DataRepresentation.new(h[:data_representation])
        }
      end

      def self.to_native(q)
        r = q.reliability
        hi = q.history
        rl = q.resource_limits
        lv = q.liveliness
        {
          reliability_kind: r.kind, max_blocking_time: Duration.of(r.max_blocking_time).to_native,
          history_kind: hi.kind, history_depth: hi.depth,
          max_samples: rl.max_samples, max_instances: rl.max_instances,
          max_samples_per_instance: rl.max_samples_per_instance,
          durability_kind: q.durability.kind,
          deadline_period: Duration.of(q.deadline.period).to_native,
          liveliness_kind: lv.kind, lease_duration: Duration.of(lv.lease_duration).to_native,
          user_data: q.user_data.value, data_representation: q.data_representation.value
        }
      end
    end
  end

  module Pub
    # The QoS of a writer (rustorb-dds's default to start from).
    class DataWriterQos < Core::EntityQos
      include Core::Policy

      policies [Reliability, :reliability], [History, :history], [ResourceLimits, :resource_limits],
               [Durability, :durability], [Deadline, :deadline], [Liveliness, :liveliness],
               [UserData, :user_data], [DataRepresentation, :data_representation],
               [WriterDataLifecycle, :writer_data_lifecycle], [DurabilityService, :durability_service]

      def initialize
        h = Native.writer_qos_default
        p = Core::EndpointQos.from_native(h)
        p[WriterDataLifecycle] = WriterDataLifecycle.new(h[:autodispose_unregistered_instances])
        p[DurabilityService] = DurabilityService.new(
          Core::Duration.new(*h[:service_cleanup_delay]), h[:service_history_kind], h[:service_history_depth],
          h[:service_max_samples], h[:service_max_instances], h[:service_max_samples_per_instance]
        )
        super(p)
      end

      def to_native
        ds = durability_service
        Core::EndpointQos.to_native(self).merge(
          autodispose_unregistered_instances: writer_data_lifecycle.autodispose_unregistered_instances ? true : false,
          service_cleanup_delay: Core::Duration.of(ds.service_cleanup_delay).to_native,
          service_history_kind: ds.history_kind, service_history_depth: ds.history_depth,
          service_max_samples: ds.max_samples, service_max_instances: ds.max_instances,
          service_max_samples_per_instance: ds.max_samples_per_instance
        )
      end
    end

    # The QoS of a publisher: its partitions.
    class PublisherQos < Core::EntityQos
      include Core::Policy

      policies [Partition, :partition]

      def initialize
        super({ Partition => Partition.new })
      end
    end
  end

  module Sub
    # The QoS of a reader (rustorb-dds's default to start from).
    class DataReaderQos < Core::EntityQos
      include Core::Policy

      policies [Reliability, :reliability], [History, :history], [ResourceLimits, :resource_limits],
               [Durability, :durability], [Deadline, :deadline], [Liveliness, :liveliness],
               [UserData, :user_data], [DataRepresentation, :data_representation],
               [TypeConsistencyEnforcement, :type_consistency]

      def initialize
        h = Native.reader_qos_default
        p = Core::EndpointQos.from_native(h)
        p[TypeConsistencyEnforcement] = TypeConsistencyEnforcement.new(
          h[:type_consistency_kind], h[:ignore_sequence_bounds], h[:ignore_string_bounds], h[:ignore_member_names],
          h[:prevent_type_widening], h[:force_type_validation]
        )
        super(p)
      end

      def to_native
        t = type_consistency
        Core::EndpointQos.to_native(self).merge(
          type_consistency_kind: t.kind, ignore_sequence_bounds: t.ignore_sequence_bounds ? true : false,
          ignore_string_bounds: t.ignore_string_bounds ? true : false,
          ignore_member_names: t.ignore_member_names ? true : false,
          prevent_type_widening: t.prevent_type_widening ? true : false,
          force_type_validation: t.force_type_validation ? true : false
        )
      end
    end

    # The QoS of a subscriber: its partitions.
    class SubscriberQos < Core::EntityQos
      include Core::Policy

      policies [Partition, :partition]

      def initialize
        super({ Partition => Partition.new })
      end
    end
  end

  module Domain
    # The QoS of a participant: USER_DATA and PROPERTY.
    class DomainParticipantQos < Core::EntityQos
      include Core::Policy

      policies [UserData, :user_data], [Property, :property]

      def initialize
        super({ UserData => UserData.new, Property => Property.new })
      end
    end
  end

  module Topic
    # The QoS of a topic: rustorb-dds takes none (the writers' and
    # readers' QoS are their own).
    class TopicQos < Core::EntityQos
      policies

      def initialize
        super({})
      end
    end
  end
end
