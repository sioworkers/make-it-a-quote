use anyhow::{Result, bail};
use shuttle_runtime::SecretStore;
use twilight_model::id::{Id, marker::ApplicationMarker};

pub struct Cfg {
	pub token: String,
	pub app: Id<ApplicationMarker>,
}

impl Cfg {
	pub fn load(sec: &SecretStore) -> Result<Self> {
		let token = get(sec, "DISCORD_TOKEN");
		if token.is_empty() { bail!("DISCORD_TOKEN is missing from Secrets.toml."); }
		Ok(Cfg { token, app: id(sec, "CLIENT_ID")? })
	}
}

pub fn get(sec: &SecretStore, k: &str) -> String {
	sec.get(k).unwrap_or_default().trim().to_string()
}

pub fn id<T>(sec: &SecretStore, k: &str) -> Result<Id<T>> {
	let v = get(sec, k);
	match ((17..=20).contains(&v.len()) && v.bytes().all(|b| b.is_ascii_digit())).then(|| v.parse().ok().and_then(Id::new_checked)).flatten() {
		Some(i) => Ok(i),
		None => bail!("{k} must be a Discord ID in Secrets.toml."),
	}
}
