use twilight_model::{
	application::{command::{Command, CommandType}, interaction::{Interaction, InteractionContextType, application_command::CommandData}},
	channel::message::{AllowedMentions, MessageFlags},
	http::interaction::{InteractionResponse, InteractionResponseType},
	oauth::ApplicationIntegrationType,
};
use twilight_util::builder::{InteractionResponseDataBuilder, command::CommandBuilder};
use worker::{Env, Result, console_error};

pub trait Mod {
	const NAMES: &[&str]; // slash/menu cmd names
	fn cmds() -> Vec<Command>;
	async fn run(env: &Env, i: &Interaction, d: &CommandData) -> Result<InteractionResponse>;
}

pub fn cmd(n: &str, d: &str, k: CommandType) -> CommandBuilder { // user + server install, usable anywhere
	CommandBuilder::new(n, d, k).integration_types([ApplicationIntegrationType::UserInstall, ApplicationIntegrationType::GuildInstall]).contexts([InteractionContextType::Guild, InteractionContextType::BotDm, InteractionContextType::PrivateChannel])
}

pub fn reply(s: &str) -> InteractionResponse { // ephemeral
	let d = InteractionResponseDataBuilder::new().content(s).flags(MessageFlags::EPHEMERAL).allowed_mentions(AllowedMentions::default()).build();
	InteractionResponse { kind: InteractionResponseType::ChannelMessageWithSource, data: Some(d) }
}

macro_rules! mods {
	($($m:ident: $t:ty),* $(,)?) => {
		$(pub mod $m;)*
		pub fn cmds() -> Vec<Command> { [$(<$t>::cmds(),)*].concat() }
		async fn run(env: &Env, i: &Interaction, d: &CommandData) -> Result<InteractionResponse> {
			$(if <$t>::NAMES.contains(&d.name.as_str()) { return <$t>::run(env, i, d).await; })*
			Ok(reply("Unknown command."))
		}
	};
}

mods! {
	ping: ping::Ping,
}

pub async fn dispatch(env: &Env, i: &Interaction, d: &CommandData) -> InteractionResponse {
	run(env, i, d).await.unwrap_or_else(|e| { console_error!("{}: {e}", d.name); reply("Something went wrong.") })
}
