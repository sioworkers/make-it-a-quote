use ab_glyph::{Font, FontRef, GlyphId, PxScale, ScaleFont, point};
use image::{GrayImage, ImageEncoder, codecs::png::{CompressionType, FilterType as PngFilter, PngEncoder}};

pub const W: u32 = 1200;
pub const H: u32 = 630;
const TX: f32 = 640.; // text area x
const TW: f32 = 500.; // text area w
const PAD: f32 = 56.; // top/bottom
pub const IMG_W: u32 = 500;
pub const IMG_H: u32 = 380; // w/o text
const IMG_H_TXT: u32 = 240; // w/ text

static REG: &[u8] = include_bytes!("../assets/serif.ttf");
static ITA: &[u8] = include_bytes!("../assets/serif-i.ttf");

type Bm = (i32, i32, u32, Vec<u8>); // off x, off y, w, coverage

struct Fs<'a>(Vec<FontRef<'a>>); // 0 reg, 1 ita, 2.. fallbacks

impl Fs<'_> {
	fn pick(&self, fi: usize, c: char) -> Option<(usize, GlyphId)> { // wanted font, then fallbacks, then reg
		[fi].into_iter().chain(2..self.0.len()).chain([0]).map(|i| (i, self.0[i].glyph_id(c))).find(|(_, g)| g.0 != 0)
	}

	fn walk(&self, fi: usize, px: f32, s: &str, mut f: impl FnMut(usize, GlyphId, f32)) -> f32 { // x of each glyph, returns width. kern only within one font
		let (mut x, mut prev) = (0., None::<(usize, GlyphId)>);
		for c in s.chars() {
			let Some((i, g)) = self.pick(fi, c) else { continue };
			let sf = self.0[i].as_scaled(PxScale::from(px));
			if let Some((pi, pg)) = prev && pi == i { x += sf.kern(pg, g); }
			f(i, g, x);x += sf.h_advance(g);prev = Some((i, g));
		}
		x
	}
}

fn ignorable(c: char) -> bool { c.is_whitespace() || c.is_control() || matches!(c, '\u{200B}'..='\u{200F}' | '\u{2060}'..='\u{2064}' | '\u{FE00}'..='\u{FE0F}' | '\u{E0000}'..='\u{E007F}') }

pub fn lacks(font: &[u8], s: &str) -> String { // chars in s this font can't draw, deduped
	let Ok(f) = FontRef::try_from_slice(font) else { return String::new() };
	let mut out = String::new();
	for c in s.chars() { if !ignorable(c) && f.glyph_id(c).0 == 0 && !out.contains(c) { out.push(c); } }
	out
}

pub fn missing(s: &str) -> String { lacks(REG, s) }

struct Cv(Vec<u8>, std::collections::HashMap<(u8, u16, u32), Option<Bm>>); // px, glyph cache by (font, glyph, px)

impl Cv {
	fn put(&mut self, x: i32, y: i32, v: f32, a: f32) {
		if x < 0 || y < 0 || x >= W as i32 || y >= H as i32 { return; }
		let d = &mut self.0[(y as u32 * W + x as u32) as usize];
		*d = (*d as f32 * (1. - a) + v * a).round() as u8;
	}

	fn img(&mut self, im: &GrayImage, x: i32, y: i32) {
		let mut py = 0;while py < im.height() {
			let mut px = 0;while px < im.width() { self.put(x + px as i32, y + py as i32, im.get_pixel(px, py).0[0] as f32, 1.);px += 1; }
			py += 1;
		}
	}

