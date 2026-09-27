use super::{Cx, Mod, cmd, reply};
use crate::card;
use image::GrayImage;
use serde_json::json;
use twilight_model::{
	application::{command::{Command, CommandType}, interaction::{Interaction, application_command::{CommandData, CommandOptionValue}}},
	channel::Message,
	http::interaction::{InteractionResponse, InteractionResponseType},
	id::{Id, marker::{ChannelMarker, MessageMarker}},
};
use twilight_util::builder::command::StringBuilder;
use worker::{Env, Fetch, Headers, Method, Request, RequestInit, Result, console_error, js_sys::Uint8Array};

const API: &str = "https://discord.com/api/v10";

pub struct Quote;

impl Mod for Quote {
	const NAMES: &[&str] = &["quote", "Quote"]; // slash, msg menu

	fn cmds() -> Vec<Command> {
		vec![
			cmd("quote", "Turn a message into a quote image", CommandType::ChatInput).option(StringBuilder::new("link", "Message link, from this channel").required(true)).build(),
			cmd("Quote", "", CommandType::Message).build(),
		]
	}

	async fn run(cx: &Cx, i: &Interaction, d: &CommandData) -> Result<InteractionResponse> {
		let m = if d.kind == CommandType::Message {
			d.target_id.and_then(|t| d.resolved.as_ref()?.messages.get(&t.cast()).cloned())
		} else {
			let link = d.options.iter().find(|o| o.name == "link").and_then(|o| if let CommandOptionValue::String(s) = &o.value { Some(s.as_str()) } else { None }).unwrap_or_default();
			let Some((c, m)) = parse(link) else { return Ok(reply("That's not a message link. Right-click a message, Copy Message Link.")) };
			if i.channel.as_ref().map(|x| x.id) != Some(c) { return Ok(reply("I can only quote messages from this channel. For others, right-click the message, then Apps, then Quote.")); } // else anyone could read chans they can't see
			match get(&cx.env, c, m).await { Some(m) => Some(m), None => return Ok(reply("I can't see that message. Right-click it, then Apps, then Quote instead.")) }
		};
		let Some(m) = m else { return Ok(reply("Couldn't find that message.")) };
		let i = i.clone();
		cx.wc.wait_until(async move {
			if let Err(e) = make(&i, &m).await {
				console_error!("quote: {e}");
				if let Err(e) = edit(&i, "Couldn't make that quote.", None).await { console_error!("quote edit: {e}"); }
			}
		});
		Ok(InteractionResponse { kind: InteractionResponseType::DeferredChannelMessageWithSource, data: None })
	}
}

fn parse(s: &str) -> Option<(Id<ChannelMarker>, Id<MessageMarker>)> { // .../channels/{guild|@me}/{chan}/{msg}
	let mut p = s.trim().trim_matches(['<', '>']).split("/channels/").nth(1)?.split('/').skip(1);
	Some((p.next()?.parse().ok()?, p.next()?.split(['?', '#']).next()?.parse().ok()?))
}

async fn get(env: &Env, c: Id<ChannelMarker>, m: Id<MessageMarker>) -> Option<Message> {
	let tok = env.secret("DISCORD_TOKEN").ok()?.to_string();
	let h = Headers::new();h.set("Authorization", &format!("Bot {tok}")).ok()?;
	let mut init = RequestInit::new();init.with_headers(h);
	let mut r = Fetch::Request(Request::new_with_init(&format!("{API}/channels/{c}/messages/{m}"), &init).ok()?).send().await.ok()?;
	if r.status_code() != 200 { return None; }
	r.json().await.ok()
}

async fn fetch(url: &str) -> Result<Vec<u8>> {
	let mut r = Fetch::Url(url.parse()?).send().await?;
	if r.status_code() != 200 { return Err(format!("{url}: {}", r.status_code()).into()); }
	r.bytes().await
}

async fn gray(url: &str) -> Option<GrayImage> {
	let b = fetch(url).await.map_err(|e| console_error!("img: {e}")).ok()?;
	image::load_from_memory(&b).map(|i| i.to_luma8()).map_err(|e| console_error!("img decode: {e}")).ok()
}

fn pic(m: &Message, text: bool) -> Option<String> { // first image: attachment, then embed image/thumb. asks media proxy for the exact size we draw
	let a = m.attachments.iter().find(|a| a.content_type.as_deref().is_some_and(|t| t.starts_with("image/")) || (a.content_type.is_none() && a.width.is_some())).map(|a| (a.proxy_url.clone(), a.width, a.height));
	let e = || m.embeds.iter().find_map(|e| e.image.as_ref().map(|i| (i.proxy_url.clone(), i.width, i.height)).or_else(|| e.thumbnail.as_ref().map(|t| (t.proxy_url.clone(), t.width, t.height)))).and_then(|(u, w, h)| Some((u?, w, h)));
	let (u, w, h) = a.or_else(e)?;
	let sep = if u.contains('?') { '&' } else { '?' };
	Some(match (w, h) {
		(Some(w), Some(h)) => { let (w, h) = card::fit_img(w as u32, h as u32, text); format!("{u}{sep}format=png&width={w}&height={h}") }
		_ => format!("{u}{sep}format=png"),
	})
}

