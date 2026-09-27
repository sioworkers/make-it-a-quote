use crate::ctx::Ctx;
use anyhow::Result;
use std::sync::Arc;
use twilight_gateway::Event;
use twilight_model::{
	application::{command::{Command, CommandType}, interaction::{Interaction, InteractionContextType, InteractionData, application_command::CommandData}},
	gateway::Intents,
	oauth::ApplicationIntegrationType,
};
use twilight_util::builder::command::CommandBuilder;

pub fn cmd(n: &str, d: &str, k: CommandType) -> CommandBuilder { // user + server install, usable anywhere
	CommandBuilder::new(n, d, k).integration_types([ApplicationIntegrationType::UserInstall, ApplicationIntegrationType::GuildInstall]).contexts([InteractionContextType::Guild, InteractionContextType::BotDm, InteractionContextType::PrivateChannel])
}

pub trait Mod: Sized {
	const NAMES: &[&str] = &[]; // slash/menu cmd names
	const INTENTS: Intents = Intents::empty();
	fn load() -> Result<Self>;
	fn cmds(&self) -> Vec<Command> { vec![] }
	async fn run(&self, _: &Arc<Ctx>, _: &Interaction, _: &CommandData) -> Result<()> { Ok(()) }
	async fn event(&self, _: &Arc<Ctx>, _: &Event) -> Result<()> { Ok(()) } // every gw event except cmds
}

macro_rules! mods {
	($($m:ident: $t:ty),* $(,)?) => {
		$(pub mod $m;)*
		pub struct Mods { $(pub $m: $t,)* }
		impl Mods {
			pub fn load() -> Result<Self> { Ok(Mods { $($m: <$t>::load()?,)* }) }
			pub fn intents() -> Intents { Intents::empty() $(| <$t>::INTENTS)* }
			pub fn cmds(&self) -> Vec<Command> { [$(self.$m.cmds(),)*].concat() }
			async fn run(&self, ctx: &Arc<Ctx>, i: &Interaction, d: &CommandData) -> Result<()> {
				$(if <$t>::NAMES.contains(&d.name.as_str()) { return self.$m.run(ctx, i, d).await; })*
				Ok(())
			}
			async fn event(&self, ctx: &Arc<Ctx>, e: &Event) {
				$(if let Err(err) = self.$m.event(ctx, e).await { eprintln!("{}: {err}", stringify!($m)); })*
			}
		}
	};
}

mods! {
	ping: ping::Ping,
}

pub async fn dispatch(ctx: Arc<Ctx>, e: Event) {
	if let Event::InteractionCreate(i) = &e && let Some(InteractionData::ApplicationCommand(d)) = &i.data {
		if let Err(err) = ctx.mods.run(&ctx, i, d).await { eprintln!("/{}: {err}", d.name); }
		return;
	}
	ctx.mods.event(&ctx, &e).await;
}
