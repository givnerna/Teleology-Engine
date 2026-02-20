//! Pop-up event system (multi-choice, chainable, dev-editable text).
//!
//! Events are data-driven definitions stored in an `EventRegistry`, and runtime instances
//! are queued in an `EventQueue` for UI consumption.

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::num::NonZeroU32;

use crate::event_bus::scope_types;
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

/// Extensible scope / target of an event instance.
///
/// Uses the same scope_type discriminant as [`crate::event_bus::EntityScopeRef`].
/// Well-known types: Global (0), Province (1), Nation (2), Character (3), Army (4).
/// Custom scopes use values >= 1000 by convention.
#[derive(Clone, Copy, Serialize, Deserialize)]
pub struct EventScope {
    /// Scope type (0=Global, 1=Province, 2=Nation, 3=Character, 4=Army, >=1000=custom).
    pub scope_type: u32,
    /// Primary raw entity id (meaning depends on scope_type).
    pub raw: u32,
    /// Secondary raw id (high 32 bits for 64-bit ids like characters).
    pub raw_hi: u32,
}

impl EventScope {
    pub const fn global() -> Self {
        Self { scope_type: scope_types::GLOBAL, raw: 0, raw_hi: 0 }
    }

    pub fn nation(id: NationId) -> Self {
        Self { scope_type: scope_types::NATION, raw: id.0.get(), raw_hi: 0 }
    }

    pub fn province(id: ProvinceId) -> Self {
        Self { scope_type: scope_types::PROVINCE, raw: id.0.get(), raw_hi: 0 }
    }

    pub fn character_raw(raw: u64) -> Self {
        Self {
            scope_type: scope_types::CHARACTER,
            raw: raw as u32,
            raw_hi: (raw >> 32) as u32,
        }
    }

    pub fn army_raw(raw: u32) -> Self {
        Self { scope_type: scope_types::ARMY, raw, raw_hi: 0 }
    }

    /// Custom scope with a game-defined type discriminant.
    pub const fn custom(scope_type: u32, raw: u32) -> Self {
        Self { scope_type, raw, raw_hi: 0 }
    }

    pub fn is_global(&self) -> bool {
        self.scope_type == scope_types::GLOBAL
    }

    // Backwards-compatible constructors matching old enum variant names.

    #[inline]
    #[allow(non_snake_case)]
    pub fn Global() -> Self { Self::global() }
    #[inline]
    #[allow(non_snake_case)]
    pub fn Nation(id: NationId) -> Self { Self::nation(id) }
    #[inline]
    #[allow(non_snake_case)]
    pub fn Province(id: ProvinceId) -> Self { Self::province(id) }
    #[inline]
    #[allow(non_snake_case)]
    pub fn CharacterRaw(raw: u64) -> Self { Self::character_raw(raw) }
    #[inline]
    #[allow(non_snake_case)]
    pub fn ArmyRaw(raw: u32) -> Self { Self::army_raw(raw) }
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
    /// Optional image path shown in the pop-up (overrides global style image).
    pub image: String,
    /// Image width (0 = use global style or auto).
    pub image_w: f32,
    /// Image height (0 = use global style or auto).
    pub image_h: f32,
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

// ---------------------------------------------------------------------------
// Pop-up styling
// ---------------------------------------------------------------------------

/// How the event pop-up should be positioned.
#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum PopupAnchor {
    /// Centered in the window (default).
    Center,
    /// Fixed position (top-left corner of the popup).
    Fixed { x: f32, y: f32 },
}

impl Default for PopupAnchor {
    fn default() -> Self {
        Self::Center
    }
}

/// Visual style for event pop-ups. Scripts set this before queueing an event
/// and it applies to the next pop-up shown.
#[derive(Resource, Clone, Serialize, Deserialize)]
pub struct EventPopupStyle {
    /// Where to anchor the popup window.
    pub anchor: PopupAnchor,
    /// Popup width (0 = auto).
    pub width: f32,
    /// Background color (RGBA).
    pub bg_color: [u8; 4],
    /// Title text color.
    pub title_color: [u8; 4],
    /// Body text color.
    pub body_color: [u8; 4],
    /// Button text color.
    pub button_color: [u8; 4],
    /// Optional image path to show above the body text.
    pub image_path: String,
    /// Image dimensions (if image_path is set).
    pub image_w: f32,
    pub image_h: f32,
    /// Whether the game should pause while this event is showing.
    pub modal: bool,
    /// Title font size (0 = default).
    pub title_font_size: f32,
    /// Body font size (0 = default).
    pub body_font_size: f32,
}

