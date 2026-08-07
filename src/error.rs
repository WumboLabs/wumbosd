use std::{error::Error, fmt, io};

#[derive(Debug)]
pub enum ServiceError {
    SessionBusConnection(zbus::Error),
    NameAcquisition(zbus::Error),
    ObjectRegistration(zbus::Error),
    ServiceLoop(io::Error),
}

impl fmt::Display for ServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SessionBusConnection(error) => {
                write!(formatter, "could not connect to the session D-Bus: {error}")
            }
            Self::NameAcquisition(error) => {
                write!(
                    formatter,
                    "could not acquire D-Bus name org.wumbos.wumbosd: {error}"
                )
            }
            Self::ObjectRegistration(error) => {
                write!(
                    formatter,
                    "could not register the Foundation v1 D-Bus object: {error}"
                )
            }
            Self::ServiceLoop(error) => write!(formatter, "service event loop failed: {error}"),
        }
    }
}

impl Error for ServiceError {}
