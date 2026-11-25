use crate::picture::Picture;
use crate::track::{FrameData, P};
use std::fmt::Write;

#[derive(Clone)]
pub struct SrcLine {
    pub index: usize,
    pub first: usize,
    pub end: usize,
    head: String,
    mid: String,
    pub text: String,
    scale_x: f64,
    scale_y: f64,
    angle: f64,
    outline: Option<f64>,
    shadow: Option<f64>,
}

impl SrcLine {
    pub fn standalone(text: &str, style: &str) -> Self {
        SrcLine {
            index: 0,
            first: 0,
            end: usize::MAX,
            head: "Dialogue: 0".into(),
            mid: format!("{style},,0,0,0,"),
            text: text.into(),
            scale_x: 100.,
            scale_y: 100.,
            angle: 0.,
            outline: None,
            shadow: None,
        }
    }

    pub fn with_text(&self, text: &str) -> Self {
        SrcLine { first: 0, end: usize::MAX, text: text.into(), ..self.clone() }
    }

    pub fn parse(fields: &str) -> Option<Self> {
        let f: Vec<&str> = fields.splitn(9, '\t').collect();
        let num = |i: usize| f.get(i)?.parse::<f64>().ok();
        let mut raw = f.get(8)?.splitn(10, ',');
        let head = raw.next()?.to_string();
        let mid = raw.by_ref().skip(2).take(6).collect::<Vec<_>>().join(",");
        Some(SrcLine {
            index: f[0].parse().ok()?,
            first: num(1)? as usize,
            end: num(2)? as usize,
            head,
            mid,
            text: raw.next()?.to_string(),
            scale_x: num(3)?,
            scale_y: num(4)?,
            angle: num(5)?,
            outline: num(6).filter(|v| *v > 0.),
            shadow: num(7).filter(|v| *v > 0.),
        })
    }
}

pub struct OutLine {
    pub added: bool,
    pub src: usize,
    pub start: i64,
    pub end: i64,
    pub text: String,
}

pub struct Export<'a> {
    pub frames: &'a [Option<FrameData>],
    pub ref_pts: &'a [P],
    pub warp: bool,
    pub times: &'a [i64],
    pub scale: [f64; 2],
    pub mask: &'a [P],
    pub anchor: P,
    pub own_pos: bool,
    pub add_text: bool,
    pub color: [u8; 3],
    pub gradients: &'a [Gradient],
    pub bands: usize,
    pub picture: Option<&'a Picture>,
    pub blur: f32,
    pub mask_opacity: bool,
    pub text_opacity: bool,
    pub mask_follow: Follow,
    pub text_follow: Follow,
    pub merge: bool,
}

#[derive(Clone, Copy)]
pub struct Gradient {
    pub color: [u8; 3],
    pub angle: f32,
    pub force: f32,
}

pub struct Layer {
    pub loops: Vec<Vec<P>>,
    pub color: [u8; 3],
    pub solid: f32,
}

fn cut(poly: &[P], d: P, t: f32) -> Vec<P> {
    let side = |p: P| p[0] * d[0] + p[1] * d[1] - t;
    let mut out = vec![];
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        let (sa, sb) = (side(a), side(b));
        if sa >= 0. {
            out.push(a);
        }
        if (sa >= 0.) != (sb >= 0.) {
            let k = sa / (sa - sb);
            out.push([a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k]);
        }
    }
    out
}

