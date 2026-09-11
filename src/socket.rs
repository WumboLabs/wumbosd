// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::{
    env, io,
    os::{
        fd::{FromRawFd, RawFd},
        unix::fs::{FileTypeExt, PermissionsExt},
    },
    path::PathBuf,
};

use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    sync::broadcast,
    task::JoinHandle,
};

use crate::{
    attention::{AttentionEvent, AttentionState, AttentionUpdate},
    notification::{
        ActiveNotification, NOTIFICATION_INTERFACE_NAME, NOTIFICATION_OBJECT_PATH,
        NotificationState,
    },
    service::{API_VERSION, FoundationState, PACKAGE_VERSION, SERVICE_STATE},
};

#[cfg(test)]
use crate::attention::EVENT_STORE_CAPACITY;

pub const PROTOCOL_VERSION: u32 = 1;
const MAX_FRAME_BYTES: usize = 8 * 1024;
const SOCKET_DIRECTORY_NAME: &str = "wumbos";
const SOCKET_FILE_NAME: &str = "wumbosd.sock";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ListenerOwnership {
    Standalone,
    Systemd,
}

pub struct SocketServer {
    path: PathBuf,
    ownership: ListenerOwnership,
    task: JoinHandle<()>,
}

impl SocketServer {
    pub async fn start(
        state: FoundationState,
        attention: AttentionState,
        notifications: NotificationState,
        connection: zbus::Connection,
    ) -> io::Result<Self> {
        let path = socket_path()?;
        let (listener, ownership) = match inherited_listener()? {
            Some(listener) => (listener, ListenerOwnership::Systemd),
            None => {
                prepare_socket_directory(&path).await?;
                remove_stale_socket(&path).await?;

                let listener = UnixListener::bind(&path)?;
                tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).await?;
                (listener, ListenerOwnership::Standalone)
            }
        };

        let task = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let client_state = state.clone();
                        let client_attention = attention.clone();
                        let client_notifications = notifications.clone();
                        let client_connection = connection.clone();
                        let events = attention.subscribe();
                        tokio::spawn(async move {
                            let _ = serve_client(
                                stream,
                                client_state,
                                client_attention,
                                client_notifications,
                                client_connection,
                                events,
                            )
                            .await;
                        });
                    }
                    Err(error) => {
                        eprintln!("wumbosd: socket accept failed: {error}");
                        break;
                    }
                }
            }
        });

        Ok(Self {
            path,
            ownership,
            task,
        })
    }

    pub async fn shutdown(self) -> io::Result<()> {
        self.task.abort();
        let _ = self.task.await;
        if should_remove_socket(self.ownership) {
            remove_socket_if_present(&self.path).await?;
        }
        Ok(())
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    pub fn is_systemd_activated(&self) -> bool {
        self.ownership == ListenerOwnership::Systemd
    }
}

fn inherited_listener() -> io::Result<Option<UnixListener>> {
    let listen_pid = env::var("LISTEN_PID").ok();
    let listen_fds = env::var("LISTEN_FDS").ok();
    let Some((pid, fds)) = activation_environment(listen_pid.as_deref(), listen_fds.as_deref())?
    else {
        return Ok(None);
    };

    if pid != std::process::id() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "LISTEN_PID does not match wumbosd",
        ));
    }
    if fds != 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "wumbosd requires exactly one inherited listener",
        ));
    }

    listener_from_fd(3).map(Some)
}

fn activation_environment(
    listen_pid: Option<&str>,
    listen_fds: Option<&str>,
) -> io::Result<Option<(u32, u32)>> {
    match (listen_pid, listen_fds) {
        (None, None) => Ok(None),
        (Some(pid), Some(fds)) => {
            let pid = pid.parse().map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "LISTEN_PID is not an unsigned integer",
                )
            })?;
            let fds = fds.parse().map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "LISTEN_FDS is not an unsigned integer",
                )
            })?;
            Ok(Some((pid, fds)))
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "incomplete systemd socket activation environment",
        )),
    }
}

fn listener_from_fd(fd: RawFd) -> io::Result<UnixListener> {
    let listener = unsafe { std::os::unix::net::UnixListener::from_raw_fd(fd) };
    listener.set_nonblocking(true)?;
    UnixListener::from_std(listener)
}

fn should_remove_socket(ownership: ListenerOwnership) -> bool {
    ownership == ListenerOwnership::Standalone
}

