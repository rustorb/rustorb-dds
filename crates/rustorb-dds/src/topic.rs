//! `Topic`.

use std::sync::{Arc, Mutex};

use rustorb_rtps::Guid;
use rustorb_rtps::sedp::ContentFilterProperty;
use rustorb_xtypes::dynamic::ContentFilter;
use rustorb_xtypes::dynamic::filter::DDSSQL;

use crate::error::Result;
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
    pub(crate) shared: Arc<Shared>,
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

impl<T> Clone for Topic<T> {
    fn clone(&self) -> Self {
        Topic {
            name: self.name.clone(),
            shared: self.shared.clone(),
            samples: self.samples.clone(),
        }
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

/// A topic whose readers get only the samples of its related topic that
/// pass a filter (DDS 1.4, 2.2.2.3.3): a filter expression (DDS 1.4,
/// Annex B, see [`ContentFilter`]) with parameters, which can change.
///
/// Made by
/// [`DomainParticipant::create_contentfilteredtopic`](crate::DomainParticipant::create_contentfilteredtopic).
/// Its readers announce the filter (DDSI-RTPS 2.5, 8.7.3.1), so that
/// remote writers that can filter do, and filter the samples they get
/// themselves.
pub struct ContentFilteredTopic<T> {
    name: String,
    related: Topic<T>,
    state: Arc<Mutex<FilterState>>,
}

/// What the readers of a content filtered topic share.
struct FilterState {
    filter: ContentFilter,
    readers: Vec<Guid>,
}

impl<T> ContentFilteredTopic<T> {
    pub(crate) fn new(name: &str, related: &Topic<T>, filter: ContentFilter) -> Self {
        ContentFilteredTopic {
            name: name.to_owned(),
            related: related.clone(),
            state: Arc::new(Mutex::new(FilterState {
                filter,
                readers: Vec::new(),
            })),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, FilterState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The name of the content filtered topic.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The type name, that of the related topic.
    pub fn type_name(&self) -> &str {
        self.related.type_name()
    }

    /// The related topic (`get_related_topic`).
    pub fn related_topic(&self) -> &Topic<T> {
        &self.related
    }

    /// The filter expression.
    pub fn filter_expression(&self) -> String {
        self.state().filter.expression().to_owned()
    }

    /// The expression parameters: those of the last successful
    /// [`set_expression_parameters`](Self::set_expression_parameters),
    /// or those it was made with.
    pub fn expression_parameters(&self) -> Vec<String> {
        self.state().filter.parameters().to_vec()
    }

    /// Changes the expression parameters: the readers filter with them
    /// from now on, and announce them. Fails with
    /// [`Error::Dynamic`](crate::Error::Dynamic) when there are
    /// fewer than the expression uses.
    pub fn set_expression_parameters(&self, parameters: &[String]) -> Result<()> {
        let mut st = self.state();
        st.filter.set_parameters(parameters)?;
        st.readers
            .retain(|g| self.related.shared.lock_any().readers.contains_key(g));
        self.related
            .shared
            .set_filter_parameters(&st.readers, parameters)
    }

    /// The filter and the property that announces it, for a new reader.
    pub(crate) fn reader_filter(&self) -> (ContentFilter, ContentFilterProperty) {
        let st = self.state();
        let property = ContentFilterProperty {
            content_filtered_topic_name: self.name.clone(),
            related_topic_name: self.related.name().to_owned(),
            filter_class_name: DDSSQL.to_owned(),
            filter_expression: st.filter.expression().to_owned(),
            expression_parameters: st.filter.parameters().to_vec(),
        };
        (st.filter.clone(), property)
    }

    /// Notes a reader of the topic, whose parameters change with it.
    pub(crate) fn add_reader(&self, guid: Guid) {
        self.state().readers.push(guid);
    }
}

impl<T> std::fmt::Debug for ContentFilteredTopic<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContentFilteredTopic")
            .field("name", &self.name)
            .field("related_topic", &self.related.name())
            .field("filter_expression", &self.filter_expression())
            .field("expression_parameters", &self.expression_parameters())
            .finish()
    }
}

/// What readers can be created for (`TopicDescription`, DDS 1.4,
/// 2.2.2.3.1): a [`Topic`] or a [`ContentFilteredTopic`].
pub trait TopicDescription<T>: sealed::Description<T> {
    /// The name.
    fn get_name(&self) -> &str;
    /// The type name.
    fn get_type_name(&self) -> &str;
}

pub(crate) mod sealed {
    use super::*;

    /// The topic and the filter of a reader.
    pub trait Description<T> {
        #[doc(hidden)]
        fn topic(&self) -> &Topic<T>;
        #[doc(hidden)]
        fn content_filter(&self) -> Option<&ContentFilteredTopic<T>> {
            None
        }
    }
}

impl<T> sealed::Description<T> for Topic<T> {
    fn topic(&self) -> &Topic<T> {
        self
    }
}

impl<T> TopicDescription<T> for Topic<T> {
    fn get_name(&self) -> &str {
        self.name()
    }
    fn get_type_name(&self) -> &str {
        self.type_name()
    }
}

impl<T> sealed::Description<T> for ContentFilteredTopic<T> {
    fn topic(&self) -> &Topic<T> {
        &self.related
    }
    fn content_filter(&self) -> Option<&ContentFilteredTopic<T>> {
        Some(self)
    }
}

impl<T> TopicDescription<T> for ContentFilteredTopic<T> {
    fn get_name(&self) -> &str {
        self.name()
    }
    fn get_type_name(&self) -> &str {
        self.type_name()
    }
}
