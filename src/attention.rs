use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use zbus::zvariant::Type;

pub const EVENT_STORE_CAPACITY: usize = 128;
pub const EVENT_CHANNEL_CAPACITY: usize = 16;
pub const MAX_SOURCE_BYTES: usize = 64;
pub const MAX_KIND_BYTES: usize = 64;
pub const MAX_TITLE_BYTES: usize = 256;
pub const MAX_BODY_BYTES: usize = 4096;

pub const ATTENTION_OBJECT_PATH: &str = "/org/wumbos/wumbosd/Attention";
pub const ATTENTION_INTERFACE_NAME: &str = "org.wumbos.wumbosd.Attention1";

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, Type)]
pub struct AttentionEvent {
    pub id: u64,
    pub created_at_ms: u64,
    pub source: String,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub urgency: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PublishError {
    EmptyField(&'static str),
    FieldTooLong(&'static str),
    InvalidUrgency,
    IdExhausted,
}

impl std::fmt::Display for PublishError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyField(field) => write!(formatter, "{field} must not be empty"),
            Self::FieldTooLong(field) => write!(formatter, "{field} exceeds its byte limit"),
            Self::InvalidUrgency => write!(
                formatter,
                "urgency must be low (0), normal (1), or critical (2)"
            ),
            Self::IdExhausted => write!(formatter, "attention event id space exhausted"),
        }
    }
}

pub struct EventStore {
    next_id: Option<u64>,
    events: VecDeque<AttentionEvent>,
}

impl EventStore {
    pub fn new() -> Self {
        Self {
            next_id: Some(1),
            events: VecDeque::with_capacity(EVENT_STORE_CAPACITY),
        }
    }

    pub fn publish(
        &mut self,
        source: String,
        kind: String,
        title: String,
        body: String,
        urgency: u8,
    ) -> Result<AttentionEvent, PublishError> {
        validate_publish(&source, &kind, &title, &body, urgency)?;
        let id = self.next_id.ok_or(PublishError::IdExhausted)?;
        self.next_id = id.checked_add(1);
        let event = AttentionEvent {
            id,
            created_at_ms: epoch_milliseconds(),
            source,
            kind,
            title,
            body,
            urgency,
        };
        self.events.push_back(event.clone());
        if self.events.len() > EVENT_STORE_CAPACITY {
            self.events.pop_front();
        }
        Ok(event)
    }

    pub fn dismiss(&mut self, id: u64) -> bool {
        let Some(index) = self.events.iter().position(|event| event.id == id) else {
            return false;
        };
        self.events.remove(index);
        true
    }

    pub fn clear(&mut self) -> u32 {
        let count = self.events.len().try_into().unwrap_or(u32::MAX);
        self.events.clear();
        count
    }

