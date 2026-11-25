use crate::track::P;
use std::collections::HashMap;
use std::path::Path;

pub struct Picture {
    pub w: usize,
    pub h: usize,
    pub rgba: Vec<u8>,
    pub layers: Vec<Layer>,
}

pub type Layer = ([u8; 3], Vec<Vec<P>>, f32);

pub enum Loaded {
    Raster((Vec<u8>, usize, usize)),
    Vector(Picture),
}

pub fn load(path: &Path) -> Result<Loaded, String> {
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("svg")) {
        return svg(path).map(Loaded::Vector);
    }
    let info = crate::video::probe(path).map_err(|e| e.replace(t!("Vidéo", "Video"), "Image"))?;
    let k = (200. / info.w.max(info.h) as f32).min(1.);
    let (w, h) = (((info.w as f32 * k).round() as usize).max(1), ((info.h as f32 * k).round() as usize).max(1));
    let out = crate::video::cmd("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-frames:v", "1", "-vf", &format!("scale={w}:{h}"), "-pix_fmt", "rgba", "-f", "rawvideo", "-"])
        .output()
        .map_err(|e| tf!("ffmpeg introuvable (installez ffmpeg et ajoutez-le au PATH) : {e}", "ffmpeg not found (install ffmpeg and add it to the PATH): {e}"))?;
    if out.stdout.len() != w * h * 4 {
        return Err(tf!("Image illisible : {}", "Image unreadable: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(Loaded::Raster((out.stdout, w, h)))
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

fn slim(mut lp: Vec<P>, tol: f32) -> Vec<P> {
    let far = (0..lp.len()).max_by(|&i, &j| off(lp[i], lp[0], lp[0]).total_cmp(&off(lp[j], lp[0], lp[0]))).unwrap_or(0);
    let mut out = vec![];
    thin(&lp[..=far], tol, &mut out);
    lp.push(lp[0]);
    thin(&lp[far..], tol, &mut out);
    out
}

fn flatten(path: &usvg::tiny_skia_path::Path) -> Vec<Vec<P>> {
    use usvg::tiny_skia_path::PathSegment as S;
    let (mut loops, mut cur): (Vec<Vec<P>>, Vec<P>) = (vec![], vec![]);
    let mix = |a: P, b: P, t: f32| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
    for seg in path.segments() {
        let last = cur.last().copied().unwrap_or([0., 0.]);
        match seg {
            S::MoveTo(p) => loops.push(std::mem::replace(&mut cur, vec![[p.x, p.y]])),
            S::LineTo(p) => cur.push([p.x, p.y]),
            S::QuadTo(c, p) => cur.extend((1..=8).map(|i| i as f32 / 8.).map(|t| mix(mix(last, [c.x, c.y], t), mix([c.x, c.y], [p.x, p.y], t), t))),
            S::CubicTo(c, d, p) => cur.extend((1..=12).map(|i| i as f32 / 12.).map(|t| {
                let (a, b, e) = (mix(last, [c.x, c.y], t), mix([c.x, c.y], [d.x, d.y], t), mix([d.x, d.y], [p.x, p.y], t));
                mix(mix(a, b, t), mix(b, e, t), t)
            })),
            S::Close => {}
        }
    }
    loops.push(cur);
    loops.into_iter().filter(|l| l.len() >= 3).map(|l| slim(l, 0.25)).filter(|l| l.len() >= 3).collect()
}

fn walk(group: &usvg::Group, k: f32, out: &mut Vec<Layer>) {
    for node in group.children() {
        match node {
            usvg::Node::Group(g) => walk(g, k, out),
            usvg::Node::Path(p) if p.is_visible() => {
                let ts = p.abs_transform().post_scale(k, k);
                let fill = p.fill().map(|f| (f.paint(), f.opacity().get(), p.data().clone()));
                let edge = p.stroke().and_then(|s| Some((s.paint(), s.opacity().get(), p.data().stroke(&s.to_tiny_skia(), k)?)));
                for (paint, solid, path) in fill.into_iter().chain(edge) {
                    let color = match paint {
                        usvg::Paint::Color(c) => [c.red, c.green, c.blue],
                        usvg::Paint::LinearGradient(g) => g.stops().first().map_or([128; 3], |s| [s.color().red, s.color().green, s.color().blue]),
                        usvg::Paint::RadialGradient(g) => g.stops().first().map_or([128; 3], |s| [s.color().red, s.color().green, s.color().blue]),
                        usvg::Paint::Pattern(_) => [128; 3],
                    };
                    let loops = path.transform(ts).map(|p| flatten(&p)).unwrap_or_default();
                    if !loops.is_empty() {
                        out.push((color, loops, solid));
                    }
                }
            }
            _ => {}
        }
    }
}

fn raster(layers: &[Layer], w: usize, h: usize) -> Vec<u8> {
    let mut px = vec![[0f32; 4]; w * h];
    for (color, loops, solid) in layers {
        for y in 0..h {
            let at = y as f32 + 0.5;
            let mut hits: Vec<(f32, i32)> = vec![];
            for lp in loops {
                for i in 0..lp.len() {
                    let (a, b) = (lp[i], lp[(i + 1) % lp.len()]);
                    if (a[1] <= at) != (b[1] <= at) {
                        hits.push((a[0] + (at - a[1]) / (b[1] - a[1]) * (b[0] - a[0]), if b[1] > a[1] { 1 } else { -1 }));
                    }
                }
            }
            hits.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut wind = 0;
            for pair in hits.windows(2) {
                wind += pair[0].1;
                if wind != 0 {
                    for x in (pair[0].0.round().max(0.) as usize)..(pair[1].0.round().clamp(0., w as f32) as usize) {
                        let p = &mut px[y * w + x];
                        *p = std::array::from_fn(|c| if c < 3 { color[c] as f32 * solid + p[c] * (1. - solid) } else { solid + p[3] * (1. - solid) });
                    }
                }
            }
        }
    }
    px.iter().flat_map(|p| [(p[0] / p[3].max(1e-6)) as u8, (p[1] / p[3].max(1e-6)) as u8, (p[2] / p[3].max(1e-6)) as u8, (p[3] * 255.) as u8]).collect()
}

fn svg(path: &Path) -> Result<Picture, String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    let tree = usvg::Tree::from_data(&data, &usvg::Options::default()).map_err(|e| tf!("SVG illisible : {e}", "SVG unreadable: {e}"))?;
    let size = tree.size();
    let k = 400. / size.width().max(size.height());
    let (w, h) = (((size.width() * k).ceil() as usize).max(1), ((size.height() * k).ceil() as usize).max(1));
    let mut layers = vec![];
    walk(tree.root(), k, &mut layers);
    if layers.is_empty() {
        return Err(t!("Ce SVG ne contient aucune forme pleine ou tracée.", "This SVG holds no filled or stroked shape.").into());
    }
    Ok(Picture { w, h, rgba: raster(&layers, w, h), layers })
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
            let slim = slim(lp, 0.8);
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
    let layers = order.iter().enumerate().map(|(j, &i)| (pal[i], outline(w, h, &|x, y| at(x, y) != usize::MAX && at(x, y) >= j), 1.)).filter(|l| !l.1.is_empty()).collect();
    let rgba = idx.iter().flat_map(|v| v.map_or([0; 4], |i| [pal[i][0], pal[i][1], pal[i][2], 255])).collect();
    Picture { w, h, rgba, layers }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_svg_keeps_its_shapes_colours_and_opacity() {
        let path = std::env::temp_dir().join(format!("aegisigntrack-test-{}.svg", std::process::id()));
        std::fs::write(&path, r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50" viewBox="0 0 100 50"><rect x="10" y="10" width="40" height="30" fill="#ff0000"/><circle cx="75" cy="25" r="15" fill="#0000ff" fill-opacity="0.5"/></svg>"##).unwrap();
        let loaded = load(&path);
        std::fs::remove_file(&path).unwrap();
        let Ok(Loaded::Vector(pic)) = loaded else { panic!("SVG non lu") };
        assert_eq!((pic.w, pic.h, pic.layers.len()), (400, 200, 2));
        assert_eq!((pic.layers[0].0, pic.layers[0].2), ([255, 0, 0], 1.));
        let mut corners = pic.layers[0].1[0].clone();
        corners.sort_by(|a, b| (a[0] + 2. * a[1]).total_cmp(&(b[0] + 2. * b[1])));
        assert_eq!(corners, vec![[40., 40.], [200., 40.], [40., 160.], [200., 160.]]);
        assert_eq!((pic.layers[1].0, pic.layers[1].2), ([0, 0, 255], 0.5));
        assert!(pic.layers[1].1[0].len() > 12, "cercle en {} points", pic.layers[1].1[0].len());
        let at = |x: usize, y: usize| &pic.rgba[(y * 400 + x) * 4..][..4];
        assert_eq!((at(120, 100), at(300, 100), at(390, 10)), (&[255, 0, 0, 255][..], &[0, 0, 255, 127][..], &[0, 0, 0, 0][..]));
    }

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
