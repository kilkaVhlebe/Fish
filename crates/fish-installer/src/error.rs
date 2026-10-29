//! Installer errors.

/// Everything that can stop an install, repair, update or uninstall.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No usable `horizon-launcher` was found.
    #[error("no working horizon-launcher was found ({0}); pass --launcher <path>")]
    LauncherNotFound(String),
    /// A launcher command failed.
    #[error("horizon-launcher {command} failed: {message}")]
    Launcher {
        /// The command line (without the program).
        command: String,
        /// What the launcher said.
        message: String,
    },
    /// The launcher printed something unexpected.
    #[error("unexpected output from horizon-launcher {command}: {message}")]
    LauncherOutput {
        /// The command line.
        command: String,
        /// What was wrong.
        message: String,
    },
    /// The instance exists and is set up for something else.
    #[error(
        "instance \"{instance}\" already runs through another wrapper ({wrapper}); pass --force to replace it, or choose another --instance"
    )]
    ForeignInstance {
        /// The instance name.
        instance: String,
        /// Its wrapper.
        wrapper: String,
    },
    /// No edition source could be used.
    #[error("{0}")]
    Source(String),
    /// The edition's signature is missing or wrong.
    #[error("{0}")]
    Signature(String),
    /// A download or an archive is damaged.
    #[error("{0}")]
    Integrity(String),
    /// No edition fits.
    #[error("{0}")]
    NoEdition(String),
    /// Something is not installed.
    #[error("{0}")]
    NotInstalled(String),
    /// A file operation failed.
    #[error("{context}: {source}")]
    Io {
        /// What was being done.
        context: String,
        /// The error.
        source: std::io::Error,
    },
}

impl Error {
    pub(crate) fn io(context: impl Into<String>, source: std::io::Error) -> Error {
        Error::Io {
            context: context.into(),
            source,
        }
    }
}

/// Result alias.
pub type Result<T, E = Error> = std::result::Result<T, E>;