fn tag(t: &str) -> String { // inside <...>
	if let Some(e) = t.strip_prefix("a:").or_else(|| t.strip_prefix(':')) { return format!(":{}:", e.split(':').next().unwrap_or_default()); }
	if t.starts_with("@&") { return "@role".into(); }
	if t.starts_with('@') { return "@user".into(); }
	if t.starts_with('#') { return "#channel".into(); }
	if let Some(ts) = t.strip_prefix("t:") { return ts.split(':').next().and_then(|n| n.parse().ok()).map_or(t.into(), date); }
	if t.starts_with("http") { return t.into(); }
	format!("<{t}>")
}

fn date(t: i64) -> String { // unix -> 2026-09-27, civil from days
	let z = t.div_euclid(86400) + 719468;let era = z.div_euclid(146097);let doe = z - era * 146097;
	let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
	let mp = (5 * doy + 2) / 153;let (d, m) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
	format!("{}-{m:02}-{d:02}", yoe + era * 400 + (m <= 2) as i64)
}

fn clean(m: &Message) -> String { // discord markup -> plain text
	let mut s = m.content.clone();
	for u in &m.mentions {
		let n = format!("@{}", u.member.as_ref().and_then(|x| x.nick.as_deref()).unwrap_or(&u.name));
		s = s.replace(&format!("<@{}>", u.id), &n).replace(&format!("<@!{}>", u.id), &n);
	}
	let (mut o, mut rest) = (String::new(), s.as_str());
	while let Some(a) = rest.find('<') {
		o += &rest[..a];
		match rest[a..].find('>') { Some(b) => { o += &tag(&rest[a + 1..a + b]);rest = &rest[a + b + 1..]; } None => { o += &rest[a..];rest = ""; } }
	}
	o += rest;
	let o: Vec<String> = o.lines().map(|l| { let l = l.trim_start(); l.trim_start_matches("-# ").trim_start_matches(['#', '>']).trim_start().to_string() }).collect();
	o.join("\n").replace("**", "").replace("__", "").replace("~~", "").replace("||", "").replace('`', "").chars().filter(|&c| card::known(c)).collect()
}

fn avatar(m: &Message) -> String {
	let u = &m.author;
	match u.avatar { Some(h) => format!("https://cdn.discordapp.com/avatars/{}/{h}.png?size=256", u.id), None => format!("https://cdn.discordapp.com/embed/avatars/{}.png", (u.id.get() >> 22) % 6) }
}

async fn make(i: &Interaction, m: &Message) -> Result<()> {
	let text = clean(m);
	let av = gray(&avatar(m)).await;
	let img = match pic(m, !text.trim().is_empty()) { Some(u) => gray(&u).await, None => None };
	let name = m.author.global_name.clone().unwrap_or_else(|| m.author.name.clone());
	let png = card::render(av.as_ref(), img.as_ref(), &text, &name, &format!("@{}", m.author.name));
	let g = i.guild_id.map_or("@me".into(), |g| g.to_string());
	edit(i, &format!("-# [Source](https://discord.com/channels/{g}/{}/{})", m.channel_id, m.id), Some(png)).await
}

async fn edit(i: &Interaction, s: &str, png: Option<Vec<u8>>) -> Result<()> { // PATCH @original, multipart when there's a file
	let files = if png.is_some() { json!([{ "id": 0, "filename": "quote.png" }]) } else { json!([]) };
	let pay = json!({ "content": s, "allowed_mentions": { "parse": [] }, "attachments": files }).to_string();
	let b = "miaqmiaqmiaq";
	let mut body = format!("--{b}\r\nContent-Disposition: form-data; name=\"payload_json\"\r\nContent-Type: application/json\r\n\r\n{pay}\r\n").into_bytes();
	if let Some(p) = png {
		body.extend(format!("--{b}\r\nContent-Disposition: form-data; name=\"files[0]\"; filename=\"quote.png\"\r\nContent-Type: image/png\r\n\r\n").as_bytes());
		body.extend(p);body.extend(b"\r\n");
	}
	body.extend(format!("--{b}--\r\n").as_bytes());
	let h = Headers::new();h.set("Content-Type", &format!("multipart/form-data; boundary={b}"))?;
	let mut init = RequestInit::new();init.with_method(Method::Patch).with_headers(h).with_body(Some(Uint8Array::from(&body[..]).into()));
	let mut r = Fetch::Request(Request::new_with_init(&format!("{API}/webhooks/{}/{}/messages/@original", i.application_id, i.token), &init)?).send().await?;
	if !(200..300).contains(&r.status_code()) { return Err(format!("edit {}: {}", r.status_code(), r.text().await.unwrap_or_default()).into()); }
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn links() {
		assert_eq!(parse("https://discord.com/channels/1/22/333").map(|(c, m)| (c.get(), m.get())), Some((22, 333)));
		assert_eq!(parse("<https://canary.discord.com/channels/@me/22/333>").map(|(c, m)| (c.get(), m.get())), Some((22, 333)));
		assert_eq!(parse("hello"), None);
	}

	#[test]
	fn tags() {
		assert_eq!(tag(":kek:123"), ":kek:");
		assert_eq!(tag("a:dance:123"), ":dance:");
		assert_eq!(tag("#123"), "#channel");
		assert_eq!(tag("@&123"), "@role");
		assert_eq!(tag("t:1700000000:R"), "2023-11-14");
		assert_eq!(tag("https://x.y"), "https://x.y");
	}
}
