use ab_glyph::{Font, FontRef, GlyphId, PxScale, ScaleFont, point};
use image::{ImageEncoder, RgbImage, codecs::png::{CompressionType, FilterType as PngFilter, PngEncoder}};

pub const W: u32 = 1200;
pub const H: u32 = 630;
const TX: f32 = 640.; // text area x
const TW: f32 = 500.; // text area w
const PAD: f32 = 56.; // top/bottom
pub const IMG_W: u32 = 500;
pub const IMG_H: u32 = 380; // w/o text
const IMG_H_TXT: u32 = 240; // w/ text
const UP: f32 = 2.; // small imgs/gifs get scaled up at most this much
const MAXF: usize = 150; // gif frames decoded at most
const OUTF: usize = 40; // gif frames written at most, rest dropped evenly
const GIF_PX: u64 = 8_000_000; // decode budget, frames * w * h
pub const EMO: u32 = 0xF0000; // custom emoji k = char EMO+k (private use)

static REG: &[u8] = include_bytes!("../assets/serif.ttf");
static ITA: &[u8] = include_bytes!("../assets/serif-i.ttf");

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Theme { Black, White, Color }

impl Theme {
	fn pal(self) -> (u8, u8, u8, u8) { match self { Theme::White => (255, 15, 45, 125), _ => (0, 255, 235, 130) } } // bg, text, name, user
	fn gray(self) -> bool { self != Theme::Color }
}

pub struct Card<'a> {
	pub av: Option<&'a RgbImage>,
	pub text: &'a str,
	pub name: &'a str,
	pub user: &'a str,
	pub reply: Option<&'a str>, // "Replying to @x"
	pub fb: &'a [Vec<u8>], // extra fonts for chars noto serif lacks
	pub emoji: &'a [RgbImage],
	pub theme: Theme,
}

type Bm = (i32, i32, u32, Vec<u8>); // off x, off y, w, coverage

#[derive(Clone, Copy)]
enum Gl { F(usize, GlyphId), E(usize) } // font glyph or custom emoji

struct Fs<'a>(Vec<FontRef<'a>>, &'a [RgbImage]); // 0 reg, 1 ita, 2.. fallbacks; custom emoji

impl Fs<'_> {
	fn pick(&self, fi: usize, c: char) -> Option<Gl> { // emoji, wanted font, then fallbacks, then reg
		let e = (c as u32).wrapping_sub(EMO) as usize;
		if e < self.1.len() { return Some(Gl::E(e)); }
		[fi].into_iter().chain(2..self.0.len()).chain([0]).map(|i| (i, self.0[i].glyph_id(c))).find(|(_, g)| g.0 != 0).map(|(i, g)| Gl::F(i, g))
	}

	fn walk(&self, fi: usize, px: f32, s: &str, mut f: impl FnMut(Gl, f32)) -> f32 { // x of each glyph, returns width. kern only within one font
		let (mut x, mut prev) = (0., None::<(usize, GlyphId)>);
		for c in s.chars() {
			let Some(gl) = self.pick(fi, c) else { continue };
			match gl {
				Gl::E(_) => { f(gl, x);x += px * 1.15;prev = None; }
				Gl::F(i, g) => {
					let sf = self.0[i].as_scaled(PxScale::from(px));
					if let Some((pi, pg)) = prev && pi == i { x += sf.kern(pg, g); }
					f(gl, x);x += sf.h_advance(g);prev = Some((i, g));
				}
			}
		}
		x
	}
}

fn ignorable(c: char) -> bool { c.is_whitespace() || c.is_control() || (c as u32) >= EMO || matches!(c, '\u{200B}'..='\u{200F}' | '\u{2060}'..='\u{2064}' | '\u{FE00}'..='\u{FE0F}' | '\u{E0000}'..='\u{E007F}') }

pub fn lacks(font: &[u8], s: &str) -> String { // chars in s this font can't draw, deduped
	let Ok(f) = FontRef::try_from_slice(font) else { return String::new() };
	let mut out = String::new();
	for c in s.chars() { if !ignorable(c) && f.glyph_id(c).0 == 0 && !out.contains(c) { out.push(c); } }
	out
}

