# frozen_string_literal: true

# ping of bench/bindings.py in Ruby (rustorb-dds#107): the round trip
# through `rustorb-dds-perf pong`, as `rustorb-dds-perf ping` measures it.
#
#   ruby ping.rb <domain> <sizes> <warmup_s> <duration_s>
#
# Writes RustorbPerf::Sample (bench/bindings/generated/ruby, from
# rustorb-idl -bruby -Wbdds_only) on RustorbPerf_ping (reliable,
# KEEP_LAST 1), waits on a WaitSet for the sample of the same seq on
# RustorbPerf_pong, and prints one line of JSON per size. The gem's
# extension must be built (cd ruby && rake compile).

require "json"
$LOAD_PATH.unshift(File.expand_path("../../../ruby/lib", __dir__))
require "rustorb/dds"
require_relative "../generated/ruby/Perf_dds"

include DDS::Core::Policy # rubocop:disable Style/MixinUsage

def now = Process.clock_gettime(Process::CLOCK_MONOTONIC, :nanosecond)

def at(ns, p)
  return 0.0 if ns.empty?

  ns[[[(p * ns.size).ceil, 1].max, ns.size].min - 1] / 1000.0
end

domain = Integer(ARGV[0])
sizes = ARGV[1].split(",").map { |s| Integer(s) }
warmup = (Float(ARGV[2]) * 1e9).to_i
duration = (Float(ARGV[3]) * 1e9).to_i

dp = DDS::Domain::DomainParticipant.new(domain)
begin
  ping_topic = DDS::Topic::Topic.new(dp, "RustorbPerf_ping", RustorbPerf::Sample)
  pong_topic = DDS::Topic::Topic.new(dp, "RustorbPerf_pong", RustorbPerf::Sample)
  writer = DDS::Pub::DataWriter.new(DDS::Pub::Publisher.new(dp), ping_topic,
                                    DDS::Pub::DataWriterQos.new << Reliability.reliable << History.keep_last(1))
  reader = DDS::Sub::DataReader.new(DDS::Sub::Subscriber.new(dp), pong_topic,
                                    DDS::Sub::DataReaderQos.new << Reliability.reliable << History.keep_last(1))
  writer.wait_for_subscriptions(1, 30)
  status = reader.status_condition
  status.enabled_statuses = DDS::Core::Status::StatusMask::DATA_AVAILABLE
  waitset = DDS::Core::Cond::WaitSet.new
  waitset << status
  start = now
  seq = 0
  sizes.each do |size|
    sample = RustorbPerf::Sample.new(0, 0, "\0".b * size)
    reader.take
    warm_end = now + warmup
    finish = warm_end + duration
    rtts = []
    lost = 0
    while now < finish
      measuring = now >= warm_end
      seq += 1
      sent = now
      sample.seq = seq
      sample.stamp = sent - start
      writer.write(sample)
      deadline = sent + 1_000_000_000
      loop do
        begin
          waitset.wait([(deadline - now) / 1e9, 0].max)
        rescue DDS::Core::TimeoutError
          lost += 1 if measuring
          break
        end
        if reader.take.any? { |s| s.valid? && s.data.seq == seq }
          rtts << (now - sent) if measuring
          break
        end
      end
    end
    rtts.sort!
    puts JSON.generate(
      tool: "bench/bindings/ruby", impl: "rustorb-dds-ruby", mode: "ping", size: size,
      reliability: "reliable", lost: lost, seconds: duration / 1e9, count: rtts.size,
      rtt_min_us: at(rtts, 0), rtt_p50_us: at(rtts, 0.5), rtt_p90_us: at(rtts, 0.9),
      rtt_p99_us: at(rtts, 0.99), rtt_max_us: rtts.empty? ? 0.0 : rtts[-1] / 1000.0
    )
    $stdout.flush
  end
ensure
  dp.close
end
