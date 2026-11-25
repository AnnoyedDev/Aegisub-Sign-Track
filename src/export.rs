use crate::picture::Picture;
use crate::track::{apply, fit, FrameData, Motion, H, P};
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
    align: u8,
    font: f64,
    edge: String,
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
            align: 2,
            font: 0.,
            edge: "&H000000&".into(),
        }
    }

    pub fn set_edge(&mut self, styles: &[String]) {
        let name = self.mid.split(',').next().unwrap_or("");
        let found = styles.iter().filter_map(|s| s.strip_prefix("Style: ")).map(|s| s.split(',').collect::<Vec<_>>()).find(|f| f[0] == name);
        if let Some(c) = found.and_then(|f| f.get(5)?.get(4..10)) {
            self.edge = format!("&H{c}&");
        }
    }

    pub fn set_box(&mut self, align: u8, font: f64) {
        (self.align, self.font) = (align, font);
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
            align: 2,
            font: 0.,
            edge: "&H000000&".into(),
        })
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Cover {
    Clear,
    Cut([f32; 3]),
    Hidden,
}

pub fn frontier(f: &FrameData, ref_pts: &[P], warp: bool, line: [f32; 3]) -> [f32; 3] {
    let at = |along: f32, across: f32| f.map(ref_pts, warp, [line[0] * (line[2] + across) - line[1] * along, line[1] * (line[2] + across) + line[0] * along]);
    let (a, b, kept) = (at(-50., 0.), at(50., 0.), at(0., -50.));
    let len = (b[0] - a[0]).hypot(b[1] - a[1]).max(1e-6);
    let n = [(b[1] - a[1]) / len, (a[0] - b[0]) / len];
    let d = n[0] * a[0] + n[1] * a[1];
    if n[0] * kept[0] + n[1] * kept[1] <= d { [n[0], n[1], d] } else { [-n[0], -n[1], -d] }
}

pub fn clip(poly: &[P], line: [f32; 3]) -> Vec<P> {
    let side = |p: P| line[0] * p[0] + line[1] * p[1] - line[2];
    let mut out = vec![];
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        let (sa, sb) = (side(a), side(b));
        if sa <= 0. {
            out.push(a);
        }
        if (sa <= 0.) != (sb <= 0.) {
            let t = sa / (sa - sb);
            out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
        }
    }
    out
}

pub struct OutLine {
    pub added: bool,
    pub src: usize,
    pub start: i64,
    pub end: i64,
    pub text: String,
    pub layer: i32,
}

pub struct Export<'a> {
    pub frames: &'a [Option<FrameData>],
    pub ref_pts: &'a [P],
    pub warp: bool,
    pub times: &'a [i64],
    pub scale: [f64; 2],
    pub size: [f64; 2],
    pub mask: &'a [P],
    pub anchor: P,
    pub own_pos: bool,
    pub add_text: bool,
    pub color: [u8; 3],
    pub tints: Vec<Option<[u8; 3]>>,
    pub gradients: &'a [Gradient],
    pub bands: usize,
    pub picture: Option<&'a Picture>,
    pub pin: bool,
    pub blur: f32,
    pub mask_opacity: bool,
    pub text_opacity: bool,
    pub mask_follow: Follow,
    pub text_follow: Follow,
    pub glued: &'a [Option<FrameData>],
    pub seat: crate::track::H,
    pub merge: bool,
    pub group: bool,
    pub cut: Cut,
    pub grow: f32,
    pub twin: bool,
    pub mask_layer: i32,
    pub text_layer: i32,
    pub text_pose: [f64; 4],
    pub covers: &'a [Cover],
}

#[derive(Clone, Copy, PartialEq)]
pub enum Cut {
    None,
    Inside,
    Outside,
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

pub fn read_tracking(text: &str) -> Option<(f64, Vec<(usize, H)>)> {
    let (mut width, mut section) = (0f64, String::new());
    let mut rows: std::collections::BTreeMap<String, Vec<(usize, Vec<f64>)>> = Default::default();
    for line in text.lines() {
        let cells: Vec<&str> = line.split('\t').map(str::trim).filter(|c| !c.is_empty()).collect();
        match cells.as_slice() {
            ["Source Width", w] => width = w.parse().ok()?,
            ["Frame", ..] => {}
            [first, rest @ ..] if first.parse::<f64>().is_ok() && !section.is_empty() => {
                rows.entry(section.clone()).or_default().push((first.parse::<f64>().ok()? as usize, rest.iter().filter_map(|c| c.parse().ok()).collect()));
            }
            [.., last] if !last.starts_with("Frame") && !last.contains("pixels") => section = last.to_string(),
            _ => {}
        }
    }
    let pins: Vec<&Vec<(usize, Vec<f64>)>> = (2..6).filter_map(|n| rows.get(&format!("CC Power Pin-000{n}"))).collect();
    let all = Motion { translation: true, scale: true, rotation: true, shear: true, perspective: true };
    let xy = |v: &[f64]| [v[0] as f32, v[1] as f32];
    let out: Vec<(usize, H)> = if pins.len() == 4 && pins.iter().all(|p| p.len() == pins[0].len() && p.iter().all(|r| r.1.len() >= 2)) {
        let at = |i: usize| -> Vec<P> { pins.iter().map(|p| xy(&p[i].1)).collect() };
        (0..pins[0].len()).filter_map(|i| Some((pins[0][i].0, fit(&all, &at(0), &at(i), &[true; 4])?))).collect()
    } else {
        let place = rows.get("Position").filter(|p| p.iter().all(|r| r.1.len() >= 2))?;
        let find = |name: &str, frame: usize, col: usize, or: f64| rows.get(name).and_then(|v| v.iter().find(|r| r.0 == frame)).and_then(|r| r.1.get(col).copied()).unwrap_or(or);
        let first = place.first()?.0;
        place
            .iter()
            .map(|(frame, p)| {
                let (kx, ky) = (find("Scale", *frame, 0, 100.) / find("Scale", first, 0, 100.), find("Scale", *frame, 1, 100.) / find("Scale", first, 1, 100.));
                let turn = (find("Rotation", *frame, 0, 0.) - find("Rotation", first, 0, 0.)).to_radians();
                let lin = [kx * turn.cos(), -ky * turn.sin(), 0., kx * turn.sin(), ky * turn.cos(), 0., 0., 0., 1.];
                let from = &place[0].1;
                (*frame, crate::track::mul(&crate::track::translation(p[0], p[1]), &crate::track::mul(&lin, &crate::track::translation(-from[0], -from[1]))))
            })
            .collect()
    };
    (width > 0. && !out.is_empty()).then_some((width, out))
}

pub fn inflate(poly: &[P], d: f32) -> Vec<P> {
    let n = poly.len();
    if d == 0. || n < 3 {
        return poly.to_vec();
    }
    let area: f32 = (0..n).map(|i| poly[i][0] * poly[(i + 1) % n][1] - poly[(i + 1) % n][0] * poly[i][1]).sum();
    let side = if area >= 0. { 1. } else { -1. };
    let out = |u: P, v: P| {
        let l = (v[0] - u[0]).hypot(v[1] - u[1]).max(1e-6);
        [(v[1] - u[1]) / l * side, -(v[0] - u[0]) / l * side]
    };
    (0..n)
        .map(|i| {
            let (a, b) = (out(poly[(i + n - 1) % n], poly[i]), out(poly[i], poly[(i + 1) % n]));
            let k = d / (1. + a[0] * b[0] + a[1] * b[1]).max(0.2);
            [poly[i][0] + (a[0] + b[0]) * k, poly[i][1] + (a[1] + b[1]) * k]
        })
        .collect()
}

pub fn seat(mask: &[P], w: f32, h: f32, pin: bool) -> H {
    let b = mask.iter().fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |b, p| [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])]);
    let around = [((b[2] - b[0]) / w) as f64, 0., b[0] as f64, 0., ((b[3] - b[1]) / h) as f64, b[1] as f64, 0., 0., 1.];
    if !pin || mask.len() != 4 {
        return around;
    }
    let all = Motion { translation: true, scale: true, rotation: true, shear: true, perspective: true };
    fit(&all, &[[0., 0.], [w, 0.], [w, h], [0., h]], mask, &[true; 4]).unwrap_or(around)
}

