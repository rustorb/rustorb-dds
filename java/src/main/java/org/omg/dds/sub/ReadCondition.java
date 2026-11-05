// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.sub;

import java.io.Closeable;
import java.util.Set;
import org.omg.dds.core.Condition;

/** True while the reader has samples in the states of the condition. */
public interface ReadCondition<TYPE> extends Closeable, Condition {
    public Set<SampleState> getSampleStates();

    public Set<ViewState> getViewStates();

    public Set<InstanceState> getInstanceStates();

    public DataReader<TYPE> getParent();

    @Override
    public void close();
}
