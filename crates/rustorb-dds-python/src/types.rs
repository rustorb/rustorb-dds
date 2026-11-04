//! Types: [`PyDynamicType`] (`rustorb_dds._native.DynamicType`), made
//! from a description in plain Python values that `rustorb_dds.types`
//! writes, and described back the same way.
//!
//! A description is one of:
//!
//! - the name of a primitive type: `"boolean"`, `"byte"`, `"int8"`,
//!   `"uint8"`, `"int16"`, `"uint16"`, `"int32"`, `"uint32"`, `"int64"`,
//!   `"uint64"`, `"float32"`, `"float64"`, `"float128"`, `"char8"`,
//!   `"char16"`;
//! - `("string", bound)`, `("wstring", bound)` (0 for no bound);
//! - `("sequence", element, bound)`, `("array", element, [dimensions])`,
//!   `("map", key, element, bound)`;
//! - `("alias", name, related)`;
//! - `("enum", name, bit_bound, extensibility, [(literal, value,
//!   default)])`;
//! - `("bitmask", name, bit_bound, extensibility, [(flag, position)])`;
//! - `("struct", name, extensibility, nested, base or None, [(member, id,
//!   type, key, optional, must_understand, external)])`;
//! - `("union", name, extensibility, nested, discriminator, [(member, id,
//!   type, labels, default)])`;
//! - a `DynamicType`.
//!
//! The extensibility is `"final"`, `"appendable"` or `"mutable"`.

use std::sync::Arc;

use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyList, PyString, PyTuple};
use rustorb_dds::DynamicType;
use rustorb_xtypes::types::*;

/// A DDS-XTypes type (`DynamicType`): of a topic, or of a member.
#[pyclass(name = "DynamicType", module = "rustorb_dds._native", frozen)]
#[derive(Debug, Clone)]
pub struct PyDynamicType {
    pub(crate) ty: DynamicType,
}

#[pymethods]
impl PyDynamicType {
    /// Makes a type from its description (see the module).
    #[new]
    fn py_new(desc: &Bound<'_, PyAny>) -> PyResult<Self> {
        let ty = type_of(desc)?;
        // Checks it can be described as TypeObjects, as a topic type must.
        TypeObjects::new(&ty).map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(PyDynamicType {
            ty: DynamicType::from(ty),
        })
    }

    /// The name of the type.
    #[getter]
    fn name(&self) -> String {
        self.ty.get_name()
    }

    /// Whether the type has key members.
    #[getter]
    fn keyed(&self) -> bool {
        self.ty.is_keyed()
    }

    /// The description of the type (see the module).
    fn describe<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        describe(py, self.ty.as_type())
    }

    fn __eq__(&self, other: &Self) -> bool {
        self.ty == other.ty
    }

    fn __repr__(&self) -> String {
        format!("DynamicType({:?})", self.ty.get_name())
    }
}

fn bad(what: impl Into<String>) -> PyErr {
    PyValueError::new_err(what.into())
}

fn extensibility(s: &str) -> PyResult<Extensibility> {
    Ok(match s {
        "final" => Extensibility::Final,
        "appendable" => Extensibility::Appendable,
        "mutable" => Extensibility::Mutable,
        _ => return Err(bad(format!("unknown extensibility {s:?}"))),
    })
}

fn extensibility_name(e: Extensibility) -> &'static str {
    match e {
        Extensibility::Final => "final",
        Extensibility::Appendable => "appendable",
        Extensibility::Mutable => "mutable",
    }
}

const PRIMITIVES: [(&str, Primitive); 15] = [
    ("boolean", Primitive::Boolean),
    ("byte", Primitive::Byte),
    ("int8", Primitive::Int8),
    ("uint8", Primitive::Uint8),
    ("int16", Primitive::Int16),
    ("uint16", Primitive::Uint16),
    ("int32", Primitive::Int32),
    ("uint32", Primitive::Uint32),
    ("int64", Primitive::Int64),
    ("uint64", Primitive::Uint64),
    ("float32", Primitive::Float32),
    ("float64", Primitive::Float64),
    ("float128", Primitive::Float128),
    ("char8", Primitive::Char8),
    ("char16", Primitive::Char16),
];

fn item<'py>(t: &Bound<'py, PyTuple>, i: usize) -> PyResult<Bound<'py, PyAny>> {
    t.get_item(i)
}

