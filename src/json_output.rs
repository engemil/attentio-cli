use serde::Serialize;
use serde_json::json;

use crate::error::AttentioError;

/// Standard JSON success response wrapper.
#[derive(Serialize)]
pub struct JsonSuccess {
    pub status: String,
    #[serde(flatten)]
    pub data: serde_json::Value,
}

/// Standard JSON error response wrapper.
#[derive(Serialize)]
pub struct JsonError {
    pub status: String,
    pub error: String,
    pub error_type: String,
    #[serde(flatten)]
    pub context: serde_json::Value,
}

/// Format a success response as JSON.
///
/// The response will have `"status": "OK"` and the provided data fields.
///
/// # Example
///
/// ```
/// let output = format_success(json!({
///     "device": "AL1MB1-12345678",
///     "command": "version",
///     "response": "1.2.3",
/// }));
/// ```
///
/// Result:
/// ```json
/// {
///   "status": "OK",
///   "device": "AL1MB1-12345678",
///   "command": "version",
///   "response": "1.2.3"
/// }
/// ```
pub fn format_success(data: serde_json::Value) -> String {
    let response = JsonSuccess {
        status: "OK".to_string(),
        data,
    };
    serde_json::to_string_pretty(&response).unwrap_or_else(|_| "{}".to_string())
}

/// Format an error response as JSON.
///
/// The response will have `"status": "ERROR"`, error message, error type,
/// and any additional context fields.
///
/// # Example
///
/// ```
/// let err = anyhow::anyhow!("something went wrong");
/// let output = format_error(&err, json!({
///     "device": "AL1MB1-12345678",
///     "command": "badcmd",
/// }));
/// ```
///
/// Result:
/// ```json
/// {
///   "status": "ERROR",
///   "error": "something went wrong",
///   "error_type": "Other",
///   "device": "AL1MB1-12345678",
///   "command": "badcmd"
/// }
/// ```
pub fn format_error(error: &anyhow::Error, mut context: serde_json::Value) -> String {
    // Try to downcast to AttentioError to get structured error info
    let (error_type, error_context) =
        if let Some(attentio_err) = error.downcast_ref::<AttentioError>() {
            (attentio_err.error_type(), attentio_err.context_data())
        } else {
            ("Other", json!({}))
        };

    // Merge error-specific context with provided context
    if let serde_json::Value::Object(ref mut map) = context {
        if let serde_json::Value::Object(err_map) = error_context {
            for (k, v) in err_map {
                map.insert(k, v);
            }
        }
    }

    let response = JsonError {
        status: "ERROR".to_string(),
        error: error.to_string(),
        error_type: error_type.to_string(),
        context,
    };

    serde_json::to_string_pretty(&response).unwrap_or_else(|_| {
        json!({
            "status": "ERROR",
            "error": error.to_string(),
            "error_type": "Other"
        })
        .to_string()
    })
}
