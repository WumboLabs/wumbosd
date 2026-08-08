use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use zbus::{fdo, object_server::SignalEmitter, zvariant::OwnedValue};

use crate::attention::{
    ATTENTION_INTERFACE_NAME, ATTENTION_OBJECT_PATH, AttentionEvent, AttentionState,
    MAX_BODY_BYTES, MAX_SOURCE_BYTES, MAX_TITLE_BYTES,
};

pub const NOTIFICATION_BUS_NAME: &str = "org.freedesktop.Notifications";
pub const NOTIFICATION_OBJECT_PATH: &str = "/org/freedesktop/Notifications";
pub const NOTIFICATION_INTERFACE_NAME: &str = "org.freedesktop.Notifications";
pub const NOTIFICATION_SPEC_VERSION: &str = "1.3";
pub const NOTIFICATION_SERVER_NAME: &str = "wumbOS wumbosd";
pub const NOTIFICATION_SERVER_VENDOR: &str = "wumbOS";

pub const MAX_ACTIONS: usize = 16;
pub const MAX_ACTION_KEY_BYTES: usize = 256;
pub const MAX_ACTION_LABEL_BYTES: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotificationAction {
    pub key: String,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveNotification {
    pub id: u32,
    pub attention_event_id: u64,
    pub actions: Vec<NotificationAction>,
}

#[derive(Default)]
struct NotificationStore {
    next_id: u32,
    active: HashMap<u32, ActiveNotification>,
}

impl NotificationStore {
    fn new() -> Self {
        Self {
            next_id: 1,
            active: HashMap::new(),
        }
    }

    fn notification_id(&mut self, replaces_id: u32) -> Result<u32, NotificationError> {
        if replaces_id != 0 {
            return Ok(replaces_id);
        }

        loop {
            let id = self.next_id;
            self.next_id = self
                .next_id
                .checked_add(1)
                .ok_or(NotificationError::IdExhausted)?;
            if !self.active.contains_key(&id) {
                return Ok(id);
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum NotificationError {
    IdExhausted,
    UnknownId,
    UnknownAction,
}

impl std::fmt::Display for NotificationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IdExhausted => write!(formatter, "notification id space exhausted"),
            Self::UnknownId => write!(formatter, "unknown notification id"),
            Self::UnknownAction => write!(formatter, "unknown notification action"),
        }
    }
}

#[derive(Clone)]
pub struct NotificationState {
    store: Arc<Mutex<NotificationStore>>,
    attention: AttentionState,
}

impl NotificationState {
    pub fn new(attention: AttentionState) -> Self {
        Self {
            store: Arc::new(Mutex::new(NotificationStore::new())),
            attention,
        }
    }

    fn notify(
        &self,
        notification: NotificationInput,
    ) -> Result<(u32, AttentionEvent), NotificationError> {
        let id = self
            .store
            .lock()
            .expect("notification store lock poisoned")
            .notification_id(notification.replaces_id)?;
        let actions = notification.actions;
        let store = self.store.clone();
        let event = self
            .attention
            .publish_with(
                notification.source,
                "notification".to_owned(),
                notification.title,
                notification.body,
                notification.urgency,
                move |event| {
                    store
                        .lock()
                        .expect("notification store lock poisoned")
                        .active
                        .insert(
                            id,
                            ActiveNotification {
                                id,
                                attention_event_id: event.id,
                                actions,
                            },
                        );
                },
            )
            .map_err(|_| NotificationError::IdExhausted)?;
        Ok((id, event))
    }

    pub fn active_for_event(&self, event_id: u64) -> Option<ActiveNotification> {
        self.store
            .lock()
            .expect("notification store lock poisoned")
            .active
            .values()
            .find(|notification| notification.attention_event_id == event_id)
            .cloned()
    }

    pub fn invoke(&self, id: u32, action_key: &str) -> Result<(), NotificationError> {
        let store = self.store.lock().expect("notification store lock poisoned");
        let notification = store.active.get(&id).ok_or(NotificationError::UnknownId)?;
        if notification
            .actions
            .iter()
            .any(|action| action.key == action_key)
        {
            Ok(())
        } else {
            Err(NotificationError::UnknownAction)
        }
    }

    fn close(&self, id: u32) -> Result<(), NotificationError> {
        if self
            .store
            .lock()
            .expect("notification store lock poisoned")
            .active
            .remove(&id)
            .is_some()
        {
            Ok(())
        } else {
            Err(NotificationError::UnknownId)
        }
    }
}

pub struct NotificationService {
    state: NotificationState,
}

impl NotificationService {
    pub fn new(state: NotificationState) -> Self {
        Self { state }
    }
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl NotificationService {
    fn get_capabilities(&self) -> Vec<&'static str> {
        vec!["body", "actions"]
    }

    #[allow(clippy::too_many_arguments)] // Freedesktop Notify has eight fixed protocol parameters.
    async fn notify(
        &self,
        app_name: String,
        replaces_id: u32,
        _app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: std::collections::HashMap<String, OwnedValue>,
        _expire_timeout: i32,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<u32> {
        let notification =
            NotificationInput::new(app_name, replaces_id, summary, body, actions, hints);
        let (id, event) = self
            .state
            .notify(notification)
            .map_err(|error| fdo::Error::Failed(error.to_string()))?;
        let attention_emitter = SignalEmitter::new(emitter.connection(), ATTENTION_OBJECT_PATH)
            .map_err(|error| fdo::Error::Failed(error.to_string()))?;
        attention_emitter
            .emit(ATTENTION_INTERFACE_NAME, "EventAdded", &event)
            .await
            .map_err(|error| fdo::Error::Failed(error.to_string()))?;
        Ok(id)
    }

    async fn close_notification(
        &self,
        id: u32,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        self.state
            .close(id)
            .map_err(|_| fdo::Error::InvalidArgs("unknown notification id".to_owned()))?;
        Self::notification_closed(&emitter, id, 3)
            .await
            .map_err(|error| fdo::Error::Failed(error.to_string()))
    }

    fn get_server_information(&self) -> (&'static str, &'static str, &'static str, &'static str) {
        (
            NOTIFICATION_SERVER_NAME,
            NOTIFICATION_SERVER_VENDOR,
            env!("CARGO_PKG_VERSION"),
            NOTIFICATION_SPEC_VERSION,
        )
    }

    #[zbus(signal)]
    async fn notification_closed(
        emitter: &SignalEmitter<'_>,
        id: u32,
        reason: u32,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn action_invoked(
        emitter: &SignalEmitter<'_>,
        id: u32,
        action_key: &str,
    ) -> zbus::Result<()>;
}

struct NotificationInput {
    replaces_id: u32,
    source: String,
    title: String,
    body: String,
    urgency: u8,
    actions: Vec<NotificationAction>,
}

impl NotificationInput {
    fn new(
        app_name: String,
        replaces_id: u32,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: std::collections::HashMap<String, OwnedValue>,
    ) -> Self {
        let desktop_entry = hints
            .get("desktop-entry")
            .and_then(|value| <&str>::try_from(value).ok())
            .filter(|value| !value.is_empty());
        let source = truncate_utf8(
            desktop_entry.unwrap_or_else(|| {
                if app_name.is_empty() {
                    "notifications"
                } else {
                    &app_name
                }
            }),
            MAX_SOURCE_BYTES,
        );
        let title = truncate_utf8(
            if summary.is_empty() {
                if app_name.is_empty() {
                    "Notification"
                } else {
                    &app_name
                }
            } else {
                &summary
            },
            MAX_TITLE_BYTES,
        );
        let urgency = hints
            .get("urgency")
            .and_then(|value| u8::try_from(value).ok())
            .filter(|urgency| *urgency <= 2)
            .unwrap_or(1);

        Self {
            replaces_id,
            source,
            title,
            body: truncate_utf8(&body, MAX_BODY_BYTES),
            urgency,
            actions: parse_actions(actions),
        }
    }
}

fn parse_actions(actions: Vec<String>) -> Vec<NotificationAction> {
    actions
        .chunks_exact(2)
        .take(MAX_ACTIONS)
        .map(|pair| NotificationAction {
            key: truncate_utf8(&pair[0], MAX_ACTION_KEY_BYTES),
            label: truncate_utf8(&pair[1], MAX_ACTION_LABEL_BYTES),
        })
        .collect()
}

fn truncate_utf8(value: &str, maximum_bytes: usize) -> String {
    if value.len() <= maximum_bytes {
        return value.to_owned();
    }

    let mut end = maximum_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attention::{MAX_BODY_BYTES, MAX_SOURCE_BYTES, MAX_TITLE_BYTES};

    fn input(app_name: &str, replaces_id: u32) -> NotificationInput {
        NotificationInput::new(
            app_name.to_owned(),
            replaces_id,
            "Summary".to_owned(),
            "Body".to_owned(),
            Default::default(),
            Default::default(),
        )
    }

    #[test]
    fn notification_ids_start_nonzero_and_increase() {
        let state = NotificationState::new(AttentionState::new());
        let (first, _) = state.notify(input("app", 0)).unwrap();
        let (second, _) = state.notify(input("app", 0)).unwrap();
        assert_eq!((first, second), (1, 2));
    }

    #[test]
    fn replacement_reuses_id_and_appends_attention_event() {
        let attention = AttentionState::new();
        let state = NotificationState::new(attention.clone());
        let (id, first) = state.notify(input("app", 0)).unwrap();
        let (replacement_id, second) = state.notify(input("app", id)).unwrap();
        assert_eq!(replacement_id, id);
        assert_eq!((first.id, second.id), (1, 2));
        assert_eq!(attention.recent(10).len(), 2);
    }

    #[test]
    fn replacement_of_unknown_id_retains_requested_id_without_reusing_it() {
        let state = NotificationState::new(AttentionState::new());
        let (replacement_id, _) = state.notify(input("app", 1)).unwrap();
        let (new_id, _) = state.notify(input("app", 0)).unwrap();
        assert_eq!(replacement_id, 1);
        assert_eq!(new_id, 2);
    }

    #[test]
    fn close_known_succeeds_and_unknown_fails() {
        let state = NotificationState::new(AttentionState::new());
        let (id, _) = state.notify(input("app", 0)).unwrap();
        assert!(state.close(id).is_ok());
        assert!(matches!(state.close(id), Err(NotificationError::UnknownId)));
    }

    #[test]
    fn mapping_uses_desktop_entry_and_urgency() {
        let mut hints = std::collections::HashMap::new();
        hints.insert(
            "desktop-entry".to_owned(),
            OwnedValue::from(zbus::zvariant::Str::from("org.example.App")),
        );
        hints.insert("urgency".to_owned(), OwnedValue::from(2_u8));
        let input = NotificationInput::new(
            "fallback".to_owned(),
            0,
            String::new(),
            String::new(),
            Default::default(),
            hints,
        );
        assert_eq!(input.source, "org.example.App");
        assert_eq!(input.title, "fallback");
        assert_eq!(input.urgency, 2);
    }

    #[test]
    fn missing_urgency_defaults_to_normal() {
        assert_eq!(input("app", 0).urgency, 1);
    }

    #[test]
    fn urgency_hint_maps_low_normal_and_critical() {
        for urgency in 0_u8..=2 {
            let mut hints = std::collections::HashMap::new();
            hints.insert("urgency".to_owned(), OwnedValue::from(urgency));
            assert_eq!(
                NotificationInput::new(
                    "app".to_owned(),
                    0,
                    "Summary".to_owned(),
                    String::new(),
                    Default::default(),
                    hints,
                )
                .urgency,
                urgency
            );
        }
    }

    #[test]
    fn normalization_is_utf8_safe_and_bounded() {
        let value = "é".repeat(MAX_BODY_BYTES);
        let normalized = truncate_utf8(&value, MAX_BODY_BYTES);
        assert!(normalized.is_char_boundary(normalized.len()));
        assert!(normalized.len() <= MAX_BODY_BYTES);
        assert_eq!(
            truncate_utf8(&"x".repeat(MAX_SOURCE_BYTES + 1), MAX_SOURCE_BYTES).len(),
            MAX_SOURCE_BYTES
        );
        assert_eq!(
            truncate_utf8(&"x".repeat(MAX_TITLE_BYTES + 1), MAX_TITLE_BYTES).len(),
            MAX_TITLE_BYTES
        );
    }

    #[test]
    fn unsupported_hints_do_not_affect_mapping() {
        let mut hints = std::collections::HashMap::new();
        hints.insert(
            "category".to_owned(),
            OwnedValue::from(zbus::zvariant::Str::from("email")),
        );
        let input = NotificationInput::new(
            "app".to_owned(),
            0,
            "Summary".to_owned(),
            String::new(),
            Default::default(),
            hints,
        );
        assert_eq!(input.source, "app");
        assert_eq!(input.urgency, 1);
    }

    #[test]
    fn action_pairs_preserve_order_default_and_ignore_dangling_value() {
        let actions = parse_actions(vec![
            "default".to_owned(),
            "Open".to_owned(),
            "ack".to_owned(),
            "Acknowledge".to_owned(),
            "dangling".to_owned(),
        ]);
        assert_eq!(actions[0].key, "default");
        assert_eq!(actions[0].label, "Open");
        assert_eq!(actions[1].key, "ack");
        assert_eq!(actions[1].label, "Acknowledge");
    }

    #[test]
    fn active_actions_validate_close_and_replacement_state() {
        let state = NotificationState::new(AttentionState::new());
        let mut notification = input("app", 0);
        notification.actions = parse_actions(vec!["ack".to_owned(), "Acknowledge".to_owned()]);
        let (id, old_event) = state.notify(notification).unwrap();
        assert!(state.invoke(id, "ack").is_ok());
        assert!(matches!(
            state.invoke(id, "missing"),
            Err(NotificationError::UnknownAction)
        ));
        let mut replacement = input("app", id);
        replacement.actions = parse_actions(vec!["default".to_owned(), "Open".to_owned()]);
        let (_, new_event) = state.notify(replacement).unwrap();
        assert!(state.active_for_event(old_event.id).is_none());
        assert!(state.active_for_event(new_event.id).is_some());
        assert!(matches!(
            state.invoke(id, "ack"),
            Err(NotificationError::UnknownAction)
        ));
        state.close(id).unwrap();
        assert!(matches!(
            state.invoke(id, "default"),
            Err(NotificationError::UnknownId)
        ));
    }

    #[test]
    fn capabilities_truthfully_include_actions() {
        assert_eq!(
            NotificationService::new(NotificationState::new(AttentionState::new()))
                .get_capabilities(),
            vec!["body", "actions"]
        );
    }
}