pub fn missing(s: &str) -> String { lacks(REG, s) }

fn luma(p: [u8; 3]) -> u8 { ((p[0] as u32 * 299 + p[1] as u32 * 587 + p[2] as u32 * 114) / 1000) as u8 }

struct Cv { p: Vec<u8>, gc: std::collections::HashMap<(u8, u16, u32), Option<Bm>>, th: Theme } // rgb px, glyph cache by (font, glyph, px)

impl Cv {
	fn put(&mut self, x: i32, y: i32, c: [u8; 3], a: f32) {
		if x < 0 || y < 0 || x >= W as i32 || y >= H as i32 { return; }
		let i = (y as u32 * W + x as u32) as usize * 3;
		for (d, s) in self.p[i..i + 3].iter_mut().zip(c) { *d = (*d as f32 * (1. - a) + s as f32 * a).round() as u8; }
	}

	fn img(&mut self, im: &RgbImage, x: u32, y: u32) {
		let (w, gray) = (im.width() as usize, self.th.gray());
		for (r, row) in im.as_raw().chunks_exact(w * 3).enumerate() {
			if y as usize + r >= H as usize { break; }
			let d = ((y as usize + r) * W as usize + x as usize) * 3;
			let n = (w * 3).min(W as usize * 3 - x as usize * 3);
			self.p[d..d + n].copy_from_slice(&row[..n]);
			if gray { for px in self.p[d..d + n].as_chunks_mut::<3>().0 { let v = luma(*px);*px = [v; 3]; } }
		}
	}

	fn line(&mut self, fs: &Fs, fi: usize, px: f32, s: &str, base: f32, v: u8) { // centered in text area, glyphs snapped to whole px so they can be cached
		let x0 = TX + (TW - width(fs, fi, px, s)) / 2.;
		let mut gs = vec![];fs.walk(fi, px, s, |g, x| gs.push((g, x)));
		for (gl, x) in gs {
			let (gx, gy) = ((x0 + x).round() as i32, base.round() as i32);
			let (i, g) = match gl {
				Gl::E(e) => { let sz = px as u32;let im = scale(&fs.1[e], sz, sz);self.img(&im, (gx.max(0) as u32 + 1).min(W - 1), (gy - (px * 0.86) as i32).max(0) as u32);continue; }
				Gl::F(i, g) => (i, g),
			};
			let k = (i as u8, g.0, px.to_bits());
			let bm = self.gc.entry(k).or_insert_with(|| {
				let o = fs.0[i].outline_glyph(g.with_scale_and_position(px, point(0., 0.)))?;
				let b = o.px_bounds();let w = b.width() as u32;
				let mut cov = vec![0u8; (w * b.height() as u32) as usize];
				o.draw(|x, y, a| cov[(y * w + x) as usize] = (a.min(1.) * 255.) as u8);
				Some((b.min.x as i32, b.min.y as i32, w, cov))
			}).take();
			let Some((ox, oy, w, cov)) = bm else { continue };
			let mut j = 0;while j < cov.len() {
				if cov[j] > 0 { self.put(gx + ox + (j as u32 % w) as i32, gy + oy + (j as u32 / w) as i32, [v; 3], cov[j] as f32 / 255.); }
				j += 1;
			}
			self.gc.insert(k, Some((ox, oy, w, cov)));
		}
	}
}

fn width(fs: &Fs, fi: usize, px: f32, s: &str) -> f32 { fs.walk(fi, px, s, |_, _| {}) }