impl Default for EventPopupStyle {
    fn default() -> Self {
        Self {
            anchor: PopupAnchor::Center,
            width: 0.0,
            bg_color: [30, 30, 40, 230],
            title_color: [255, 220, 120, 255],
            body_color: [220, 220, 220, 255],
            button_color: [200, 200, 255, 255],
            image_path: String::new(),
            image_w: 0.0,
            image_h: 0.0,
            modal: true,
            title_font_size: 0.0,
            body_font_size: 0.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Built-in event templates
// ---------------------------------------------------------------------------

/// Pre-made event template identifiers. Scripts can instantiate these
/// to quickly create common event patterns, then customize as needed.
pub enum EventTemplate {
    /// Simple notification: title + body + "OK" button.
    Notification,
    /// Binary choice: title + body + 2 buttons (Accept/Decline).
    BinaryChoice,
    /// Three-way: title + body + 3 buttons.
    ThreeWayChoice,
    /// Narrative event: title + body + "Continue" (chains to next_event).
    Narrative,
    /// Diplomatic proposal: title + body + Accept/Decline/Negotiate.
    DiplomaticProposal,
}

impl EventTemplate {
    /// Create an `EventDefinition` from this template. The caller should
    /// customize the returned definition (change title, body, choice text, etc.)
    /// and then insert it into the `EventRegistry`.
    pub fn create(&self) -> EventDefinition {
        match self {
            EventTemplate::Notification => EventDefinition {
                id: EventId(NonZeroU32::new(1).unwrap()), // placeholder; registry reassigns
                title: "Notification".into(),
                body: "Something has happened in your realm.".into(),
                choices: vec![EventChoice {
                    text: "Acknowledged".into(),
                    next_event: None,
                    effects_payload: Vec::new(),
                }],
                image: String::new(),
                image_w: 0.0,
                image_h: 0.0,
            },
            EventTemplate::BinaryChoice => EventDefinition {
                id: EventId(NonZeroU32::new(1).unwrap()),
                title: "A Decision Awaits".into(),
                body: "You must choose between two paths.".into(),
                choices: vec![
                    EventChoice {
                        text: "Accept".into(),
                        next_event: None,
                        effects_payload: Vec::new(),
                    },
                    EventChoice {
                        text: "Decline".into(),
                        next_event: None,
                        effects_payload: Vec::new(),
                    },
                ],
                image: String::new(),
                image_w: 0.0,
                image_h: 0.0,
            },
            EventTemplate::ThreeWayChoice => EventDefinition {
                id: EventId(NonZeroU32::new(1).unwrap()),
                title: "A Complex Situation".into(),
                body: "Three options present themselves.".into(),
                choices: vec![
                    EventChoice {
                        text: "Option A".into(),
                        next_event: None,
                        effects_payload: Vec::new(),
                    },
                    EventChoice {
                        text: "Option B".into(),
                        next_event: None,
                        effects_payload: Vec::new(),
                    },
                    EventChoice {
                        text: "Option C".into(),
                        next_event: None,
                        effects_payload: Vec::new(),
                    },
                ],
                image: String::new(),
                image_w: 0.0,
                image_h: 0.0,
            },
            EventTemplate::Narrative => EventDefinition {
                id: EventId(NonZeroU32::new(1).unwrap()),
                title: "A Tale Unfolds".into(),
                body: "The story continues...".into(),
                choices: vec![EventChoice {
                    text: "Continue".into(),
                    next_event: None,
                    effects_payload: Vec::new(),
                }],
                image: String::new(),
                image_w: 0.0,
                image_h: 0.0,
            },
            EventTemplate::DiplomaticProposal => EventDefinition {
                id: EventId(NonZeroU32::new(1).unwrap()),
                title: "Diplomatic Proposal".into(),
                body: "A foreign power approaches with a proposition.".into(),
                choices: vec![
                    EventChoice {
                        text: "Accept".into(),
                        next_event: None,
                        effects_payload: Vec::new(),
                    },
                    EventChoice {
                        text: "Decline".into(),
                        next_event: None,
                        effects_payload: Vec::new(),
                    },
                    EventChoice {
                        text: "Negotiate".into(),
                        next_event: None,
                        effects_payload: Vec::new(),
                    },
                ],
                image: String::new(),
                image_w: 0.0,
                image_h: 0.0,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Keyword tooltip system
// ---------------------------------------------------------------------------

/// A single keyword entry: when this keyword appears in event text,
/// it becomes hoverable and shows the description in a tooltip panel.
#[derive(Clone, Serialize, Deserialize)]
pub struct KeywordEntry {
    /// The keyword string to match in text (case-insensitive matching).
    pub keyword: String,
    /// Short title shown at the top of the tooltip.
    pub title: String,
    /// Longer description body shown in the tooltip panel.
    pub description: String,
    /// Optional icon/image path displayed in the tooltip.
    pub icon: String,
    /// Highlight color for the keyword in text (RGBA). [0,0,0,0] = use default.
    pub color: [u8; 4],
}

/// Global keyword registry. Scripts register keywords here and any event/UI
/// text that contains them will render them as highlighted, hoverable spans.
#[derive(Resource, Clone, Default, Serialize, Deserialize)]
pub struct KeywordRegistry {
    pub entries: Vec<KeywordEntry>,
}

impl KeywordRegistry {
    /// Register a keyword. Returns its index.
    pub fn add(&mut self, entry: KeywordEntry) -> usize {
        let idx = self.entries.len();
        self.entries.push(entry);
        idx
    }

    /// Remove a keyword by index. Returns true if removed.
    pub fn remove(&mut self, idx: usize) -> bool {
        if idx < self.entries.len() {
            self.entries.remove(idx);
            true
        } else {
            false
        }
    }

    /// Clear all keywords.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Find all keyword matches in a piece of text.
    /// Returns (byte_start, byte_end, entry_index) sorted by position.
    pub fn find_matches(&self, text: &str) -> Vec<(usize, usize, usize)> {
        let lower = text.to_lowercase();
        let mut matches = Vec::new();
        for (i, entry) in self.entries.iter().enumerate() {
            let kw = entry.keyword.to_lowercase();
            if kw.is_empty() {
                continue;
            }
            let mut start = 0;
            while let Some(pos) = lower[start..].find(&kw) {
                let abs = start + pos;
                matches.push((abs, abs + kw.len(), i));
                start = abs + kw.len();
            }
        }
        // Sort by position, then prefer longer matches first at same position
        matches.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
        // Remove overlapping matches (greedy: keep first)
        let mut filtered = Vec::new();
        let mut end = 0usize;
        for m in matches {
            if m.0 >= end {
                filtered.push(m);
                end = m.1;
            }
        }
        filtered
    }

    /// Load keywords from a JSON string, appending to existing entries.
    /// The JSON should be an array of keyword objects:
    /// ```json
    /// [
    ///   {
    ///     "keyword": "Prestige",
    ///     "title": "Prestige",
    ///     "description": "A measure of your realm's renown.",
    ///     "icon": "icons/prestige.png",
    ///     "color": [255, 215, 0, 255]
    ///   }
    /// ]
    /// ```
    /// Fields `icon` and `color` are optional (default to "" and [0,0,0,0]).
    pub fn load_from_json(&mut self, json: &str) -> Result<usize, String> {
        let defs: Vec<KeywordJsonDef> =
            serde_json::from_str(json).map_err(|e| format!("keyword JSON parse error: {e}"))?;
        let count = defs.len();
        for d in defs {
            self.add(KeywordEntry {
                keyword: d.keyword,
                title: d.title,
                description: d.description,
                icon: d.icon.unwrap_or_default(),
                color: d.color.unwrap_or([0, 0, 0, 0]),
            });
        }
        Ok(count)
    }

    /// Load keywords from a JSON file, appending to existing entries.
    /// Returns the number of keywords loaded.
    pub fn load_from_file(&mut self, path: &std::path::Path) -> Result<usize, String> {
        let contents = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        self.load_from_json(&contents)
    }

    /// Save current keywords to a JSON file (pretty-printed).
    pub fn save_to_file(&self, path: &std::path::Path) -> Result<(), String> {
        let defs: Vec<KeywordJsonDef> = self
            .entries
            .iter()
            .map(|e| KeywordJsonDef {
                keyword: e.keyword.clone(),
                title: e.title.clone(),
                description: e.description.clone(),
                icon: if e.icon.is_empty() { None } else { Some(e.icon.clone()) },
                color: if e.color == [0, 0, 0, 0] { None } else { Some(e.color) },
            })
            .collect();
        let json = serde_json::to_string_pretty(&defs)
            .map_err(|e| format!("keyword JSON serialize error: {e}"))?;
        std::fs::write(path, json)
            .map_err(|e| format!("failed to write {}: {e}", path.display()))
    }
}

/// Internal JSON-friendly representation for keyword serialization.
/// Allows `icon` and `color` to be omitted in the file.
#[derive(Serialize, Deserialize)]
struct KeywordJsonDef {
    keyword: String,
    title: String,
    description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    color: Option<[u8; 4]>,
}

/// Register all built-in templates into an `EventRegistry`, returning
/// the IDs so scripts can reference them. Returns (notification, binary,
/// three_way, narrative, diplomatic).
pub fn register_builtin_templates(reg: &mut EventRegistry) -> [EventId; 5] {
    let templates = [
        EventTemplate::Notification,
        EventTemplate::BinaryChoice,
        EventTemplate::ThreeWayChoice,
        EventTemplate::Narrative,
        EventTemplate::DiplomaticProposal,
    ];
    let mut ids = [EventId(NonZeroU32::new(1).unwrap()); 5];
    for (i, tmpl) in templates.iter().enumerate() {
        ids[i] = reg.insert(tmpl.create());
    }
    ids
}
