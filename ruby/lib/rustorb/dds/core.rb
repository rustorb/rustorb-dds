# frozen_string_literal: true

# DDS::Core: the exceptions, values, statuses and policies, as the C++ PSM
# (DDS-PSM-Cxx 1.0, dds::core) has them, in Ruby's names.
#
# Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

module DDS
  module Core
    # The exceptions of the C++ PSM (dds::core::Exception), for the
    # ReturnCode_t of a call.
    class Error < StandardError; end
    class AlreadyClosedError < Error; end
    class IllegalOperationError < Error; end
    class ImmutablePolicyError < Error; end
    class InconsistentPolicyError < Error; end
    class InvalidArgumentError < Error; end
    class NotEnabledError < Error; end
    class OutOfResourcesError < Error; end
    class PreconditionNotMetError < Error; end
    class TimeoutError < Error; end
    class UnsupportedError < Error; end
    # NO_DATA: no sample (a reader's key_value of an unknown instance...).
    class NoDataError < Error; end

    # By ReturnCode_t (RDDS_RETCODE_*).
    ERRORS = [nil, Error, UnsupportedError, InvalidArgumentError, PreconditionNotMetError, OutOfResourcesError,
              NotEnabledError, ImmutablePolicyError, InconsistentPolicyError, AlreadyClosedError, TimeoutError,
              NoDataError, IllegalOperationError].freeze

    # A duration: seconds and nanoseconds.
    class Duration
      include Comparable

      INFINITE_SEC = 0x7fffffff
      INFINITE_NSEC = 0x7fffffff

      attr_reader :sec, :nanosec

      def initialize(sec = 0, nanosec = 0)
        @sec = Integer(sec)
        @nanosec = Integer(nanosec)
        freeze
      end

      def self.from_secs(s)
        return infinite if s.respond_to?(:infinite?) && s.infinite?

        whole = s.floor
        new(whole, ((s - whole) * 1_000_000_000).round.clamp(0, 999_999_999))
      end

      def self.from_millisecs(ms)
        new(ms / 1000, (ms % 1000) * 1_000_000)
      end

      def self.zero
        new(0, 0)
      end

      def self.infinite
        new(INFINITE_SEC, INFINITE_NSEC)
      end

      def infinite?
        @sec == INFINITE_SEC && @nanosec == INFINITE_NSEC
      end

      def to_f
        infinite? ? Float::INFINITY : @sec + @nanosec / 1e9
      end

      def <=>(other)
        return nil unless other.is_a?(Duration)

        [@sec, @nanosec] <=> [other.sec, other.nanosec]
      end

      def hash
        [@sec, @nanosec].hash
      end
      alias eql? ==

      def to_native
        [@sec, @nanosec]
      end

      # A Duration, or seconds (Numeric).
      def self.of(v)
        v.is_a?(Duration) ? v : from_secs(v)
      end

      def inspect
        infinite? ? "#<DDS::Core::Duration infinite>" : "#<DDS::Core::Duration #{to_f}s>"
      end
    end

    # A time: seconds and nanoseconds since 1970-01-01T00:00:00Z.
    class Time
      include Comparable

      INVALID_SEC = -1
      INVALID_NSEC = 0xffffffff

      attr_reader :sec, :nanosec

      def initialize(sec = 0, nanosec = 0)
        @sec = Integer(sec)
        @nanosec = Integer(nanosec)
        freeze
      end

      def self.from_secs(s)
        whole = s.floor
        new(whole, ((s - whole) * 1_000_000_000).round.clamp(0, 999_999_999))
      end

      def self.invalid
        new(INVALID_SEC, INVALID_NSEC)
      end

      def self.now
        t = ::Time.now
        new(t.to_i, t.nsec)
      end

      def valid?
        !(@sec == INVALID_SEC && @nanosec == INVALID_NSEC)
      end

      def to_f
        @sec + @nanosec / 1e9
      end

      def to_time
        ::Time.at(@sec, @nanosec, :nsec)
      end

      def <=>(other)
        return nil unless other.is_a?(Time)

        [@sec, @nanosec] <=> [other.sec, other.nanosec]
      end

      def hash
        [@sec, @nanosec].hash
      end
      alias eql? ==

      def to_native
        [@sec, @nanosec]
      end

      # A Time, a ::Time or seconds since the epoch (Numeric).
      def self.of(v)
        case v
        when Time then v
        when ::Time then new(v.to_i, v.nsec)
        else from_secs(v)
        end
      end

      def inspect
        valid? ? "#<DDS::Core::Time #{to_f}>" : "#<DDS::Core::Time invalid>"
      end
    end

    # An instance handle (16 octets; all zeros is nil): of an instance, or
    # of an entity (its GUID).
    class InstanceHandle
      attr_reader :value

      def initialize(value = ("\0" * 16).b)
        raise ArgumentError, "16 octets expected" unless value.bytesize == 16

        @value = value.b.freeze
        freeze
      end

      NIL = new

      def self.nil_handle
        NIL
      end

      def is_nil # rubocop:disable Naming/PredicateName
        @value == NIL.value
      end
      alias nil_handle? is_nil

      def ==(other)
        other.is_a?(InstanceHandle) && other.value == @value
      end
      alias eql? ==

      def hash
        @value.hash
      end

      def inspect
        "#<DDS::Core::InstanceHandle #{@value.unpack1('H*')}>"
      end
      alias to_s inspect
    end

    module Status
      # StatusKind bits (DDS 1.4, 2.2.4.1).
      module StatusMask
        INCONSISTENT_TOPIC = 1 << 0
        OFFERED_DEADLINE_MISSED = 1 << 1
        REQUESTED_DEADLINE_MISSED = 1 << 2
        OFFERED_INCOMPATIBLE_QOS = 1 << 5
        REQUESTED_INCOMPATIBLE_QOS = 1 << 6
        SAMPLE_LOST = 1 << 7
        SAMPLE_REJECTED = 1 << 8
        DATA_ON_READERS = 1 << 9
        DATA_AVAILABLE = 1 << 10
        LIVELINESS_LOST = 1 << 11
        LIVELINESS_CHANGED = 1 << 12
        PUBLICATION_MATCHED = 1 << 13
        SUBSCRIPTION_MATCHED = 1 << 14
        ALL = 0xffffffff
        NONE = 0
      end

      def self.handle(bytes)
        InstanceHandle.new(bytes)
      end

      PublicationMatchedStatus = ::Struct.new(:total_count, :total_count_change, :current_count,
                                              :current_count_change, :last_subscription_handle) do
        def self.of(a) = new(a[0], a[1], a[2], a[3], Status.handle(a[4]))
      end
      SubscriptionMatchedStatus = ::Struct.new(:total_count, :total_count_change, :current_count,
                                               :current_count_change, :last_publication_handle) do
        def self.of(a) = new(a[0], a[1], a[2], a[3], Status.handle(a[4]))
      end
      QosPolicyCount = ::Struct.new(:policy_id, :count)
      OfferedIncompatibleQosStatus = ::Struct.new(:total_count, :total_count_change, :last_policy_id, :policies) do
        def self.of(a) = new(a[0], a[1], a[2], a[3].map { |id, n| QosPolicyCount.new(id, n) })
      end
      RequestedIncompatibleQosStatus = ::Struct.new(:total_count, :total_count_change, :last_policy_id, :policies) do
        def self.of(a) = new(a[0], a[1], a[2], a[3].map { |id, n| QosPolicyCount.new(id, n) })
      end
      LivelinessLostStatus = ::Struct.new(:total_count, :total_count_change) do
        def self.of(a) = new(a[0], a[1])
      end
      LivelinessChangedStatus = ::Struct.new(:alive_count, :not_alive_count, :alive_count_change,
                                             :not_alive_count_change, :last_publication_handle) do
        def self.of(a) = new(a[0], a[1], a[2], a[3], Status.handle(a[4]))
      end
      OfferedDeadlineMissedStatus = ::Struct.new(:total_count, :total_count_change, :last_instance_handle) do
        def self.of(a) = new(a[0], a[1], Status.handle(a[2]))
      end
      RequestedDeadlineMissedStatus = ::Struct.new(:total_count, :total_count_change, :last_instance_handle) do
        def self.of(a) = new(a[0], a[1], Status.handle(a[2]))
      end
      SampleLostStatus = ::Struct.new(:total_count, :total_count_change) do
        def self.of(a) = new(a[0], a[1])
      end
      # last_reason: SampleRejectedState (NOT_REJECTED...).
      SampleRejectedStatus = ::Struct.new(:total_count, :total_count_change, :last_reason, :last_instance_handle) do
        def self.of(a) = new(a[0], a[1], a[2], Status.handle(a[3]))
      end
      InconsistentTopicStatus = ::Struct.new(:total_count, :total_count_change) do
        def self.of(a) = new(a[0], a[1])
      end

      module SampleRejectedState
        NOT_REJECTED = 0
        REJECTED_BY_INSTANCES_LIMIT = 1
        REJECTED_BY_SAMPLES_LIMIT = 2
        REJECTED_BY_SAMPLES_PER_INSTANCE_LIMIT = 3
      end

      # The status of each listener call, by its name.
      OF_EVENT = {
        on_offered_deadline_missed: OfferedDeadlineMissedStatus,
        on_offered_incompatible_qos: OfferedIncompatibleQosStatus,
        on_liveliness_lost: LivelinessLostStatus,
        on_publication_matched: PublicationMatchedStatus,
        on_requested_deadline_missed: RequestedDeadlineMissedStatus,
        on_requested_incompatible_qos: RequestedIncompatibleQosStatus,
        on_sample_rejected: SampleRejectedStatus,
        on_liveliness_changed: LivelinessChangedStatus,
        on_subscription_matched: SubscriptionMatchedStatus,
        on_sample_lost: SampleLostStatus,
        on_inconsistent_topic: InconsistentTopicStatus
      }.freeze
    end

    # What every entity has (dds::core::Entity).
    class Entity
      # Whether close was called (or the parent closed).
      def closed?
        @closed ? true : false
      end

      # Entities are enabled when they are made.
      def enable; end

      def listener
        @listener
      end

      # Sets the listener (nil clears it) for the statuses of mask.
      def set_listener(listener, mask = Status::StatusMask::ALL)
        check_open
        id = listener ? Native.listener(self, listener) : nil
        Native.set_listener(@handle, id, mask)
        @listener = listener
        @listener_mask = mask
      end

      def close
        return if @closed

        close_contained
        @closed = true
        Native.release(@handle) if @handle
        @parent&.send(:forget, self)
      end

      # Releases the native handle without telling the parent (the parent
      # is closing).
      def close_from_parent
        return if @closed

        close_contained
        @closed = true
        Native.release(@handle) if @handle
      end

      # The native handle (DDS::Native::Handle).
      attr_reader :handle

      # When a parent closes it: topics after the rest.
      def close_order
        0
      end

      # The entity's StatusCondition (DDS 1.4, 2.2.2.1.1.5); writers and
      # readers have one.
      def status_condition
        check_open
        @status_condition ||= Cond::StatusCondition.new(self, Native.status_condition(@handle))
      end

      # The statuses that changed since they were last read (StatusMask
      # bits); writers and readers have them.
      def status_changes
        check_open
        Native.status_changes(@handle)
      end

      # Entities this one made, which close with it.
      def adopt(child)
        (@contained ||= []) << child
        child
      end

      def forget(child)
        @contained&.delete(child)
      end

      private

      def check_open
        raise AlreadyClosedError, "the #{self.class.name.split('::').last} is closed" if @closed
      end

      def close_contained
        # Writers and readers before the topics they use.
        (@contained || []).sort_by(&:close_order).each(&:close_from_parent)
        @contained = []
      end

      # A listener call of a participant, publisher, subscriber or topic:
      # the writer, reader, subscriber or topic it is about, found in
      # participant.
      def deliver_group(participant, event, endpoint, arg, listener)
        case event
        when :on_data_on_readers
          sub = participant.subscriber_for(arg)
          listener.on_data_on_readers(sub) if sub && listener.respond_to?(:on_data_on_readers)
        when :on_inconsistent_topic
          topic = participant.find_topic(endpoint)
          if topic && listener.respond_to?(event)
            listener.on_inconsistent_topic(topic, Status::InconsistentTopicStatus.of(arg))
          end
        when :on_data_available
          return unless listener.respond_to?(event)

          participant.endpoints.deliver(endpoint) { |r| listener.on_data_available(r) }
        else
          return unless listener.respond_to?(event)

          status = Status::OF_EVENT.fetch(event).of(arg)
          participant.endpoints.deliver(endpoint) { |e| listener.public_send(event, e, status) }
        end
      end

      # A listener call of a writer's or reader's own listener.
      def deliver_own(event, arg, listener)
        return unless listener.respond_to?(event)

        if event == :on_data_available
          listener.on_data_available(self)
        else
          listener.public_send(event, self, Status::OF_EVENT.fetch(event).of(arg))
        end
      end
    end

    # Conditions of WaitSets (dds::core::cond).
    module Cond
      # What every condition has: its trigger value and a handler a
      # WaitSet's dispatch calls.
      class Condition
        attr_reader :handle

        def trigger_value
          Native.trigger_value(@handle)
        end

        # The block WaitSet#dispatch calls when the condition is true.
        def handler(&blk)
          @handler = blk
          self
        end

        def dispatch
          @handler&.call(self)
        end

        def ==(other)
          other.is_a?(Condition) && Native.condition_equals(@handle, other.handle)
        end
        alias eql? ==

        def hash
          self.class.hash
        end
      end

      # A condition the application sets.
      class GuardCondition < Condition
        def initialize
          super
          @handle = Native.guard_condition
        end

        def trigger_value=(v)
          Native.set_trigger_value(@handle, v)
        end
      end

      # The condition of an entity's statuses (enabled_statuses).
      class StatusCondition < Condition
        attr_reader :entity

        def initialize(entity, handle)
          super()
          @entity = entity
          @handle = handle
        end

        def enabled_statuses
          Native.enabled_statuses(@handle)
        end

        def enabled_statuses=(mask)
          Native.set_enabled_statuses(@handle, mask)
        end
      end

      # Waits for conditions.
      class WaitSet
        def initialize
          @handle = Native.waitset_new
          @conditions = []
          @lock = Mutex.new
        end

        def attach_condition(c)
          @lock.synchronize do
            return self if @conditions.any? { |x| x.equal?(c) }

            Native.waitset_attach(@handle, c.handle, true)
            @conditions << c
          end
          self
        end
        alias << attach_condition

        def detach_condition(c)
          @lock.synchronize do
            i = @conditions.index { |x| x.equal?(c) } or return false
            Native.waitset_attach(@handle, c.handle, false)
            @conditions.delete_at(i)
          end
          true
        end

        def conditions
          @lock.synchronize { @conditions.dup }
        end

        # The conditions that are true, waiting up to timeout (a Duration
        # or seconds) for one; DDS::Core::TimeoutError when none is.
        def wait(timeout = Duration.infinite)
          attached = conditions
          Native.waitset_wait(@handle, Duration.of(timeout).to_native, attached.size).map { |i| attached[i] }
        end

        # Waits, then calls the handlers of the conditions that are true.
        def dispatch(timeout = Duration.infinite)
          wait(timeout).each(&:dispatch)
        end
      end
    end
  end
end
