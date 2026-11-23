# frozen_string_literal: true

# DDS::Topic::Topic and ContentFilteredTopic (dds::topic of the C++ PSM).
#
# Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

module DDS
  module Topic
    # The listener of a topic.
    module TopicListener
      def on_inconsistent_topic(topic, status); end
    end

    # A topic: a name and a type, a class (or module) rustorb-idl -bruby
    # -Wbdds_only (or -Wbdds) wrote, or a DDS::Types::Type.
    class Topic < Core::Entity
      attr_reader :name, :type, :participant, :qos

      def initialize(participant, name, type, qos = nil, listener = nil, mask = Core::Status::StatusMask::ALL)
        super()
        @participant = participant
        @name = name.to_s.dup.freeze
        @type = Types.of(type)
        unless @type.resolved.is_a?(Types::StructType) || @type.resolved.is_a?(Types::UnionType)
          raise ArgumentError, "the type of a topic is a struct or a union, not #{@type.name}"
        end

        @qos = (qos || TopicQos.new).dup.freeze
        @handle = Native.create_topic(participant.handle, @name, @type.native)
        @parent = participant
        begin
          participant.add_topic(self)
        rescue StandardError
          Native.release(@handle)
          raise
        end
        set_listener(listener, mask) if listener
      end

      def type_name
        @type.name
      end

      def close_order
        2
      end

      def inconsistent_topic_status
        check_open
        Core::Status::InconsistentTopicStatus.of(Native.topic_inconsistent_status(@handle))
      end

      def deliver(event, endpoint, arg, listener)
        deliver_group(@participant, event, endpoint, arg, listener)
      end

      def inspect
        "#<DDS::Topic::Topic #{@name} (#{type_name})>"
      end
    end

    # A topic whose readers get the samples of the related topic that pass
    # the filter (DDS 1.4, 2.2.2.3.3 and Annex B), with parameters for its
    # %0 .. %99.
    class ContentFilteredTopic < Core::Entity
      attr_reader :name, :topic, :filter_expression, :participant

      def initialize(topic, name, filter_expression, parameters = [])
        super()
        @topic = topic
        @participant = topic.participant
        @name = name.to_s.dup.freeze
        @filter_expression = filter_expression.to_s.dup.freeze
        @parameters = parameters.map(&:to_s).freeze
        @handle = Native.create_cftopic(@participant.handle, @name, topic.handle, @filter_expression, @parameters)
        @parent = @participant
        begin
          @participant.add_topic(self)
        rescue StandardError
          Native.release(@handle)
          raise
        end
      end

      alias related_topic topic

      def close_order
        1
      end

      def type
        @topic.type
      end

      def type_name
        @topic.type_name
      end

      def expression_parameters
        @parameters
      end

      def expression_parameters=(parameters)
        check_open
        p = parameters.map(&:to_s).freeze
        Native.cftopic_set_parameters(@handle, p)
        @parameters = p
      end

      def inspect
        "#<DDS::Topic::ContentFilteredTopic #{@name} of #{@topic.name}: #{@filter_expression}>"
      end
    end
  end
end
