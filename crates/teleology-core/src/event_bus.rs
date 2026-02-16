//! EventBus: dev-facing publish/poll messaging, intended for scripts and modular systems.
//!
//! Delivery model: queued. Systems can publish events during a tick; consumers poll/drain at safe points.

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::num::NonZeroU32;

/// Stable id for an event topic (string-backed).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EventTopicId(pub NonZeroU32);

impl EventTopicId {
    #[inline]
    pub fn raw(self) -> u32 {
        self.0.get()
    }
}

/// Optional scope for routing/filtering.
#[derive(Clone, Copy, Serialize, Deserialize)]
pub enum EventScopeRef {
    Global,
    NationRaw(u32),
    ProvinceRaw(u32),
    CharacterRaw(u64),
    ArmyRaw(u32),
}

/// Opaque payload.
#[derive(Clone, Serialize, Deserialize)]
pub struct EventPayload {
    pub payload_type_id: u32,
    pub bytes: Vec<u8>,
}

/// One published event.
#[derive(Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub topic: EventTopicId,
    pub scope: EventScopeRef,
    pub payload: EventPayload,
    /// Game-defined timestamp for ordering (optional; 0 if unused).
    pub timestamp: i64,
}

/// EventBus resource: topic registry + queued events.
#[derive(Resource, Clone, Default, Serialize, Deserialize)]
pub struct EventBus {
    /// id_to_topic[id_raw-1] = topic string
    pub id_to_topic: Vec<String>,
    pub next_topic_raw: u32,

    pub queue: VecDeque<EventEnvelope>,

    #[serde(skip)]
    topic_to_id: HashMap<String, EventTopicId>,
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            id_to_topic: Vec::new(),
            next_topic_raw: 1,
            queue: VecDeque::new(),
            topic_to_id: HashMap::new(),
        }
    }

    fn ensure_index(&mut self) {
        if !self.topic_to_id.is_empty() {
            return;
        }
        for (i, name) in self.id_to_topic.iter().enumerate() {
            let raw = (i as u32) + 1;
            self.topic_to_id
                .insert(name.clone(), EventTopicId(NonZeroU32::new(raw).unwrap()));
            self.next_topic_raw = self.next_topic_raw.max(raw.saturating_add(1));
        }
        self.next_topic_raw = self.next_topic_raw.max(1);
    }

    pub fn get_or_register_topic(&mut self, name: &str) -> EventTopicId {
        self.ensure_index();
        if let Some(id) = self.topic_to_id.get(name).copied() {
            return id;
        }
        let raw = self.next_topic_raw.max(1);
        self.next_topic_raw = raw.saturating_add(1);
        let id = EventTopicId(NonZeroU32::new(raw).unwrap());
        self.id_to_topic.push(name.to_string());
        self.topic_to_id.insert(name.to_string(), id);
        id
    }

    pub fn topic_name(&self, id: EventTopicId) -> Option<&str> {
        self.id_to_topic.get((id.raw() - 1) as usize).map(String::as_str)
    }

    pub fn publish(&mut self, env: EventEnvelope) {
        self.queue.push_back(env);
    }

    pub fn poll(&mut self) -> Option<EventEnvelope> {
        self.queue.pop_front()
    }

    pub fn drain_all(&mut self) -> Vec<EventEnvelope> {
        self.queue.drain(..).collect()
    }
}

/// Helper: publish to EventBus if enabled.
pub fn publish_event(
    world: &mut World,
    topic: &str,
    scope: EventScopeRef,
    payload_type_id: u32,
    bytes: Vec<u8>,
    timestamp: i64,
) {
    let Some(mut bus) = world.get_resource_mut::<EventBus>() else { return };
    let topic_id = bus.get_or_register_topic(topic);
    bus.publish(EventEnvelope {
        topic: topic_id,
        scope,
        payload: EventPayload {
            payload_type_id,
            bytes,
        },
        timestamp,
    });
}

