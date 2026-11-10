//! The DCPS entities: participant, topic, publisher, subscriber, writer
//! and reader, their QoS and statuses, and the conditions and WaitSet.
//!
//! The Python package (`rustorb_dds/__init__.py`) wraps these classes:
//! it turns samples into the user's classes, and statuses and sample
//! information into named tuples. Every call that waits releases the GIL.

use std::sync::Arc;
use std::time::Duration;

use pyo3::BoundObject;
use pyo3::exceptions::{PyRuntimeError, PyTimeoutError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyTuple};
use rustorb_dds::{
    Condition, ContentFilteredTopic, DataReader, DataReaderListener, DataReaderQos, DataWriter,
    DataWriterListener, DataWriterQos, DeadlineMissedStatus, DomainParticipant, DurabilityKind,
    DynamicData, GuardCondition, Guid, History, IncompatibleQosStatus, InstanceHandle,
    InstanceState, InstanceStateMask, LivelinessChangedStatus, LivelinessKind,
    LivelinessLostStatus, MatchedStatus, ParticipantConfig, Publisher, PublisherQos, Reliability,
    Sample, SampleLostStatus, SampleRejectedStatus, SampleState, SampleStateMask, StateFilter,
    StatusMask, Subscriber, SubscriberQos, Time, Topic, ViewState, ViewStateMask, WaitSet,
    data_representation,
};

use crate::types::PyDynamicType;
use crate::value::{from_py, to_py};

fn dds_err(e: rustorb_dds::Error) -> PyErr {
    match e {
        rustorb_dds::Error::Timeout => PyTimeoutError::new_err(e.to_string()),
        rustorb_dds::Error::Dynamic(_) | rustorb_dds::Error::Serialize(_) => {
            PyValueError::new_err(e.to_string())
        }
        _ => PyRuntimeError::new_err(e.to_string()),
    }
}

fn secs(s: f64) -> PyResult<Duration> {
    Duration::try_from_secs_f64(s).map_err(|e| PyValueError::new_err(e.to_string()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn guid(g: Guid) -> String {
    hex(&g.to_bytes())
}

fn handle(h: InstanceHandle) -> String {
    hex(&h.0)
}

fn parse_handle(s: &str) -> PyResult<InstanceHandle> {
    let bad = || PyValueError::new_err("an instance handle is 32 hexadecimal digits");
    if s.len() != 32 {
        return Err(bad());
    }
    let mut h = [0u8; 16];
    for (i, b) in h.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).map_err(|_| bad())?;
    }
    Ok(InstanceHandle(h))
}

// ---------- QoS -----------------------------------------------------------

/// The options common to writers and readers, from a dict (see
/// `rustorb_dds.Qos`).
struct Common {
    reliability: Reliability,
    history: History,
    durability: DurabilityKind,
    deadline: Option<Duration>,
    liveliness: rustorb_dds::Liveliness,
    data_representation: Vec<i16>,
    user_data: Vec<u8>,
    resource_limits: rustorb_dds::ResourceLimits,
}

fn get<'py>(d: &Bound<'py, PyDict>, key: &str) -> PyResult<Option<Bound<'py, PyAny>>> {
    Ok(d.get_item(key)?.filter(|v| !v.is_none()))
}

fn common(d: &Bound<'_, PyDict>, reliable_default: bool) -> PyResult<Common> {
    let mut reliability = if reliable_default {
        Reliability::reliable()
    } else {
        Reliability::best_effort()
    };
    if let Some(r) = get(d, "reliability")? {
        let r: String = r.extract()?;
        let max_blocking_time = reliability.max_blocking_time;
        reliability = match r.as_str() {
            "reliable" => Reliability::reliable(),
            "best_effort" => Reliability::best_effort(),
            _ => return Err(PyValueError::new_err(format!("unknown reliability {r:?}"))),
        };
        reliability.max_blocking_time = max_blocking_time;
    }
    if let Some(t) = get(d, "max_blocking_time")? {
        reliability.max_blocking_time = secs(t.extract()?)?;
    }
    let history = match get(d, "history")? {
        None => History::default(),
        Some(h) => match h.extract::<String>() {
            Ok(s) if s == "keep_all" => History::KeepAll,
            Ok(s) => return Err(PyValueError::new_err(format!("unknown history {s:?}"))),
            Err(_) => History::KeepLast(h.extract()?),
        },
    };
    let durability = match get(d, "durability")?
        .map(|v| v.extract::<String>())
        .transpose()?
        .as_deref()
    {
        None | Some("volatile") => DurabilityKind::Volatile,
        Some("transient_local") => DurabilityKind::TransientLocal,
        Some("transient") => DurabilityKind::Transient,
        Some("persistent") => DurabilityKind::Persistent,
        Some(s) => return Err(PyValueError::new_err(format!("unknown durability {s:?}"))),
    };
    let deadline = get(d, "deadline")?
        .map(|v| v.extract::<f64>().and_then(secs))
        .transpose()?;
    let kind = match get(d, "liveliness")?
        .map(|v| v.extract::<String>())
        .transpose()?
        .as_deref()
    {
        None | Some("automatic") => LivelinessKind::Automatic,
        Some("manual_by_participant") => LivelinessKind::ManualByParticipant,
        Some("manual_by_topic") => LivelinessKind::ManualByTopic,
        Some(s) => return Err(PyValueError::new_err(format!("unknown liveliness {s:?}"))),
    };
    let liveliness = rustorb_dds::Liveliness {
        kind,
        lease_duration: get(d, "lease_duration")?
            .map(|v| v.extract::<f64>().and_then(secs))
            .transpose()?,
    };
    let data_representation = match get(d, "data_representation")? {
        None => Vec::new(),
        Some(v) => v
            .extract::<Vec<String>>()?
            .iter()
            .map(|s| match s.as_str() {
                "xcdr" => Ok(data_representation::XCDR),
                "xcdr2" => Ok(data_representation::XCDR2),
                _ => Err(PyValueError::new_err(format!(
                    "unknown data representation {s:?}"
                ))),
            })
            .collect::<PyResult<_>>()?,
    };
    let user_data = get(d, "user_data")?
        .map(|v| v.extract::<Vec<u8>>())
        .transpose()?
        .unwrap_or_default();
    let limit = |k: &str| -> PyResult<Option<usize>> {
        get(d, k)?.map(|v| v.extract::<usize>()).transpose()
    };
    let resource_limits = rustorb_dds::ResourceLimits {
        max_samples: limit("max_samples")?,
        max_instances: limit("max_instances")?,
        max_samples_per_instance: limit("max_samples_per_instance")?,
    };
    Ok(Common {
        reliability,
        history,
        durability,
        deadline,
        liveliness,
        data_representation,
        user_data,
        resource_limits,
    })
}

