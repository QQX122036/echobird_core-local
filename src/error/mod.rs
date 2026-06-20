//! Crate-wide error type. All public functions return [`Result<T>`];
//! the type is intentionally rich (not `anyhow`) so the command
//! layer can pattern-match on a specific failure mode when it needs
//! to surface a different IPC response shape to the frontend.
//!
//! IPC contract: the frontend `src/api/tauri.ts` invokes commands and
//! catches thrown errors. Tauri serializes the error message back as
//! a JS string, and callers do `if (err instanceof Error) ...`. So
//! our `Display` impl carries the user-facing summary, and the
//! `Code` enum tag travels in the prefix as a machine-readable key
//! the UI can grep for.
//!
//! Error code mapping table (stable, frontend-safe):
//!
//! | `Code`           | JS-readable prefix     | Typical user remediation
//! |------------------|------------------------|--------------------------------------
//! | `NotFound`       | `not_found:`           | "model was deleted", re-add it
//! | `Validation`     | `validation:`          | "missing required field X"
//! | `Conflict`       | `conflict:`            | "internalId already in use"
//! | `Upstream`       | `upstream:`            | "upstream returned 4xx/5xx"
//! | `Network`        | `network:`             | "no DNS / TCP refused"
//! | `Unauthorized`   | `unauthorized:`        | "API key rejected"
//! | `RateLimited`    | `rate_limited:`        | "back off, retry in N seconds"
//! | `Timeout`        | `timeout:`             | "request exceeded deadline"
//! | `Storage`        | `storage:`             | "disk full / permission denied"
//! | `NotImplemented` | `not_implemented:`     | "this feature is on the roadmap"
//! | `Internal`       | `internal:`            | "unexpected; see logs"
//!
//! The frontend is already coded defensively: errors that don't match
//! any of these prefixes are shown verbatim to the user. So the
//! prefix is the contract.

use std::fmt;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Code {
    NotFound,
    Validation,
    Conflict,
    Upstream,
    Network,
    Unauthorized,
    RateLimited,
    Timeout,
    Storage,
    NotImplemented,
    Internal,
}

impl Code {
    /// Stable prefix used in the formatted error. Keep in sync with
    /// the table in the module doc comment.
    pub fn as_prefix(self) -> &'static str {
        match self {
            Code::NotFound => "not_found",
            Code::Validation => "validation",
            Code::Conflict => "conflict",
            Code::Upstream => "upstream",
            Code::Network => "network",
            Code::Unauthorized => "unauthorized",
            Code::RateLimited => "rate_limited",
            Code::Timeout => "timeout",
            Code::Storage => "storage",
            Code::NotImplemented => "not_implemented",
            Code::Internal => "internal",
        }
    }
}

#[derive(Debug, thiserror::Error, serde::Serialize)]
#[serde(tag = "code", content = "message")]
pub enum Error {
    #[error("{prefix}: {message}", prefix = Code::NotFound.as_prefix())]
    NotFound { message: String },

    #[error("{prefix}: {message}", prefix = Code::Validation.as_prefix())]
    Validation { message: String },

    #[error("{prefix}: {message}", prefix = Code::Conflict.as_prefix())]
    Conflict { message: String },

    #[error("{prefix}: {message}", prefix = Code::Upstream.as_prefix())]
    Upstream { message: String },

    #[error("{prefix}: {message}", prefix = Code::Network.as_prefix())]
    Network { message: String },

    #[error("{prefix}: {message}", prefix = Code::Unauthorized.as_prefix())]
    Unauthorized { message: String },

    #[error("{prefix}: {message}", prefix = Code::RateLimited.as_prefix())]
    RateLimited { message: String },

    #[error("{prefix}: {message}", prefix = Code::Timeout.as_prefix())]
    Timeout { message: String },

    #[error("{prefix}: {message}", prefix = Code::Storage.as_prefix())]
    Storage { message: String },

    #[error("{prefix}: {message}", prefix = Code::NotImplemented.as_prefix())]
    NotImplemented { message: String },

    #[error("{prefix}: {message}", prefix = Code::Internal.as_prefix())]
    Internal { message: String },
}

impl Error {
    pub fn code(&self) -> Code {
        match self {
            Error::NotFound { .. } => Code::NotFound,
            Error::Validation { .. } => Code::Validation,
            Error::Conflict { .. } => Code::Conflict,
            Error::Upstream { .. } => Code::Upstream,
            Error::Network { .. } => Code::Network,
            Error::Unauthorized { .. } => Code::Unauthorized,
            Error::RateLimited { .. } => Code::RateLimited,
            Error::Timeout { .. } => Code::Timeout,
            Error::Storage { .. } => Code::Storage,
            Error::NotImplemented { .. } => Code::NotImplemented,
            Error::Internal { .. } => Code::Internal,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Error::NotFound { message }
            | Error::Validation { message }
            | Error::Conflict { message }
            | Error::Upstream { message }
            | Error::Network { message }
            | Error::Unauthorized { message }
            | Error::RateLimited { message }
            | Error::Timeout { message }
            | Error::Storage { message }
            | Error::NotImplemented { message }
            | Error::Internal { message } => message,
        }
    }

