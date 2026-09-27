use crate::cfg::get;
use anyhow::{Result, bail};
use shuttle_runtime::SecretStore;
use twilight_gateway::{CloseFrame, ConfigBuilder, Event, EventTypeFlags, Intents, Shard, ShardId, StreamExt};
use twilight_model::gateway::{payload::outgoing::update_presence::UpdatePresencePayload, presence::{ActivityType, MinimalActivity, Status}};

async fn shutdown() {
	#[cfg(unix)]
	{
		let mut t = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).unwrap();
		tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = t.recv() => {} }
	}
	#[cfg(not(unix))]
	let _ = tokio::signal::ctrl_c().await;
}

pub fn presence(sec: &SecretStore) -> UpdatePresencePayload { // PRESENCE, ACTIVITY_TYPE, ACTIVITY
	let status = match get(sec, "PRESENCE").to_lowercase().as_str() { "idle" => Status::Idle, "dnd" => Status::DoNotDisturb, "invisible" | "offline" => Status::Invisible, _ => Status::Online };
	let kind = match get(sec, "ACTIVITY_TYPE").to_lowercase().as_str() { "watching" => ActivityType::Watching, "listening" => ActivityType::Listening, "competing" => ActivityType::Competing, _ => ActivityType::Playing };
	let name = get(sec, "ACTIVITY");
	let activities = if name.is_empty() { vec![] } else { vec![MinimalActivity { kind, name, url: None }.into()] };
	UpdatePresencePayload { activities, afk: false, since: None, status }
}

pub async fn run(token: String, intents: Intents, pres: UpdatePresencePayload, on: impl Fn(Event)) -> Result<()> {
	let mut sh = Shard::with_config(ShardId::ONE, ConfigBuilder::new(token, intents).presence(pres).build());
	let stop = shutdown();tokio::pin!(stop);
	let mut stopping = false;
	loop {
		let ev = tokio::select! {
			e = sh.next_event(EventTypeFlags::all()) => e,
			_ = &mut stop, if !stopping => { stopping = true; sh.close(CloseFrame::NORMAL); continue }
		};
		match ev {
			None => bail!("Gateway closed for good."),
			Some(Err(e)) => eprintln!("Gateway error: {e}"),
			Some(Ok(Event::GatewayClose(f))) => {
				if stopping { return Ok(()); }
				let code = f.map_or(1005, |f| f.code);
				if [4004, 4010, 4013, 4014].contains(&code) { bail!("Discord closed the connection ({code}). Check the bot token and gateway settings."); }
				eprintln!("Disconnected from Discord ({code}). Reconnecting...");
			}
			Some(Ok(Event::Ready(r))) => println!("Connected to Discord as {}.", r.user.name),
			Some(Ok(Event::Resumed)) => println!("Reconnected to Discord."),
			Some(Ok(e)) => on(e),
		}
	}
}
