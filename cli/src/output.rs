use serde::Serialize;
use std::sync::atomic::AtomicBool;

/// Whether JSON output mode is enabled.
pub static JSON_MODE: AtomicBool = AtomicBool::new(false);
/// Whether auto-confirm mode ("yes" mode) is enabled.
pub static YES_MODE: AtomicBool = AtomicBool::new(false);

/// Standard JSON envelope wrapping a response payload.
#[derive(Serialize)]
pub struct JsonEnvelope<T: Serialize> {
    /// Whether the operation succeeded.
    pub success: bool,
    /// RFC 3339 timestamp of the response.
    pub timestamp: String,
    /// Optional success payload.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    /// Optional error details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonError>,
}

/// Error details included in a failed JSON envelope.
#[derive(Serialize)]
pub struct JsonError {
    /// Human-readable error description.
    pub message: String,
    /// Machine-readable error code.
    pub code: String,
}

impl<T: Serialize> JsonEnvelope<T> {
    /// Creates a success envelope wrapping the given data.
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            timestamp: chrono::Utc::now().to_rfc3339(),
            data: Some(data),
            error: None,
        }
    }

    /// Creates an error envelope with the given message and code.
    #[must_use]
    pub fn error(message: &str, code: &str) -> Self {
        Self {
            success: false,
            timestamp: chrono::Utc::now().to_rfc3339(),
            data: None,
            error: Some(JsonError {
                message: message.to_string(),
                code: code.to_string(),
            }),
        }
    }
}

/// Prints a standard JSON success response to stdout and exits.
pub fn print_success<T: Serialize>(data: T) {
    let envelope = JsonEnvelope::success(data);
    let json = serde_json::to_string_pretty(&envelope).unwrap_or_default();
    println!("{json}");
}

/// Prints a standard JSON error response to stdout.
pub fn print_error(message: &str, code: &str) {
    let envelope = JsonEnvelope::<()>::error(message, code);
    let json = serde_json::to_string_pretty(&envelope).unwrap_or_default();
    println!("{json}");
}
