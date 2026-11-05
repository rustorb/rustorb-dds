// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.sub;

import java.io.Serializable;
import java.util.ListIterator;
import org.omg.dds.core.DDSObject;
import org.omg.dds.core.ModifiableInstanceHandle;
import org.omg.dds.core.Time;

/** A sample read: its data (null if it has none: a dispose or an
 * unregistration) and its SampleInfo. */
public interface Sample<TYPE> extends Cloneable, Serializable, DDSObject {
    public TYPE getData();

    public SampleState getSampleState();

    public ViewState getViewState();

    public InstanceState getInstanceState();

    public Time getSourceTimestamp();

    public ModifiableInstanceHandle getInstanceHandle();

    public ModifiableInstanceHandle getPublicationHandle();

    public int getDisposedGenerationCount();

    public int getNoWritersGenerationCount();

    public int getSampleRank();

    public int getGenerationRank();

    public int getAbsoluteGenerationRank();

    public Sample<TYPE> clone();

    /** The samples of a read or take. rustorb-dds copies them: closing
     * does nothing else. remove, set and add throw
     * UnsupportedOperationException. */
    public static interface Iterator<IT_DATA> extends java.io.Closeable, ListIterator<Sample<IT_DATA>> {
        @Override
        public void close() throws java.io.IOException;

        @Override
        public void remove();

        @Override
        public void set(Sample<IT_DATA> o);

        @Override
        public void add(Sample<IT_DATA> o);
    }
}
