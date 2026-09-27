mod card;
mod mods;
mod web;

use ed25519_dalek::{Signature, VerifyingKey};
use twilight_model::{
	application::interaction::{Interaction, InteractionData, InteractionType},
	http::interaction::{InteractionResponse, InteractionResponseType},
};
use worker::*;

fn key(env: &Env) -> Result<VerifyingKey> {
	let k = env.secret("PUBLIC_KEY").map(|s| s.to_string()).or_else(|_| env.var("PUBLIC_KEY").map(|v| v.to_string()))?;
	hex::decode(k.trim()).ok().and_then(|b| b.try_into().ok()).and_then(|b| VerifyingKey::from_bytes(&b).ok()).ok_or_else(|| "PUBLIC_KEY must be the app's hex public key.".into())
}

fn verify(k: &VerifyingKey, sig: &str, ts: &str, body: &[u8]) -> bool {
	hex::decode(sig).ok().and_then(|s| Signature::from_slice(&s).ok()).is_some_and(|s| k.verify_strict(&[ts.as_bytes(), body].concat(), &s).is_ok())
}

async fn interaction(mut req: Request, cx: &mods::Cx) -> Result<Response> {
	let (sig, ts) = (req.headers().get("X-Signature-Ed25519")?.unwrap_or_default(), req.headers().get("X-Signature-Timestamp")?.unwrap_or_default());
	let body = req.bytes().await?;
	if !verify(&key(&cx.env)?, &sig, &ts, &body) { return Response::error("Bad signature", 401); }
	let i: Interaction = serde_json::from_slice(&body)?;
	let r = match (i.kind, &i.data) {
		(InteractionType::Ping, _) => InteractionResponse { kind: InteractionResponseType::Pong, data: None },
		(InteractionType::ApplicationCommand, Some(InteractionData::ApplicationCommand(d))) => mods::dispatch(cx, &i, d).await,
		_ => return Response::error("Unsupported", 400),
	};
	Response::from_json(&r)
}

#[event(fetch)]
async fn fetch(req: Request, env: Env, wc: Context) -> Result<Response> {
	match (req.method(), req.path().as_str()) {
		(Method::Post, "/") => interaction(req, &mods::Cx { env, wc }).await,
		(Method::Get, "/cmds") => Response::from_json(&mods::cmds()), // what this version registers
		(Method::Get, "/") => web::home(&req, &env),
		(Method::Get, "/done") => web::done(&req, &env).await,
		(Method::Get, "/terms-of-service") => web::page(web::TERMS, &[]),
		(Method::Get, "/privacy-policy") => web::page(web::PRIVACY, &[]),
		(Method::Get, "/style.css") => web::css(),
		(Method::Get, "/logo.png") => web::png("logo"),
		(Method::Get, "/bubble.png") => web::png("bubble"),
		_ => Response::error("Not found", 404),
	}
}
