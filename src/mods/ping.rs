use super::{Mod, cmd, reply};
use twilight_model::{
	application::{command::{Command, CommandType}, interaction::{Interaction, application_command::CommandData}},
	http::interaction::InteractionResponse,
};
use worker::{Date, Env, Result};

pub struct Ping;

impl Mod for Ping {
	const NAMES: &[&str] = &["ping", "Ping"]; // slash, msg menu

	fn cmds() -> Vec<Command> {
		vec![cmd("ping", "Check if the bot is alive", CommandType::ChatInput).build(), cmd("Ping", "", CommandType::Message).build()]
	}

	async fn run(_: &Env, i: &Interaction, _: &CommandData) -> Result<InteractionResponse> {
		let ms = Date::now().as_millis().saturating_sub((i.id.get() >> 22) + 1420070400000); // snowflake ts -> discord to worker
		Ok(reply(&format!("Pong. {ms}ms")))
	}
}
