//! The TypeObjects a participant knows, by TypeIdentifier.

use std::collections::{HashMap, HashSet};

use crate::Result;
use crate::type_object::{
    MinimalTypeObject, TypeIdentifier, TypeIdentifierTypeObjectPair, TypeIdentifierWithSize,
    TypeInformation, TypeObject,
};
use crate::types::TypeObjects;

/// TypeObjects by their TypeIdentifier: those of the local types, and
/// those learnt from other participants (TypeLookup, 7.6.3.3).
#[derive(Debug, Clone, Default)]
pub struct TypeRegistry {
    objects: HashMap<TypeIdentifier, TypeObject>,
}

impl TypeRegistry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds the TypeObjects of a type and of the types it depends on.
    pub fn add_type(&mut self, objects: &TypeObjects) {
        for p in objects.minimal.iter().chain(&objects.complete) {
            self.objects
                .insert(p.type_identifier.clone(), p.type_object.clone());
        }
    }

    /// Adds TypeObjects after checking that each hashes to the
    /// TypeIdentifier it comes with; the others are left out. Returns how
    /// many were added.
    pub fn add_pairs(&mut self, pairs: &[TypeIdentifierTypeObjectPair]) -> Result<usize> {
        let mut added = 0;
        for p in pairs {
            if p.type_object.type_identifier()? == p.type_identifier {
                self.objects
                    .insert(p.type_identifier.clone(), p.type_object.clone());
                added += 1;
            }
        }
        Ok(added)
    }

    /// The TypeObject `id` identifies.
    pub fn get(&self, id: &TypeIdentifier) -> Option<&TypeObject> {
        self.objects.get(id)
    }

    /// The minimal TypeObject `id` identifies.
    pub fn minimal(&self, id: &TypeIdentifier) -> Option<&MinimalTypeObject> {
        match self.objects.get(id) {
            Some(TypeObject::Minimal(m)) => Some(m),
            _ => None,
        }
    }

    /// Whether the TypeObject of `id` is here. Identifiers that are not
    /// hashes have no TypeObject and count as known.
    pub fn knows(&self, id: &TypeIdentifier) -> bool {
        !id.is_direct_hash() || self.objects.contains_key(id)
    }

    /// The minimal TypeIdentifiers of `info` (the type and those it
    /// depends on) whose TypeObjects are not here.
    pub fn missing(&self, info: &TypeInformation) -> Vec<TypeIdentifier> {
        std::iter::once(&info.minimal.typeid_with_size)
            .chain(&info.minimal.dependent_typeids)
            .map(|t| &t.type_id)
            .filter(|id| !self.knows(id))
            .cloned()
            .collect()
    }

    /// The direct hash TypeIdentifiers that the type `id` depends on,
    /// directly or not, in the order they are first used; `None` when the
    /// TypeObject of `id` is not here. Types whose TypeObjects are not here
    /// are listed, but not what they depend on.
    pub fn dependencies(&self, id: &TypeIdentifier) -> Option<Vec<TypeIdentifier>> {
        let root = self.objects.get(id)?;
        let mut seen = HashSet::from([id]);
        let mut out = Vec::new();
        let mut stack: Vec<&TypeIdentifier> = root.referenced_types().into_iter().rev().collect();
        while let Some(t) = stack.pop() {
            if !seen.insert(t) {
                continue;
            }
            out.push(t.clone());
            if let Some(object) = self.objects.get(t) {
                stack.extend(object.referenced_types().into_iter().rev());
            }
        }
        Some(out)
    }

    /// The [`TypeIdentifierWithSize`] of `id`, when its TypeObject is
    /// here.
    pub fn with_size(&self, id: &TypeIdentifier) -> Option<TypeIdentifierWithSize> {
        let size = self.objects.get(id)?.to_bytes().ok()?.len();
        Some(TypeIdentifierWithSize {
            type_id: id.clone(),
            typeobject_serialized_size: u32::try_from(size).ok()?,
        })
    }

    #[cfg(test)]
    pub(crate) fn objects_mut(&mut self) -> &mut HashMap<TypeIdentifier, TypeObject> {
        &mut self.objects
    }

    /// The number of TypeObjects.
    pub fn len(&self) -> usize {
        self.objects.len()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;

    fn st(name: &str, members: Vec<StructMember>) -> Type {
        StructType {
            name: name.into(),
            extensibility: Extensibility::Appendable,
            members,
            ..Default::default()
        }
        .into()
    }

    #[test]
    fn dependencies_are_those_of_the_type_information() {
        let inner = st(
            "M::Inner",
            vec![StructMember::new("x", 0, Type::Primitive(Primitive::Int32))],
        );
        let middle = st(
            "M::Middle",
            vec![StructMember::new("i", 0, Type::sequence(inner.clone()))],
        );
        let outer = st(
            "M::Outer",
            vec![
                StructMember::new("m", 0, middle),
                StructMember::new("i", 1, inner),
            ],
        );
        let objects = TypeObjects::new(&outer).unwrap();
        let mut registry = TypeRegistry::new();
        registry.add_type(&objects);
        for info in [&objects.information.minimal, &objects.information.complete] {
            let root = &info.typeid_with_size.type_id;
            let expected: Vec<_> = info
                .dependent_typeids
                .iter()
                .map(|t| t.type_id.clone())
                .collect();
            assert_eq!(expected.len(), 2);
            assert_eq!(registry.dependencies(root).unwrap(), expected);
            assert_eq!(
                registry.with_size(root).as_ref(),
                Some(&info.typeid_with_size)
            );
            // The innermost type depends on nothing.
            assert_eq!(registry.dependencies(&expected[1]).unwrap(), vec![]);
        }
        assert_eq!(
            registry.dependencies(&TypeIdentifier::Minimal([0; 14])),
            None
        );
    }
}
