//! `Topic`.

use std::marker::PhantomData;
use std::sync::Arc;

use rustorb_typesupport::TopicType;

use crate::participant::Shared;

/// A DDS topic (DDS 1.4, 2.2.2.3.2): a name and the type `T` of its
/// samples. Writers and readers match by topic name and type name.
pub struct Topic<T: TopicType> {
    name: String,
    _shared: Arc<Shared>,
    _type: PhantomData<fn() -> T>,
}

impl<T: TopicType> Topic<T> {
    pub(crate) fn new(name: &str, shared: Arc<Shared>) -> Self {
        Topic {
            name: name.to_owned(),
            _shared: shared,
            _type: PhantomData,
        }
    }

    /// The topic name.
    pub fn name(&self) -> &str {
        &self.name
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
