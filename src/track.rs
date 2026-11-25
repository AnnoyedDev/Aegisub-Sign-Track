use std::sync::atomic::{AtomicBool, Ordering};

pub type P = [f32; 2];
pub type H = [f64; 9];
pub const IDENT: H = [1., 0., 0., 0., 1., 0., 0., 0., 1.];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Channel {
    Luma,
    R,
    G,
    B,
    Auto,
    Mix([f32; 3]),
}

#[derive(Clone, Copy, Debug)]
pub struct Motion {
    pub translation: bool,
    pub scale: bool,
    pub rotation: bool,
    pub shear: bool,
    pub perspective: bool,
}

#[derive(Clone)]
pub struct Params {
    pub motion: Motion,
    pub search: f32,
    pub pattern: i32,
    pub channel: Channel,
    pub user_pts: bool,
    pub warp: bool,
    pub hold_fades: bool,
    pub realign: bool,
    pub hole: Vec<P>,
    pub axis: u8,
}

#[derive(Clone)]
pub struct FrameData {
    pub h: H,
    pub pts: Vec<P>,
    pub opacity: f32,
    pub guessed: bool,
    pub score: f32,
}

impl FrameData {
    pub fn map(&self, ref_pts: &[P], warp: bool, p: P) -> P {
        let q = apply(&self.h, p);
        if !warp {
            return q;
        }
        let (mut sx, mut sy, mut sw) = (0f32, 0f32, 0f32);
        for (r, c) in ref_pts.iter().zip(&self.pts) {
            let d2 = (p[0] - r[0]).powi(2) + (p[1] - r[1]).powi(2);
            if d2 < 1e-6 {
                return *c;
            }
            let m = apply(&self.h, *r);
            sx += (c[0] - m[0]) / d2;
            sy += (c[1] - m[1]) / d2;
            sw += 1. / d2;
        }
        if sw == 0. { q } else { [q[0] + sx / sw, q[1] + sy / sw] }
    }

    pub fn unmap(&self, ref_pts: &[P], warp: bool, q: P) -> P {
        let inv = inv(&self.h);
        let mut p = apply(&inv, q);
        if warp {
            for _ in 0..3 {
                let (m, lin) = (self.map(ref_pts, true, p), apply(&self.h, p));
                p = apply(&inv, [q[0] - (m[0] - lin[0]), q[1] - (m[1] - lin[1])]);
            }
        }
        p
    }
}

pub struct Gray {
    pub w: usize,
    pub h: usize,
    d: Vec<f32>,
}

impl Gray {
    pub fn from_rgb(rgb: &[u8], w: usize, h: usize, ch: Channel) -> Gray {
        let d = rgb
            .chunks_exact(3)
            .map(|p| match ch {
                Channel::Luma | Channel::Auto => 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32,
                Channel::Mix(k) => k[0] * p[0] as f32 + k[1] * p[1] as f32 + k[2] * p[2] as f32,
                Channel::R => p[0] as f32,
                Channel::G => p[1] as f32,
                Channel::B => p[2] as f32,
            })
            .collect();
        Gray { w, h, d }
    }

    pub fn view(&self) -> Vec<u8> {
        let low = self.d.iter().fold(0f32, |m, v| m.min(*v));
        self.d.iter().map(|v| (v - low).clamp(0., 255.) as u8).collect()
    }

    fn px(&self, x: i32, y: i32) -> f32 {
        self.d[y.clamp(0, self.h as i32 - 1) as usize * self.w + x.clamp(0, self.w as i32 - 1) as usize]
    }

    pub fn at(&self, x: f32, y: f32) -> f32 {
        let x = x.clamp(0., (self.w - 1) as f32);
        let y = y.clamp(0., (self.h - 1) as f32);
        let (x0, y0) = (x as usize, y as usize);
        let (x1, y1) = ((x0 + 1).min(self.w - 1), (y0 + 1).min(self.h - 1));
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let (a, b) = (self.d[y0 * self.w + x0], self.d[y0 * self.w + x1]);
        let (c, d) = (self.d[y1 * self.w + x0], self.d[y1 * self.w + x1]);
        (a + (b - a) * fx) * (1. - fy) + (c + (d - c) * fx) * fy
    }

    fn half(&self) -> Gray {
        const K: [f32; 5] = [1. / 16., 4. / 16., 6. / 16., 4. / 16., 1. / 16.];
        let (w, h) = (self.w.div_ceil(2), self.h.div_ceil(2));
        let mut tmp = vec![0f32; w * self.h];
        for y in 0..self.h {
            for x in 0..w {
                tmp[y * w + x] = (0..5).map(|i| K[i] * self.px(2 * x as i32 + i as i32 - 2, y as i32)).sum();
            }
        }
        let mut d = vec![0f32; w * h];
        for y in 0..h {
            for x in 0..w {
                d[y * w + x] = (0..5).map(|i| K[i] * tmp[(2 * y as i32 + i as i32 - 2).clamp(0, self.h as i32 - 1) as usize * w + x]).sum();
            }
        }
        Gray { w, h, d }
    }
}

fn levels(p: &Params) -> usize {
    1 + (p.search / p.pattern as f32).max(1.).log2().ceil() as usize
}

pub fn pyramid(g: Gray, search: f32, pattern: i32) -> Vec<Gray> {
    let levels = 1 + (search / pattern as f32).max(1.).log2().ceil() as usize;
    let mut v = vec![g];
    while (v.len() < levels || v[v.len() - 1].w > 320) && v[v.len() - 1].w.min(v[v.len() - 1].h) >= 8 * pattern as usize {
        let next = v[v.len() - 1].half();
        v.push(next);
    }
    v
}

const MIN_EIG: f32 = 1.0;

fn lk(a: &[Gray], b: &[Gray], p: P, start: P, m: [f32; 4], half: i32) -> Option<P> {
    let n = (2 * half + 1) as usize;
    let side = n + 2;
    let mut patch = vec![0f32; side * side];
    let (mut t, mut gx, mut gy, mut cur) = (vec![0f32; n * n], vec![0f32; n * n], vec![0f32; n * n], vec![0f32; n * n]);
    let top = (1u32 << (a.len() - 1)) as f32;
    let mut g = [(start[0] - p[0]) / top, (start[1] - p[1]) / top];
    for l in (0..a.len()).rev() {
        let s = (1u32 << l) as f32;
        let q = [p[0] / s, p[1] / s];
        for j in 0..side {
            for i in 0..side {
                patch[j * side + i] = a[l].at(q[0] + (i as i32 - half - 1) as f32, q[1] + (j as i32 - half - 1) as f32);
            }
        }
        let (mut gxx, mut gxy, mut gyy, mut tm) = (0f32, 0f32, 0f32, 0f32);
        for j in 0..n {
            for i in 0..n {
                let c = (j + 1) * side + i + 1;
                let (ix, iy) = ((patch[c + 1] - patch[c - 1]) * 0.5, (patch[c + side] - patch[c - side]) * 0.5);
                let k = j * n + i;
                (t[k], gx[k], gy[k]) = (patch[c], ix, iy);
                gxx += ix * ix;
                gxy += ix * iy;
                gyy += iy * iy;
                tm += patch[c];
            }
        }
        tm /= (n * n) as f32;
        let min_eig = (gxx + gyy - ((gxx - gyy).powi(2) + 4. * gxy * gxy).sqrt()) * 0.5;
        if min_eig >= MIN_EIG * (n * n) as f32 {
            let det = gxx * gyy - gxy * gxy;
            for _ in 0..20 {
                let mut cm = 0f32;
                for j in 0..n {
                    for i in 0..n {
                        let (ox, oy) = ((i as i32 - half) as f32, (j as i32 - half) as f32);
                        let v = b[l].at(q[0] + g[0] + m[0] * ox + m[1] * oy, q[1] + g[1] + m[2] * ox + m[3] * oy);
                        cur[j * n + i] = v;
                        cm += v;
                    }
                }
                cm /= (n * n) as f32;
                let (mut bx, mut by) = (0f32, 0f32);
                for k in 0..n * n {
                    let d = (t[k] - tm) - (cur[k] - cm);
                    bx += d * gx[k];
                    by += d * gy[k];
                }
                let (dx, dy) = ((gyy * bx - gxy * by) / det, (gxx * by - gxy * bx) / det);
                g = [g[0] + m[0] * dx + m[1] * dy, g[1] + m[2] * dx + m[3] * dy];
                if dx * dx + dy * dy < 1e-4 {
                    break;
                }
            }
        } else if l == 0 {
            return None;
        }
        if l > 0 {
            g = [g[0] * 2., g[1] * 2.];
        }
    }
    let r = [p[0] + g[0], p[1] + g[1]];
    (r[0] >= 0. && r[1] >= 0. && r[0] <= (b[0].w - 1) as f32 && r[1] <= (b[0].h - 1) as f32).then_some(r)
}

fn dist(a: P, b: P) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

fn par<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let chunk = items.len().div_ceil(threads).max(1);
    std::thread::scope(|sc| {
        let hs: Vec<_> = items.chunks(chunk).map(|c| sc.spawn(|| c.iter().map(&f).collect::<Vec<_>>())).collect();
        hs.into_iter().flat_map(|h| h.join().unwrap()).collect()
    })
}

fn track_pts(a: &[Gray], b: &[Gray], pts: &[P], starts: &[P], warp: &(dyn Fn(usize) -> [f32; 4] + Sync), limit: f32, half: i32) -> Vec<Option<P>> {
    let one = |&i: &usize| {
        let (q, s, m) = (pts[i], starts[i], warp(i));
        let det = m[0] * m[3] - m[1] * m[2];
        let inv = [m[3] / det, -m[1] / det, -m[2] / det, m[0] / det];
        let r = lk(a, b, q, s, m, half)?;
        let back = lk(b, a, r, q, inv, half)?;
        (dist(back, q) < 1.0 && dist(r, s) <= limit).then_some(r)
    };
    par(&(0..pts.len()).collect::<Vec<_>>(), one)
}

fn jacobian(h: &H, p: P) -> [f32; 4] {
    let (px, mx, py, my) = (apply(h, [p[0] + 1., p[1]]), apply(h, [p[0] - 1., p[1]]), apply(h, [p[0], p[1] + 1.]), apply(h, [p[0], p[1] - 1.]));
    [(px[0] - mx[0]) / 2., (py[0] - my[0]) / 2., (px[1] - mx[1]) / 2., (py[1] - my[1]) / 2.]
}