pub fn socket_path() -> io::Result<PathBuf> {
    let runtime_directory = env::var_os("XDG_RUNTIME_DIR").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "XDG_RUNTIME_DIR is required for the wumbosd socket",
        )
    })?;

    Ok(PathBuf::from(runtime_directory)
        .join(SOCKET_DIRECTORY_NAME)
        .join(SOCKET_FILE_NAME))
}

async fn prepare_socket_directory(socket_path: &std::path::Path) -> io::Result<()> {
    let directory = socket_path.parent().expect("socket path has a parent");
    match tokio::fs::symlink_metadata(directory).await {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "wumbosd socket directory is not a real directory",
                ));
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            tokio::fs::create_dir(directory).await?;
        }
        Err(error) => return Err(error),
    }

    tokio::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700)).await
}

async fn remove_stale_socket(path: &std::path::Path) -> io::Result<()> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) if metadata.file_type().is_socket() => tokio::fs::remove_file(path).await,
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "refusing to replace non-socket at wumbosd socket path",
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

async fn remove_socket_if_present(path: &std::path::Path) -> io::Result<()> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) if metadata.file_type().is_socket() => tokio::fs::remove_file(path).await,
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "refusing to remove non-socket at wumbosd socket path",
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[derive(Serialize)]
struct HelloFrame<'a> {
    protocol: u32,
    #[serde(rename = "type")]
    frame_type: &'static str,
    api_version: u32,
    package_version: &'a str,
    status: &'a str,
    uptime_ms: u64,
}

#[derive(Serialize)]
struct PongFrame {
    protocol: u32,
    #[serde(rename = "type")]
    frame_type: &'static str,
    uptime_ms: u64,
}

#[derive(Serialize)]
struct AttentionEventFrame<'a> {
    protocol: u32,
    #[serde(rename = "type")]
    frame_type: &'static str,
    event: &'a AttentionEvent,
    #[serde(skip_serializing_if = "Option::is_none")]
    notification: Option<NotificationFrame<'a>>,
}
#[derive(Serialize)]
struct NotificationActionFrame<'a> {
    key: &'a str,
    label: &'a str,
}

#[derive(Serialize)]
struct NotificationFrame<'a> {
    id: u32,
    actions: Vec<NotificationActionFrame<'a>>,
}

#[derive(Serialize)]
struct AttentionActionResultFrame {
    protocol: u32,
    #[serde(rename = "type")]
    frame_type: &'static str,
    notification_id: u32,
    action_key: String,
    accepted: bool,
}

#[derive(Serialize)]
struct AttentionRecentFrame<'a> {
    protocol: u32,
    #[serde(rename = "type")]
    frame_type: &'static str,
    events: Vec<AttentionEvent>,
    notifications: std::collections::HashMap<u64, NotificationFrame<'a>>,
}
#[derive(Serialize)]
struct AttentionDismissResultFrame {
    protocol: u32,
    #[serde(rename = "type")]
    frame_type: &'static str,
    id: u64,
    removed: bool,
}

#[derive(Serialize)]
struct AttentionClearResultFrame {
    protocol: u32,
    #[serde(rename = "type")]
    frame_type: &'static str,
    removed: u32,
}

#[derive(Serialize)]
struct AttentionRemovedFrame {
    protocol: u32,
    #[serde(rename = "type")]
    frame_type: &'static str,
    id: u64,
}

#[derive(Serialize)]
struct AttentionClearedFrame {
    protocol: u32,
    #[serde(rename = "type")]
    frame_type: &'static str,
}

#[derive(Deserialize)]
struct ClientFrame {
    protocol: u32,
    #[serde(rename = "type")]
    frame_type: String,
    limit: Option<u32>,
    id: Option<u64>,
    notification_id: Option<u32>,
    action_key: Option<String>,
}

fn notification_frame(notification: &ActiveNotification) -> NotificationFrame<'_> {
    NotificationFrame {
        id: notification.id,
        actions: notification
            .actions
            .iter()
            .map(|action| NotificationActionFrame {
                key: &action.key,
                label: &action.label,
            })
            .collect(),
    }
}

