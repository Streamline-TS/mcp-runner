//! Actix Web error adapters for MCP Runner errors.
//!
//! This module provides implementations of Actix Web error traits
//! for the MCP Runner error types, allowing them to be used in Actix Web handlers.

use crate::error::Error;
use actix_web::{HttpResponse, ResponseError, http::StatusCode};
use serde_json::json;

// Implement ResponseError for our Error type
impl ResponseError for Error {
    fn error_response(&self) -> HttpResponse {
        let status_code = match self {
            Error::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            Error::ServerNotFound(_) => StatusCode::NOT_FOUND,
            Error::ToolNotFound(_) => StatusCode::NOT_FOUND,
            Error::ResourceNotFound(_) => StatusCode::NOT_FOUND,
            Error::ConfigInvalid(_) => StatusCode::BAD_REQUEST,
            Error::ConfigParse(_) => StatusCode::BAD_REQUEST,
            Error::ConfigValidation(_) => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };

        HttpResponse::build(status_code)
            .content_type("application/json")
            .json(json!({
                "error": self.to_string(),
                "code": status_code.as_u16()
            }))
    }

    fn status_code(&self) -> StatusCode {
        match self {
            Error::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            Error::ServerNotFound(_) => StatusCode::NOT_FOUND,
            Error::ToolNotFound(_) => StatusCode::NOT_FOUND,
            Error::ResourceNotFound(_) => StatusCode::NOT_FOUND,
            Error::ConfigInvalid(_) => StatusCode::BAD_REQUEST,
            Error::ConfigParse(_) => StatusCode::BAD_REQUEST,
            Error::ConfigValidation(_) => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

// Create a wrapper error type for request validation errors
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("Unsupported operation: {0}")]
    UnsupportedOperation(String),

    #[error("Internal server error: {0}")]
    Internal(#[from] Error),
}

impl ResponseError for ApiError {
    fn error_response(&self) -> HttpResponse {
        let status_code = match self {
            ApiError::InvalidRequest(_) => StatusCode::BAD_REQUEST,
            ApiError::UnsupportedOperation(_) => StatusCode::NOT_IMPLEMENTED,
            ApiError::Internal(e) => e.status_code(),
        };

        HttpResponse::build(status_code)
            .content_type("application/json")
            .json(json!({
                "error": self.to_string(),
                "code": status_code.as_u16()
            }))
    }

    fn status_code(&self) -> StatusCode {
        match self {
            ApiError::InvalidRequest(_) => StatusCode::BAD_REQUEST,
            ApiError::UnsupportedOperation(_) => StatusCode::NOT_IMPLEMENTED,
            ApiError::Internal(e) => e.status_code(),
        }
    }
}
