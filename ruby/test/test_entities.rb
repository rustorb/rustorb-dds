# frozen_string_literal: true

# The entities of the Ruby binding on a loopback-only domain: writers and
# readers, keyed instances, listeners, conditions and wait sets, QoS and
# statuses.
#
# Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

require_relative "helper"

class TestEntities < Minitest::Test
  include TestHelper
  include DDS::Core::Policy
  Mask = DDS::Core::Status::StatusMask
  SampleState = DDS::Sub::Status::SampleState
  ViewState = DDS::Sub::Status::ViewState
  InstanceState = DDS::Sub::Status::InstanceState

  def setup
    @domain = TestHelper.next_domain
    @dp = participant(domain: @domain)
  end

  def teardown
    @dp.close
  end

  def topic(name, type = HelloWorld::Greeting)
    DDS::Topic::Topic.new(@dp, name, type)
  end

  def pub(qos = nil) = DDS::Pub::Publisher.new(@dp, qos)
  def sub(qos = nil, listener = nil, mask = Mask::ALL) = DDS::Sub::Subscriber.new(@dp, qos, listener, mask)

  def greeting(i, m) = HelloWorld::Greeting.new(i, m)

  def test_participant
    assert_equal @domain, @dp.domain_id
    assert @dp.current_time.sec.positive?
    refute @dp.instance_handle.is_nil
    assert @dp.discovered_participants >= 0
    @dp.assert_liveliness
    assert_equal DDS::Core::Duration.new(1, 500_000_000), DDS::Core::Duration.from_secs(1.5)
    assert_in_delta 1.5, DDS::Core::Duration.new(1, 500_000_000).to_f
    assert DDS::Core::Duration.from_secs(Float::INFINITY).infinite?
  end

  def test_write_and_take
    t = topic("greetings")
    assert_equal "greetings", t.name
    assert_equal "HelloWorld::Greeting", t.type_name
    assert_same t, @dp.find_topic("greetings")
    p = pub
    s = sub
    writer = DDS::Pub::DataWriter.new(p, t, reliable_writer)
    reader = DDS::Sub::DataReader.new(s, t, reliable_reader)
    writer.wait_for_subscriptions(1, TEN_SECONDS)
    reader.wait_for_publications(1, TEN_SECONDS)
    (1..3).each { |i| writer.write(greeting(i, "hello #{i}")) }
    writer.wait_for_acknowledgments(TEN_SECONDS)
    reader.wait_for_data(TEN_SECONDS)

    # Read leaves them, and marks them read.
    read = reader.read
    assert_equal 3, read.size
    assert_equal SampleState::NOT_READ, read[0].info.sample_state
    assert_equal ViewState::NEW, read[0].info.view_state
    assert_equal InstanceState::ALIVE, read[0].info.instance_state
    assert read[0].info.source_timestamp.sec.positive?
    assert_equal SampleState::READ, reader.read[0].info.sample_state
    assert_equal 2, reader.read(max_samples: 2).size
    # Take with max samples: the first to arrive.
    one = reader.take(max_samples: 1)
    assert_equal 1, one.size
    assert_equal 1, one[0].data.index
    taken = reader.take
    assert_equal [2, 3], taken.map { |x| x.data.index }
    assert_equal ["hello 2", "hello 3"], taken.map { |x| x.data.message }
    assert_empty reader.take

    assert_equal 1, writer.publication_matched_status.current_count
    assert_equal 1, reader.subscription_matched_status.current_count
    assert_equal 1, writer.matched_subscriptions.size
    assert_equal 1, reader.matched_publications.size
    assert_same p, writer.publisher
    assert_same t, writer.topic
    assert_same s, reader.subscriber
    writer << greeting(4, "shift")
    reader.wait_for_data(TEN_SECONDS)
    eventually { reader.read_data.size == 1 }
    assert_equal [greeting(4, "shift")], reader.take_data
  end

  def test_qos
    t = topic("qos")
    p = pub(DDS::Pub::PublisherQos.new << Partition.new(%w[a b]))
    assert_equal %w[a b], p.qos.partition.name
    q = reliable_writer << History.keep_last(7) << Durability.transient_local <<
        Deadline.new(DDS::Core::Duration.new(2, 0)) << UserData.new("\x01\x02\x03")
    writer = DDS::Pub::DataWriter.new(p, t, q)
    assert_equal ReliabilityKind::RELIABLE, writer.qos.reliability.kind
    assert_equal 7, writer.qos.history.depth
    assert_equal 2.0, writer.qos.deadline.period.to_f
    assert_equal "\x01\x02\x03".b, writer.qos.user_data.value
    reader = DDS::Sub::DataReader.new(sub, t)
    assert_equal ReliabilityKind::BEST_EFFORT, reader.qos.reliability.kind
    assert_equal DDS::Sub::DataReaderQos.new, DDS::Sub::DataReaderQos.new
    assert_raises(ArgumentError) { DDS::Sub::SubscriberQos.new << History.keep_all }
    assert_raises(FrozenError) { History.keep_all.depth = 3 }
  end

  def test_keyed_instances
    t = topic("shapes", Keyed::Shape)
    writer = DDS::Pub::DataWriter.new(pub, t, reliable_writer << WriterDataLifecycle.new(false))
    reader = DDS::Sub::DataReader.new(sub, t, reliable_reader)
    writer.wait_for_subscriptions(1, TEN_SECONDS)

    two = writer.register_instance(Keyed::Shape.new(2, 0, ""))
    refute two.is_nil
    assert_equal two, writer.lookup_instance(Keyed::Shape.new(2, 99, "other"))
    assert writer.lookup_instance(Keyed::Shape.new(5, 0, "")).is_nil
    (1..6).each { |i| writer.write(Keyed::Shape.new(((i - 1) % 3) + 1, i, "s")) }
    writer.dispose_instance(Keyed::Shape.new(2, 0, ""))
    writer.unregister_instance(Keyed::Shape.new(3, 0, ""))
    writer.wait_for_acknowledgments(TEN_SECONDS)

    reader_two = reader.lookup_instance(Keyed::Shape.new(2, 0, ""))
    refute reader_two.is_nil
    eventually { reader.read.size >= 8 }
    assert_equal 2, reader.key_value(reader_two).id
    # The samples of one instance, and of the instances after one.
    of_two = reader.read(instance: reader_two)
    assert of_two.size >= 2
    assert(of_two.all? { |x| x.info.instance_handle == reader_two })
    first = reader.read(next_instance: DDS::Core::InstanceHandle::NIL)
    refute_empty first
    assert(first.all? { |x| x.info.instance_handle == first[0].info.instance_handle })
    disposed = no_writers = false
    valid = 0
    reader.take.each do |x|
      if x.valid?
        valid += 1
      elsif x.info.instance_handle == reader_two
        disposed ||= x.info.instance_state == InstanceState::NOT_ALIVE_DISPOSED
      else
        no_writers ||= x.info.instance_state == InstanceState::NOT_ALIVE_NO_WRITERS
      end
    end
    assert_equal 6, valid
    assert disposed
    assert no_writers
  end

  class WriterListener
    include DDS::Pub::DataWriterListener
    attr_reader :matched

    def initialize
      @matched = Queue.new
    end

    def on_publication_matched(_writer, status)
      @matched << true if status.current_count == 1
    end
  end

  class TakingListener
    include DDS::Sub::DataReaderListener
    attr_reader :matched, :got

    def initialize
      @matched = Queue.new
      @got = Queue.new
    end

    def on_subscription_matched(_reader, _status)
      @matched << true
    end

    def on_data_available(reader)
      reader.take.each { |x| @got << x.data.index if x.valid? }
    end
  end

  def pop(queue, seconds = 10)
    deadline = Process.clock_gettime(Process::CLOCK_MONOTONIC) + seconds
    loop do
      return queue.pop(true)
    rescue ThreadError
      flunk "timed out" if Process.clock_gettime(Process::CLOCK_MONOTONIC) > deadline
      sleep 0.01
    end
  end

  def test_listeners
    t = topic("listened")
    wl = WriterListener.new
    writer = DDS::Pub::DataWriter.new(pub, t, reliable_writer, wl)
    rl = TakingListener.new
    reader = DDS::Sub::DataReader.new(sub, t, reliable_reader, rl)
    assert_same rl, reader.listener
    pop(rl.matched)
    pop(wl.matched)
    (1..3).each { |i| writer.write(greeting(i, "x")) }
    got = Array.new(3) { pop(rl.got) }
    assert_equal [1, 2, 3], got
    # Without a listener, no more calls.
    reader.set_listener(nil)
    writer.write(greeting(4, "x"))
    reader.wait_for_data(TEN_SECONDS)
    sleep 0.1
    assert rl.got.empty?
  end

  class ParticipantListener
    include DDS::Domain::DomainParticipantListener
    attr_reader :calls, :published

    def initialize
      @calls = Queue.new
      @published = Queue.new
    end

    def on_publication_matched(writer, _status)
      @calls << "participant:matched #{writer.topic.name}"
      @published << true
    end

    def on_data_available(_reader)
      @calls << "participant:data_available"
    end
  end

  class SubscriberListener
    include DDS::Sub::SubscriberListener
    attr_reader :on_readers

    def initialize(calls)
      @calls = calls
      @on_readers = Queue.new
    end

    def on_data_on_readers(subscriber)
      @calls << "subscriber:data_on_readers"
      subscriber.notify_datareaders
      @on_readers << subscriber
    end
  end

  class ReaderListener
    include DDS::Sub::DataReaderListener
    attr_reader :data

    def initialize(calls)
      @calls = calls
      @data = Queue.new
    end

    def on_data_available(reader)
      @calls << "reader:data_available"
      @data << reader
    end
  end

  def test_group_listeners
    pl = ParticipantListener.new
    @dp.set_listener(pl, Mask::PUBLICATION_MATCHED | Mask::DATA_AVAILABLE)
    t = topic("grouped")
    p = pub
    sl = SubscriberListener.new(pl.calls)
    s = sub(nil, sl, Mask::DATA_ON_READERS)
    rl = ReaderListener.new(pl.calls)
    reader = DDS::Sub::DataReader.new(s, t, reliable_reader, rl, Mask::DATA_AVAILABLE)
    writer = DDS::Pub::DataWriter.new(p, t, reliable_writer)
    pop(pl.published)
    calls = []
    calls << pl.calls.pop until pl.calls.empty?
    assert_includes calls, "participant:matched grouped"
    writer.write(greeting(1, "g"))
    assert_same s, pop(sl.on_readers)
    assert_same reader, pop(rl.data)
    calls << pl.calls.pop until pl.calls.empty?
    refute_includes calls, "participant:data_available"
    assert_equal 1, reader.take.size
    @dp.set_listener(nil)
  end

  def test_content_filtered_topic
    t = topic("filtered_greetings")
    cft = DDS::Topic::ContentFilteredTopic.new(t, "big_greetings", "index > %0 AND message LIKE 'h%'", ["3"])
    assert_equal "big_greetings", cft.name
    assert_equal "HelloWorld::Greeting", cft.type_name
    assert_same t, cft.related_topic
    assert_same cft, @dp.lookup_topicdescription("big_greetings")
    assert_equal ["3"], cft.expression_parameters
    writer = DDS::Pub::DataWriter.new(pub, t, reliable_writer)
    reader = DDS::Sub::DataReader.new(sub, cft, reliable_reader)
    assert_same cft, reader.topic_description
    writer.wait_for_subscriptions(1, TEN_SECONDS)
    (1..6).each { |i| writer.write(greeting(i, i == 5 ? "bye" : "hello")) }
    writer.wait_for_acknowledgments(TEN_SECONDS)
    got = []
    eventually do
      got.concat(reader.take_data.map(&:index))
      got.size >= 2
    end
    assert_equal [4, 6], got
    cft.expression_parameters = ["5"]
    assert_equal ["5"], cft.expression_parameters
    assert_raises(DDS::Core::Error) { cft.expression_parameters = [] }
    assert_raises(DDS::Core::Error) { DDS::Topic::ContentFilteredTopic.new(t, "bad", "nope = 1") }
  end

  def test_transient_samples_outlive_the_writer
    defaults = DDS::Pub::DataWriterQos.new
    assert_equal 1, defaults.durability_service.history_depth
    assert_equal(-1, defaults.durability_service.max_samples)
    t = topic("kept")
    ds = defaults.durability_service.to_h.merge(history_depth: 2)
    q = reliable_writer << Durability.transient << History.keep_last(3) << DurabilityService.new(*ds.values)
    writer = DDS::Pub::DataWriter.new(pub, t, q)
    (1..3).each { |i| writer.write(greeting(i, "kept #{i}")) }
    writer.close
    assert writer.closed?
    reader = DDS::Sub::DataReader.new(sub, t, reliable_reader << Durability.transient)
    got = []
    eventually do
      got.concat(reader.take_data.map(&:index))
      got.size >= 2
    end
    assert_equal [2, 3], got
  end

  def test_conditions_and_wait_sets
    t = topic("waited")
    writer = DDS::Pub::DataWriter.new(pub, t, reliable_writer)
    reader = DDS::Sub::DataReader.new(sub, t, reliable_reader)
    ws = DDS::Core::Cond::WaitSet.new

    # A guard condition, with a handler.
    guard = DDS::Core::Cond::GuardCondition.new
    ws << guard
    assert_raises(DDS::Core::TimeoutError) { ws.wait(0.05) }
    guard.trigger_value = true
    assert_equal [guard], ws.wait(TEN_SECONDS)
    handled = []
    guard.handler { |c| handled << c }
    ws.dispatch(TEN_SECONDS)
    assert_equal [guard], handled
    ws.detach_condition(guard)

    # A status condition.
    status = reader.status_condition
    status.enabled_statuses = Mask::DATA_AVAILABLE
    assert_equal Mask::DATA_AVAILABLE, status.enabled_statuses
    assert_same reader, status.entity
    assert_same status, reader.status_condition
    ws << status
    writer.wait_for_subscriptions(1, TEN_SECONDS)
    writer.write(greeting(1, "x"))
    assert_equal [status], ws.wait(TEN_SECONDS)
    assert(reader.status_changes & Mask::DATA_AVAILABLE != 0)
    ws.detach_condition(status)

    # A read condition on samples not read.
    not_read = DDS::Sub::Cond::ReadCondition.new(reader, sample_state: SampleState::NOT_READ)
    ws << not_read
    assert_equal [not_read], ws.wait(TEN_SECONDS)
    assert not_read.trigger_value
    assert_equal [1], reader.read(condition: not_read).map { |x| x.data.index }
    refute not_read.trigger_value
    assert_raises(DDS::Core::TimeoutError) { ws.wait(0.05) }
    assert_equal [not_read], ws.conditions
  end

  def test_close
    t = topic("closed")
    writer = DDS::Pub::DataWriter.new(pub, t)
    writer.close
    assert_raises(DDS::Core::AlreadyClosedError) { writer.write(greeting(1, "x")) }
    # Deleting a participant's entities closes them.
    p = pub
    reader = DDS::Sub::DataReader.new(sub, t)
    @dp.delete_contained_entities
    assert_raises(DDS::Core::AlreadyClosedError) { reader.take }
    assert_raises(DDS::Core::AlreadyClosedError) { DDS::Pub::DataWriter.new(p, t) }
    assert_nil @dp.find_topic("closed")
    # A topic of that name again.
    topic("closed")
    assert_raises(DDS::Core::PreconditionNotMetError) { topic("closed") }
    assert_raises(ArgumentError) { topic("not_a_type", String) }
  end

  def secure_qos(who)
    certs = "file:#{File.join(TestHelper::ROOT, 'crates', 'rustorb-dds-security', 'tests', 'certs')}/"
    DDS::Domain::DomainParticipantQos.new << Property.new(
      "dds.sec.auth.identity_ca" => "#{certs}identity_ca.pem",
      "dds.sec.auth.identity_certificate" => "#{certs}#{who}_cert.pem",
      "dds.sec.auth.private_key" => "#{certs}#{who}_key.pem",
      "dds.sec.access.permissions_ca" => "#{certs}permissions_ca.pem",
      "dds.sec.access.governance" => "#{certs}governance.p7s",
      "dds.sec.access.permissions" => "#{certs}permissions_#{who}.p7s"
    )
  end

  def test_security_from_the_property
    d = TestHelper.next_domain
    # Some of the dds.sec properties only: refused.
    partial = DDS::Domain::DomainParticipantQos.new << Property.new("dds.sec.auth.identity_ca" => "file:/nowhere.pem")
    assert_raises(DDS::Core::Error) { participant(partial, domain: d) }
    # A protected topic (Square* in the governance) between two
    # participants that authenticate each other.
    p1 = participant(secure_qos("p1"), domain: d)
    p2 = participant(secure_qos("p2"), domain: d)
    begin
      writer = DDS::Pub::DataWriter.new(DDS::Pub::Publisher.new(p1),
                                        DDS::Topic::Topic.new(p1, "Square_ruby", HelloWorld::Greeting),
                                        reliable_writer)
      reader = DDS::Sub::DataReader.new(DDS::Sub::Subscriber.new(p2),
                                        DDS::Topic::Topic.new(p2, "Square_ruby", HelloWorld::Greeting),
                                        reliable_reader)
      writer.wait_for_subscriptions(1, TEN_SECONDS)
      writer.write(greeting(1, "secret"))
      got = []
      eventually do
        got.concat(reader.take_data.map(&:message))
        got.any?
      end
      assert_equal ["secret"], got
    ensure
      p1.close
      p2.close
    end
  end
end