fn wrap(fs: &Fs, fi: usize, px: f32, s: &str, max: f32) -> Vec<String> { // greedy, breaks long words by char, word widths summed (no kern across spaces)
	let sp = width(fs, fi, px, " ");
	let mut out = vec![];
	for para in s.split('\n') {
		let (mut cur, mut cw) = (String::new(), 0.);
		for w in para.split_whitespace() {
			let ww = width(fs, fi, px, w);
			if cur.is_empty() && ww <= max { cur = w.into();cw = ww;continue; }
			if !cur.is_empty() && cw + sp + ww <= max { cur.push(' ');cur += w;cw += sp + ww;continue; }
			if !cur.is_empty() { out.push(std::mem::take(&mut cur));cw = 0.; }
			if ww <= max { cur = w.into();cw = ww;continue; }
			for c in w.chars() { // too long for a line
				let c_w = width(fs, fi, px, c.encode_utf8(&mut [0; 4]));
				if cw + c_w > max && !cur.is_empty() { out.push(std::mem::take(&mut cur));cw = 0.; }
				cur.push(c);cw += c_w;
			}
		}
		out.push(cur);
	}
	while out.last().is_some_and(|l| l.is_empty()) { out.pop(); }
	out
}

fn fit(fs: &Fs, fi: usize, px: f32, s: &str, max: f32) -> String { // cut + ... to one line
	if width(fs, fi, px, s) <= max { return s.into(); }
	let mut t: String = s.into();
	while !t.is_empty() && width(fs, fi, px, &format!("{t}...")) > max { t.pop(); }
	format!("{}...", t.trim_end())
}

pub fn fit_img(w: u32, h: u32, text: bool) -> (u32, u32) { // into img box, keep ratio, small ones up to UP x
	let (bw, bh) = (IMG_W as f32, if text { IMG_H_TXT } else { IMG_H } as f32);
	let k = (bw / w.max(1) as f32).min(bh / h.max(1) as f32).min(UP);
	(((w as f32 * k) as u32).max(1), ((h as f32 * k) as u32).max(1))
}

fn axis(s: usize, n: usize) -> Vec<(usize, usize, u32)> { // bilinear taps per dst px: src i, i+1, weight/256
	(0..n).map(|d| { let f = ((d as f32 + 0.5) * s as f32 / n as f32 - 0.5).clamp(0., s as f32 - 1.);let i = f as usize;(i, (i + 1).min(s - 1), ((f - i as f32) * 256.) as u32) }).collect()
}

fn scale(im: &RgbImage, w: u32, h: u32) -> RgbImage { // bilinear, fixed point
	if im.dimensions() == (w, h) { return im.clone(); }
	let (sw, sh, w, h) = (im.width() as usize, im.height() as usize, w as usize, h as usize);
	let (xs, ys, src) = (axis(sw, w), axis(sh, h), im.as_raw());
	let mut out = vec![0u8; w * h * 3];
	let mut y = 0;while y < h {
		let (y0, y1, ty) = ys[y];let (r0, r1) = (&src[y0 * sw * 3..][..sw * 3], &src[y1 * sw * 3..][..sw * 3]);
		let mut x = 0;while x < w {
			let (x0, x1, tx) = xs[x];
			let mut c = 0;while c < 3 {
				let top = r0[x0 * 3 + c] as u32 * (256 - tx) + r0[x1 * 3 + c] as u32 * tx;let bot = r1[x0 * 3 + c] as u32 * (256 - tx) + r1[x1 * 3 + c] as u32 * tx;
				out[(y * w + x) * 3 + c] = ((top * (256 - ty) + bot * ty) >> 16) as u8;
				c += 1;
			}
			x += 1;
		}
		y += 1;
	}
	RgbImage::from_raw(w as u32, h as u32, out).unwrap_or_default()
}

fn avatar(cv: &mut Cv, a: &RgbImage) { // to HxH at x=0, smoothstep fade into bg
	let a = scale(a, H, H);
	let (bg, gray, n) = (cv.th.pal().0 as i32, cv.th.gray(), H as usize);
	let fade: Vec<i32> = (0..n).map(|x| { let t = ((x as f32 - 200.) / 430.).clamp(0., 1.);((1. - t * t * (3. - 2. * t)) * 256.) as i32 }).collect();
	for (y, row) in a.as_raw().chunks_exact(n * 3).enumerate() {
		let dst = &mut cv.p[y * W as usize * 3..][..n * 3];
		let mut x = 0;while x < n {
			let s = [row[x * 3], row[x * 3 + 1], row[x * 3 + 2]];
			let s = if gray { [luma(s); 3] } else { s };
			let mut c = 0;while c < 3 { dst[x * 3 + c] = (bg + (((s[c] as i32 - bg) * fade[x]) >> 8)) as u8;c += 1; }
			x += 1;
		}
	}
}

