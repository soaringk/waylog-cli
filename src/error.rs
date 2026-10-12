use thiserror::Error;

#[derive(Error, Debug)]
pub enum WaylogError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON parsing error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Provider not found: {0}")]
    ProviderNotFound(String),

    #[error("Session not found for {provider}: {session_id}")]
    SessionNotFound {
        provider: String,
        session_id: String,
    },

    #[error("All {0} session sync tasks failed")]
    AllSessionsFailed(usize),

    #[error("Path error: {0}")]
    PathError(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl WaylogError {
    /// Get the exit code for this error
    pub fn exit_code(&self) -> i32 {
        match self {
            // Command line usage errors
            WaylogError::ProviderNotFound(_) => exitcode::USAGE,
            // Data format errors
            WaylogError::Json(_) => exitcode::DATAERR,
            // Input file/resource errors
            WaylogError::SessionNotFound { .. } | WaylogError::Io(_) => exitcode::NOINPUT,
            // Internal software errors
            WaylogError::AllSessionsFailed(_)
            | WaylogError::PathError(_)
            | WaylogError::Internal(_) => exitcode::SOFTWARE,
        }
    }

    /// Provider validation reports its error before initialization.
    pub fn is_already_displayed(&self) -> bool {
        matches!(self, WaylogError::ProviderNotFound(_))
    }
}

pub type Result<T> = std::result::Result<T, WaylogError>;
