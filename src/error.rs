use crate::model::PageContext;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};

type BoxErr = Box<dyn std::error::Error + Send + Sync>;

/// application error with status and message
pub struct AppError {
    status: StatusCode,
    inner: BoxErr,
}

impl std::fmt::Debug for AppError {
    /// debug format for logging
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppError")
            .field("status", &self.status)
            .field("inner", &self.inner)
            .finish()
    }
}

impl AppError {
    /// create a new app error
    pub fn new(status: StatusCode, msg: impl Into<BoxErr>) -> Self {
        let inner = msg.into();
        Self { inner, status }
    }
}

impl IntoResponse for AppError {
    /// render error as html response
    fn into_response(self) -> Response {
        tracing::error!("{:?}", self);

        let html = PageContext::new()
            .insert("code", &self.status.as_u16())
            .insert("reason", &self.status.canonical_reason().unwrap_or("Error"))
            .insert("message", &self.inner.to_string())
            .render("error.html")
            .unwrap();
        (self.status, Html(html)).into_response()
    }
}

macro_rules! impl_from {
    ($ty:ty, $status:expr) => {
        impl From<$ty> for AppError {
            fn from(e: $ty) -> Self {
                Self::new($status, e)
            }
        }
    };
}

impl_from!(std::io::Error, StatusCode::INTERNAL_SERVER_ERROR);
impl_from!(redb::Error, StatusCode::INTERNAL_SERVER_ERROR);
impl_from!(redb::StorageError, StatusCode::INTERNAL_SERVER_ERROR);
impl_from!(redb::TableError, StatusCode::INTERNAL_SERVER_ERROR);
impl_from!(redb::TransactionError, StatusCode::INTERNAL_SERVER_ERROR);
impl_from!(redb::CommitError, StatusCode::INTERNAL_SERVER_ERROR);
impl_from!(tera::Error, StatusCode::INTERNAL_SERVER_ERROR);
