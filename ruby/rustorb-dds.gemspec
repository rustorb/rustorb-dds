# frozen_string_literal: true

require_relative "lib/rustorb/dds/version"

Gem::Specification.new do |s|
  s.name = "rustorb-dds"
  s.version = DDS::VERSION
  s.summary = "DDS for Ruby on rustorb-dds (DDS 1.4, DDS-XTypes 1.3, RTPS 2.5)"
  s.description = <<~TEXT
    The Ruby binding of rustorb-dds, a DDS implementation written in Rust: participants,
    topics, writers and readers, QoS, listeners, WaitSets, instances, content filtered
    topics and DDS Security, with an API after the DDS C++ PSM, over rustorb-dds's C ABI.
    Types are generated with rustorb-idl -bruby -Wbdds_only.
  TEXT
  s.homepage = "https://github.com/rustorb/rustorb-dds"
  s.licenses = ["MIT", "Apache-2.0"]
  s.authors = ["rustorb contributors"]
  s.required_ruby_version = ">= 3.0"
  # vendor/: librustorb_dds_ffi.a per platform and rustorb/dds.h, which the
  # release workflow puts there before gem build.
  s.files = Dir["lib/**/*.rb", "ext/rustorb_dds_ext/*.{c,rb}", "vendor/*/librustorb_dds_ffi.a",
                "vendor/include/rustorb/dds.h", "README.md"]
  s.extensions = ["ext/rustorb_dds_ext/extconf.rb"]
  s.require_paths = ["lib"]
  s.metadata = {
    "source_code_uri" => "https://github.com/rustorb/rustorb-dds/tree/main/ruby",
    "documentation_uri" => "https://github.com/rustorb/rustorb-dds/blob/main/docs/ruby.md"
  }
end
