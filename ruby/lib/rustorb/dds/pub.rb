# frozen_string_literal: true

# DDS::Pub::Publisher and DataWriter (dds::pub of the C++ PSM).
#
# Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

module DDS
  module Pub
    # The listener of a writer.
    module DataWriterListener
      def on_offered_deadline_missed(writer, status); end
      def on_offered_incompatible_qos(writer, status); end
      def on_liveliness_lost(writer, status); end
      def on_publication_matched(writer, status); end
    end

    # The listener of a publisher: the calls of its writers' listeners.
    module PublisherListener
      include DataWriterListener
    end

    # Makes writers, which close with it.
    class Publisher < Core::Entity
      attr_reader :participant, :qos

      def initialize(participant, qos = nil, listener = nil, mask = Core::Status::StatusMask::ALL)
        super()
        @participant = participant
        @qos = (qos || PublisherQos.new).dup.freeze
        @handle = Native.create_publisher(participant.handle, @qos.partition.name)
        @parent = participant
        participant.adopt(self)
        set_listener(listener, mask) if listener
      end

      def default_datawriter_qos = DataWriterQos.new

      def deliver(event, endpoint, arg, listener)
        deliver_group(@participant, event, endpoint, arg, listener)
      end

      # Waits until the writers' samples are acknowledged.
      def wait_for_acknowledgments(timeout)
        (@contained || []).each { |w| w.wait_for_acknowledgments(timeout) if w.is_a?(DataWriter) }
      end

      def inspect
        "#<DDS::Pub::Publisher #{@qos.partition.name.inspect}>"
      end
    end

    # Writes the samples of a topic.
    class DataWriter < Core::Entity
      attr_reader :publisher, :topic, :qos

      def initialize(publisher, topic, qos = nil, listener = nil, mask = Core::Status::StatusMask::ALL)
        super()
        @publisher = publisher
        @topic = topic
        @type = topic.type
        @qos = (qos || DataWriterQos.new).dup.freeze
        slot = listener ? Native::Slot.new : nil
        id = slot ? Native.listener(slot, listener) : nil
        begin
          @handle = Native.create_writer(publisher.handle, topic.handle, @qos.to_native, id, mask)
        rescue StandardError
          Native.forget_listener(id) if id
          raise
        end
        @listener = listener
        @guid = Native.writer_guid(@handle)
        @parent = publisher
        publisher.adopt(self)
        participant.endpoints.add(@guid, self)
        slot.entity = self if slot
      end

      def participant
        @publisher.participant
      end

      def instance_handle
        Core::InstanceHandle.new(@guid)
      end

      # Writes a sample (timestamp: a DDS::Core::Time, a ::Time, or nil for
      # now).
      def write(sample, timestamp = nil)
        op(0, sample, timestamp)
        self
      end

      def <<(sample)
        write(sample)
      end

      # The handle of the instance of the sample's key, registered.
      def register_instance(sample)
        Core::InstanceHandle.new(op(3, sample, nil))
      end

      # The handle of the instance of the sample's key, or the nil handle.
      def lookup_instance(sample)
        Core::InstanceHandle.new(op(4, sample, nil))
      end

      # Unregisters the instance of the sample's key.
      def unregister_instance(sample, timestamp = nil)
        op(2, sample, timestamp)
        self
      end

      # Disposes of the instance of the sample's key.
      def dispose_instance(sample, timestamp = nil)
        op(1, sample, timestamp)
        self
      end

      # Waits until the samples written are acknowledged by the reliable
      # readers (DDS::Core::TimeoutError after timeout).
      def wait_for_acknowledgments(timeout)
        check_open
        Native.wait(@handle, 0, 0, Core::Duration.of(timeout).to_native)
      end

      # Waits until count readers match (a rustorb-dds extension).
      def wait_for_subscriptions(count = 1, timeout = Core::Duration.infinite)
        check_open
        Native.wait(@handle, 1, count, Core::Duration.of(timeout).to_native)
      end

      def matched_subscriptions
        check_open
        Native.matched(@handle).map { |g| Core::InstanceHandle.new(g) }
      end

      def assert_liveliness
        check_open
        Native.assert_liveliness(@handle)
      end

      def publication_matched_status = status(:publication_matched, Core::Status::PublicationMatchedStatus)
      def offered_incompatible_qos_status = status(:offered_incompatible_qos,
                                                   Core::Status::OfferedIncompatibleQosStatus)
      def liveliness_lost_status = status(:liveliness_lost, Core::Status::LivelinessLostStatus)
      def offered_deadline_missed_status = status(:offered_deadline_missed,
                                                  Core::Status::OfferedDeadlineMissedStatus)

      def deliver(event, _endpoint, arg, listener)
        deliver_own(event, arg, listener)
      end

      def close
        participant.endpoints.remove(@guid) unless closed?
        super
      end

      def close_from_parent
        participant.endpoints.remove(@guid) unless closed?
        super
      end

      def inspect
        "#<DDS::Pub::DataWriter of #{@topic.name}>"
      end

      private

      def status(name, cls)
        check_open
        cls.of(Native.status(@handle, name))
      end

      def op(code, sample, timestamp)
        check_open
        out = Types.out(@type, sample)
        Native.writer_op(@handle, code, out, timestamp.nil? ? nil : Core::Time.of(timestamp).to_native)
      end
    end
  end
end
