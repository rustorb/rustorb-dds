# rustorb-dds (Ruby)

The Ruby binding of [rustorb-dds](https://github.com/rustorb/rustorb-dds),
a DDS implementation written in Rust (DDS 1.4, DDS-XTypes 1.3, RTPS 2.5):
the structure of the DDS C++ PSM in Ruby (`DDS::Domain`, `DDS::Pub`,
`DDS::Sub`, `DDS::Topic`, `DDS::Core`) as a C extension on rustorb-dds's C
ABI. Types are generated from IDL with
[rustorb-idl](https://github.com/rustorb/rustorb-idl):
`rustorb-idl -bruby -Wbdds_only Foo.idl` writes `Foo_dds.rb`.

```ruby
require "rustorb/dds"
require_relative "HelloWorld_dds"

dp = DDS::Domain::DomainParticipant.new(0)
topic = DDS::Topic::Topic.new(dp, "hello_world", HelloWorld::Greeting)
qos = DDS::Pub::DataWriterQos.new << DDS::Core::Policy::Reliability.reliable
writer = DDS::Pub::DataWriter.new(DDS::Pub::Publisher.new(dp), topic, qos)
writer.write(HelloWorld::Greeting.new(1, "Hello, world!"))
```

From a checkout: `cd ruby && rake` builds `librustorb_dds_ffi` (cargo) and
the extension and runs the tests. See
[docs/ruby.md](https://github.com/rustorb/rustorb-dds/blob/main/docs/ruby.md).

Licensed under MIT OR Apache-2.0.