/// The type a description describes.
pub(crate) fn type_of(desc: &Bound<'_, PyAny>) -> PyResult<Type> {
    if let Ok(t) = desc.cast::<PyDynamicType>() {
        return Ok(t.get().ty.as_type().clone());
    }
    if let Ok(s) = desc.cast::<PyString>() {
        let s: String = s.extract()?;
        let s = s.as_str();
        return PRIMITIVES
            .iter()
            .find(|(n, _)| *n == s)
            .map(|(_, p)| Type::Primitive(*p))
            .ok_or_else(|| bad(format!("unknown primitive type {s:?}")));
    }
    let t = desc.cast::<PyTuple>().map_err(|_| {
        PyTypeError::new_err("a type description is a str, a tuple or a DynamicType")
    })?;
    let kind: String = item(t, 0)?.extract()?;
    Ok(match kind.as_str() {
        "string" | "wstring" => Type::String {
            wide: kind == "wstring",
            bound: item(t, 1)?.extract()?,
        },
        "sequence" => Type::Sequence(Arc::new(SequenceType {
            element: Element::new(type_of(&item(t, 1)?)?),
            bound: item(t, 2)?.extract()?,
        })),
        "array" => Type::Array(Arc::new(ArrayType {
            element: Element::new(type_of(&item(t, 1)?)?),
            dimensions: item(t, 2)?.extract()?,
        })),
        "map" => Type::Map(Arc::new(MapType {
            key: Element::new(type_of(&item(t, 1)?)?),
            element: Element::new(type_of(&item(t, 2)?)?),
            bound: item(t, 3)?.extract()?,
        })),
        "alias" => Type::from(AliasType {
            name: item(t, 1)?.extract()?,
            related: type_of(&item(t, 2)?)?,
        }),
        "enum" => {
            let literals: Vec<(String, i32, bool)> = item(t, 4)?.extract()?;
            Type::from(EnumType {
                name: item(t, 1)?.extract()?,
                bit_bound: item(t, 2)?.extract()?,
                extensibility: extensibility(&item(t, 3)?.extract::<String>()?)?,
                literals: literals
                    .into_iter()
                    .map(|(name, value, default)| EnumLiteral {
                        name,
                        value,
                        default,
                    })
                    .collect(),
            })
        }
        "bitmask" => {
            let flags: Vec<(String, u16)> = item(t, 4)?.extract()?;
            Type::from(BitmaskType {
                name: item(t, 1)?.extract()?,
                bit_bound: item(t, 2)?.extract()?,
                extensibility: extensibility(&item(t, 3)?.extract::<String>()?)?,
                flags: flags
                    .into_iter()
                    .map(|(name, position)| Bitflag { name, position })
                    .collect(),
            })
        }
        "struct" => {
            let base = item(t, 4)?;
            let mut members = Vec::new();
            for m in item(t, 5)?.try_iter()? {
                let m = m?;
                let m = m.cast::<PyTuple>()?;
                members.push(StructMember {
                    key: item(m, 3)?.extract()?,
                    optional: item(m, 4)?.extract()?,
                    must_understand: item(m, 5)?.extract()?,
                    external: item(m, 6)?.extract()?,
                    ..StructMember::new(
                        item(m, 0)?.extract::<String>()?,
                        item(m, 1)?.extract()?,
                        type_of(&item(m, 2)?)?,
                    )
                });
            }
            Type::from(StructType {
                name: item(t, 1)?.extract()?,
                extensibility: extensibility(&item(t, 2)?.extract::<String>()?)?,
                nested: item(t, 3)?.extract()?,
                autoid_hash: false,
                base: if base.is_none() {
                    None
                } else {
                    Some(type_of(&base)?)
                },
                members,
            })
        }
        "union" => {
            let mut members = Vec::new();
            for m in item(t, 5)?.try_iter()? {
                let m = m?;
                let m = m.cast::<PyTuple>()?;
                members.push(UnionMember {
                    default: item(m, 4)?.extract()?,
                    ..UnionMember::new(
                        item(m, 0)?.extract::<String>()?,
                        item(m, 1)?.extract()?,
                        type_of(&item(m, 2)?)?,
                        item(m, 3)?.extract()?,
                    )
                });
            }
            Type::from(UnionType {
                name: item(t, 1)?.extract()?,
                extensibility: extensibility(&item(t, 2)?.extract::<String>()?)?,
                nested: item(t, 3)?.extract()?,
                autoid_hash: false,
                discriminator: type_of(&item(t, 4)?)?,
                discriminator_key: false,
                discriminator_try_construct: TryConstruct::Discard,
                members,
            })
        }
        _ => return Err(bad(format!("unknown type kind {kind:?}"))),
    })
}