type Rect = (u32, u32, u32, u32); // x, y, w, h

fn compose(c: &Card, img: Option<&RgbImage>) -> (Vec<u8>, Option<Rect>) {
	let fs = Fs([REG, ITA].into_iter().chain(c.fb.iter().map(|b| &b[..])).filter_map(|b| FontRef::try_from_slice(b).ok()).collect(), c.emoji); // bundled 2 can't fail
	let (bg, tc, nc, uc) = c.theme.pal();
	let mut cv = Cv { p: vec![bg; (W * H * 3) as usize], gc: Default::default(), th: c.theme };
	if let Some(a) = c.av { avatar(&mut cv, a); }
	let text = c.text.trim();
	let img = img.map(|i| { let (w, h) = fit_img(i.width(), i.height(), !text.is_empty()); scale(i, w, h) });
	let (np, up, rp) = (30., 22., 20.);
	let foot = np * 1.3 + 8. + up * 1.3; // name + user
	let rh = if c.reply.is_some() { rp * 1.3 + 14. } else { 0. };
	let ih = img.as_ref().map_or(0., |i| i.height() as f32 + if text.is_empty() { 0. } else { 28. });
	let room = H as f32 - PAD * 2. - foot - 32. - ih - rh;
	let (mut px, mut lines) = (64., vec![]);
	while !text.is_empty() {
		lines = wrap(&fs, 0, px, text, TW);
		if lines.len() as f32 * px * 1.3 <= room || px <= 22. { break; }
		px -= 2.;
	}
	let max = (room / (px * 1.3)).floor().max(1.) as usize;
	if lines.len() > max { lines.truncate(max);let l = lines.pop().unwrap_or_default();lines.push(fit(&fs, 0, px, &format!("{l}..."), TW)); }
	let th = lines.len() as f32 * px * 1.3;
	let total = rh + ih + th + if text.is_empty() { 0. } else { 32. } + foot;
	let mut y = ((H as f32 - total) / 2.).max(PAD);
	if let Some(r) = c.reply { let r = fit(&fs, 1, rp, r, TW);cv.line(&fs, 1, rp, &r, y + rp * 1.3 * 0.78, uc);y += rh; }
	let mut rect = None;
	if let Some(i) = &img { let x = (TX + (TW - i.width() as f32) / 2.) as u32;cv.img(i, x, y as u32);rect = Some((x, y as u32, i.width(), i.height()));y += ih; }
	for l in &lines {
		let lh = px * 1.3;
		cv.line(&fs, 0, px, l, y + lh * 0.78, tc);
		y += lh;
	}
	if !text.is_empty() { y += 32.; }
	let n = fit(&fs, 1, np, &format!("- {}", c.name), TW);
	cv.line(&fs, 1, np, &n, y + np * 1.3 * 0.78, nc);
	y += np * 1.3 + 8.;
	let u = fit(&fs, 0, up, c.user, TW);
	cv.line(&fs, 0, up, &u, y + up * 1.3 * 0.78, uc);
	(cv.p, rect)
}

pub fn render(c: &Card, img: Option<&RgbImage>) -> Vec<u8> {
	let (px, _) = compose(c, img);
	let mut out = vec![];
	let enc = PngEncoder::new_with_quality(&mut out, CompressionType::Fast, PngFilter::Sub);
	let _ = if c.theme.gray() { enc.write_image(&px.iter().step_by(3).copied().collect::<Vec<u8>>(), W, H, image::ExtendedColorType::L8) } else { enc.write_image(&px, W, H, image::ExtendedColorType::Rgb8) };
	out
}