fn writer_qos(d: Option<&Bound<'_, PyDict>>) -> PyResult<DataWriterQos> {
    let Some(d) = d else {
        return Ok(DataWriterQos::default());
    };
    let c = common(d, true)?;
    let mut qos = DataWriterQos {
        reliability: c.reliability,
        history: c.history,
        resource_limits: c.resource_limits,
        durability: c.durability,
        deadline: c.deadline,
        liveliness: c.liveliness,
        user_data: c.user_data,
        ..DataWriterQos::default()
    };
    if !c.data_representation.is_empty() {
        qos.data_representation = c.data_representation;
    }
    if let Some(v) = get(d, "autodispose_unregistered_instances")? {
        qos.writer_data_lifecycle.autodispose_unregistered_instances = v.extract()?;
    }
    Ok(qos)
}

fn reader_qos(d: Option<&Bound<'_, PyDict>>) -> PyResult<DataReaderQos> {
    let Some(d) = d else {
        return Ok(DataReaderQos::default());
    };
    let c = common(d, false)?;
    let mut qos = DataReaderQos {
        reliability: c.reliability,
        history: c.history,
        resource_limits: c.resource_limits,
        durability: c.durability,
        deadline: c.deadline,
        liveliness: c.liveliness,
        user_data: c.user_data,
        ..DataReaderQos::default()
    };
    if !c.data_representation.is_empty() {
        qos.data_representation = c.data_representation;
    }
    Ok(qos)
}

fn partition(d: Option<&Bound<'_, PyDict>>) -> PyResult<Vec<String>> {
    match d {
        Some(d) => Ok(get(d, "partition")?
            .map(|v| v.extract())
            .transpose()?
            .unwrap_or_default()),
        None => Ok(Vec::new()),
    }
}

// ---------- statuses ------------------------------------------------------

fn dict<'py>(
    py: Python<'py>,
    items: Vec<(&str, Bound<'py, PyAny>)>,
) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    for (k, v) in items {
        d.set_item(k, v)?;
    }
    Ok(d)
}

fn o<'py, T: IntoPyObject<'py>>(py: Python<'py>, v: T) -> PyResult<Bound<'py, PyAny>> {
    v.into_pyobject(py)
        .map(|b| b.into_any().into_bound())
        .map_err(Into::into)
}

/// A status as a dict.
pub(crate) trait Status {
    fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>>;
}

impl Status for MatchedStatus {
    fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        dict(
            py,
            vec![
                ("total_count", o(py, self.total_count)?),
                ("total_count_change", o(py, self.total_count_change)?),
                ("current_count", o(py, self.current_count)?),
                ("current_count_change", o(py, self.current_count_change)?),
                ("last_handle", o(py, self.last_handle.map(guid))?),
            ],
        )
    }
}

impl Status for LivelinessChangedStatus {
    fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        dict(
            py,
            vec![
                ("alive_count", o(py, self.alive_count)?),
                ("not_alive_count", o(py, self.not_alive_count)?),
                ("alive_count_change", o(py, self.alive_count_change)?),
                (
                    "not_alive_count_change",
                    o(py, self.not_alive_count_change)?,
                ),
                (
                    "last_publication_handle",
                    o(py, self.last_publication_handle.map(guid))?,
                ),
            ],
        )
    }
}

impl Status for LivelinessLostStatus {
    fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        dict(
            py,
            vec![
                ("total_count", o(py, self.total_count)?),
                ("total_count_change", o(py, self.total_count_change)?),
            ],
        )
    }
}