async fn serve_client(
    stream: UnixStream,
    state: FoundationState,
    attention: AttentionState,
    notifications: NotificationState,
    connection: zbus::Connection,
    mut events: broadcast::Receiver<AttentionUpdate>,
) -> io::Result<()> {
    eprintln!("wumbosd: socket client connected");
    let (mut reader, mut writer) = stream.into_split();

    write_frame(
        &mut writer,
        &HelloFrame {
            protocol: PROTOCOL_VERSION,
            frame_type: "hello",
            api_version: API_VERSION,
            package_version: PACKAGE_VERSION,
            status: SERVICE_STATE,
            uptime_ms: state.elapsed_milliseconds(),
        },
    )
    .await?;

    let mut pending = Vec::with_capacity(MAX_FRAME_BYTES);
    let mut read_buffer = [0; 1024];
    loop {
        tokio::select! {
            read = reader.read(&mut read_buffer) => {
                let read = read?;
                if read == 0 {
                    return Ok(());
                }

                pending.extend_from_slice(&read_buffer[..read]);
                while let Some(newline) = pending.iter().position(|byte| *byte == b'\n') {
                    if newline > MAX_FRAME_BYTES {
                        return Ok(());
                    }

                    let frame = pending.drain(..=newline).collect::<Vec<_>>();
                    let payload = &frame[..frame.len() - 1];
                    let Ok(frame) = serde_json::from_slice::<ClientFrame>(payload) else {
                        return Ok(());
                    };
                    if frame.protocol != PROTOCOL_VERSION {
                        return Ok(());
                    }
                    match frame.frame_type.as_str() {
                        "ping" => {
                            eprintln!("wumbosd: socket ping received");
                            write_frame(
                                &mut writer,
                                &PongFrame {
                                    protocol: PROTOCOL_VERSION,
                                    frame_type: "pong",
                                    uptime_ms: state.elapsed_milliseconds(),
                                },
                            )
                            .await?;
                        }
                        "attention_recent_request" => {
                            let Some(limit) = frame.limit else {
                                return Ok(());
                            };
                            let events = attention.recent(limit);
                            let active: Vec<_> = events
                                .iter()
                                .filter_map(|event| notifications.active_for_event(event.id))
                                .collect();
                            let notification_map = active
                                .iter()
                                .map(|notification| {
                                    (notification.attention_event_id, notification_frame(notification))
                                })
                                .collect();
                            write_frame(
                                &mut writer,
                                &AttentionRecentFrame {
                                    protocol: PROTOCOL_VERSION,
                                    frame_type: "attention_recent",
                                    events,
                                    notifications: notification_map,
                                },
                            )
                            .await?;
                        }
                        "attention_dismiss" => {
                            let Some(id) = frame.id else {
                                return Ok(());
                            };
                            let removed = attention.dismiss(id);
                            write_frame(
                                &mut writer,
                                &AttentionDismissResultFrame {
                                    protocol: PROTOCOL_VERSION,
                                    frame_type: "attention_dismiss_result",
                                    id,
                                    removed,
                                },
                            )
                            .await?;
                        }
                        "attention_clear" => {
                            let removed = attention.clear();
                            write_frame(
                                &mut writer,
                                &AttentionClearResultFrame {
                                    protocol: PROTOCOL_VERSION,
                                    frame_type: "attention_clear_result",
                                    removed,
                                },
                            )
                            .await?;
                        }
                        "attention_action" => {
                            let (Some(notification_id), Some(action_key)) =
                                (frame.notification_id, frame.action_key)
                            else {
                                return Ok(());
                            };
                            let accepted = notifications.invoke(notification_id, &action_key).is_ok();
                            if accepted {
                                let emitter = zbus::object_server::SignalEmitter::new(
                                    &connection,
                                    NOTIFICATION_OBJECT_PATH,
                                )
                                .map_err(io::Error::other)?;
                                emitter
                                    .emit(
                                        NOTIFICATION_INTERFACE_NAME,
                                        "ActionInvoked",
                                        &(notification_id, action_key.as_str()),
                                    )
                                    .await
                                    .map_err(io::Error::other)?;
                            }
                            write_frame(
                                &mut writer,
                                &AttentionActionResultFrame {
                                    protocol: PROTOCOL_VERSION,
                                    frame_type: "attention_action_result",
                                    notification_id,
                                    action_key,
                                    accepted,
                                },
                            )
                            .await?;
                        }
                        _ => return Ok(()),
                    }
                }
                if pending.len() > MAX_FRAME_BYTES {
                    return Ok(());
                }
            }
            event = events.recv() => {
                let update = match event {
                    Ok(update) => update,
                    Err(broadcast::error::RecvError::Lagged(_))
                    | Err(broadcast::error::RecvError::Closed) => return Ok(()),
                };
                match update {
                    AttentionUpdate::Added(event) => {
                        let active = notifications.active_for_event(event.id);
                        write_frame(
                            &mut writer,
                            &AttentionEventFrame {
                                protocol: PROTOCOL_VERSION,
                                frame_type: "attention_event",
                                event: &event,
                                notification: active.as_ref().map(notification_frame),
                            },
                        )
                        .await?;
                    }
                    AttentionUpdate::Removed(id) => {
                        write_frame(
                            &mut writer,
                            &AttentionRemovedFrame {
                                protocol: PROTOCOL_VERSION,
                                frame_type: "attention_removed",
                                id,
                            },
                        )
                        .await?;
                    }
                    AttentionUpdate::Cleared => {
                        write_frame(
                            &mut writer,
                            &AttentionClearedFrame {
                                protocol: PROTOCOL_VERSION,
                                frame_type: "attention_cleared",
                            },
                        )
                        .await?;
                    }
                }
            }
        }
    }
}