fn realign(refp: &[Gray], b: &[Gray], anchors: &[P], refs: &[P], fd: &FrameData, p: &Params, limit: f32) -> Option<FrameData> {
    let (refp, b) = (&refp[..levels(p).min(refp.len())], &b[..levels(p).min(b.len())]);
    let h = fd.h;
    let starts: Vec<P> = if p.warp { fd.pts.clone() } else { anchors.iter().map(|q| apply(&h, *q)).collect() };
    let found = track_pts(refp, b, anchors, &starts, &|i| jacobian(&h, anchors[i]), limit, p.pattern);
    let mut ok: Vec<bool> = found.iter().map(Option::is_some).collect();
    let dst: Vec<P> = found.iter().zip(&starts).map(|(f, s)| f.unwrap_or(*s)).collect();
    if p.warp {
        let h = fit(&p.motion, anchors, &dst, &vec![true; dst.len()])?;
        return Some(FrameData { h, pts: dst, opacity: fd.opacity, guessed: false, score: 1. });
    }
    let better = fit_robust(&p.motion, anchors, &dst, &mut ok)?;
    let kept = ok.iter().filter(|o| **o).count();
    if kept < 8 || kept * 5 < anchors.len() || anchors.iter().any(|q| dist(apply(&better, *q), apply(&h, *q)) > 2. * limit) {
        return None;
    }
    Some(FrameData { h: better, pts: refs.iter().map(|q| apply(&better, *q)).collect(), opacity: fd.opacity, guessed: false, score: 1. })
}

fn edge(g: &Gray, x: f32, y: f32) -> f32 {
    (g.at(x + 1., y) - g.at(x - 1., y)).hypot(g.at(x, y + 1.) - g.at(x, y - 1.))
}

struct Sample {
    p: P,
    t: f32,
    part: usize,
    j: [f64; 8],
}

#[derive(Clone, Copy)]
pub struct Match {
    pub h: H,
    pub score: f32,
    pub weakest: f32,
    pub gain: f32,
}

pub struct Template {
    c: [f64; 2],
    s: f64,
    levels: Vec<(Vec<Sample>, f32, [bool; 4])>,
}

impl Template {
    pub fn new(refp: &[Gray], zone: &[P], margin: f32, hole: &[P]) -> Template {
        let b = bbox(zone);
        let b = [b[0] - margin, b[1] - margin, b[2] + margin, b[3] + margin];
        let c = [((b[0] + b[2]) / 2.) as f64, ((b[1] + b[3]) / 2.) as f64];
        let s = (((b[2] - b[0]) + (b[3] - b[1])) / 4.).max(1.) as f64;
        let levels = (0..refp.len())
            .map(|l| {
                let k = (1u32 << l) as f32;
                let st = k.max(((b[2] - b[0]) * (b[3] - b[1]) / 12000.).sqrt());
                let e = |x: f32, y: f32| edge(&refp[l], x / k, y / k);
                let mut v = vec![];
                let mut row = b[1];
                while row <= b[3] {
                    let mut col = b[0];
                    while col <= b[2] {
                        let n = v.len() as f32;
                        let (x, y) = (col + st * ((n * 0.754_877_7).fract() - 0.5), row + st * ((n * 0.569_840_3).fract() - 0.5));
                        let g = [((e(x + k, y) - e(x - k, y)) / (2. * k)) as f64 * s, ((e(x, y + k) - e(x, y - k)) / (2. * k)) as f64 * s];
                        let (u, w) = ((x as f64 - c[0]) / s, (y as f64 - c[1]) / s);
                        let f = [[1., 0.], [0., 1.], [u, w], [-w, u], [u, -w], [w, u], [-u * u, -u * w], [-u * w, -w * w]];
                        v.push((Sample { p: [x, y], t: e(x, y), part: (u > 0.) as usize + 2 * (w > 0.) as usize, j: f.map(|f| g[0] * f[0] + g[1] * f[1]) }, in_poly(zone, [x, y])));
                        col += st;
                    }
                    row += st;
                }
                let top = v.iter().fold(0f32, |m, q| m.max(q.0.t));
                let v: Vec<Sample> = v.into_iter().filter(|q| (q.1 || q.0.t > 0.1 * top) && !in_poly(hole, q.0.p)).map(|q| q.0).collect();
                let energy = |part: usize| v.iter().filter(|q| q.part == part).map(|q| q.t * q.t).sum::<f32>();
                let all: f32 = (0..4).map(energy).sum();
                let parts = std::array::from_fn(|k| energy(k) >= 0.08 * all);
                (v, top, parts)
            })
            .collect();
        Template { c, s, levels }
    }

    fn look(&self, b: &[Gray], h: &H, l: usize, at: usize) -> (Vec<f32>, Match) {
        let k = (1u32 << at) as f32;
        let sm = &self.levels[l].0;
        let cur: Vec<f32> = sm.iter().map(|q| apply(h, q.p)).map(|w| edge(&b[at], w[0] / k, w[1] / k)).collect();
        let mut sums = [[1e-6f32; 3]; 4];
        for (v, q) in cur.iter().zip(sm) {
            let g = &mut sums[q.part];
            *g = [g[0] + v * q.t, g[1] + v * v, g[2] + q.t * q.t];
        }
        let all: [f32; 3] = std::array::from_fn(|i| sums.iter().map(|g| g[i]).sum());
        let cos = |g: &[f32; 3]| g[0] / (g[1] * g[2]).sqrt();
        let weakest = (0..4).filter(|&k| self.levels[l].2[k]).map(|k| cos(&sums[k])).fold(cos(&all), f32::min);
        (cur, Match { h: *h, score: cos(&all), weakest, gain: (all[1] / all[2]).sqrt() })
    }

    pub fn align(&self, b: &[Gray], start: &H, m: &Motion, top: usize) -> Match {
        let free = m.shear || m.perspective;
        let on = [m.translation, m.translation, m.scale || free, m.rotation || free, free, free, m.perspective, m.perspective];
        let (c, s) = (self.c, self.s);
        let pre = [1. / s, 0., -c[0] / s, 0., 1. / s, -c[1] / s, 0., 0., 1.];
        let post = [s, 0., c[0], 0., s, c[1], 0., 0., 1.];
        let mut out = Match { h: *start, score: 0., weakest: 0., gain: 0. };
        let m = jacobian(start, [c[0] as f32, c[1] as f32]);
        let shrink = (-(m[0] * m[3] - m[1] * m[2]).abs().max(1e-9).sqrt().log2()).round().clamp(1. - b.len() as f32, self.levels.len() as f32 - 1.) as i32;
        for (l, at) in (0..top).map(|l| (l + shrink.max(0) as usize, l + (-shrink).max(0) as usize)).filter(|&(l, at)| l < self.levels.len() && at < b.len()).rev() {
            let (sm, peak, _) = &self.levels[l];
            let mut h = out.h;
            out.score = f32::MIN;
            for round in 0..16 {
                let (cur, seen) = self.look(b, &h, l, at);
                if seen.score > out.score {
                    out = seen;
                }
                if seen.score < out.score - 0.01 || round == 15 {
                    break;
                }
                let gain = seen.gain.clamp(0.5, 10.);
                let res: Vec<f32> = cur.iter().zip(sm).map(|(v, q)| v / gain - q.t).collect();
                let mut mags: Vec<f32> = res.iter().zip(sm).filter(|(_, q)| q.t > 0.1 * peak).map(|(r, _)| r.abs()).collect();
                mags.sort_by(f32::total_cmp);
                let kh = (2. * mags.get(mags.len() / 2).copied().unwrap_or(0.)).max(0.05 * peak);
                let rows = sm.iter().zip(&res).map(|(q, r)| {
                    let w = (1. / (1. + (r / kh).powi(2)).sqrt()) as f64;
                    (std::array::from_fn(|k| if on[k] { q.j[k] * w } else { 0. }), *r as f64 * w)
                });
                let damp = (0..8).map(|k| (std::array::from_fn(|i| if i == k { 1. } else { 0. }), 0.));
                let Some(d) = lstsq::<8>(rows.chain(damp)) else { break };
                let step = mul(&post, &mul(&[1. + d[2] + d[4], d[5] - d[3], d[0], d[3] + d[5], 1. + d[2] - d[4], d[1], d[6], d[7], 1.], &pre));
                let next = mul(&h, &inv(&step));
                if next.iter().any(|v| !v.is_finite()) || next[8].abs() < 1e-9 || d.iter().all(|v| v.abs() * s < 0.02 * (1u32 << l) as f64) {
                    break;
                }
                h = next.map(|v| v / next[8]);
            }
        }
        out
    }

    pub fn search(&self, b: &[Gray], around: &H, m: &Motion, turn: f32) -> Vec<H> {
        let l = self.levels.len().min(b.len()) - 1;
        let k = (1u32 << l) as f32;
        let (g, sm) = (&b[l], &self.levels[l].0);
        let centre = apply(around, [self.c[0] as f32, self.c[1] as f32]);
        let pick: Vec<(P, f32)> = sm.iter().step_by((sm.len() / 256).max(1)).map(|q| (apply(around, q.p), q.t)).map(|(w, t)| ([(w[0] - centre[0]) / k, (w[1] - centre[1]) / k], t)).collect();
        let reach = pick.iter().map(|q| q.0[0].hypot(q.0[1])).fold(0f32, f32::max);
        let free = m.shear || m.perspective;
        let scales: &[f32] = if m.scale || free { &[0.8, 1., 1.25] } else { &[1.] };
        let turns = if m.rotation || free { (turn / 8.).ceil() as i32 } else { 0 };
        let pad = (reach * 1.25).ceil() as usize + 2;
        let (w, h) = (g.w + 2 * pad, g.h + 2 * pad);
        let mut map = vec![0f32; w * h];
        for y in 1..g.h.saturating_sub(1) {
            for x in 1..g.w.saturating_sub(1) {
                map[(y + pad) * w + x + pad] = (g.d[y * g.w + x + 1] - g.d[y * g.w + x - 1]).hypot(g.d[(y + 1) * g.w + x] - g.d[(y - 1) * g.w + x]);
            }
        }
        let et = pick.iter().map(|q| q.1 * q.1).sum::<f32>().max(1e-6);
        let poses: Vec<(f32, f32)> = (-turns..=turns).flat_map(|t| scales.iter().map(move |z| ((t as f32 * 8.).to_radians(), *z))).collect();
        let one = |&(th, z): &(f32, f32)| {
            let (sin, cos) = th.sin_cos();
            let offs: Vec<(usize, f32)> = pick
                .iter()
                .map(|(d, t)| {
                    let (x, y) = (z * (cos * d[0] - sin * d[1]), z * (sin * d[0] + cos * d[1]));
                    (((y.round() as i64 + pad as i64) * w as i64 + x.round() as i64 + pad as i64) as usize, *t)
                })
                .collect();
            let mut best = (0f32, 0usize, 0usize);
            for y in 0..g.h {
                for x in 0..g.w {
                    let at = y * w + x;
                    let (mut dot, mut ec) = (0f32, 1e-6f32);
                    for (o, t) in &offs {
                        let v = map[at + o];
                        dot += v * t;
                        ec += v * v;
                    }
                    let sc = dot / (ec * et).sqrt();
                    if sc > best.0 {
                        best = (sc, x, y);
                    }
                }
            }
            let lin = [(z * cos) as f64, -(z * sin) as f64, 0., (z * sin) as f64, (z * cos) as f64, 0., 0., 0., 1.];
            let place = mul(&translation((best.1 as f32 * k) as f64, (best.2 as f32 * k) as f64), &mul(&lin, &translation(-centre[0] as f64, -centre[1] as f64)));
            (best.0, [best.1 as f32, best.2 as f32], mul(&place, around))
        };
        let mut all: Vec<(f32, P, H)> = par(&poses, one);
        all.sort_by(|a, b| b.0.total_cmp(&a.0));
        let mut kept: Vec<(f32, P, H)> = vec![];
        for c in all {
            if kept.len() < 5 && kept.iter().all(|o| dist(o.1, c.1) > reach) {
                kept.push(c);
            }
        }
        kept.into_iter().map(|c| c.2).collect()
    }
}