pub fn frames(b: &[u8]) -> Vec<(RgbImage, u16)> { // composited gif frames + delay in 1/100s, capped then thinned to OUTF
	let mut o = gif::DecodeOptions::new();o.set_color_output(gif::ColorOutput::Indexed);
	let Ok(mut d) = o.read_info(std::io::Cursor::new(b)) else { return vec![] };
	let (w, h) = (d.width() as usize, d.height() as usize);
	let pal = |p: &[u8]| { let mut l = [0u8; 768];let n = p.len().min(768);l[..n].copy_from_slice(&p[..n]);l };
	let glob = d.global_palette().map(pal).unwrap_or([0; 768]);
	let (mut cv, mut out, mut px) = (vec![0u8; w * h * 3], vec![], 0u64);
	while let Ok(Some(f)) = d.read_next_frame() {
		let l = f.palette.as_deref().map(pal).unwrap_or(glob);
		let (fx, fy, fw, fh) = (f.left as usize, f.top as usize, f.width as usize, f.height as usize);
		let prev = (f.dispose == gif::DisposalMethod::Previous).then(|| cv.clone());
		let mut y = 0;while y < fh {
			if fy + y < h {
				let row = &f.buffer[y * fw..][..fw];
				let mut x = 0;while x < fw && fx + x < w { let i = row[x];if f.transparent != Some(i) { let d = ((fy + y) * w + fx + x) * 3;cv[d..d + 3].copy_from_slice(&l[i as usize * 3..][..3]); } x += 1; }
			}
			y += 1;
		}
		out.push((RgbImage::from_raw(w as u32, h as u32, cv.clone()).unwrap_or_default(), if f.delay < 2 { 10 } else { f.delay })); // <2 = browsers use 10
		match (f.dispose, prev) {
			(gif::DisposalMethod::Background, _) => { let mut y = fy;while y < (fy + fh).min(h) { let e = (fx + fw).min(w);if fx < e { cv[(y * w + fx) * 3..(y * w + e) * 3].fill(0); } y += 1; } }
			(gif::DisposalMethod::Previous, Some(p)) => cv = p,
			_ => {}
		}
		px += (w * h) as u64;
		if out.len() >= MAXF || px >= GIF_PX { break; }
	}
	if out.len() <= OUTF { return out; }
	let k = out.len().div_ceil(OUTF);
	out.chunks(k).map(|c| (c[0].0.clone(), c.iter().map(|f| f.1).sum())).collect()
}

fn quant(p: &[u8], gray: bool) -> Vec<u8> { // rgb -> palette idx. gray: idx = luma. color: 6x6x6 cube, near-gray px -> 40 step ramp at 216..
	p.as_chunks::<3>().0.iter().map(|&c| {
		if gray { return c[0]; }
		let (mx, mn) = (c[0].max(c[1]).max(c[2]), c[0].min(c[1]).min(c[2]));
		if mx - mn < 12 { return 216 + (luma(c) as u32 * 39 / 255) as u8; }
		let q = |v: u8| (v as u32 * 5 + 127) / 255;
		(q(c[0]) * 36 + q(c[1]) * 6 + q(c[2])) as u8
	}).collect()
}

fn palette(gray: bool) -> Vec<u8> {
	if gray { return (0..=255u8).flat_map(|v| [v, v, v]).collect(); }
	let mut p: Vec<u8> = (0..216u32).flat_map(|i| [i / 36, i / 6 % 6, i % 6].map(|v| (v * 51) as u8)).collect();
	p.extend((0..40u32).flat_map(|i| [(i * 255 / 39) as u8; 3]));
	p
}