	fn line(&mut self, fs: &Fs, fi: usize, px: f32, s: &str, base: f32, v: f32) { // centered in text area, glyphs snapped to whole px so they can be cached
		let x0 = TX + (TW - width(fs, fi, px, s)) / 2.;
		let mut gs = vec![];fs.walk(fi, px, s, |i, g, x| gs.push((i, g, x)));
		for (i, g, x) in gs {
			let (gx, gy) = ((x0 + x).round() as i32, base.round() as i32);
			let k = (i as u8, g.0, px.to_bits());
			let bm = self.1.entry(k).or_insert_with(|| {
				let o = fs.0[i].outline_glyph(g.with_scale_and_position(px, point(0., 0.)))?;
				let b = o.px_bounds();let w = b.width() as u32;
				let mut cov = vec![0u8; (w * b.height() as u32) as usize];
				o.draw(|x, y, a| cov[(y * w + x) as usize] = (a.min(1.) * 255.) as u8);
				Some((b.min.x as i32, b.min.y as i32, w, cov))
			}).take();
			let Some((ox, oy, w, cov)) = bm else { continue };
			let mut j = 0;while j < cov.len() {
				if cov[j] > 0 { self.put(gx + ox + (j as u32 % w) as i32, gy + oy + (j as u32 / w) as i32, v, cov[j] as f32 / 255.); }
				j += 1;
			}
			self.1.insert(k, Some((ox, oy, w, cov)));
		}
	}
}

fn width(fs: &Fs, fi: usize, px: f32, s: &str) -> f32 { fs.walk(fi, px, s, |_, _, _| {}) }

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

pub fn fit_img(w: u32, h: u32, text: bool) -> (u32, u32) { // scale down into img box, keep ratio
	let (bw, bh) = (IMG_W as f32, if text { IMG_H_TXT } else { IMG_H } as f32);
	let k = (bw / w.max(1) as f32).min(bh / h.max(1) as f32).min(1.);
	(((w as f32 * k) as u32).max(1), ((h as f32 * k) as u32).max(1))
}

fn scale(im: &GrayImage, w: u32, h: u32) -> GrayImage { // bilinear
	if im.dimensions() == (w, h) { return im.clone(); }
	let (sw, sh) = (im.width() as f32, im.height() as f32);
	let (kx, ky) = (sw / w as f32, sh / h as f32);
	let src = im.as_raw();
	let at = |x: usize, y: usize| src[y * im.width() as usize + x] as f32;
	GrayImage::from_fn(w, h, |x, y| {
		let (fx, fy) = (((x as f32 + 0.5) * kx - 0.5).clamp(0., sw - 1.), ((y as f32 + 0.5) * ky - 0.5).clamp(0., sh - 1.));
		let (x0, y0) = (fx as usize, fy as usize);let (x1, y1) = ((x0 + 1).min(sw as usize - 1), (y0 + 1).min(sh as usize - 1));
		let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
		let top = at(x0, y0) * (1. - tx) + at(x1, y0) * tx;let bot = at(x0, y1) * (1. - tx) + at(x1, y1) * tx;
		image::Luma([(top * (1. - ty) + bot * ty) as u8])
	})
}

fn avatar(cv: &mut Cv, a: &GrayImage) { // bilinear to HxH at x=0, fixed point, fused w/ smoothstep fade to black
	let (sw, sh, n) = (a.width() as usize, a.height() as usize, H as usize);
	let axis = |s: usize| -> Vec<(usize, usize, u32)> { (0..n).map(|d| { let f = ((d as f32 + 0.5) * s as f32 / n as f32 - 0.5).clamp(0., s as f32 - 1.);let i = f as usize;(i, (i + 1).min(s - 1), ((f - i as f32) * 256.) as u32) }).collect() };
	let (xs, ys) = (axis(sw), axis(sh));
	let fade: Vec<u32> = (0..n).map(|x| { let t = ((x as f32 - 200.) / 430.).clamp(0., 1.);((1. - t * t * (3. - 2. * t)) * 256.) as u32 }).collect();
	let src = a.as_raw();
	let mut y = 0;while y < n {
		let (y0, y1, ty) = ys[y];let (r0, r1) = (&src[y0 * sw..][..sw], &src[y1 * sw..][..sw]);
		let dst = &mut cv.0[y * W as usize..][..n];
		let mut x = 0;while x < n {
			let (x0, x1, tx) = xs[x];
			let top = r0[x0] as u32 * (256 - tx) + r0[x1] as u32 * tx;let bot = r1[x0] as u32 * (256 - tx) + r1[x1] as u32 * tx;
			dst[x] = ((((top * (256 - ty) + bot * ty) >> 16) * fade[x]) >> 8) as u8;
			x += 1;
		}
		y += 1;
	}
}

