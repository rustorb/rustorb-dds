//! `Topic`.

use std::sync::Arc;

use crate::participant::Shared;
use crate::status::InconsistentTopicStatus;
use crate::typed::SampleType;

/// A DDS topic (DDS 1.4, 2.2.2.3.2): a name and the type of its samples,
/// which are of Rust type `T`: a [`TopicType`](crate::TopicType) the
/// program was built with, or [`DynamicData`](crate::DynamicData) of a
/// type known at run time
/// ([`DomainParticipant::create_dynamic_topic`](crate::DomainParticipant::create_dynamic_topic)).
/// Writers and readers match by topic name and type name, or by type
/// assignability for registered types.
pub struct Topic<T> {
    name: String,
    shared: Arc<Shared>,
    pub(crate) samples: Arc<dyn SampleType<T>>,
}

impl<T> Topic<T> {
    pub(crate) fn new(name: &str, shared: Arc<Shared>, samples: Arc<dyn SampleType<T>>) -> Self {
        Topic {
            name: name.to_owned(),
            shared,
            samples,
        }
    }

    /// The topic name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The INCONSISTENT_TOPIC status (DDS 1.4, 2.2.4.1): the writers and
    /// readers of this topic found in other participants whose types are
    /// not consistent with those of the local ones. Reading it resets
    /// `total_count_change`.
    pub fn inconsistent_topic_status(&self) -> InconsistentTopicStatus {
        let mut g = self.shared.lock_any();
        g.inconsistent_topics
            .get_mut(&self.name)
            .map(|s| s.take())
            .unwrap_or_default()
    }

    /// The type name: `T::TYPE_NAME` for a [`TopicType`](crate::TopicType),
    /// the name of the DynamicType otherwise.
    pub fn type_name(&self) -> &str {
        self.samples.type_name()
    }
}

impl<T> std::fmt::Debug for Topic<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Topic")
            .field("name", &self.name)
            .field("type_name", &self.type_name())
            .finish()
    }
}
