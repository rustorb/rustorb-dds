// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.topic;

import java.util.List;

/** A topic whose readers get only the samples of its related topic that
 * pass a filter (DDS 1.4, 2.2.2.3.3; the filter expressions of Annex B). */
public interface ContentFilteredTopic<TYPE> extends TopicDescription<TYPE> {
    public String getFilterExpression();

    public List<String> getExpressionParameters();

    public void setExpressionParameters(List<String> expressionParameters);

    public void setExpressionParameters(String... expressionParameters);

    public Topic<? extends TYPE> getRelatedTopic();
}