impl Status for DeadlineMissedStatus {
    fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        dict(
            py,
            vec![
                ("total_count", o(py, self.total_count)?),
                ("total_count_change", o(py, self.total_count_change)?),
                (
                    "last_instance_handle",
                    o(py, self.last_instance_handle.map(handle))?,
                ),
            ],
        )
    }
}

impl Status for IncompatibleQosStatus {
    fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let policies = self
            .policies
            .iter()
            .map(|p| (format!("{:?}", p.policy_id), p.count))
            .collect::<Vec<_>>();
        dict(
            py,
            vec![
                ("total_count", o(py, self.total_count)?),
                ("total_count_change", o(py, self.total_count_change)?),
                (
                    "last_policy_id",
                    o(py, self.last_policy_id.map(|p| format!("{p:?}")))?,
                ),
                ("policies", o(py, policies)?),
            ],
        )
    }
}

impl Status for SampleLostStatus {
    fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        dict(
            py,
            vec![
                ("total_count", o(py, self.total_count)?),
                ("total_count_change", o(py, self.total_count_change)?),
            ],
        )
    }
}

impl Status for SampleRejectedStatus {
    fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        dict(
            py,
            vec![
                ("total_count", o(py, self.total_count)?),
                ("total_count_change", o(py, self.total_count_change)?),
                (
                    "last_reason",
                    o(py, self.last_reason.map(|r| format!("{r:?}")))?,
                ),
                (
                    "last_instance_handle",
                    o(py, self.last_instance_handle.map(handle))?,
                ),
            ],
        )
    }
}

fn time(t: Time) -> f64 {
    f64::from(t.seconds) + f64::from(t.fraction) / 4_294_967_296.0
}

fn time_of(s: f64) -> PyResult<Time> {
    if !(0.0..4_294_967_296.0).contains(&s) {
        return Err(PyValueError::new_err("a timestamp out of range"));
    }
    Ok(Time {
        seconds: s as u32,
        fraction: (s.fract() * 4_294_967_296.0) as u32,
    })
}

/// A sample as a tuple `(data or None, info dict)`.
fn sample<'py>(py: Python<'py>, s: &Sample<DynamicData>) -> PyResult<Bound<'py, PyAny>> {
    let data = match &s.data {
        Some(d) => to_py(py, d)?,
        None => py.None().into_bound(py),
    };
    let i = &s.info;
    let info = dict(
        py,
        vec![
            (
                "sample_state",
                o(
                    py,
                    match i.sample_state {
                        SampleState::Read => "read",
                        SampleState::NotRead => "not_read",
                    },
                )?,
            ),
            (
                "view_state",
                o(
                    py,
                    match i.view_state {
                        ViewState::New => "new",
                        ViewState::NotNew => "not_new",
                    },
                )?,
            ),
            (
                "instance_state",
                o(
                    py,
                    match i.instance_state {
                        InstanceState::Alive => "alive",
                        InstanceState::NotAliveDisposed => "not_alive_disposed",
                        InstanceState::NotAliveNoWriters => "not_alive_no_writers",
                    },
                )?,
            ),
            ("valid_data", o(py, i.valid_data)?),
            ("source_timestamp", o(py, i.source_timestamp.map(time))?),
            ("instance_handle", o(py, handle(i.instance_handle))?),
            ("publication_handle", o(py, guid(i.writer))?),
            ("sequence_number", o(py, i.sequence_number.0)?),
            (
                "disposed_generation_count",
                o(py, i.disposed_generation_count)?,
            ),
            (
                "no_writers_generation_count",
                o(py, i.no_writers_generation_count)?,
            ),
            ("sample_rank", o(py, i.sample_rank)?),
            ("generation_rank", o(py, i.generation_rank)?),
            (
                "absolute_generation_rank",
                o(py, i.absolute_generation_rank)?,
            ),
        ],
    )?;
    Ok(PyTuple::new(py, [data, info.into_any()])?.into_any())
}

fn samples<'py>(py: Python<'py>, v: &[Sample<DynamicData>]) -> PyResult<Bound<'py, PyList>> {
    PyList::new(
        py,
        v.iter()
            .map(|s| sample(py, s))
            .collect::<PyResult<Vec<_>>>()?,
    )
}