const ZOOMS: [f32; 10] = [0.8, 1.25, 0.63, 1.6, 0.5, 2., 0.4, 2.5, 0.32, 3.2];

const SIMILARITY: Motion = Motion { translation: true, scale: true, rotation: true, shear: false, perspective: false };

fn follow(a: &[Gray], b: &[Gray], prev: &[P], guess: &H, p: &Params) -> (Vec<Option<P>>, H) {
    let (a, b) = (&a[..levels(p).min(a.len())], &b[..levels(p).min(b.len())]);
    let found = |t: &[Option<P>]| t.iter().flatten().count();
    let lin = |h: &H| [h[0] as f32, h[1] as f32, h[3] as f32, h[4] as f32];
    let go = |pts: &[P], h: &H| track_pts(a, b, pts, &pts.iter().map(|q| apply(h, *q)).collect::<Vec<_>>(), &|_| lin(h), p.search, p.pattern);
    let measure = |t: &[Option<P>]| {
        let ok: Vec<bool> = t.iter().map(Option::is_some).collect();
        let dst: Vec<P> = t.iter().zip(prev).map(|(t, q)| t.unwrap_or(*q)).collect();
        fit(&SIMILARITY, prev, &dst, &ok)
    };
    let refine = |mut cur: Vec<Option<P>>, mut used: H| {
        for _ in 0..4 {
            let Some(h) = measure(&cur) else { break };
            if (0..4).all(|i| (lin(&h)[i] - lin(&used)[i]).abs() < 0.004) {
                break;
            }
            let fine = go(prev, &h);
            if found(&fine) < found(&cur) {
                break;
            }
            (cur, used) = (fine, h);
        }
        cur
    };

    let mut best = refine(go(prev, guess), *guess);
    if found(&best) * 10 < prev.len() * 8 && *guess != IDENT {
        let still = refine(go(prev, &IDENT), IDENT);
        if found(&still) > found(&best) {
            best = still;
        }
    }
    let lost = if p.user_pts { found(&best) < prev.len() } else { found(&best) * 2 < prev.len() };
    if lost {
        let n = prev.len() as f32;
        let c = prev.iter().fold([0f64; 2], |c, q| [c[0] + (q[0] / n) as f64, c[1] + (q[1] / n) as f64]);
        let zoom = |z: f32| -> H { [z as f64, 0., c[0] * (1. - z as f64), 0., z as f64, c[1] * (1. - z as f64), 0., 0., 1.] };
        let sample: Vec<P> = prev.iter().step_by((prev.len() / 24).max(1)).copied().collect();
        let (mut top, mut at) = (found(&best) * sample.len() / prev.len(), None);
        for z in ZOOMS {
            let got = found(&go(&sample, &zoom(z)));
            if got > top {
                (top, at) = (got, Some(z));
            }
        }
        if let Some(z) = at {
            let cur = refine(go(prev, &zoom(z)), zoom(z));
            let enough = if p.user_pts { found(&cur) == prev.len() } else { found(&cur) * 2 >= prev.len() && found(&cur) * 2 >= found(&best) * 3 };
            if enough {
                best = cur;
            }
        }
    }
    let moved = measure(&best).unwrap_or(IDENT);
    (best, moved)
}

pub fn apply(h: &H, p: P) -> P {
    let (x, y) = (p[0] as f64, p[1] as f64);
    let w = h[6] * x + h[7] * y + h[8];
    [((h[0] * x + h[1] * y + h[2]) / w) as f32, ((h[3] * x + h[4] * y + h[5]) / w) as f32]
}

pub fn mul(a: &H, b: &H) -> H {
    std::array::from_fn(|i| (0..3).map(|k| a[i / 3 * 3 + k] * b[k * 3 + i % 3]).sum())
}

pub fn translation(dx: f64, dy: f64) -> H {
    [1., 0., dx, 0., 1., dy, 0., 0., 1.]
}

pub fn inv(h: &H) -> H {
    let c = |a: usize, b: usize, c: usize, d: usize| h[a] * h[b] - h[c] * h[d];
    let adj = [c(4, 8, 5, 7), c(2, 7, 1, 8), c(1, 5, 2, 4), c(5, 6, 3, 8), c(0, 8, 2, 6), c(2, 3, 0, 5), c(3, 7, 4, 6), c(1, 6, 0, 7), c(0, 4, 1, 3)];
    let det = h[0] * adj[0] + h[1] * adj[3] + h[2] * adj[6];
    adj.map(|v| v / det)
}

fn lstsq<const N: usize>(rows: impl Iterator<Item = ([f64; N], f64)>) -> Option<[f64; N]> {
    let (mut m, mut v) = ([[0f64; N]; N], [0f64; N]);
    for (r, y) in rows {
        for i in 0..N {
            v[i] += r[i] * y;
            for j in 0..N {
                m[i][j] += r[i] * r[j];
            }
        }
    }
    for c in 0..N {
        let piv = (c..N).max_by(|&i, &j| m[i][c].abs().total_cmp(&m[j][c].abs()))?;
        if m[piv][c].abs() < 1e-10 {
            return None;
        }
        m.swap(c, piv);
        v.swap(c, piv);
        for r in 0..N {
            if r != c {
                let f = m[r][c] / m[c][c];
                for k in c..N {
                    m[r][k] -= f * m[c][k];
                }
                v[r] -= f * v[c];
            }
        }
    }
    Some(std::array::from_fn(|i| v[i] / m[i][i]))
}

pub fn fit(m: &Motion, src: &[P], dst: &[P], ok: &[bool]) -> Option<H> {
    let idx: Vec<usize> = (0..src.len()).filter(|&i| ok[i]).collect();
    let n = idx.len();
    if n == 0 {
        return None;
    }
    let centre = |pts: &[P]| {
        let s = idx.iter().fold([0f64; 2], |s, &i| [s[0] + pts[i][0] as f64, s[1] + pts[i][1] as f64]);
        [s[0] / n as f64, s[1] / n as f64]
    };
    let (cs, cd) = (centre(src), centre(dst));
    let s = (idx.iter().map(|&i| (src[i][0] as f64 - cs[0]).hypot(src[i][1] as f64 - cs[1])).sum::<f64>() / n as f64).max(1e-6);
    let norm: Vec<[f64; 4]> = idx
        .iter()
        .map(|&i| [(src[i][0] as f64 - cs[0]) / s, (src[i][1] as f64 - cs[1]) / s, (dst[i][0] as f64 - cd[0]) / s, (dst[i][1] as f64 - cd[1]) / s])
        .collect();

    let mut hn: Option<H> = None;
    if m.perspective && n >= 4 {
        let rows = norm.iter().flat_map(|&[x, y, u, v]| [([x, y, 1., 0., 0., 0., -x * u, -y * u], u), ([0., 0., 0., x, y, 1., -x * v, -y * v], v)]);
        hn = lstsq::<8>(rows).map(|h| [h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7], 1.]);
    }
    if hn.is_none() && (m.perspective || m.shear) && n >= 3 {
        let rows = norm.iter().flat_map(|&[x, y, u, v]| [([x, y, 1., 0., 0., 0.], u), ([0., 0., 0., x, y, 1.], v)]);
        hn = lstsq::<6>(rows).map(|a| [a[0], a[1], a[2], a[3], a[4], a[5], 0., 0., 1.]);
    }
    let hn = hn.unwrap_or_else(|| {
        let (mut dot, mut cross, mut pp) = (0f64, 0f64, 0f64);
        for &[x, y, u, v] in &norm {
            dot += x * u + y * v;
            cross += x * v - y * u;
            pp += x * x + y * y;
        }
        let (mut a, mut b) = (1f64, 0f64);
        if n >= 2 && pp > 1e-9 {
            if m.rotation {
                let k = if m.scale { dot.hypot(cross) / pp } else { 1. };
                let th = cross.atan2(dot);
                (a, b) = (k * th.cos(), k * th.sin());
            } else if m.scale {
                a = (dot / pp).max(0.01);
            }
        }
        [a, -b, 0., b, a, 0., 0., 0., 1.]
    });
    let pre = [1. / s, 0., -cs[0] / s, 0., 1. / s, -cs[1] / s, 0., 0., 1.];
    let post = [s, 0., cd[0], 0., s, cd[1], 0., 0., 1.];
    let mut h = mul(&post, &mul(&hn, &pre));
    if !m.translation {
        let c = [cs[0] as f32, cs[1] as f32];
        let q = apply(&h, c);
        h = mul(&translation((c[0] - q[0]) as f64, (c[1] - q[1]) as f64), &h);
    }
    let w = h[8];
    Some(h.map(|v| v / w))
}

fn fit_robust(m: &Motion, src: &[P], dst: &[P], ok: &mut [bool]) -> Option<H> {
    let mut h = fit(m, src, dst, ok)?;
    for _ in 0..3 {
        let res: Vec<f32> = src.iter().zip(dst).map(|(s, d)| dist(apply(&h, *s), *d)).collect();
        let mut good: Vec<f32> = res.iter().zip(ok.iter()).filter(|(_, o)| **o).map(|(r, _)| *r).collect();
        good.sort_by(f32::total_cmp);
        let thr = (2.5 * good[good.len() / 2]).max(1.0);
        let before = good.len();
        for (o, r) in ok.iter_mut().zip(&res) {
            *o &= *r <= thr;
        }
        if ok.iter().filter(|o| **o).count() == before {
            break;
        }
        h = fit(m, src, dst, ok)?;
    }
    Some(h)
}

fn opacity(g: &Gray, fd: &FrameData, ref_pts: &[P], warp: bool, opa: &[(P, f32)]) -> f32 {
    if opa.len() < 8 {
        return 1.;
    }
    let n = opa.len() as f32;
    let mr = opa.iter().map(|o| o.1).sum::<f32>() / n;
    let var: f32 = opa.iter().map(|o| (o.1 - mr) * (o.1 - mr)).sum();
    if var < 4. * n {
        return 1.;
    }
    let at: Vec<P> = opa.iter().map(|(p, _)| fd.map(ref_pts, warp, *p)).collect();
    let mut best = 0f32;
    for (dx, dy) in (-1..=1).flat_map(|dy| (-1..=1).map(move |dx| (dx as f32, dy as f32))) {
        let cur: Vec<f32> = at.iter().map(|q| g.at(q[0] + dx, q[1] + dy)).collect();
        let mc = cur.iter().sum::<f32>() / n;
        let cov: f32 = opa.iter().zip(&cur).map(|((_, r), c)| (r - mr) * (c - mc)).sum();
        best = best.max(cov / var);
    }
    best.clamp(0., 4.)
}

