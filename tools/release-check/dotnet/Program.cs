// A program against the NuGet package Rustorb.Dds of a release
// (release.yml, job nuget): loads librustorb_dds_ffi from the package's
// runtimes/<rid>/native and makes a participant on the loopback interface.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

using System;
using Omg.Dds.Domain;
using Rustorb.Dds;

using DomainParticipant dp = Participants.Localhost(7);
Console.WriteLine($"rustorb-dds (.NET): domain {dp.DomainId}");
