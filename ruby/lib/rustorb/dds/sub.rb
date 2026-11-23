# frozen_string_literal: true

# DDS::Sub::Subscriber, DataReader, their samples and ReadCondition
# (dds::sub of the C++ PSM).
#
# Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

module DDS
  module Sub
    # The listener of a reader.
    module DataReaderListener
      def on_requested_deadline_missed(reader, status); end
      def on_requested_incompatible_qos(reader, status); end
      def on_sample_rejected(reader, status); end
      def on_liveliness_changed(reader, status); end
      def on_data_available(reader); end
      def on_subscription_matched(reader, status); end
      def on_sample_lost(reader, status); end
    end

    # The listener of a subscriber: on_data_on_readers, and the calls of
    # its readers' listeners. When it (or the participant's) takes
    # DATA_ON_READERS, on_data_available is not called
    # (subscriber.notify_datareaders calls it).
    module SubscriberListener
      include DataReaderListener

      def on_data_on_readers(subscriber); end
    end

    module Status
      # Sample, view and instance states (DDS 1.4, 2.2.2.5.1.8).
      module SampleState
        READ = 1
        NOT_READ = 2
        ANY = 3
      end

      module ViewState
        NEW = 1
        NOT_NEW = 2
        ANY = 3
      end

      module InstanceState
        ALIVE = 1
        NOT_ALIVE_DISPOSED = 2
        NOT_ALIVE_NO_WRITERS = 4
        NOT_ALIVE = 6
        ANY = 7
      end
    end

    SampleInfo = ::Struct.new(:sample_state, :view_state, :instance_state, :disposed_generation_count,
                              :no_writers_generation_count, :sample_rank, :generation_rank,
                              :absolute_generation_rank, :source_timestamp, :instance_handle,
                              :publication_handle, :sequence_number, :valid_data) do
      def self.of(a)
        new(a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7], Core::Time.new(*a[8]), Core::InstanceHandle.new(a[9]),
            Core::InstanceHandle.new(a[10]), a[11], a[12])
      end

      def valid?
        valid_data
      end
    end

    # A sample: its data (nil when the info has no valid data) and info.
    Sample = ::Struct.new(:data, :info) do
      def valid?
        info.valid_data
      end
    end

    # Makes readers, which close with it.
    class Subscriber < Core::Entity
      attr_reader :participant, :qos

      def initialize(participant, qos = nil, listener = nil, mask = Core::Status::StatusMask::ALL)
        super()
        @participant = participant
        @qos = (qos || SubscriberQos.new).dup.freeze
        @handle = Native.create_subscriber(participant.handle, @qos.partition.name)
        @parent = participant
        participant.add_subscriber(self)
        set_listener(listener, mask) if listener
      end

      def default_datareader_qos = DataReaderQos.new

      # Calls on_data_available of the listeners of its readers that have
      # new data.
      def notify_datareaders
        check_open
        Native.notify_datareaders(@handle)
      end

      def deliver(event, endpoint, arg, listener)
        deliver_group(@participant, event, endpoint, arg, listener)
      end

      def inspect
        "#<DDS::Sub::Subscriber #{@qos.partition.name.inspect}>"
      end
    end

    # Reads the samples of a topic (or content filtered topic).
    class DataReader < Core::Entity
      attr_reader :subscriber, :topic_description, :qos

      LENGTH_UNLIMITED = -1

      def initialize(subscriber, topic, qos = nil, listener = nil, mask = Core::Status::StatusMask::ALL)
        super()
        @subscriber = subscriber
        @topic_description = topic
        @type = topic.type
        @qos = (qos || DataReaderQos.new).dup.freeze
        slot = listener ? Native::Slot.new : nil
        id = slot ? Native.listener(slot, listener) : nil
        begin
          @handle = Native.create_reader(subscriber.handle, topic.handle, @qos.to_native, id, mask)
        rescue StandardError
          Native.forget_listener(id) if id
          raise
        end
        @listener = listener
        @guid = Native.reader_guid(@handle)
        @parent = subscriber
        subscriber.adopt(self)
        participant.endpoints.add(@guid, self)
        slot.entity = self if slot
      end

      alias topic topic_description

      def participant
        @subscriber.participant
      end

      def instance_handle
        Core::InstanceHandle.new(@guid)
      end

      # The samples (DDS::Sub::Sample), left in the reader: at most
      # max_samples of those in the states given, of instance (a handle),
      # or of the instance after next_instance (a handle, nil handle for
      # the first), or those of condition (a ReadCondition).
      def read(**selection)
        select(false, **selection)
      end

      # The samples, taken from the reader (as read).
      def take(**selection)
        select(true, **selection)
      end

      # The values of the valid samples taken.
      def take_data(**selection)
        take(**selection).select(&:valid?).map(&:data)
      end

      # The values of the valid samples read.
      def read_data(**selection)
        read(**selection).select(&:valid?).map(&:data)
      end

      # The key of an instance, as a sample (its other members zero).
      def key_value(handle)
        check_open
        Types.read(@type, Native.key_value(@handle, handle.value))
      end

      # The handle of the instance of the sample's key, or the nil handle.
      def lookup_instance(sample)
        check_open
        Core::InstanceHandle.new(Native.reader_lookup_instance(@handle, Types.out(@type, sample)))
      end

      # Waits until a sample is there (a rustorb-dds extension;
      # DDS::Core::TimeoutError after timeout).
      def wait_for_data(timeout = Core::Duration.infinite)
        check_open
        Native.wait(@handle, 2, 0, Core::Duration.of(timeout).to_native)
      end

      # Waits until count writers match (a rustorb-dds extension).
      def wait_for_publications(count = 1, timeout = Core::Duration.infinite)
        check_open
        Native.wait(@handle, 3, count, Core::Duration.of(timeout).to_native)
      end

      # The samples of a TRANSIENT_LOCAL or stronger durability come as the
      # writers are matched; nothing to wait for here.
      def wait_for_historical_data(_timeout); end

      def matched_publications
        check_open
        Native.matched(@handle).map { |g| Core::InstanceHandle.new(g) }
      end

      def subscription_matched_status = status(:subscription_matched, Core::Status::SubscriptionMatchedStatus)
      def requested_incompatible_qos_status = status(:requested_incompatible_qos,
                                                     Core::Status::RequestedIncompatibleQosStatus)
      def liveliness_changed_status = status(:liveliness_changed, Core::Status::LivelinessChangedStatus)
      def requested_deadline_missed_status = status(:requested_deadline_missed,
                                                    Core::Status::RequestedDeadlineMissedStatus)
      def sample_lost_status = status(:sample_lost, Core::Status::SampleLostStatus)
      def sample_rejected_status = status(:sample_rejected, Core::Status::SampleRejectedStatus)

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
        "#<DDS::Sub::DataReader of #{@topic_description.name}>"
      end

      private

      def status(name, cls)
        check_open
        cls.of(Native.status(@handle, name))
      end

      def select(take, max_samples: LENGTH_UNLIMITED, sample_state: Status::SampleState::ANY,
                 view_state: Status::ViewState::ANY, instance_state: Status::InstanceState::ANY,
                 instance: nil, next_instance: nil, condition: nil)
        check_open
        raw = if condition
                Native.select_w_condition(@handle, take, max_samples, condition.handle)
              elsif instance
                Native.select(@handle, take, max_samples, sample_state, view_state, instance_state, 1, instance.value)
              elsif !next_instance.nil?
                Native.select(@handle, take, max_samples, sample_state, view_state, instance_state, 2,
                              next_instance.value)
              else
                Native.select(@handle, take, max_samples, sample_state, view_state, instance_state, 0, nil)
              end
        raw.map do |info, inp|
          Sample.new(inp ? Types.read(@type, inp) : nil, SampleInfo.of(info))
        end
      end
    end

    module Cond
      # The condition of a reader's samples in some states.
      class ReadCondition < Core::Cond::Condition
        attr_reader :reader, :sample_state, :view_state, :instance_state

        def initialize(reader, sample_state: Status::SampleState::ANY, view_state: Status::ViewState::ANY,
                       instance_state: Status::InstanceState::ANY)
          super()
          @reader = reader
          @sample_state = sample_state
          @view_state = view_state
          @instance_state = instance_state
          @handle = Native.create_readcondition(reader.handle, sample_state, view_state, instance_state)
        end
      end
    end
  end
end