fn step(a: &[Gray], b: &[Gray], ref_pts: &[P], prev: &FrameData, guess: &H, p: &Params, opa: &[(P, f32)]) -> Result<(FrameData, usize, H), String> {
    let (tracked, moved) = follow(a, b, &prev.pts, guess, p);
    let mut ok: Vec<bool> = tracked.iter().map(Option::is_some).collect();
    if p.user_pts {
        if let Some(i) = ok.iter().position(|o| !*o) {
            return Err(tf!("point {} perdu — corrigez-le à la main puis relancez", "point {} lost — fix it by hand, then run again", i + 1));
        }
    }
    let mut pts: Vec<P> = tracked.iter().zip(&prev.pts).map(|(t, q)| t.unwrap_or(*q)).collect();
    let h = if p.warp { fit(&p.motion, ref_pts, &pts, &ok) } else { fit_robust(&p.motion, ref_pts, &pts, &mut ok) };
    let h = h.ok_or(t!("plus aucun point suivi", "no tracked point left"))?;
    if !p.warp {
        let reach = (0..pts.len()).filter(|&i| ok[i]).map(|i| dist(pts[i], prev.pts[i])).fold(0f32, f32::max);
        if ref_pts.iter().zip(&prev.pts).any(|(r, old)| dist(apply(&h, *r), *old) > 2. * reach + p.pattern as f32) {
            return Err(t!("trop peu de points suivis", "too few tracked points").into());
        }
    }
    let m = &p.motion;
    let need = if m.perspective { 4 } else if m.shear { 3 } else if m.scale || m.rotation { 2 } else { 1 };
    if ok.iter().filter(|o| **o).count() < (pts.len() / 10).max(need.min(pts.len())) {
        return Err(t!("trop peu de points suivis", "too few tracked points").into());
    }
    for i in (0..pts.len()).filter(|&i| !ok[i]) {
        let q = apply(&h, ref_pts[i]);
        pts[i] = if p.warp {
            let old = apply(&prev.h, ref_pts[i]);
            [q[0] + prev.pts[i][0] - old[0], q[1] + prev.pts[i][1] - old[1]]
        } else {
            q
        };
    }
    let score = ok.iter().filter(|o| **o).count() as f32 / ok.len().max(1) as f32;
    let mut fd = FrameData { h, pts, opacity: 1., guessed: false, score };
    fd.opacity = opacity(&b[0], &fd, ref_pts, p.warp, opa);
    Ok((fd, ok.iter().filter(|o| **o).count(), moved))
}

fn reseed(g: &Gray, zone: &[P], fd: &FrameData, hole: &[P]) -> Option<(Vec<P>, FrameData)> {
    let poly: Vec<P> = zone.iter().map(|v| apply(&fd.h, *v)).collect();
    let back = inv(&fd.h);
    let mut found = detect(g, &poly);
    found.retain(|q| !in_poly(hole, apply(&back, *q)));
    if found.len() < 4 {
        return None;
    }
    let n = fd.pts.len();
    let pts: Vec<P> = (0..n).map(|i| found[i * found.len() / n]).collect();
    Some((pts.iter().map(|q| apply(&back, *q)).collect(), FrameData { pts, ..fd.clone() }))
}

fn visible(g: &Gray, zone: &[P], fd: &FrameData) -> f32 {
    let b = bbox(&zone.iter().map(|v| apply(&fd.h, *v)).collect::<Vec<_>>());
    let w = (b[2].min(g.w as f32) - b[0].max(0.)).max(0.);
    let h = (b[3].min(g.h as f32) - b[1].max(0.)).max(0.);
    w * h / ((b[2] - b[0]) * (b[3] - b[1])).max(1.)
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    load: &dyn Fn(usize) -> Option<Vec<Gray>>,
    ref_pts: &[P],
    zone: &[P],
    opa: &[(P, f32)],
    ref_frame: Option<usize>,
    start: usize,
    fd: FrameData,
    end: usize,
    p: &Params,
    stop: &AtomicBool,
    emit: &mut dyn FnMut(usize, FrameData),
) -> Result<(), String> {
    let mut a = load(start).ok_or(t!("lecture de la vidéo impossible", "cannot read the video"))?;
    let refp = ref_frame.filter(|_| p.realign).and_then(load);
    let renew = !p.warp && zone.len() >= 3;
    let tpl = refp.as_ref().filter(|_| renew).map(|r| Template::new(r, zone, p.pattern as f32, &p.hole));
    let mut refs: Vec<P> = if renew { fd.pts.iter().map(|q| apply(&inv(&fd.h), *q)).collect() } else { ref_pts.to_vec() };
    let (mut prev, mut i) = (fd, start);
    let mut peak = prev.opacity;
    let c0 = refs_centre(zone, ref_pts);
    let centre = |f: &FrameData| apply(&f.h, c0);
    let mut trail = vec![centre(&prev)];
    let kept = trail[0];
    let pin = |f: FrameData| pinned(f, c0, kept, p.axis);
    let mut guess = IDENT;
    let (mut base, mut since, mut lost) = (1f32, u32::MAX, 0u32);
    while i != end && !stop.load(Ordering::Relaxed) {
        let j = if end > i { i + 1 } else { i - 1 };
        let b = load(j).ok_or(t!("lecture de la vidéo impossible", "cannot read the video"))?;
        let faint = p.hold_fades && prev.opacity < 0.3 * peak;
        let fading = p.hold_fades && prev.opacity < 0.5 * peak;
        let pred = mul(&guess, &prev.h);
        let tpl = tpl.as_ref().filter(|_| !fading);
        let dense = |h: &H, top: usize| tpl.map(|t| t.align(&b, h, &p.motion, top));
        let near = dense(&pred, if lost > 0 { 9 } else { 2 });
        let back = |f: &Match| f.score >= 0.75 * base && f.weakest >= 0.6 * base;
        let direct = near.is_some_and(|f| if lost > 0 { back(&f) } else { f.score >= 0.95 * base });
        let mut tracked = if faint || lost > 0 || direct { Err(String::new()) } else { step(&a, &b, &refs, &prev, &guess, p, opa) };
        if tracked.is_err() && renew && !fading && lost == 0 && !direct {
            if let Some((r, f)) = reseed(&a[0], zone, &prev, &p.hole) {
                if let Ok(t) = step(&a, &b, &r, &f, &guess, p, opa) {
                    (refs, tracked) = (r, Ok(t));
                }
            }
        }
        if fading && matches!(&tracked, Ok((f, inliers, _)) if inliers * 2 < f.pts.len()) {
            tracked = Err(String::new());
        }
        let mut found = near.filter(|_| direct);
        let mut far = false;
        if let (Some(t), false) = (tpl, direct) {
            let others: Vec<H> = if lost >= 5 {
                t.search(&b, &pred, &p.motion, 40. + 2. * lost as f32)
            } else if lost > 0 {
                vec![]
            } else {
                tracked.iter().map(|t| t.0.h).chain([pred]).collect()
            };
            let grown = |h: &H| {
                let (from, to): (Vec<P>, Vec<P>) = zone.iter().map(|v| (apply(&pred, *v), apply(h, *v))).unzip();
                fit(&SIMILARITY, &from, &to, &vec![true; from.len()]).map_or(9., |m| (m[0].hypot(m[3]).ln().abs()) as f32)
            };
            let reach = (lost + 2) as f32 * (2. * dist(apply(&guess, centre(&prev)), centre(&prev)) + p.pattern as f32);
            let fits = |f: &Match| lost == 0 || (back(f) && f.score >= 0.85 * base && f.gain >= 0.5 && grown(&f.h) <= 0.1 + 0.005 * lost as f32 && dist(apply(&f.h, c0), apply(&pred, c0)) <= reach);
            let best = par(&others, |h| t.align(&b, h, &p.motion, 9)).into_iter().filter(fits).chain(near.filter(|_| lost == 0)).max_by(|x, y| x.score.total_cmp(&y.score));
            found = best.filter(|f| lost > 0 || f.score >= if since == u32::MAX { 0.5 } else { 0.8 * base });
            far = lost > 0 && found.is_some();
            if found.is_none() && lost == 0 {
                found = near.filter(|f| f.score >= 0.6 * base);
            }
        }
        let drift = |h: &H, from: &H| zone.iter().map(|v| dist(apply(h, *v), apply(from, *v))).fold(0f32, f32::max);
        let trusted = matches!(&tracked, Ok((f, inliers, _)) if inliers * 2 >= f.pts.len() && drift(&f.h, &pred) <= 0.5 * (drift(&pred, &prev.h) + p.pattern as f32));
        if let Some(Match { h, score, .. }) = found {
            let sure = score >= 0.8 * base;
            let mut f = FrameData { h, pts: refs.iter().map(|q| apply(&h, *q)).collect(), opacity: prev.opacity, guessed: false, score: (score / base).min(1.) };
            f.opacity = opacity(&b[0], &f, &refs, false, opa);
            let (from, to): (Vec<P>, Vec<P>) = zone.iter().map(|v| (apply(&prev.h, *v), apply(&h, *v))).unzip();
            let moved = if far { IDENT } else if lost > 0 || !sure { guess } else { fit(&SIMILARITY, &from, &to, &vec![true; from.len()]).unwrap_or(IDENT) };
            if since == u32::MAX {
                base = score;
            } else if score >= 0.9 * base {
                base = 0.9 * base + 0.1 * score;
            }
            if far {
                trail = vec![centre(&f)];
            }
            tracked = Ok((f, usize::MAX, moved));
        } else if tpl.is_some() && (lost > 0 || tracked.is_err() || (since < 10 && !trusted)) {
            let (k, c) = (guess[0].hypot(guess[3]).max(1e-9), centre(&prev));
            let (to, slow) = (apply(&guess, c), k.powf(0.8) / k);
            let lin = [guess[0] * slow, guess[1] * slow, 0., guess[3] * slow, guess[4] * slow, 0., 0., 0., 1.];
            guess = mul(&translation(to[0] as f64, to[1] as f64), &mul(&lin, &translation(-c[0] as f64, -c[1] as f64)));
            let pred = mul(&guess, &prev.h);
            let held = pin(FrameData { h: pred, pts: prev.pts.iter().map(|q| apply(&guess, *q)).collect(), opacity: prev.opacity, guessed: true, score: 0. });
            emit(j, held.clone());
            guess = carry(&guess, centre(&prev), centre(&held));
            (a, i, prev, lost) = (b, j, held, lost + 1);
            continue;
        }
        (since, lost) = (if found.is_some() { 0 } else { since.saturating_add(1) }, 0);
        if let Ok((fd, _, moved)) = &mut tracked {
            guess = *moved;
            if let Some(better) = refp.as_ref().filter(|_| !fading && p.warp).and_then(|r| realign(r, &b, ref_pts, &refs, fd, p, p.pattern as f32)) {
                *fd = FrameData { opacity: opacity(&b[0], &better, &refs, p.warp, opa), ..better };
            }
        }
        prev = match tracked {
            Ok((fd, inliers, _)) if renew && !fading && inliers.saturating_mul(10) < fd.pts.len() * 6 => match reseed(&b[0], zone, &fd, &p.hole) {
                Some((r, f)) => {
                    refs = r;
                    f
                }
                None => fd,
            },
            Ok((fd, ..)) => fd,
            Err(_) if fading || (renew && visible(&b[0], zone, &prev) < 0.3) => {
                let (first, last, steps) = (trail[0], trail[trail.len() - 1], (trail.len() - 1).max(1) as f32);
                let v = [(last[0] - first[0]) / steps, (last[1] - first[1]) / steps];
                let mut held = pin(FrameData { h: mul(&translation(v[0] as f64, v[1] as f64), &prev.h), pts: prev.pts.iter().map(|q| [q[0] + v[0], q[1] + v[1]]).collect(), opacity: prev.opacity, guessed: true, score: 0. });
                if fading {
                    held.opacity = opacity(&b[0], &held, &refs, p.warp, opa);
                }
                emit(j, held.clone());
                (a, i, prev) = (b, j, held);
                continue;
            }
            Err(e) => return Err(tf!("Frame {} : {e}", "Frame {}: {e}", j + 1)),
        };
        prev = pin(prev);
        peak = peak.max(prev.opacity);
        guess = carry(&guess, trail[trail.len() - 1], centre(&prev));
        trail.push(centre(&prev));
        if trail.len() > 6 {
            trail.remove(0);
        }
        emit(j, prev.clone());
        (a, i) = (b, j);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn refine(
    load: &dyn Fn(usize) -> Option<Vec<Gray>>,
    ref_pts: &[P],
    ref_frame: usize,
    frames: Vec<(usize, FrameData)>,
    p: &Params,
    stop: &AtomicBool,
    emit: &mut dyn FnMut(usize, FrameData),
) -> Result<usize, String> {
    let refp = load(ref_frame).ok_or(t!("lecture de la vidéo impossible", "cannot read the video"))?;
    let mut done = 0;
    let centre = refs_centre(&[], ref_pts);
    for (i, mut fd) in frames {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let kept = apply(&fd.h, centre);
        if i == ref_frame {
            continue;
        }
        let b = load(i).ok_or(t!("lecture de la vidéo impossible", "cannot read the video"))?;
        let refs: Vec<P> = if p.warp { ref_pts.to_vec() } else { fd.pts.iter().map(|q| apply(&inv(&fd.h), *q)).collect() };
        let mut changed = false;
        for _ in 0..2 {
            let Some(better) = realign(&refp, &b, ref_pts, &refs, &fd, p, p.search) else { break };
            (fd, changed) = (better, true);
        }
        if changed {
            done += 1;
            emit(i, pinned(fd, centre, kept, p.axis));
        }
    }
    Ok(done)
}

pub fn between(a: &FrameData, b: &FrameData, corners: &[P], t: f32) -> FrameData {
    let (from, to): (Vec<P>, Vec<P>) = corners.iter().map(|v| (apply(&a.h, *v), apply(&b.h, *v))).unzip();
    let m = fit(&SIMILARITY, &from, &to, &vec![true; from.len()]).unwrap_or(IDENT);
    let n = from.len().max(1) as f32;
    let c = from.iter().fold([0f32; 2], |c, q| [c[0] + q[0] / n, c[1] + q[1] / n]);
    let e = apply(&m, c);
    let (k, turn) = (m[0].hypot(m[3]).max(1e-9).powf(t as f64), m[3].atan2(m[0]) * t as f64);
    let lin = [k * turn.cos(), -k * turn.sin(), 0., k * turn.sin(), k * turn.cos(), 0., 0., 0., 1.];
    let at = [c[0] + (e[0] - c[0]) * t, c[1] + (e[1] - c[1]) * t];
    let step = mul(&translation(at[0] as f64, at[1] as f64), &mul(&lin, &translation(-c[0] as f64, -c[1] as f64)));
    let pts = if a.pts.len() == b.pts.len() {
        a.pts.iter().zip(&b.pts).map(|(p, q)| [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]).collect()
    } else {
        a.pts.iter().map(|p| apply(&step, *p)).collect()
    };
    FrameData { h: mul(&step, &a.h), pts, opacity: a.opacity + (b.opacity - a.opacity) * t, guessed: true, score: 0. }
}

fn pinned(f: FrameData, centre: P, kept: P, axis: u8) -> FrameData {
    let at = apply(&f.h, centre);
    let d = match axis {
        1 => [0., kept[1] - at[1]],
        2 => [kept[0] - at[0], 0.],
        _ => return f,
    };
    FrameData { h: mul(&translation(d[0] as f64, d[1] as f64), &f.h), pts: f.pts.iter().map(|q| [q[0] + d[0], q[1] + d[1]]).collect(), ..f }
}

fn carry(g: &H, from: P, to: P) -> H {
    let q = apply(g, from);
    let (x, y) = ((to[0] + q[0] - from[0]) as f64, (to[1] + q[1] - from[1]) as f64);
    [g[0], g[1], x - g[0] * to[0] as f64 - g[1] * to[1] as f64, g[3], g[4], y - g[3] * to[0] as f64 - g[4] * to[1] as f64, 0., 0., 1.]
}

fn refs_centre(zone: &[P], ref_pts: &[P]) -> P {
    let pts = if zone.is_empty() { ref_pts } else { zone };
    let n = pts.len().max(1) as f32;
    pts.iter().fold([0f32; 2], |c, q| [c[0] + q[0] / n, c[1] + q[1] / n])
}

pub fn in_poly(poly: &[P], p: P) -> bool {
    let mut inside = false;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + poly.len() - 1) % poly.len()]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
            inside = !inside;
        }
    }
    inside
}

