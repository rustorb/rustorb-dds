# frozen_string_literal: true

# DDS::Domain::DomainParticipant (dds::domain of the C++ PSM).
#
# Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

module DDS
  module Domain
    # The listener of a participant: every call of the other listeners,
    # for the entities whose own listeners do not take it.
    module DomainParticipantListener
      include Pub::PublisherListener
      include Sub::SubscriberListener
      include DDS::Topic::TopicListener
    end

    # A participant in a domain: makes topics, publishers and subscribers,
    # which close with it.
    class DomainParticipant < Core::Entity
      attr_reader :domain_id, :qos, :endpoints

      # localhost: only the loopback interface (for tests; a rustorb-dds
      # extension).
      def initialize(domain_id = 0, qos = nil, listener = nil, mask = Core::Status::StatusMask::ALL, localhost: false)
        super()
        qos ||= DomainParticipantQos.new
        @qos = qos.dup.freeze
        @handle = Native.participant_new(domain_id, qos.user_data.value, qos.property.value.to_a, localhost ? 1 : 0)
        @domain_id, @guid, = Native.participant_info(@handle)
        @endpoints = Native::Endpoints.new
        @topics = {}
        @subscribers = []
        @lock = Mutex.new
        set_listener(listener, mask) if listener
      end

      def instance_handle
        Core::InstanceHandle.new(@guid)
      end

      # How many other participants are known (a rustorb-dds extension).
      def discovered_participants
        check_open
        Native.participant_info(@handle)[2]
      end

      def assert_liveliness
        check_open
        Native.participant_assert_liveliness(@handle)
      end

      def default_publisher_qos = Pub::PublisherQos.new
      def default_subscriber_qos = Sub::SubscriberQos.new
      def default_topic_qos = DDS::Topic::TopicQos.new
      def default_datawriter_qos = Pub::DataWriterQos.new
      def default_datareader_qos = Sub::DataReaderQos.new

      # The topic of that name this participant made, or nil.
      def find_topic(name)
        t = lookup_topicdescription(name)
        t.is_a?(DDS::Topic::Topic) ? t : nil
      end

      # The topic or content filtered topic of that name, or nil.
      def lookup_topicdescription(name)
        @lock.synchronize { @topics[name] }
      end

      def current_time
        Core::Time.now
      end

      # Closes the topics, publishers and subscribers it made.
      def delete_contained_entities
        check_open
        close_contained
        @lock.synchronize do
          @topics.clear
          @subscribers.clear
        end
      end

      def add_topic(topic)
        @lock.synchronize do
          raise Core::PreconditionNotMetError, "the topic #{topic.name} exists" if @topics.key?(topic.name)

          @topics[topic.name] = topic
        end
        adopt(topic)
      end

      def add_subscriber(sub)
        @lock.synchronize { @subscribers << sub }
        adopt(sub)
      end

      def forget(child)
        super
        @lock.synchronize do
          @topics.delete(child.name) if @topics[child.name].equal?(child)
          @subscribers.delete(child)
        end
      end

      # The subscriber of a handle the library gave a listener (which is
      # released).
      def subscriber_for(handle)
        return nil unless handle

        subs = @lock.synchronize { @subscribers.dup }
        subs.find { |s| !s.closed? && Native.subscriber_same(s.handle, handle) }
      ensure
        Native.release(handle) if handle
      end

      def deliver(event, endpoint, arg, listener)
        deliver_group(self, event, endpoint, arg, listener)
      end

      def close
        super
        @lock.synchronize do
          @topics.clear
          @subscribers.clear
        end
      end

      def inspect
        "#<DDS::Domain::DomainParticipant domain=#{@domain_id}#{closed? ? ' closed' : ''}>"
      end
    end
  end
end
