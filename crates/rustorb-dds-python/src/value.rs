//! Samples: [`DynamicData`] to and from plain Python values.
//!
//! - integers, floats (`float128` as a float), booleans;
//! - `char8` and `char16` as a str of one character, strings as str;
//! - enums and bitmasks as int (the value of the literal, the bits);
//! - sequences and arrays as list, or bytes for elements of `byte` or
//!   `uint8`; arrays of several dimensions as nested lists;
//! - maps as dict;
//! - structs as dict by member name, an absent optional member as None
//!   (an object with the members as attributes is taken too);
//! - unions as a tuple `(discriminator, value)`, `value` None when the
//!   discriminator selects no member.

use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyList, PyString, PyTuple};
use rustorb_dds::{DynamicData, DynamicDataFactory, DynamicType, MEMBER_ID_INVALID};
use rustorb_xtypes::type_object::*;
use rustorb_xtypes::types::Type;

fn err(e: rustorb_xtypes::Error) -> PyErr {
    PyValueError::new_err(e.to_string())
}

const ME: MemberId = MEMBER_ID_INVALID;

/// Whether the elements of a collection of `ty` are octets, which are
/// bytes in Python.
fn octets(ty: &Type) -> bool {
    use rustorb_xtypes::types::Primitive;
    matches!(
        resolve(ty),
        Type::Primitive(Primitive::Byte | Primitive::Uint8)
    )
}

fn resolve(mut ty: &Type) -> &Type {
    while let Type::Alias(a) = ty {
        ty = &a.related;
    }
    ty
}

/// The Python value of `d`.
pub(crate) fn to_py<'py>(py: Python<'py>, d: &DynamicData) -> PyResult<Bound<'py, PyAny>> {
    let ty = d.get_type();
    Ok(match d.get_kind() {
        TK_BOOLEAN => d
            .get_boolean_value(ME)
            .map_err(err)?
            .into_pyobject(py)?
            .to_owned()
            .into_any(),
        TK_BYTE => d
            .get_byte_value(ME)
            .map_err(err)?
            .into_pyobject(py)?
            .into_any(),
        TK_UINT8 => d
            .get_uint8_value(ME)
            .map_err(err)?
            .into_pyobject(py)?
            .into_any(),
        TK_INT8 => d
            .get_int8_value(ME)
            .map_err(err)?
            .into_pyobject(py)?
            .into_any(),
        TK_INT16 => d
            .get_int16_value(ME)
            .map_err(err)?
            .into_pyobject(py)?
            .into_any(),
        TK_UINT16 => d
            .get_uint16_value(ME)
            .map_err(err)?
            .into_pyobject(py)?
            .into_any(),
        TK_INT32 | TK_ENUM => d
            .get_int32_value(ME)
            .map_err(err)?
            .into_pyobject(py)?
            .into_any(),
        TK_UINT32 => d
            .get_uint32_value(ME)
            .map_err(err)?
            .into_pyobject(py)?
            .into_any(),
        TK_INT64 => d
            .get_int64_value(ME)
            .map_err(err)?
            .into_pyobject(py)?
            .into_any(),
        TK_UINT64 | TK_BITMASK => d
            .get_uint64_value(ME)
            .map_err(err)?
            .into_pyobject(py)?
            .into_any(),
        TK_FLOAT32 => d
            .get_float32_value(ME)
            .map_err(err)?
            .into_pyobject(py)?
            .into_any(),
        TK_FLOAT64 => d
            .get_float64_value(ME)
            .map_err(err)?
            .into_pyobject(py)?
            .into_any(),
        TK_FLOAT128 => d
            .get_float128_value(ME)
            .map_err(err)?
            .to_f64()
            .into_pyobject(py)?
            .into_any(),
        TK_CHAR8 => d
            .get_char8_value(ME)
            .map_err(err)?
            .into_pyobject(py)?
            .into_any(),
        TK_CHAR16 => d
            .get_char16_value(ME)
            .map_err(err)?
            .into_pyobject(py)?
            .into_any(),
        TK_STRING8 => PyString::new(py, &d.get_string_value(ME).map_err(err)?).into_any(),
        TK_STRING16 => PyString::new(py, &d.get_wstring_value(ME).map_err(err)?).into_any(),
        TK_SEQUENCE | TK_ARRAY => {
            let (element, dims) = match ty.as_type() {
                Type::Sequence(s) => (&s.element.ty, vec![]),
                Type::Array(a) => (&a.element.ty, a.dimensions.clone()),
                _ => unreachable!("a collection"),
            };
            let n = d.get_item_count();
            if octets(element) {
                let byte = matches!(
                    resolve(element),
                    Type::Primitive(rustorb_xtypes::types::Primitive::Byte)
                );
                let bytes = (0..n)
                    .map(|i| {
                        if byte {
                            d.get_byte_value(i)
                        } else {
                            d.get_uint8_value(i)
                        }
                    })
                    .collect::<Result<Vec<u8>, _>>()
                    .map_err(err)?;
                PyBytes::new(py, &bytes).into_any()
            } else {
                let items = (0..n)
                    .map(|i| to_py(py, d.get_value(i).map_err(err)?))
                    .collect::<PyResult<Vec<_>>>()?;
                nest(py, items, &dims)?
            }
        }
        TK_MAP => {
            let dict = PyDict::new(py);
            for (k, v) in d.map_entries().map_err(err)? {
                dict.set_item(to_py(py, k)?, to_py(py, v)?)?;
            }
            dict.into_any()
        }
        TK_STRUCTURE => {
            let dict = PyDict::new(py);
            for m in ty.members() {
                let v = match d.get_value(m.get_id()) {
                    Ok(v) => to_py(py, v)?,
                    Err(_) => py.None().into_bound(py),
                };
                dict.set_item(m.get_name(), v)?;
            }
            dict.into_any()
        }
        TK_UNION => {
            let disc = d.get_discriminator_value().map_err(err)?;
            let value = match d.selected_member() {
                Some(id) => to_py(py, d.get_value(id).map_err(err)?)?,
                None => py.None().into_bound(py),
            };
            PyTuple::new(py, [disc.into_pyobject(py)?.into_any(), value])?.into_any()
        }
        k => {
            return Err(PyTypeError::new_err(format!(
                "type kind {k:#x} not supported"
            )));
        }
    })
}

