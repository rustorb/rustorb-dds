# frozen_string_literal: true

# rustorb-dds for Ruby: DDS (DDS 1.4, DDS-XTypes 1.3, RTPS 2.5) on
# rustorb-dds through its C ABI, with an API after the C++ PSM
# (DDS-PSM-Cxx 1.0) in Ruby's names. Types come from rustorb-idl -bruby
# -Wbdds_only (or -Wbdds). See docs/ruby.md of rustorb-dds.
#
#   require "rustorb/dds"
#   require_relative "HelloWorld_dds"
#
#   dp = DDS::Domain::DomainParticipant.new(0)
#   topic = DDS::Topic::Topic.new(dp, "HelloWorldTopic", HelloWorld::Greeting)
#   writer = DDS::Pub::DataWriter.new(DDS::Pub::Publisher.new(dp), topic)
#   writer.write(HelloWorld::Greeting.new(1, "hello"))
#
# Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

require_relative "dds/version"
require_relative "dds/core"
require_relative "dds/rustorb_dds_ext"
require_relative "dds/native"
require_relative "dds/types"
require_relative "dds/qos"
require_relative "dds/pub"
require_relative "dds/sub"
require_relative "dds/topic"
require_relative "dds/domain"

DDS::Native.setup(DDS::Core::ERRORS)
if DDS::Native.abi_version != DDS::ABI_VERSION
  raise LoadError, "librustorb_dds_ffi has ABI #{DDS::Native.abi_version}, the gem needs #{DDS::ABI_VERSION}"
end