/// A state filter from three lists of state names (None for any).
fn filter(
    sample_states: Option<Vec<String>>,
    view_states: Option<Vec<String>>,
    instance_states: Option<Vec<String>>,
) -> PyResult<StateFilter> {
    let bad = |s: &str| PyValueError::new_err(format!("unknown state {s:?}"));
    let mut f = StateFilter::ANY;
    if let Some(v) = sample_states {
        let mut m = None;
        for s in &v {
            let x = match s.as_str() {
                "read" => SampleStateMask::READ,
                "not_read" => SampleStateMask::NOT_READ,
                _ => return Err(bad(s)),
            };
            m = Some(m.map_or(x, |m| m | x));
        }
        f.sample_states = m.ok_or_else(|| bad(""))?;
    }
    if let Some(v) = view_states {
        let mut m = None;
        for s in &v {
            let x = match s.as_str() {
                "new" => ViewStateMask::NEW,
                "not_new" => ViewStateMask::NOT_NEW,
                _ => return Err(bad(s)),
            };
            m = Some(m.map_or(x, |m| m | x));
        }
        f.view_states = m.ok_or_else(|| bad(""))?;
    }
    if let Some(v) = instance_states {
        let mut m = None;
        for s in &v {
            let x = match s.as_str() {
                "alive" => InstanceStateMask::ALIVE,
                "not_alive_disposed" => InstanceStateMask::NOT_ALIVE_DISPOSED,
                "not_alive_no_writers" => InstanceStateMask::NOT_ALIVE_NO_WRITERS,
                _ => return Err(bad(s)),
            };
            m = Some(m.map_or(x, |m| m | x));
        }
        f.instance_states = m.ok_or_else(|| bad(""))?;
    }
    Ok(f)
}

// ---------- entities ------------------------------------------------------

/// `DomainParticipant`.
#[pyclass(name = "DomainParticipant", module = "rustorb_dds._native", frozen)]
#[derive(Debug)]
pub struct PyParticipant {
    inner: DomainParticipant,
}

#[pymethods]
impl PyParticipant {
    /// Joins `domain_id`. With `localhost`, discovery and data stay on
    /// the loopback interface (for tests).
    #[new]
    #[pyo3(signature = (domain_id, localhost = false))]
    fn new(py: Python<'_>, domain_id: u32, localhost: bool) -> PyResult<Self> {
        let config = if localhost {
            ParticipantConfig::localhost(domain_id)
        } else {
            ParticipantConfig::new(domain_id)
        };
        let inner = py
            .detach(|| DomainParticipant::with_config(config))
            .map_err(dds_err)?;
        Ok(PyParticipant { inner })
    }

    #[getter]
    fn domain_id(&self) -> u32 {
        self.inner.domain_id()
    }

    #[getter]
    fn guid(&self) -> String {
        guid(self.inner.guid())
    }

    /// Creates a topic of `ty`.
    fn create_topic(&self, name: &str, ty: &PyDynamicType) -> PyResult<PyTopic> {
        let topic = self
            .inner
            .create_dynamic_topic(name, &ty.ty)
            .map_err(dds_err)?;
        Ok(PyTopic {
            topic: Arc::new(topic),
            ty: ty.clone(),
        })
    }

    /// A content filtered topic of `related`.
    #[pyo3(signature = (name, related, filter_expression, expression_parameters = Vec::new()))]
    fn create_contentfilteredtopic(
        &self,
        name: &str,
        related: &PyTopic,
        filter_expression: &str,
        expression_parameters: Vec<String>,
    ) -> PyResult<PyContentFilteredTopic> {
        let topic = self
            .inner
            .create_contentfilteredtopic(
                name,
                &related.topic,
                filter_expression,
                &expression_parameters,
            )
            .map_err(dds_err)?;
        Ok(PyContentFilteredTopic {
            topic: Arc::new(topic),
            ty: related.ty.clone(),
        })
    }

    /// The type remote endpoints of `topic_name` announce, or None.
    fn discover_topic_type(
        &self,
        py: Python<'_>,
        topic_name: &str,
        timeout: f64,
    ) -> PyResult<Option<PyDynamicType>> {
        let timeout = secs(timeout)?;
        let ty = py
            .detach(|| self.inner.discover_topic_type(topic_name, timeout))
            .map_err(dds_err)?;
        Ok(ty.map(|ty| PyDynamicType { ty }))
    }

    #[pyo3(signature = (qos = None))]
    fn create_publisher(&self, qos: Option<&Bound<'_, PyDict>>) -> PyResult<PyPublisher> {
        let qos = PublisherQos {
            partition: partition(qos)?,
        };
        Ok(PyPublisher {
            inner: self.inner.create_publisher_with_qos(qos).map_err(dds_err)?,
        })
    }

    #[pyo3(signature = (qos = None))]
    fn create_subscriber(&self, qos: Option<&Bound<'_, PyDict>>) -> PyResult<PySubscriber> {
        let qos = SubscriberQos {
            partition: partition(qos)?,
        };
        Ok(PySubscriber {
            inner: self
                .inner
                .create_subscriber_with_qos(qos)
                .map_err(dds_err)?,
        })
    }

    fn assert_liveliness(&self) -> PyResult<()> {
        self.inner.assert_liveliness().map_err(dds_err)
    }

    fn discovered_participants(&self) -> Vec<String> {
        self.inner
            .discovered_participants()
            .into_iter()
            .map(|p| hex(&p.0))
            .collect()
    }

    fn learnt_types(&self) -> usize {
        self.inner.learnt_types()
    }
}

/// `Topic`.
#[pyclass(name = "Topic", module = "rustorb_dds._native", frozen)]
#[derive(Debug)]
pub struct PyTopic {
    topic: Arc<Topic<DynamicData>>,
    ty: PyDynamicType,
}