pub fn layers(mask: &[P], color: [u8; 3], gradients: &[Gradient], bands: usize, picture: Option<(&Picture, bool)>) -> Vec<Layer> {
    if let Some((pic, pin)) = picture {
        let to = seat(mask, pic.w as f32, pic.h as f32, pin);
        return pic.layers.iter().map(|(color, loops, solid)| Layer { loops: loops.iter().map(|l| l.iter().map(|p| apply(&to, *p)).collect()).collect(), color: *color, solid: *solid }).collect();
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

    fn cover(&self, i: usize) -> Cover {
        self.covers.get(i).copied().unwrap_or(Cover::Clear)
    }

    fn uncovered(&self, f: &FrameData, line: [f32; 3], poly: Option<Vec<P>>) -> String {
        let (w, h) = ((self.size[0] / self.scale[0]) as f32, (self.size[1] / self.scale[1]) as f32);
        let all = poly.unwrap_or_else(|| vec![[-w, -h], [2. * w, -h], [2. * w, 2. * h], [-w, 2. * h]]);
        let left = clip(&all, frontier(f, self.ref_pts, self.warp, line));
        let mut t = String::from("\\clip(4,");
        for k in 0..left.len().max(3) {
            let q = left.get(k).copied().unwrap_or([0., 0.]);
            write!(t, "{}{} {}", ["m ", " l ", " "][k.min(2)], (q[0] as f64 * self.scale[0] * 8.).round(), (q[1] as f64 * self.scale[1] * 8.).round()).unwrap();
        }
        t + ")"
    }

    pub fn mask_lines(&self, src: usize) -> Vec<OutLine> {
        let centre = centroid(self.mask);
        let mut all = vec![];
        let wide = inflate(self.mask, if self.picture.is_some() { 0. } else { self.grow });
        for (n, layer) in layers(&wide, self.color, self.gradients, self.bands, self.picture.map(|p| (p, self.pin))).into_iter().enumerate() {
            let (mut out, mut run) = (vec![], None);
            for (i, start, end, f) in self.each().filter(|e| self.cover(e.0) != Cover::Hidden) {
                let live = self.tints.get(i).copied().flatten().filter(|_| n == 0 && self.picture.is_none());
                let [r, g, b] = live.unwrap_or(layer.color);
                let mut t = format!("{{\\an7\\pos(0,0)\\fscx100\\fscy100\\frz0\\bord0\\shad0\\1c&H{b:02X}{g:02X}{r:02X}&");
                if self.blur > 0. {
                    write!(t, "\\blur{}", num(self.blur as f64, 2)).unwrap();
                }
                let seen = layer.solid * if self.mask_opacity { relative_opacity(self.frames, f) } else { 1. };
                let alpha = (255. * (1. - seen)).round() as u8;
                if self.mask_opacity || layer.solid < 1. {
                    write!(t, "\\alpha&H{alpha:02X}&").unwrap();
                }
                let cut = if let Cover::Cut(line) = self.cover(i) { self.uncovered(f, line, None) } else { String::new() };
                t.push_str(&cut);
                t.push_str("\\p4}");
                let mut key = vec![alpha as f64 / STILL_ALPHA, r as f64 / 2., g as f64 / 2., b as f64 / 2.];
                key.extend(cut.split(|c: char| !c.is_ascii_digit() && c != '-').filter_map(|v| v.parse::<f64>().ok()).skip(1).map(|v| v / 8. / STILL_PX));
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
                keep(self.merge, &mut out, &mut run, i, key, OutLine { added: true, src, start, end, text: t, layer: self.mask_layer });
            }
            all.extend(out);
        }
        all
    }

    fn cut_tag(&self, i: usize, f: &FrameData) -> String {
        let shaped = self.cut != Cut::None && self.mask.len() >= 3;
        let centre = centroid(self.mask);
        if let (Cover::Cut(line), false) = (self.cover(i), shaped && self.cut == Cut::Outside) {
            let inside = shaped.then(|| inflate(self.mask, self.grow).iter().map(|v| place(f, self.ref_pts, self.warp, centre, self.mask_follow, *v)).collect());
            return self.uncovered(f, line, inside);
        }
        if !shaped {
            return String::new();
        }
        let mut t = String::from(if self.cut == Cut::Inside { "\\clip(4," } else { "\\iclip(4," });
        for (k, v) in inflate(self.mask, self.grow).iter().enumerate() {
            let q = place(f, self.ref_pts, self.warp, centre, self.mask_follow, *v);
            write!(t, "{}{} {}", ["m ", " l ", " "][k.min(2)], (q[0] as f64 * self.scale[0] * 8.).round(), (q[1] as f64 * self.scale[1] * 8.).round()).unwrap();
        }
        t + ")"
    }

    fn spot(&self, f: &FrameData, anchor: P, dx: f64, dy: f64) -> [f64; 2] {
        let (x, y) = (anchor[0] as f64 + dx / self.scale[0], anchor[1] as f64 + dy / self.scale[1]);
        if self.warp {
            let q = f.map(self.ref_pts, true, [x as f32, y as f32]);
            return [q[0] as f64 * self.scale[0], q[1] as f64 * self.scale[1]];
        }
        let w = f.h[6] * x + f.h[7] * y + f.h[8];
        [(f.h[0] * x + f.h[1] * y + f.h[2]) / w * self.scale[0], (f.h[3] * x + f.h[4] * y + f.h[5]) / w * self.scale[1]]
    }

    fn pose(&self, f: &FrameData, anchor: P, b: &Base) -> ([f64; 10], Option<(f64, f64)>) {
        let fol = self.text_follow;
        let alpha = self.alpha(self.text_opacity, f).unwrap_or(0) as f64;
        let still = [anchor[0] as f64 * self.scale[0], anchor[1] as f64 * self.scale[1]];
        if !fol.deform {
            let (a, sx, sy, r) = local_motion(f, self.ref_pts, self.warp, anchor, self.scale[0] / self.scale[1]);
            let at = if fol.translation { [a[0] as f64 * self.scale[0], a[1] as f64 * self.scale[1]] } else { still };
            let k = if fol.scale { (sx * sy).sqrt() } else { 1. };
            let turn = if fol.rotation { r.to_degrees() } else { 0. };
            let (c, s) = (k * turn.to_radians().cos(), k * turn.to_radians().sin());
            let org = b.org.map(|o| (o[0] - still[0], o[1] - still[1])).map(|(x, y)| (at[0] + x * c - y * s, at[1] + x * s + y * c));
            return ([at[0], at[1], b.fx * k, b.fy * k, b.fr - turn, b.tx, b.ty, 0., k, alpha], org);
        }
        const H: f64 = 32.;
        let drop = [1., 1., 1., 0.5, 0.5, 0.5, 0., 0., 0.][(b.align.clamp(1, 9) - 1) as usize];
        let (ax, ay, az) = (-b.tx.to_radians(), b.ty.to_radians(), -b.fr.to_radians());
        let away = b.org.map_or([0., 0.], |o| [still[0] - o[0], still[1] - o[1]]);
        let lay = |u: f64, v: f64| {
            let (x, y) = (b.fx / 100. * (u + b.fax * (v + b.font * drop)) + away[0], b.fy / 100. * v + away[1]);
            let (x1, y1) = (az.cos() * x - az.sin() * y, az.sin() * x + az.cos() * y);
            let (y2, z2) = (ax.cos() * y1, -ax.sin() * y1);
            let (x3, z3) = (ay.cos() * x1 + ay.sin() * z2, ay.sin() * x1 - ay.cos() * z2);
            self.spot(f, anchor, x3 * DEPTH / (z3 + DEPTH) - away[0], y2 * DEPTH / (z3 + DEPTH) - away[1])
        };
        let o = lay(0., 0.);
        let mut n = [[0f64; 2]; 3];
        for (c, (u, v)) in [(H, 0.), (0., H)].into_iter().enumerate() {
            let (p, m) = (lay(u, v), lay(-u, -v));
            let (sum, dif) = ([p[0] + m[0] - 2. * o[0], p[1] + m[1] - 2. * o[1]], [p[0] - m[0], p[1] - m[1]]);
            let w = -(sum[0] * dif[0] + sum[1] * dif[1]) / (H * (dif[0] * dif[0] + dif[1] * dif[1]).max(1e-12));
            let k = (1. - w * w * H * H) / (2. * H);
            (n[0][c], n[1][c], n[2][c]) = (dif[0] * k, dif[1] * k, DEPTH * w);
        }
        let col = |c: usize| -> [f64; 3] { std::array::from_fn(|r| n[r][c]) };
        let dot = |a: &[f64; 3], b: &[f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let (n1, n2) = (col(0), col(1));
        let sx = dot(&n1, &n1).sqrt().max(1e-9);
        let q1 = n1.map(|v| v / sx);
        let lean = dot(&q1, &n2);
        let rest: [f64; 3] = std::array::from_fn(|i| n2[i] - lean * q1[i]);
        let sy = dot(&rest, &rest).sqrt().max(1e-9);
        let q2 = rest.map(|v| v / sy);
        let q3 = [q1[1] * q2[2] - q1[2] * q2[1], q1[2] * q2[0] - q1[0] * q2[2], q1[0] * q2[1] - q1[1] * q2[0]];
        let (az, ax, ay) = (q1[1].atan2(q2[1]), (-q3[1]).atan2(q1[1].hypot(q2[1])), (-q3[0]).atan2(q3[2]));
        let fax = lean / sx;
        let (fscx, fscy, grow) = if fol.scale { (100. * sx, 100. * sy, (sx * sy / (b.fx * b.fy / 1e4).max(1e-9)).sqrt()) } else { (b.fx, b.fy, 1.) };
        let frz = if fol.rotation { -az.to_degrees() } else { b.fr };
        let at = if fol.translation { o } else { still };
        let slip = fax * fscx / 100. * b.font * drop;
        let org = (slip.abs() > 0.05).then_some((at[0], at[1]));
        ([at[0] - if org.is_some() { slip } else { 0. }, at[1], fscx, fscy, frz, -ax.to_degrees(), ay.to_degrees(), fax, grow, alpha], org)
    }

    pub fn text_lines(&self, lines: &[SrcLine]) -> Vec<OutLine> {
        let fol = self.text_follow;
        let mut out = vec![];
        for l in lines {
            let own = line_pos(&l.text).filter(|_| self.own_pos).map(|(x, y)| [(x / self.scale[0]) as f32, (y / self.scale[1]) as f32]);
            let anchor = crate::track::apply(&self.seat, own.unwrap_or(self.anchor));
            let mut base = Base::read(l);
            let [tx, ty, turn, size] = self.text_pose;
            (base.tx, base.ty, base.fr, base.fx, base.fy) = (base.tx + tx, base.ty + ty, base.fr + turn, base.fx * size / 100., base.fy * size / 100.);
            let mut poses: Vec<Pose> = vec![];
            for (i, start, end, f) in self.each().filter(|(i, ..)| (l.first..l.end).contains(i) && self.cover(*i) != Cover::Hidden) {
                let f = self.glued.get(i).and_then(Option::as_ref).unwrap_or(f);
                let (mut v, org) = self.pose(f, anchor, &base);
                if let Some(last) = poses.last() {
                    for c in 4..7 {
                        v[c] += 360. * ((last.v[c] - v[c]) / 360.).round();
                    }
                }
                poses.push(Pose { i, at: self.times[i], start, end, v, org, cut: self.cut_tag(i, f) });
            }
            let any = |c: usize, min: f64| poses.iter().any(|p| p.v[c].abs() > min);
            let plan = Plan { size: fol.scale || size != 100., turn: fol.rotation || turn != 0., deform: fol.deform, tilt: any(5, 0.01) || any(6, 0.01), lean: fol.deform && any(7, 0.0005), alpha: self.text_opacity, org: poses.iter().any(|p| p.org.is_some()), cut: poses.iter().any(|p| !p.cut.is_empty()) };
            base.keep(&plan);
            let mut s = 0;
            while s < poses.len() {
                let mut e = s;
                while e + 1 < poses.len() && poses[e + 1].i == poses[e].i + 1 && self.joins(&poses[s..=e + 1]) {
                    e += 1;
                }
                let text = base.write(&plan, &poses[s], &poses[e]);
                let both = if self.twin {
                    let block = text.split('}').next().unwrap_or("");
                    let edge = block.rfind("\\3c").map_or(l.edge.as_str(), |i| block[i + 3..].split('\\').next().unwrap_or(""));
                    vec![text.replacen('}', &format!("\\1c{edge}}}"), 1), text.replacen('}', "\\bord0\\shad0}", 1)]
                } else {
                    vec![text]
                };
                for text in both {
                    out.push(OutLine { added: self.add_text, src: l.index, start: poses[s].start, end: poses[e].end, text, layer: self.text_layer });
                }
                s = e + 1;
            }
        }
        out
    }

    fn joins(&self, run: &[Pose]) -> bool {
        let (a, b) = (&run[0], &run[run.len() - 1]);
        let same = |p: &Pose| p.cut == a.cut && p.org.is_some() == a.org.is_some();
        let still = || self.merge && run.iter().all(|p| same(p) && (0..10).all(|c| (p.v[c] - a.v[c]).abs() <= STILL[c]) && p.org.zip(a.org).is_none_or(|(p, a)| (p.0 - a.0).abs() <= STILL_PX && (p.1 - a.1).abs() <= STILL_PX));
        if !self.group {
            return still();
        }
        let span = (b.at - a.at).max(1) as f64;
        still() || run.iter().all(|p| same(p) && p.org.is_none() && (0..10).all(|c| (p.v[c] - (a.v[c] + (b.v[c] - a.v[c]) * (p.at - a.at) as f64 / span)).abs() <= SMOOTH[c]))
    }

    pub fn power_pin(&self, fps: f64) -> String {
        let b = self.mask.iter().fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |b, p| [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])]);
        let corners = if self.mask.len() == 4 { [self.mask[0], self.mask[1], self.mask[3], self.mask[2]] } else { [[b[0], b[1]], [b[2], b[1]], [b[0], b[3]], [b[2], b[3]]] };
        let centre = centroid(self.mask);
        let first = self.each().next().map_or(0, |e| e.0);
        let mut t = format!("Adobe After Effects 6.0 Keyframe Data\n\n\tUnits Per Second\t{}\n\tSource Width\t{}\n\tSource Height\t{}\n\tSource Pixel Aspect Ratio\t1\n\tComp Pixel Aspect Ratio\t1\n", num(fps, 3), num(self.size[0], 0), num(self.size[1], 0));
        for (n, corner) in corners.iter().enumerate() {
            write!(t, "\nEffects\tCC Power Pin #1\tCC Power Pin-000{}\n\tFrame\tX pixels\tY pixels\n", n + 2).unwrap();
            for (i, _, _, f) in self.each() {
                let q = place(f, self.ref_pts, self.warp, centre, self.mask_follow, *corner);
                writeln!(t, "\t{}\t{}\t{}", i - first, num(q[0] as f64 * self.scale[0], 3), num(q[1] as f64 * self.scale[1], 3)).unwrap();
            }
        }
        t + "\nEnd of Keyframe Data\n"
    }

    pub fn keyframes(&self, fps: f64) -> String {
        let (mut place, mut size, mut turn) = (String::new(), String::new(), String::new());
        let first = self.each().next().map_or(0, |e| e.0);
        for (i, _, _, f) in self.each() {
            let (a, sx, sy, r) = local_motion(f, self.ref_pts, self.warp, self.anchor, self.scale[0] / self.scale[1]);
            let n = i - first;
            writeln!(place, "\t{n}\t{}\t{}\t0", num(a[0] as f64 * self.scale[0], 3), num(a[1] as f64 * self.scale[1], 3)).unwrap();
            writeln!(size, "\t{n}\t{}\t{}\t100", num(sx * 100., 3), num(sy * 100., 3)).unwrap();
            writeln!(turn, "\t{n}\t{}", num(r.to_degrees(), 3)).unwrap();
        }
        format!("Adobe After Effects 6.0 Keyframe Data\n\n\tUnits Per Second\t{}\n\tSource Width\t{}\n\tSource Height\t{}\n\tSource Pixel Aspect Ratio\t1\n\tComp Pixel Aspect Ratio\t1\n\nPosition\n\tFrame\tX pixels\tY pixels\tZ pixels\n{place}\nScale\n\tFrame\tX percent\tY percent\tZ percent\n{size}\nRotation\n\tFrame\tDegrees\n{turn}\nEnd of Keyframe Data\n", num(fps, 3), num(self.size[0], 0), num(self.size[1], 0))
    }
}