pub fn layers(mask: &[P], color: [u8; 3], gradients: &[Gradient], bands: usize, picture: Option<&Picture>) -> Vec<Layer> {
    if let Some(pic) = picture {
        let b = mask.iter().fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |b, p| [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])]);
        let fit = |p: &P| [b[0] + p[0] / pic.w as f32 * (b[2] - b[0]), b[1] + p[1] / pic.h as f32 * (b[3] - b[1])];
        return pic.layers.iter().map(|(color, loops)| Layer { loops: loops.iter().map(|l| l.iter().map(fit).collect()).collect(), color: *color, solid: 1. }).collect();
    }
    let mut out = vec![Layer { loops: vec![mask.to_vec()], color, solid: 1. }];
    for g in gradients {
        let d = [g.angle.to_radians().cos(), g.angle.to_radians().sin()];
        let (lo, hi) = mask.iter().map(|p| p[0] * d[0] + p[1] * d[1]).fold((f32::MAX, f32::MIN), |m, v| (m.0.min(v), m.1.max(v)));
        let mut under = 0f32;
        for k in 0..bands {
            let total = g.force * (k + 1) as f32 / bands as f32;
            let solid = 1. - (1. - total) / (1. - under).max(1e-6);
            under = total;
            let part = cut(mask, d, lo + (hi - lo) * k as f32 / bands as f32);
            if part.len() >= 3 && solid > 0.002 {
                out.push(Layer { loops: vec![part], color: g.color, solid });
            }
        }
    }
    out
}

#[derive(Clone, Copy, PartialEq)]
pub struct Follow {
    pub translation: bool,
    pub scale: bool,
    pub rotation: bool,
    pub deform: bool,
}

impl Follow {
    pub const ALL: Follow = Follow { translation: true, scale: true, rotation: true, deform: true };
}

fn turn(d: P, a: f32) -> P {
    [d[0] * a.cos() - d[1] * a.sin(), d[0] * a.sin() + d[1] * a.cos()]
}

pub fn centroid(pts: &[P]) -> P {
    let n = pts.len().max(1) as f32;
    pts.iter().fold([0., 0.], |c, p| [c[0] + p[0] / n, c[1] + p[1] / n])
}

pub fn place(f: &FrameData, ref_pts: &[P], warp: bool, c: P, fol: Follow, v: P) -> P {
    let (pc, sx, sy, th) = local_motion(f, ref_pts, warp, c, 1.);
    let (s, th) = ((sx * sy).sqrt() as f32, th as f32);
    let mut d = if fol.deform {
        let q = f.map(ref_pts, warp, v);
        [q[0] - pc[0], q[1] - pc[1]]
    } else {
        [v[0] - c[0], v[1] - c[1]]
    };
    if fol.deform != fol.scale {
        let k = if fol.scale { s } else { 1. / s };
        d = [d[0] * k, d[1] * k];
    }
    if fol.deform != fol.rotation {
        d = turn(d, if fol.rotation { th } else { -th });
    }
    let o = if fol.translation { pc } else { c };
    [o[0] + d[0], o[1] + d[1]]
}

pub fn unplace(f: &FrameData, ref_pts: &[P], warp: bool, c: P, fol: Follow, q: P) -> P {
    let (pc, sx, sy, th) = local_motion(f, ref_pts, warp, c, 1.);
    let (s, th) = ((sx * sy).sqrt() as f32, th as f32);
    let o = if fol.translation { pc } else { c };
    let mut d = [q[0] - o[0], q[1] - o[1]];
    if fol.deform != fol.rotation {
        d = turn(d, if fol.rotation { -th } else { th });
    }
    if fol.deform != fol.scale {
        let k = if fol.scale { 1. / s } else { s };
        d = [d[0] * k, d[1] * k];
    }
    if fol.deform { f.unmap(ref_pts, warp, [pc[0] + d[0], pc[1] + d[1]]) } else { [c[0] + d[0], c[1] + d[1]] }
}

fn cs(ms: i64) -> i64 {
    ms.max(0) / 10 * 10
}

fn num(v: f64, dec: usize) -> String {
    let s = format!("{v:.dec$}");
    let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.') } else { &s };
    if s == "-0" { "0".into() } else { s.into() }
}