fn bbox(poly: &[P]) -> [f32; 4] {
    poly.iter().fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |b, p| [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])])
}

pub fn detect(g: &Gray, poly: &[P]) -> Vec<P> {
    let b = bbox(poly);
    let (x0, y0) = ((b[0] as i32).max(3), (b[1] as i32).max(3));
    let (x1, y1) = ((b[2] as i32).min(g.w as i32 - 4), (b[3] as i32).min(g.h as i32 - 4));
    if x1 <= x0 || y1 <= y0 {
        return vec![];
    }
    let area = ((x1 - x0) * (y1 - y0)) as f32;
    let cell = (area / 150.).sqrt().clamp(6., 48.);
    let stride = if area > 400_000. { 2 } else { 1 };
    let cols = ((x1 - x0) as f32 / cell) as usize + 1;
    let rows = ((y1 - y0) as f32 / cell) as usize + 1;
    let mut best = vec![(0f32, [0f32; 2]); cols * rows];
    for y in (y0..=y1).step_by(stride) {
        for x in (x0..=x1).step_by(stride) {
            if !in_poly(poly, [x as f32, y as f32]) {
                continue;
            }
            let (mut gxx, mut gxy, mut gyy) = (0f32, 0f32, 0f32);
            for v in y - 2..=y + 2 {
                for u in x - 2..=x + 2 {
                    let (ix, iy) = (g.px(u + 1, v) - g.px(u - 1, v), g.px(u, v + 1) - g.px(u, v - 1));
                    gxx += ix * ix;
                    gxy += ix * iy;
                    gyy += iy * iy;
                }
            }
            let r = gxx + gyy - ((gxx - gyy).powi(2) + 4. * gxy * gxy).sqrt();
            let c = &mut best[((y - y0) as f32 / cell) as usize * cols + ((x - x0) as f32 / cell) as usize];
            if r > c.0 {
                *c = (r, [x as f32, y as f32]);
            }
        }
    }
    let top = best.iter().fold(0f32, |m, c| m.max(c.0));
    let strong: Vec<P> = best.iter().filter(|c| c.0 > 0.03 * top).map(|c| c.1).collect();
    if strong.len() >= 8 { strong } else { best.iter().filter(|c| c.0 > 0.).map(|c| c.1).collect() }
}

pub fn edge_blur(g: &Gray, poly: &[P]) -> Option<f32> {
    let b = bbox(poly);
    let (x0, y0, x1, y1) = ((b[0] - 6.).max(8.), (b[1] - 6.).max(8.), (b[2] + 6.).min(g.w as f32 - 9.), (b[3] + 6.).min(g.h as f32 - 9.));
    let stride = (((x1 - x0) * (y1 - y0) / 60000.).sqrt().ceil() as usize).max(1);
    let mut found: Vec<(f32, f32)> = vec![];
    for y in (y0 as i32..=y1 as i32).step_by(stride) {
        for x in (x0 as i32..=x1 as i32).step_by(stride) {
            let (gx, gy) = ((g.px(x + 1, y) - g.px(x - 1, y)) / 2., (g.px(x, y + 1) - g.px(x, y - 1)) / 2.);
            let m = gx.hypot(gy);
            if m < 4. {
                continue;
            }
            let (dx, dy, x, y) = (gx / m, gy / m, x as f32, y as f32);
            let slope = |t: f32| (g.at(x + (t + 0.5) * dx, y + (t + 0.5) * dy) - g.at(x + (t - 0.5) * dx, y + (t - 0.5) * dy)).abs();
            let peak = slope(0.);
            if peak < slope(1.) || peak < slope(-1.) {
                continue;
            }
            let step = (g.at(x + 7. * dx, y + 7. * dy) - g.at(x - 7. * dx, y - 7. * dy)).abs();
            found.push((peak, step / (peak * (2. * std::f32::consts::PI).sqrt())));
        }
    }
    found.sort_by(|a, b| b.0.total_cmp(&a.0));
    found.truncate((found.len() / 5).max(20));
    let mut wide: Vec<f32> = found.iter().map(|f| f.1).collect();
    wide.sort_by(f32::total_cmp);
    (wide.len() >= 20).then(|| (wide[wide.len() / 2].powi(2) - 0.62).max(0.).sqrt())
}

pub fn contrast(rgb: &[u8], w: usize, h: usize, shape: &[P], margin: f32) -> [f32; 3] {
    let b = bbox(shape);
    let (x0, y0) = (((b[0] - margin) as usize).max(1), ((b[1] - margin) as usize).max(1));
    let (x1, y1) = (((b[2] + margin) as usize).min(w.saturating_sub(2)), ((b[3] + margin) as usize).min(h.saturating_sub(2)));
    let mut m = [[0f64; 3]; 3];
    for y in y0..=y1 {
        for x in x0..=x1 {
            let i = (y * w + x) * 3;
            for (a, b) in [(i + 3, i - 3), (i + 3 * w, i - 3 * w)] {
                let d: [f64; 3] = std::array::from_fn(|c| rgb[a + c] as f64 - rgb[b + c] as f64);
                for r in 0..3 {
                    for c in 0..3 {
                        m[r][c] += d[r] * d[c];
                    }
                }
            }
        }
    }
    let mut v = [0.299f64, 0.587, 0.114];
    for _ in 0..50 {
        let n: [f64; 3] = std::array::from_fn(|r| (0..3).map(|c| m[r][c] * v[c]).sum());
        let len = n.iter().map(|x| x * x).sum::<f64>().sqrt();
        if len < 1e-9 {
            break;
        }
        v = n.map(|x| x / len);
    }
    let sum = v.iter().map(|x| x.abs()).sum::<f64>() * if v.iter().sum::<f64>() < 0. { -1. } else { 1. };
    v.map(|x| (x / sum) as f32)
}

