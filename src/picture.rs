use crate::track::P;
use std::collections::HashMap;
use std::path::Path;

pub struct Picture {
    pub w: usize,
    pub h: usize,
    pub rgba: Vec<u8>,
    pub layers: Vec<([u8; 3], Vec<Vec<P>>)>,
}

pub fn load(path: &Path) -> Result<(Vec<u8>, usize, usize), String> {
    let info = crate::video::probe(path).map_err(|e| e.replace("Vidéo", "Image"))?;
    let k = (200. / info.w.max(info.h) as f32).min(1.);
    let (w, h) = (((info.w as f32 * k).round() as usize).max(1), ((info.h as f32 * k).round() as usize).max(1));
    let out = crate::video::cmd("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-frames:v", "1", "-vf", &format!("scale={w}:{h}"), "-pix_fmt", "rgba", "-f", "rawvideo", "-"])
        .output()
        .map_err(|e| format!("ffmpeg introuvable (installez ffmpeg et ajoutez-le au PATH) : {e}"))?;
    if out.stdout.len() != w * h * 4 {
        return Err(format!("Image illisible : {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok((out.stdout, w, h))
}

fn off(p: P, a: P, b: P) -> f32 {
    let v = [b[0] - a[0], b[1] - a[1]];
    let t = (((p[0] - a[0]) * v[0] + (p[1] - a[1]) * v[1]) / (v[0] * v[0] + v[1] * v[1]).max(1e-9)).clamp(0., 1.);
    (p[0] - a[0] - t * v[0]).hypot(p[1] - a[1] - t * v[1])
}

fn thin(p: &[P], tol: f32, out: &mut Vec<P>) {
    let (a, b) = (p[0], p[p.len() - 1]);
    match (1..p.len() - 1).map(|i| (i, off(p[i], a, b))).max_by(|x, y| x.1.total_cmp(&y.1)) {
        Some((i, d)) if d > tol => {
            thin(&p[..=i], tol, out);
            thin(&p[i..], tol, out);
        }
        _ => out.push(a),
    }
}

fn outline(w: usize, h: usize, inside: &dyn Fn(i32, i32) -> bool) -> Vec<Vec<P>> {
    let mut next: HashMap<(i32, i32), Vec<(i32, i32)>> = HashMap::new();
    for y in 0..h as i32 {
        for x in (0..w as i32).filter(|&x| inside(x, y)) {
            for (out, a, b) in [((x, y - 1), (x, y), (x + 1, y)), ((x + 1, y), (x + 1, y), (x + 1, y + 1)), ((x, y + 1), (x + 1, y + 1), (x, y + 1)), ((x - 1, y), (x, y + 1), (x, y))] {
                if !inside(out.0, out.1) {
                    next.entry(a).or_default().push(b);
                }
            }
        }
    }
    let mut starts: Vec<(i32, i32)> = next.keys().copied().collect();
    starts.sort();
    let mut loops = vec![];
    for s in starts {
        while let Some(first) = next.get_mut(&s).and_then(Vec::pop) {
            let (mut lp, mut cur) = (vec![[s.0 as f32, s.1 as f32]], first);
            while cur != s {
                lp.push([cur.0 as f32, cur.1 as f32]);
                let Some(n) = next.get_mut(&cur).and_then(Vec::pop) else { break };
                cur = n;
            }
            let far = (0..lp.len()).max_by(|&i, &j| off(lp[i], lp[0], lp[0]).total_cmp(&off(lp[j], lp[0], lp[0]))).unwrap_or(0);
            let mut slim = vec![];
            thin(&lp[..=far], 0.8, &mut slim);
            lp.push(lp[0]);
            thin(&lp[far..], 0.8, &mut slim);
            let area: f32 = (0..slim.len()).map(|i| (slim[i], slim[(i + 1) % slim.len()])).map(|(a, b)| a[0] * b[1] - b[0] * a[1]).sum();
            if slim.len() >= 3 && area.abs() >= 4. {
                loops.push(slim);
            }
        }
    }
    loops
}

pub fn trace(rgba: &[u8], w: usize, h: usize, colors: usize) -> Picture {
    let px: Vec<Option<[u8; 3]>> = rgba.chunks_exact(4).map(|p| (p[3] >= 128).then_some([p[0], p[1], p[2]])).collect();
    let mut boxes = vec![px.iter().flatten().copied().collect::<Vec<_>>()];
    let spread = |b: &[[u8; 3]]| (0..3).map(|c| (b.iter().map(|p| p[c]).max().unwrap_or(0) - b.iter().map(|p| p[c]).min().unwrap_or(0), c)).max().unwrap();
    while boxes.len() < colors {
        let weight = |b: &Vec<[u8; 3]>| spread(b).0 as f32 * (b.len() as f32).sqrt();
        let i = (0..boxes.len()).max_by(|&i, &j| weight(&boxes[i]).total_cmp(&weight(&boxes[j]))).unwrap();
        let (range, c) = spread(&boxes[i]);
        if range == 0 {
            break;
        }
        let b = boxes.swap_remove(i);
        let mid = b.iter().map(|p| p[c]).min().unwrap_or(0) + range / 2;
        let (lo, hi): (Vec<_>, Vec<_>) = b.into_iter().partition(|p| p[c] <= mid);
        boxes.extend([lo, hi]);
    }
    let pal: Vec<[u8; 3]> = boxes.iter().filter(|b| !b.is_empty()).map(|b| std::array::from_fn(|c| (b.iter().map(|p| p[c] as usize).sum::<usize>() / b.len()) as u8)).collect();
    let near = |p: [u8; 3]| (0..pal.len()).min_by_key(|&i| (0..3).map(|c| (pal[i][c] as i32 - p[c] as i32).pow(2)).sum::<i32>()).unwrap();
    let idx: Vec<Option<usize>> = px.iter().map(|p| p.map(near)).collect();
    let mut order: Vec<usize> = (0..pal.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(idx.iter().filter(|v| **v == Some(i)).count()));
    let rank: Vec<usize> = idx.iter().map(|v| v.map_or(usize::MAX, |i| order.iter().position(|&o| o == i).unwrap())).collect();
    let at = |x: i32, y: i32| if x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h { rank[y as usize * w + x as usize] } else { usize::MAX };
    let layers = order.iter().enumerate().map(|(j, &i)| (pal[i], outline(w, h, &|x, y| at(x, y) != usize::MAX && at(x, y) >= j))).filter(|l| !l.1.is_empty()).collect();
    let rgba = idx.iter().flat_map(|v| v.map_or([0; 4], |i| [pal[i][0], pal[i][1], pal[i][2], 255])).collect();
    Picture { w, h, rgba, layers }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ring_becomes_an_outline_with_a_hole() {
        let (w, h) = (40, 30);
        let colour = |x: usize, y: usize| {
            let ring = (8..32).contains(&x) && (6..24).contains(&y) && !((14..26).contains(&x) && (10..20).contains(&y));
            if x < 4 { [0, 0, 0, 0] } else if ring { [250, 20, 20, 255] } else { [10, 10, 200, 255] }
        };
        let rgba: Vec<u8> = (0..w * h).flat_map(|i| colour(i % w, i / w)).collect();
        let pic = trace(&rgba, w, h, 2);
        assert_eq!(pic.layers.len(), 2);
        let (blue, red) = (&pic.layers[0], &pic.layers[1]);
        assert!(blue.0[2] > 150 && red.0[0] > 200, "couleurs {:?} {:?}", blue.0, red.0);
        assert_eq!(blue.1, vec![vec![[4., 0.], [40., 0.], [40., 30.], [4., 30.]]]);
        assert_eq!((red.1.len(), red.1[0].len(), red.1[1].len()), (2, 4, 4));
        let turn = |l: &Vec<P>| (0..4).map(|i| l[i][0] * l[(i + 1) % 4][1] - l[(i + 1) % 4][0] * l[i][1]).sum::<f32>();
        assert!(turn(&red.1[0]) * turn(&red.1[1]) < 0., "le trou doit tourner dans l'autre sens");
        assert_eq!(&pic.rgba[..4], &[0, 0, 0, 0]);
    }
}