impl Export<'_> {
    fn each(&self) -> impl Iterator<Item = (usize, i64, i64, &FrameData)> {
        self.frames.iter().enumerate().filter_map(|(i, f)| Some((i, cs(*self.times.get(i)?), cs(*self.times.get(i + 1)?), f.as_ref()?)))
    }

    fn alpha(&self, on: bool, f: &FrameData) -> Option<u8> {
        on.then_some((255. * (1. - relative_opacity(self.frames, f))).round() as u8)
    }

    pub fn mask_lines(&self, src: usize) -> Vec<OutLine> {
        let centre = centroid(self.mask);
        let mut all = vec![];
        for layer in layers(self.mask, self.color, self.gradients, self.bands, self.picture) {
            let [r, g, b] = layer.color;
            let (mut out, mut run) = (vec![], None);
            for (i, start, end, f) in self.each() {
                let mut t = format!("{{\\an7\\pos(0,0)\\fscx100\\fscy100\\frz0\\bord0\\shad0\\1c&H{b:02X}{g:02X}{r:02X}&");
                if self.blur > 0. {
                    write!(t, "\\blur{}", num(self.blur as f64, 2)).unwrap();
                }
                let seen = layer.solid * if self.mask_opacity { relative_opacity(self.frames, f) } else { 1. };
                let alpha = (255. * (1. - seen)).round() as u8;
                if self.mask_opacity || layer.solid < 1. {
                    write!(t, "\\alpha&H{alpha:02X}&").unwrap();
                }
                t.push_str("\\p4}");
                let mut key = vec![alpha as f64 / STILL_ALPHA];
                for (n, lp) in layer.loops.iter().enumerate() {
                    for (k, v) in lp.iter().enumerate() {
                        let q = place(f, self.ref_pts, self.warp, centre, self.mask_follow, *v);
                        let (x, y) = (q[0] as f64 * self.scale[0], q[1] as f64 * self.scale[1]);
                        let cmd = match (n, k) {
                            (0, 0) => "m ",
                            (_, 0) => " m ",
                            (_, 1) => " l ",
                            _ => " ",
                        };
                        write!(t, "{cmd}{} {}", (x * 8.).round(), (y * 8.).round()).unwrap();
                        key.extend([x / STILL_PX, y / STILL_PX]);
                    }
                }
                keep(self.merge, &mut out, &mut run, i, key, OutLine { added: true, src, start, end, text: t });
            }
            all.extend(out);
        }
        all
    }

    pub fn text_lines(&self, lines: &[SrcLine]) -> Vec<OutLine> {
        let mut out = vec![];
        for l in lines {
            let mut run = None;
            let own = line_pos(&l.text).filter(|_| self.own_pos).map(|(x, y)| [(x / self.scale[0]) as f32, (y / self.scale[1]) as f32]);
            let anchor = own.unwrap_or(self.anchor);
            for (i, start, end, f) in self.each().filter(|(i, ..)| (l.first..l.end).contains(i)) {
                let (a, sx, sy, r) = local_motion(f, self.ref_pts, self.warp, anchor, self.scale[0] / self.scale[1]);
                let fol = self.text_follow;
                let a = if fol.translation { a } else { anchor };
                let (x, y) = (a[0] as f64 * self.scale[0], a[1] as f64 * self.scale[1]);
                let even = (sx * sy).sqrt();
                let size = fol.scale.then_some(if fol.deform { (sx, sy) } else { (even, even) });
                let rot = fol.rotation.then_some(r.to_degrees());
                let alpha = self.alpha(self.text_opacity, f);
                let (kx, ky) = size.unwrap_or((1., 1.));
                let key = vec![x / STILL_PX, y / STILL_PX, kx * 100. / STILL_SCALE, ky * 100. / STILL_SCALE, rot.unwrap_or(0.) / STILL_ROT, alpha.unwrap_or(0) as f64 / STILL_ALPHA];
                keep(self.merge, &mut out, &mut run, i, key, OutLine { added: self.add_text, src: l.index, start, end, text: retag(l, Some((x, y)), size, rot, alpha) });
            }
        }
        out
    }
}

const STILL_PX: f64 = 0.15;
const STILL_SCALE: f64 = 0.05;
const STILL_ROT: f64 = 0.03;
const STILL_ALPHA: f64 = 2.;

fn keep(merge: bool, out: &mut Vec<OutLine>, run: &mut Option<(usize, Vec<f64>)>, i: usize, key: Vec<f64>, line: OutLine) {
    if let (Some((last, first)), Some(prev)) = (run.as_mut(), out.last_mut()) {
        if merge && *last + 1 == i && first.len() == key.len() && first.iter().zip(&key).all(|(a, b)| (a - b).abs() <= 1.) {
            (*last, prev.end) = (i, line.end);
            return;
        }
    }
    *run = Some((i, key));
    out.push(line);
}

