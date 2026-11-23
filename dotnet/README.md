# Rustorb.Dds

DDS for .NET on [rustorb-dds](https://github.com/rustorb/rustorb-dds): the
structure of the DDS Java 5 PSM in C# (`Omg.Dds.*`), over the C ABI of
`librustorb_dds_ffi`. Types come from
[rustorb-idl](https://github.com/rustorb/rustorb-idl)
`-bcsharp -Wbdds_only`.

```csharp
using DomainParticipant dp = new DomainParticipant(0);
Topic<HelloWorld.Greeting> topic = dp.CreateTopic<HelloWorld.Greeting>("hello_world");
DataWriter<HelloWorld.Greeting> writer = dp.CreatePublisher().CreateDataWriter(topic,
    DataWriterQos.Default with { Reliability = Reliability.Reliable() });
writer.Write(new HelloWorld.Greeting(1, "Hello, world!"));
```

It talks with Cyclone DDS, Fast DDS, OpenDDS and the other bindings of
rustorb-dds (Rust, Python, C++, Java). See
[docs/dotnet.md](https://github.com/rustorb/rustorb-dds/blob/main/docs/dotnet.md)
(in Japanese).

Licensed under MIT OR Apache-2.0.