pub fn render(av: Option<&GrayImage>, img: Option<&GrayImage>, text: &str, name: &str, user: &str, fb: &[Vec<u8>]) -> Vec<u8> { // fb = extra fonts for chars noto serif lacks
	let fs = Fs([REG, ITA].into_iter().chain(fb.iter().map(|b| &b[..])).filter_map(|b| FontRef::try_from_slice(b).ok()).collect()); // bundled 2 can't fail
	let mut cv = Cv(vec![0; (W * H) as usize], Default::default());
	if let Some(a) = av {
		avatar(&mut cv, a);
	}
	let text = text.trim();
	let img = img.map(|i| { let (w, h) = fit_img(i.width(), i.height(), !text.is_empty()); scale(i, w, h) });
	let (np, up) = (30., 22.);
	let foot = np * 1.3 + 8. + up * 1.3; // name + user
	let ih = img.as_ref().map_or(0., |i| i.height() as f32 + if text.is_empty() { 0. } else { 28. });
	let room = H as f32 - PAD * 2. - foot - 32. - ih;
	let (mut px, mut lines) = (64., vec![]);
	while !text.is_empty() {
		lines = wrap(&fs, 0, px, text, TW);
		if lines.len() as f32 * px * 1.3 <= room || px <= 22. { break; }
		px -= 2.;
	}
	let max = (room / (px * 1.3)).floor().max(1.) as usize;
	if lines.len() > max { lines.truncate(max);let l = lines.pop().unwrap_or_default();lines.push(fit(&fs, 0, px, &format!("{l}..."), TW)); }
	let th = lines.len() as f32 * px * 1.3;
	let total = ih + th + if text.is_empty() { 0. } else { 32. } + foot;
	let mut y = ((H as f32 - total) / 2.).max(PAD);
	if let Some(i) = &img { cv.img(i, (TX + (TW - i.width() as f32) / 2.) as i32, y as i32);y += ih; }
	for l in &lines {
		let lh = px * 1.3;
		cv.line(&fs, 0, px, l, y + lh * 0.78, 255.);
		y += lh;
	}
	if !text.is_empty() { y += 32.; }
	let n = fit(&fs, 1, np, &format!("- {name}"), TW);
	cv.line(&fs, 1, np, &n, y + np * 1.3 * 0.78, 235.);
	y += np * 1.3 + 8.;
	let u = fit(&fs, 0, up, user, TW);
	cv.line(&fs, 0, up, &u, y + up * 1.3 * 0.78, 130.);
	let mut out = vec![];
	let _ = PngEncoder::new_with_quality(&mut out, CompressionType::Fast, PngFilter::Sub).write_image(&cv.0, W, H, image::ExtendedColorType::L8);
	out
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn cards() { // MIAQ_OUT=dir cargo test to look at them
		let av = GrayImage::from_fn(256, 256, |x, y| image::Luma([((x ^ y) & 255) as u8]));
		let pic = GrayImage::from_fn(800, 600, |x, _| image::Luma([(x / 4) as u8]));
		let long = "the quick brown fox jumps over the lazy dog ".repeat(30);
		let cases: [(&str, Option<&GrayImage>, &str); 4] = [("short", None, "never gonna give you up"), ("long", None, &long), ("img", Some(&pic), "look at this"), ("only_img", Some(&pic), "")];
		let fb: Vec<Vec<u8>> = std::env::var("MIAQ_FB").map(|v| v.split(',').filter_map(|p| std::fs::read(p).ok()).collect()).unwrap_or_default(); // MIAQ_FB=a.ttf,b.ttf
		let name = if fb.is_empty() { "Some Person" } else { "𝐁𝐮𝐭𝐭𝐞𝐫𝐃𝐞𝐯 𝟐.𝟏 😀" };
		assert!(!missing(name).contains('B') && missing("𝐁 x").chars().count() == 1);
		for (n, im, t) in cases {
			let t0 = std::time::Instant::now();
			let png = render(Some(&av), im, t, name, "@someone", &fb);
			eprintln!("{n}: {:?} {}KB", t0.elapsed(), png.len() / 1024);
			assert_eq!(&png[1..4], b"PNG");
			if let Ok(d) = std::env::var("MIAQ_OUT") { std::fs::write(format!("{d}/{n}.png"), png).unwrap(); }
		}
	}
}
