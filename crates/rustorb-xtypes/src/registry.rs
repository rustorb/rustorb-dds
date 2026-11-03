//! The TypeObjects a participant knows, by TypeIdentifier.

use std::collections::HashMap;

use crate::Result;
use crate::type_object::{
    MinimalTypeObject, TypeIdentifier, TypeIdentifierTypeObjectPair, TypeInformation, TypeObject,
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
