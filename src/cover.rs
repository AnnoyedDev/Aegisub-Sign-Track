use crate::export::Cover;
use crate::track::{in_poly, P};

pub type Seen = Option<(u64, Vec<bool>)>;
pub type Fit = Option<(Vec<bool>, Cover)>;

pub fn cells(zone: &[P], hole: &[P]) -> Vec<P> {
    let b = zone.iter().fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |b, p| [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])]);
    let step = ((b[2] - b[0]) * (b[3] - b[1]) / 900.).sqrt().max(2.);
    let (nx, ny) = (((b[2] - b[0]) / step).ceil() as usize, ((b[3] - b[1]) / step).ceil() as usize);
    let at = |k: usize| [b[0] + step * ((k % nx) as f32 + (k as f32 * 0.754_877_7).fract()), b[1] + step * ((k / nx) as f32 + (k as f32 * 0.569_840_3).fract())];
    (0..nx * ny).map(at).filter(|p| in_poly(zone, *p) && !(hole.len() >= 3 && in_poly(hole, *p))).collect()
}

fn pixel(rgb: &[u8], w: usize, h: usize, x: i64, y: i64) -> [u8; 3] {
    let i = (y.clamp(0, h as i64 - 1) as usize * w + x.clamp(0, w as i64 - 1) as usize) * 3;
    [rgb[i], rgb[i + 1], rgb[i + 2]]
}

pub fn colours(rgb: &[u8], w: usize, h: usize, at: &[P]) -> Vec<[u8; 3]> {
    at.iter().map(|p| pixel(rgb, w, h, p[0].round() as i64, p[1].round() as i64)).collect()
}

pub fn seen(rgb: &[u8], size: [usize; 2], cells: &[P], colours: &[[u8; 3]], place: &dyn Fn(P) -> P, tolerance: i32) -> Vec<bool> {
    let close = |a: [u8; 3], b: &[u8; 3]| (0..3).all(|k| (a[k] as i32 - b[k] as i32).abs() <= tolerance);
    let near = |q: P, c: &[u8; 3]| (-1..=1).any(|dy| (-1..=1).any(|dx| close(pixel(rgb, size[0], size[1], q[0].round() as i64 + dx, q[1].round() as i64 + dy), c)));
    cells.iter().zip(colours).map(|(p, c)| near(place(*p), c)).collect()
}

fn edge(cells: &[P], vis: &[bool]) -> ([f32; 3], usize) {
    let shown = vis.iter().filter(|v| **v).count();
    let mut best = ([1., 0., f32::MAX], usize::MAX);
    for a in 0..36 {
        let turn = a as f32 * std::f32::consts::PI / 36.;
        let n = [turn.cos(), turn.sin()];
        let mut order: Vec<(f32, bool)> = cells.iter().zip(vis).map(|(p, v)| (n[0] * p[0] + n[1] * p[1], *v)).collect();
        order.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut low = 0;
        for k in 0..=order.len() {
            let d = if k == 0 {
                order.first().map_or(0., |o| o.0 - 1.)
            } else if k == order.len() {
                order[k - 1].0 + 1.
            } else {
                (order[k - 1].0 + order[k].0) / 2.
            };
            let (below, above) = ((k - low) + (shown - low), low + (order.len() - shown) - (k - low));
            if below < best.1 {
                best = ([n[0], n[1], d], below);
            }
            if above < best.1 {
                best = ([-n[0], -n[1], -d], above);
            }
            if order.get(k).is_some_and(|o| o.1) {
                low += 1;
            }
        }
    }
    best
}