const DEPTH: f64 = 312.5;
const STILL: [f64; 10] = [STILL_PX, STILL_PX, STILL_SCALE, STILL_SCALE, STILL_ROT, STILL_ROT, STILL_ROT, 0.0005, 0.001, STILL_ALPHA];
const SMOOTH: [f64; 10] = [0.25, 0.25, 0.1, 0.1, 0.05, 0.05, 0.05, 0.002, 0.01, 2.];

struct Pose {
    i: usize,
    at: i64,
    start: i64,
    end: i64,
    v: [f64; 10],
    org: Option<(f64, f64)>,
    cut: String,
}

struct Plan {
    size: bool,
    turn: bool,
    deform: bool,
    tilt: bool,
    lean: bool,
    alpha: bool,
    org: bool,
    cut: bool,
}

struct Base {
    fx: f64,
    fy: f64,
    fr: f64,
    tx: f64,
    ty: f64,
    fax: f64,
    org: Option<[f64; 2]>,
    bord: Option<f64>,
    shad: Option<f64>,
    font: f64,
    align: u8,
    toks: Vec<String>,
    kept: String,
    rest: String,
}

impl Base {
    fn read(l: &SrcLine) -> Base {
        let (toks, rest) = first_block(&l.text);
        let mut b = Base { fx: l.scale_x, fy: l.scale_y, fr: l.angle, tx: 0., ty: 0., fax: 0., org: None, bord: l.outline, shad: l.shadow, font: l.font, align: l.align, toks: toks.iter().map(|t| t.to_string()).collect(), kept: String::new(), rest: rest.into() };
        for t in &toks {
            let t = t.strip_prefix('\\').unwrap_or("");
            if let Some(v) = tag_num(t, "fscx") {
                b.fx = v;
            } else if let Some(v) = tag_num(t, "fscy") {
                b.fy = v;
            } else if let Some(v) = tag_num(t, "frz").or_else(|| tag_num(t, "fr")) {
                b.fr = v;
            } else if let Some(v) = tag_num(t, "frx") {
                b.tx = v;
            } else if let Some(v) = tag_num(t, "fry") {
                b.ty = v;
            } else if let Some(v) = tag_num(t, "fax") {
                b.fax = v;
            } else if let Some(v) = t.strip_prefix("org(") {
                let at: Vec<f64> = v.trim_end_matches(')').split(',').filter_map(|n| n.trim().parse().ok()).collect();
                b.org = at.try_into().ok();
            } else if let Some(v) = tag_num(t, "bord") {
                b.bord = Some(v);
            } else if let Some(v) = tag_num(t, "shad") {
                b.shad = Some(v);
            } else if let Some(v) = tag_num(t, "fs") {
                b.font = v;
            } else if let Some(v) = tag_num(t, "an") {
                b.align = v as u8;
            }
        }
        b
    }

