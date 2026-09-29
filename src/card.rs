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
const MAXF: usize = 150; // gif frames decoded at most
const OUTF: usize = 40; // gif frames written at most, rest dropped evenly
const GIF_PX: u64 = 8_000_000; // decode budget, frames * w * h

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

fn axis(s: usize, n: usize) -> Vec<(usize, usize, u32)> { // bilinear taps per dst px: src i, i+1, weight/256
	(0..n).map(|d| { let f = ((d as f32 + 0.5) * s as f32 / n as f32 - 0.5).clamp(0., s as f32 - 1.);let i = f as usize;(i, (i + 1).min(s - 1), ((f - i as f32) * 256.) as u32) }).collect()
}

fn scale(im: &GrayImage, w: u32, h: u32) -> GrayImage { // bilinear, fixed point
	if im.dimensions() == (w, h) { return im.clone(); }
	let (sw, sh, w, h) = (im.width() as usize, im.height() as usize, w as usize, h as usize);
	let (xs, ys, src) = (axis(sw, w), axis(sh, h), im.as_raw());
	let mut out = vec![0u8; w * h];
	let mut y = 0;while y < h {
		let (y0, y1, ty) = ys[y];let (r0, r1) = (&src[y0 * sw..][..sw], &src[y1 * sw..][..sw]);
		let mut x = 0;while x < w {
			let (x0, x1, tx) = xs[x];
			let top = r0[x0] as u32 * (256 - tx) + r0[x1] as u32 * tx;let bot = r1[x0] as u32 * (256 - tx) + r1[x1] as u32 * tx;
			out[y * w + x] = ((top * (256 - ty) + bot * ty) >> 16) as u8;
			x += 1;
		}
		y += 1;
	}
	GrayImage::from_raw(w as u32, h as u32, out).unwrap_or_default()
}

fn avatar(cv: &mut Cv, a: &GrayImage) { // bilinear to HxH at x=0, fixed point, fused w/ smoothstep fade to black
	let (sw, sh, n) = (a.width() as usize, a.height() as usize, H as usize);
	let (xs, ys) = (axis(sw, n), axis(sh, n));
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

type Rect = (u32, u32, u32, u32); // x, y, w, h

fn compose(av: Option<&GrayImage>, img: Option<&GrayImage>, text: &str, name: &str, user: &str, fb: &[Vec<u8>]) -> (Vec<u8>, Option<Rect>) { // fb = extra fonts for chars noto serif lacks
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
	let mut rect = None;
	if let Some(i) = &img { let x = (TX + (TW - i.width() as f32) / 2.) as u32;cv.img(i, x as i32, y as i32);rect = Some((x, y as u32, i.width(), i.height()));y += ih; }
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
	(cv.0, rect)
}

pub fn render(av: Option<&GrayImage>, img: Option<&GrayImage>, text: &str, name: &str, user: &str, fb: &[Vec<u8>]) -> Vec<u8> {
	let (px, _) = compose(av, img, text, name, user, fb);
	let mut out = vec![];
	let _ = PngEncoder::new_with_quality(&mut out, CompressionType::Fast, PngFilter::Sub).write_image(&px, W, H, image::ExtendedColorType::L8);
	out
}

fn lut(p: &[u8]) -> [u8; 256] { // palette rgb -> gray
	let mut l = [0u8; 256];
	for (i, c) in p.as_chunks::<3>().0.iter().take(256).enumerate() { l[i] = ((c[0] as u32 * 299 + c[1] as u32 * 587 + c[2] as u32 * 114) / 1000) as u8; }
	l
}

pub fn frames(b: &[u8]) -> Vec<(GrayImage, u16)> { // composited gif frames as gray + delay in 1/100s, capped then thinned to OUTF
	let mut o = gif::DecodeOptions::new();o.set_color_output(gif::ColorOutput::Indexed);
	let Ok(mut d) = o.read_info(std::io::Cursor::new(b)) else { return vec![] };
	let (w, h) = (d.width() as usize, d.height() as usize);
	let glob = d.global_palette().map(lut).unwrap_or([0; 256]);
	let (mut cv, mut out, mut px) = (vec![0u8; w * h], vec![], 0u64);
	while let Ok(Some(f)) = d.read_next_frame() {
		let l = f.palette.as_deref().map(lut).unwrap_or(glob);
		let (fx, fy, fw, fh) = (f.left as usize, f.top as usize, f.width as usize, f.height as usize);
		let prev = (f.dispose == gif::DisposalMethod::Previous).then(|| cv.clone());
		let mut y = 0;while y < fh {
			if fy + y < h {
				let row = &f.buffer[y * fw..][..fw];
				let mut x = 0;while x < fw && fx + x < w { let i = row[x];if f.transparent != Some(i) { cv[(fy + y) * w + fx + x] = l[i as usize]; } x += 1; }
			}
			y += 1;
		}
		out.push((GrayImage::from_raw(w as u32, h as u32, cv.clone()).unwrap_or_default(), if f.delay < 2 { 10 } else { f.delay })); // <2 = browsers use 10
		match (f.dispose, prev) {
			(gif::DisposalMethod::Background, _) => { let mut y = fy;while y < (fy + fh).min(h) { let e = (fx + fw).min(w);if fx < e { cv[y * w + fx..y * w + e].fill(0); } y += 1; } }
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

pub fn render_gif(av: Option<&GrayImage>, fr: &[(GrayImage, u16)], text: &str, name: &str, user: &str, fb: &[Vec<u8>]) -> Vec<u8> { // frame 0 = whole card, rest only redraw the gif rect
	let (base, rect) = compose(av, fr.first().map(|f| &f.0), text, name, user, fb);
	let pal: Vec<u8> = (0..=255u8).flat_map(|v| [v, v, v]).collect(); // index = gray, no quantizing
	let mut out = vec![];
	{
		let Ok(mut e) = gif::Encoder::new(&mut out, W as u16, H as u16, &pal) else { return out };
		let _ = e.set_repeat(gif::Repeat::Infinite);
		let _ = e.write_frame(&gif::Frame { width: W as u16, height: H as u16, buffer: base.into(), delay: fr.first().map_or(10, |f| f.1), dispose: gif::DisposalMethod::Keep, ..Default::default() });
		if let Some((x, y, w, h)) = rect {
			for f in fr.iter().skip(1) {
				let s = scale(&f.0, w, h);
				let _ = e.write_frame(&gif::Frame { left: x as u16, top: y as u16, width: w as u16, height: h as u16, buffer: s.into_raw().into(), delay: f.1, dispose: gif::DisposalMethod::Keep, ..Default::default() });
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
		if let Ok(g) = std::env::var("MIAQ_GIF") { // MIAQ_GIF=a.gif,b.gif
			for (k, p) in g.split(',').enumerate() {
				let b = std::fs::read(p).unwrap();
				let t0 = std::time::Instant::now();
				let fr = frames(&b);let t1 = t0.elapsed();
				let out = render_gif(Some(&av), &fr, if k == 0 { "" } else { "when the cat" }, name, "@someone", &fb);
				eprintln!("gif{k}: {} frames, decode {t1:?}, total {:?}, {}KB", fr.len(), t0.elapsed(), out.len() / 1024);
				assert_eq!(&out[..3], b"GIF");
				if let Ok(d) = std::env::var("MIAQ_OUT") { std::fs::write(format!("{d}/gif{k}.gif"), out).unwrap(); }
			}
		}
	}
}