#[pymethods]
impl PyTopic {
    #[getter]
    fn name(&self) -> String {
        self.topic.name().to_owned()
    }

    #[getter]
    fn type_name(&self) -> String {
        self.topic.type_name().to_owned()
    }

    #[getter]
    fn r#type(&self) -> PyDynamicType {
        self.ty.clone()
    }

    fn inconsistent_topic_status<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let s = self.topic.inconsistent_topic_status();
        dict(
            py,
            vec![
                ("total_count", o(py, s.total_count)?),
                ("total_count_change", o(py, s.total_count_change)?),
            ],
        )
    }
}

/// `ContentFilteredTopic`.
#[pyclass(name = "ContentFilteredTopic", module = "rustorb_dds._native", frozen)]
#[derive(Debug)]
pub struct PyContentFilteredTopic {
    topic: Arc<ContentFilteredTopic<DynamicData>>,
    ty: PyDynamicType,
}

#[pymethods]
impl PyContentFilteredTopic {
    #[getter]
    fn name(&self) -> String {
        self.topic.name().to_owned()
    }

    #[getter]
    fn type_name(&self) -> String {
        self.topic.type_name().to_owned()
    }

    #[getter]
    fn r#type(&self) -> PyDynamicType {
        self.ty.clone()
    }

    #[getter]
    fn related_topic_name(&self) -> String {
        self.topic.related_topic().name().to_owned()
    }

    #[getter]
    fn filter_expression(&self) -> String {
        self.topic.filter_expression()
    }

    #[getter]
    fn expression_parameters(&self) -> Vec<String> {
        self.topic.expression_parameters()
    }

    fn set_expression_parameters(&self, parameters: Vec<String>) -> PyResult<()> {
        self.topic
            .set_expression_parameters(&parameters)
            .map_err(dds_err)
    }
}

/// `Publisher`.
#[pyclass(name = "Publisher", module = "rustorb_dds._native", frozen)]
#[derive(Debug)]
pub struct PyPublisher {
    inner: Publisher,
}

#[pymethods]
impl PyPublisher {
    /// A writer of `topic`; `listener`, a function called as in
    /// `DataWriter.set_listener`, is set before the writer is matched.
    #[pyo3(signature = (topic, qos = None, listener = None, mask = u32::MAX))]
    fn create_datawriter(
        &self,
        topic: &PyTopic,
        qos: Option<&Bound<'_, PyDict>>,
        listener: Option<Py<PyAny>>,
        mask: u32,
    ) -> PyResult<PyWriter> {
        let qos = writer_qos(qos)?;
        let writer = match listener {
            None => self.inner.create_datawriter_with_qos(&topic.topic, qos),
            Some(f) => self.inner.create_datawriter_with_listener(
                &topic.topic,
                qos,
                WriterCallback(f),
                StatusMask(mask),
            ),
        }
        .map_err(dds_err)?;
        Ok(PyWriter {
            inner: Arc::new(writer),
            ty: topic.ty.clone(),
        })
    }
}

/// `Subscriber`.
#[pyclass(name = "Subscriber", module = "rustorb_dds._native", frozen)]
#[derive(Debug)]
pub struct PySubscriber {
    inner: Subscriber,
}

#[pymethods]
impl PySubscriber {
    /// A reader of `topic`; `listener`, a function called as in
    /// `DataReader.set_listener`, is set before the reader is matched.
    #[pyo3(signature = (topic, qos = None, listener = None, mask = u32::MAX))]
    fn create_datareader(
        &self,
        topic: &PyTopic,
        qos: Option<&Bound<'_, PyDict>>,
        listener: Option<Py<PyAny>>,
        mask: u32,
    ) -> PyResult<PyReader> {
        let qos = reader_qos(qos)?;
        let reader = match listener {
            None => self.inner.create_datareader_with_qos(&*topic.topic, qos),
            Some(f) => self.inner.create_datareader_with_listener(
                &*topic.topic,
                qos,
                ReaderCallback(f),
                StatusMask(mask),
            ),
        }
        .map_err(dds_err)?;
        Ok(PyReader {
            inner: Arc::new(reader),
            ty: topic.ty.clone(),
        })
    }

    /// A reader of the content filtered topic `topic`, as
    /// `create_datareader`.
    #[pyo3(signature = (topic, qos = None, listener = None, mask = u32::MAX))]
    fn create_filtered_datareader(
        &self,
        topic: &PyContentFilteredTopic,
        qos: Option<&Bound<'_, PyDict>>,
        listener: Option<Py<PyAny>>,
        mask: u32,
    ) -> PyResult<PyReader> {
        let qos = reader_qos(qos)?;
        let reader = match listener {
            None => self.inner.create_datareader_with_qos(&*topic.topic, qos),
            Some(f) => self.inner.create_datareader_with_listener(
                &*topic.topic,
                qos,
                ReaderCallback(f),
                StatusMask(mask),
            ),
        }
        .map_err(dds_err)?;
        Ok(PyReader {
            inner: Arc::new(reader),
            ty: topic.ty.clone(),
        })
    }
}

