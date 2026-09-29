use super::{Cx, Mod, cmd, reply};
use crate::card;
use futures_util::future::join_all;
use image::{GrayImage, ImageReader};
use serde_json::json;
use twilight_model::{
	application::{command::{Command, CommandType}, interaction::{Interaction, application_command::{CommandData, CommandOptionValue}}},
	channel::Message,
	http::interaction::{InteractionResponse, InteractionResponseType},
	id::{Id, marker::{ChannelMarker, MessageMarker}},
};
use twilight_util::builder::command::StringBuilder;
use worker::{Env, Fetch, Headers, Method, Request, RequestInit, Result, Url, console_error, console_log, js_sys::Uint8Array};

const API: &str = "https://discord.com/api/v10";
const SRC: &str = "https://github.com/sioworkers/make-it-a-quote";
const FB: &[&str] = &["Noto Sans Math", "Noto Emoji", "Noto Sans Symbols 2", "Noto Sans Symbols", "Noto Sans JP", "Noto Sans KR", "Noto Sans SC", "Noto Sans Arabic", "Noto Sans Hebrew", "Noto Sans Devanagari", "Noto Sans Thai"]; // fallback order
const MAX_PX: u64 = 4_200_000; // decode cap (~2048x2048), bigger = cpu limit
const UA: &str = "Mozilla/5.0 (compatible; MIAQ; +https://miaq.sioworker.workers.dev)";

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
	let h = Headers::new();h.set("User-Agent", UA)?;
	let mut init = RequestInit::new();init.with_headers(h);
	let mut r = Fetch::Request(Request::new_with_init(url, &init)?).send().await?;
	if r.status_code() != 200 { return Err(format!("{url}: {}", r.status_code()).into()); }
	r.bytes().await
}

async fn gray(url: &str) -> Option<GrayImage> {
	let b = fetch(url).await.map_err(|e| console_error!("img: {e}")).ok()?;
	let r = ImageReader::new(std::io::Cursor::new(&b)).with_guessed_format().ok()?;
	let (w, h) = r.into_dimensions().map_err(|e| console_error!("img dims: {e}")).ok()?;
	if w as u64 * h as u64 > MAX_PX { console_error!("img too big: {w}x{h} {url}");return None; }
	image::load_from_memory(&b).map(|i| i.to_luma8()).map_err(|e| console_error!("img decode: {e}")).ok()
}

async fn gfont(fam: &str, text: &str) -> Option<Vec<u8>> { // google fonts subset w/ only these chars, old UA gets ttf
	let u = Url::parse_with_params("https://fonts.googleapis.com/css2", [("family", fam), ("text", text)]).ok()?;
	let h = Headers::new();h.set("User-Agent", "Wget/1.21").ok()?;
	let mut init = RequestInit::new();init.with_headers(h);
	let css = Fetch::Request(Request::new_with_init(u.as_str(), &init).ok()?).send().await.ok()?.text().await.ok()?;
	fetch(css.split("url(").nth(1)?.split(')').next()?).await.ok()
}

async fn fonts(s: &str) -> Vec<Vec<u8>> { // fallbacks for what noto serif can't draw
	let need = card::missing(s);
	if need.is_empty() { return vec![]; }
	let (mut left, mut out) = (need.clone(), vec![]);
	for b in join_all(FB.iter().map(|f| gfont(f, &need))).await.into_iter().flatten() {
		let l = card::lacks(&b, &left);
		if l.chars().count() < left.chars().count() { out.push(b);left = l; }
		if left.is_empty() { break; }
	}
	if !left.is_empty() { console_log!("no font for {:?}", left); }
	out
}

fn pic(m: &Message, text: bool) -> Vec<String> { // first image as urls to try: media proxy at the exact size we draw, then the original (proxy 403s workers sometimes)
	let a = m.attachments.iter().find(|a| a.content_type.as_deref().is_some_and(|t| t.starts_with("image/")) || (a.content_type.is_none() && a.width.is_some())).map(|a| (Some(a.proxy_url.clone()), a.url.clone(), a.width, a.height));
	let e = || m.embeds.iter().find_map(|e| e.image.as_ref().map(|i| (i.proxy_url.clone(), i.url.clone(), i.width, i.height)).or_else(|| e.thumbnail.as_ref().map(|t| (t.proxy_url.clone(), t.url.clone(), t.width, t.height))));
	let Some((px, orig, w, h)) = a.or_else(e) else { return vec![] };
	let mut out = vec![];
	if let Some(u) = px {
		let sep = if u.ends_with(['?', '&']) { "" } else if u.contains('?') { "&" } else { "?" };
		out.push(match (w, h) { (Some(w), Some(h)) => { let (w, h) = card::fit_img(w as u32, h as u32, text);format!("{u}{sep}format=webp&width={w}&height={h}") } _ => format!("{u}{sep}format=webp") });
	}
	out.push(orig);
	out
}

