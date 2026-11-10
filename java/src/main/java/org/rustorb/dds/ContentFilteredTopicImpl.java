// ContentFilteredTopic, on rdds_cftopic.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.rustorb.dds;

import static java.lang.foreign.ValueLayout.JAVA_LONG;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.util.Arrays;
import java.util.List;

import org.omg.dds.core.ServiceEnvironment;
import org.omg.dds.domain.DomainParticipant;
import org.omg.dds.topic.ContentFilteredTopic;
import org.omg.dds.topic.Topic;
import org.omg.dds.type.TypeSupport;

final class ContentFilteredTopicImpl<T> implements ContentFilteredTopic<T> {
    final ServiceEnvironment env;
    final TopicImpl<T> related;
    private final String name;
    private final String expression;
    private volatile List<String> parameters;
    private volatile long handle;

    ContentFilteredTopicImpl(TopicImpl<T> related, String name, String expression, List<String> parameters) {
        this.env = related.env;
        this.related = related;
        this.name = name;
        this.expression = expression;
        this.parameters = List.copyOf(parameters);
        this.handle = EntityBase.make(env, (arena, out) -> {
            MemorySegment params = strings(arena, this.parameters);
            return (int) Ffi.rdds_participant_create_contentfilteredtopic.invokeExact(related.participant.handle(),
                    arena.allocateFrom(name).address(), related.handle(), arena.allocateFrom(expression).address(),
                    params.address(), (long) this.parameters.size(), out);
        });
    }

    /** An array of C strings, in `arena`. */
    private static MemorySegment strings(Arena arena, List<String> v) {
        MemorySegment a = arena.allocate(JAVA_LONG, Math.max(1, v.size()));
        for (int i = 0; i < v.size(); i++) {
            a.setAtIndex(JAVA_LONG, i, arena.allocateFrom(v.get(i)).address());
        }
        return a;
    }

    long handle() {
        long h = handle;
        if (h == 0) {
            throw Errors.closed(env, "ContentFilteredTopic " + name);
        }
        return h;
    }

    Codec<T> codec() {
        return related.codec();
    }

    @Override
    public String getFilterExpression() {
        return expression;
    }

    @Override
    public List<String> getExpressionParameters() {
        return parameters;
    }

    @Override
    public void setExpressionParameters(List<String> expressionParameters) {
        List<String> p = List.copyOf(expressionParameters);
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment params = strings(arena, p);
            EntityBase.call(env, () -> (int) Ffi.rdds_cftopic_set_expression_parameters.invokeExact(handle(),
                    params.address(), (long) p.size()));
        }
        parameters = p;
    }

    @Override
    public void setExpressionParameters(String... expressionParameters) {
        setExpressionParameters(Arrays.asList(expressionParameters));
    }

    @Override
    public Topic<? extends T> getRelatedTopic() {
        return related;
    }

    @Override
    public TypeSupport<T> getTypeSupport() {
        return related.getTypeSupport();
    }

    @SuppressWarnings("unchecked")
    @Override
    public <OTHER> ContentFilteredTopicImpl<OTHER> cast() {
        return (ContentFilteredTopicImpl<OTHER>) this;
    }

    @Override
    public String getTypeName() {
        return related.getTypeName();
    }

    @Override
    public String getName() {
        return name;
    }

    @Override
    public DomainParticipant getParent() {
        return related.participant;
    }

    @Override
    public ServiceEnvironment getEnvironment() {
        return env;
    }

    @Override
    public synchronized void close() {
        long h = handle;
        handle = 0;
        if (h != 0) {
            related.participant.filteredTopics.remove(this);
            try {
                Ffi.rdds_cftopic_release.invokeExact(h);
            } catch (Throwable t) {
                throw Library.rethrow(t);
            }
        }
    }

    @Override
    public String toString() {
        return "ContentFilteredTopic(" + name + ", " + related.getName() + ", " + expression + ")";
    }
}