pub fn render_gif(c: &Card, fr: &[(RgbImage, u16)]) -> Vec<u8> { // frame 0 = whole card, rest only redraw the gif rect
	let (base, rect) = compose(c, fr.first().map(|f| &f.0));
	let gray = c.theme.gray();
	let mut out = vec![];
	{
		let Ok(mut e) = gif::Encoder::new(&mut out, W as u16, H as u16, &palette(gray)) else { return out };
		let _ = e.set_repeat(gif::Repeat::Infinite);
		let _ = e.write_frame(&gif::Frame { width: W as u16, height: H as u16, buffer: quant(&base, gray).into(), delay: fr.first().map_or(10, |f| f.1), dispose: gif::DisposalMethod::Keep, ..Default::default() });
		if let Some((x, y, w, h)) = rect {
			for f in fr.iter().skip(1) {
				let mut s = scale(&f.0, w, h);
				if gray { for px in s.as_mut().as_chunks_mut::<3>().0 { let v = luma(*px);*px = [v; 3]; } }
				let _ = e.write_frame(&gif::Frame { left: x as u16, top: y as u16, width: w as u16, height: h as u16, buffer: quant(s.as_raw(), gray).into(), delay: f.1, dispose: gif::DisposalMethod::Keep, ..Default::default() });
			}
		}
	}
	out
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn cards() { // MIAQ_OUT=dir cargo test to look at them
		let av = RgbImage::from_fn(256, 256, |x, y| image::Rgb([((x ^ y) & 255) as u8, (x & 255) as u8, (y & 255) as u8]));
		let pic = RgbImage::from_fn(800, 600, |x, y| image::Rgb([(x / 4) as u8, (y / 3) as u8, 120]));
		let emo = [RgbImage::from_pixel(64, 64, image::Rgb([250, 200, 0]))];
		let long = "the quick brown fox jumps over the lazy dog ".repeat(30);
		let e = char::from_u32(EMO).unwrap();
		let fb: Vec<Vec<u8>> = std::env::var("MIAQ_FB").map(|v| v.split(',').filter_map(|p| std::fs::read(p).ok()).collect()).unwrap_or_default(); // MIAQ_FB=a.ttf,b.ttf
		let name = if fb.is_empty() { "Some Person" } else { "𝐁𝐮𝐭𝐭𝐞𝐫𝐃𝐞𝐯 𝟐.𝟏 😀" };
		assert!(!missing(name).contains('B') && missing("𝐁 x").chars().count() == 1 && missing(&format!("{e}x")).is_empty());
		let short = format!("never gonna give you up {e}");
		type Case<'a> = (&'a str, Option<&'a RgbImage>, &'a str, Theme, Option<&'a str>);
		let cases: [Case; 6] = [("short", None, &short, Theme::Black, None), ("long", None, &long, Theme::Black, None), ("img", Some(&pic), "look at this", Theme::Black, Some("Replying to @someone")), ("only_img", Some(&pic), "", Theme::Black, None), ("white", Some(&pic), &short, Theme::White, Some("Replying to @someone")), ("color", Some(&pic), &short, Theme::Color, None)];
		for (n, im, t, th, reply) in cases {
			let t0 = std::time::Instant::now();
			let c = Card { av: Some(&av), text: t, name, user: "@someone", reply, fb: &fb, emoji: &emo, theme: th };
			let png = render(&c, im);
			eprintln!("{n}: {:?} {}KB", t0.elapsed(), png.len() / 1024);
			assert_eq!(&png[1..4], b"PNG");
			if let Ok(d) = std::env::var("MIAQ_OUT") { std::fs::write(format!("{d}/{n}.png"), png).unwrap(); }
		}
		if let Ok(g) = std::env::var("MIAQ_GIF") { // MIAQ_GIF=a.gif,b.gif
			for (k, p) in g.split(',').enumerate() {
				let b = std::fs::read(p).unwrap();
				let t0 = std::time::Instant::now();
				let fr = frames(&b);let t1 = t0.elapsed();
				let th = if k == 0 { Theme::Black } else { Theme::Color };
				let c = Card { av: Some(&av), text: if k == 0 { "" } else { "when the cat" }, name, user: "@someone", reply: None, fb: &fb, emoji: &emo, theme: th };
				let out = render_gif(&c, &fr);
				eprintln!("gif{k}: {} frames, decode {t1:?}, total {:?}, {}KB", fr.len(), t0.elapsed(), out.len() / 1024);
				assert_eq!(&out[..3], b"GIF");
				if let Ok(d) = std::env::var("MIAQ_OUT") { std::fs::write(format!("{d}/gif{k}.gif"), out).unwrap(); }
			}
		}
	}
}
