mod cfg;
mod ctx;
mod err;
mod gw;
mod mods;

use cfg::Cfg;
use ctx::Ctx;
use err::Api;
use mods::Mods;
use shuttle_runtime::{SecretStore, Secrets};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use twilight_model::gateway::payload::outgoing::update_presence::UpdatePresencePayload;

struct Bot {
	ctx: Arc<Ctx>,
	pres: UpdatePresencePayload,
}

#[shuttle_runtime::async_trait]
impl shuttle_runtime::Service for Bot {
	async fn bind(self, _: SocketAddr) -> Result<(), shuttle_runtime::Error> { // not http, addr unused
		let c = self.ctx.clone();
		gw::run(self.ctx.cfg.token.clone(), Mods::intents(), self.pres, move |e| { tokio::spawn(mods::dispatch(c.clone(), e)); }).await?;
		Ok(())
	}
}

#[shuttle_runtime::main]
async fn main(#[Secrets] sec: SecretStore) -> Result<Bot, shuttle_runtime::Error> {
	let _ = rustls::crypto::ring::default_provider().install_default();
	let cfg = Cfg::load(&sec)?;
	let http = twilight_http::Client::builder().token(cfg.token.clone()).timeout(Duration::from_secs(15)).build();
	let ctx = Arc::new(Ctx { cfg, http, mods: Mods::load()? });
	let cmds = ctx.mods.cmds();
	ctx.http.interaction(ctx.cfg.app).set_global_commands(&cmds).await.api()?;
	for c in &cmds { println!("Registered /{}.", c.name); }
	Ok(Bot { ctx, pres: gw::presence(&sec) })
}