/// `DataWriter` of DynamicData.
#[pyclass(name = "DataWriter", module = "rustorb_dds._native", frozen)]
#[derive(Debug)]
pub struct PyWriter {
    inner: Arc<DataWriter<DynamicData>>,
    ty: PyDynamicType,
}

impl PyWriter {
    fn data(&self, v: &Bound<'_, PyAny>) -> PyResult<DynamicData> {
        from_py(&self.ty.ty, v)
    }
}

#[pymethods]
impl PyWriter {
    #[getter]
    fn guid(&self) -> String {
        guid(self.inner.guid())
    }

    /// Writes a sample, with the source timestamp given (seconds since
    /// 1970) or now.
    #[pyo3(signature = (sample, timestamp = None))]
    fn write(
        &self,
        py: Python<'_>,
        sample: &Bound<'_, PyAny>,
        timestamp: Option<f64>,
    ) -> PyResult<()> {
        let d = self.data(sample)?;
        let t = timestamp
            .map(time_of)
            .transpose()?
            .unwrap_or_else(Time::now);
        py.detach(|| self.inner.write_w_timestamp(&d, t))
            .map_err(dds_err)
    }

    fn register_instance(&self, sample: &Bound<'_, PyAny>) -> PyResult<String> {
        let d = self.data(sample)?;
        self.inner
            .register_instance(&d)
            .map(handle)
            .map_err(dds_err)
    }

    #[pyo3(signature = (sample, timestamp = None))]
    fn unregister_instance(
        &self,
        sample: &Bound<'_, PyAny>,
        timestamp: Option<f64>,
    ) -> PyResult<()> {
        let d = self.data(sample)?;
        let t = timestamp
            .map(time_of)
            .transpose()?
            .unwrap_or_else(Time::now);
        self.inner
            .unregister_instance_w_timestamp(&d, t)
            .map_err(dds_err)
    }

    #[pyo3(signature = (sample, timestamp = None))]
    fn dispose(&self, sample: &Bound<'_, PyAny>, timestamp: Option<f64>) -> PyResult<()> {
        let d = self.data(sample)?;
        let t = timestamp
            .map(time_of)
            .transpose()?
            .unwrap_or_else(Time::now);
        self.inner.dispose_w_timestamp(&d, t).map_err(dds_err)
    }

    fn lookup_instance(&self, sample: &Bound<'_, PyAny>) -> PyResult<Option<String>> {
        let d = self.data(sample)?;
        Ok(self.inner.lookup_instance(&d).map(handle))
    }

    fn wait_for_acknowledgments(&self, py: Python<'_>, timeout: f64) -> PyResult<()> {
        let t = secs(timeout)?;
        py.detach(|| self.inner.wait_for_acknowledgments(t))
            .map_err(dds_err)
    }

    fn wait_for_subscriptions(&self, py: Python<'_>, count: usize, timeout: f64) -> PyResult<bool> {
        let t = secs(timeout)?;
        Ok(py.detach(|| self.inner.wait_for_subscriptions(count, t)))
    }

    fn matched_subscriptions(&self) -> Vec<String> {
        self.inner
            .matched_subscriptions()
            .into_iter()
            .map(guid)
            .collect()
    }

    fn assert_liveliness(&self) -> PyResult<()> {
        self.inner.assert_liveliness().map_err(dds_err)
    }

    fn publication_matched_status<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        self.inner.publication_matched_status().to_dict(py)
    }

    fn offered_incompatible_qos_status<'py>(
        &self,
        py: Python<'py>,
    ) -> PyResult<Bound<'py, PyDict>> {
        self.inner.offered_incompatible_qos_status().to_dict(py)
    }

    fn liveliness_lost_status<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        self.inner.liveliness_lost_status().to_dict(py)
    }

    fn offered_deadline_missed_status<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        self.inner.offered_deadline_missed_status().to_dict(py)
    }

    fn status_changes(&self) -> u32 {
        self.inner.status_changes().0
    }

    fn status_condition(&self) -> PyCondition {
        PyCondition {
            inner: Condition::Status(self.inner.status_condition()),
        }
    }

    /// Calls `callback(event, status)` for the statuses in `mask`, on the
    /// participant's thread: `event` is the name of the status
    /// (`"publication_matched"`...), `status` its dict.
    fn set_listener(&self, callback: Py<PyAny>, mask: u32) {
        self.inner
            .set_listener(WriterCallback(callback), StatusMask(mask));
    }

    fn clear_listener(&self) {
        self.inner.clear_listener();
    }
}

/// A [`DataWriterListener`] that calls a Python function.
struct WriterCallback(Py<PyAny>);

impl WriterCallback {
    fn call(&self, event: &str, status: &dyn Status) {
        Python::attach(|py| {
            let r = status
                .to_dict(py)
                .and_then(|s| self.0.call1(py, (event, s)));
            if let Err(e) = r {
                e.print(py);
            }
        });
    }
}