pub fn mesh_nodes(poly: &[P], size: f32) -> Vec<P> {
    let b = bbox(poly);
    let mut v = poly.to_vec();
    let mut y = b[1] + size / 2.;
    while y < b[3] {
        let mut x = b[0] + size / 2.;
        while x < b[2] {
            if in_poly(poly, [x, y]) {
                v.push([x, y]);
            }
            x += size;
        }
        y += size;
    }
    v
}

pub fn opacity_samples(g: &Gray, shape: &[P], pattern: i32) -> Vec<(P, f32)> {
    let boxes: Vec<[f32; 4]> = if shape.len() >= 3 {
        vec![bbox(shape)]
    } else {
        shape.iter().map(|p| [p[0] - pattern as f32, p[1] - pattern as f32, p[0] + pattern as f32, p[1] + pattern as f32]).collect()
    };
    let mut v = vec![];
    for b in &boxes {
        let step = (((b[2] - b[0]) * (b[3] - b[1]) / 1500.).sqrt()).max(1.);
        let mut y = b[1];
        while y <= b[3] {
            let mut x = b[0];
            while x <= b[2] {
                if shape.len() < 3 || in_poly(shape, [x, y]) {
                    v.push(([x, y], g.at(x, y)));
                }
                x += step;
            }
            y += step;
        }
    }
    v
}

pub fn parts(h: &H, centre: P) -> [f64; 4] {
    let (p, j) = (apply(h, centre), jacobian(h, centre));
    [p[0] as f64, p[1] as f64, ((j[0] * j[3] - j[1] * j[2]).abs().max(1e-12) as f64).sqrt(), ((j[2] - j[1]) as f64).atan2((j[0] + j[3]) as f64)]
}

pub fn retouch(f: &FrameData, centre: P, to: [f64; 4]) -> FrameData {
    let was = parts(&f.h, centre);
    let (k, turn) = (to[2] / was[2], to[3] - was[3]);
    let lin = [k * turn.cos(), -k * turn.sin(), 0., k * turn.sin(), k * turn.cos(), 0., 0., 0., 1.];
    let step = mul(&translation(to[0], to[1]), &mul(&lin, &translation(-was[0], -was[1])));
    FrameData { h: mul(&step, &f.h), pts: f.pts.iter().map(|q| apply(&step, *q)).collect(), ..f.clone() }
}

pub fn ease(frames: &[Option<FrameData>], centre: P, sigma: [f32; 3]) -> Vec<Option<FrameData>> {
    let mut out = frames.to_vec();
    let mut s = 0;
    while s < frames.len() && sigma.iter().any(|v| *v > 0.) {
        if frames[s].is_none() {
            s += 1;
            continue;
        }
        let e = (s..frames.len()).find(|&i| frames[i].is_none()).unwrap_or(frames.len());
        let mut rows: Vec<[f64; 4]> = frames[s..e].iter().map(|f| parts(&f.as_ref().unwrap().h, centre)).collect();
        for i in 1..rows.len() {
            rows[i][3] += std::f64::consts::TAU * ((rows[i - 1][3] - rows[i][3]) / std::f64::consts::TAU).round();
            rows[i][2] = rows[i][2].ln();
        }
        rows[0][2] = rows[0][2].ln();
        let last = rows.len() as i64 - 1;
        let soft = |c: usize, sg: f32| -> Vec<f64> {
            let r = (3. * sg).ceil() as i64;
            let at = |i: i64| if i < 0 { 2. * rows[0][c] - rows[(-i).min(last) as usize][c] } else if i > last { 2. * rows[last as usize][c] - rows[(2 * last - i).max(0) as usize][c] } else { rows[i as usize][c] };
            (0..=last).map(|i| if sg <= 0. { rows[i as usize][c] } else { (-r..=r).map(|d| (-(d * d) as f64 / (2. * (sg * sg) as f64)).exp() * at(i + d)).sum::<f64>() / (-r..=r).map(|d| (-(d * d) as f64 / (2. * (sg * sg) as f64)).exp()).sum::<f64>() }).collect()
        };
        let (x, y, k, t) = (soft(0, sigma[0]), soft(1, sigma[0]), soft(2, sigma[1]), soft(3, sigma[2]));
        for i in 0..rows.len() {
            out[s + i] = Some(retouch(frames[s + i].as_ref().unwrap(), centre, [x[i], y[i], k[i].exp(), t[i]]));
        }
        s = e;
    }
    out
}