pub fn relative_opacity(frames: &[Option<FrameData>], f: &FrameData) -> f32 {
    let peak = frames.iter().flatten().fold(1e-3f32, |m, f| m.max(f.opacity));
    let o = f.opacity / peak;
    if o > 0.92 { 1. } else { o }
}

pub fn local_motion(f: &FrameData, ref_pts: &[P], warp: bool, anchor: P, k: f64) -> (P, f64, f64, f64) {
    let m = |dx: f32, dy: f32| f.map(ref_pts, warp, [anchor[0] + dx, anchor[1] + dy]);
    let (px, mx, py, my) = (m(1., 0.), m(-1., 0.), m(0., 1.), m(0., -1.));
    let (j00, j10) = ((px[0] - mx[0]) as f64 / 2., (px[1] - mx[1]) as f64 / 2. / k);
    let (j01, j11) = ((py[0] - my[0]) as f64 / 2. * k, (py[1] - my[1]) as f64 / 2.);
    (m(0., 0.), j00.hypot(j10), j01.hypot(j11), (j10 - j01).atan2(j00 + j11))
}

fn first_block(text: &str) -> (Vec<&str>, &str) {
    let Some((inner, rest)) = text.strip_prefix('{').and_then(|t| t.split_once('}')) else {
        return (vec![], text);
    };
    let (mut toks, mut depth, mut start) = (vec![], 0i32, 0);
    for (i, c) in inner.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            '\\' if depth <= 0 && i > start => {
                toks.push(&inner[start..i]);
                start = i;
            }
            _ => {}
        }
    }
    toks.push(&inner[start..]);
    (toks, rest)
}

fn tag_num(tag: &str, name: &str) -> Option<f64> {
    tag.strip_prefix(name)?.trim().parse().ok()
}

pub fn line_pos(text: &str) -> Option<(f64, f64)> {
    first_block(text).0.iter().find_map(|t| {
        let args = t.strip_prefix("\\pos(").or_else(|| t.strip_prefix("\\move("))?;
        let mut it = args.trim_end_matches(')').split(',').map(|v| v.trim().parse::<f64>());
        Some((it.next()?.ok()?, it.next()?.ok()?))
    })
}