    fn keep(&mut self, p: &Plan) {
        let gone = |t: &str| {
            let is = |name: &str| tag_num(t, name).is_some();
            t.starts_with("pos(") || t.starts_with("move(")
                || (p.size && (is("fscx") || is("fscy") || is("bord") || is("shad")))
                || (p.turn && (is("frz") || is("fr")))
                || (p.tilt && (is("frx") || is("fry")))
                || (p.deform && (p.tilt || p.lean || p.org) && is("fax"))
                || ((p.org || p.deform && (p.tilt || p.lean)) && t.starts_with("org("))
                || (p.alpha && t.starts_with("alpha"))
                || (p.cut && (t.starts_with("clip(") || t.starts_with("iclip(")))
        };
        self.kept = self.toks.iter().filter(|t| !gone(t.strip_prefix('\\').unwrap_or(""))).cloned().collect();
    }

    fn parts(&self, p: &Plan, v: &[f64; 10]) -> Vec<String> {
        let mut o = vec![];
        if p.size {
            o.push(format!("\\fscx{}", num(v[2], 2)));
            o.push(format!("\\fscy{}", num(v[3], 2)));
            for (name, w) in [("bord", self.bord), ("shad", self.shad)] {
                if let Some(w) = w {
                    o.push(format!("\\{name}{}", num(w * v[8], 2)));
                }
            }
        }
        if p.turn {
            o.push(format!("\\frz{}", num(v[4], 3)));
        }
        if p.tilt {
            o.push(format!("\\frx{}", num(v[5], 3)));
            o.push(format!("\\fry{}", num(v[6], 3)));
        }
        if p.lean {
            o.push(format!("\\fax{}", num(v[7], 4)));
        }
        if p.alpha {
            o.push(format!("\\alpha&H{:02X}&", v[9].round().clamp(0., 255.) as u8));
        }
        o
    }

