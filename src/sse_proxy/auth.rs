//! Authentication middleware for the SSE proxy.
//!
//! This module provides authentication handling for the Actix Web-based SSE proxy,
//! implementing bearer token authentication and access control.

use crate::config::SSEProxyConfig;
use crate::error::Error;
use crate::sse_proxy::actix_error::ApiError; // Add this import

use actix_web::{
    Error as ActixError,
    dev::{Service, ServiceRequest, ServiceResponse, Transform, forward_ready},
};
use futures::future::{LocalBoxFuture, Ready, ready};
use std::sync::Arc;
use tracing;

/// Authentication middleware factory
pub struct Authentication {
    config: Arc<SSEProxyConfig>,
}

impl Authentication {
    /// Create a new Authentication middleware
    pub fn new(config: Arc<SSEProxyConfig>) -> Self {
        Self { config }
    }
}

impl<S, B> Transform<S, ServiceRequest> for Authentication
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = ActixError> + 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = ActixError;
    type Transform = AuthenticationMiddleware<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(AuthenticationMiddleware {
            service,
            config: self.config.clone(),
        }))
    }
}

/// Authentication middleware implementation
pub struct AuthenticationMiddleware<S> {
    service: S,
    config: Arc<SSEProxyConfig>,
}

impl<S, B> Service<ServiceRequest> for AuthenticationMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = ActixError> + 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = ActixError;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        // Skip authentication for OPTIONS requests (CORS preflight)
        if req.method() == "OPTIONS" {
            let fut = self.service.call(req);
            return Box::pin(async move {
                let res = fut.await?;
                Ok(res)
            });
        }

        // Check if authentication is required
        if let Some(auth_config) = &self.config.authenticate {
            if let Some(bearer_config) = &auth_config.bearer {
                let expected_token = &bearer_config.token;

                // Extract the Authorization header
                if let Some(auth_header) = req.headers().get("Authorization") {
                    if let Ok(auth_str) = auth_header.to_str() {
                        if let Some(token) = auth_str.strip_prefix("Bearer ") {
                            if token == expected_token {
                                // Token is valid, proceed with the request
                                let fut = self.service.call(req);
                                return Box::pin(async move {
                                    let res = fut.await?;
                                    Ok(res)
                                });
                            }
                        }
                    }
                }

                // Invalid or missing token
                tracing::warn!("Authentication failed: Invalid or missing bearer token");
                return Box::pin(async move {
                    // Convert to ApiError first, then into ActixError
                    Err(ApiError::from(Error::Unauthorized(
                        "Invalid or missing bearer token".to_string(),
                    ))
                    .into())
                });
            }
        }

        // No authentication required, proceed with the request
        let fut = self.service.call(req);
        Box::pin(async move {
            let res = fut.await?;
            Ok(res)
        })
    }
}