fn gif_src(m: &Message) -> Option<String> { // animated source: gif upload, tenor/giphy embed, or direct .gif link
	if let Some(a) = m.attachments.iter().find(|a| a.content_type.as_deref() == Some("image/gif") || a.filename.to_lowercase().ends_with(".gif")) { return Some(a.url.clone()); }
	m.embeds.iter().find_map(|e| {
		let urls: Vec<&str> = [e.video.as_ref().and_then(|v| v.url.as_deref()), e.thumbnail.as_ref().map(|t| t.url.as_str()), e.image.as_ref().map(|i| i.url.as_str()), e.url.as_deref()].into_iter().flatten().collect();
		urls.iter().find_map(|u| tenor(u).or_else(|| giphy(u))).or_else(|| urls.iter().find(|u| u.split(['?', '#']).next().is_some_and(|p| p.to_lowercase().ends_with(".gif"))).map(|u| u.to_string()))
	})
}

fn tenor(u: &str) -> Option<String> { // media.tenor.com/{11 id}{5 fmt}/{slug}.{ext} -> AAAAM = medium gif
	let p = u.split("media.tenor.com/").nth(1)?;let mut it = p.split('/');
	let (id, slug) = (it.next()?, it.next()?.split(['?', '.']).next()?);
	(id.len() == 16).then(|| format!("https://media.tenor.com/{}AAAAM/{slug}.gif", &id[..11]))
}

fn giphy(u: &str) -> Option<String> { // media*.giphy.com/media/{id}/... -> 200px tall gif
	if !u.contains("giphy.com/media/") { return None; }
	let id = u.split("/media/").nth(1)?.split('/').next()?;
	Some(format!("https://media.giphy.com/media/{id}/200.gif"))
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
	o.join("\n").replace("**", "").replace("__", "").replace("~~", "").replace("||", "").replace('`', "")
}

fn avatar(m: &Message) -> String {
	let u = &m.author;
	match u.avatar { Some(h) => format!("https://cdn.discordapp.com/avatars/{}/{h}.png?size=512", u.id), None => format!("https://cdn.discordapp.com/embed/avatars/{}.png", (u.id.get() >> 22) % 6) }
}

async fn make(i: &Interaction, m: &Message) -> Result<()> {
	let mut text = clean(m);
	let link_only = !m.embeds.is_empty() && m.attachments.is_empty() && !text.trim().contains(char::is_whitespace) && text.trim().starts_with("http");
	if link_only { text.clear(); } // the link is the image, don't also print it
	let av = gray(&avatar(m)).await;
	let (name, user) = (m.author.global_name.clone().unwrap_or_else(|| m.author.name.clone()), format!("@{}", m.author.name));
	let fb = fonts(&format!("{text}{name}{user}")).await;
	let mut fr = vec![];
	if let Some(u) = gif_src(m) { console_log!("gif {u}");if let Ok(b) = fetch(&u).await.map_err(|e| console_error!("gif: {e}")) { fr = card::frames(&b); } }
	let file = if fr.len() > 1 {
		(card::render_gif(av.as_ref(), &fr, &text, &name, &user, &fb), "quote.gif", "image/gif")
	} else {
		let mut img = fr.pop().map(|f| f.0);
		for u in pic(m, !text.trim().is_empty()) { if img.is_some() { break; } console_log!("pic {u}");img = gray(&u).await; }
		(card::render(av.as_ref(), img.as_ref(), &text, &name, &user, &fb), "quote.png", "image/png")
	};
	let g = i.guild_id.map_or("@me".into(), |g| g.to_string());
	edit(i, &format!("-# [Jump to message](<https://discord.com/channels/{g}/{}/{}>) | [Source](<{SRC}>)", m.channel_id, m.id), Some(file)).await
}

async fn edit(i: &Interaction, s: &str, file: Option<(Vec<u8>, &str, &str)>) -> Result<()> { // PATCH @original, multipart when there's a file (bytes, name, mime)
	let files = match &file { Some((_, n, _)) => json!([{ "id": 0, "filename": n }]), None => json!([]) };
	let pay = json!({ "content": s, "allowed_mentions": { "parse": [] }, "attachments": files }).to_string();
	let b = "miaqmiaqmiaq";
	let mut body = format!("--{b}\r\nContent-Disposition: form-data; name=\"payload_json\"\r\nContent-Type: application/json\r\n\r\n{pay}\r\n").into_bytes();
	if let Some((p, n, t)) = file {
		body.extend(format!("--{b}\r\nContent-Disposition: form-data; name=\"files[0]\"; filename=\"{n}\"\r\nContent-Type: {t}\r\n\r\n").as_bytes());
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

	#[test]
	fn gifs() {
		assert_eq!(tenor("https://media.tenor.com/1s5oUIWc42UAAAP1/cats-cat.mp4").as_deref(), Some("https://media.tenor.com/1s5oUIWc42UAAAAM/cats-cat.gif"));
		assert_eq!(tenor("https://media.tenor.com/1s5oUIWc42UAAAAe/cats-cat.png?x=1").as_deref(), Some("https://media.tenor.com/1s5oUIWc42UAAAAM/cats-cat.gif"));
		assert_eq!(giphy("https://media2.giphy.com/media/abc123/giphy.mp4").as_deref(), Some("https://media.giphy.com/media/abc123/200.gif"));
		assert_eq!(tenor("https://tenor.com/view/cat-gif-123"), None);
	}
}
