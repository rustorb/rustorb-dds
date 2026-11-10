//! `rustorb_dds._native`: the native part of the rustorb-dds Python
//! package, built with PyO3.
//!
//! Samples are [`DynamicData`](rustorb_dds::DynamicData) of a
//! [`DynamicType`](rustorb_dds::DynamicType): the Python package
//! describes the types of its classes in plain Python values (`types.rs`)
//! and gets samples as plain Python values (`value.rs`). The DCPS entities
//! are in `entity.rs`.

mod entity;
mod types;
mod value;

use pyo3::prelude::*;

#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<types::PyDynamicType>()?;
    m.add_class::<entity::PyParticipant>()?;
    m.add_class::<entity::PyTopic>()?;
    m.add_class::<entity::PyContentFilteredTopic>()?;
    m.add_class::<entity::PyPublisher>()?;
    m.add_class::<entity::PySubscriber>()?;
    m.add_class::<entity::PyWriter>()?;
    m.add_class::<entity::PyReader>()?;
    m.add_class::<entity::PyCondition>()?;
    m.add_class::<entity::PyWaitSet>()?;
    Ok(())
}
