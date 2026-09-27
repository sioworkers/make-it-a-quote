use worker::{Env, Request, Response, Result, Url};

const HEAD: &str = include_str!("../web/partials/head.html");
const HOME: &str = include_str!("../web/templates/index.html");
const DONE: &str = include_str!("../web/templates/done.html");
const CSS: &str = include_str!("../web/style.css");
const LOGO: &[u8] = include_bytes!("../web/static/logo.png"); // favicon
const BUBBLE: &[u8] = include_bytes!("../web/static/bubble.png"); // page logo, cropped

fn page(t: &str, kv: &[(&str, &str)]) -> Result<Response> { // %%key%% -> val, vals are ours, never user input
	let mut s = t.replace("%%head%%", HEAD);
	for (k, v) in kv { s = s.replace(&format!("%%{k}%%"), v); }
	Response::from_html(s)
}

fn auth(app: &str, cb: &str, extra: &[(&str, &str)]) -> Result<String> {
	let q = [("client_id", app), ("response_type", "code"), ("redirect_uri", cb)];
	Ok(Url::parse_with_params("https://discord.com/oauth2/authorize", q.iter().chain(extra))?.into())
}

pub fn home(req: &Request, env: &Env) -> Result<Response> {
	let app = env.var("CLIENT_ID")?.to_string();
	let cb = format!("{}/done", req.url()?.origin().ascii_serialization());
	let user = auth(&app, &cb, &[("integration_type", "1"), ("scope", "applications.commands")])?;
	let guild = auth(&app, &cb, &[("integration_type", "0"), ("scope", "bot"), ("permissions", "2147600384")])?; // send, embed, attach, history, app cmds
	page(HOME, &[("user", &user), ("guild", &guild)])
}

pub fn done(req: &Request) -> Result<Response> { // oauth2 redirect, code unused since install is all we need
	let q: Vec<(String, String)> = req.url()?.query_pairs().into_owned().collect();
	let has = |k: &str| q.iter().any(|(n, _)| n == k);
	let (t, s) = match () {
		_ if has("error") => ("Cancelled", "Nothing was added."),
		_ if has("guild_id") => ("Added", "MIAQ is in your server. Right-click a message, then Apps, then Quote, or use /quote."),
		_ => ("Added", "MIAQ is on your account. Right-click any message, then Apps, then Quote."),
	};
	page(DONE, &[("title", t), ("text", s)])
}

fn asset(mut r: Response, ty: &str) -> Result<Response> {
	r.headers_mut().set("Content-Type", ty)?;
	r.headers_mut().set("Cache-Control", "public, max-age=3600")?;
	Ok(r)
}

pub fn css() -> Result<Response> { asset(Response::ok(CSS)?, "text/css; charset=utf-8") }

pub fn png(b: &str) -> Result<Response> { asset(Response::from_bytes(if b == "bubble" { BUBBLE } else { LOGO }.to_vec())?, "image/png") }
