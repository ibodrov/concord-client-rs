use tokio_tungstenite::tungstenite;

#[derive(Debug)]
pub struct ApiError {
    pub message: String,
}

impl ApiError {
    pub fn simple(message: &str) -> Self {
        ApiError {
            message: message.to_string(),
        }
    }
}

#[macro_export]
macro_rules! api_error {
    ($($arg:tt)*) => {
        ApiError {
            message: format!($($arg)*),
        }
    };
}

#[macro_export]
macro_rules! api_err {
    ($($arg:tt)*) => {
        Err($crate::api_error!($($arg)*))
    };
}

impl std::error::Error for ApiError {}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl From<tungstenite::Error> for ApiError {
    fn from(e: tungstenite::Error) -> Self {
        ApiError {
            message: e.to_string(),
        }
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(e: serde_json::Error) -> Self {
        ApiError {
            message: e.to_string(),
        }
    }
}

impl From<reqwest::Error> for ApiError {
    fn from(e: reqwest::Error) -> Self {
        ApiError {
            message: e.to_string(),
        }
    }
}

impl From<url::ParseError> for ApiError {
    fn from(e: url::ParseError) -> Self {
        ApiError {
            message: e.to_string(),
        }
    }
}
