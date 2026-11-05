// Subscriber.DataState: the sample, view and instance states to select.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.rustorb.dds;

import java.util.Collections;
import java.util.EnumSet;
import java.util.Set;

import org.omg.dds.core.ServiceEnvironment;
import org.omg.dds.sub.InstanceState;
import org.omg.dds.sub.SampleState;
import org.omg.dds.sub.Subscriber;
import org.omg.dds.sub.ViewState;

/** The states of each kind; a kind with none selects any state of it. */
final class DataStateImpl implements Subscriber.DataState {
    private final ServiceEnvironment env;
    private final EnumSet<SampleState> samples;
    private final EnumSet<ViewState> views;
    private final EnumSet<InstanceState> instances;

    DataStateImpl(ServiceEnvironment env) {
        this(env, EnumSet.noneOf(SampleState.class), EnumSet.noneOf(ViewState.class),
                EnumSet.noneOf(InstanceState.class));
    }

    private DataStateImpl(ServiceEnvironment env, EnumSet<SampleState> s, EnumSet<ViewState> v,
            EnumSet<InstanceState> i) {
        this.env = env;
        this.samples = s;
        this.views = v;
        this.instances = i;
    }

    static DataStateImpl of(ServiceEnvironment env, Subscriber.DataState s) {
        if (s == null) {
            return new DataStateImpl(env);
        }
        if (s instanceof DataStateImpl d) {
            return d;
        }
        DataStateImpl d = new DataStateImpl(env);
        d.samples.addAll(s.getSampleStates());
        d.views.addAll(s.getViewStates());
        d.instances.addAll(s.getInstanceStates());
        return d;
    }

    int sampleMask() {
        int m = 0;
        for (SampleState s : samples) {
            m |= s.value;
        }
        return m == 0 ? 3 : m;
    }

    int viewMask() {
        int m = 0;
        for (ViewState s : views) {
            m |= s.value;
        }
        return m == 0 ? 3 : m;
    }

    int instanceMask() {
        int m = 0;
        for (InstanceState s : instances) {
            m |= s.value;
        }
        return m == 0 ? 7 : m;
    }

    @Override
    public ServiceEnvironment getEnvironment() {
        return env;
    }

    @Override
    public Set<SampleState> getSampleStates() {
        return Collections.unmodifiableSet(samples);
    }

    @Override
    public Set<ViewState> getViewStates() {
        return Collections.unmodifiableSet(views);
    }

    @Override
    public Set<InstanceState> getInstanceStates() {
        return Collections.unmodifiableSet(instances);
    }

    @Override
    public Subscriber.DataState with(SampleState state) {
        samples.add(state);
        return this;
    }

    @Override
    public Subscriber.DataState with(ViewState state) {
        views.add(state);
        return this;
    }

    @Override
    public Subscriber.DataState with(InstanceState state) {
        instances.add(state);
        return this;
    }

    @Override
    public Subscriber.DataState withAnySampleState() {
        samples.addAll(EnumSet.allOf(SampleState.class));
        return this;
    }

    @Override
    public Subscriber.DataState withAnyViewState() {
        views.addAll(EnumSet.allOf(ViewState.class));
        return this;
    }

    @Override
    public Subscriber.DataState withAnyInstanceState() {
        instances.addAll(EnumSet.allOf(InstanceState.class));
        return this;
    }

    @Override
    public Subscriber.DataState withNotAliveInstanceStates() {
        instances.add(InstanceState.NOT_ALIVE_DISPOSED);
        instances.add(InstanceState.NOT_ALIVE_NO_WRITERS);
        return this;
    }

    @Override
    public DataStateImpl clone() {
        return new DataStateImpl(env, samples.clone(), views.clone(), instances.clone());
    }

    @Override
    public boolean equals(Object o) {
        return o instanceof DataStateImpl d && sampleMask() == d.sampleMask() && viewMask() == d.viewMask()
                && instanceMask() == d.instanceMask();
    }

    @Override
    public int hashCode() {
        return (sampleMask() << 8) | (viewMask() << 4) | instanceMask();
    }

    @Override
    public String toString() {
        return "DataState(" + samples + ", " + views + ", " + instances + ")";
    }
}
