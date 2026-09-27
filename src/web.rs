use worker::{Env, Fetch, Headers, Method, Request, RequestInit, Response, Result, Url, console_error, url::form_urlencoded};

const HEAD: &str = include_str!("../web/partials/head.html");
const FOOT: &str = include_str!("../web/partials/footer.html");
const NAV: &str = include_str!("../web/partials/nav.html");
const HOME: &str = include_str!("../web/templates/index.html");
const DONE: &str = include_str!("../web/templates/done.html");
pub const TERMS: &str = include_str!("../web/templates/terms.html");
pub const PRIVACY: &str = include_str!("../web/templates/privacy.html");
const CSS: &str = include_str!("../web/style.css");
const LOGO: &[u8] = include_bytes!("../web/static/logo.png"); // favicon
const BUBBLE: &[u8] = include_bytes!("../web/static/bubble.png"); // page logo, cropped

pub fn page(t: &str, kv: &[(&str, &str)]) -> Result<Response> { // %%key%% -> val, vals are ours, never user input
	let mut s = t.replace("%%head%%", HEAD).replace("%%footer%%", FOOT).replace("%%nav%%", NAV);
	for (k, v) in kv { s = s.replace(&format!("%%{k}%%"), v); }
	Response::from_html(s)
}

fn auth(app: &str, cb: &str, extra: &[(&str, &str)]) -> Result<String> {
	let q = [("client_id", app), ("response_type", "code"), ("redirect_uri", cb)];
	Ok(Url::parse_with_params("https://discord.com/oauth2/authorize", q.iter().chain(extra))?.into())
}

fn cb(req: &Request) -> Result<String> { Ok(format!("{}/done", req.url()?.origin().ascii_serialization())) }

pub fn home(req: &Request, env: &Env) -> Result<Response> {
	let (app, cb) = (env.var("CLIENT_ID")?.to_string(), cb(req)?);
	let user = auth(&app, &cb, &[("integration_type", "1"), ("scope", "applications.commands")])?;
	let guild = auth(&app, &cb, &[("integration_type", "0"), ("scope", "bot"), ("permissions", "2147600384")])?; // send, embed, attach, history, app cmds
	page(HOME, &[("user", &user), ("guild", &guild)])
}

async fn swap(env: &Env, code: &str, cb: &str) -> Result<()> { // code -> token finishes the install, token itself is dropped
	let (app, sec) = (env.var("CLIENT_ID")?.to_string(), env.secret("CLIENT_SECRET")?.to_string());
	let body = form_urlencoded::Serializer::new(String::new()).extend_pairs([("grant_type", "authorization_code"), ("code", code), ("redirect_uri", cb), ("client_id", &app), ("client_secret", &sec)]).finish();
	let h = Headers::new();h.set("Content-Type", "application/x-www-form-urlencoded")?;
	let mut init = RequestInit::new();init.with_method(Method::Post).with_headers(h).with_body(Some(body.into()));
	let mut r = Fetch::Request(Request::new_with_init("https://discord.com/api/v10/oauth2/token", &init)?).send().await?;
	if r.status_code() != 200 { return Err(format!("oauth2 {}: {}", r.status_code(), r.text().await.unwrap_or_default()).into()); }
	Ok(())
}

pub async fn done(req: &Request, env: &Env) -> Result<Response> { // oauth2 redirect
	let q: Vec<(String, String)> = req.url()?.query_pairs().into_owned().collect();
	let get = |k: &str| q.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
	let ok = match get("code") { Some(c) => swap(env, c, &cb(req)?).await.map_err(|e| console_error!("{e}")).is_ok(), None => false };
	let (t, s) = match () {
		_ if get("error").is_some() => ("Cancelled", "Nothing was added."),
		_ if !ok => ("That didn't work", "Discord didn't finish adding MIAQ. Go back and try again."),
		_ if get("guild_id").is_some() => ("Added", "MIAQ is in your server. Right-click a message, then Apps, then Quote, or use /quote."),
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
