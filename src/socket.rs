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
    attention::{AttentionEvent, AttentionState},
    service::{API_VERSION, FoundationState, PACKAGE_VERSION, SERVICE_STATE},
};

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
    pub async fn start(state: FoundationState, attention: AttentionState) -> io::Result<Self> {
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
                        let events = attention.subscribe();
                        tokio::spawn(async move {
                            let _ = serve_client(stream, client_state, events).await;
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
}

#[derive(Deserialize)]
struct ClientFrame {
    protocol: u32,
    #[serde(rename = "type")]
    frame_type: String,
}

async fn serve_client(
    stream: UnixStream,
    state: FoundationState,
    mut events: broadcast::Receiver<AttentionEvent>,
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
                    if frame.protocol != PROTOCOL_VERSION || frame.frame_type != "ping" {
                        return Ok(());
                    }
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
                if pending.len() > MAX_FRAME_BYTES {
                    return Ok(());
                }
            }
            event = events.recv() => {
                let event = match event {
                    Ok(event) => event,
                    Err(broadcast::error::RecvError::Lagged(_))
                    | Err(broadcast::error::RecvError::Closed) => return Ok(()),
                };
                write_frame(
                    &mut writer,
                    &AttentionEventFrame {
                        protocol: PROTOCOL_VERSION,
                        frame_type: "attention_event",
                        event: &event,
                    },
                )
                .await?;
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
        let task = tokio::spawn(serve_client(
            server,
            FoundationState::new(),
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
