mod error;
mod service;

use std::process::ExitCode;

use crate::{
    error::ServiceError,
    service::{BUS_NAME, FoundationService, INTERFACE_NAME, OBJECT_PATH, PACKAGE_VERSION},
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

    let connection = Connection::session()
        .await
        .map_err(ServiceError::SessionBusConnection)?;

    connection
        .request_name_with_flags(BUS_NAME, zbus::fdo::RequestNameFlags::DoNotQueue.into())
        .await
        .map_err(ServiceError::NameAcquisition)?;

    connection
        .object_server()
        .at(OBJECT_PATH, FoundationService::new())
        .await
        .map_err(ServiceError::ObjectRegistration)?;

    eprintln!("wumbosd: available on {BUS_NAME}{OBJECT_PATH} ({INTERFACE_NAME})");

    let mut sigterm = signal(SignalKind::terminate()).map_err(ServiceError::ServiceLoop)?;
    tokio::select! {
        result = tokio::signal::ctrl_c() => result.map_err(ServiceError::ServiceLoop)?,
        _ = sigterm.recv() => {}
    }

    eprintln!("wumbosd: clean shutdown");
    drop(connection);
    Ok(())
}