pub fn covers(cells: &[P], seen: &[Seen], start: usize, lasting: bool, memo: &mut [Fit]) -> Vec<Cover> {
    let mut out = vec![Cover::Clear; seen.len()];
    let start = start.min(seen.len());
    let passes: [Box<dyn Iterator<Item = usize>>; 2] = [Box::new(start..seen.len()), Box::new((0..start).rev())];
    for pass in passes {
        let mut kept = vec![true; cells.len()];
        for i in pass {
            let Some((_, now)) = &seen[i] else { continue };
            let vis: Vec<bool> = now.iter().zip(&kept).map(|(a, b)| *a && *b).collect();
            if lasting {
                kept.clone_from(&vis);
            }
            let share = vis.iter().filter(|v| **v).count() as f32 / cells.len().max(1) as f32;
            if memo[i].as_ref().map(|m| &m.0) != Some(&vis) {
                let (line, wrong) = edge(cells, &vis);
                let left = vis.iter().filter(|v| **v).count();
                let gone = vis.len() - left;
                let cover = if share < 0.02 || (share < 0.25 && wrong * 3 > left) {
                    Cover::Hidden
                } else if share >= 0.93 || (share >= 0.9 && wrong * 2 > gone) {
                    Cover::Clear
                } else {
                    Cover::Cut(line)
                };
                memo[i] = Some((vis, cover));
            }
            out[i] = memo[i].as_ref().unwrap().1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> Vec<P> {
        cells(&[[100., 100.], [300., 100.], [300., 300.], [100., 300.]], &[[100., 100.], [140., 100.], [140., 140.], [100., 140.]])
    }

    #[test]
    fn a_straight_edge_is_found_through_the_cells() {
        let grid = square();
        assert!(grid.len() > 700 && grid.iter().all(|p| !(p[0] < 140. && p[1] < 140.)), "{} cellules", grid.len());
        let side = |p: &P| 0.8 * p[0] + 0.6 * p[1] <= 290.;
        let vis: Vec<bool> = grid.iter().map(side).collect();
        let (line, wrong) = edge(&grid, &vis);
        assert!(wrong * 50 < grid.len(), "{wrong} cellules mal rangées sur {}", grid.len());
        for p in [[150., 150.], [280., 290.], [210., 180.], [230., 200.]] {
            assert_eq!(line[0] * p[0] + line[1] * p[1] <= line[2], side(&p), "{p:?} avec {line:?}");
        }
    }

    #[test]
    fn what_is_covered_comes_back_or_not() {
        let grid = square();
        let part = |limit: f32| Some((0, grid.iter().map(|p| p[0] <= limit).collect::<Vec<bool>>()));
        let seen = [part(400.), part(400.), part(220.), part(0.), part(220.), part(400.)];
        let sliver = covers(&grid, &[part(400.), part(115.)], 0, true, &mut vec![None; 2]);
        assert!(matches!(sliver[1], Cover::Cut(_)), "un reste étroit mais net reste couvert : {sliver:?}");
        let dust = Some((0, (0..grid.len()).map(|k| k % 7 == 0).collect::<Vec<bool>>()));
        assert_eq!(covers(&grid, &[part(400.), dust], 0, false, &mut vec![None; 2])[1], Cover::Hidden, "des restes éparpillés ne comptent pas");
        let passing = covers(&grid, &seen, 0, false, &mut vec![None; 6]);
        assert!(matches!(passing[..], [Cover::Clear, Cover::Clear, Cover::Cut(_), Cover::Hidden, Cover::Cut(_), Cover::Clear]), "{passing:?}");
        let lasting = covers(&grid, &seen, 0, true, &mut vec![None; 6]);
        assert!(matches!(lasting[..], [Cover::Clear, Cover::Clear, Cover::Cut(_), Cover::Hidden, Cover::Hidden, Cover::Hidden]), "{lasting:?}");
        let back = covers(&grid, &seen, 5, true, &mut vec![None; 6]);
        assert!(matches!(back[..], [Cover::Hidden, Cover::Hidden, Cover::Hidden, Cover::Hidden, Cover::Cut(_), Cover::Clear]), "{back:?}");
        let Cover::Cut(line) = passing[2] else { unreachable!() };
        assert!((line[0] - 1.).abs() < 0.01 && (line[2] - 220.).abs() < 6., "{line:?}");
    }

    #[test]
    fn colours_tell_what_is_still_there() {
        let (w, h) = (40, 20);
        let paint = |split: usize| -> Vec<u8> { (0..w * h).flat_map(|i| if i % w < split { [200, 30, 30] } else { [240, 240, 235] }).collect() };
        let at: Vec<P> = (0..10).map(|k| [2. + 4. * k as f32, 10.]).collect();
        let reference = colours(&paint(40), w, h, &at);
        let now = seen(&paint(21), [w, h], &at, &reference, &|p| [p[0] + 0.4, p[1]], 12);
        assert_eq!(now, [true, true, true, true, true, false, false, false, false, false]);
    }
}
