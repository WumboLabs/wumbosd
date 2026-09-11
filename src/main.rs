// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

mod attention;
mod notification;

mod error;
mod service;
mod socket;

use std::process::ExitCode;

use crate::{
    attention::{
        ATTENTION_INTERFACE_NAME, ATTENTION_OBJECT_PATH, AttentionService, AttentionState,
    },
    error::ServiceError,
    notification::{
        NOTIFICATION_BUS_NAME, NOTIFICATION_INTERFACE_NAME, NOTIFICATION_OBJECT_PATH,
        NotificationService, NotificationState,
    },
    service::{
        BUS_NAME, FoundationService, FoundationState, INTERFACE_NAME, OBJECT_PATH, PACKAGE_VERSION,
    },
    socket::SocketServer,
};
use tokio::signal::unix::{SignalKind, signal};
use zbus::Connection;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("wumbosd: fatal startup failure: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), ServiceError> {
    eprintln!("wumbosd {PACKAGE_VERSION} starting");

    let foundation_state = FoundationState::new();
    let attention_state = AttentionState::new();

    let notification_state = NotificationState::new(attention_state.clone());

    let notification_enabled =
        std::env::var_os("WUMBOSD_NOTIFICATION_SERVER").is_some_and(|value| value == "1");

    let connection = Connection::session()
        .await
        .map_err(ServiceError::SessionBusConnection)?;

    connection
        .request_name_with_flags(BUS_NAME, zbus::fdo::RequestNameFlags::DoNotQueue.into())
        .await
        .map_err(ServiceError::NameAcquisition)?;

    connection
        .object_server()
        .at(
            OBJECT_PATH,
            FoundationService::new(foundation_state.clone()),
        )
        .await
        .map_err(ServiceError::ObjectRegistration)?;

    connection
        .object_server()
        .at(
            ATTENTION_OBJECT_PATH,
            AttentionService::new(attention_state.clone()),
        )
        .await
        .map_err(ServiceError::ObjectRegistration)?;

    if notification_enabled {
        connection
            .request_name_with_flags(
                NOTIFICATION_BUS_NAME,
                zbus::fdo::RequestNameFlags::DoNotQueue.into(),
            )
            .await
            .map_err(ServiceError::NameAcquisition)?;
        connection
            .object_server()
            .at(
                NOTIFICATION_OBJECT_PATH,
                NotificationService::new(notification_state.clone()),
            )
            .await
            .map_err(ServiceError::ObjectRegistration)?;
        eprintln!(
            "wumbosd: notification server available at {NOTIFICATION_OBJECT_PATH} ({NOTIFICATION_INTERFACE_NAME})"
        );
    }

    eprintln!("wumbosd: available on {BUS_NAME}{OBJECT_PATH} ({INTERFACE_NAME})");
    eprintln!(
        "wumbosd: attention available at {ATTENTION_OBJECT_PATH} ({ATTENTION_INTERFACE_NAME})"
    );

    let socket_server = SocketServer::start(
        foundation_state,
        attention_state,
        notification_state,
        connection.clone(),
    )
    .await
    .map_err(ServiceError::SocketStartup)?;
    let listener_owner = if socket_server.is_systemd_activated() {
        "systemd socket activation"
    } else {
        "standalone binding"
    };
    eprintln!(
        "wumbosd: socket available at {} ({listener_owner})",
        socket_server.path().display()
    );

    let mut sigterm = signal(SignalKind::terminate()).map_err(ServiceError::ServiceLoop)?;
    tokio::select! {
        result = tokio::signal::ctrl_c() => result.map_err(ServiceError::ServiceLoop)?,
        _ = sigterm.recv() => {}
    }

    eprintln!("wumbosd: clean shutdown");
    drop(connection);
    socket_server
        .shutdown()
        .await
        .map_err(ServiceError::ServiceLoop)?;
    Ok(())
}