    // ─── Ergonomic constructors ─────────────────────────────────
    pub fn not_found<S: Into<String>>(m: S) -> Self {
        Error::NotFound { message: m.into() }
    }
    pub fn validation<S: Into<String>>(m: S) -> Self {
        Error::Validation { message: m.into() }
    }
    pub fn conflict<S: Into<String>>(m: S) -> Self {
        Error::Conflict { message: m.into() }
    }
    pub fn upstream<S: Into<String>>(m: S) -> Self {
        Error::Upstream { message: m.into() }
    }
    pub fn network<S: Into<String>>(m: S) -> Self {
        Error::Network { message: m.into() }
    }
    pub fn unauthorized<S: Into<String>>(m: S) -> Self {
        Error::Unauthorized { message: m.into() }
    }
    pub fn rate_limited<S: Into<String>>(m: S) -> Self {
        Error::RateLimited { message: m.into() }
    }
    pub fn timeout<S: Into<String>>(m: S) -> Self {
        Error::Timeout { message: m.into() }
    }
    pub fn storage<S: Into<String>>(m: S) -> Self {
        Error::Storage { message: m.into() }
    }
    pub fn not_implemented<S: Into<String>>(m: S) -> Self {
        Error::NotImplemented { message: m.into() }
    }
    pub fn internal<S: Into<String>>(m: S) -> Self {
        Error::Internal { message: m.into() }
    }
}

pub type CoreResult<T> = std::result::Result<T, Error>;

// ─── Conversions from common upstream error types ────────────────
//
// We map storage / network / serde errors to the closest semantic
// `Code`. The mapping is conservative — anything we can't classify
// goes to `Internal` so the frontend doesn't get a misleading
// "not_found" just because we ran out of detail.
impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        match e {
            rusqlite::Error::QueryReturnedNoRows => {
                Error::not_found("record not found in storage")
            }
            rusqlite::Error::SqliteFailure(err, _)
                if err.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                Error::conflict(format!("constraint violation: {err}"))
            }
            other => Error::storage(other.to_string()),
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::validation(format!("invalid JSON: {e}"))
    }
}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            Error::timeout(e.to_string())
        } else if e.is_connect() || e.is_request() {
            Error::network(e.to_string())
        } else if e.is_status() {
            // The HTTP-layer conversion in commands will turn this into
            // a more specific Upstream / Unauthorized / RateLimited,
            // but if the caller ignores status, default to Upstream.
            Error::upstream(e.to_string())
        } else {
            Error::internal(e.to_string())
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        match e.kind() {
            std::io::ErrorKind::NotFound => Error::not_found(e.to_string()),
            std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::AlreadyExists => {
                Error::conflict(e.to_string())
            }
            _ => Error::storage(e.to_string()),
        }
    }
}

impl From<url::ParseError> for Error {
    fn from(e: url::ParseError) -> Self {
        Error::validation(format!("invalid URL: {e}"))
    }
}

// `From` for `&'static str` and `String` is intentional — most call
// sites use `?` on a stringly-typed error, and forcing them to spell
// out `.into()` for every propagation would be noise. We map both to
// `Internal` because a bare string at a `?` boundary almost always
// means "I should have wrapped this in a real Error variant". This
// shows up loudly in code review.
impl From<&'static str> for Error {
    fn from(s: &'static str) -> Self {
        Error::internal(s)
    }
}
impl From<String> for Error {
    fn from(s: String) -> Self {
        Error::internal(s)
    }
}

// Used by the command layer to surface an Error to the Tauri IPC.
// Tauri's `#[tauri::command]` handler can return any `Result<T, E>`
// where `E: Display`, and Tauri serializes the Display output as the
// rejected promise's string. We already format the code prefix into
// Display via `thiserror`, so nothing else is needed here.
pub fn to_ipc_error(e: Error) -> impl fmt::Display + Send + Sync + 'static {
    struct IpcErr(Error);
    impl fmt::Display for IpcErr {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            // Match the `thiserror` derive output — we can't reuse the
            // Error directly because Tauri wants a 'static Display
            // (and we don't want to clone every error path).
            write!(f, "{}", self.0)
        }
    }
    IpcErr(e)
}