/// The description of `ty`.
pub(crate) fn describe<'py>(py: Python<'py>, ty: &Type) -> PyResult<Bound<'py, PyAny>> {
    let tuple = |items: Vec<Bound<'py, PyAny>>| -> PyResult<Bound<'py, PyAny>> {
        Ok(PyTuple::new(py, items)?.into_any())
    };
    let obj = |v: &dyn ToPy| v.to_py(py);
    match ty {
        Type::Primitive(p) => {
            let name = PRIMITIVES
                .iter()
                .find(|(_, q)| q == p)
                .map(|(n, _)| *n)
                .unwrap_or("int32");
            Ok(PyString::new(py, name).into_any())
        }
        Type::String { wide, bound } => tuple(vec![
            obj(&if *wide { "wstring" } else { "string" })?,
            obj(bound)?,
        ]),
        Type::Sequence(s) => tuple(vec![
            obj(&"sequence")?,
            describe(py, &s.element.ty)?,
            obj(&s.bound)?,
        ]),
        Type::Array(a) => tuple(vec![
            obj(&"array")?,
            describe(py, &a.element.ty)?,
            PyList::new(py, &a.dimensions)?.into_any(),
        ]),
        Type::Map(m) => tuple(vec![
            obj(&"map")?,
            describe(py, &m.key.ty)?,
            describe(py, &m.element.ty)?,
            obj(&m.bound)?,
        ]),
        Type::Alias(a) => tuple(vec![
            obj(&"alias")?,
            obj(&a.name.as_str())?,
            describe(py, &a.related)?,
        ]),
        Type::Enum(e) => {
            let literals = e
                .literals
                .iter()
                .map(|l| {
                    (l.name.as_str(), l.value, l.default)
                        .into_pyobject(py)
                        .map(|t| t.into_any())
                })
                .collect::<PyResult<Vec<_>>>()?;
            tuple(vec![
                obj(&"enum")?,
                obj(&e.name.as_str())?,
                obj(&e.bit_bound)?,
                obj(&extensibility_name(e.extensibility))?,
                PyList::new(py, literals)?.into_any(),
            ])
        }
        Type::Bitmask(b) => {
            let flags = b
                .flags
                .iter()
                .map(|f| {
                    (f.name.as_str(), f.position)
                        .into_pyobject(py)
                        .map(|t| t.into_any())
                })
                .collect::<PyResult<Vec<_>>>()?;
            tuple(vec![
                obj(&"bitmask")?,
                obj(&b.name.as_str())?,
                obj(&b.bit_bound)?,
                obj(&extensibility_name(b.extensibility))?,
                PyList::new(py, flags)?.into_any(),
            ])
        }
        Type::Struct(s) => {
            let members = s
                .members
                .iter()
                .map(|m| {
                    tuple(vec![
                        obj(&m.name.as_str())?,
                        obj(&m.id)?,
                        describe(py, &m.ty)?,
                        obj(&m.key)?,
                        obj(&m.optional)?,
                        obj(&m.must_understand)?,
                        obj(&m.external)?,
                    ])
                })
                .collect::<PyResult<Vec<_>>>()?;
            tuple(vec![
                obj(&"struct")?,
                obj(&s.name.as_str())?,
                obj(&extensibility_name(s.extensibility))?,
                obj(&s.nested)?,
                match &s.base {
                    Some(b) => describe(py, b)?,
                    None => py.None().into_bound(py),
                },
                PyList::new(py, members)?.into_any(),
            ])
        }
        Type::Union(u) => {
            let members = u
                .members
                .iter()
                .map(|m| {
                    tuple(vec![
                        obj(&m.name.as_str())?,
                        obj(&m.id)?,
                        describe(py, &m.ty)?,
                        PyList::new(py, &m.labels)?.into_any(),
                        obj(&m.default)?,
                    ])
                })
                .collect::<PyResult<Vec<_>>>()?;
            tuple(vec![
                obj(&"union")?,
                obj(&u.name.as_str())?,
                obj(&extensibility_name(u.extensibility))?,
                obj(&u.nested)?,
                describe(py, &u.discriminator)?,
                PyList::new(py, members)?.into_any(),
            ])
        }
    }
}

/// A value that converts to a Python object, for [`describe`].
trait ToPy {
    fn to_py<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>>;
}

macro_rules! to_py {
    ($($t:ty),*) => {
        $(impl ToPy for $t {
            fn to_py<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
                Ok((*self).into_pyobject(py)?.to_owned().into_any())
            }
        })*
    };
}

to_py!(&str, bool, u16, u32);
