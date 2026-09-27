use super::{Mod, cmd};
use crate::ctx::Ctx;
use anyhow::Result;
use std::{sync::Arc, time::Instant};
use twilight_model::application::{command::{Command, CommandType}, interaction::{Interaction, application_command::CommandData}};

pub struct Ping;

impl Mod for Ping {
	const NAMES: &[&str] = &["ping", "Ping"]; // slash, msg menu

	fn load() -> Result<Self> { Ok(Ping) }

	fn cmds(&self) -> Vec<Command> {
		vec![cmd("ping", "Check if the bot is alive", CommandType::ChatInput).build(), cmd("Ping", "", CommandType::Message).build()]
	}

	async fn run(&self, ctx: &Arc<Ctx>, i: &Interaction, _: &CommandData) -> Result<()> {
		let t = Instant::now();ctx.reply(i, "Pong.").await?;
		ctx.edit(i, &format!("Pong. {}ms", t.elapsed().as_millis())).await
	}
}