impl DataWriterListener<DynamicData> for WriterCallback {
    fn on_publication_matched(&self, _: &DataWriter<DynamicData>, s: MatchedStatus) {
        self.call("publication_matched", &s);
    }
    fn on_liveliness_lost(&self, _: &DataWriter<DynamicData>, s: LivelinessLostStatus) {
        self.call("liveliness_lost", &s);
    }
    fn on_offered_deadline_missed(&self, _: &DataWriter<DynamicData>, s: DeadlineMissedStatus) {
        self.call("offered_deadline_missed", &s);
    }
    fn on_offered_incompatible_qos(&self, _: &DataWriter<DynamicData>, s: IncompatibleQosStatus) {
        self.call("offered_incompatible_qos", &s);
    }
}

/// `DataReader` of DynamicData.
#[pyclass(name = "DataReader", module = "rustorb_dds._native", frozen)]
#[derive(Debug)]
pub struct PyReader {
    inner: Arc<DataReader<DynamicData>>,
    ty: PyDynamicType,
}

#[pymethods]
impl PyReader {
    #[getter]
    fn guid(&self) -> String {
        guid(self.inner.guid())
    }

    /// Takes (or reads) the samples passing the filter, or those of one
    /// instance when `instance` is given: a list of `(data or None,
    /// info)`.
    #[pyo3(signature = (take, sample_states = None, view_states = None, instance_states = None, instance = None))]
    fn select<'py>(
        &self,
        py: Python<'py>,
        take: bool,
        sample_states: Option<Vec<String>>,
        view_states: Option<Vec<String>>,
        instance_states: Option<Vec<String>>,
        instance: Option<&str>,
    ) -> PyResult<Bound<'py, PyList>> {
        let f = filter(sample_states, view_states, instance_states)?;
        let v = match (instance.map(parse_handle).transpose()?, take) {
            (None, true) => self.inner.take_with(f),
            (None, false) => self.inner.read_with(f),
            (Some(h), true) => self.inner.take_instance(h),
            (Some(h), false) => self.inner.read_instance(h),
        };
        samples(py, &v)
    }

    /// Takes (or reads) the samples a read condition selects.
    fn select_w_condition<'py>(
        &self,
        py: Python<'py>,
        take: bool,
        condition: &PyCondition,
    ) -> PyResult<Bound<'py, PyList>> {
        let Condition::Read(c) = &condition.inner else {
            return Err(PyValueError::new_err("not a read condition"));
        };
        let v = if take {
            self.inner.take_w_condition(c)
        } else {
            self.inner.read_w_condition(c)
        }
        .map_err(dds_err)?;
        samples(py, &v)
    }

    fn lookup_instance(&self, sample: &Bound<'_, PyAny>) -> PyResult<Option<String>> {
        let d = from_py(&self.ty.ty, sample)?;
        Ok(self.inner.lookup_instance(&d).map(handle))
    }

    fn get_key_value<'py>(
        &self,
        py: Python<'py>,
        instance: &str,
    ) -> PyResult<Option<Bound<'py, PyAny>>> {
        self.inner
            .get_key_value(parse_handle(instance)?)
            .map(|d| to_py(py, &d))
            .transpose()
    }

    fn wait_for_data(&self, py: Python<'_>, timeout: f64) -> PyResult<bool> {
        let t = secs(timeout)?;
        Ok(py.detach(|| self.inner.wait_for_data(t)))
    }

    fn wait_for_publications(&self, py: Python<'_>, count: usize, timeout: f64) -> PyResult<bool> {
        let t = secs(timeout)?;
        Ok(py.detach(|| self.inner.wait_for_publications(count, t)))
    }

    fn matched_publications(&self) -> Vec<String> {
        self.inner
            .matched_publications()
            .into_iter()
            .map(guid)
            .collect()
    }

    fn subscription_matched_status<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        self.inner.subscription_matched_status().to_dict(py)
    }

    fn requested_incompatible_qos_status<'py>(
        &self,
        py: Python<'py>,
    ) -> PyResult<Bound<'py, PyDict>> {
        self.inner.requested_incompatible_qos_status().to_dict(py)
    }

    fn liveliness_changed_status<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        self.inner.liveliness_changed_status().to_dict(py)
    }

    fn requested_deadline_missed_status<'py>(
        &self,
        py: Python<'py>,
    ) -> PyResult<Bound<'py, PyDict>> {
        self.inner.requested_deadline_missed_status().to_dict(py)
    }

    fn sample_lost_status<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        self.inner.sample_lost_status().to_dict(py)
    }

    fn sample_rejected_status<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        self.inner.sample_rejected_status().to_dict(py)
    }

    fn status_changes(&self) -> u32 {
        self.inner.status_changes().0
    }

    fn status_condition(&self) -> PyCondition {
        PyCondition {
            inner: Condition::Status(self.inner.status_condition()),
        }
    }

    #[pyo3(signature = (sample_states = None, view_states = None, instance_states = None))]
    fn create_readcondition(
        &self,
        sample_states: Option<Vec<String>>,
        view_states: Option<Vec<String>>,
        instance_states: Option<Vec<String>>,
    ) -> PyResult<PyCondition> {
        let f = filter(sample_states, view_states, instance_states)?;
        Ok(PyCondition {
            inner: Condition::Read(self.inner.create_readcondition(f)),
        })
    }

    /// Calls `callback(event, status)` for the statuses in `mask`, on the
    /// participant's thread: `event` is the name of the status
    /// (`"data_available"`, `"subscription_matched"`...), `status` its
    /// dict (None for DATA_AVAILABLE).
    fn set_listener(&self, callback: Py<PyAny>, mask: u32) {
        self.inner
            .set_listener(ReaderCallback(callback), StatusMask(mask));
    }

    fn clear_listener(&self) {
        self.inner.clear_listener();
    }
}

