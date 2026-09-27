use crate::{cfg::Cfg, err::Api, mods::Mods};
use anyhow::Result;
use twilight_model::{
	application::interaction::Interaction,
	channel::message::{AllowedMentions, MessageFlags},
	http::interaction::{InteractionResponse, InteractionResponseType},
};
use twilight_util::builder::InteractionResponseDataBuilder;

pub struct Ctx {
	pub cfg: Cfg,
	pub http: twilight_http::Client,
	pub mods: Mods,
}

impl Ctx {
	async fn respond(&self, i: &Interaction, kind: InteractionResponseType, s: Option<&str>) -> Result<()> {
		let mut d = InteractionResponseDataBuilder::new().flags(MessageFlags::EPHEMERAL).allowed_mentions(AllowedMentions::default());
		if let Some(s) = s { d = d.content(s); }
		self.http.interaction(self.cfg.app).create_response(i.id, &i.token, &InteractionResponse { kind, data: Some(d.build()) }).await.api()?;
		Ok(())
	}

	pub async fn reply(&self, i: &Interaction, s: &str) -> Result<()> { // ephemeral
		self.respond(i, InteractionResponseType::ChannelMessageWithSource, Some(s)).await
	}

	pub async fn edit(&self, i: &Interaction, s: &str) -> Result<()> {
		self.http.interaction(self.cfg.app).update_response(&i.token).content(Some(s)).allowed_mentions(Some(&AllowedMentions::default())).await.api()?;
		Ok(())
	}
}