pub fn strip_tags(text: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in text.chars() {
        match c {
            '{' => depth += 1,
            '}' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.replace("\\N", " ")
}

fn retag(l: &SrcLine, pos: Option<(f64, f64)>, size: Option<(f64, f64)>, rot: Option<f64>, alpha: Option<u8>) -> String {
    let (toks, rest) = first_block(&l.text);
    let (mut fx, mut fy, mut fr, mut bord, mut shad) = (l.scale_x, l.scale_y, l.angle, l.outline, l.shadow);
    let mut keep = String::new();
    for t in toks {
        let b = t.strip_prefix('\\').unwrap_or("");
        let replaced = if b.starts_with("pos(") || b.starts_with("move(") {
            pos.is_some()
        } else if let Some(v) = tag_num(b, "fscx") {
            fx = v;
            size.is_some()
        } else if let Some(v) = tag_num(b, "fscy") {
            fy = v;
            size.is_some()
        } else if let Some(v) = tag_num(b, "frz").or_else(|| tag_num(b, "fr")) {
            fr = v;
            rot.is_some()
        } else if let Some(v) = tag_num(b, "bord") {
            bord = Some(v);
            size.is_some()
        } else if let Some(v) = tag_num(b, "shad") {
            shad = Some(v);
            size.is_some()
        } else {
            b.starts_with("alpha") && alpha.is_some()
        };
        if !replaced {
            keep.push_str(t);
        }
    }
    let mut o = String::from("{");
    if let Some((x, y)) = pos {
        write!(o, "\\pos({},{})", num(x, 2), num(y, 2)).unwrap();
    }
    if let Some((sx, sy)) = size {
        write!(o, "\\fscx{}\\fscy{}", num(fx * sx, 2), num(fy * sy, 2)).unwrap();
        for (name, v) in [("bord", bord), ("shad", shad)] {
            if let Some(v) = v {
                write!(o, "\\{name}{}", num(v * (sx * sy).sqrt(), 2)).unwrap();
            }
        }
    }
    if let Some(r) = rot {
        write!(o, "\\frz{}", num(fr - r, 3)).unwrap();
    }
    if let Some(a) = alpha {
        write!(o, "\\alpha&H{a:02X}&").unwrap();
    }
    o + &keep + "}" + rest
}

fn stamp(ms: i64) -> String {
    format!("{}:{:02}:{:02}.{:02}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms / 10 % 100)
}

pub fn clipboard(out: &[OutLine], lines: &[SrcLine], timestamps: bool) -> String {
    let t0 = if timestamps { 0 } else { out.iter().map(|o| o.start).min().unwrap_or(0) };
    out.iter()
        .filter_map(|o| {
            let l = lines.iter().find(|l| l.index == o.src)?;
            Some(format!("{},{},{},{},{}\n", l.head, stamp(o.start - t0), stamp(o.end - t0), l.mid, o.text))
        })
        .collect()
}

pub fn result_file(out: &[OutLine]) -> String {
    let mut s = String::new();
    for o in out {
        writeln!(s, "{}\t{}\t{}\t{}\t{}", if o.added { "mask" } else { "text" }, o.src, o.start, o.end, o.text).unwrap();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::{mul, translation, IDENT};

    fn export<'a>(frames: &'a [Option<FrameData>], times: &'a [i64], mask: &'a [P]) -> Export<'a> {
        Export { frames, ref_pts: &[], warp: false, times, scale: [2., 2.], mask, anchor: [100., 50.], own_pos: false, add_text: false, color: [0x11, 0x22, 0x33], gradients: &[], bands: 4, picture: None, blur: 0., mask_opacity: false, text_opacity: false, mask_follow: Follow::ALL, text_follow: Follow::ALL, merge: true }
    }

    fn fd(h: [f64; 9]) -> Option<FrameData> {
        Some(FrameData { h, pts: vec![], opacity: 0.5, guessed: false })
    }

    #[test]
    fn text_gets_tracked_tags_and_keeps_the_rest() {
        let rot = mul(&translation(110., 50.), &mul(&[0., -1.5, 0., 1.5, 0., 0., 0., 0., 1.], &translation(-100., -50.)));
        let frames = [fd(IDENT), fd(rot), Some(FrameData { h: IDENT, pts: vec![], opacity: 1., guessed: false })];
        let line = SrcLine::parse("7\t0\t2\t100\t80\t10\t2\t1\tDialogue: 3,0:00:00.00,0:00:05.00,Sign,Bob,1,2,3,fx,{\\fscx200\\pos(5,5)\\c&HFF&\\t(0,100,\\fs50)}Sal{\\i1}ut, toi").unwrap();
        let mut e = export(&frames, &[0, 41, 83, 125], &[]);
        (e.mask_opacity, e.text_opacity) = (true, true);
        let out = e.text_lines(&[line.clone()]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].text, "{\\pos(200,100)\\fscx200\\fscy80\\bord2\\shad1\\frz10\\alpha&H80&\\c&HFF&\\t(0,100,\\fs50)}Sal{\\i1}ut, toi");
        assert_eq!(out[1].text, "{\\pos(220,100)\\fscx300\\fscy120\\bord3\\shad1.5\\frz-80\\alpha&H80&\\c&HFF&\\t(0,100,\\fs50)}Sal{\\i1}ut, toi");
        assert_eq!((out[1].src, out[1].start, out[1].end), (7, 40, 80));
        assert_eq!(clipboard(&out[1..], &[line], true), "Dialogue: 3,0:00:00.04,0:00:00.08,Sign,Bob,1,2,3,fx,".to_string() + &out[1].text + "\n");
        assert_eq!(line_pos("{\\an8\\move(12.5, 3,40,40)}x"), Some((12.5, 3.)));
        assert_eq!(strip_tags("{\\an8}Sal{\\i1}ut\\Ntoi"), "Salut toi");
    }

    #[test]
    fn tracking_alone_moves_each_line_from_its_own_position() {
        let frames = [fd(translation(3., 0.))];
        let mut e = export(&frames, &[0, 41], &[]);
        e.own_pos = true;
        let placed = SrcLine::parse("1\t0\t1\t100\t100\t0\t0\t0\tDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{\\pos(40,20)}a").unwrap();
        let out = e.text_lines(&[placed, SrcLine::standalone("", "Default")]);
        assert_eq!(out[0].text, "{\\pos(46,20)\\fscx100\\fscy100\\frz0}a");
        assert_eq!(out[1].text, "{\\pos(206,100)\\fscx100\\fscy100\\frz0}");
    }

    #[test]
    fn a_gradient_is_a_stack_of_nested_shapes() {
        let frames = [fd(IDENT)];
        let mask = [[0., 0.], [10., 0.], [10., 10.], [0., 10.]];
        let fade = [Gradient { color: [255, 0, 0], angle: 0., force: 1. }];
        let mut e = export(&frames, &[0, 41], &mask);
        e.gradients = &fade;
        let out = e.mask_lines(0);
        let tail = |i: usize| out[i].text.split_once("\\1c").unwrap().1.to_string();
        assert_eq!(out.len(), 5);
        assert_eq!(tail(0), "&H332211&\\p4}m 0 0 l 160 0 160 160 0 160");
        assert_eq!(tail(1), "&H0000FF&\\alpha&HBF&\\p4}m 0 0 l 160 0 160 160 0 160");
        assert_eq!(tail(3), "&H0000FF&\\alpha&H80&\\p4}m 80 0 l 160 0 160 160 80 160");
        assert_eq!(tail(4), "&H0000FF&\\p4}m 120 0 l 160 0 160 160 120 160");
    }

    #[test]
    fn typed_text_is_added_next_to_the_selection() {
        let frames = [fd(IDENT)];
        let mut e = export(&frames, &[0, 41], &[]);
        e.add_text = true;
        let picked = SrcLine::parse("9\t5\t6\t100\t100\t0\t0\t0\tDialogue: 0,0:00:00.00,0:00:01.00,Sign,,0,0,0,,vieux").unwrap();
        let out = e.text_lines(&[picked.with_text("neuf")]);
        assert_eq!(result_file(&out), "mask\t9\t0\t40\t{\\pos(200,100)\\fscx100\\fscy100\\frz0}neuf\n");
    }

    #[test]
    fn only_frames_inside_the_line_and_only_enabled_tags() {
        let frames = [fd(IDENT), fd(IDENT), fd(IDENT)];
        let mut line = SrcLine::standalone("Texte", "Default");
        (line.first, line.end) = (1, 2);
        let mut e = export(&frames, &[0, 41, 83, 125], &[]);
        (e.text_follow.scale, e.text_follow.rotation) = (false, false);
        let out = e.text_lines(&[line]);
        assert_eq!(out.len(), 1);
        assert_eq!((out[0].start, out[0].text.as_str()), (40, "{\\pos(200,100)}Texte"));
    }

    #[test]
    fn mask_is_a_drawing_per_frame() {
        let frames = [None, fd(translation(1., 0.5))];
        let mask = [[0., 0.], [10., 0.], [10., 10.], [0., 10.]];
        let mut e = export(&frames, &[1000, 1041, 1083], &mask);
        e.blur = 1.5;
        let out = e.mask_lines(4);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "{\\an7\\pos(0,0)\\fscx100\\fscy100\\frz0\\bord0\\shad0\\1c&H332211&\\blur1.5\\p4}m 16 8 l 176 8 176 168 16 168");
        let lines = [SrcLine { index: 4, ..SrcLine::standalone("", "Default") }];
        assert!(clipboard(&out, &lines, true).starts_with("Dialogue: 0,0:00:01.04,0:00:01.08,Default,,0,0,0,,{"));
        assert!(clipboard(&out, &lines, false).starts_with("Dialogue: 0,0:00:00.00,0:00:00.04,Default,,0,0,0,,{"));
        assert_eq!(result_file(&out), format!("mask\t4\t1040\t1080\t{}\n", out[0].text));
    }

    #[test]
    fn frames_that_stay_still_share_one_line() {
        let frames = [fd(IDENT), fd(translation(0.03, -0.02)), fd(IDENT), fd(translation(5., 0.)), fd(translation(5., 0.)), None, fd(translation(5., 0.))];
        let times = [0, 40, 80, 120, 160, 200, 240, 280];
        let mask = [[0., 0.], [10., 0.], [10., 10.]];
        let e = export(&frames, &times, &mask);
        let spans = |out: Vec<OutLine>| out.iter().map(|o| (o.start, o.end)).collect::<Vec<_>>();
        assert_eq!(spans(e.mask_lines(0)), [(0, 120), (120, 200), (240, 280)]);
        let text = e.text_lines(&[SrcLine::standalone("T", "Default")]);
        assert_eq!(text[0].text, "{\\pos(200,100)\\fscx100\\fscy100\\frz0}T");
        assert_eq!(spans(text), [(0, 120), (120, 200), (240, 280)]);
        let two = [SrcLine { end: 2, ..SrcLine::standalone("A", "Default") }, SrcLine { first: 2, ..SrcLine::standalone("B", "Default") }];
        assert_eq!(spans(e.text_lines(&two)), [(0, 80), (80, 120), (120, 200), (240, 280)]);
        let apart = Export { merge: false, ..e };
        assert_eq!(spans(apart.mask_lines(0)), [(0, 40), (40, 80), (80, 120), (120, 160), (160, 200), (240, 280)]);
        assert_eq!(apart.text_lines(&[SrcLine::standalone("T", "Default")]).len(), 6);
    }

    #[test]
    fn objects_follow_only_what_is_asked() {
        let turned = mul(&translation(130., 70.), &mul(&[0., -2., 0., 2., 0., 0., 0., 0., 1.], &translation(-100., -50.)));
        let f = fd(turned).unwrap();
        let (c, v) = ([100., 50.], [110., 50.]);
        let at = |fol: Follow| place(&f, &[], false, c, fol, v).map(f32::round);
        let only = |translation, scale, rotation| Follow { translation, scale, rotation, deform: false };
        assert_eq!(at(Follow::ALL), [130., 90.]);
        assert_eq!(at(only(true, true, true)), [130., 90.]);
        assert_eq!(at(only(true, false, false)), [140., 70.]);
        assert_eq!(at(only(true, true, false)), [150., 70.]);
        assert_eq!(at(only(true, false, true)), [130., 80.]);
        assert_eq!(at(only(false, false, false)), [110., 50.]);
        assert_eq!(at(only(false, true, true)), [100., 70.]);
        assert_eq!(at(Follow { scale: false, ..Follow::ALL }), [130., 80.]);
        assert_eq!(at(Follow { rotation: false, ..Follow::ALL }), [150., 70.]);
        for fol in [Follow::ALL, only(true, false, true), only(false, true, false), Follow { translation: false, scale: false, ..Follow::ALL }] {
            let back = unplace(&f, &[], false, c, fol, place(&f, &[], false, c, fol, v));
            assert!((back[0] - v[0]).abs() < 0.01 && (back[1] - v[1]).abs() < 0.01, "{back:?}");
        }
    }

    #[test]
    fn text_can_follow_the_position_only() {
        let grown = mul(&translation(110., 50.), &mul(&[2., 0., 0., 0., 2., 0., 0., 0., 1.], &translation(-100., -50.)));
        let frames = [fd(grown)];
        let mut e = export(&frames, &[0, 41], &[]);
        e.text_follow = Follow { translation: true, scale: false, rotation: false, deform: false };
        assert_eq!(e.text_lines(&[SrcLine::standalone("T", "Titre")])[0].text, "{\\pos(220,100)}T");
        e.text_follow = Follow { translation: false, ..Follow::ALL };
        assert_eq!(e.text_lines(&[SrcLine::standalone("T", "Titre")])[0].text, "{\\pos(200,100)\\fscx200\\fscy200\\frz0}T");
    }
}
