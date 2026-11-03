//! `Topic`.

use std::marker::PhantomData;
use std::sync::Arc;

use rustorb_typesupport::TopicType;

use crate::participant::Shared;
use crate::status::InconsistentTopicStatus;

/// A DDS topic (DDS 1.4, 2.2.2.3.2): a name and the type `T` of its
/// samples. Writers and readers match by topic name and type name.
pub struct Topic<T: TopicType> {
    name: String,
    shared: Arc<Shared>,
    _type: PhantomData<fn() -> T>,
}

impl<T: TopicType> Topic<T> {
    pub(crate) fn new(name: &str, shared: Arc<Shared>) -> Self {
        Topic {
            name: name.to_owned(),
            shared,
            _type: PhantomData,
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

    /// The type name, as given by the type support (`T::TYPE_NAME`).
    pub fn type_name(&self) -> &'static str {
        T::TYPE_NAME
    }
}

impl<T: TopicType> std::fmt::Debug for Topic<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Topic")
            .field("name", &self.name)
            .field("type_name", &T::TYPE_NAME)
            .finish()
    }
}