    pub fn recent(&self, limit: u32) -> Vec<AttentionEvent> {
        self.events
            .iter()
            .rev()
            .take(
                usize::try_from(limit)
                    .unwrap_or(usize::MAX)
                    .min(EVENT_STORE_CAPACITY),
            )
            .cloned()
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttentionUpdate {
    Added(AttentionEvent),
    Removed(u64),
    Cleared,
}

#[derive(Clone)]
pub struct AttentionState {
    store: Arc<Mutex<EventStore>>,
    sender: broadcast::Sender<AttentionUpdate>,
}

impl AttentionState {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        Self {
            store: Arc::new(Mutex::new(EventStore::new())),
            sender,
        }
    }

    pub fn publish(
        &self,
        source: String,
        kind: String,
        title: String,
        body: String,
        urgency: u8,
    ) -> Result<AttentionEvent, PublishError> {
        let event = self
            .store
            .lock()
            .expect("attention event store lock poisoned")
            .publish(source, kind, title, body, urgency)?;
        let _ = self.sender.send(AttentionUpdate::Added(event.clone()));
        Ok(event)
    }

    pub fn publish_with(
        &self,
        source: String,
        kind: String,
        title: String,
        body: String,
        urgency: u8,
        before_broadcast: impl FnOnce(&AttentionEvent),
    ) -> Result<AttentionEvent, PublishError> {
        let event = self
            .store
            .lock()
            .expect("attention event store lock poisoned")
            .publish(source, kind, title, body, urgency)?;
        before_broadcast(&event);
        let _ = self.sender.send(AttentionUpdate::Added(event.clone()));
        Ok(event)
    }

    pub fn dismiss(&self, id: u64) -> bool {
        let dismissed = self
            .store
            .lock()
            .expect("attention event store lock poisoned")
            .dismiss(id);
        if dismissed {
            let _ = self.sender.send(AttentionUpdate::Removed(id));
        }
        dismissed
    }

    pub fn clear(&self) -> u32 {
        let count = self
            .store
            .lock()
            .expect("attention event store lock poisoned")
            .clear();
        if count > 0 {
            let _ = self.sender.send(AttentionUpdate::Cleared);
        }
        count
    }

    pub fn recent(&self, limit: u32) -> Vec<AttentionEvent> {
        self.store
            .lock()
            .expect("attention event store lock poisoned")
            .recent(limit)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AttentionUpdate> {
        self.sender.subscribe()
    }
}

pub struct AttentionService {
    state: AttentionState,
}

impl AttentionService {
    pub fn new(state: AttentionState) -> Self {
        Self { state }
    }
}

#[zbus::interface(name = "org.wumbos.wumbosd.Attention1")]
impl AttentionService {
    async fn publish(
        &self,
        source: String,
        kind: String,
        title: String,
        body: String,
        urgency: u8,
        #[zbus(signal_emitter)] emitter: zbus::object_server::SignalEmitter<'_>,
    ) -> zbus::fdo::Result<u64> {
        let event = self
            .state
            .publish(source, kind, title, body, urgency)
            .map_err(|error| zbus::fdo::Error::InvalidArgs(error.to_string()))?;
        Self::event_added(&emitter, &event)
            .await
            .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?;
        Ok(event.id)
    }

    fn recent(&self, limit: u32) -> Vec<AttentionEvent> {
        self.state.recent(limit)
    }

    async fn dismiss(
        &self,
        id: u64,
        #[zbus(signal_emitter)] emitter: zbus::object_server::SignalEmitter<'_>,
    ) -> zbus::fdo::Result<bool> {
        let dismissed = self.state.dismiss(id);
        if dismissed {
            Self::event_removed(&emitter, id)
                .await
                .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?;
        }
        Ok(dismissed)
    }

    async fn clear(
        &self,
        #[zbus(signal_emitter)] emitter: zbus::object_server::SignalEmitter<'_>,
    ) -> zbus::fdo::Result<u32> {
        let count = self.state.clear();
        if count > 0 {
            Self::event_cleared(&emitter)
                .await
                .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?;
        }
        Ok(count)
    }

    #[zbus(signal)]
    async fn event_added(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        event: &AttentionEvent,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn event_removed(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        id: u64,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn event_cleared(emitter: &zbus::object_server::SignalEmitter<'_>) -> zbus::Result<()>;
}

fn validate_publish(
    source: &str,
    kind: &str,
    title: &str,
    body: &str,
    urgency: u8,
) -> Result<(), PublishError> {
    validate_required_field("source", source, MAX_SOURCE_BYTES)?;
    validate_required_field("kind", kind, MAX_KIND_BYTES)?;
    validate_required_field("title", title, MAX_TITLE_BYTES)?;
    validate_optional_field("body", body, MAX_BODY_BYTES)?;
    if urgency > 2 {
        return Err(PublishError::InvalidUrgency);
    }
    Ok(())
}

fn validate_required_field(
    name: &'static str,
    value: &str,
    maximum_bytes: usize,
) -> Result<(), PublishError> {
    if value.is_empty() {
        return Err(PublishError::EmptyField(name));
    }
    validate_optional_field(name, value, maximum_bytes)
}

fn validate_optional_field(
    name: &'static str,
    value: &str,
    maximum_bytes: usize,
) -> Result<(), PublishError> {
    if value.len() > maximum_bytes {
        return Err(PublishError::FieldTooLong(name));
    }
    Ok(())
}

fn epoch_milliseconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn publish(store: &mut EventStore, source: &str) -> AttentionEvent {
        store
            .publish(
                source.to_owned(),
                "message".to_owned(),
                "Title".to_owned(),
                String::new(),
                1,
            )
            .unwrap()
    }

    #[test]
    fn allocates_first_and_monotonic_ids_with_timestamp() {
        let mut store = EventStore::new();
        let first = publish(&mut store, "first");
        let second = publish(&mut store, "second");

        assert_eq!(first.id, 1);
        assert_eq!(second.id, 2);
        assert!(first.created_at_ms > 0);
        assert!(second.created_at_ms >= first.created_at_ms);
    }

    #[test]
    fn event_dbus_signature_is_stable() {
        assert_eq!(<AttentionEvent as Type>::SIGNATURE.to_string(), "(ttssssy)");
    }

    #[test]
    fn rejects_missing_and_invalid_fields() {
        let mut store = EventStore::new();
        for (source, kind, title, urgency) in [
            ("", "message", "Title", 1),
            ("source", "", "Title", 1),
            ("source", "message", "", 1),
            ("source", "message", "Title", 3),
        ] {
            assert!(
                store
                    .publish(
                        source.to_owned(),
                        kind.to_owned(),
                        title.to_owned(),
                        String::new(),
                        urgency,
                    )
                    .is_err()
            );
        }
    }

    #[test]
    fn rejects_each_field_byte_bound() {
        let mut store = EventStore::new();
        for (source, kind, title, body, expected_field) in [
            (
                "x".repeat(MAX_SOURCE_BYTES + 1),
                "kind".to_owned(),
                "title".to_owned(),
                String::new(),
                "source",
            ),
            (
                "source".to_owned(),
                "x".repeat(MAX_KIND_BYTES + 1),
                "title".to_owned(),
                String::new(),
                "kind",
            ),
            (
                "source".to_owned(),
                "kind".to_owned(),
                "x".repeat(MAX_TITLE_BYTES + 1),
                String::new(),
                "title",
            ),
            (
                "source".to_owned(),
                "kind".to_owned(),
                "title".to_owned(),
                "x".repeat(MAX_BODY_BYTES + 1),
                "body",
            ),
        ] {
            assert_eq!(
                store.publish(source, kind, title, body, 1),
                Err(PublishError::FieldTooLong(expected_field))
            );
        }
    }

    #[test]
    fn retains_128_events_and_evicts_oldest() {
        let mut store = EventStore::new();
        for index in 0..=EVENT_STORE_CAPACITY {
            publish(&mut store, &format!("source-{index}"));
        }

        let events = store.recent(u32::MAX);
        assert_eq!(events.len(), EVENT_STORE_CAPACITY);
        assert_eq!(events.first().unwrap().id, 129);
        assert_eq!(events.last().unwrap().id, 2);
        assert!(store.recent(0).is_empty());
        assert_eq!(store.recent(2).len(), 2);
    }
    #[test]
    fn dismiss_removes_only_matching_event_and_preserves_ids() {
        let mut store = EventStore::new();
        let first = publish(&mut store, "first");
        let second = publish(&mut store, "second");
        assert!(store.dismiss(first.id));
        assert!(!store.dismiss(first.id));
        assert_eq!(store.recent(u32::MAX), vec![second.clone()]);
        assert_eq!(publish(&mut store, "third").id, 3);
    }

    #[test]
    fn clear_returns_count_and_does_not_reset_ids() {
        let mut store = EventStore::new();
        publish(&mut store, "first");
        publish(&mut store, "second");
        assert_eq!(store.clear(), 2);
        assert_eq!(store.clear(), 0);
        assert_eq!(publish(&mut store, "third").id, 3);
    }
}
