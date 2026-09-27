use std::fmt;
use twilight_http::{api_error::ApiError, error::ErrorType};

#[derive(Debug)]
pub struct ApiErr {
	msg: String,
}

impl fmt::Display for ApiErr {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { f.write_str(&self.msg) }
}

impl std::error::Error for ApiErr {}

pub trait Api<T> {
	fn api(self) -> anyhow::Result<T>;
}

impl<T> Api<T> for Result<T, twilight_http::Error> {
	fn api(self) -> anyhow::Result<T> {
		self.map_err(|e| {
			let (code, why) = match e.kind() {
				ErrorType::Response { status, error, .. } => (status.get(), match error {
					ApiError::General(g) => g.message.clone(),
					ApiError::Ratelimited(r) => r.message.clone(),
					_ => String::new(),
				}),
				ErrorType::Unauthorized => (401, "401: Unauthorized".into()),
				_ => return e.into(),
			};
			ApiErr { msg: format!("Discord API error {code}: {why}") }.into()
		})
	}
}