async fn write_frame<W: AsyncWrite + Unpin, T: Serialize>(
    stream: &mut W,
    frame: &T,
) -> io::Result<()> {
    let mut payload = serde_json::to_vec(frame).map_err(io::Error::other)?;
    payload.push(b'\n');
    stream.write_all(&payload).await?;
    stream.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use tokio::io::{AsyncBufReadExt, BufReader};

    async fn connected_pair() -> (
        BufReader<tokio::net::unix::OwnedReadHalf>,
        tokio::net::unix::OwnedWriteHalf,
        AttentionState,
        JoinHandle<io::Result<()>>,
    ) {
        let (client, server) = UnixStream::pair().unwrap();
        let attention = AttentionState::new();
        let notifications = NotificationState::new(attention.clone());
        let connection = zbus::Connection::session().await.unwrap();
        let task = tokio::spawn(serve_client(
            server,
            FoundationState::new(),
            attention.clone(),
            notifications,
            connection,
            attention.subscribe(),
        ));
        let (reader, writer) = client.into_split();
        (BufReader::new(reader), writer, attention, task)
    }

    #[tokio::test]
    async fn hello_and_multiple_pings_are_framed() {
        let (mut reader, mut writer, _attention, task) = connected_pair().await;
        let mut hello = String::new();
        reader.read_line(&mut hello).await.unwrap();
        let hello: Value = serde_json::from_str(&hello).unwrap();
        assert_eq!(hello["protocol"], PROTOCOL_VERSION);
        assert_eq!(hello["type"], "hello");
        assert_eq!(hello["api_version"], API_VERSION);

        writer
            .write_all(b"{\"protocol\":1,\"type\":\"ping\"}\n{\"protocol\":1,\"type\":\"ping\"}\n")
            .await
            .unwrap();
        for _ in 0..2 {
            let mut pong = String::new();
            reader.read_line(&mut pong).await.unwrap();
            let pong: Value = serde_json::from_str(&pong).unwrap();
            assert_eq!(pong["type"], "pong");
        }
        drop(writer);
        assert!(task.await.unwrap().is_ok());
    }

    #[tokio::test]
    async fn published_event_is_serialized_as_live_attention_frame() {
        let (mut reader, writer, attention, task) = connected_pair().await;
        let mut hello = String::new();
        reader.read_line(&mut hello).await.unwrap();

        let event = attention
            .publish(
                "validation".to_owned(),
                "message".to_owned(),
                "Attention event test".to_owned(),
                "Synthetic validation event".to_owned(),
                1,
            )
            .unwrap();
        let mut frame = String::new();
        reader.read_line(&mut frame).await.unwrap();
        let frame: Value = serde_json::from_str(&frame).unwrap();
        assert_eq!(frame["protocol"], PROTOCOL_VERSION);
        assert_eq!(frame["type"], "attention_event");
        assert_eq!(frame["event"]["id"], event.id);
        assert_eq!(frame["event"]["created_at_ms"], event.created_at_ms);
        assert_eq!(frame["event"]["source"], event.source);
        assert_eq!(frame["event"]["kind"], event.kind);
        assert_eq!(frame["event"]["title"], event.title);
        assert_eq!(frame["event"]["body"], event.body);
        assert_eq!(frame["event"]["urgency"], event.urgency);
        drop(writer);
        assert!(task.await.unwrap().is_ok());
    }

    #[tokio::test]
    async fn recent_request_returns_newest_first_attention_events() {
        let (mut reader, mut writer, attention, task) = connected_pair().await;
        let mut hello = String::new();
        reader.read_line(&mut hello).await.unwrap();

        let first = attention
            .publish(
                "validation".to_owned(),
                "message".to_owned(),
                "First attention".to_owned(),
                "First synthetic event".to_owned(),
                0,
            )
            .unwrap();
        let second = attention
            .publish(
                "validation".to_owned(),
                "warning".to_owned(),
                "Second attention".to_owned(),
                "Second synthetic event".to_owned(),
                2,
            )
            .unwrap();
        for _ in 0..2 {
            let mut live_frame = String::new();
            reader.read_line(&mut live_frame).await.unwrap();
        }

        writer
            .write_all(b"{\"protocol\":1,\"type\":\"attention_recent_request\",\"limit\":32}\n")
            .await
            .unwrap();
        let mut response = String::new();
        reader.read_line(&mut response).await.unwrap();
        let response: Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response["protocol"], PROTOCOL_VERSION);
        assert_eq!(response["type"], "attention_recent");
        assert_eq!(
            response["events"],
            serde_json::to_value([second, first]).unwrap()
        );
        drop(writer);
        assert!(task.await.unwrap().is_ok());
    }

    #[tokio::test]
    async fn recent_request_honors_zero_and_caps_limit_at_store_capacity() {
        let (mut reader, mut writer, attention, task) = connected_pair().await;
        let mut hello = String::new();
        reader.read_line(&mut hello).await.unwrap();
        for index in 0..=EVENT_STORE_CAPACITY {
            attention
                .publish(
                    "validation".to_owned(),
                    "message".to_owned(),
                    format!("Attention {index}"),
                    "Synthetic event".to_owned(),
                    1,
                )
                .unwrap();
            let mut live_frame = String::new();
            reader.read_line(&mut live_frame).await.unwrap();
        }

        writer
            .write_all(b"{\"protocol\":1,\"type\":\"attention_recent_request\",\"limit\":0}\n{\"protocol\":1,\"type\":\"attention_recent_request\",\"limit\":999}\n")
            .await
            .unwrap();
        let mut zero_response = String::new();
        reader.read_line(&mut zero_response).await.unwrap();
        let zero_response: Value = serde_json::from_str(&zero_response).unwrap();
        assert_eq!(zero_response["events"], serde_json::json!([]));
        let mut capped_response = String::new();
        reader.read_line(&mut capped_response).await.unwrap();
        let capped_response: Value = serde_json::from_str(&capped_response).unwrap();
        let events = capped_response["events"].as_array().unwrap();
        assert_eq!(events.len(), EVENT_STORE_CAPACITY);
        assert_eq!(
            events.first().unwrap()["id"],
            EVENT_STORE_CAPACITY as u64 + 1
        );
        assert_eq!(events.last().unwrap()["id"], 2);
        drop(writer);
        assert!(task.await.unwrap().is_ok());
    }

    async fn assert_client_is_rejected(payload: &[u8]) {
        let (mut reader, mut writer, _attention, task) = connected_pair().await;
        let mut hello = String::new();
        reader.read_line(&mut hello).await.unwrap();
        writer.write_all(payload).await.unwrap();
        drop(writer);
        assert!(task.await.unwrap().is_ok());
    }

    #[tokio::test]
    async fn malformed_and_oversized_frames_close_only_client() {
        assert_client_is_rejected(b"{not json}\n").await;
        assert_client_is_rejected(b"{\"protocol\":1,\"type\":\"attention_recent_request\"}\n")
            .await;
        let oversized = vec![b'x'; MAX_FRAME_BYTES + 1];
        assert_client_is_rejected(&oversized).await;
    }

    #[test]
    fn activation_environment_requires_complete_numeric_values() {
        assert_eq!(activation_environment(None, None).unwrap(), None);
        assert_eq!(
            activation_environment(Some("42"), Some("1")).unwrap(),
            Some((42, 1))
        );
        assert!(activation_environment(Some("42"), None).is_err());
        assert!(activation_environment(Some("x"), Some("1")).is_err());
    }

    #[tokio::test]
    async fn inherited_listener_is_adopted_nonblocking() {
        use std::os::fd::{AsRawFd, IntoRawFd};

        let path = std::env::temp_dir().join(format!(
            "wumbosd-listener-adoption-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        assert!(listener.as_raw_fd() >= 0);
        let listener = listener_from_fd(listener.into_raw_fd()).unwrap();

        assert_eq!(
            listener.local_addr().unwrap().as_pathname(),
            Some(path.as_path())
        );
        drop(listener);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn only_standalone_mode_removes_the_socket() {
        assert!(should_remove_socket(ListenerOwnership::Standalone));
        assert!(!should_remove_socket(ListenerOwnership::Systemd));
    }
}
