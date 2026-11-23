# frozen_string_literal: true

# Hello world over DDS, from Ruby (rustorb-dds's gem).
#
# In one terminal: ruby hello_world.rb sub
# In another:      ruby hello_world.rb pub
#
# The same as the Rust example hello_world (topic "hello_world" of
# HelloWorld::Greeting) and the same command line, used by interop/run.sh:
#
#   hello_world.rb pub <domain> <count> <interval_ms> [best_effort|reliable|transient_local]
#   hello_world.rb sub <domain> <count> <timeout_s> [best_effort|reliable|transient_local]
#
# The subscriber exits once it has count samples (all of them, in order,
# when reliable), with an error on timeout. With RUSTORB_DDS_LOCALHOST set,
# only the loopback interface is used. HelloWorld_dds.rb is what
# rustorb-idl -bruby -Wbdds_only writes for HelloWorld.idl.
#
# Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

$LOAD_PATH.unshift(File.expand_path("../lib", __dir__))
require "rustorb/dds"
require_relative "generated/HelloWorld_dds"

include DDS::Core::Policy # rubocop:disable Style/MixinUsage

MESSAGE = "Hello, world!"

def fail!(why)
  warn "rustorb-dds (Ruby): #{why}"
  exit 1
end

def wait_for_subscriber(writer)
  writer.wait_for_subscriptions(1, 1)
rescue DDS::Core::TimeoutError
  retry
end

role = ARGV[0] || "pub"
domain = Integer(ARGV[1] || 0)
count = Integer(ARGV[2] || 10)
kind = ARGV[4] || "best_effort"
fail!("unknown reliability #{kind}") unless %w[best_effort reliable transient_local].include?(kind)
reliable = kind != "best_effort"
transient_local = kind == "transient_local"
durability = transient_local ? Durability.transient_local : Durability.volatile

dp = DDS::Domain::DomainParticipant.new(domain, nil, nil, DDS::Core::Status::StatusMask::ALL,
                                        localhost: !ENV["RUSTORB_DDS_LOCALHOST"].nil?)
begin
  topic = DDS::Topic::Topic.new(dp, "hello_world", HelloWorld::Greeting)
  case role
  when "pub"
    interval = Integer(ARGV[3] || 1000) / 1000.0
    qos = DDS::Pub::DataWriterQos.new
    if reliable
      qos << Reliability.reliable << History.keep_all << durability
    else
      qos << Reliability.best_effort
    end
    writer = DDS::Pub::DataWriter.new(DDS::Pub::Publisher.new(dp), topic, qos)
    unless transient_local
      puts "rustorb-dds (Ruby): waiting for a subscriber..."
      wait_for_subscriber(writer)
      sleep 0.5
    end
    (1..count).each do |i|
      writer.write(HelloWorld::Greeting.new(i, MESSAGE))
      puts "rustorb-dds (Ruby): sent #{i}"
      $stdout.flush
      sleep interval
    end
    wait_for_subscriber(writer) if transient_local
    if reliable
      writer.wait_for_acknowledgments(30)
      puts "rustorb-dds (Ruby): all acknowledged"
    end
  when "sub"
    timeout = Integer(ARGV[3] || 3600)
    deadline = Process.clock_gettime(Process::CLOCK_MONOTONIC) + timeout
    qos = DDS::Sub::DataReaderQos.new << (reliable ? Reliability.reliable : Reliability.best_effort) <<
          History.keep_all << durability
    sleep 2 if transient_local
    reader = DDS::Sub::DataReader.new(DDS::Sub::Subscriber.new(dp), topic, qos)
    status = reader.status_condition
    status.enabled_statuses = DDS::Core::Status::StatusMask::DATA_AVAILABLE
    waitset = DDS::Core::Cond::WaitSet.new
    waitset << status
    puts "rustorb-dds (Ruby): waiting for greetings..."
    $stdout.flush
    got = 0
    while got < count
      left = deadline - Process.clock_gettime(Process::CLOCK_MONOTONIC)
      begin
        waitset.wait([left, 0].max)
      rescue DDS::Core::TimeoutError
        fail!("got #{got} of #{count} samples")
      end
      reader.take.each do |s|
        next unless s.valid?

        g = s.data
        fail!("bad message #{g.message} for #{g.index}") if g.message != MESSAGE
        puts "rustorb-dds (Ruby): received #{g.index}"
        $stdout.flush
        got += 1
        fail!("expected #{got}, got #{g.index}") if reliable && g.index != got
      end
    end
  else
    fail!("unknown role #{role}: use pub or sub")
  end
ensure
  dp.close
end