/// A [`DataReaderListener`] that calls a Python function.
struct ReaderCallback(Py<PyAny>);

impl ReaderCallback {
    fn call(&self, event: &str, status: Option<&dyn Status>) {
        Python::attach(|py| {
            let r = match status {
                Some(s) => s.to_dict(py).map(|d| d.into_any()),
                None => Ok(py.None().into_bound(py)),
            }
            .and_then(|s| self.0.call1(py, (event, s)));
            if let Err(e) = r {
                e.print(py);
            }
        });
    }
}

impl DataReaderListener<DynamicData> for ReaderCallback {
    fn on_data_available(&self, _: &DataReader<DynamicData>) {
        self.call("data_available", None);
    }
    fn on_subscription_matched(&self, _: &DataReader<DynamicData>, s: MatchedStatus) {
        self.call("subscription_matched", Some(&s));
    }
    fn on_liveliness_changed(&self, _: &DataReader<DynamicData>, s: LivelinessChangedStatus) {
        self.call("liveliness_changed", Some(&s));
    }
    fn on_requested_deadline_missed(&self, _: &DataReader<DynamicData>, s: DeadlineMissedStatus) {
        self.call("requested_deadline_missed", Some(&s));
    }
    fn on_requested_incompatible_qos(&self, _: &DataReader<DynamicData>, s: IncompatibleQosStatus) {
        self.call("requested_incompatible_qos", Some(&s));
    }
    fn on_sample_rejected(&self, _: &DataReader<DynamicData>, s: SampleRejectedStatus) {
        self.call("sample_rejected", Some(&s));
    }
    fn on_sample_lost(&self, _: &DataReader<DynamicData>, s: SampleLostStatus) {
        self.call("sample_lost", Some(&s));
    }
}

// ---------- conditions ----------------------------------------------------

/// A condition: a guard condition, or the status condition or a read
/// condition of an entity.
#[pyclass(name = "Condition", module = "rustorb_dds._native", frozen)]
#[derive(Debug)]
pub struct PyCondition {
    inner: Condition,
}

#[pymethods]
impl PyCondition {
    /// A new guard condition.
    #[staticmethod]
    fn guard() -> Self {
        PyCondition {
            inner: Condition::Guard(GuardCondition::new()),
        }
    }

    #[getter]
    fn trigger_value(&self) -> bool {
        self.inner.trigger_value()
    }

    /// Sets the trigger value of a guard condition.
    fn set_trigger_value(&self, value: bool) -> PyResult<()> {
        match &self.inner {
            Condition::Guard(g) => {
                g.set_trigger_value(value);
                Ok(())
            }
            _ => Err(PyValueError::new_err("not a guard condition")),
        }
    }

    /// The statuses a status condition is enabled for.
    #[getter]
    fn enabled_statuses(&self) -> PyResult<u32> {
        match &self.inner {
            Condition::Status(s) => Ok(s.enabled_statuses().0),
            _ => Err(PyValueError::new_err("not a status condition")),
        }
    }

    fn set_enabled_statuses(&self, mask: u32) -> PyResult<()> {
        match &self.inner {
            Condition::Status(s) => {
                s.set_enabled_statuses(StatusMask(mask));
                Ok(())
            }
            _ => Err(PyValueError::new_err("not a status condition")),
        }
    }

    fn __eq__(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

/// `WaitSet`.
#[pyclass(name = "WaitSet", module = "rustorb_dds._native", frozen)]
#[derive(Debug)]
pub struct PyWaitSet {
    inner: WaitSet,
}

#[pymethods]
impl PyWaitSet {
    #[new]
    fn new() -> Self {
        PyWaitSet {
            inner: WaitSet::new(),
        }
    }

    fn attach_condition(&self, condition: &PyCondition) -> PyResult<()> {
        self.inner
            .attach_condition(condition.inner.clone())
            .map_err(dds_err)
    }

    fn detach_condition(&self, condition: &PyCondition) -> PyResult<()> {
        self.inner
            .detach_condition(condition.inner.clone())
            .map_err(dds_err)
    }

    /// Waits until a condition is true, or `timeout` seconds pass
    /// (`TimeoutError`). Returns the indices, in the order they were
    /// attached, of those that are true.
    fn wait(&self, py: Python<'_>, timeout: f64) -> PyResult<Vec<usize>> {
        let t = secs(timeout)?;
        let active = py.detach(|| self.inner.wait(t)).map_err(dds_err)?;
        let all = self.inner.conditions();
        Ok(all
            .iter()
            .enumerate()
            .filter(|(_, c)| active.contains(c))
            .map(|(i, _)| i)
            .collect())
    }
}