/// `items` as nested lists of the array dimensions `dims`.
fn nest<'py>(
    py: Python<'py>,
    items: Vec<Bound<'py, PyAny>>,
    dims: &[u32],
) -> PyResult<Bound<'py, PyAny>> {
    if dims.len() <= 1 {
        return Ok(PyList::new(py, items)?.into_any());
    }
    let inner: usize = dims[1..].iter().map(|d| *d as usize).product();
    let mut rows = Vec::new();
    let mut items = items.into_iter();
    for _ in 0..dims[0] {
        let row: Vec<_> = items.by_ref().take(inner).collect();
        rows.push(nest(py, row, &dims[1..])?);
    }
    Ok(PyList::new(py, rows)?.into_any())
}

/// A sample of `ty` from the Python value `v`.
pub(crate) fn from_py(ty: &DynamicType, v: &Bound<'_, PyAny>) -> PyResult<DynamicData> {
    let mut d = DynamicDataFactory::create_data(ty);
    fill(&mut d, v)?;
    Ok(d)
}

fn int<T: for<'a, 'py> FromPyObject<'a, 'py>>(v: &Bound<'_, PyAny>) -> PyResult<T> {
    v.extract::<T>().map_err(Into::into)
}

fn char_of(v: &Bound<'_, PyAny>) -> PyResult<char> {
    let s: String = v.extract()?;
    let mut chars = s.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Ok(c),
        _ => Err(PyValueError::new_err("a char is a str of one character")),
    }
}

