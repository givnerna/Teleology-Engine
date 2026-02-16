//! Generic tag system for abstract mechanics (religion, culture, ideology, etc.).
//!
//! Goals:
//! - Data-driven: games can register their own tag types and tag values.
//! - Flexible: tags can be attached to provinces and/or nations.
//! - Serializable: stored in MapFile so maps/scenarios can be shared.

use bevy_ecs::prelude::Resource;
use crate::world::{NationId, ProvinceId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::num::NonZeroU32;

/// Stable id for a tag type/category (e.g. religion, culture, ideology, custom).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TagTypeId(pub NonZeroU32);

impl TagTypeId {
    #[inline]
    pub fn raw(self) -> u32 {
        self.0.get()
    }
}

/// Stable id for a tag value (e.g. "Catholic", "English", "Democracy").
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TagId(pub NonZeroU32);

impl TagId {
    #[inline]
    pub fn raw(self) -> u32 {
        self.0.get()
    }
}

/// A tag type/category definition.
#[derive(Clone, Serialize, Deserialize)]
pub struct TagTypeDef {
    pub id: TagTypeId,
    pub name: String,
}

/// A tag value definition.
#[derive(Clone, Serialize, Deserialize)]
pub struct TagDef {
    pub id: TagId,
    pub type_id: TagTypeId,
    pub name: String,
}

/// Registry of tag types and tag values. Data-driven; games can seed this at init.
#[derive(Resource, Clone, Default, Serialize, Deserialize)]
pub struct TagRegistry {
    pub types: Vec<TagTypeDef>,
    pub tags: Vec<TagDef>,

    /// Next ids (1-based). Stored so save/load preserves stable ids.
    pub next_type_raw: u32,
    pub next_tag_raw: u32,

    /// Runtime indexes for fast lookup (rebuilt lazily after deserialize).
    #[serde(skip)]
    type_by_name: HashMap<String, TagTypeId>,
    #[serde(skip)]
    tag_by_type_and_name: HashMap<(u32, String), TagId>,
    #[serde(skip)]
    type_name_by_raw: HashMap<u32, String>,
    #[serde(skip)]
    tag_def_by_raw: HashMap<u32, TagDef>,
}

impl TagRegistry {
    pub fn new() -> Self {
        Self {
            types: Vec::new(),
            tags: Vec::new(),
            next_type_raw: 1,
            next_tag_raw: 1,
            type_by_name: HashMap::new(),
            tag_by_type_and_name: HashMap::new(),
            type_name_by_raw: HashMap::new(),
            tag_def_by_raw: HashMap::new(),
        }
    }

    fn ensure_indexes(&mut self) {
        if !self.type_by_name.is_empty() || !self.tag_def_by_raw.is_empty() {
            return;
        }
        self.rebuild_indexes();
    }

    pub fn rebuild_indexes(&mut self) {
        self.type_by_name.clear();
        self.tag_by_type_and_name.clear();
        self.type_name_by_raw.clear();
        self.tag_def_by_raw.clear();

        for t in &self.types {
            self.type_by_name.insert(t.name.clone(), t.id);
            self.type_name_by_raw.insert(t.id.raw(), t.name.clone());
            self.next_type_raw = self.next_type_raw.max(t.id.raw().saturating_add(1));
        }
        for tag in &self.tags {
            self.tag_by_type_and_name
                .insert((tag.type_id.raw(), tag.name.clone()), tag.id);
            self.tag_def_by_raw.insert(tag.id.raw(), tag.clone());
            self.next_tag_raw = self.next_tag_raw.max(tag.id.raw().saturating_add(1));
        }
        self.next_type_raw = self.next_type_raw.max(1);
        self.next_tag_raw = self.next_tag_raw.max(1);
    }

    /// Register (or get existing) tag type by name.
    pub fn register_type(&mut self, name: impl Into<String>) -> TagTypeId {
        let name = name.into();
        self.ensure_indexes();
        if let Some(id) = self.type_by_name.get(&name).copied() {
            return id;
        }
        let raw = self.next_type_raw.max(1);
        self.next_type_raw = raw.saturating_add(1);
        let id = TagTypeId(NonZeroU32::new(raw).unwrap());
        self.types.push(TagTypeDef {
            id,
            name: name.clone(),
        });
        self.type_by_name.insert(name.clone(), id);
        self.type_name_by_raw.insert(raw, name);
        id
    }

    /// Register (or get existing) tag value.
    pub fn register_tag(&mut self, type_id: TagTypeId, name: impl Into<String>) -> TagId {
        let name = name.into();
        self.ensure_indexes();
        if let Some(id) = self
            .tag_by_type_and_name
            .get(&(type_id.raw(), name.clone()))
            .copied()
        {
            return id;
        }
        let raw = self.next_tag_raw.max(1);
        self.next_tag_raw = raw.saturating_add(1);
        let id = TagId(NonZeroU32::new(raw).unwrap());
        let def = TagDef {
            id,
            type_id,
            name: name.clone(),
        };
        self.tags.push(def.clone());
        self.tag_by_type_and_name
            .insert((type_id.raw(), name), id);
        self.tag_def_by_raw.insert(raw, def);
        id
    }

    pub fn get_type_name(&mut self, id: TagTypeId) -> Option<&str> {
        self.ensure_indexes();
        self.type_name_by_raw.get(&id.raw()).map(String::as_str)
    }

    pub fn get_tag(&mut self, id: TagId) -> Option<&TagDef> {
        self.ensure_indexes();
        self.tag_def_by_raw.get(&id.raw())
    }
}

/// Tags assigned to provinces: (province, tag_type) -> tag_value.
#[derive(Resource, Clone, Default, Serialize, Deserialize)]
pub struct ProvinceTags {
    pub tags: HashMap<(ProvinceId, TagTypeId), TagId>,
}

impl ProvinceTags {
    pub fn get(&self, province: ProvinceId, ty: TagTypeId) -> Option<TagId> {
        self.tags.get(&(province, ty)).copied()
    }

    pub fn set(&mut self, province: ProvinceId, ty: TagTypeId, tag: TagId) {
        self.tags.insert((province, ty), tag);
    }

    pub fn clear(&mut self, province: ProvinceId, ty: TagTypeId) {
        self.tags.remove(&(province, ty));
    }
}

/// Tags assigned to nations: (nation, tag_type) -> tag_value.
#[derive(Resource, Clone, Default, Serialize, Deserialize)]
pub struct NationTags {
    pub tags: HashMap<(NationId, TagTypeId), TagId>,
}

impl NationTags {
    pub fn get(&self, nation: NationId, ty: TagTypeId) -> Option<TagId> {
        self.tags.get(&(nation, ty)).copied()
    }

    pub fn set(&mut self, nation: NationId, ty: TagTypeId, tag: TagId) {
        self.tags.insert((nation, ty), tag);
    }

    pub fn clear(&mut self, nation: NationId, ty: TagTypeId) {
        self.tags.remove(&(nation, ty));
    }
}

