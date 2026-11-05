// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core;

import java.lang.reflect.Constructor;
import java.lang.reflect.InvocationTargetException;
import java.util.Map;
import java.util.Set;
import java.util.concurrent.TimeUnit;
import org.omg.dds.core.policy.PolicyFactory;
import org.omg.dds.core.status.Status;
import org.omg.dds.domain.DomainParticipantFactory;
import org.omg.dds.type.TypeSupport;

/**
 * An instance of a DDS implementation in the JVM, the root of the other
 * objects. {@link #createInstance(ClassLoader)} loads the class named by
 * the system property {@value #IMPLEMENTATION_CLASS_NAME_PROPERTY}, or,
 * when it is not set, rustorb-dds's ({@value #DEFAULT_IMPLEMENTATION_CLASS_NAME}).
 */
public abstract class ServiceEnvironment implements DDSObject {
    public static final String IMPLEMENTATION_CLASS_NAME_PROPERTY = "org.omg.dds.serviceClassName";

    /** The implementation used when the property is not set (rustorb-dds's). */
    public static final String DEFAULT_IMPLEMENTATION_CLASS_NAME = "org.rustorb.dds.RustorbServiceEnvironment";

    public static ServiceEnvironment createInstance(ClassLoader classLoader) {
        return createInstance(IMPLEMENTATION_CLASS_NAME_PROPERTY, null, classLoader);
    }

    public static ServiceEnvironment createInstance(
            String implClassNameProperty, Map<String, Object> environment, ClassLoader classLoader) {
        String className = System.getProperty(implClassNameProperty);
        if (className == null || className.isEmpty()) {
            className = DEFAULT_IMPLEMENTATION_CLASS_NAME;
        }
        if (classLoader == null) {
            classLoader = ServiceEnvironment.class.getClassLoader();
        }
        if (classLoader == null) {
            classLoader = ClassLoader.getSystemClassLoader();
        }
        try {
            Class<?> c = classLoader.loadClass(className);
            Object env;
            try {
                Constructor<?> ctor = c.getConstructor(Map.class);
                env = ctor.newInstance(environment);
            } catch (NoSuchMethodException e) {
                env = c.getConstructor().newInstance();
            }
            return (ServiceEnvironment) env;
        } catch (ExceptionInInitializerError e) {
            throw new ServiceInitializationException("cannot initialize " + className, e.getCause());
        } catch (InvocationTargetException e) {
            throw new ServiceInitializationException("cannot initialize " + className, e.getCause());
        } catch (ReflectiveOperationException | LinkageError | ClassCastException e) {
            throw new ServiceConfigurationException("cannot load the DDS implementation " + className, e);
        }
    }

    /** The factories of the implementation. */
    public abstract ServiceProviderInterface getSPI();

    @Override
    public final ServiceEnvironment getEnvironment() {
        return this;
    }

    /** What the static factory methods of the PSM classes call. */
    public static interface ServiceProviderInterface {
        public DomainParticipantFactory getParticipantFactory();

        public <TYPE> TypeSupport<TYPE> newTypeSupport(Class<TYPE> type, String registeredName);

        public Duration newDuration(long duration, TimeUnit unit);

        public Duration infiniteDuration();

        public Duration zeroDuration();

        public ModifiableTime newTime(long time, TimeUnit units);

        public Time invalidTime();

        public ModifiableInstanceHandle newInstanceHandle();

        public InstanceHandle nilHandle();

        public GuardCondition newGuardCondition();

        public WaitSet newWaitSet();

        public Set<Class<? extends Status>> allStatusKinds();

        public Set<Class<? extends Status>> noStatusKinds();

        public PolicyFactory getPolicyFactory();
    }
}
