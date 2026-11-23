// Topic and ContentFilteredTopic (DDS 1.4, 2.2.2.3).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

using System;
using System.Collections.Generic;
using System.Collections.Immutable;
using Omg.Dds.Core;
using Omg.Dds.Core.Status;
using Omg.Dds.Domain;
using Rustorb.Dds;
using Rustorb.Dds.Native;

namespace Omg.Dds.Topic;

/// <summary>What a reader reads: a topic or a content filtered topic.</summary>
public interface ITopicDescription
{
    string Name { get; }
    string TypeName { get; }
    DomainParticipant Participant { get; }
}

/// <summary>A topic or content filtered topic of T.</summary>
public interface ITopicDescription<T> : ITopicDescription
{
}

/// <summary>A topic, whatever its type.</summary>
public interface ITopic : ITopicDescription
{
    InconsistentTopicStatus GetInconsistentTopicStatus();
}

/// <summary>A topic of T.</summary>
public sealed unsafe class Topic<T> : Entity, ITopicDescription<T>, ITopic
{
    internal readonly TopicHandle handle;
    internal readonly ICodec<T> codec;

    internal Topic(DomainParticipant participant, TopicHandle handle, string name, ICodec<T> codec)
    {
        Participant = participant;
        this.handle = handle;
        Name = name;
        this.codec = codec;
    }

    internal nint Raw => handle.Raw;

    public string Name { get; }

    public string TypeName => codec.TypeName;

    public DomainParticipant Participant { get; }

    public override InstanceHandle InstanceHandle => InstanceHandle.Nil;

    public InconsistentTopicStatus GetInconsistentTopicStatus()
    {
        RddsCountStatus s;
        Errors.Check(Ffi.rdds_topic_inconsistent_topic_status(Raw, &s));
        GC.KeepAlive(this);
        return InconsistentTopicStatus.Of(s);
    }

    /// <summary>Sets the listener (null clears it).</summary>
    public void SetListener(ITopicListener? listener, StatusKind mask = StatusKind.All)
    {
        Participant.SetGroupListener(listener, mask, (l, m) => Ffi.rdds_topic_set_listener(Raw, (RddsListener*)l, m));
        GC.KeepAlive(this);
    }

    protected override void Close()
    {
        Participant.Remove(this);
        handle.Dispose();
    }
}

/// <summary>A content filtered topic of T (DDS 1.4, 2.2.2.3.3).</summary>
public sealed unsafe class ContentFilteredTopic<T> : Entity, ITopicDescription<T>
{
    internal readonly CfTopicHandle handle;
    private ImmutableArray<string> parameters;

    internal ContentFilteredTopic(DomainParticipant participant, CfTopicHandle handle, string name,
        Topic<T> related, string expression, string[] parameters)
    {
        Participant = participant;
        this.handle = handle;
        Name = name;
        RelatedTopic = related;
        FilterExpression = expression;
        this.parameters = parameters.ToImmutableArray();
    }

    internal nint Raw => handle.Raw;

    public string Name { get; }

    public string TypeName => RelatedTopic.TypeName;

    public DomainParticipant Participant { get; }

    public Topic<T> RelatedTopic { get; }

    public string FilterExpression { get; }

    public override InstanceHandle InstanceHandle => InstanceHandle.Nil;

    /// <summary>The values of %0, %1... of the filter expression.</summary>
    public IReadOnlyList<string> ExpressionParameters
    {
        get => parameters;
        set
        {
            string[] p = new List<string>(value).ToArray();
            using (var a = new Utf8Array(p))
            {
                Errors.Check(Ffi.rdds_cftopic_set_expression_parameters(Raw, a.Pointer, (nuint)p.Length));
            }
            GC.KeepAlive(this);
            parameters = p.ToImmutableArray();
        }
    }

    protected override void Close()
    {
        Participant.Remove(this);
        handle.Dispose();
    }
}
