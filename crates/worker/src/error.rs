use moni_protocol::api::{ApiError, ApiErrorCode};
use worker::{Response, Result};

pub(crate) fn error(status: u16, code: ApiErrorCode, message: &str) -> Result<Response> {
    Ok(Response::from_json(&ApiError {
        error: code,
        message: message.to_owned(),
    })?
    .with_status(status))
}

pub(crate) fn finish(result: Result<Response>) -> Result<Response> {
    match result {
        Ok(response) => Ok(response),
        Err(_) => error(500, ApiErrorCode::Internal, "Internal server error"),
    }
}

pub(crate) fn bad_request(message: &str) -> Result<Response> {
    error(400, ApiErrorCode::BadRequest, message)
}

pub(crate) fn not_found() -> Result<Response> {
    error(404, ApiErrorCode::NotFound, "Not found")
}

pub(crate) fn unauthorized() -> Result<Response> {
    error(401, ApiErrorCode::Unauthorized, "Authentication required")
}
