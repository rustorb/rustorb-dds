//! The TypeObjects a participant knows, by TypeIdentifier.

use std::collections::{HashMap, HashSet};

use crate::Result;
use crate::type_object::{
    MinimalTypeObject, TypeIdentifier, TypeIdentifierTypeObjectPair, TypeIdentifierWithSize,
    TypeInformation, TypeObject, TypeObjectHashId, component_hash_matches,
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
    /// TypeIdentifier it comes with; the others are left out. The types of
    /// a strongly connected component are checked together, and added
    /// only when all of them are there (7.3.4.9.2). Returns how many were
    /// added.
    pub fn add_pairs(&mut self, pairs: &[TypeIdentifierTypeObjectPair]) -> Result<usize> {
        let mut added = 0;
        let mut components: HashMap<TypeIdentifier, Vec<&TypeIdentifierTypeObjectPair>> =
            HashMap::new();
        for p in pairs {
            if let Some(scc) = p.type_identifier.scc_identifier() {
                components.entry(scc).or_default().push(p);
            } else if p.type_object.type_identifier()? == p.type_identifier {
                self.objects
                    .insert(p.type_identifier.clone(), p.type_object.clone());
                added += 1;
            }
        }
        for (scc, mut members) in components {
            let TypeIdentifier::StronglyConnectedComponent(c) = &scc else {
                continue;
            };
            members.sort_by_key(|p| p.type_identifier.clone());
            members.dedup_by_key(|p| p.type_identifier.clone());
            let expected = scc.scc_members();
            let ids: Vec<&TypeIdentifier> = members.iter().map(|p| &p.type_identifier).collect();
            if expected.is_empty() || ids != expected.iter().collect::<Vec<_>>() {
                continue;
            }
            let (hash, complete) = match c.sc_component_id {
                TypeObjectHashId::Complete(h) => (h, true),
                TypeObjectHashId::Minimal(h) => (h, false),
            };
            let objects: Vec<TypeObject> = members.iter().map(|p| p.type_object.clone()).collect();
            let kinds_match = objects
                .iter()
                .all(|o| matches!(o, TypeObject::Complete(_)) == complete);
            if kinds_match && component_hash_matches(&objects, &hash)? {
                for p in members {
                    self.objects
                        .insert(p.type_identifier.clone(), p.type_object.clone());
                    added += 1;
                }
            }
        }
        Ok(added)
    }

    /// The TypeObjects of the types of the strongly connected component
    /// `scc` identifies (or one of its types), in the order of their
    /// `scc_index`, when all are here.
    pub fn component(&self, scc: &TypeIdentifier) -> Option<Vec<TypeIdentifierTypeObjectPair>> {
        let members = scc.scc_members();
        if members.is_empty() {
            return None;
        }
        members
            .into_iter()
            .map(|id| {
                let type_object = self.objects.get(&id)?.clone();
                Some(TypeIdentifierTypeObjectPair {
                    type_identifier: id,
                    type_object,
                })
            })
            .collect()
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

    /// Whether the TypeObject of `id` is here; for the identifier of a
    /// strongly connected component, those of all its types. Identifiers
    /// that are not hashes have no TypeObject and count as known.
    pub fn knows(&self, id: &TypeIdentifier) -> bool {
        if id.is_scc_identifier() {
            return id
                .scc_members()
                .iter()
                .all(|m| self.objects.contains_key(m));
        }
        !id.is_direct_hash() || self.objects.contains_key(id)
    }

    /// The minimal TypeIdentifiers of `info` (the type and those it
    /// depends on) whose TypeObjects are not here, as they are asked for
    /// with TypeLookup: a type of a strongly connected component by the
    /// identifier of the component (7.6.3.3.4.2).
    pub fn missing(&self, info: &TypeInformation) -> Vec<TypeIdentifier> {
        let mut out: Vec<TypeIdentifier> = Vec::new();
        for id in std::iter::once(&info.minimal.typeid_with_size)
            .chain(&info.minimal.dependent_typeids)
            .map(|t| &t.type_id)
            .filter(|id| !self.knows(id))
        {
            let id = id.scc_identifier().unwrap_or_else(|| id.clone());
            if !out.contains(&id) {
                out.push(id);
            }
        }
        out
    }

    /// The direct hash TypeIdentifiers that the type `id` (or the types
    /// of the strongly connected component `id` identifies) depends on,
    /// directly or not, in the order they are first used; `None` when the
    /// TypeObject of `id` is not here. Types whose TypeObjects are not here
    /// are listed, but not what they depend on. The types of a strongly
    /// connected component are listed as the identifier of the component
    /// (7.6.3.3.4.1), unless it is that of `id` itself.
    pub fn dependencies(&self, id: &TypeIdentifier) -> Option<Vec<TypeIdentifier>> {
        let roots: Vec<TypeIdentifier> = if id.is_scc_identifier() {
            self.component(id)?
                .into_iter()
                .map(|p| p.type_identifier)
                .collect()
        } else {
            self.objects.get(id)?;
            vec![id.clone()]
        };
        let own = id.scc_identifier();
        let mut seen: HashSet<&TypeIdentifier> = roots.iter().collect();
        let mut out: Vec<TypeIdentifier> = Vec::new();
        let mut stack: Vec<&TypeIdentifier> = roots
            .iter()
            .rev()
            .flat_map(|r| self.objects[r].referenced_types().into_iter().rev())
            .collect();
        while let Some(t) = stack.pop() {
            if !seen.insert(t) {
                continue;
            }
            let listed = t.scc_identifier().unwrap_or_else(|| t.clone());
            if Some(&listed) != own.as_ref() && !out.contains(&listed) {
                out.push(listed);
            }
            if let Some(object) = self.objects.get(t) {
                stack.extend(object.referenced_types().into_iter().rev());
            }
        }
        Some(out)
    }

    /// The [`TypeIdentifierWithSize`] of `id`, when its TypeObject is
    /// here; for the identifier of a strongly connected component, with
    /// the size of the TypeObjects of its types in sequence.
    pub fn with_size(&self, id: &TypeIdentifier) -> Option<TypeIdentifierWithSize> {
        let size = if id.is_scc_identifier() {
            let objects: Vec<TypeObject> = self
                .component(id)?
                .into_iter()
                .map(|p| p.type_object)
                .collect();
            crate::xcdr2::to_bytes(&objects, rustorb_cdr::ByteOrder::Little)
                .ok()?
                .len()
        } else {
            self.objects.get(id)?.to_bytes().ok()?.len()
        };
        Some(TypeIdentifierWithSize {
            type_id: id.clone(),
            typeobject_serialized_size: u32::try_from(size).ok()?,
        })
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
