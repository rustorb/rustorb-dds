# frozen_string_literal: true

# What the tests share: the gem from lib/, the types rustorb-idl -bruby
# -Wbdds_only wrote for the test IDL (generated/), and participants on the
# loopback interface only, in domains of their own.
#
# Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

$LOAD_PATH.unshift(File.expand_path("../lib", __dir__))
$LOAD_PATH.unshift(File.expand_path("generated", __dir__))

require "minitest/autorun"
require "rustorb/dds"
require "Generated_dds"
require "HelloWorld_dds"
require "Keyed_dds"
require "CppTypes_dds"

module TestHelper
  ROOT = File.expand_path("../..", __dir__)
  TEN_SECONDS = DDS::Core::Duration.new(10, 0)

  @domain = 80
  @lock = Mutex.new

  def self.next_domain
    @lock.synchronize { @domain += 1 }
  end

  def participant(qos = nil, domain: TestHelper.next_domain)
    DDS::Domain::DomainParticipant.new(domain, qos, nil, DDS::Core::Status::StatusMask::ALL, localhost: true)
  end

  def reliable_writer
    DDS::Pub::DataWriterQos.new << DDS::Core::Policy::Reliability.reliable << DDS::Core::Policy::History.keep_all
  end

  def reliable_reader
    DDS::Sub::DataReaderQos.new << DDS::Core::Policy::Reliability.reliable << DDS::Core::Policy::History.keep_all
  end

  def eventually(seconds = 10)
    deadline = Process.clock_gettime(Process::CLOCK_MONOTONIC) + seconds
    until yield
      flunk "timed out" if Process.clock_gettime(Process::CLOCK_MONOTONIC) > deadline
      sleep 0.01
    end
  end
end
