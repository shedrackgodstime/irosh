use serde::Serialize;
use std::sync::atomic::AtomicBool;

pub static JSON_MODE: AtomicBool = AtomicBool::new(false);
pub static YES_MODE: AtomicBool = AtomicBool::new(false);

#[derive(Serialize)]
pub struct JsonEnvelope<T: Serialize> {
    pub success: bool,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonError>,
}

#[derive(Serialize)]
pub struct JsonError {
    pub message: String,
    pub code: String,
}

impl<T: Serialize> JsonEnvelope<T> {
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            timestamp: chrono::Utc::now().to_rfc3339(),
            data: Some(data),
            error: None,
        }
    }

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
    println!("{}", json);
}

/// Prints a standard JSON error response to stdout.
pub fn print_error(message: &str, code: &str) {
    let envelope = JsonEnvelope::<()>::error(message, code);
    let json = serde_json::to_string_pretty(&envelope).unwrap_or_default();
    println!("{}", json);
}
