//! Pop-up event system (multi-choice, chainable, dev-editable text).
//!
//! Events are data-driven definitions stored in an `EventRegistry`, and runtime instances
//! are queued in an `EventQueue` for UI consumption.

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::num::NonZeroU32;

use crate::world::{NationId, ProvinceId};

/// Stable id for an event definition.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EventId(pub NonZeroU32);

impl EventId {
    #[inline]
    pub fn raw(self) -> u32 {
        self.0.get()
    }
}

/// Scope / target of an event instance.
#[derive(Clone, Copy, Serialize, Deserialize)]
pub enum EventScope {
    Global,
    Nation(NationId),
    Province(ProvinceId),
    /// Raw ids for optional modules (characters/armies) to keep this module independent.
    CharacterRaw(u64),
    ArmyRaw(u32),
}

/// One choice in an event.
#[derive(Clone, Serialize, Deserialize)]
pub struct EventChoice {
    pub text: String,
    /// Optional next event to chain into after choosing this option.
    pub next_event: Option<EventId>,
    /// Game-defined opaque effects payload. Scripts or engine systems can interpret this.
    pub effects_payload: Vec<u8>,
}

/// Event definition (data-driven).
#[derive(Clone, Serialize, Deserialize)]
pub struct EventDefinition {
    pub id: EventId,
    pub title: String,
    pub body: String,
    pub choices: Vec<EventChoice>,
}

/// Event registry: stores definitions.
#[derive(Resource, Clone, Default, Serialize, Deserialize)]
pub struct EventRegistry {
    pub events: HashMap<u32, EventDefinition>,
    pub next_id_raw: u32,
}

impl EventRegistry {
    pub fn new() -> Self {
        Self {
            events: HashMap::new(),
            next_id_raw: 1,
        }
    }

    fn alloc_id(&mut self) -> EventId {
        let raw = self.next_id_raw.max(1);
        self.next_id_raw = raw.saturating_add(1);
        EventId(NonZeroU32::new(raw).unwrap())
    }

    pub fn insert(&mut self, mut def: EventDefinition) -> EventId {
        let id = self.alloc_id();
        def.id = id;
        self.events.insert(id.raw(), def);
        id
    }

    pub fn get(&self, id: EventId) -> Option<&EventDefinition> {
        self.events.get(&id.raw())
    }
}

/// One queued event instance (runtime).
#[derive(Clone, Serialize, Deserialize)]
pub struct EventInstance {
    pub event_id: EventId,
    pub scope: EventScope,
    /// Optional payload for parameterized events.
    pub payload: Vec<u8>,
}

/// Queue of pending events to show as pop-ups.
#[derive(Resource, Clone, Default, Serialize, Deserialize)]
pub struct EventQueue {
    pub pending: VecDeque<EventInstance>,
}

impl EventQueue {
    pub fn push(&mut self, inst: EventInstance) {
        self.pending.push_back(inst);
    }

    pub fn pop(&mut self) -> Option<EventInstance> {
        self.pending.pop_front()
    }
}

/// Active event (currently displayed in UI).
#[derive(Resource, Clone, Default, Serialize, Deserialize)]
pub struct ActiveEvent {
    pub current: Option<EventInstance>,
}

/// Helper: queue an event instance if the queue exists.
pub fn queue_event(world: &mut World, event_id: EventId, scope: EventScope, payload: Vec<u8>) {
    if let Some(mut q) = world.get_resource_mut::<EventQueue>() {
        q.push(EventInstance {
            event_id,
            scope,
            payload,
        });
    }
}

/// Helper: advance active event (UI should call each frame/tick).
pub fn pull_next_event(world: &mut World) {
    let next = {
        let Some(mut q) = world.get_resource_mut::<EventQueue>() else { return };
        q.pop()
    };
    if let Some(mut active) = world.get_resource_mut::<ActiveEvent>() {
        if active.current.is_none() {
            active.current = next;
        } else if next.is_some() {
            // If already showing one, push it back (FIFO).
            if let Some(mut q) = world.get_resource_mut::<EventQueue>() {
                q.pending.push_front(next.unwrap());
            }
        }
    }
}