/// Sets `d` (of its type, already made) to the Python value `v`.
fn fill(d: &mut DynamicData, v: &Bound<'_, PyAny>) -> PyResult<()> {
    let ty = d.get_type();
    match d.get_kind() {
        TK_BOOLEAN => d.set_boolean_value(ME, v.is_truthy()?),
        TK_BYTE => d.set_byte_value(ME, int(v)?),
        TK_UINT8 => d.set_uint8_value(ME, int(v)?),
        TK_INT8 => d.set_int8_value(ME, int(v)?),
        TK_INT16 => d.set_int16_value(ME, int(v)?),
        TK_UINT16 => d.set_uint16_value(ME, int(v)?),
        TK_INT32 => d.set_int32_value(ME, int(v)?),
        TK_UINT32 => d.set_uint32_value(ME, int(v)?),
        TK_INT64 => d.set_int64_value(ME, int(v)?),
        TK_UINT64 => d.set_uint64_value(ME, int(v)?),
        TK_FLOAT32 => d.set_float32_value(ME, int(v)?),
        TK_FLOAT64 => d.set_float64_value(ME, int(v)?),
        TK_FLOAT128 => d.set_float128_value(ME, int::<f64>(v)?.into()),
        TK_CHAR8 => d.set_char8_value(ME, char_of(v)?),
        TK_CHAR16 => d.set_char16_value(ME, char_of(v)?),
        TK_STRING8 => d.set_string_value(ME, &v.extract::<String>()?),
        TK_STRING16 => d.set_wstring_value(ME, &v.extract::<String>()?),
        TK_ENUM => {
            // Set with the narrowest integer that holds it, which any
            // bit bound takes.
            let x: i32 = int(v)?;
            if let Ok(x) = i8::try_from(x) {
                d.set_int8_value(ME, x)
            } else if let Ok(x) = i16::try_from(x) {
                d.set_int16_value(ME, x)
            } else {
                d.set_int32_value(ME, x)
            }
        }
        TK_BITMASK => {
            let x: u64 = int(v)?;
            if let Ok(x) = u8::try_from(x) {
                d.set_uint8_value(ME, x)
            } else if let Ok(x) = u16::try_from(x) {
                d.set_uint16_value(ME, x)
            } else if let Ok(x) = u32::try_from(x) {
                d.set_uint32_value(ME, x)
            } else {
                d.set_uint64_value(ME, x)
            }
        }
        TK_SEQUENCE | TK_ARRAY => {
            let mut items = Vec::new();
            // The dimensions of an array after the first are nested lists.
            let depth = match ty.as_type() {
                Type::Array(a) => a.dimensions.len().saturating_sub(1),
                _ => 0,
            };
            flatten(v, depth, &mut items)?;
            let array = d.get_kind() == TK_ARRAY;
            if array && items.len() != d.get_item_count() as usize {
                return Err(PyValueError::new_err(format!(
                    "an array of {} elements, not {}",
                    d.get_item_count(),
                    items.len()
                )));
            }
            if !array {
                d.clear_all_values().map_err(err)?;
            }
            for (i, item) in items.iter().enumerate() {
                fill(d.loan_value(i as MemberId).map_err(err)?, item)?;
            }
            Ok(())
        }
        TK_MAP => {
            let Type::Map(m) = ty.as_type() else {
                unreachable!("a map")
            };
            let (kt, et) = (
                DynamicType::from(m.key.ty.clone()),
                DynamicType::from(m.element.ty.clone()),
            );
            let dict = v.cast::<PyDict>()?;
            for (k, e) in dict.iter() {
                d.map_insert(from_py(&kt, &k)?, from_py(&et, &e)?)
                    .map_err(err)?;
            }
            Ok(())
        }
        TK_STRUCTURE => {
            let dict = v.cast::<PyDict>().ok();
            for m in ty.members() {
                let item = match dict {
                    Some(dict) => dict.get_item(m.get_name())?,
                    None => v.getattr(m.get_name()).ok(),
                };
                match item {
                    Some(item) if !item.is_none() => {
                        fill(d.loan_value(m.get_id()).map_err(err)?, &item)?
                    }
                    _ if m.get_descriptor().is_optional => {}
                    _ => {
                        return Err(PyValueError::new_err(format!(
                            "member {:?} of {} is missing",
                            m.get_name(),
                            ty.get_name()
                        )));
                    }
                }
            }
            Ok(())
        }
        TK_UNION => {
            let t = v
                .cast::<PyTuple>()
                .map_err(|_| PyTypeError::new_err("a union is (discriminator, value)"))?;
            let disc: i64 = t.get_item(0)?.extract()?;
            d.set_discriminator_value(disc).map_err(err)?;
            if let Some(id) = d.selected_member() {
                let value = t.get_item(1)?;
                fill(d.loan_value(id).map_err(err)?, &value)?;
            }
            Ok(())
        }
        k => {
            return Err(PyTypeError::new_err(format!(
                "type kind {k:#x} not supported"
            )));
        }
    }
    .map_err(err)
}

/// The elements of a list, or of `depth` levels of nested lists for an
/// array of several dimensions; the octets of bytes.
fn flatten<'py>(
    v: &Bound<'py, PyAny>,
    depth: usize,
    out: &mut Vec<Bound<'py, PyAny>>,
) -> PyResult<()> {
    if let Ok(b) = v.cast::<PyBytes>() {
        let py = v.py();
        for x in b.as_bytes() {
            out.push(x.into_pyobject(py)?.into_any());
        }
        return Ok(());
    }
    if v.is_instance_of::<PyString>() {
        return Err(PyTypeError::new_err("a str is not a list"));
    }
    for item in v.try_iter()? {
        let item = item?;
        if depth > 0 {
            flatten(&item, depth - 1, out)?;
        } else {
            out.push(item);
        }
    }
    Ok(())
}
