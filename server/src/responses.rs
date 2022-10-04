use std::error::Error;
use std::{
    borrow::Cow,
    fmt::{Display, Formatter},
};

use rouille::Response;

#[derive(Clone, Debug)]
pub struct ErrorResponse {
    status: u16,
    error: Cow<'static, str>,
}

impl Error for ErrorResponse {}

impl ErrorResponse {
    pub fn unauthorized() -> Self {
        Self {
            status: 401,
            error: "Unauthorized".into(),
        }
    }

    pub fn unimplemented() -> Self {
        Self {
            status: 501,
            error: "Not implemented yet :/".into(),
        }
    }

    pub fn not_found() -> Self {
        Self {
            status: 404,
            error: "404 - Not found :/".into(),
        }
    }
}

impl Display for ErrorResponse {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.error)
    }
}

impl Into<Response> for ErrorResponse {
    fn into(self) -> Response {
        Response::text(self.error.to_string()).with_status_code(self.status)
    }
}
