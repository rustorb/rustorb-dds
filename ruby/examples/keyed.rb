# frozen_string_literal: true

# Keyed instances over DDS, from Ruby: write, dispose and unregister.
#
# The same as the Rust example keyed (topic "keyed" of Keyed::Shape, keyed
# by id) and the same command line, used by interop/run.sh:
#
#   keyed.rb pub <domain> <count> <interval_ms>
#   keyed.rb sub <domain> <count> <timeout_s>
#
# The publisher writes count samples to the instances id 1, 2, 3 in turn,
# then disposes of id 2 and unregisters id 3. The subscriber exits once it
# has count samples, each instance's in order, and has seen id 2
# NOT_ALIVE_DISPOSED and id 3 NOT_ALIVE_NO_WRITERS. With
# RUSTORB_DDS_LOCALHOST set, only the loopback interface is used.
#
# Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

$LOAD_PATH.unshift(File.expand_path("../lib", __dir__))
require "rustorb/dds"
require_relative "generated/Keyed_dds"

include DDS::Core::Policy # rubocop:disable Style/MixinUsage

MESSAGE = "Hello, instance!"
InstanceState = DDS::Sub::Status::InstanceState

def fail!(why)
  warn "rustorb-dds (Ruby): #{why}"
  exit 1
end

role = ARGV[0] || "pub"
domain = Integer(ARGV[1] || 0)
count = Integer(ARGV[2] || 30)
dp = DDS::Domain::DomainParticipant.new(domain, nil, nil, DDS::Core::Status::StatusMask::ALL,
                                        localhost: !ENV["RUSTORB_DDS_LOCALHOST"].nil?)
begin
  topic = DDS::Topic::Topic.new(dp, "keyed", Keyed::Shape)
  case role
  when "pub"
    interval = Integer(ARGV[3] || 100) / 1000.0
    qos = DDS::Pub::DataWriterQos.new << Reliability.reliable << History.keep_all << WriterDataLifecycle.new(false)
    writer = DDS::Pub::DataWriter.new(DDS::Pub::Publisher.new(dp), topic, qos)
    puts "rustorb-dds (Ruby): waiting for a subscriber..."
    begin
      writer.wait_for_subscriptions(1, 1)
    rescue DDS::Core::TimeoutError
      retry
    end
    sleep 0.5
    writer.register_instance(Keyed::Shape.new(2, 0, ""))
    (1..count).each do |i|
      id = ((i - 1) % 3) + 1
      writer.write(Keyed::Shape.new(id, i, MESSAGE))
      puts "rustorb-dds (Ruby): sent id #{id} index #{i}"
      $stdout.flush
      sleep interval
    end
    writer.dispose_instance(Keyed::Shape.new(2, 0, ""))
    writer.unregister_instance(Keyed::Shape.new(3, 0, ""))
    puts "rustorb-dds (Ruby): disposed 2, unregistered 3"
    writer.wait_for_acknowledgments(30)
    puts "rustorb-dds (Ruby): all acknowledged"
  when "sub"
    timeout = Integer(ARGV[3] || 3600)
    deadline = Process.clock_gettime(Process::CLOCK_MONOTONIC) + timeout
    qos = DDS::Sub::DataReaderQos.new << Reliability.reliable << History.keep_all
    reader = DDS::Sub::DataReader.new(DDS::Sub::Subscriber.new(dp), topic, qos)
    puts "rustorb-dds (Ruby): waiting for shapes..."
    $stdout.flush
    got = 0
    last = Hash.new(0)
    # The id of each instance, from its samples: a sample without data can
    # come after its instance is gone.
    ids = {}
    disposed = no_writers = false
    while got < count || !disposed || !no_writers
      if Process.clock_gettime(Process::CLOCK_MONOTONIC) >= deadline
        fail!("got #{got} of #{count} samples, disposed #{disposed}, no writers #{no_writers}")
      end
      begin
        reader.wait_for_data(0.2)
      rescue DDS::Core::TimeoutError
        next
      end
      reader.take.each do |s|
        if s.valid?
          d = s.data
          if d.message != MESSAGE || d.index <= last[d.id] || ((d.index - 1) % 3) + 1 != d.id
            fail!("bad sample id #{d.id} index #{d.index}")
          end
          last[d.id] = d.index
          ids[s.info.instance_handle] = d.id
          got += 1
          puts "rustorb-dds (Ruby): received id #{d.id} index #{d.index}"
        else
          id = ids[s.info.instance_handle] || reader.key_value(s.info.instance_handle).id
          state = s.info.instance_state
          puts "rustorb-dds (Ruby): id #{id} is #{state}"
          disposed ||= id == 2 && state == InstanceState::NOT_ALIVE_DISPOSED
          no_writers ||= id == 3 && state == InstanceState::NOT_ALIVE_NO_WRITERS
        end
        $stdout.flush
      end
    end
  else
    fail!("unknown role #{role}: use pub or sub")
  end
ensure
  dp.close
end