pub fn smooth(frames: &[Option<FrameData>], sigma: f32) -> Vec<Option<FrameData>> {
    let mut out = frames.to_vec();
    if sigma <= 0. {
        return out;
    }
    let r = (3. * sigma).ceil() as i64;
    let k: Vec<f64> = (-r..=r).map(|i| (-(i * i) as f64 / (2. * (sigma * sigma) as f64)).exp()).collect();
    let ksum: f64 = k.iter().sum();
    let flat = |f: &FrameData| -> Vec<f64> {
        f.h.iter().copied().chain(f.pts.iter().flat_map(|p| [p[0] as f64, p[1] as f64])).chain([f.opacity as f64]).collect()
    };
    let mut s = 0;
    while s < frames.len() {
        if frames[s].is_none() {
            s += 1;
            continue;
        }
        let e = (s..frames.len()).find(|&i| frames[i].is_none()).unwrap_or(frames.len());
        let rows: Vec<Vec<f64>> = frames[s..e].iter().map(|f| flat(f.as_ref().unwrap())).collect();
        let last = rows.len() as i64 - 1;
        let val = |i: i64, c: usize| -> f64 {
            if i < 0 {
                2. * rows[0][c] - rows[(-i).min(last) as usize][c]
            } else if i > last {
                2. * rows[last as usize][c] - rows[(2 * last - i).max(0) as usize][c]
            } else {
                rows[i as usize][c]
            }
        };
        for i in 0..rows.len() {
            let v: Vec<f64> = (0..rows[i].len()).map(|c| (-r..=r).map(|d| k[(d + r) as usize] * val(i as i64 + d, c)).sum::<f64>() / ksum).collect();
            let f = out[s + i].as_mut().unwrap();
            f.h = std::array::from_fn(|j| v[j]);
            for (j, p) in f.pts.iter_mut().enumerate() {
                *p = [v[9 + 2 * j] as f32, v[10 + 2 * j] as f32];
            }
            f.opacity = v[v.len() - 1] as f32;
        }
        s = e;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lattice(i: i32, j: i32, seed: u32) -> f32 {
        let mut h = (i as u32).wrapping_mul(0x9E37_79B1) ^ (j as u32).wrapping_mul(0x85EB_CA6B) ^ seed;
        h = (h ^ (h >> 15)).wrapping_mul(0x2C1B_3C6D);
        ((h ^ (h >> 13)) & 0xFFFF) as f32 / 65535.
    }

    fn noise(p: P, cell: f32, seed: u32) -> f32 {
        let (x, y) = (p[0] / cell, p[1] / cell);
        let (i, j) = (x.floor() as i32, y.floor() as i32);
        let (fx, fy) = (x - i as f32, y - j as f32);
        let (sx, sy) = (fx * fx * (3. - 2. * fx), fy * fy * (3. - 2. * fy));
        let row = |j| lattice(i, j, seed) * (1. - sx) + lattice(i + 1, j, seed) * sx;
        row(j) * (1. - sy) + row(j + 1) * sy
    }

    fn tex(p: P) -> f32 {
        30. + 70. * noise(p, 7., 1) + 70. * noise(p, 19., 2) + 60. * noise(p, 43., 3)
    }

    fn drawing(p: P) -> f32 {
        130. + 100. * ((noise(p, 23., 4) - 0.5) * 12.).clamp(-0.5, 0.5) + 60. * ((noise(p, 9., 5) - 0.5) * 12.).clamp(-0.5, 0.5)
    }

    fn frame(tex: fn(P) -> f32, h: &H, alpha: f32) -> Gray {
        let (w, hh) = (320, 240);
        let hi = inv(h);
        let px = |i: usize| (0..9).map(|k| tex(apply(&hi, [(i % w) as f32 + (k % 3) as f32 / 3. - 1. / 3., (i / w) as f32 + (k / 3) as f32 / 3. - 1. / 3.]))).sum::<f32>() / 9.;
        let d = (0..w * hh).map(|i| alpha * px(i) + (1. - alpha) * 90.).collect();
        Gray { w, h: hh, d }
    }

    fn params(motion: Motion, user_pts: bool) -> Params {
        Params { motion, search: 48., pattern: 10, channel: Channel::Luma, user_pts, warp: user_pts, hold_fades: false, realign: false, hole: vec![], axis: 0 }
    }

    const ALL: Motion = Motion { translation: true, scale: true, rotation: true, shear: true, perspective: true };

    fn track(tex: fn(P) -> f32, hs: &[H], alphas: &[f32], ref_pts: Vec<P>, shape: &[P], p: &Params) -> Vec<FrameData> {
        let load = |i: usize| Some(pyramid(frame(tex, &hs[i], alphas[i]), p.search, p.pattern));
        let opa = opacity_samples(&frame(tex, &hs[0], alphas[0]), shape, p.pattern);
        let start = FrameData { h: IDENT, pts: ref_pts.clone(), opacity: 1., guessed: false, score: 1. };
        let mut out = vec![start.clone()];
        let zone = if p.warp { &[][..] } else { shape };
        run(&load, &ref_pts, zone, &opa, Some(0), 0, start, hs.len() - 1, p, &AtomicBool::new(false), &mut |_, f| out.push(f)).unwrap();
        out
    }

    #[test]
    fn dense_alignment_finds_the_pose() {
        let shape = [[90., 60.], [230., 60.], [230., 180.], [90., 180.]];
        let lin = [1.04 * 0.03f64.cos(), -1.04 * 0.03f64.sin(), 0., 1.04 * 0.03f64.sin(), 1.04 * 0.03f64.cos(), 0., 0.00005, 0.00002, 1.];
        let truth = mul(&translation(163., 118.), &mul(&lin, &translation(-160., -120.)));
        for texture in [tex, drawing] {
            let refp = pyramid(frame(texture, &IDENT, 1.), 48., 10);
            let cur = pyramid(frame(texture, &truth, 1.), 48., 10);
            let t = Template::new(&refp, &shape, 10., &[]);
            for (dx, top) in [(0., 9), (3., 9), (1., 1)] {
                let Match { h, score, .. } = t.align(&cur, &mul(&translation(dx, 0.), &truth), &ALL, top);
                let worst = shape.iter().map(|v| dist(apply(&h, *v), apply(&truth, *v))).fold(0f32, f32::max);
                assert!(worst < 0.1 && score > 0.95, "départ à {dx} px : erreur {worst} px, score {score}");
            }
        }
    }

    #[test]
    fn a_locked_axis_does_not_move() {
        let hs: Vec<H> = (0..20).map(|i| translation(3. * i as f64, [0., 1.5, -1.][i % 3])).collect();
        let shape = [[60., 80.], [120., 80.], [120., 140.], [60., 140.]];
        let ref_pts = detect(&frame(tex, &hs[0], 1.), &shape);
        for (axis, realign) in [(1, true), (1, false), (2, true)] {
            let p = Params { axis, realign, motion: SIMILARITY, ..params(ALL, false) };
            let out = track(tex, &hs, &[1.; 20], ref_pts.clone(), &shape, &p);
            for (i, f) in out.iter().enumerate() {
                let c = apply(&f.h, [90., 110.]);
                if axis == 1 {
                    assert!((c[1] - 110.).abs() < 1e-3 && (c[0] - 90. - 3. * i as f32).abs() < 1., "frame {i} : {c:?}");
                } else {
                    assert!((c[0] - 90.).abs() < 1e-3, "frame {i} : {c:?}");
                }
            }
        }
        let free = track(tex, &hs, &[1.; 20], ref_pts, &shape, &Params { realign: true, motion: SIMILARITY, ..params(ALL, false) });
        assert!((apply(&free[1].h, [90., 110.])[1] - 111.5).abs() < 0.3, "sans verrou, le mouvement vertical est suivi");
    }

    #[test]
    fn a_hidden_zone_is_held_then_found_again() {
        let hs: Vec<H> = (0..30).map(|i| translation(4. * i as f64 + if i > 16 { 30. } else { 0. }, i as f64 - if i > 16 { 20. } else { 0. })).collect();
        let alphas: Vec<f32> = (0..30).map(|i| if (8..=16).contains(&i) { 0. } else { 1. }).collect();
        let shape = [[60., 80.], [120., 80.], [120., 140.], [60., 140.]];
        let ref_pts = detect(&frame(drawing, &hs[0], 1.), &shape);
        let p = Params { realign: true, motion: SIMILARITY, ..params(ALL, false) };
        let out = track(drawing, &hs, &alphas, ref_pts, &shape, &p);
        let off = |i: usize| shape.iter().map(|v| dist(apply(&out[i].h, *v), apply(&hs[i], *v))).fold(0f32, f32::max);
        assert!(out[7].guessed == false && off(7) < 0.3, "avant : {} px", off(7));
        assert!((8..=16).all(|i| out[i].guessed), "les frames cachées doivent être marquées estimées");
        assert!(off(12) < 3., "trajectoire prolongée : {} px", off(12));
        assert!(!out[29].guessed && off(29) < 0.5, "après : {} px", off(29));
    }

    #[test]
    fn interpolation_turns_and_grows_evenly_between_two_poses() {
        let shape = [[40., 40.], [80., 40.], [80., 70.], [40., 70.]];
        let pose = |t: f64| {
            let (k, r) = (2f64.powf(t), 1.2 * t);
            let lin = [k * r.cos(), -k * r.sin(), 0., k * r.sin(), k * r.cos(), 0., 0., 0., 1.];
            mul(&translation(60. + 100. * t, 55. - 30. * t), &mul(&lin, &translation(-60., -55.)))
        };
        let key = |t: f64| FrameData { h: pose(t), pts: vec![], opacity: 1. - t as f32, guessed: false, score: 1. };
        let mid = between(&key(0.), &key(1.), &shape, 0.25);
        let off = shape.iter().map(|v| dist(apply(&mid.h, *v), apply(&pose(0.25), *v))).fold(0f32, f32::max);
        assert!(off < 0.01 && mid.guessed && (mid.opacity - 0.75).abs() < 1e-6, "écart {off} px");
    }

    #[test]
    fn ignored_area_takes_no_part_in_the_template() {
        let shape = [[90., 60.], [230., 60.], [230., 180.], [90., 180.]];
        let hole = [[150., 60.], [230., 60.], [230., 180.], [150., 180.]];
        let truth = translation(3., -2.);
        let refp = pyramid(frame(drawing, &IDENT, 1.), 48., 10);
        let mut spoiled = frame(drawing, &truth, 1.);
        for y in 0..spoiled.h {
            for x in 150..spoiled.w {
                spoiled.d[y * spoiled.w + x] = tex([x as f32 * 3., y as f32]);
            }
        }
        let cur = pyramid(spoiled, 48., 10);
        let start = translation(2., -1.);
        let blind = Template::new(&refp, &shape, 10., &[]).align(&cur, &start, &SIMILARITY, 9);
        let aware = Template::new(&refp, &shape, 10., &hole).align(&cur, &start, &SIMILARITY, 9);
        let off = |m: &Match| dist(apply(&m.h, shape[0]), apply(&truth, shape[0]));
        assert!(off(&aware) < 0.2 && aware.score > blind.score, "avec zone ignorée : {} px, score {} contre {}", off(&aware), aware.score, blind.score);
    }

    #[test]
    fn edge_blur_is_measured_on_a_soft_and_on_a_sharp_edge() {
        let shape = [[100., 60.], [220., 60.], [220., 180.], [100., 180.]];
        for (sigma, low, high) in [(0.01f32, 0., 0.5), (2.5, 2., 3.)] {
            let soft = |x: f32| 40. + 160. / (1. + (-(x - 160.) / (0.5513 * sigma)).exp());
            let g = Gray { w: 320, h: 240, d: (0..320 * 240).map(|i| soft((i % 320) as f32)).collect() };
            let got = edge_blur(&g, &shape).unwrap();
            assert!((low..=high).contains(&got), "flou {sigma} mesuré {got}");
        }
    }

    #[test]
    fn auto_channel_separates_colours_of_equal_brightness() {
        let (a, b) = ([200u8, 60, 0], [0u8, 162, 0]);
        let rgb: Vec<u8> = (0..40 * 40).flat_map(|i| if i % 40 < 20 { a } else { b }).collect();
        let k = contrast(&rgb, 40, 40, &[[5., 5.], [35., 5.], [35., 35.], [5., 35.]], 2.);
        let seen = |c: [u8; 3]| (0..3).map(|i| k[i] * c[i] as f32).sum::<f32>();
        let luma = |c: [u8; 3]| 0.299 * c[0] as f32 + 0.587 * c[1] as f32 + 0.114 * c[2] as f32;
        assert!((luma(a) - luma(b)).abs() < 2. && (seen(a) - seen(b)).abs() > 60., "mélange {k:?}");
    }

    #[test]
    fn zone_follows_perspective() {
        let hs: Vec<H> = (0..12)
            .map(|i| {
                let t = i as f64;
                let (c, s) = ((0.02 * t).cos() * (1. + 0.01 * t), (0.02 * t).sin() * (1. + 0.01 * t));
                let lin = [c, -s, 0., s, c, 0., 0.00004 * t, 0.00002 * t, 1.];
                mul(&translation(160. + 3. * t, 120. - 2. * t), &mul(&lin, &translation(-160., -120.)))
            })
            .collect();
        let shape = [[90., 60.], [230., 60.], [230., 180.], [90., 180.]];
        let ref_pts = detect(&frame(tex, &hs[0], 1.), &shape);
        assert!(ref_pts.len() >= 30, "{} points détectés", ref_pts.len());
        let out = track(tex, &hs, &[1.; 12], ref_pts.clone(), &shape, &params(ALL, false));
        for v in shape {
            let (got, want) = (out[11].map(&ref_pts, false, v), apply(&hs[11], v));
            assert!(dist(got, want) < 0.5, "{got:?} au lieu de {want:?}");
            assert_eq!(out[11].unmap(&ref_pts, false, got).map(f32::round), v);
        }
    }

    const POP: [f64; 6] = [1., 2., 3., 4.2, 3.5, 3.];

    fn zoom(k: f64) -> H {
        mul(&translation(160., 120.), &mul(&[k, 0., 0., 0., k, 0., 0., 0., 1.], &translation(-160., -120.)))
    }

    #[test]
    fn zone_follows_a_pop_in_zoom() {
        let hs: Vec<H> = POP.map(zoom).to_vec();
        let shape = [[130., 100.], [190., 100.], [190., 140.], [130., 140.]];
        let ref_pts = detect(&frame(drawing, &hs[0], 1.), &shape);
        let out = track(drawing, &hs, &[1.; 6], ref_pts.clone(), &shape, &params(ALL, false));
        for ((f, h), k) in out.iter().zip(&hs).zip(POP) {
            for v in shape {
                let (got, want) = (f.map(&ref_pts, false, v), apply(h, v));
                assert!(dist(got, want) < 0.75 * k as f32, "zoom {k} : {got:?} au lieu de {want:?}");
            }
        }
    }

    #[test]
    fn two_points_follow_a_pop_in_zoom() {
        let hs: Vec<H> = POP.map(zoom).to_vec();
        let corners = detect(&frame(drawing, &hs[0], 1.), &[[130., 100.], [190., 100.], [190., 140.], [130., 140.]]);
        let pts = vec![corners[0], corners[corners.len() - 1]];
        let out = track(drawing, &hs, &[1.; 6], pts.clone(), &pts, &params(ALL, true));
        for (q, want) in pts.iter().map(|q| (out[5].map(&pts, true, *q), apply(&hs[5], *q))) {
            assert!(dist(q, want) < 1.5, "{q:?} au lieu de {want:?}");
        }
    }

    #[test]
    fn single_point_follows_large_translation() {
        let hs: Vec<H> = (0..8).map(|i| translation(17. * i as f64, -9. * i as f64)).collect();
        let pt = [100., 150.];
        let only_t = Motion { scale: false, rotation: false, shear: false, perspective: false, ..ALL };
        let out = track(tex, &hs, &[1.; 8], vec![pt], &[pt], &params(only_t, true));
        let got = out[7].map(&[pt], true, pt);
        assert!(dist(got, [219., 87.]) < 0.3, "{got:?}");
    }

    #[test]
    fn disabled_components_stay_fixed() {
        let hs: Vec<H> = (0..6).map(|i| mul(&translation(160., 120.), &mul(&[1. + 0.02 * i as f64, 0., 0., 0., 1. + 0.02 * i as f64, 0., 0., 0., 1.], &translation(-160., -120.)))).collect();
        let shape = [[100., 70.], [220., 70.], [220., 170.], [100., 170.]];
        let ref_pts = detect(&frame(tex, &hs[0], 1.), &shape);
        let no_scale = Motion { scale: false, shear: false, perspective: false, ..ALL };
        let out = track(tex, &hs, &[1.; 6], ref_pts.clone(), &shape, &params(no_scale, false));
        let (a, b) = (out[5].map(&ref_pts, false, shape[0]), out[5].map(&ref_pts, false, shape[1]));
        assert!((dist(a, b) - 120.).abs() < 0.2, "largeur {}", dist(a, b));
    }

    #[test]
    fn mesh_follows_a_surface_that_bends() {
        let bend = |p: P, t: f32| [p[0], p[1] - t * ((p[0] - 160.) / 100.).clamp(0., 1.)];
        let frame = |t: f32| Gray { w: 320, h: 240, d: (0..320 * 240).map(|i| tex(bend([(i % 320) as f32, (i / 320) as f32], t))).collect() };
        let shape = [[60., 60.], [260., 60.], [260., 180.], [60., 180.]];
        let nodes = mesh_nodes(&shape, 25.);
        let p = Params { warp: true, ..params(ALL, false) };
        let load = |i: usize| Some(pyramid(frame(i as f32), p.search, p.pattern));
        let start = FrameData { h: IDENT, pts: nodes.clone(), opacity: 1., guessed: false, score: 1. };
        let mut last = start.clone();
        run(&load, &nodes, &[], &[], None, 0, start, 8, &p, &AtomicBool::new(false), &mut |_, f| last = f).unwrap();
        for (v, dy) in [(shape[0], 0.), (shape[3], 0.), (shape[1], 8.), (shape[2], 8.)] {
            let got = last.map(&nodes, true, v);
            assert!(dist(got, [v[0], v[1] + dy]) < 1., "{v:?} → {got:?}");
        }
    }

    #[test]
    fn opacity_follows_a_fade() {
        let alphas = [1., 0.85, 0.7, 0.55, 0.4];
        let shape = [[90., 60.], [230., 60.], [230., 180.], [90., 180.]];
        let ref_pts = detect(&frame(tex, &IDENT, 1.), &shape);
        let out = track(tex, &[IDENT; 5], &alphas, ref_pts, &shape, &params(ALL, false));
        for (f, a) in out.iter().zip(alphas) {
            assert!((f.opacity - a).abs() < 0.03, "{} au lieu de {a}", f.opacity);
        }
    }

    #[test]
    fn a_full_fade_out_keeps_moving_and_is_measured() {
        let alphas = [1., 0.8, 0.6, 0.4, 0.25, 0.1, 0.03, 0.];
        let hs: Vec<H> = (0..8).map(|i| translation(3. * i as f64, 0.)).collect();
        let shape = [[90., 60.], [200., 60.], [200., 180.], [90., 180.]];
        let ref_pts = detect(&frame(tex, &IDENT, 1.), &shape);
        let hold = Params { hold_fades: true, ..params(ALL, false) };
        let out = track(tex, &hs, &alphas, ref_pts.clone(), &shape, &hold);
        assert_eq!(out.len(), 8);
        for ((f, a), h) in out.iter().zip(alphas).zip(&hs) {
            assert!((f.opacity - a).abs() < 0.04, "{} au lieu de {a}", f.opacity);
            let (got, want) = (f.map(&ref_pts, false, shape[0]), apply(h, shape[0]));
            assert!(dist(got, want) < 1., "{got:?} au lieu de {want:?}");
        }
        let load = |i: usize| Some(pyramid(frame(tex, &hs[i], alphas[i]), 48., 10));
        let start = FrameData { h: IDENT, pts: ref_pts.clone(), opacity: 1., guessed: false, score: 1. };
        assert!(run(&load, &ref_pts, &shape, &[], None, 0, start, 7, &params(ALL, false), &AtomicBool::new(false), &mut |_, _| {}).is_err());
    }

    #[test]
    fn zone_keeps_going_while_it_leaves_the_frame() {
        let hs: Vec<H> = (0..11).map(|i| translation(25. * i as f64, 0.)).collect();
        let shape = [[90., 60.], [230., 60.], [230., 180.], [90., 180.]];
        let ref_pts = detect(&frame(tex, &IDENT, 1.), &shape);
        let only_t = Motion { scale: false, rotation: false, shear: false, perspective: false, ..ALL };
        let out = track(tex, &hs, &[1.; 11], ref_pts.clone(), &shape, &params(only_t, false));
        assert_eq!(out.len(), 11);
        for (i, (f, h)) in out.iter().zip(&hs).enumerate() {
            let (got, want) = (f.map(&ref_pts, false, shape[0]), apply(h, shape[0]));
            assert!(dist(got, want) < if i <= 7 { 0.5 } else { 3. }, "frame {i} : {got:?} au lieu de {want:?}");
        }
    }

    #[test]
    fn zone_takes_new_points_when_the_first_ones_are_gone() {
        let hs: Vec<H> = (0..8).map(|i| { let k = 1.3f64.powi(i); mul(&translation(40., 40.), &mul(&[k, 0., 0., 0., k, 0., 0., 0., 1.], &translation(-40., -40.))) }).collect();
        let shape = [[40., 40.], [200., 40.], [200., 160.], [40., 160.]];
        let ref_pts = detect(&frame(drawing, &IDENT, 1.), &shape);
        let out = track(drawing, &hs, &[1.; 8], ref_pts.clone(), &shape, &params(Motion { shear: false, perspective: false, ..ALL }, false));
        assert_eq!(out.len(), 8);
        let (got, want) = (out[7].map(&ref_pts, false, [100., 90.]), apply(&hs[7], [100., 90.]));
        assert!(dist(got, want) < 4., "{got:?} au lieu de {want:?}");
    }

    fn wander(i: usize) -> H {
        let t = i as f64;
        let (c, s) = ((0.006 * t).cos() * (1. + 0.004 * t), (0.006 * t).sin() * (1. + 0.004 * t));
        let lin = [c, -s, 0., s, c, 0., 0.000012 * t, 0.000006 * t, 1.];
        mul(&translation(160. + 1.2 * t, 120. + 8. * (0.2 * t).sin()), &mul(&lin, &translation(-160., -120.)))
    }

    fn worst(out: &[FrameData], hs: &[H], ref_pts: &[P], shape: &[P]) -> f32 {
        out.iter().zip(hs).flat_map(|(f, h)| shape.iter().map(move |v| dist(f.map(ref_pts, false, *v), apply(h, *v)))).fold(0., f32::max)
    }

    #[test]
    fn realigning_on_the_reference_stops_the_drift() {
        let hs: Vec<H> = (0..50).map(wander).collect();
        let shape = [[100., 70.], [220., 70.], [220., 170.], [100., 170.]];
        let ref_pts = detect(&frame(tex, &hs[0], 1.), &shape);
        let chained = track(tex, &hs, &[1.; 50], ref_pts.clone(), &shape, &params(ALL, false));
        let anchored = track(tex, &hs, &[1.; 50], ref_pts.clone(), &shape, &Params { realign: true, ..params(ALL, false) });
        let (drift, held) = (worst(&chained, &hs, &ref_pts, &shape), worst(&anchored, &hs, &ref_pts, &shape));
        assert!(held < 0.25 && held < drift, "recalé {held}, sans recalage {drift}");
    }

    #[test]
    fn refining_repairs_a_track_that_drifted() {
        let hs: Vec<H> = (0..20).map(wander).collect();
        let shape = [[100., 70.], [220., 70.], [220., 170.], [100., 170.]];
        let ref_pts = detect(&frame(tex, &hs[0], 1.), &shape);
        let p = Params { realign: true, ..params(ALL, false) };
        let pushed: Vec<(usize, FrameData)> = hs
            .iter()
            .enumerate()
            .map(|(i, h)| {
                let off = mul(&translation(2.5, -1.5), h);
                (i, FrameData { h: off, pts: ref_pts.iter().map(|q| apply(&off, *q)).collect(), opacity: 1., guessed: false, score: 1. })
            })
            .collect();
        let load = |i: usize| Some(pyramid(frame(tex, &hs[i], 1.), p.search, p.pattern));
        let mut fixed = vec![];
        let n = refine(&load, &ref_pts, 0, pushed, &p, &AtomicBool::new(false), &mut |i, f| fixed.push((i, f))).unwrap();
        assert_eq!(n, 19);
        for (i, f) in &fixed {
            let (got, want) = (f.map(&ref_pts, false, shape[2]), apply(&hs[*i], shape[2]));
            assert!(dist(got, want) < 0.25, "frame {i} : {got:?} au lieu de {want:?}");
        }
    }

    #[test]
    fn one_part_of_the_motion_can_be_smoothed_or_retouched_alone() {
        let c = [50., 50.];
        let pose = |i: usize| {
            let (turn, jolt) = (0.01 * i as f64, if i % 2 == 0 { 3. } else { -3. });
            let lin = [turn.cos(), -turn.sin(), 0., turn.sin(), turn.cos(), 0., 0., 0., 1.];
            Some(FrameData { h: mul(&translation(50. + 2. * i as f64 + jolt, 50.), &mul(&lin, &translation(-50., -50.))), pts: vec![], opacity: 1., guessed: false, score: 1. })
        };
        let frames: Vec<Option<FrameData>> = (0..40).map(pose).collect();
        let eased = ease(&frames, c, [3., 0., 0.]);
        let (raw, got) = (parts(&frames[20].as_ref().unwrap().h, c), parts(&eased[20].as_ref().unwrap().h, c));
        assert!((got[0] - 90.).abs() < 0.4 && (raw[0] - 93.).abs() < 1e-3, "x lissé {} (brut {})", got[0], raw[0]);
        assert!((got[3] - raw[3]).abs() < 1e-6 && (got[2] - 1.).abs() < 1e-6, "rotation et taille inchangées");
        let moved = retouch(frames[5].as_ref().unwrap(), c, [10., 20., 2., 0.5]);
        let p = parts(&moved.h, c);
        assert!((p[0] - 10.).abs() < 1e-3 && (p[1] - 20.).abs() < 1e-3 && (p[2] - 2.).abs() < 1e-4 && (p[3] - 0.5).abs() < 1e-4, "{p:?}");
    }

    #[test]
    fn smoothing_keeps_linear_motion_and_damps_jitter() {
        let mk = |x: f32| Some(FrameData { h: translation(x as f64, 0.), pts: vec![[x, 0.]], opacity: 1., guessed: false, score: 1. });
        let line: Vec<_> = (0..20).map(|i| mk(3. * i as f32)).collect();
        for (i, f) in smooth(&line, 2.).iter().enumerate() {
            assert!((f.as_ref().unwrap().pts[0][0] - 3. * i as f32).abs() < 1e-3);
        }
        let jitter: Vec<_> = (0..20).map(|i| mk(if i % 2 == 0 { 1. } else { -1. })).collect();
        assert!(smooth(&jitter, 2.)[10].as_ref().unwrap().pts[0][0].abs() < 0.1);
        let mut gap = line.clone();
        gap[7] = None;
        assert!(smooth(&gap, 2.)[7].is_none());
    }
}