    fn write(&self, p: &Plan, a: &Pose, b: &Pose) -> String {
        let (from, to) = (self.parts(p, &a.v), self.parts(p, &b.v));
        let spot = |v: &[f64; 10]| format!("{},{}", num(v[0], 2), num(v[1], 2));
        let moved = spot(&a.v) != spot(&b.v);
        let changed: String = from.iter().zip(&to).filter(|(x, y)| x != y).map(|(_, y)| y.as_str()).collect();
        let (t1, t2) = (a.at - a.start, b.at - a.start);
        let mut o = String::from("{");
        if moved {
            write!(o, "\\move({},{},{t1},{t2})", spot(&a.v), spot(&b.v)).unwrap();
        } else {
            write!(o, "\\pos({})", spot(&a.v)).unwrap();
        }
        if let Some((x, y)) = a.org {
            write!(o, "\\org({},{})", num(x, 2), num(y, 2)).unwrap();
        }
        o.extend(from);
        o.push_str(&a.cut);
        if !changed.is_empty() {
            write!(o, "\\t({t1},{t2},{changed})").unwrap();
        }
        o + &self.kept + "}" + &self.rest
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

fn stamp(ms: i64) -> String {
    format!("{}:{:02}:{:02}.{:02}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms / 10 % 100)
}

pub fn clipboard(out: &[OutLine], lines: &[SrcLine], t0: i64) -> String {
    out.iter()
        .filter_map(|o| {
            let l = lines.iter().find(|l| l.index == o.src)?;
            let (kind, base) = l.head.split_once(": ").unwrap_or(("Dialogue", "0"));
            Some(format!("{kind}: {},{},{},{},{}\n", base.parse::<i32>().unwrap_or(0) + o.layer, stamp(o.start - t0), stamp(o.end - t0), l.mid, o.text))
        })
        .collect()
}

pub fn result_file(out: &[OutLine]) -> String {
    let mut s = String::new();
    for o in out {
        let layer = if o.layer == 0 { String::new() } else { format!("L{}\t", o.layer) };
        writeln!(s, "{}\t{}\t{}\t{}\t{layer}{}", if o.added { "mask" } else { "text" }, o.src, o.start, o.end, o.text).unwrap();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::{mul, translation, IDENT};

    fn export<'a>(frames: &'a [Option<FrameData>], times: &'a [i64], mask: &'a [P]) -> Export<'a> {
        Export { frames, ref_pts: &[], warp: false, times, scale: [2., 2.], size: [1280., 720.], mask, anchor: [100., 50.], own_pos: false, add_text: false, color: [0x11, 0x22, 0x33], tints: vec![], gradients: &[], bands: 4, picture: None, pin: false, blur: 0., mask_opacity: false, text_opacity: false, mask_follow: Follow::ALL, text_follow: Follow::ALL, glued: &[], seat: IDENT, merge: true, group: false, cut: Cut::None, grow: 0., twin: false, mask_layer: 0, text_layer: 0, text_pose: [0., 0., 0., 100.], covers: &[] }
    }

    fn fd(h: [f64; 9]) -> Option<FrameData> {
        Some(FrameData { h, pts: vec![], opacity: 0.5, guessed: false, score: 1. })
    }

    #[test]
    fn glued_frames_move_the_text_and_leave_the_mask() {
        let frames = [fd(IDENT), fd(IDENT)];
        let glued = [None, fd(translation(10., 5.))];
        let shape = [[0., 0.], [10., 0.], [10., 10.]];
        let mut e = export(&frames, &[0, 40, 80], &shape);
        e.glued = &glued;
        let text = e.text_lines(&[SrcLine::standalone("T", "Default")]);
        assert!(text[0].text.starts_with("{\\pos(200,100)") && text[1].text.starts_with("{\\pos(220,110)"), "{} / {}", text[0].text, text[1].text);
        assert_eq!(e.mask_lines(0).len(), 1);
        e.seat = translation(-10., -5.);
        assert!(e.text_lines(&[SrcLine::standalone("T", "Default")])[1].text.starts_with("{\\pos(200,100)"));
    }

    #[test]
    fn text_gets_tracked_tags_and_keeps_the_rest() {
        let rot = mul(&translation(110., 50.), &mul(&[0., -1.5, 0., 1.5, 0., 0., 0., 0., 1.], &translation(-100., -50.)));
        let frames = [fd(IDENT), fd(rot), Some(FrameData { h: IDENT, pts: vec![], opacity: 1., guessed: false, score: 1. })];
        let line = SrcLine::parse("7\t0\t2\t100\t80\t10\t2\t1\tDialogue: 3,0:00:00.00,0:00:05.00,Sign,Bob,1,2,3,fx,{\\fscx200\\pos(5,5)\\c&HFF&\\t(0,100,\\fs50)}Sal{\\i1}ut, toi").unwrap();
        let mut e = export(&frames, &[0, 41, 83, 125], &[]);
        (e.mask_opacity, e.text_opacity) = (true, true);
        let out = e.text_lines(&[line.clone()]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].text, "{\\pos(200,100)\\fscx200\\fscy80\\bord2\\shad1\\frz10\\alpha&H80&\\c&HFF&\\t(0,100,\\fs50)}Sal{\\i1}ut, toi");
        assert_eq!(out[1].text, "{\\pos(220,100)\\fscx300\\fscy120\\bord3\\shad1.5\\frz-80\\alpha&H80&\\c&HFF&\\t(0,100,\\fs50)}Sal{\\i1}ut, toi");
        assert_eq!((out[1].src, out[1].start, out[1].end), (7, 40, 80));
        assert_eq!(clipboard(&out[1..], &[line], 0), "Dialogue: 3,0:00:00.04,0:00:00.08,Sign,Bob,1,2,3,fx,".to_string() + &out[1].text + "\n");
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

    fn tag(text: &str, name: &str) -> f64 {
        let at = text.find(name).unwrap_or_else(|| panic!("{name} absent de {text}")) + name.len();
        text[at..].split(['\\', '}', ')', ',']).next().unwrap().parse().unwrap()
    }

    fn libass(text: &str, u: [f64; 2]) -> [f64; 2] {
        let (ax, ay, az) = (-tag(text, "\\frx").to_radians(), tag(text, "\\fry").to_radians(), -tag(text, "\\frz").to_radians());
        let (x, y) = (tag(text, "\\fscx") / 100. * (u[0] + tag(text, "\\fax") * u[1]), tag(text, "\\fscy") / 100. * u[1]);
        let (x1, y1) = (az.cos() * x - az.sin() * y, az.sin() * x + az.cos() * y);
        let (y2, z2) = (ax.cos() * y1, -ax.sin() * y1);
        let (x3, z3) = (ay.cos() * x1 + ay.sin() * z2, ay.sin() * x1 - ay.cos() * z2);
        [x3 * DEPTH / (z3 + DEPTH), y2 * DEPTH / (z3 + DEPTH)]
    }

    #[test]
    fn perspective_becomes_rotations_that_libass_draws_at_the_same_place() {
        let tilt = mul(&translation(110., 50.), &mul(&[1.1, -0.2, 0., 0.15, 0.9, 0., 0.0006, -0.0004, 1.], &translation(-100., -50.)));
        let frames = [fd(tilt)];
        let e = export(&frames, &[0, 41], &[]);
        let text = e.text_lines(&[SrcLine::standalone("T", "Default")])[0].text.clone();
        assert!(text.starts_with("{\\pos(220,100)\\fscx"), "{text}");
        for u in [[40., 0.], [0., 30.], [-60., 25.], [80., -50.]] {
            let want = e.spot(frames[0].as_ref().unwrap(), [100., 50.], u[0], u[1]);
            let got = libass(&text, u);
            assert!((got[0] + 220. - want[0]).hypot(got[1] + 100. - want[1]) < 0.05, "{u:?} : {got:?} au lieu de {:?} avec {text}", [want[0] - 220., want[1] - 100.]);
        }
        let mut low = SrcLine::standalone("T", "Default");
        low.set_box(2, 40.);
        let moved = e.text_lines(&[low])[0].text.clone();
        let slip = tag(&moved, "\\fax") * tag(&moved, "\\fscx") / 100. * 40.;
        assert!(moved.contains("\\org(220,100)") && (tag(&moved, "\\pos(") - (220. - slip)).abs() < 0.01, "{moved}");
    }

    #[test]
    fn text_starts_from_the_pose_it_is_given() {
        let frames = [fd(IDENT)];
        let mut e = export(&frames, &[0, 41], &[]);
        e.text_pose = [20., -30., 10., 150.];
        for deform in [true, false] {
            e.text_follow.deform = deform;
            let text = e.text_lines(&[SrcLine::standalone("T", "Default")])[0].text.clone();
            for (name, want) in [("\\frx", 20.), ("\\fry", -30.), ("\\frz", 10.), ("\\fscx", 150.), ("\\fscy", 150.)] {
                assert!((tag(&text, name) - want).abs() < 0.05, "{name} dans {text}");
            }
        }
        e.text_pose = [0., 0., 0., 100.];
        let own = e.text_lines(&[SrcLine::standalone("{\\frx15\\fry-25}T", "Default")])[0].text.clone();
        assert!((tag(&own, "\\frx") - 15.).abs() < 0.05 && (tag(&own, "\\fry") + 25.).abs() < 0.05 && own.matches("\\frx").count() == 1, "{own}");
        let tilt = mul(&translation(110., 50.), &mul(&[1.1, -0.2, 0., 0.15, 0.9, 0., 0.0006, -0.0004, 1.], &translation(-100., -50.)));
        let moved = [fd(tilt)];
        let mut e = export(&moved, &[0, 41], &[]);
        e.text_pose = [20., -30., 10., 150.];
        let text = e.text_lines(&[SrcLine::standalone("T", "Default")])[0].text.clone();
        let (ax, ay, az) = (-20f64.to_radians(), -30f64.to_radians(), -10f64.to_radians());
        for u in [[40., 0.], [0., 30.], [-60., 25.]] {
            let (x, y) = (1.5 * u[0], 1.5 * u[1]);
            let (x1, y1) = (az.cos() * x - az.sin() * y, az.sin() * x + az.cos() * y);
            let (y2, z2) = (ax.cos() * y1, -ax.sin() * y1);
            let (x3, z3) = (ay.cos() * x1 + ay.sin() * z2, ay.sin() * x1 - ay.cos() * z2);
            let want = e.spot(moved[0].as_ref().unwrap(), [100., 50.], x3 * DEPTH / (z3 + DEPTH), y2 * DEPTH / (z3 + DEPTH));
            let got = libass(&text, u);
            assert!((got[0] + 220. - want[0]).hypot(got[1] + 100. - want[1]) < 0.05, "{u:?} : {got:?} avec {text}");
        }
    }

    fn drawn(text: &str, u: [f64; 2]) -> [f64; 2] {
        let pair = |name: &str| -> Option<[f64; 2]> {
            let v: Vec<f64> = text[text.find(name)? + name.len()..].split(')').next()?.split(',').filter_map(|n| n.parse().ok()).collect();
            v.get(..2)?.try_into().ok()
        };
        let pos = pair("\\pos(").unwrap();
        let org = pair("\\org(").unwrap_or(pos);
        let (ax, ay, az) = (-tag(text, "\\frx").to_radians(), tag(text, "\\fry").to_radians(), -tag(text, "\\frz").to_radians());
        let (x, y) = (tag(text, "\\fscx") / 100. * (u[0] + tag(text, "\\fax") * (u[1] + 40.)) + pos[0] - org[0], tag(text, "\\fscy") / 100. * u[1] + pos[1] - org[1]);
        let (x1, y1) = (az.cos() * x - az.sin() * y, az.sin() * x + az.cos() * y);
        let (y2, z2) = (ax.cos() * y1, -ax.sin() * y1);
        let (x3, z3) = (ay.cos() * x1 + ay.sin() * z2, ay.sin() * x1 - ay.cos() * z2);
        [org[0] + x3 * DEPTH / (z3 + DEPTH), org[1] + y2 * DEPTH / (z3 + DEPTH)]
    }

    #[test]
    fn a_perspective_set_in_aegisub_is_kept_and_carried_by_the_tracking() {
        let given = "{\\pos(220,100)\\org(180,140)\\frz10\\frx20\\fry-30\\fax0.2\\fscx120\\fscy90}T";
        let mut line = SrcLine::standalone(given, "Default");
        line.set_box(2, 40.);
        let tilt = mul(&translation(110., 50.), &mul(&[1.1, -0.2, 0., 0.15, 0.9, 0., 0.0006, -0.0004, 1.], &translation(-100., -50.)));
        for h in [IDENT, tilt] {
            let frames = [fd(h)];
            let mut e = export(&frames, &[0, 41], &[]);
            e.own_pos = true;
            let out = e.text_lines(&[line.clone()])[0].text.clone();
            assert_eq!((out.matches("\\org(").count(), out.matches("\\fax").count(), out.matches("\\frx").count()), (1, 1, 1), "{out}");
            for u in [[0., 0.], [40., 0.], [0., -30.], [-60., 25.], [80., -50.]] {
                let was = drawn(given, u);
                let want = e.spot(frames[0].as_ref().unwrap(), [110., 50.], was[0] - 220., was[1] - 100.);
                let got = drawn(&out, u);
                assert!((got[0] - want[0]).hypot(got[1] - want[1]) < 0.05, "{u:?} : {got:?} au lieu de {want:?} avec {out}");
            }
        }
        let mut e = export(&[], &[0, 41], &[]);
        let still = [fd(translation(10., 0.))];
        (e.frames, e.own_pos, e.text_follow.deform) = (&still, true, false);
        let flat = e.text_lines(&[line])[0].text.clone();
        assert!(flat.contains("\\pos(240,100)\\org(200,140)") && flat.contains("\\fax0.2") && flat.matches("\\org(").count() == 1, "{flat}");
    }

    #[test]
    fn regular_motion_is_grouped_into_one_moving_line() {
        let grow = |k: f64| mul(&translation(100. + 3. * k, 50.), &mul(&[1. + 0.1 * k, 0., 0., 0., 1. + 0.1 * k, 0., 0., 0., 1.], &translation(-100., -50.)));
        let frames = [fd(grow(0.)), fd(grow(1.)), fd(grow(2.)), fd(grow(3.)), fd(translation(40., 0.))];
        let mut e = export(&frames, &[0, 40, 80, 120, 160, 200], &[]);
        e.group = true;
        let out = e.text_lines(&[SrcLine::standalone("T", "Default")]);
        assert_eq!(out.len(), 2);
        assert_eq!((out[0].start, out[0].end), (0, 160));
        assert_eq!(out[0].text, "{\\move(200,100,218,100,0,120)\\fscx100\\fscy100\\frz0\\t(0,120,\\fscx130\\fscy130)}T");
        assert_eq!(out[1].text, "{\\pos(280,100)\\fscx100\\fscy100\\frz0}T");
    }

    #[test]
    fn the_mask_colour_can_change_with_each_frame() {
        let frames = [fd(IDENT), fd(IDENT), fd(IDENT)];
        let mask = [[0., 0.], [10., 0.], [10., 10.]];
        let mut e = export(&frames, &[0, 40, 80, 120], &mask);
        e.tints = vec![Some([1, 2, 3]), Some([1, 2, 3]), Some([200, 2, 3])];
        let out = e.mask_lines(0);
        assert_eq!(out.len(), 2);
        assert!(out[0].text.contains("\\1c&H030201&") && out[1].text.contains("\\1c&H0302C8&"), "{} / {}", out[0].text, out[1].text);
        assert_eq!((out[0].start, out[0].end, out[1].start), (0, 80, 80));
    }

    #[test]
    fn a_picture_can_sit_on_the_four_points_of_the_mask() {
        let mask = [[10., 10.], [110., 30.], [100., 90.], [20., 70.]];
        let (w, h) = (40., 20.);
        for (corner, want) in [[0., 0.], [w, 0.], [w, h], [0., h]].into_iter().zip(mask) {
            let got = apply(&seat(&mask, w, h, true), corner);
            assert!((got[0] - want[0]).hypot(got[1] - want[1]) < 1e-3, "{corner:?} -> {got:?} au lieu de {want:?}");
        }
        assert_eq!(apply(&seat(&mask, w, h, false), [w, h]), [110., 90.]);
        assert_eq!(apply(&seat(&mask[..3], w, h, true), [0., 0.]), [10., 10.]);
    }

    #[test]
    fn the_mask_can_be_widened_and_the_text_doubled() {
        assert_eq!(inflate(&[[0., 0.], [10., 0.], [10., 10.], [0., 10.]], 2.), [[-2., -2.], [12., -2.], [12., 12.], [-2., 12.]]);
        assert_eq!(inflate(&[[0., 10.], [10., 10.], [10., 0.], [0., 0.]], -1.), [[1., 9.], [9., 9.], [9., 1.], [1., 1.]]);
        let frames = [fd(IDENT)];
        let mask = [[0., 0.], [10., 0.], [10., 10.], [0., 10.]];
        let mut e = export(&frames, &[0, 41], &mask);
        (e.grow, e.twin) = (1., true);
        assert!(e.mask_lines(0)[0].text.ends_with("m -16 -16 l 176 -16 176 176 -16 176"));
        let mut line = SrcLine::standalone("{\\bord3}T", "Sign");
        line.set_edge(&["Style: Default,Arial,48,&H00FFFFFF,&H000000FF,&H00112233,&H00000000,0".into(), "Style: Sign,Arial,48,&H00FFFFFF,&H000000FF,&H00A1B2C3,&H00000000,0".into()]);
        let out = e.text_lines(&[line]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].text, "{\\pos(200,100)\\fscx100\\fscy100\\bord3\\frz0\\1c&HA1B2C3&}T");
        assert_eq!(out[1].text, "{\\pos(200,100)\\fscx100\\fscy100\\bord3\\frz0\\bord0\\shad0}T");
        let out = e.text_lines(&[SrcLine::standalone("{\\3c&H0000FF&}T", "Default")]);
        assert!(out[0].text.ends_with("\\3c&H0000FF&\\frz0\\1c&H0000FF&}T") || out[0].text.ends_with("\\1c&H0000FF&}T"), "{}", out[0].text);
        let pins = e.power_pin(25.);
        assert!(pins.contains("Effects\tCC Power Pin #1\tCC Power Pin-0002\n\tFrame\tX pixels\tY pixels\n\t0\t0\t0\n"), "{pins}");
        assert!(pins.contains("CC Power Pin-0004\n\tFrame\tX pixels\tY pixels\n\t0\t0\t20\n") && pins.contains("CC Power Pin-0005\n\tFrame\tX pixels\tY pixels\n\t0\t20\t20\n"), "{pins}");
    }

    #[test]
    fn tracking_data_made_elsewhere_can_be_read_back() {
        let frames = [fd(IDENT), fd(mul(&translation(105., 50.), &mul(&[0., -2., 0., 2., 0., 0., 0., 0., 1.], &translation(-100., -50.))))];
        let mask = [[90., 40.], [110., 40.], [110., 60.], [90., 60.]];
        let e = export(&frames, &[0, 40, 80], &mask);
        for data in [e.keyframes(25.), e.power_pin(25.)] {
            let (width, back) = read_tracking(&data).unwrap();
            assert_eq!((width, back.len(), back[1].0), (1280., 2, 1));
            let got = apply(&back[1].1, [220., 80.]);
            assert!((got[0] - 250.).abs() < 0.01 && (got[1] - 140.).abs() < 0.01, "{got:?} avec {data}");
        }
    }

    #[test]
    fn exported_lines_can_sit_on_higher_layers() {
        let frames = [fd(IDENT)];
        let mask = [[0., 0.], [10., 0.], [10., 10.], [0., 10.]];
        let lines = [SrcLine::standalone("T", "Default")];
        let mut e = export(&frames, &[0, 41], &mask);
        assert!(clipboard(&e.mask_lines(0), &lines, 0).starts_with("Dialogue: 0,0:00:00.00,") && !result_file(&e.text_lines(&lines)).contains("\tL"));
        (e.mask_layer, e.text_layer) = (2, 5);
        assert!(clipboard(&e.mask_lines(0), &lines, 0).starts_with("Dialogue: 2,0:00:00.00,"));
        assert!(clipboard(&e.text_lines(&lines), &lines, 0).starts_with("Dialogue: 5,0:00:00.00,"));
        assert!(result_file(&e.text_lines(&lines)).contains("\tL5\t{\\pos"), "{}", result_file(&e.text_lines(&lines)));
        assert!(result_file(&e.mask_lines(0)).starts_with("mask\t0\t") && result_file(&e.mask_lines(0)).contains("\tL2\t{"));
    }

    #[test]
    fn what_is_covered_is_cut_off_or_left_out() {
        assert_eq!(clip(&[[0., 0.], [10., 0.], [10., 10.], [0., 10.]], [1., 0., 4.]), [[0., 0.], [4., 0.], [4., 10.], [0., 10.]]);
        let moved = fd(translation(100., 0.)).unwrap();
        assert_eq!(frontier(&moved, &[], false, [1., 0., 4.]), [1., 0., 104.]);
        let frames = [fd(IDENT), fd(IDENT), fd(IDENT)];
        let mask = [[0., 0.], [10., 0.], [10., 10.], [0., 10.]];
        let covers = [Cover::Clear, Cover::Cut([1., 0., 4.]), Cover::Hidden];
        let mut e = export(&frames, &[0, 40, 80, 120], &mask);
        e.covers = &covers;
        let m = e.mask_lines(0);
        assert_eq!((m.len(), m[0].end, m[1].end), (2, 40, 80));
        assert!(!m[0].text.contains("clip") && m[1].text.contains("\\clip(4,m -10240 -5760 l 64 -5760 64 11520 -10240 11520)\\p4}"), "{}", m[1].text);
        let t = e.text_lines(&[SrcLine::standalone("T", "Default")]);
        assert_eq!(t.len(), 2);
        assert!(!t[0].text.contains("clip") && t[1].text.contains("\\clip(4,m -10240 -5760 l 64 -5760 64 11520 -10240 11520)"), "{}", t[1].text);
        e.cut = Cut::Inside;
        let t = e.text_lines(&[SrcLine::standalone("T", "Default")]);
        assert!(t[1].text.contains("\\clip(4,m 0 0 l 64 0 64 160 0 160)"), "{}", t[1].text);
    }

    #[test]
    fn the_mask_can_cut_the_text() {
        let frames = [fd(translation(1., 0.))];
        let mask = [[0., 0.], [10., 0.], [10., 10.]];
        let mut e = export(&frames, &[0, 41], &mask);
        e.cut = Cut::Outside;
        let line = SrcLine::parse("1\t0\t1\t100\t100\t0\t0\t0\tDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{\\clip(1,2,3,4)\\b1}a").unwrap();
        assert_eq!(e.text_lines(&[line])[0].text, "{\\pos(202,100)\\fscx100\\fscy100\\frz0\\iclip(4,m 16 0 l 176 0 176 160)\\b1}a");
    }

    #[test]
    fn tracking_data_uses_the_after_effects_layout() {
        let frames = [None, fd(IDENT), fd(mul(&translation(105., 50.), &mul(&[0., -2., 0., 2., 0., 0., 0., 0., 1.], &translation(-100., -50.))))];
        let e = export(&frames, &[0, 40, 80, 120], &[]);
        let data = e.keyframes(25.);
        assert!(data.starts_with("Adobe After Effects 6.0 Keyframe Data\n\n\tUnits Per Second\t25\n\tSource Width\t1280\n\tSource Height\t720\n"), "{data}");
        assert!(data.contains("Position\n\tFrame\tX pixels\tY pixels\tZ pixels\n\t0\t200\t100\t0\n\t1\t210\t100\t0\n"), "{data}");
        assert!(data.contains("Scale\n\tFrame\tX percent\tY percent\tZ percent\n\t0\t100\t100\t100\n\t1\t200\t200\t100\n"), "{data}");
        assert!(data.contains("Rotation\n\tFrame\tDegrees\n\t0\t0\n\t1\t90\n\nEnd of Keyframe Data\n"), "{data}");
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
        assert!(clipboard(&out, &lines, 0).starts_with("Dialogue: 0,0:00:01.04,0:00:01.08,Default,,0,0,0,,{"));
        assert!(clipboard(&out, &lines, out[0].start).starts_with("Dialogue: 0,0:00:00.00,0:00:00.04,Default,,0,0,0,,{"));
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
