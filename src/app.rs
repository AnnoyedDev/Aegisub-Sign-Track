use crate::cover;
use crate::export::{self, Cover, Cut, Export, Follow, Gradient, OutLine, SrcLine};
use crate::picture::{self, Loaded, Picture};
use crate::track::{self, Channel, FrameData, Gray, Motion, Params, P};
use crate::video::{self, Clip, Info, Loading, Matrix};
use crate::{lang, Job};
use eframe::egui::{self, Align2, Color32, FontId, PointerButton, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Vec2};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

#[derive(PartialEq, Clone, Copy)]
enum Mode {
    Track,
    Mask,
    Text,
}

#[derive(PartialEq, Clone, Copy)]
enum Pick {
    Mask,
    Gradient(usize),
    Text,
    Outline,
    Shadow,
}

struct TextStyle {
    custom: bool,
    font: String,
    size: f32,
    bold: bool,
    italic: bool,
    color: [u8; 3],
    outline_color: [u8; 3],
    outline: f32,
    shadow_color: [u8; 3],
    shadow: f32,
    blur: f32,
}

impl TextStyle {
    fn tags(&self) -> String {
        let bgr = |c: [u8; 3]| format!("&H{:02X}{:02X}{:02X}&", c[2], c[1], c[0]);
        format!(
            "\\fn{}\\fs{}\\b{}\\i{}\\c{}\\3c{}\\4c{}\\bord{}\\shad{}\\blur{}",
            self.font,
            self.size,
            self.bold as u8,
            self.italic as u8,
            bgr(self.color),
            bgr(self.outline_color),
            bgr(self.shadow_color),
            self.outline,
            self.shadow,
            self.blur
        )
    }

    fn apply(&self, text: &str) -> String {
        match (self.custom, text.strip_prefix('{')) {
            (false, _) => text.into(),
            (true, Some(rest)) => format!("{{{}{rest}", self.tags()),
            (true, None) => format!("{{{}}}{text}", self.tags()),
        }
    }
}

#[derive(Clone, Copy)]
enum Drag {
    Vertex(usize),
    Whole,
}

enum Msg {
    Frame(usize, FrameData),
    Done(Option<String>),
}

struct Worker {
    rx: mpsc::Receiver<Msg>,
    stop: Arc<AtomicBool>,
    what: &'static str,
}

struct Snap {
    zone: bool,
    shape: Vec<P>,
    hole: Vec<P>,
    mask: Vec<P>,
    anchor: Option<P>,
    ref_pts: Vec<P>,
    opa: Vec<(P, f32)>,
    warp: bool,
    ref_frame: usize,
    mix: [f32; 3],
    frames: Vec<Option<FrameData>>,
}

struct Pending {
    path: PathBuf,
    info: Info,
    start: f64,
    dur: f64,
}

struct View {
    fd: Option<FrameData>,
    ref_pts: Vec<P>,
    warp: bool,
}

impl View {
    fn place(&self, c: P, fol: Follow, v: P) -> P {
        self.fd.as_ref().map_or(v, |f| export::place(f, &self.ref_pts, self.warp, c, fol, v))
    }
    fn unplace(&self, c: P, fol: Follow, q: P) -> P {
        self.fd.as_ref().map_or(q, |f| export::unplace(f, &self.ref_pts, self.warp, c, fol, q))
    }
    fn map(&self, p: P) -> P {
        self.fd.as_ref().map_or(p, |f| f.map(&self.ref_pts, self.warp, p))
    }
    fn unmap(&self, q: P) -> P {
        self.fd.as_ref().map_or(q, |f| f.unmap(&self.ref_pts, self.warp, q))
    }
}

struct Object {
    cover: usize,
    tolerance: i32,
    grid: Option<(u64, Vec<P>, Vec<[u8; 3]>)>,
    seen: Vec<cover::Seen>,
    fits: Vec<cover::Fit>,
    covers: Vec<Cover>,
    axis: usize,
    mask_layer: i32,
    text_layer: i32,
    ease: [u32; 3],
    grow: f32,
    twin: bool,
    pin: bool,
    color_spot: Option<P>,
    color_live: bool,
    cut: Cut,
    zone: bool,
    shape: Vec<P>,
    hole: Vec<P>,
    mask: Vec<P>,
    anchor: Option<P>,
    motion: Motion,
    mesh: bool,
    mesh_size: f32,
    search: f32,
    pattern: i32,
    channel: Channel,
    smoothing: u32,
    ref_pts: Vec<P>,
    opa: Vec<(P, f32)>,
    warp: bool,
    frames: Vec<Option<FrameData>>,
    mask_on: bool,
    text_on: bool,
    gradients: Vec<Gradient>,
    bands: usize,
    image: Option<(Vec<u8>, usize, usize)>,
    image_colors: usize,
    picture: Option<(Picture, egui::TextureHandle)>,
    mask_follow: Follow,
    text_follow: Follow,
    style: TextStyle,
    style_name: String,
    color: [u8; 3],
    blur: f32,
    opacity: bool,
    mask_opacity: bool,
    text_opacity: bool,
    import: bool,
    realign: bool,
    ref_frame: usize,
    mix: [f32; 3],
    text: String,
    undo: Vec<Snap>,
    redo: Vec<Snap>,
}

impl Object {
    fn smoothed(&self) -> Vec<Option<FrameData>> {
        let centre = export::centroid(if self.shape.is_empty() { &self.ref_pts } else { &self.shape });
        track::ease(&track::smooth(&self.frames, self.smoothing as f32 * 0.5), centre, self.ease.map(|v| v as f32 * 0.5))
    }

    fn tracked(&self) -> bool {
        self.frames.iter().any(Option::is_some)
    }

    fn mask_poly(&self) -> &[P] {
        if self.mask.is_empty() { &self.shape } else { &self.mask }
    }

    fn anchor_pt(&self) -> P {
        self.anchor.unwrap_or_else(|| export::centroid(self.mask_poly()))
    }

    fn new() -> Object {
        Object {
            cover: 0,
            tolerance: 12,
            grid: None,
            seen: vec![],
            fits: vec![],
            covers: vec![],
            axis: 0,
            mask_layer: 0,
            text_layer: 0,
            ease: [0; 3],
            grow: 0.,
            twin: false,
            pin: true,
            color_spot: None,
            color_live: false,
            cut: Cut::None,
            zone: true,
            shape: vec![],
            hole: vec![],
            mask: vec![],
            anchor: None,
            motion: Motion { translation: true, scale: true, rotation: true, shear: false, perspective: false },
            mesh: false,
            mesh_size: 40.,
            search: 24.,
            pattern: 10,
            channel: Channel::Auto,
            smoothing: 0,
            ref_pts: vec![],
            opa: vec![],
            warp: false,
            frames: vec![],
            mask_on: false,
            text_on: false,
            gradients: vec![],
            bands: 16,
            image: None,
            image_colors: 8,
            picture: None,
            mask_follow: Follow::ALL,
            text_follow: Follow::ALL,
            style: TextStyle { custom: false, font: "Arial".into(), size: 60., bold: false, italic: false, color: [255, 255, 255], outline_color: [0, 0, 0], outline: 2., shadow_color: [0, 0, 0], shadow: 0., blur: 0. },
            style_name: "Default".into(),
            color: [0, 0, 0],
            blur: 0.,
            opacity: false,
            mask_opacity: false,
            text_opacity: false,
            import: false,
            realign: true,
            ref_frame: 0,
            mix: [0.299, 0.587, 0.114],
            text: t!("Texte", "Text").into(),
            undo: vec![],
            redo: vec![],
        }
    }
}

trait Knob {
    fn put(&self) -> String;
    fn get(&mut self, s: &str);
}

macro_rules! plain {
    ($($t:ty),*) => {
        $(impl Knob for $t {
            fn put(&self) -> String {
                self.to_string()
            }
            fn get(&mut self, s: &str) {
                if let Ok(v) = s.parse() {
                    *self = v;
                }
            }
        })*
    };
}

plain!(bool, f32, i32, u32, usize);

impl Knob for String {
    fn put(&self) -> String {
        self.clone()
    }
    fn get(&mut self, s: &str) {
        *self = s.into();
    }
}

macro_rules! choice {
    ($t:ty, $($v:expr),*) => {
        impl Knob for $t {
            fn put(&self) -> String {
                [$($v),*].iter().position(|v| v == self).unwrap_or(0).to_string()
            }
            fn get(&mut self, s: &str) {
                if let Some(v) = s.parse::<usize>().ok().and_then(|i| [$($v),*].get(i).copied()) {
                    *self = v;
                }
            }
        }
    };
}

choice!(Channel, Channel::Auto, Channel::Luma, Channel::R, Channel::G, Channel::B);
choice!(Matrix, Matrix::Auto, Matrix::Bt709, Matrix::Bt601);
choice!(Cut, Cut::None, Cut::Inside, Cut::Outside);

fn flags<const N: usize>(s: &str) -> Option<[bool; N]> {
    (s.len() == N).then(|| std::array::from_fn(|i| s.as_bytes()[i] == b'1'))
}

fn digits(v: &[bool]) -> String {
    v.iter().map(|b| if *b { '1' } else { '0' }).collect()
}

impl Knob for [u8; 3] {
    fn put(&self) -> String {
        format!("{},{},{}", self[0], self[1], self[2])
    }
    fn get(&mut self, s: &str) {
        let v: Vec<u8> = s.split(',').filter_map(|t| t.parse().ok()).collect();
        if let Ok(v) = v.try_into() {
            *self = v;
        }
    }
}

impl Knob for Motion {
    fn put(&self) -> String {
        digits(&[self.translation, self.scale, self.rotation, self.shear, self.perspective])
    }
    fn get(&mut self, s: &str) {
        if let Some([translation, scale, rotation, shear, perspective]) = flags(s) {
            *self = Motion { translation, scale, rotation, shear, perspective };
        }
    }
}

impl Knob for Follow {
    fn put(&self) -> String {
        digits(&[self.translation, self.scale, self.rotation, self.deform])
    }
    fn get(&mut self, s: &str) {
        if let Some([translation, scale, rotation, deform]) = flags(s) {
            *self = Follow { translation, scale, rotation, deform };
        }
    }
}

impl Knob for Vec<Gradient> {
    fn put(&self) -> String {
        self.iter().map(|g| format!("{},{},{}", g.color.put(), g.angle, g.force)).collect::<Vec<_>>().join(";")
    }
    fn get(&mut self, s: &str) {
        *self = s.split(';').filter_map(|g| {
            let v: Vec<f32> = g.split(',').filter_map(|t| t.parse().ok()).collect();
            (v.len() == 5).then(|| Gradient { color: [v[0] as u8, v[1] as u8, v[2] as u8], angle: v[3], force: v[4] })
        }).collect();
    }
}

fn dots(v: &[P]) -> String {
    v.iter().map(|p| format!("{},{}", p[0], p[1])).collect::<Vec<_>>().join(";")
}

fn undots(s: &str) -> Vec<P> {
    s.split(';').filter_map(|t| t.split_once(',')).filter_map(|(x, y)| Some([x.parse().ok()?, y.parse().ok()?])).collect()
}

fn newer(tag: &str, build: &str) -> bool {
    let mine = build.trim_end_matches("+local");
    tag != mine && tag.starts_with(|c: char| c.is_ascii_digit()) && tag.get(..10).is_some_and(|d| Some(d) >= mine.get(..10))
}

pub fn language() {
    let saved = config().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
    lang::set(saved.lines().find_map(|l| l.strip_prefix("langue=")).map_or_else(lang::system, |l| l == "en"));
}

fn config() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA").or_else(|| std::env::var_os("XDG_CONFIG_HOME")).map(PathBuf::from).or_else(|| Some(PathBuf::from(std::env::var_os("HOME")?).join(".config")))?;
    Some(base.join("aegisigntrack").join("config.txt"))
}

type Crop = (u64, [usize; 2], [usize; 2], Vec<u8>);

struct Batch {
    keys: Vec<u64>,
    rx: mpsc::Receiver<Option<Crop>>,
    stop: Arc<AtomicBool>,
    started: bool,
}

fn render(script: String, w: usize, h: usize, keys: Vec<u64>, tx: mpsc::Sender<Option<Crop>>, stop: Arc<AtomicBool>, ctx: egui::Context) {
    use std::io::Read;
    static COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir();
    let name = format!("aegisigntrack-{}-apercu-{}.ass", std::process::id(), COUNT.fetch_add(1, Ordering::Relaxed));
    let source = format!("color=c=black@0.0:s={w}x{h}:r=25:d={},format=rgba", keys.len() as f64 / 25. + 1.);
    let child = std::fs::write(dir.join(&name), script).and_then(|_| {
        let args = ["-v", "error", "-f", "lavfi", "-i", &source, "-vf", &format!("ass={name}:alpha=1"), "-frames:v", &keys.len().to_string(), "-f", "rawvideo", "-pix_fmt", "rgba", "-"];
        video::cmd("ffmpeg").current_dir(&dir).args(args).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).spawn()
    });
    let mut drawn = false;
    if let Ok(mut child) = child {
        let mut out = child.stdout.take().unwrap();
        let mut frame = vec![0u8; w * h * 4];
        for key in keys {
            if stop.load(Ordering::Relaxed) || out.read_exact(&mut frame).is_err() {
                break;
            }
            drawn = true;
            let lit = |x: usize, y: usize| frame[(y * w + x) * 4 + 3] != 0;
            let ys: Vec<usize> = (0..h).filter(|&y| (0..w).any(|x| lit(x, y))).collect();
            let crop = match (ys.first(), ys.last()) {
                (Some(&y0), Some(&y1)) => {
                    let xs: Vec<usize> = (0..w).filter(|&x| (y0..=y1).any(|y| lit(x, y))).collect();
                    let (x0, x1) = (xs[0], xs[xs.len() - 1]);
                    (key, [x0, y0], [x1 - x0 + 1, y1 - y0 + 1], (y0..=y1).flat_map(|y| frame[(y * w + x0) * 4..(y * w + x1 + 1) * 4].iter().copied()).collect())
                }
                _ => (key, [0, 0], [0, 0], vec![]),
            };
            if tx.send(Some(crop)).is_err() {
                break;
            }
            ctx.request_repaint();
        }
        let _ = child.kill();
        let _ = child.wait();
    }
    if !drawn && !stop.load(Ordering::Relaxed) {
        let _ = tx.send(None);
    }
    ctx.request_repaint();
    let _ = std::fs::remove_file(dir.join(&name));
}

pub struct App {
    see: bool,
    tex_mark: u64,
    loop_play: bool,
    hide: bool,
    check: bool,
    check_now: bool,
    update: Option<mpsc::Receiver<String>>,
    news: Option<String>,
    resume: Option<String>,
    curves: bool,
    loupe: bool,
    cuts: bool,
    previews: std::collections::HashMap<u64, Option<(egui::TextureHandle, [usize; 2])>>,
    preview_bytes: usize,
    last_preview: Option<u64>,
    rendering: Option<Batch>,
    no_preview: bool,
    ffmpeg_dir: Option<PathBuf>,
    group: bool,
    holing: bool,
    lock_view: bool,
    graph: bool,
    o: Object,
    objects: Vec<Object>,
    sel: usize,
    job: Option<Job>,
    pending: Option<Pending>,
    loading: Option<(Loading, Info, f64, bool)>,
    source: Option<(PathBuf, Info, f64, usize)>,
    matrix: Matrix,
    clip: Option<Arc<Clip>>,
    fps: f64,
    times: Vec<i64>,
    res: [f64; 2],
    cur: usize,
    tex: Option<(usize, egui::TextureHandle)>,
    playing: bool,
    next_tick: f64,
    zoom: f32,
    pan: Vec2,
    fit: bool,
    mode: Mode,
    drag: Option<Drag>,
    shown: Vec<Option<FrameData>>,
    dirty: bool,
    range: [usize; 2],
    worker: Option<Worker>,
    confirm: Option<bool>,
    fonts: Vec<String>,
    ask_import: bool,
    image_ask: Option<(Loaded, usize)>,
    timestamps: bool,
    merge: bool,
    picking: Option<Pick>,
    status: String,
}

const ACCENT: Color32 = Color32::from_rgb(255, 196, 0);
const HIT: f32 = 10.;

fn add(p: P, d: P) -> P {
    [p[0] + d[0], p[1] + d[1]]
}

impl App {
    pub fn new(job: Option<Job>, open: Option<PathBuf>, error: Option<String>) -> Self {
        let saved = config().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
        let key = |k: &str| saved.lines().find_map(|l| l.strip_prefix(k)?.strip_prefix('='));
        let ffmpeg_dir = key("ffmpeg").map(PathBuf::from);
        let check = key("maj") != Some("0");
        video::set_dir(ffmpeg_dir.clone());
        let error = error.or_else(video::missing);
        let mut app = App {
            see: false,
            tex_mark: 0,
            loop_play: false,
            hide: false,
            check,
            check_now: check,
            update: None,
            news: None,
            resume: None,
            curves: false,
            loupe: false,
            cuts: true,
            previews: Default::default(),
            preview_bytes: 0,
            last_preview: None,
            rendering: None,
            no_preview: false,
            ffmpeg_dir,
            group: false,
            holing: false,
            lock_view: false,
            graph: false,
            o: Object::new(),
            objects: vec![Object::new()],
            sel: 0,
            job: None,
            pending: None,
            loading: None,
            source: None,
            matrix: Matrix::Auto,
            clip: None,
            fps: 25.,
            times: vec![],
            res: [1920., 1080.],
            cur: 0,
            tex: None,
            playing: false,
            next_tick: 0.,
            zoom: 1.,
            pan: Vec2::ZERO,
            fit: true,
            mode: Mode::Track,
            drag: None,
            shown: vec![],
            dirty: false,
            range: [0, 0],
            worker: None,
            confirm: None,
            fonts: vec![],
            ask_import: false,
            image_ask: None,
            timestamps: true,
            merge: true,
            picking: None,
            status: error.unwrap_or_else(|| t!("Ouvrez une vidéo, ou glissez-la dans la fenêtre.", "Open a video, or drop it into the window.").into()),
        };
        if let Some(j) = job {
            let (start, n) = ((j.times[0] as f64 - 0.4) / 1000., j.times.len() - 1);
            app.ask_import = j.lines.iter().any(|l| !export::strip_tags(&l.text).trim().is_empty());
            match video::probe(&j.video) {
                Ok(info) => app.load(&j.video.clone(), info, start, n),
                Err(e) => app.status = e,
            }
            app.job = Some(j);
        } else if let Some(p) = open {
            app.open(p);
        }
        if key("langue").is_none() && !cfg!(test) {
            app.save_config();
        }
        app
    }

    fn open(&mut self, path: PathBuf) {
        match video::probe(&path) {
            Ok(info) => self.pending = Some(Pending { path, info, start: 0., dur: info.duration.clamp(0.1, 10.) }),
            Err(e) => self.status = e,
        }
    }

    fn load(&mut self, path: &std::path::Path, info: Info, start: f64, frames: usize) {
        self.source = Some((path.to_path_buf(), info, start, frames));
        self.reload(false);
    }

    fn reload(&mut self, keep: bool) {
        let Some((path, info, start, frames)) = self.source.clone() else { return };
        self.stop();
        match video::extract(&path, &info, start, frames, self.matrix) {
            Ok(l) => {
                self.loading = Some((l, info, start, keep));
                self.status = t!("Extraction de la plage vidéo...", "Extracting the video range...").into();
            }
            Err(e) => self.status = e,
        }
    }

    fn loaded(&mut self, clip: Clip, info: Info, start: f64, keep: bool) {
        if keep && self.o.frames.len() == clip.n {
            self.tex = None;
            self.status = t!("Vidéo rechargée avec la nouvelle matrice de couleur.", "Video reloaded with the new colour matrix.").into();
            self.clip = Some(Arc::new(clip));
            return;
        }
        self.fps = info.fps;
        (self.times, self.res) = match &self.job {
            Some(j) => (j.times.iter().copied().take(clip.n + 1).collect(), j.res),
            None => ((0..=clip.n).map(|i| ((start + i as f64 / info.fps) * 1000.).round() as i64).collect(), [info.w as f64, info.h as f64]),
        };
        let pos = self.job.as_ref().and_then(|j| export::line_pos(&j.lines[0].text));
        self.o.anchor = pos.map(|(x, y)| [(x * clip.w as f64 / self.res[0]) as f32, (y * clip.h as f64 / self.res[1]) as f32]);
        (self.objects, self.sel) = (vec![Object::new()], 0);
        (self.o.shape, self.o.mask) = (vec![], vec![]);
        (self.o.undo, self.o.redo) = (vec![], vec![]);
        self.o.frames = vec![None; clip.n];
        self.dirty = true;
        (self.cur, self.range, self.tex, self.fit) = (0, [0, clip.n - 1], None, true);
        self.status = tf!("{} images chargées ({}×{}). Dessinez la sélection puis lancez le tracking.", "{} frames loaded ({}×{}). Draw the selection, then start tracking.", clip.n, clip.w, clip.h);
        (self.previews, self.preview_bytes, self.last_preview) = (Default::default(), 0, None);
        self.clip = Some(Arc::new(clip));
        self.recall();
        self.resume = self.session().and_then(|f| std::fs::read_to_string(f).ok());
    }

    fn snap(&self) -> Snap {
        Snap { zone: self.o.zone, shape: self.o.shape.clone(), hole: self.o.hole.clone(), mask: self.o.mask.clone(), anchor: self.o.anchor, ref_pts: self.o.ref_pts.clone(), opa: self.o.opa.clone(), warp: self.o.warp, ref_frame: self.o.ref_frame, mix: self.o.mix, frames: self.o.frames.clone() }
    }

    fn checkpoint(&mut self) {
        self.o.undo.push(self.snap());
        if self.o.undo.len() > 100 {
            self.o.undo.remove(0);
        }
        self.o.redo.clear();
    }

    fn time_travel(&mut self, back: bool) {
        let popped = if back { self.o.undo.pop() } else { self.o.redo.pop() };
        let Some(s) = popped else {
            self.status = if back { t!("Rien à annuler.", "Nothing to undo.").into() } else { t!("Rien à rétablir.", "Nothing to redo.").into() };
            return;
        };
        let now = self.snap();
        if back { self.o.redo.push(now) } else { self.o.undo.push(now) }
        (self.o.zone, self.o.shape, self.o.mask, self.o.anchor, self.o.ref_pts, self.o.opa, self.o.warp, self.o.frames) = (s.zone, s.shape, s.mask, s.anchor, s.ref_pts, s.opa, s.warp, s.frames);
        self.o.ref_frame = s.ref_frame;
        self.o.hole = s.hole;
        self.o.mix = s.mix;
        self.dirty = true;
        self.status = if back { t!("Action annulée (Ctrl+Maj+Z pour la rétablir).", "Action undone (Ctrl+Shift+Z to redo it).").into() } else { t!("Action rétablie.", "Action redone.").into() };
    }

    fn tracked(&self) -> bool {
        self.o.tracked()
    }

    fn nearest(&self, i: usize) -> Option<usize> {
        (0..self.o.frames.len()).filter(|&j| self.o.frames[j].is_some()).min_by_key(|&j| j.abs_diff(i))
    }

    fn mask_poly(&self) -> &[P] {
        self.o.mask_poly()
    }

    fn anchor_pt(&self) -> P {
        self.o.anchor_pt()
    }

    fn all(&self) -> impl Iterator<Item = &Object> {
        (0..self.objects.len()).map(|i| if i == self.sel { &self.o } else { &self.objects[i] })
    }

    fn select(&mut self, i: usize) {
        if i == self.sel || i >= self.objects.len() {
            return;
        }
        std::mem::swap(&mut self.o, &mut self.objects[self.sel]);
        self.sel = i;
        std::mem::swap(&mut self.o, &mut self.objects[i]);
        (self.dirty, self.drag, self.picking) = (true, None, None);
    }

    fn add_object(&mut self) {
        let mut o = Object::new();
        o.frames = vec![None; self.o.frames.len()];
        self.objects.push(o);
        self.select(self.objects.len() - 1);
    }

    fn remove_object(&mut self) {
        let gone = self.sel;
        self.select(if gone == 0 { 1 } else { gone - 1 });
        self.objects.remove(gone);
        if gone < self.sel {
            self.sel -= 1;
        }
    }

    fn stop(&mut self) {
        if let Some(w) = &self.worker {
            w.stop.store(true, Ordering::Relaxed);
        }
    }

    fn clear_track(&mut self) {
        self.o.frames.iter_mut().for_each(|f| *f = None);
        self.dirty = true;
    }

    fn start_track(&mut self, dirs: &[i32], ctx: &egui::Context) {
        let Some(clip) = self.clip.clone() else { return };
        if self.o.shape.is_empty() || (self.o.zone && self.o.shape.len() < 3) {
            self.status = if self.o.zone { t!("Dessinez d'abord une zone d'au moins 3 points sur la vidéo.", "First draw an area of at least 3 points on the video.").into() } else { t!("Posez d'abord au moins un point sur la vidéo.", "First place at least one point on the video.").into() };
            return;
        }
        self.checkpoint();
        if !self.tracked() {
            if let Ok(rgb) = clip.rgb(self.cur) {
                self.o.mix = track::contrast(&rgb, clip.w, clip.h, &self.o.shape, self.o.pattern as f32);
            }
        }
        let (p, load) = self.tracker(&clip);
        if !self.tracked() {
            self.o.ref_frame = self.cur;
            let Some(g) = load(self.cur) else {
                self.status = t!("Lecture de la vidéo impossible.", "Cannot read the video.").into();
                return;
            };
            self.o.ref_pts = if !self.o.zone {
                self.o.shape.clone()
            } else if self.o.mesh {
                track::mesh_nodes(&self.o.shape, self.o.mesh_size)
            } else {
                track::detect(&g[0], &self.o.shape)
            };
            let (zone, hole) = (self.o.zone, self.o.hole.clone());
            self.o.ref_pts.retain(|q| !zone || !track::in_poly(&hole, *q));
            if self.o.ref_pts.is_empty() {
                self.status = t!("Rien à suivre : la zone est hors de l'image.", "Nothing to follow: the area is outside the picture.").into();
                return;
            }
            self.o.opa = track::opacity_samples(&g[0], &self.o.shape, self.o.pattern);
            self.o.opa.retain(|q| !zone || !track::in_poly(&hole, q.0));
            self.o.warp = p.warp;
            self.o.frames[self.cur] = Some(FrameData { h: track::IDENT, pts: self.o.ref_pts.clone(), opacity: 1., guessed: false, score: 1. });
        }
        let mut passes = vec![];
        for &d in dirs {
            let (start, end) = if d > 0 {
                ((0..=self.cur).rev().find(|&i| self.o.frames[i].is_some()), self.range[1])
            } else {
                ((self.cur..clip.n).find(|&i| self.o.frames[i].is_some()), self.range[0])
            };
            let keys = self.job.as_ref().filter(|_| self.cuts).map_or(&[][..], |j| &j.keyframes);
            let end = match start {
                Some(s) if d > 0 => keys.iter().filter(|&&k| k > s).min().map_or(end, |&k| end.min(k - 1)),
                Some(s) => keys.iter().filter(|&&k| k <= s).max().map_or(end, |&k| end.max(k)),
                None => end,
            };
            if let Some(s) = start.filter(|&s| if d > 0 { end > s } else { end < s }) {
                passes.push((s, self.o.frames[s].clone().unwrap(), end));
            }
        }
        self.dirty = true;
        if passes.is_empty() {
            self.status = t!("Rien à tracker dans ce sens : placez-vous sur une frame déjà trackée, à l'intérieur de la plage.", "Nothing to track in this direction: go to a frame that is already tracked, inside the range.").into();
            return;
        }
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let (ref_pts, opa, stop2, ctx) = (self.o.ref_pts.clone(), self.o.opa.clone(), stop.clone(), ctx.clone());
        let zone = if p.warp { vec![] } else { self.o.shape.clone() };
        let ref_frame = self.o.ref_frame;
        std::thread::spawn(move || {
            let mut err = None;
            for (s, fd, end) in passes {
                let mut emit = |i, f| {
                    let _ = tx.send(Msg::Frame(i, f));
                    ctx.request_repaint();
                };
                if let Err(e) = track::run(&load, &ref_pts, &zone, &opa, Some(ref_frame), s, fd, end, &p, &stop2, &mut emit) {
                    err.get_or_insert(e);
                }
            }
            let _ = tx.send(Msg::Done(err));
            ctx.request_repaint();
        });
        self.worker = Some(Worker { rx, stop, what: "Tracking" });
        self.playing = false;
        self.status = t!("Tracking en cours...", "Tracking...").into();
    }

    fn tracker(&self, clip: &Arc<Clip>) -> (Params, impl Fn(usize) -> Option<Vec<Gray>> + Send + 'static) {
        let p = Params {
            motion: self.o.motion,
            search: self.o.search,
            pattern: self.o.pattern,
            channel: if self.o.channel == Channel::Auto { Channel::Mix(self.o.mix) } else { self.o.channel },
            user_pts: !self.o.zone,
            warp: !self.o.zone || self.o.mesh,
            hold_fades: self.o.opacity,
            realign: self.o.realign,
            hole: self.o.hole.clone(),
            axis: self.o.axis as u8,
        };
        let (clip, q) = (clip.clone(), p.clone());
        (p, move |i: usize| clip.rgb(i).ok().map(|rgb| track::pyramid(Gray::from_rgb(&rgb, clip.w, clip.h, q.channel), q.search, q.pattern)))
    }

    fn start_refine(&mut self, ctx: &egui::Context) {
        let Some(clip) = self.clip.clone() else { return };
        let (p, load) = self.tracker(&clip);
        let frames: Vec<(usize, FrameData)> = (self.range[0]..=self.range[1]).filter_map(|i| Some((i, self.o.frames[i].clone()?))).collect();
        self.checkpoint();
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let (ref_pts, ref_frame, stop2, ctx) = (self.o.ref_pts.clone(), self.o.ref_frame, stop.clone(), ctx.clone());
        std::thread::spawn(move || {
            let mut emit = |i, f| {
                let _ = tx.send(Msg::Frame(i, f));
                ctx.request_repaint();
            };
            let err = track::refine(&load, &ref_pts, ref_frame, frames, &p, &stop2, &mut emit).err();
            let _ = tx.send(Msg::Done(err));
            ctx.request_repaint();
        });
        self.worker = Some(Worker { rx, stop, what: t!("Affinage", "Refining") });
        self.playing = false;
        self.status = t!("Affinage en cours...", "Refining...").into();
    }

    fn preview_events(&self, clip: &Clip, frame: usize, times: [i64; 2]) -> String {
        let o = &self.o;
        let styled = |t: &str| o.style.apply(t);
        let lines: Vec<SrcLine> = match &self.job {
            Some(j) if o.import => j.lines.iter().filter(|l| (l.first..l.end).contains(&frame)).map(|l| l.with_text(&styled(&l.text))).collect(),
            Some(j) => vec![j.lines[0].with_text(&styled(&o.text))],
            None => vec![SrcLine::standalone(&styled(&o.text), &o.style_name)],
        };
        let fd = self.nearest(frame).and_then(|i| self.shown[i].clone());
        let one = [Some(fd.unwrap_or(FrameData { h: track::IDENT, pts: o.ref_pts.clone(), opacity: 1., guessed: false, score: 1. }))];
        let hides = [o.covers.get(frame).copied().unwrap_or(Cover::Clear)];
        let mut e = self.exporter(o, &one, clip, false);
        (e.times, e.group, e.covers) = (&times, false, &hides);
        export::clipboard(&e.text_lines(&lines), &lines, 0)
    }

    fn preview_key(head: &str, events: &str, clip: &Clip) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (head, events, clip.w, clip.h).hash(&mut h);
        h.finish()
    }

    fn take_previews(&mut self, ctx: &egui::Context) {
        let Some(b) = &mut self.rendering else { return };
        let mut over = false;
        loop {
            match b.rx.try_recv() {
                Ok(Some((key, at, size, rgba))) => {
                    b.started = true;
                    self.preview_bytes += rgba.len();
                    let tex = (!rgba.is_empty()).then(|| (ctx.load_texture("preview", egui::ColorImage::from_rgba_unmultiplied(size, &rgba), Default::default()), at));
                    self.previews.insert(key, tex);
                }
                Ok(None) => self.no_preview = true,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    over = true;
                    break;
                }
            }
        }
        if over {
            self.rendering = None;
        }
        if self.preview_bytes > 400 << 20 {
            self.previews.retain(|k, _| Some(*k) == self.last_preview);
            self.preview_bytes = 0;
        }
    }

    fn start_previews(&mut self, ctx: &egui::Context, clip: &Clip, key: u64) {
        if let Some(b) = self.rendering.take() {
            b.stop.store(true, Ordering::Relaxed);
        }
        let head = self.ass_head();
        let ahead = if self.playing { 300 } else { 1 };
        let (first, last) = (self.range[0].min(self.cur), self.range[1].max(self.cur));
        let (mut keys, mut events) = (vec![], String::new());
        for i in (self.cur..=last).chain(first..self.cur).take(ahead) {
            let k = if i == self.cur { key } else { Self::preview_key(&head, &self.preview_events(clip, i, [0, 10_000]), clip) };
            if self.previews.contains_key(&k) || keys.contains(&k) {
                continue;
            }
            let slot = keys.len() as i64 * 40;
            events += &self.preview_events(clip, i, [(slot - 20).max(0), slot + 20]);
            keys.push(k);
        }
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let (w, h, ctx, list, halt) = (clip.w, clip.h, ctx.clone(), keys.clone(), stop.clone());
        std::thread::spawn(move || render(head + &events, w, h, list, tx, halt, ctx));
        self.rendering = Some(Batch { keys, rx, stop, started: false });
    }

    fn ass_head(&self) -> String {
        let styles = match &self.job {
            Some(j) if !j.styles.is_empty() => j.styles.join("\n"),
            _ => format!("Style: {},Arial,{},&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1", self.o.style_name, (self.res[1] / 15.).round()),
        };
        format!("[Script Info]\nScriptType: v4.00+\nPlayResX: {}\nPlayResY: {}\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n{styles}\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n", self.res[0], self.res[1])
    }

    fn measured_blur(&self) -> Option<f32> {
        let clip = self.clip.as_ref()?;
        let fd = self.nearest(self.cur).and_then(|i| self.shown[i].as_ref());
        let zone: Vec<P> = self.o.shape.iter().map(|v| fd.map_or(*v, |f| f.map(&self.o.ref_pts, self.o.warp, *v))).collect();
        let g = Gray::from_rgb(&clip.rgb(self.cur).ok()?, clip.w, clip.h, Channel::Luma);
        let wide = track::edge_blur(&g, &zone).filter(|_| zone.len() >= 3)?;
        Some((12. * wide * (self.res[1] / clip.h as f64) as f32).round() / 10.)
    }

    fn import_tracking(&mut self, path: &std::path::Path) {
        let read = std::fs::read_to_string(path).ok().and_then(|t| export::read_tracking(&t));
        let (Some((width, rows)), Some(clip)) = (read, self.clip.clone()) else {
            self.status = t!("Fichier illisible : il faut des données « After Effects Keyframe Data » (position, ou les 4 coins Power Pin).", "Unreadable file: it must hold “After Effects Keyframe Data” (position, or the 4 Power Pin corners).").into();
            return;
        };
        self.checkpoint();
        let k = clip.w as f64 / width;
        let (to, back) = ([k, 0., 0., 0., k, 0., 0., 0., 1.], [1. / k, 0., 0., 0., 1. / k, 0., 0., 0., 1.]);
        let first = rows[0].0;
        if self.o.shape.len() < 3 {
            let c = track::apply(&track::mul(&to, &track::mul(&rows[0].1, &back)), [clip.w as f32 / 2., clip.h as f32 / 2.]);
            self.o.shape = vec![[c[0] - 60., c[1] - 40.], [c[0] + 60., c[1] - 40.], [c[0] + 60., c[1] + 40.], [c[0] - 60., c[1] + 40.]];
        }
        self.o.frames = vec![None; clip.n];
        (self.o.zone, self.o.warp, self.o.ref_pts, self.o.ref_frame) = (true, false, self.o.shape.clone(), self.range[0]);
        let mut done = 0;
        for (frame, h) in &rows {
            if let Some(slot) = self.o.frames.get_mut(self.range[0] + frame - first) {
                let h = track::mul(&to, &track::mul(h, &back));
                *slot = Some(FrameData { h, pts: self.o.shape.iter().map(|v| track::apply(&h, *v)).collect(), opacity: 1., guessed: false, score: 1. });
                done += 1;
            }
        }
        self.dirty = true;
        self.status = tf!("{done} frames de tracking importées à partir de la frame {}.", "{done} tracking frames imported, starting at frame {}.", self.range[0] + 1);
    }

    fn curves_window(&mut self, ctx: &egui::Context) {
        if !self.curves {
            return;
        }
        let mut open = true;
        egui::Window::new(t!("Courbes du tracking", "Tracking curves")).id(egui::Id::unique("courbes")).open(&mut open).resizable(false).show(ctx, |ui| {
            let centre = export::centroid(if self.o.shape.is_empty() { &self.o.ref_pts } else { &self.o.shape });
            let series = |frames: &[Option<FrameData>]| -> Vec<Option<[f64; 4]>> { frames.iter().map(|f| f.as_ref().map(|f| track::parts(&f.h, centre))).collect() };
            let (raw, soft) = (series(&self.o.frames), series(&self.shown));
            if soft.iter().all(Option::is_none) {
                ui.label(t!("Lancez d'abord un tracking.", "Run a tracking first."));
                return;
            }
            let n = soft.len().max(2);
            let show = |v: &[f64; 4], c: usize| [v[0], v[1], v[2] * 100., v[3].to_degrees()][c];
            for (c, name) in ["X (px)", "Y (px)", t!("Taille (%)", "Size (%)"), "Rotation (°)"].into_iter().enumerate() {
                let all: Vec<f64> = raw.iter().chain(&soft).flatten().map(|v| show(v, c)).collect();
                let (lo, hi) = all.iter().fold((f64::MAX, f64::MIN), |m, v| (m.0.min(*v), m.1.max(*v)));
                let room = ((1. - (hi - lo)) / 2.).max(0.);
                let (lo, hi) = (lo - room, hi + room);
                let now = soft.get(self.cur).copied().flatten().map_or(String::new(), |v| format!("{:.2}", show(&v, c)));
                ui.label(format!("{name}   {now}"));
                let (area, _) = ui.allocate_exact_size(Vec2::new(440., 54.), Sense::hover());
                let p = ui.painter_at(area);
                p.rect_filled(area, 2., ui.visuals().extreme_bg_color);
                let at = |i: usize, v: f64| Pos2::new(area.left() + area.width() * i as f32 / (n - 1) as f32, area.bottom() - 3. - (area.height() - 6.) * ((v - lo) / (hi - lo).max(1e-9)) as f32);
                for (rows, color) in [(&raw, Color32::from_gray(120)), (&soft, ACCENT)] {
                    let mut line = vec![];
                    for (i, v) in rows.iter().enumerate() {
                        match v {
                            Some(v) => line.push(at(i, show(v, c))),
                            None => drop(p.add(Shape::line(std::mem::take(&mut line), Stroke::new(1.5, color)))),
                        }
                    }
                    p.add(Shape::line(line, Stroke::new(1.5, color)));
                }
                let x = area.left() + area.width() * self.cur as f32 / (n - 1) as f32;
                p.line_segment([Pos2::new(x, area.top()), Pos2::new(x, area.bottom())], Stroke::new(1., Color32::from_gray(200)));
            }
            ui.separator();
            ui.strong(t!("Lissage par composante", "Smoothing per component"));
            for (i, name) in ["Position", t!("Taille", "Size"), "Rotation"].into_iter().enumerate() {
                self.dirty |= ui.add(egui::Slider::new(&mut self.o.ease[i], 0..=20).text(name)).changed();
            }
            ui.weak(t!("En gris : le tracking brut. En couleur : après lissage. Tout à 0 : rien n'est modifié.", "Grey: the raw tracking. Colour: after smoothing. All at 0: nothing is changed."));
            ui.separator();
            ui.strong(tf!("Retoucher la frame {}", "Retouch frame {}", self.cur + 1));
            if let Some(f) = self.o.frames.get(self.cur).cloned().flatten() {
                let was = track::parts(&f.h, centre);
                let mut v = [was[0], was[1], was[2] * 100., was[3].to_degrees()];
                let mut touched = false;
                ui.horizontal(|ui| {
                    for (i, (name, speed)) in [("X ", 0.1), ("Y ", 0.1), (t!("Taille ", "Size "), 0.05), ("Rotation ", 0.05)].into_iter().enumerate() {
                        let r = ui.add(egui::DragValue::new(&mut v[i]).speed(speed).prefix(name));
                        if r.drag_started() || r.gained_focus() {
                            self.checkpoint();
                        }
                        touched |= r.changed();
                    }
                });
                if touched {
                    let mut fixed = track::retouch(&f, centre, [v[0], v[1], (v[2] / 100.).max(0.01), v[3].to_radians()]);
                    fixed.guessed = false;
                    (self.o.frames[self.cur], self.dirty) = (Some(fixed), true);
                }
            } else {
                ui.weak(t!("Cette frame n'est pas suivie.", "This frame is not tracked."));
            }
        });
        self.curves &= open;
    }

    fn extrapolate(&mut self) {
        self.checkpoint();
        let sure: Vec<usize> = (0..self.o.frames.len()).filter(|&i| self.o.frames[i].as_ref().is_some_and(|f| !f.guessed)).collect();
        let corners = if self.o.zone { self.o.shape.clone() } else { self.o.ref_pts.clone() };
        let mut done = 0;
        if let (Some(&first), Some(&last), true) = (sure.first(), sure.last(), sure.len() >= 2) {
            let empty: Vec<usize> = (self.range[0]..=self.range[1]).filter(|&i| self.o.frames[i].is_none()).collect();
            for i in empty {
                let (near, far) = if i > last { (last, sure[sure.len().saturating_sub(6).min(sure.len() - 2)]) } else if i < first { (first, sure[5.min(sure.len() - 1)]) } else { continue };
                let (a, b) = (self.o.frames[far].as_ref().unwrap(), self.o.frames[near].as_ref().unwrap());
                self.o.frames[i] = Some(track::between(a, b, &corners, (i as f32 - far as f32) / (near as f32 - far as f32)));
                done += 1;
            }
        }
        self.dirty = true;
        self.status = tf!("{done} frames ajoutées en prolongeant le mouvement ; elles sont en orange.", "{done} frames added by extending the motion; they are shown in orange.");
    }

    fn knobs(&mut self) -> Vec<(&'static str, &mut dyn Knob)> {
        let App { o, merge, group, timestamps, lock_view, graph, matrix, loupe, cuts, loop_play, .. } = self;
        let Object { pin, zone, motion, mesh, mesh_size, search, pattern, channel, smoothing, realign, opacity, color, color_live, blur, bands, gradients, mask_follow, mask_opacity, style, style_name, text_follow, text_opacity, cut, grow, twin, ease: [ease_place, ease_size, ease_turn], axis, mask_layer, text_layer, cover, tolerance, .. } = o;
        let TextStyle { custom, font, size, bold, italic, color: ink, outline_color, outline, shadow_color, shadow, blur: soft } = style;
        vec![
            ("zone", zone),
            ("mouvement", motion),
            ("mesh", mesh),
            ("maille", mesh_size),
            ("recherche", search),
            ("motif", pattern),
            ("canal", channel),
            ("lissage", smoothing),
            ("recaler", realign),
            ("opacite", opacity),
            ("cache.couleur", color),
            ("cache.couleur_suivie", color_live),
            ("cache.flou", blur),
            ("cache.bandes", bands),
            ("cache.degrades", gradients),
            ("cache.suit", mask_follow),
            ("cache.image_4_points", pin),
            ("cache.opacite", mask_opacity),
            ("texte.personnalise", custom),
            ("texte.police", font),
            ("texte.taille", size),
            ("texte.gras", bold),
            ("texte.italique", italic),
            ("texte.couleur", ink),
            ("texte.couleur_contour", outline_color),
            ("texte.contour", outline),
            ("texte.couleur_ombre", shadow_color),
            ("texte.ombre", shadow),
            ("texte.flou", soft),
            ("texte.style", style_name),
            ("texte.suit", text_follow),
            ("texte.opacite", text_opacity),
            ("texte.decoupe", cut),
            ("fusionner", merge),
            ("regrouper", group),
            ("timestamps", timestamps),
            ("vue.verrouillee", lock_view),
            ("vue.courbe", graph),
            ("matrice", matrix),
            ("cache.marge", grow),
            ("texte.double", twin),
            ("lissage.position", ease_place),
            ("lissage.taille", ease_size),
            ("lissage.rotation", ease_turn),
            ("vue.loupe", loupe),
            ("plans", cuts),
            ("axe", axis),
            ("cache.calque", mask_layer),
            ("texte.calque", text_layer),
            ("vue.boucle", loop_play),
            ("recouvrement", cover),
            ("recouvrement.tolerance", tolerance),
        ]
    }

    fn project(&self) -> Option<PathBuf> {
        let name = match &self.job {
            Some(j) if !j.script.is_empty() => j.script.clone(),
            _ => self.source.as_ref()?.0.to_string_lossy().into_owned(),
        };
        let id = name.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
        Some(config()?.parent()?.join("projets").join(format!("{id:016x}.txt")))
    }

    fn remember(&mut self) {
        let Some(file) = self.project() else { return };
        let text: String = self.knobs().iter().map(|(k, v)| format!("{k}={}\n", v.put())).collect();
        let dir = file.parent().unwrap().to_path_buf();
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(file, text);
        if let Some(file) = self.session().filter(|_| self.all().any(|o| !o.shape.is_empty() || o.tracked())) {
            let text = self.keep();
            let _ = std::fs::write(file, text);
        }
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|d| d.as_secs() > 30 * 86400);
            if old && e.path().extension().is_some_and(|x| x == "session") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }

    fn session(&self) -> Option<PathBuf> {
        let file = self.project()?;
        Some(file.with_file_name(format!("{}-{}-{}.session", file.file_stem()?.to_str()?, self.times.first()?, self.o.frames.len())))
    }

    fn keep(&mut self) -> String {
        let was = self.sel;
        let mut text = format!("selection={was}\n");
        for i in 0..self.objects.len() {
            self.select(i);
            text += "objet=\n";
            text += &self.knobs().iter().map(|(k, v)| format!("{k}={}\n", v.put())).collect::<String>();
            let o = &self.o;
            let opa: Vec<String> = o.opa.iter().map(|(p, v)| format!("{},{},{v}", p[0], p[1])).collect();
            text += &format!("forme={}\ntrou={}\nmasque={}\npoints={}\nancre={}\npipette={}\nopa={}\n", dots(&o.shape), dots(&o.hole), dots(&o.mask), dots(&o.ref_pts), dots(o.anchor.as_slice()), dots(o.color_spot.as_slice()), opa.join(";"));
            text += &format!("warp={}\ncache.actif={}\ntexte.actif={}\ntexte.importe={}\nref={}\nmix={}\ntexte.saisi={}\n", o.warp, o.mask_on, o.text_on, o.import, o.ref_frame, o.mix.map(|v| v.to_string()).join(","), o.text);
            for (n, f) in o.frames.iter().enumerate() {
                if let Some(f) = f {
                    text += &format!("f={n} {} {} {} {} {}\n", f.h.map(|v| v.to_string()).join(" "), f.opacity, f.guessed, f.score, dots(&f.pts));
                }
            }
        }
        self.select(was);
        text
    }

    fn restore(&mut self, text: &str) {
        let asked = self.matrix;
        for (i, part) in text.split("objet=\n").skip(1).enumerate() {
            if i > 0 {
                self.add_object();
            }
            let line = |key: &str| part.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix('=')).unwrap_or("");
            for (key, knob) in self.knobs() {
                knob.get(line(key));
            }
            let o = &mut self.o;
            (o.shape, o.hole, o.mask, o.ref_pts) = (undots(line("forme")), undots(line("trou")), undots(line("masque")), undots(line("points")));
            (o.anchor, o.color_spot) = (undots(line("ancre")).first().copied(), undots(line("pipette")).first().copied());
            let numbers = |s: &str| -> Vec<f32> { s.split(',').filter_map(|v| v.parse().ok()).collect() };
            o.opa = line("opa").split(';').map(numbers).filter(|v| v.len() == 3).map(|v| ([v[0], v[1]], v[2])).collect();
            (o.warp, o.mask_on, o.text_on, o.import) = (line("warp") == "true", line("cache.actif") == "true", line("texte.actif") == "true", line("texte.importe") == "true");
            (o.ref_frame, o.text) = (line("ref").parse().unwrap_or(0), line("texte.saisi").into());
            o.mix = numbers(line("mix")).try_into().unwrap_or(o.mix);
            for row in part.lines().filter_map(|l| l.strip_prefix("f=")) {
                let v: Vec<&str> = row.split(' ').collect();
                let h: Vec<f64> = v.iter().skip(1).take(9).filter_map(|x| x.parse().ok()).collect();
                if let (14, Ok(h), Some(slot)) = (v.len(), h.try_into(), v[0].parse().ok().and_then(|k: usize| o.frames.get_mut(k))) {
                    *slot = Some(FrameData { h, pts: undots(v[13]), opacity: v[10].parse().unwrap_or(1.), guessed: v[11] == "true", score: v[12].parse().unwrap_or(1.) });
                }
            }
        }
        self.select(text.lines().next().and_then(|l| l.strip_prefix("selection=")?.parse().ok()).unwrap_or(0));
        (self.matrix, self.dirty, self.ask_import) = (asked, true, false);
        self.status = tf!("Session reprise : {} objet(s).", "Session resumed: {} object(s).", self.objects.len());
    }

    fn save_config(&self) {
        if let Some(file) = config() {
            let _ = std::fs::create_dir_all(file.parent().unwrap());
            let dir = self.ffmpeg_dir.as_ref().map_or(String::new(), |d| format!("ffmpeg={}\n", d.display()));
            let _ = std::fs::write(file, format!("{dir}langue={}\nmaj={}\n", if lang::english() { "en" } else { "fr" }, self.check as u8));
        }
    }

    fn resume_dialog(&mut self, ctx: &egui::Context) {
        let Some(text) = &self.resume else { return };
        let mut choice = None;
        egui::Window::new(t!("Session précédente", "Previous session")).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, [0., 0.]).show(ctx, |ui| {
            ui.set_max_width(460.);
            ui.label(tf!("Un travail enregistré existe pour cette vidéo et cette plage : {} objet(s).", "Saved work exists for this video and this range: {} object(s).", text.matches("objet=\n").count()));
            ui.label(t!("• Reprendre : zones, caches, tracking et réglages reviennent comme à la fermeture.", "• Resume: areas, masks, tracking and settings come back as they were on closing."));
            ui.label(t!("• Repartir de zéro : rien n'est repris ; l'ancien travail sera remplacé à la prochaine fermeture.", "• Start over: nothing is brought back; the old work is replaced the next time you close."));
            ui.weak(t!("L'image d'un cache n'est pas conservée : il faut la choisir à nouveau.", "A mask image is not kept: it has to be chosen again."));
            ui.horizontal(|ui| {
                if ui.button(t!("Reprendre", "Resume")).clicked() {
                    choice = Some(true);
                }
                if ui.button(t!("Repartir de zéro", "Start over")).clicked() {
                    choice = Some(false);
                }
            });
        });
        if let Some(go) = choice {
            let text = self.resume.take().unwrap();
            if go {
                self.restore(&text);
            }
        }
    }

    fn update_dialog(&mut self, ctx: &egui::Context) {
        if std::mem::take(&mut self.check_now) {
            let (tx, rx) = mpsc::channel();
            let wake = ctx.clone();
            std::thread::spawn(move || {
                let out = video::cmd("curl").args(["-sI", "--max-time", "8", concat!(env!("CARGO_PKG_REPOSITORY"), "/releases/latest")]).output();
                let head = out.map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
                let tag = head.lines().filter_map(|l| l.split_once(':')).find(|(k, _)| k.eq_ignore_ascii_case("location")).and_then(|(_, v)| v.trim().rsplit_once("/tag/"));
                if let Some((_, tag)) = tag {
                    let _ = tx.send(tag.to_string());
                    wake.request_repaint();
                }
            });
            self.update = Some(rx);
        }
        if let Some(tag) = self.update.as_ref().and_then(|r| r.try_recv().ok()) {
            self.update = None;
            self.news = Some(tag).filter(|t| newer(t, env!("AEGISIGNTRACK_BUILD")));
        }
        let Some(tag) = self.news.clone() else { return };
        let mut close = false;
        egui::Window::new(t!("Mise à jour disponible", "Update available")).collapsible(false).resizable(false).anchor(Align2::CENTER_TOP, [0., 40.]).show(ctx, |ui| {
            ui.label(tf!("Version installée : {}", "Installed version: {}", env!("AEGISIGNTRACK_BUILD")));
            ui.label(tf!("Nouvelle version : {tag}", "New version: {tag}"));
            ui.horizontal(|ui| {
                if ui.button(t!("Ouvrir la page de téléchargement", "Open the download page")).clicked() {
                    ctx.open_url(egui::OpenUrl::new_tab(format!("{}/releases/tag/{tag}", env!("CARGO_PKG_REPOSITORY"))));
                    close = true;
                }
                close |= ui.button(t!("Plus tard", "Later")).clicked();
            });
            if ui.checkbox(&mut self.check, t!("Vérifier les mises à jour au démarrage", "Check for updates at startup")).changed() {
                self.save_config();
            }
        });
        if close {
            self.news = None;
        }
    }

    fn recall(&mut self) {
        let Some(text) = self.project().and_then(|f| std::fs::read_to_string(f).ok()) else { return };
        let asked = self.matrix;
        for (key, knob) in self.knobs() {
            if let Some(v) = text.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix('=')) {
                knob.get(v);
            }
        }
        self.matrix = asked;
        self.status += t!(" Réglages de ce projet retrouvés.", " Settings of this project restored.");
    }

    fn interpolate(&mut self) {
        self.checkpoint();
        let sure = |f: &Option<FrameData>| f.as_ref().is_some_and(|f| !f.guessed);
        let corners = if self.o.zone { self.o.shape.clone() } else { self.o.ref_pts.clone() };
        let mut done = 0;
        for i in self.range[0]..=self.range[1] {
            if !self.o.frames[i].as_ref().is_some_and(|f| f.guessed) {
                continue;
            }
            let (lo, hi) = ((0..i).rev().find(|&k| sure(&self.o.frames[k])), (i + 1..self.o.frames.len()).find(|&k| sure(&self.o.frames[k])));
            if let (Some(lo), Some(hi)) = (lo, hi) {
                let (a, b) = (self.o.frames[lo].as_ref().unwrap(), self.o.frames[hi].as_ref().unwrap());
                self.o.frames[i] = Some(track::between(a, b, &corners, (i - lo) as f32 / (hi - lo) as f32));
                done += 1;
            }
        }
        self.dirty = true;
        self.status = tf!("{done} frames interpolées entre les frames sûres ; elles restent en orange.", "{done} frames interpolated between the reliable frames; they stay orange.");
    }

    fn exporter<'a>(&'a self, o: &'a Object, shown: &'a [Option<FrameData>], clip: &Clip, bare: bool) -> Export<'a> {
        Export {
            frames: shown,
            ref_pts: &o.ref_pts,
            warp: o.warp,
            times: &self.times,
            scale: [self.res[0] / clip.w as f64, self.res[1] / clip.h as f64],
            size: self.res,
            mask: o.mask_poly(),
            anchor: o.anchor_pt(),
            own_pos: bare,
            add_text: self.job.is_some() && !o.import,
            color: o.color,
            tints: match (o.color_spot, o.color_live) {
                (Some(spot), true) => shown.iter().enumerate().map(|(i, f)| clip.average(i, f.as_ref()?.map(&o.ref_pts, o.warp, spot))).collect(),
                _ => vec![],
            },
            gradients: &o.gradients,
            bands: o.bands,
            picture: o.picture.as_ref().map(|p| &p.0),
            pin: o.pin,
            blur: o.blur,
            mask_opacity: o.mask_opacity,
            text_opacity: o.text_opacity,
            mask_follow: o.mask_follow,
            text_follow: o.text_follow,
            merge: self.merge,
            group: self.group,
            cut: o.cut,
            grow: o.grow,
            twin: o.twin,
            mask_layer: o.mask_layer,
            text_layer: o.text_layer,
            covers: &o.covers,
        }
    }

    fn export_one(&self, o: &Object, shown: &[Option<FrameData>]) -> Result<(Vec<OutLine>, Vec<SrcLine>), String> {
        let clip = self.clip.as_ref().ok_or(t!("Aucune vidéo chargée.", "No video loaded."))?;
        let bare = !o.mask_on && !o.text_on;
        let typed = if bare { "" } else { &o.text };
        let mut lines = match &self.job {
            Some(j) if o.import => j.lines.clone(),
            Some(j) => vec![j.lines[0].with_text(typed)],
            None => vec![SrcLine::standalone(typed, &o.style_name)],
        };
        if !bare {
            lines.iter_mut().for_each(|l| l.text = o.style.apply(&l.text));
        }
        let e = self.exporter(o, shown, clip, bare);
        let mut out = vec![];
        if o.mask_on {
            if e.mask.len() < 3 {
                return Err(t!("Le cache a besoin d'au moins 3 points : dessinez-le dans l'onglet Cache.", "The mask needs at least 3 points: draw it in the Mask tab.").into());
            }
            out.extend(e.mask_lines(lines[0].index));
        }
        if o.text_on || bare {
            out.extend(e.text_lines(&lines));
        }
        if out.is_empty() { Err(t!("Aucune frame trackée ne tombe dans la durée des lignes.", "No tracked frame falls within the duration of the lines.").into()) } else { Ok((out, lines)) }
    }

    fn export(&self) -> Result<Vec<(Vec<OutLine>, Vec<SrcLine>)>, String> {
        let mut all = vec![];
        for (i, o) in self.all().enumerate().filter(|(_, o)| o.tracked()) {
            let smoothed;
            let shown = if i == self.sel && !self.dirty {
                &self.shown
            } else {
                smoothed = o.smoothed();
                &smoothed
            };
            all.push(self.export_one(o, shown)?);
        }
        if all.is_empty() { Err(t!("Rien à exporter : lancez d'abord le tracking.", "Nothing to export: run the tracking first.").into()) } else { Ok(all) }
    }

    fn diagnostic(&self) -> String {
        use std::fmt::Write;
        let mut t = format!("AegiSignTrack {}\n", env!("AEGISIGNTRACK_BUILD"));
        let cores = std::thread::available_parallelism().map_or(0, |n| n.get());
        writeln!(t, "système : {} {}, {cores} coeurs", std::env::consts::OS, std::env::consts::ARCH).unwrap();
        let ffmpeg = video::cmd("ffmpeg").arg("-version").output().map(|o| String::from_utf8_lossy(&o.stdout).lines().next().unwrap_or("").to_string());
        writeln!(t, "ffmpeg : {}", ffmpeg.unwrap_or_else(|e| e.to_string())).unwrap();
        if let Some((path, info, start, frames)) = &self.source {
            let (name, size) = (path.file_name().unwrap_or_default().to_string_lossy(), std::fs::metadata(path).map_or(0, |m| m.len()));
            writeln!(t, "vidéo : {name}, {size} octets, {info:?}, début {start:.4} s, {frames} frames demandées, depuis Aegisub : {}", self.job.is_some()).unwrap();
        }
        if let Some(clip) = &self.clip {
            let sum = |i: usize| clip.rgb(i).map_or(0, |d| d.iter().fold(0xcbf29ce484222325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x100000001b3)));
            writeln!(t, "plage chargée : {}x{}, {} frames, empreintes des images : première {:016x}, référence {:016x}, dernière {:016x}", clip.w, clip.h, clip.n, sum(0), sum(self.o.ref_frame.min(clip.n - 1)), sum(clip.n - 1)).unwrap();
        }
        writeln!(t, "réglages : zone {}, mesh {} ({}), {:?}, recherche {}, motif {}, canal {:?}, mélange {:?}, recaler {}, lissage {}, opacité {}, matrice {:?}, script {:?}", self.o.zone, self.o.mesh, self.o.mesh_size, self.o.motion, self.o.search, self.o.pattern, self.o.channel, self.o.mix, self.o.realign, self.o.smoothing, self.o.opacity, self.matrix, self.res).unwrap();
        writeln!(t, "sélection : {:?}\nplage : {:?}, frame de référence {}, {} points suivis", self.o.shape, self.range, self.o.ref_frame, self.o.ref_pts.len()).unwrap();
        writeln!(t, "résultat (frame, matrice 3x3, opacité, estimée) :").unwrap();
        for (i, f) in self.o.frames.iter().enumerate().filter_map(|(i, f)| Some((i, f.as_ref()?))) {
            writeln!(t, "{i} {} {:.3} {}", f.h.map(|v| format!("{v:.6}")).join(" "), f.opacity, f.guessed).unwrap();
        }
        t
    }

    fn deliver(&mut self, ctx: &egui::Context, send: bool) {
        self.remember();
        let done = self.export().and_then(|all| match &self.job {
            Some(j) if send => std::fs::write(&j.result, all.iter().map(|o| export::result_file(&o.0)).collect::<String>()).map(|_| None).map_err(|e| e.to_string()),
            _ => {
                let first = all.iter().flat_map(|o| &o.0).map(|l| l.start).min().unwrap_or(0);
                let t0 = if self.timestamps { 0 } else { first };
                ctx.copy_text(all.iter().map(|o| export::clipboard(&o.0, &o.1, t0)).collect::<String>());
                Ok(Some(all.iter().map(|o| o.0.len()).sum::<usize>()))
            }
        });
        match done {
            Ok(None) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Ok(Some(n)) => self.status = tf!("{n} lignes copiées dans le presse-papiers.", "{n} lines copied to the clipboard."),
            Err(e) => self.status = e,
        }
    }

    fn vectorise(&mut self, ctx: &egui::Context) {
        let Some((rgba, w, h)) = &self.o.image else { return };
        let pic = picture::trace(rgba, *w, *h, self.o.image_colors);
        let tex = ctx.load_texture("picture", egui::ColorImage::from_rgba_unmultiplied([pic.w, pic.h], &pic.rgba), egui::TextureOptions::NEAREST);
        let points: usize = pic.layers.iter().flat_map(|l| &l.1).map(Vec::len).sum();
        self.status = tf!("Image vectorisée : {} couleurs, {points} points par frame.", "Image vectorised: {} colours, {points} points per frame.", pic.layers.len());
        (self.o.picture, self.o.mask_on) = (Some((pic, tex)), true);
    }

    fn image_dialog(&mut self, ctx: &egui::Context) {
        let Some((kind, points)) = &self.image_ask else { return };
        let drawn = matches!(kind, Loaded::Vector(_));
        let frames = self.range[1] - self.range[0] + 1;
        let size = *points as f64 * 11. * frames as f64 / 1e6;
        let mut choice = None;
        egui::Window::new(t!("Image à la place du cache", "Image in place of the mask")).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, [0., 0.]).show(ctx, |ui| {
            ui.set_max_width(460.);
            ui.label(if drawn { t!("Ce SVG est déjà vectoriel : chaque forme devient une ligne ASS par frame, sans perte de netteté. Le texte et les dégradés du SVG ne sont pas repris.", "This SVG is already vector art: each shape becomes one ASS line per frame, with no loss of sharpness. Text and gradients inside the SVG are not kept.") } else { t!("L'image va être vectorisée : réduite à 200 px et à quelques couleurs, puis convertie en formes ASS, une ligne par couleur et par frame.", "The image will be vectorised: reduced to 200 px and a few colours, then converted to ASS shapes, one line per colour and per frame.") });
            ui.label(tf!("Le script peut très vite devenir très lourd dans Aegisub. Pour cette image : {points} points par frame, soit environ {size:.1} Mo pour les {frames} frames de la plage.", "The script can quickly become very heavy in Aegisub. For this image: {points} points per frame, about {size:.1} MB for the {frames} frames of the range."));
            ui.label(t!("Au moment de copier ou d'envoyer, AegiSignTrack peut ne plus répondre pendant plusieurs secondes, et Aegisub peut ensuite rester bloqué bien plus longtemps, voire plusieurs minutes, si l'image est grande ou détaillée, si elle a beaucoup de couleurs ou si le tracking est long.", "When copying or sending, AegiSignTrack may stop responding for several seconds, and Aegisub may then stay frozen much longer, even several minutes, if the image is large or detailed, has many colours, or if the tracking is long."));
            ui.horizontal(|ui| {
                if ui.button(t!("Utiliser l'image", "Use the image")).clicked() {
                    choice = Some(true);
                }
                if ui.button(t!("Annuler", "Cancel")).clicked() {
                    choice = Some(false);
                }
            });
        });
        match (choice, self.image_ask.take_if(|_| choice.is_some())) {
            (Some(true), Some((Loaded::Raster(img), _))) => {
                self.o.image = Some(img);
                self.vectorise(ctx);
            }
            (Some(true), Some((Loaded::Vector(pic), points))) => {
                let tex = ctx.load_texture("picture", egui::ColorImage::from_rgba_unmultiplied([pic.w, pic.h], &pic.rgba), Default::default());
                self.status = tf!("SVG chargé : {} formes, {points} points par frame.", "SVG loaded: {} shapes, {points} points per frame.", pic.layers.len());
                (self.o.image, self.o.picture, self.o.mask_on) = (None, Some((pic, tex)), true);
            }
            _ => {}
        }
    }

    fn import_dialog(&mut self, ctx: &egui::Context) {
        let Some(j) = self.job.as_ref().filter(|_| self.ask_import && self.resume.is_none()) else { return };
        let mut choice = None;
        egui::Window::new(t!("Texte des lignes sélectionnées", "Text of the selected lines")).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, [0., 0.]).show(ctx, |ui| {
            ui.set_max_width(460.);
            ui.label(tf!("{} ligne(s) sélectionnée(s) dans Aegisub :", "{} line(s) selected in Aegisub:", j.lines.len()));
            for l in j.lines.iter().take(8) {
                ui.weak(format!("• {}", export::strip_tags(&l.text)));
            }
            if j.lines.len() > 8 {
                ui.weak(tf!("... et {} autres", "... and {} more", j.lines.len() - 8));
            }
            ui.label(t!("Importer ce texte ?", "Import this text?"));
            ui.label(t!("• Importer : chaque ligne garde son texte et sera remplacée par ses versions trackées.", "• Import: each line keeps its text and is replaced by its tracked versions."));
            ui.label(t!("• Ne pas importer : les lignes ne servent qu'à délimiter la vidéo ; le texte éventuel se saisit dans l'onglet Texte et s'ajoute à côté.", "• Do not import: the lines only mark out the video; any text is typed in the Text tab and added next to them."));
            ui.weak(t!("Ce choix reste modifiable dans l'onglet Texte.", "This choice can still be changed in the Text tab."));
            ui.horizontal(|ui| {
                if ui.button(t!("Importer le texte", "Import the text")).clicked() {
                    choice = Some(true);
                }
                if ui.button(t!("Ne pas importer", "Do not import")).clicked() {
                    choice = Some(false);
                }
            });
        });
        if let Some(go) = choice {
            (self.o.import, self.ask_import) = (go, false);
        }
    }

    fn confirm_dialog(&mut self, ctx: &egui::Context) {
        let Some(send) = self.confirm else { return };
        let mut choice = None;
        egui::Window::new(t!("Tracking seul", "Tracking only")).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, [0., 0.]).show(ctx, |ui| {
            ui.label(t!("Ni le cache ni le texte ne sont cochés.", "Neither the mask nor the text is ticked."));
            ui.label(tf!("Seules les informations du tracking (position, taille, rotation) seront {}, rien d'autre.", "Only the tracking data (position, size, rotation) will be {}, nothing else.", if send { t!("envoyées à Aegisub", "sent to Aegisub") } else { t!("copiées", "copied") }));
            ui.horizontal(|ui| {
                if ui.button(t!("Continuer", "Continue")).clicked() {
                    choice = Some(true);
                }
                if ui.button(t!("Annuler", "Cancel")).clicked() {
                    choice = Some(false);
                }
            });
        });
        if let Some(go) = choice {
            self.confirm = None;
            if go {
                self.deliver(ctx, send);
            }
        }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        if self.job.is_none() {
            if let Some(p) = ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf())) {
                self.open(p);
            }
        }
        if let Some((l, info, start, keep)) = &mut self.loading {
            let (info, start, keep) = (*info, *start, *keep);
            match l.poll() {
                None => ctx.request_repaint_after(std::time::Duration::from_millis(100)),
                Some(r) => {
                    self.loading = None;
                    match r {
                        Ok(clip) => self.loaded(clip, info, start, keep),
                        Err(e) => self.status = e,
                    }
                }
            }
        }
        let mut done = None;
        let what = self.worker.as_ref().map_or("", |w| w.what);
        if let Some(w) = &self.worker {
            for msg in w.rx.try_iter() {
                match msg {
                    Msg::Frame(i, f) => {
                        self.o.frames[i] = Some(f);
                        (self.cur, self.dirty) = (i, true);
                    }
                    Msg::Done(err) => done = Some(err),
                }
            }
        }
        if let Some(err) = done {
            self.worker = None;
            let n = self.o.frames.iter().flatten().count();
            let guessed = self.o.frames.iter().flatten().filter(|f| f.guessed).count();
            self.status = match err {
                Some(e) => tf!("{what} arrêté — {e}", "{what} stopped — {e}"),
                None if guessed * 5 > n && !self.o.motion.shear && !self.o.motion.perspective => tf!("{what} terminé : {n} frames suivies, dont {guessed} estimées (en orange). Si l'objet s'écrase, s'étire ou est vu de biais, cochez Cisaillement ou Perspective et relancez.", "{what} finished: {n} frames tracked, {guessed} of them estimated (in orange). If the object squashes, stretches or is seen at an angle, tick Shear or Perspective and run again."),
                None if guessed > 0 => tf!("{what} terminé : {n} frames suivies, dont {guessed} estimées (objet masqué ou perdu, en orange sur la timeline).", "{what} finished: {n} frames tracked, {guessed} of them estimated (object hidden or lost, in orange on the timeline)."),
                None => tf!("{what} terminé : {n} frames suivies.", "{what} finished: {n} frames tracked."),
            };
        }
        let n = self.clip.as_ref().map_or(0, |c| c.n);
        if n > 0 && self.worker.is_none() && !ctx.egui_wants_keyboard_input() {
            let (redo, undo) = ctx.input_mut(|i| (i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::Z), i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z)));
            if redo || undo {
                self.time_travel(undo);
            }
            let (left, right, space) = ctx.input(|i| (i.key_pressed(egui::Key::ArrowLeft), i.key_pressed(egui::Key::ArrowRight), i.key_pressed(egui::Key::Space)));
            if ctx.input(|i| i.key_pressed(egui::Key::H)) {
                (self.hide, self.drag) = (!self.hide, None);
            }
            if left {
                self.cur = self.cur.saturating_sub(1);
            }
            if right {
                self.cur = (self.cur + 1).min(n - 1);
            }
            if space {
                self.playing = !self.playing;
            }
        }
        if self.playing {
            let now = ctx.input(|i| i.time);
            if now >= self.next_tick {
                self.next_tick = now + 1. / self.fps;
                if self.loop_play && self.cur >= self.range[1] {
                    self.cur = self.range[0];
                } else if self.cur + 1 < n {
                    self.cur += 1;
                } else {
                    self.playing = false;
                }
            }
            ctx.request_repaint();
        }
        if self.dirty {
            self.shown = self.o.smoothed();
            self.watch();
            self.dirty = false;
        }
    }

    fn watch(&mut self) {
        use std::hash::{Hash, Hasher};
        let mark = |parts: &[&[f32]], more: [i64; 2]| {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            parts.iter().for_each(|p| p.iter().for_each(|v| v.to_bits().hash(&mut h)));
            more.hash(&mut h);
            h.finish()
        };
        let o = &mut self.o;
        let Some(clip) = self.clip.clone().filter(|_| o.cover > 0 && o.zone && o.shape.len() >= 3) else {
            o.covers.clear();
            return;
        };
        let key = mark(&[o.shape.as_flattened(), o.hole.as_flattened()], [o.ref_frame as i64, o.tolerance as i64]);
        if o.grid.as_ref().map(|g| g.0) != Some(key) {
            let Ok(rgb) = clip.rgb(o.ref_frame) else { return };
            let cells = cover::cells(&o.shape, &o.hole);
            let colours = cover::colours(&rgb, clip.w, clip.h, &cells);
            (o.grid, o.seen, o.fits) = (Some((key, cells, colours)), vec![], vec![]);
        }
        o.seen.resize(o.frames.len(), None);
        o.fits.resize(o.frames.len(), None);
        let (_, cells, colours) = o.grid.as_ref().unwrap();
        for (i, f) in o.frames.iter().enumerate() {
            let Some(f) = f else {
                o.seen[i] = None;
                continue;
            };
            let at = mark(&[f.pts.as_flattened()], [f.h.iter().fold(0u64, |a, v| a.rotate_left(7) ^ v.to_bits()) as i64, 0]);
            if o.seen[i].as_ref().map(|s| s.0) != Some(at) {
                if let Ok(rgb) = clip.rgb(i) {
                    o.seen[i] = Some((at, cover::seen(&rgb, [clip.w, clip.h], cells, colours, &|p| f.map(&o.ref_pts, o.warp, p), o.tolerance)));
                }
            }
        }
        o.covers = cover::covers(cells, &o.seen, o.ref_frame, o.cover == 2, &mut o.fits);
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if self.job.is_none() && ui.button(t!("Ouvrir une vidéo...", "Open a video...")).clicked() {
                if let Some(p) = rfd::FileDialog::new().set_title(t!("Ouvrir une vidéo", "Open a video")).pick_file() {
                    self.open(p);
                }
            }
            if let Some(j) = &self.job {
                ui.strong(tf!("Aegisub · {} ligne(s)", "Aegisub · {} line(s)", j.lines.len()));
            }
            if let Some((l, ..)) = &self.loading {
                ui.add(egui::ProgressBar::new(l.progress()).desired_width(160.).show_percentage());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut ask = None;
                if self.job.is_some() && ui.button(t!("Envoyer à Aegisub", "Send to Aegisub")).on_hover_text(t!("Insère les lignes dans le script et ferme le tracker", "Inserts the lines into the script and closes the tracker")).clicked() {
                    ask = Some(true);
                }
                if ui.button(t!("Copier pour Aegisub", "Copy for Aegisub")).on_hover_text(t!("Copie les lignes : collez-les (Ctrl+V) dans la grille d'Aegisub", "Copies the lines: paste them (Ctrl+V) into the Aegisub grid")).clicked() {
                    ask = Some(false);
                }
                match ask {
                    Some(send) if self.all().any(|o| o.tracked() && !o.mask_on && !o.text_on) => self.confirm = Some(send),
                    Some(send) => self.deliver(ui.ctx(), send),
                    None => {}
                }
                ui.menu_button(t!("Avancé", "Advanced"), |ui| {
                    ui.checkbox(&mut self.curves, t!("Courbes du tracking", "Tracking curves")).on_hover_text(t!("Ouvre une fenêtre qui montre la position, la taille et la rotation dans le temps,\npermet de lisser une seule de ces composantes et de retoucher la frame affichée.", "Opens a window showing position, size and rotation over time,\nwhere one component can be smoothed alone and the displayed frame retouched."));
                    if ui.add_enabled(self.clip.is_some(), egui::Button::new(t!("Importer des données de tracking...", "Import tracking data..."))).on_hover_text(t!("Lit un fichier « After Effects Keyframe Data » (Mocha, After Effects, Blender : position, taille, rotation ou Power Pin)\net l'applique à l'objet affiché, à partir du début de la plage. Cache, dégradés et texte s'utilisent ensuite comme d'habitude.", "Reads an “After Effects Keyframe Data” file (Mocha, After Effects, Blender: position, scale, rotation or Power Pin)\nand applies it to the displayed object, from the start of the range. Mask, gradients and text then work as usual.")).clicked() {
                        if let Some(p) = rfd::FileDialog::new().set_title(t!("Données de tracking", "Tracking data")).pick_file() {
                            self.import_tracking(&p);
                        }
                    }
                    let open = self.tracked() && (self.range[0]..=self.range[1]).any(|i| self.o.frames[i].is_none());
                    if ui.add_enabled(open, egui::Button::new(t!("Extrapoler", "Extrapolate"))).on_hover_text(t!("Remplit les frames non suivies de la plage, avant et après le tracking, en prolongeant son mouvement.\nPour un sign qui sort de l'image ou se fond : régler la plage sur les frames voulues, puis cliquer.", "Fills the untracked frames of the range, before and after the tracking, by extending its motion.\nFor a sign that leaves the picture or fades: set the range to the wanted frames, then click.")).clicked() {
                        self.extrapolate();
                    }
                });
                ui.menu_button("Options", |ui| {
                    if ui.add_enabled(self.tracked(), egui::Button::new(t!("Enregistrer en .ass...", "Save as .ass..."))).on_hover_text(t!("Écrit un fichier .ass complet (en-tête, styles et lignes exportées), à ouvrir ou à importer dans Aegisub.", "Writes a complete .ass file (header, styles and exported lines), to open or import in Aegisub.")).clicked() {
                        if let Some(p) = rfd::FileDialog::new().set_title(t!("Enregistrer les lignes en .ass", "Save the lines as .ass")).set_file_name("aegisigntrack.ass").save_file() {
                            let done = self.export().and_then(|all| std::fs::write(&p, self.ass_head() + &all.iter().map(|o| export::clipboard(&o.0, &o.1, 0)).collect::<String>()).map_err(|e| e.to_string()));
                            self.status = done.map_or_else(|e| e, |_| tf!("Fichier enregistré : {}", "File saved: {}", p.display()));
                        }
                    }
                    if ui.add_enabled(self.tracked(), egui::Button::new(t!("Données Power Pin", "Power Pin data"))).on_hover_text(t!("Copie les 4 coins du cache (ou du rectangle autour de lui) frame par frame, au format « CC Power Pin »\nque lisent les scripts de perspective pour Aegisub.", "Copies the 4 corners of the mask (or of the rectangle around it) frame by frame, in the “CC Power Pin” format\nread by perspective scripts for Aegisub.")).clicked() {
                        if let Some(clip) = self.clip.clone() {
                            ui.ctx().copy_text(self.exporter(&self.o, &self.shown, &clip, false).power_pin(self.fps));
                            self.status = t!("Données Power Pin copiées dans le presse-papiers.", "Power Pin data copied to the clipboard.").into();
                        }
                    }
                    ui.menu_button("Langue / Language", |ui| {
                        for (english, name) in [(false, "Français"), (true, "English")] {
                            if ui.radio(lang::english() == english, name).clicked() {
                                lang::set(english);
                                self.save_config();
                            }
                        }
                    });
                    if ui.checkbox(&mut self.check, t!("Vérifier les mises à jour au démarrage", "Check for updates at startup")).on_hover_text(t!("Au lancement, regarde sur GitHub si une release plus récente existe et le signale.\nRien n'est téléchargé ni installé tout seul.", "At launch, looks on GitHub for a more recent release and reports it.\nNothing is downloaded or installed automatically.")).changed() {
                        self.save_config();
                    }
                    if self.job.as_ref().is_some_and(|j| !j.keyframes.is_empty()) {
                        ui.checkbox(&mut self.cuts, t!("S'arrêter aux changements de plan", "Stop at scene changes")).on_hover_text(t!("Le tracking ne dépasse pas les keyframes d'Aegisub (traits blancs sur la timeline).", "Tracking does not go past the Aegisub keyframes (white marks on the timeline)."));
                    }
                    if ui.button("Diagnostic...").on_hover_text(t!("Enregistre un fichier texte décrivant la version, ffmpeg, la vidéo, les réglages et le résultat du tracking.\nÀ comparer entre deux machines quand le tracking ne donne pas la même chose.", "Saves a text file describing the version, ffmpeg, the video, the settings and the tracking result.\nCompare it between two machines when tracking does not give the same result.")).clicked() {
                        if let Some(p) = rfd::FileDialog::new().set_title(t!("Enregistrer le diagnostic", "Save the diagnostic")).set_file_name("aegisigntrack-diagnostic.txt").save_file() {
                            self.status = match std::fs::write(&p, self.diagnostic()) {
                                Ok(()) => tf!("Diagnostic enregistré : {}", "Diagnostic saved: {}", p.display()),
                                Err(e) => e.to_string(),
                            };
                        }
                    }
                    if ui.add_enabled(self.tracked(), egui::Button::new(t!("Données de tracking", "Tracking data"))).on_hover_text(t!("Copie le tracking de l'objet affiché au format « After Effects Keyframe Data » (position, taille, rotation),\ncelui que lisent Aegisub-Motion et les autres outils.", "Copies the tracking of the displayed object as “After Effects Keyframe Data” (position, scale, rotation),\nthe format read by Aegisub-Motion and other tools.")).clicked() {
                        if let Some(clip) = self.clip.clone() {
                            ui.ctx().copy_text(self.exporter(&self.o, &self.shown, &clip, false).keyframes(self.fps));
                            self.status = t!("Données de tracking copiées dans le presse-papiers.", "Tracking data copied to the clipboard.").into();
                        }
                    }
                    ui.checkbox(&mut self.group, t!("Regrouper les mouvements réguliers", "Group steady motion")).on_hover_text(t!("Texte : quand le mouvement est régulier sur plusieurs frames, une seule ligne avec \\move et \\t au lieu d'une ligne par frame.\nSans effet sur le cache.", "Text: when the motion is steady over several frames, a single line with \\move and \\t instead of one line per frame.\nNo effect on the mask."));
                    ui.checkbox(&mut self.timestamps, t!("Avec les timestamps", "With timestamps")).on_hover_text(t!("Décoché : les lignes copiées démarrent à 0:00:00.00", "Unticked: copied lines start at 0:00:00.00"));
                    ui.checkbox(&mut self.merge, t!("Fusionner les frames immobiles", "Merge still frames")).on_hover_text(t!("Tant que le cache ou le texte ne bouge pas, une seule ligne couvre toute la durée.\nDécoché : une ligne par frame.", "While the mask or the text does not move, a single line covers the whole duration.\nUnticked: one line per frame."));
                });
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| ui.add(egui::Label::new(&self.status).truncate()));
            });
        });
    }

    fn settings(&mut self, ui: &mut egui::Ui) {
        ui.add_enabled_ui(self.worker.is_none(), |ui| {
            ui.horizontal_wrapped(|ui| {
                for i in 0..self.objects.len() {
                    if ui.selectable_label(i == self.sel, tf!("Objet {}", "Object {}", i + 1)).on_hover_text(t!("Chaque objet a son propre tracking, son cache et son texte. Tous sont exportés ensemble.", "Each object has its own tracking, mask and text. All are exported together.")).clicked() {
                        self.select(i);
                    }
                }
                if ui.button("+").on_hover_text(t!("Ajouter un objet à suivre", "Add an object to track")).clicked() {
                    self.add_object();
                }
                if self.objects.len() > 1 && ui.button("✕").on_hover_text(t!("Supprimer l'objet affiché", "Remove the displayed object")).clicked() {
                    self.remove_object();
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.mode, Mode::Track, "Tracking").on_hover_text(t!("Choisir ce qui est suivi dans la vidéo", "Choose what is tracked in the video"));
                ui.selectable_value(&mut self.mode, Mode::Mask, t!("Cache", "Mask")).on_hover_text(t!("Placer un cache qui suit le tracking", "Place a mask that follows the tracking"));
                ui.selectable_value(&mut self.mode, Mode::Text, t!("Texte", "Text")).on_hover_text(t!("Placer un texte qui suit le tracking", "Place a text that follows the tracking"));
            });
            ui.separator();
            match self.mode {
                Mode::Track => self.track_tab(ui),
                Mode::Mask => self.mask_tab(ui),
                Mode::Text => self.text_tab(ui),
            }
            ui.separator();
            let name = |m| match m {
                Matrix::Auto => "Auto",
                Matrix::Bt709 => "BT.709 (HD)",
                Matrix::Bt601 => "BT.601 (SD)",
            };
            let before = self.matrix;
            egui::ComboBox::from_label(t!("Matrice couleur", "Colour matrix")).selected_text(name(self.matrix)).show_ui(ui, |ui| {
                for m in [Matrix::Auto, Matrix::Bt709, Matrix::Bt601] {
                    ui.selectable_value(&mut self.matrix, m, name(m));
                }
            })
            .response
            .on_hover_text(t!("Conversion des couleurs de la vidéo (affichage et pipette).\nAuto : celle déclarée dans le fichier, sinon BT.709 en HD et BT.601 en dessous.", "Colour conversion of the video (display and eyedropper).\nAuto: the one declared in the file, otherwise BT.709 for HD and BT.601 below."));
            if self.matrix != before && self.loading.is_none() {
                self.reload(true);
            }
            if ui.button(t!("Dossier de ffmpeg...", "ffmpeg folder...")).on_hover_text(tf!("Dossier qui contient ffmpeg et ffprobe, s'ils ne sont ni dans le PATH ni à côté d'AegiSignTrack.\nActuel : {}", "Folder holding ffmpeg and ffprobe, when they are neither in the PATH nor next to AegiSignTrack.\nCurrent: {}", self.ffmpeg_dir.as_ref().map_or(t!("automatique", "automatic").into(), |d| d.display().to_string()))).clicked() {
                if let Some(dir) = rfd::FileDialog::new().set_title(t!("Dossier de ffmpeg et ffprobe", "Folder of ffmpeg and ffprobe")).pick_folder() {
                    video::set_dir(Some(dir.clone()));
                    self.status = video::missing().unwrap_or_else(|| t!("ffmpeg et ffprobe trouvés.", "ffmpeg and ffprobe found.").into());
                    (self.ffmpeg_dir, self.no_preview) = (Some(dir), false);
                    self.save_config();
                }
            }
            if self.job.is_none() {
                ui.horizontal(|ui| {
                    ui.label("Script");
                    ui.add(egui::DragValue::new(&mut self.res[0]).range(1.0..=16384.0));
                    ui.label("×");
                    ui.add(egui::DragValue::new(&mut self.res[1]).range(1.0..=16384.0));
                })
                .response
                .on_hover_text(t!("Résolution du script Aegisub (PlayResX × PlayResY)", "Resolution of the Aegisub script (PlayResX × PlayResY)"));
            }
        });
    }

    fn track_tab(&mut self, ui: &mut egui::Ui) {
        let tracked = self.tracked();
        ui.strong(t!("Sélection", "Selection"));
        ui.add_enabled_ui(!tracked, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.o.zone, false, "Points").on_hover_text(t!("1 point ou plus : chaque point est suivi individuellement", "1 point or more: each point is tracked on its own"));
                ui.selectable_value(&mut self.o.zone, true, t!("Zone", "Area")).on_hover_text(t!("Polygone (4 points ou plus) : toute la surface est suivie", "Polygon (4 points or more): the whole surface is tracked"));
            });
        });
        ui.weak(if tracked { t!("Glisser : corriger à la main la frame affichée (tracking manuel).", "Drag: correct the displayed frame by hand (manual tracking).") } else { t!("Clic : ajouter un point. Glisser : déplacer. Clic droit : supprimer. Ctrl+Z : annuler.", "Click: add a point. Drag: move. Right click: delete. Ctrl+Z: undo.") });
        if ui.button(t!("Effacer la sélection", "Clear the selection")).clicked() {
            self.checkpoint();
            self.o.shape.clear();
            self.clear_track();
        }
        if self.o.zone {
            ui.horizontal(|ui| {
                ui.add_enabled(!tracked, egui::Button::new(t!("Zone à ignorer", "Area to ignore")).selected(self.holing)).on_hover_text(t!("Puis cliquer sur la vidéo pour entourer une partie de la zone à ne pas suivre : un élément qui clignote, s'anime ou n'appartient pas à l'objet.\nÀ dessiner avant de lancer le tracking.", "Then click on the video to outline a part of the area that must not be tracked: something that blinks, is animated or does not belong to the object.\nDraw it before starting the tracking.")).clicked().then(|| self.holing = !self.holing);
                if !self.o.hole.is_empty() && ui.add_enabled(!tracked, egui::Button::new(t!("Effacer", "Clear"))).clicked() {
                    self.checkpoint();
                    self.o.hole.clear();
                }
            });
        }
        self.holing &= self.o.zone && !tracked;

        ui.separator();
        ui.strong(t!("Mouvement suivi", "Tracked motion"));
        let m = &mut self.o.motion;
        ui.checkbox(&mut m.translation, "Translation");
        ui.horizontal(|ui| {
            ui.label(t!("Axe", "Axis"));
            for (v, name) in [(0, t!("Libre", "Free")), (1, "Horizontal"), (2, "Vertical")] {
                ui.selectable_value(&mut self.o.axis, v, name).on_hover_text(t!("Horizontal ou Vertical : l'objet ne peut bouger que dans ce sens, le centre de l'objet reste figé sur l'autre axe, sans aucun tremblement.\nPour un mouvement purement droit, décocher aussi Échelle et Rotation.\nÀ n'utiliser que si le mouvement est vraiment droit (défilement, panoramique) : si l'objet bouge aussi dans l'autre sens, le tracking sera faux.\nÀ choisir avant de lancer le tracking.", "Horizontal or Vertical: the object can only move that way, the centre of the object stays frozen on the other axis, with no jitter at all.\nFor a purely straight motion, also untick Scale and Rotation.\nOnly for a truly straight motion (scroll, pan): if the object also moves the other way, the tracking will be wrong.\nChoose it before starting the tracking."));
            }
        });
        if m.shear || m.perspective {
            for name in [t!("Échelle", "Scale"), "Rotation"] {
                ui.add_enabled(false, egui::Checkbox::new(&mut true, name)).on_disabled_hover_text(t!("Toujours suivie avec le cisaillement ou la perspective, qui la contiennent.", "Always tracked with shear or perspective, which include it."));
            }
        } else {
            ui.checkbox(&mut m.scale, t!("Échelle", "Scale"));
            ui.checkbox(&mut m.rotation, "Rotation");
        }
        ui.checkbox(&mut m.shear, t!("Cisaillement", "Shear")).on_hover_text(t!("Inclut échelle et rotation. 3 points minimum.", "Includes scale and rotation. 3 points minimum."));
        ui.checkbox(&mut m.perspective, "Perspective").on_hover_text(t!("Inclut tout le reste. 4 points minimum.", "Includes everything else. 4 points minimum."));
        ui.add_enabled_ui(!tracked && self.o.zone, |ui| {
            ui.checkbox(&mut self.o.mesh, "Mesh").on_hover_text(t!("Grille de points suivis un par un : la sélection se déforme avec la surface", "Grid of points tracked one by one: the selection bends with the surface"));
            if self.o.mesh {
                ui.add(egui::Slider::new(&mut self.o.mesh_size, 10.0..=200.0).integer().text(t!("Maille (px)", "Cell (px)")));
            }
        });

        ui.separator();
        ui.strong(t!("Autres suivis", "Other tracking"));
        let fades = ui.checkbox(&mut self.o.opacity, t!("Opacité", "Opacity")).on_hover_text(t!("Fondus : garde la sélection en place quand elle devient invisible, et coche « Opacité (fondus) » pour le cache et le texte (\\alpha frame par frame).\nÀ cocher avant de lancer le tracking, en partant d'une frame où la zone est bien visible.", "Fades: keeps the selection in place when it becomes invisible, and ticks “Opacity (fades)” for the mask and the text (\\alpha frame by frame).\nTick it before starting the tracking, from a frame where the area is clearly visible."));
        if fades.changed() {
            (self.o.mask_opacity, self.o.text_opacity) = (self.o.opacity, self.o.opacity);
        }
        ui.horizontal(|ui| {
            ui.label(t!("Recouvrement", "Covering"));
            for (v, name) in [(0, t!("Ignorer", "Ignore")), (1, t!("Passage", "Passing")), (2, t!("Définitif", "Lasting"))] {
                self.dirty |= ui.selectable_value(&mut self.o.cover, v, name).on_hover_text(t!("Quand quelque chose passe devant l'objet suivi, la partie recouverte du cache et du texte est coupée le long d'un bord droit, et l'objet n'est plus exporté tant qu'il est entièrement recouvert (frames grises sur la timeline).\nPassage : ce qui est recouvert peut revenir (un personnage qui passe devant).\nDéfinitif : ce qui a été recouvert le reste, même si quelque chose de ressemblant apparaît au même endroit (page qui se tourne, volet, objet qui sort derrière un décor).\nCompare les couleurs de la zone avec celles de la frame de départ : mode Zone seulement, et à éviter avec un fondu ou un éclairage qui change.", "When something passes in front of the tracked object, the covered part of the mask and of the text is cut along a straight edge, and the object is no longer exported while it is entirely covered (grey frames on the timeline).\nPassing: what is covered can come back (a character walking in front).\nLasting: what has been covered stays covered, even if something similar shows up at the same place (a page being turned, a shutter, an object leaving behind the scenery).\nCompares the colours of the area with those of the start frame: Area mode only, and to avoid with a fade or changing light.")).changed();
            }
        });
        if self.o.cover > 0 {
            self.dirty |= ui.add(egui::DragValue::new(&mut self.o.tolerance).range(2..=80).prefix(t!("Tolérance ", "Tolerance "))).on_hover_text(t!("Écart de couleur accepté avant de considérer un endroit comme recouvert.\nPlus bas : détecte un recouvrement de couleur proche. Plus haut : résiste mieux au bruit de compression.", "Colour difference accepted before a spot is considered covered.\nLower: detects a cover of a close colour. Higher: copes better with compression noise.")).changed();
        }

        ui.separator();
        ui.strong(t!("Recherche", "Search"));
        ui.horizontal(|ui| {
            for (name, v) in [(t!("Petit mouvement", "Small motion"), 8.), ("Normal", 24.), (t!("Grand", "Large"), 96.)] {
                if ui.selectable_label(self.o.search == v, name).clicked() {
                    self.o.search = v;
                }
            }
        });
        ui.add(egui::Slider::new(&mut self.o.search, 2.0..=300.0).integer().logarithmic(true).text(t!("Zone de recherche (px)", "Search area (px)"))).on_hover_text(t!("Déplacement maximal d'un point entre deux frames", "Largest move of a point between two frames"));
        ui.add(egui::Slider::new(&mut self.o.pattern, 3..=40).text(t!("Motif (px)", "Pattern (px)"))).on_hover_text(t!("Demi-taille du motif suivi autour de chaque point", "Half-size of the pattern tracked around each point"));
        let name = |c| match c {
            Channel::Luma => t!("Luminance", "Luma"),
            Channel::R => t!("Rouge", "Red"),
            Channel::G => t!("Vert", "Green"),
            Channel::B => t!("Bleu", "Blue"),
            Channel::Auto | Channel::Mix(_) => t!("Automatique", "Automatic"),
        };
        ui.horizontal(|ui| {
            egui::ComboBox::from_label(t!("Canal", "Channel")).selected_text(name(self.o.channel)).show_ui(ui, |ui| {
                for c in [Channel::Auto, Channel::Luma, Channel::R, Channel::G, Channel::B] {
                    ui.selectable_value(&mut self.o.channel, c, name(c));
                }
            }).response.on_hover_text(t!("Automatique : le mélange de couleurs où la zone ressort le mieux, calculé au départ du tracking", "Automatic: the colour mix where the area stands out best, computed when tracking starts"));
            if ui.add(egui::Button::new(t!("Voir", "View")).selected(self.see)).on_hover_text(t!("Affiche la vidéo comme le tracker la voit : seulement le canal choisi, en niveaux de gris.\nAvec Automatique, c'est le mélange calculé pour la zone : la dessiner d'abord.", "Shows the video as the tracker sees it: only the chosen channel, in greyscale.\nWith Automatic, it is the mix computed for the area: draw it first.")).clicked() {
                self.see = !self.see;
            }
        });
        ui.checkbox(&mut self.o.realign, t!("Recaler sur la frame de départ", "Realign on the start frame")).on_hover_text(t!("Chaque frame est réalignée sur celle où le tracking a commencé : le suivi ne se décale plus peu à peu.\nÀ décocher si l'objet change beaucoup d'aspect en cours de route.", "Each frame is realigned on the one where tracking started: the track no longer drifts little by little.\nUntick it if the object changes a lot in appearance along the way."));
        if ui.add(egui::Slider::new(&mut self.o.smoothing, 0..=10).text(t!("Lissage", "Smoothing"))).changed() {
            self.dirty = true;
        }
    }

    fn follow_ui(ui: &mut egui::Ui, fol: &mut Follow, deform: &str, hint: &str) {
        ui.strong(t!("Ce qu'il suit du tracking", "What it follows from the tracking"));
        ui.checkbox(&mut fol.translation, t!("Déplacement", "Movement"));
        ui.checkbox(&mut fol.scale, t!("Agrandissement / rétrécissement", "Growing / shrinking"));
        ui.checkbox(&mut fol.rotation, "Rotation");
        ui.checkbox(&mut fol.deform, deform).on_hover_text(hint);
    }

    fn color_row(ui: &mut egui::Ui, label: &str, color: &mut [u8; 3], picking: &mut Option<Pick>, me: Pick) {
        ui.label(label);
        ui.color_edit_button_srgb(color);
        if ui.add(egui::Button::new(t!("Pipette", "Eyedropper")).selected(*picking == Some(me))).on_hover_text(t!("Puis cliquer sur la vidéo pour prendre la couleur à cet endroit", "Then click on the video to pick the colour at that spot")).clicked() {
            *picking = if *picking == Some(me) { None } else { Some(me) };
        }
    }

    fn mask_tab(&mut self, ui: &mut egui::Ui) {
        ui.checkbox(&mut self.o.mask_on, t!("Exporter le cache", "Export the mask"));
        ui.weak(if self.o.mask.is_empty() { t!("Le cache reprend la sélection du tracking. Clic sur la vidéo : en dessiner un autre, où vous voulez.", "The mask uses the tracking selection. Click on the video: draw another one, wherever you like.") } else { t!("Clic : ajouter un point. Glisser : déplacer. Clic droit : supprimer.", "Click: add a point. Drag: move. Right click: delete.") });
        if ui.add_enabled(!self.o.mask.is_empty(), egui::Button::new(t!("Revenir à la sélection du tracking", "Back to the tracking selection"))).clicked() {
            self.checkpoint();
            self.o.mask.clear();
        }
        ui.separator();
        ui.strong(t!("Apparence", "Appearance"));
        ui.horizontal(|ui| Self::color_row(ui, t!("Couleur", "Colour"), &mut self.o.color, &mut self.picking, Pick::Mask));
        ui.add_enabled(self.o.color_spot.is_some(), egui::Checkbox::new(&mut self.o.color_live, t!("Reprendre la couleur à chaque frame", "Pick the colour again on every frame"))).on_hover_text(t!("La couleur du cache est relevée à chaque frame à l'endroit choisi avec la pipette, qui suit le tracking.\nUtile quand l'éclairage de la scène change. Sans effet sur les dégradés et sur une image.", "The mask colour is read on every frame at the spot chosen with the eyedropper, which follows the tracking.\nUseful when the lighting of the scene changes. No effect on gradients or on an image.")).on_disabled_hover_text(t!("Choisir d'abord la couleur avec la pipette, sur la vidéo.", "First pick the colour with the eyedropper, on the video."));
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut self.o.blur).range(0.0..=20.0).speed(0.1).prefix(t!("Flou ", "Blur ")));
            if ui.button(t!("Mesurer", "Measure")).on_hover_text(t!("Mesure le flou des bords de l'objet sur la frame affichée et règle le flou du cache pour qu'il s'y fonde.", "Measures the blur of the object's edges on the displayed frame and sets the mask blur so that it blends in.")).clicked() {
                match self.measured_blur() {
                    Some(b) => self.o.blur = b,
                    None => self.status = t!("Flou non mesurable : dessinez d'abord la zone sur un objet aux bords visibles.", "Blur cannot be measured: first draw the area on an object with visible edges.").into(),
                }
            }
        });
        ui.add(egui::DragValue::new(&mut self.o.grow).range(-60.0..=60.0).speed(0.1).prefix(t!("Marge ", "Margin ")).suffix(" px")).on_hover_text(t!("Élargit le cache (valeur positive) ou le rétrécit (négative) de ce nombre de pixels, sans le redessiner.\nSans effet sur une image.", "Widens the mask (positive value) or shrinks it (negative) by this many pixels, without redrawing it.\nNo effect on an image."));
        ui.add(egui::DragValue::new(&mut self.o.mask_layer).range(0..=99).prefix(t!("Calque +", "Layer +"))).on_hover_text(t!("Ajouté au numéro de calque (Layer) de la ligne d'origine pour les lignes du cache.\nDans Aegisub, une ligne d'un calque plus élevé passe devant les autres, quel que soit l'ordre des lignes.", "Added to the layer number of the source line for the mask lines.\nIn Aegisub, a line on a higher layer is drawn in front of the others, whatever the order of the lines."));
        ui.separator();
        ui.strong(t!("Dégradés", "Gradients"));
        let mut gone = None;
        for (i, g) in self.o.gradients.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                Self::color_row(ui, &format!("{}", i + 1), &mut g.color, &mut self.picking, Pick::Gradient(i));
                if ui.button("✕").on_hover_text(t!("Retirer ce dégradé", "Remove this gradient")).clicked() {
                    gone = Some(i);
                }
            });
            ui.add(egui::Slider::new(&mut g.angle, 0.0..=360.0).suffix("°").text(t!("Orientation", "Direction"))).on_hover_text(t!("Sens dans lequel la couleur apparaît : 0° vers la droite, 90° vers le bas", "Direction in which the colour appears: 0° to the right, 90° downwards"));
            ui.add(egui::Slider::new(&mut g.force, 0.0..=1.0).custom_formatter(|v, _| format!("{:.0} %", v * 100.)).text(t!("Force", "Strength"))).on_hover_text(t!("Opacité de la couleur au bout du dégradé", "Opacity of the colour at the end of the gradient"));
        }
        if let Some(i) = gone {
            self.o.gradients.remove(i);
            self.picking = None;
        }
        if ui.button(t!("Ajouter un dégradé", "Add a gradient")).on_hover_text(t!("La couleur du dégradé se fond progressivement sur celle du cache. Plusieurs dégradés se superposent.", "The gradient colour blends gradually into the mask colour. Several gradients stack.")).clicked() {
            self.o.gradients.push(Gradient { color: [255, 255, 255], angle: 90., force: 1. });
        }
        if !self.o.gradients.is_empty() {
            ui.add(egui::Slider::new(&mut self.o.bands, 2..=64).text(t!("Bandes", "Bands"))).on_hover_text(t!("Plus de bandes : dégradé plus fin, mais autant de lignes en plus par frame et par dégradé", "More bands: a finer gradient, but as many extra lines per frame and per gradient"));
        }
        ui.separator();
        ui.strong("Image");
        ui.horizontal(|ui| {
            if ui.button(t!("Choisir une image...", "Choose an image...")).clicked() {
                if let Some(p) = rfd::FileDialog::new().set_title(t!("Image à mettre à la place du cache", "Image to put in place of the mask")).pick_file() {
                    let count = |pic: &Picture| pic.layers.iter().flat_map(|l| &l.1).map(Vec::len).sum();
                    match picture::load(&p) {
                        Ok(Loaded::Raster(img)) => {
                            let points = count(&picture::trace(&img.0, img.1, img.2, self.o.image_colors));
                            self.image_ask = Some((Loaded::Raster(img), points));
                        }
                        Ok(Loaded::Vector(pic)) => {
                            let points = count(&pic);
                            self.image_ask = Some((Loaded::Vector(pic), points));
                        }
                        Err(e) => self.status = e,
                    }
                }
            }
            if self.o.picture.is_some() && ui.button(t!("Retirer", "Remove")).clicked() {
                (self.o.image, self.o.picture) = (None, None);
            }
        });
        if self.o.image.is_some() && ui.add(egui::Slider::new(&mut self.o.image_colors, 2..=32).text(t!("Couleurs", "Colours"))).on_hover_text(t!("L'image est réduite à ce nombre de couleurs, une ligne par couleur et par frame", "The image is reduced to this number of colours, one line per colour and per frame")).changed() {
            self.vectorise(ui.ctx());
        }
        if self.o.picture.is_some() {
            ui.add_enabled(self.mask_poly().len() == 4, egui::Checkbox::new(&mut self.o.pin, t!("Poser l'image sur les 4 points du cache", "Place the image on the 4 points of the mask"))).on_hover_text(t!("Les coins de l'image vont sur les 4 points du cache, dans l'ordre où ils ont été cliqués :\nhaut gauche, haut droit, bas droit, bas gauche. L'image est alors couchée en perspective.\nDécoché : elle remplit le rectangle droit qui entoure le cache.", "The corners of the image go on the 4 points of the mask, in the order they were clicked:\ntop left, top right, bottom right, bottom left. The image then lies in perspective.\nUnticked: it fills the upright rectangle around the mask.")).on_disabled_hover_text(t!("Le cache doit avoir exactement 4 points.", "The mask must have exactly 4 points."));
        }
        ui.weak(t!("L'image remplace le cache et remplit le rectangle qui l'entoure. Elle est convertie en formes : préférer un logo ou un dessin simple.", "The image replaces the mask and fills the rectangle around it. It is converted to shapes: prefer a logo or a simple drawing."));
        ui.separator();
        Self::follow_ui(ui, &mut self.o.mask_follow, t!("Perspective et déformation", "Perspective and distortion"), t!("Décoché : le cache garde sa forme d'origine, seulement déplacée, agrandie ou tournée.", "Unticked: the mask keeps its original shape, only moved, scaled or rotated."));
        ui.checkbox(&mut self.o.mask_opacity, t!("Opacité (fondus)", "Opacity (fades)")).on_hover_text(t!("Le cache devient transparent en même temps que ce qui est suivi : \\alpha frame par frame.\nPour un objet qui disparaît complètement, cocher aussi « Opacité » sous « Autres suivis », dans l'onglet Tracking, avant de tracker.", "The mask becomes transparent together with what is tracked: \\alpha frame by frame.\nFor an object that disappears completely, also tick “Opacity” under “Other tracking”, in the Tracking tab, before tracking."));
    }

    fn text_tab(&mut self, ui: &mut egui::Ui) {
        ui.checkbox(&mut self.o.text_on, t!("Exporter le texte", "Export the text"));
        if let Some(j) = &self.job {
            ui.radio_value(&mut self.o.import, true, tf!("Texte importé d'Aegisub ({} ligne(s))", "Text imported from Aegisub ({} line(s))", j.lines.len())).on_hover_text(t!("Chaque ligne sélectionnée garde son texte et son style, et est remplacée par ses versions trackées.", "Each selected line keeps its text and style, and is replaced by its tracked versions."));
            ui.radio_value(&mut self.o.import, false, t!("Texte saisi ici", "Text typed here")).on_hover_text(t!("Une nouvelle ligne est ajoutée à côté de la sélection, qui n'est pas modifiée. Elle reprend le style de la première ligne sélectionnée.", "A new line is added next to the selection, which is left untouched. It takes the style of the first selected line."));
        }
        match &self.job {
            Some(j) if self.o.import => {
                for l in j.lines.iter().take(6) {
                    ui.weak(format!("• {}", export::strip_tags(&l.text)));
                }
                if j.lines.len() > 6 {
                    ui.weak(tf!("... et {} autres", "... and {} more", j.lines.len() - 6));
                }
            }
            _ => {
                ui.text_edit_singleline(&mut self.o.text).on_hover_text(t!("Texte de la ligne, tags ASS compris", "Text of the line, ASS tags included"));
            }
        }
        if self.job.is_none() {
            ui.horizontal(|ui| {
                ui.label("Style");
                ui.add(egui::TextEdit::singleline(&mut self.o.style_name).desired_width(120.));
            })
            .response
            .on_hover_text(t!("Nom du style Aegisub donné aux lignes copiées", "Name of the Aegisub style given to the copied lines"));
        }
        ui.weak(t!("Clic ou glisser sur la vidéo : placer le texte.", "Click or drag on the video: place the text."));
        let cuts = [(Cut::None, t!("Pas de découpe", "No clipping")), (Cut::Inside, t!("Visible seulement dans le cache (\\clip)", "Visible only inside the mask (\\clip)")), (Cut::Outside, t!("Caché par le cache (\\iclip)", "Hidden by the mask (\\iclip)"))];
        egui::ComboBox::from_id_salt("cut").selected_text(cuts.iter().find(|c| c.0 == self.o.cut).map_or("", |c| c.1)).show_ui(ui, |ui| {
            for (c, name) in cuts {
                ui.selectable_value(&mut self.o.cut, c, name);
            }
        }).response.on_hover_text(t!("Découpe le texte avec la forme du cache (onglet Cache), qui suit le tracking.\nPar exemple pour faire passer le texte derrière un personnage : dessiner le cache sur lui et choisir « Caché par le cache ».\nLe cache n'a pas besoin d'être exporté.", "Clips the text with the shape of the mask (Mask tab), which follows the tracking.\nFor example to make the text pass behind a character: draw the mask over them and choose “Hidden by the mask”.\nThe mask does not need to be exported."));
        ui.separator();
        let mut measure = false;
        let st = &mut self.o.style;
        ui.checkbox(&mut st.custom, t!("Personnaliser l'apparence", "Customise the appearance")).on_hover_text(t!("Décoché : le texte garde le style de la ligne dans Aegisub.\nCoché : les réglages ci-dessous remplacent ce style.", "Unticked: the text keeps the style of the line in Aegisub.\nTicked: the settings below replace that style."));
        ui.add_enabled_ui(st.custom, |ui| {
            if st.custom && self.fonts.is_empty() {
                let mut all = fontique::Collection::new(Default::default());
                self.fonts = all.family_names().map(str::to_owned).collect();
                self.fonts.sort_by_key(|f| f.to_lowercase());
                self.fonts.dedup();
            }
            ui.horizontal(|ui| {
                ui.label(t!("Police", "Font"));
                ui.add(egui::TextEdit::singleline(&mut st.font).desired_width(120.));
                egui::ComboBox::from_id_salt("fonts").selected_text(t!("Choisir", "Choose")).width(60.).show_ui(ui, |ui| {
                    for f in &self.fonts {
                        if ui.selectable_label(st.font == *f, f).clicked() {
                            st.font = f.clone();
                        }
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut st.size).range(1.0..=1000.0).prefix(t!("Taille ", "Size ")));
                ui.checkbox(&mut st.bold, t!("Gras", "Bold"));
                ui.checkbox(&mut st.italic, t!("Italique", "Italic"));
            });
            ui.horizontal(|ui| Self::color_row(ui, t!("Couleur", "Colour"), &mut st.color, &mut self.picking, Pick::Text));
            ui.horizontal(|ui| {
                Self::color_row(ui, t!("Contour", "Outline"), &mut st.outline_color, &mut self.picking, Pick::Outline);
                ui.add(egui::DragValue::new(&mut st.outline).range(0.0..=50.0).speed(0.1));
            });
            ui.horizontal(|ui| {
                Self::color_row(ui, t!("Ombre", "Shadow"), &mut st.shadow_color, &mut self.picking, Pick::Shadow);
                ui.add(egui::DragValue::new(&mut st.shadow).range(0.0..=50.0).speed(0.1));
            });
            ui.add(egui::DragValue::new(&mut st.blur).range(0.0..=20.0).speed(0.1).prefix(t!("Flou ", "Blur ")));
            if ui.button(t!("Mesurer", "Measure")).on_hover_text(t!("Mesure le flou des bords de l'objet suivi sur la frame affichée et donne le même au texte.", "Measures the blur of the tracked object's edges on the displayed frame and gives the same to the text.")).clicked() {
                measure = true;
            }
        });
        ui.separator();
        if let (true, Some(b)) = (measure, self.measured_blur()) {
            self.o.style.blur = b;
        }
        ui.add(egui::DragValue::new(&mut self.o.text_layer).range(0..=99).prefix(t!("Calque +", "Layer +"))).on_hover_text(t!("Ajouté au numéro de calque (Layer) de la ligne d'origine pour les lignes du texte.\nMettre 1 ici et 0 pour le cache garantit que le texte reste devant le cache, même si les lignes sont triées ou déplacées.", "Added to the layer number of the source line for the text lines.\nSetting 1 here and 0 for the mask guarantees the text stays in front of the mask, even if the lines are sorted or moved."));
        ui.checkbox(&mut self.o.twin, t!("Flouter aussi l'intérieur des lettres", "Blur the inside of the letters too")).on_hover_text(t!("En ASS, le flou d'un texte à contour ne touche que l'extérieur du contour.\nCoché : chaque ligne est exportée deux fois, la seconde sans contour par-dessus, pour que les lettres soient floutées elles aussi.", "In ASS, the blur of an outlined text only touches the outside of the outline.\nTicked: each line is exported twice, the second one without outline on top, so that the letters are blurred as well."));
        Self::follow_ui(ui, &mut self.o.text_follow, t!("Perspective et déformation", "Perspective and distortion"), t!("Le texte s'écrase, s'étire et se couche comme l'objet suivi (\\fscx, \\fscy, \\frx, \\fry, \\fax).\nIl faut que le tracking ait suivi cette déformation : cocher Cisaillement ou Perspective dans l'onglet Tracking.\nDécoché : le texte reste face à l'écran et grossit de la même façon dans les deux sens.", "The text squashes, stretches and tilts like the tracked object (\\fscx, \\fscy, \\frx, \\fry, \\fax).\nThe tracking must have followed that distortion: tick Shear or Perspective in the Tracking tab.\nUnticked: the text keeps facing the screen and grows the same way in both directions."));
        ui.checkbox(&mut self.o.text_opacity, t!("Opacité (fondus)", "Opacity (fades)")).on_hover_text(t!("Le texte devient transparent en même temps que ce qui est suivi : \\alpha frame par frame.\nPour un objet qui disparaît complètement, cocher aussi « Opacité » sous « Autres suivis », dans l'onglet Tracking, avant de tracker.", "The text becomes transparent together with what is tracked: \\alpha frame by frame.\nFor an object that disappears completely, also tick “Opacity” under “Other tracking”, in the Tracking tab, before tracking."));
    }

    fn timeline(&mut self, ui: &mut egui::Ui) {
        let Some(n) = self.clip.as_ref().map(|c| c.n) else { return };
        let busy = self.worker.is_some();
        ui.add_space(4.);
        if self.graph {
            let (area, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 36.), Sense::hover());
            let at = |i: usize, v: f32| Pos2::new(area.left() + area.width() * (i as f32 + 0.5) / n as f32, area.bottom() - 2. - (area.height() - 4.) * v.clamp(0., 1.));
            let p = ui.painter_at(area);
            p.rect_filled(area, 2., ui.visuals().extreme_bg_color);
            let mut line = vec![];
            for (i, f) in self.o.frames.iter().enumerate() {
                match f {
                    Some(f) => line.push(at(i, f.score)),
                    None => drop(p.add(Shape::line(std::mem::take(&mut line), Stroke::new(1.5, Color32::from_rgb(70, 170, 90))))),
                }
            }
            p.add(Shape::line(line, Stroke::new(1.5, Color32::from_rgb(70, 170, 90))));
        }
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 20.), Sense::click_and_drag());
        let x = |i: usize| rect.left() + rect.width() * i as f32 / n as f32;
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 2., ui.visuals().extreme_bg_color);
        p.rect_filled(Rect::from_x_y_ranges(x(self.range[0])..=x(self.range[1] + 1), rect.y_range()), 2., ui.visuals().faint_bg_color);
        for (i, f) in self.o.frames.iter().enumerate().filter_map(|(i, f)| Some((i, f.as_ref()?))) {
            let color = match (self.o.covers.get(i), f.guessed) {
                (Some(Cover::Hidden), _) => Color32::from_gray(110),
                (_, true) => Color32::from_rgb(230, 140, 40),
                _ => Color32::from_rgb(70, 170, 90),
            };
            p.rect_filled(Rect::from_x_y_ranges(x(i)..=x(i + 1), rect.bottom() - 6.0..=rect.bottom()), 0., color);
        }
        for &k in self.job.as_ref().map_or(&[][..], |j| &j.keyframes) {
            p.line_segment([Pos2::new(x(k), rect.top()), Pos2::new(x(k), rect.bottom() - 6.)], Stroke::new(1., Color32::from_gray(220)));
        }
        p.rect_filled(Rect::from_x_y_ranges(x(self.cur)..=x(self.cur + 1).max(x(self.cur) + 2.), rect.y_range()), 0., ACCENT);
        if let Some(pos) = resp.interact_pointer_pos().filter(|_| !busy) {
            self.cur = (((pos.x - rect.left()) / rect.width() * n as f32).max(0.) as usize).min(n - 1);
        }
        ui.horizontal(|ui| {
            ui.add_enabled_ui(!busy, |ui| {
                if ui.button("⏮").clicked() {
                    self.cur = self.range[0];
                }
                if ui.button("◀").clicked() {
                    self.cur = self.cur.saturating_sub(1);
                }
                if ui.button(if self.playing { "⏸" } else { "▶" }).on_hover_text(t!("Lecture (Espace)", "Play (Space)")).clicked() {
                    self.playing = !self.playing;
                }
                if ui.button("▶|").clicked() {
                    self.cur = (self.cur + 1).min(n - 1);
                }
                if ui.button("⏭").clicked() {
                    self.cur = self.range[1];
                }
                let guessed = |i: &usize| self.o.frames[*i].as_ref().is_some_and(|f| f.guessed);
                if (0..n).any(|i| guessed(&i)) {
                    if ui.button(t!("◀ estimée", "◀ estimated")).on_hover_text(t!("Frame estimée précédente (orange)", "Previous estimated frame (orange)")).clicked() {
                        self.cur = (0..self.cur).rev().find(guessed).unwrap_or(self.cur);
                    }
                    if ui.button(t!("estimée ▶", "estimated ▶")).on_hover_text(t!("Frame estimée suivante (orange)", "Next estimated frame (orange)")).clicked() {
                        self.cur = (self.cur + 1..n).find(guessed).unwrap_or(self.cur);
                    }
                }
            });
            ui.monospace(format!("{:>4} / {n}   {:.3} s", self.cur + 1, self.times.get(self.cur).copied().unwrap_or(0) as f64 / 1000.));
            if let Some(f) = self.shown.get(self.cur).and_then(Option::as_ref).filter(|_| self.o.opacity || self.o.mask_opacity || self.o.text_opacity) {
                ui.monospace(tf!("opacité {:>3.0} %", "opacity {:>3.0} %", 100. * export::relative_opacity(&self.shown, f)));
            }
            ui.separator();
            ui.add_enabled_ui(!busy, |ui| {
                ui.label(t!("Plage", "Range")).on_hover_text(t!("Passage à tracker dans la vidéo chargée : les boutons Tracker s'arrêtent à ses bornes.\n[ et ] placent le début et la fin sur la frame affichée.", "Part of the loaded video to track: the Track buttons stop at its ends.\n[ and ] set the start and the end on the displayed frame."));
                let (mut a, mut b) = (self.range[0] + 1, self.range[1] + 1);
                ui.add(egui::DragValue::new(&mut a).range(1..=b));
                if ui.button("[").on_hover_text(t!("Début de plage = frame affichée", "Range start = displayed frame")).clicked() {
                    a = (self.cur + 1).min(b);
                }
                if ui.button("]").on_hover_text(t!("Fin de plage = frame affichée", "Range end = displayed frame")).clicked() {
                    b = (self.cur + 1).max(a);
                }
                ui.add(egui::DragValue::new(&mut b).range(a..=n));
                self.range = [a - 1, b - 1];
            });
            ui.separator();
            if busy {
                ui.spinner();
                if ui.button(t!("⏹ Arrêter", "⏹ Stop")).clicked() {
                    self.stop();
                }
            } else {
                let ctx = ui.ctx().clone();
                if ui.button(t!("◀ Tracker", "◀ Track")).on_hover_text(t!("Vers le début de la plage", "Towards the start of the range")).clicked() {
                    self.start_track(&[-1], &ctx);
                }
                if ui.button(t!("Tracker ▶", "Track ▶")).on_hover_text(t!("Vers la fin de la plage", "Towards the end of the range")).clicked() {
                    self.start_track(&[1], &ctx);
                }
                if ui.button(t!("◀ Tout ▶", "◀ All ▶")).on_hover_text(t!("Dans les deux sens depuis la frame affichée", "Both ways from the displayed frame")).clicked() {
                    self.start_track(&[1, -1], &ctx);
                }
                if ui.add_enabled(self.tracked(), egui::Button::new(t!("Affiner", "Refine"))).on_hover_text(t!("Réaligne chaque frame suivie de la plage sur la frame de départ.\nÀ utiliser quand le suivi s'est décalé peu à peu, ou après une correction à la main.", "Realigns every tracked frame of the range on the start frame.\nUse it when the track has drifted little by little, or after a correction by hand.")).clicked() {
                    self.start_refine(&ctx);
                }
                let lone = (self.range[0]..=self.range[1]).any(|i| self.o.frames[i].as_ref().is_some_and(|f| f.guessed));
                if ui.add_enabled(lone, egui::Button::new(t!("Interpoler", "Interpolate"))).on_hover_text(t!("Remplace chaque suite de frames estimées (orange) par un passage régulier entre la frame sûre d'avant et celle d'après.\nCorriger d'abord à la main une frame au milieu si besoin : elle devient un point de passage. Affiner peut ensuite recaler le résultat.", "Replaces each run of estimated frames (orange) with a steady passage between the reliable frame before and the one after.\nCorrect a frame in the middle by hand first if needed: it becomes a waypoint. Refine can then realign the result.")).clicked() {
                    self.interpolate();
                }
                if ui.add_enabled(self.tracked(), egui::Button::new(t!("Effacer le tracking", "Clear the tracking"))).clicked() {
                    self.checkpoint();
                    self.clear_track();
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button(t!("Vue", "View"), |ui| {
                    if ui.button(t!("Ajuster la vue", "Fit the view")).on_hover_text(t!("Molette : zoom. Clic droit (ou clic molette) maintenu : déplacer la vue.", "Wheel: zoom. Right click (or wheel click) held: move the view.")).clicked() {
                        self.fit = true;
                    }
                    ui.checkbox(&mut self.lock_view, t!("Verrouiller la vue sur l'objet", "Lock the view on the object")).on_hover_text(t!("L'objet suivi reste au même endroit de l'écran : s'il bouge pendant la lecture, c'est que le tracking glisse.", "The tracked object stays at the same place on screen: if it moves during playback, the tracking is slipping."));
                    ui.checkbox(&mut self.loop_play, t!("Lecture en boucle sur la plage", "Loop playback over the range")).on_hover_text(t!("La lecture repart au début de la plage quand elle en atteint la fin, au lieu de s'arrêter au bout de la vidéo.", "Playback goes back to the start of the range when it reaches its end, instead of stopping at the end of the video."));
                    ui.checkbox(&mut self.hide, t!("Masquer les tracés (H)", "Hide overlays (H)")).on_hover_text(t!("N'affiche que la vidéo : ni zones, ni points, ni cache, ni texte. Pour comparer avec l'image d'origine.", "Shows the video only: no areas, points, mask or text. To compare with the original picture."));
                    ui.checkbox(&mut self.loupe, t!("Loupe", "Magnifier")).on_hover_text(t!("Agrandit la vidéo autour du pointeur, dans un coin, pour placer un point au pixel près.", "Enlarges the video around the pointer, in a corner, to place a point to the pixel."));
                    ui.checkbox(&mut self.graph, t!("Courbe de confiance", "Confidence curve")).on_hover_text(t!("Affiche la confiance du tracking frame par frame, au-dessus de la timeline.", "Shows the tracking confidence frame by frame, above the timeline."));
                });
            });
        });
        ui.add_space(2.);
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let rect = resp.rect;
        let Some(clip) = self.clip.clone() else {
            painter.text(rect.center(), Align2::CENTER_CENTER, t!("Aucune vidéo", "No video"), FontId::proportional(18.), ui.visuals().weak_text_color());
            return;
        };
        let fresh = self.o.channel == Channel::Auto && !self.tracked() && self.o.shape.len() >= 3;
        let mark = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (self.see, self.see.then(|| (self.o.channel.put(), self.o.mix.map(f32::to_bits), fresh.then(|| self.o.shape.as_flattened().iter().map(|v| v.to_bits()).collect::<Vec<u32>>())))).hash(&mut h);
            h.finish()
        };
        if self.tex.as_ref().map(|t| t.0) != Some(self.cur) || self.tex_mark != mark {
            self.tex_mark = mark;
            if let Ok(rgb) = clip.rgb(self.cur) {
                let channel = match self.o.channel {
                    Channel::Auto if fresh => Channel::Mix(track::contrast(&rgb, clip.w, clip.h, &self.o.shape, self.o.pattern as f32)),
                    Channel::Auto if self.tracked() => Channel::Mix(self.o.mix),
                    other => other,
                };
                let img = if self.see { egui::ColorImage::from_gray([clip.w, clip.h], &Gray::from_rgb(&rgb, clip.w, clip.h, channel).view()) } else { egui::ColorImage::from_rgb([clip.w, clip.h], &rgb) };
                self.tex = Some((self.cur, ui.ctx().load_texture("frame", img, Default::default())));
            }
        }

        let size = Vec2::new(clip.w as f32, clip.h as f32);
        if self.fit {
            self.zoom = (rect.width() / size.x).min(rect.height() / size.y);
            self.pan = (rect.size() - size * self.zoom) / 2.;
        }
        if let Some(pos) = resp.hover_pos() {
            let scroll = ui.input(|i| i.smooth_scroll_delta().y);
            if scroll != 0. {
                let new = (self.zoom * (scroll * 0.003).exp()).clamp(0.05, 40.);
                self.pan -= (pos - rect.min - self.pan) * (new / self.zoom - 1.);
                (self.zoom, self.fit) = (new, false);
            }
        }
        if resp.dragged_by(PointerButton::Secondary) || resp.dragged_by(PointerButton::Middle) {
            self.pan += resp.drag_delta();
            self.fit = false;
        }
        let near = self.nearest(self.cur);
        let ghost = near.is_some() && near != Some(self.cur);
        let view = View { fd: near.and_then(|i| self.shown[i].clone()), ref_pts: self.o.ref_pts.clone(), warp: self.o.warp };
        let tracked = near.is_some();
        let zoom = self.zoom;
        let held = export::centroid(&self.o.shape);
        let slip = view.map(held);
        let lock = if self.lock_view && !self.o.shape.is_empty() { Vec2::new(held[0] - slip[0], held[1] - slip[1]) * zoom } else { Vec2::ZERO };
        let origin = rect.min + self.pan + lock;
        let scr = |p: P| origin + Vec2::new(p[0], p[1]) * zoom;
        let img = |p: Pos2| -> P { [(p.x - origin.x) / zoom, (p.y - origin.y) / zoom] };

        let mask_ref: Vec<P> = self.mask_poly().to_vec();
        let (mask_c, mask_fol, text_fol) = (export::centroid(&mask_ref), self.o.mask_follow, self.o.text_follow);
        if let Some(pick) = self.picking {
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            }
            let at = resp.interact_pointer_pos().filter(|_| resp.clicked()).map(|pos| img(pos).map(|v| v.round() as i64));
            if let Some([x, y]) = at.filter(|[x, y]| (0..clip.w as i64).contains(x) && (0..clip.h as i64).contains(y)) {
                if let Some(color) = clip.average(self.cur, [x as f32, y as f32]) {
                    match pick {
                        Pick::Mask => (self.o.color, self.o.color_spot) = (color, Some(view.unmap([x as f32, y as f32]))),
                        Pick::Gradient(i) => self.o.gradients[i].color = color,
                        Pick::Text => self.o.style.color = color,
                        Pick::Outline => self.o.style.outline_color = color,
                        Pick::Shadow => self.o.style.shadow_color = color,
                    }
                    self.picking = None;
                    self.status = tf!("Couleur prélevée : #{:02X}{:02X}{:02X}.", "Colour picked: #{:02X}{:02X}{:02X}.", color[0], color[1], color[2]);
                }
            }
        } else if self.worker.is_none() && !self.hide {
            let active: Vec<P> = match self.mode {
                Mode::Track => self.o.shape.iter().map(|v| view.map(*v)).collect(),
                Mode::Mask => self.o.mask.iter().map(|v| view.place(mask_c, mask_fol, *v)).collect(),
                Mode::Text => vec![],
            };
            let pointer = resp.interact_pointer_pos();
            if self.mode == Mode::Text && (resp.clicked() || resp.drag_started_by(PointerButton::Primary)) {
                self.checkpoint();
                self.o.text_on = true;
            }
            if let Some(pos) = pointer.filter(|_| self.mode == Mode::Text && (resp.clicked() || resp.dragged_by(PointerButton::Primary))) {
                self.o.anchor = Some(if text_fol.translation { view.unmap(img(pos)) } else { img(pos) });
            }
            let hit = |pos: Pos2| active.iter().position(|v| scr(*v).distance(pos) < HIT);
            if let Some(pos) = ui.input(|i| i.pointer.press_origin()).filter(|_| resp.drag_started_by(PointerButton::Primary)) {
                self.drag = match hit(pos) {
                    Some(i) => Some(Drag::Vertex(i)),
                    None => track::in_poly(&active, img(pos)).then_some(Drag::Whole),
                };
                if self.drag.is_some() {
                    self.checkpoint();
                }
            }
            if let (Some(d), Some(pos)) = (self.drag, pointer.filter(|_| resp.dragged_by(PointerButton::Primary))) {
                let delta = match d {
                    Drag::Vertex(i) => [img(pos)[0] - active[i][0], img(pos)[1] - active[i][1]],
                    Drag::Whole => (resp.drag_delta() / zoom).into(),
                };
                self.apply_drag(d, delta, &view, tracked);
            }
            if resp.drag_stopped() {
                self.drag = None;
            }
            if let Some(pos) = pointer.filter(|p| resp.clicked() && hit(*p).is_none()) {
                match self.mode {
                    Mode::Track if !tracked => {
                        self.checkpoint();
                        if self.holing { self.o.hole.push(img(pos)) } else { self.o.shape.push(img(pos)) }
                    }
                    Mode::Track => self.status = t!("La sélection est verrouillée : glissez-la pour corriger cette frame, ou effacez le tracking pour la redessiner.", "The selection is locked: drag it to correct this frame, or clear the tracking to redraw it.").into(),
                    Mode::Mask => {
                        self.checkpoint();
                        self.o.mask_on = true;
                        let v = if self.o.mask.is_empty() {
                            if mask_fol.translation { view.unmap(img(pos)) } else { img(pos) }
                        } else {
                            view.unplace(mask_c, mask_fol, img(pos))
                        };
                        self.o.mask.push(v);
                    }
                    Mode::Text => {}
                }
            }
            if let Some(i) = pointer.filter(|_| resp.secondary_clicked()).and_then(hit) {
                if self.mode == Mode::Mask || !tracked {
                    self.checkpoint();
                }
                match self.mode {
                    Mode::Track if !tracked => drop(self.o.shape.remove(i)),
                    Mode::Mask => drop(self.o.mask.remove(i)),
                    _ => {}
                }
            }
        }

        if let Some((_, tex)) = &self.tex {
            painter.image(tex.id(), Rect::from_min_size(origin, size * zoom), Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)), Color32::WHITE);
        }
        if self.hide {
            painter.text(rect.center_top() + Vec2::Y * 10., Align2::CENTER_TOP, t!("Tracés masqués (H pour les revoir)", "Overlays hidden (H to show them)"), FontId::proportional(13.), Color32::from_gray(200));
            return;
        }
        if self.picking.is_some() {
            let hint = painter.layout_no_wrap(t!("Pipette : cliquez sur la vidéo pour prendre la couleur", "Eyedropper: click on the video to pick the colour").into(), FontId::proportional(15.), Color32::WHITE);
            let at = Pos2::new(rect.center().x - hint.size().x / 2., rect.top() + 10.);
            painter.rect_filled(Rect::from_min_size(at, hint.size()).expand(6.), 4., Color32::from_black_alpha(200));
            painter.galley(at, hint, Color32::WHITE);
        }
        let hides = self.o.covers.get(self.cur).copied().filter(|_| !ghost).unwrap_or(Cover::Clear);
        let front = match (hides, &view.fd) {
            (Cover::Cut(line), Some(f)) => Some(export::frontier(f, &view.ref_pts, view.warp, line)),
            _ => None,
        };
        let line = if ghost || hides == Cover::Hidden { Color32::GRAY } else { ACCENT };
        if self.o.zone {
            for p in view.fd.iter().flat_map(|f| &f.pts) {
                painter.circle_filled(scr(*p), 1.5, Color32::from_rgb(90, 220, 120));
            }
        }
        let mask_ref: Vec<P> = self.mask_poly().to_vec();
        let mask_c = export::centroid(&mask_ref);
        let mask: Vec<Pos2> = mask_ref.iter().map(|v| scr(view.place(mask_c, self.o.mask_follow, *v))).collect();
        if self.o.mask_on && mask.len() >= 3 {
            let seen = view.fd.as_ref().filter(|_| self.o.mask_opacity).map_or(1., |f| export::relative_opacity(&self.shown, f));
            if let Some((pic, tex)) = &self.o.picture {
                let to = export::seat(&mask_ref, pic.w as f32, pic.h as f32, self.o.pin);
                let mut mesh = egui::Mesh::with_texture(tex.id());
                for k in 0..81u32 {
                    let (u, v) = ((k % 9) as f32 / 8., (k / 9) as f32 / 8.);
                    let pos = scr(view.place(mask_c, self.o.mask_follow, track::apply(&to, [u * pic.w as f32, v * pic.h as f32])));
                    mesh.vertices.push(egui::epaint::Vertex { pos, uv: Pos2::new(u, v), color: Color32::from_white_alpha((255. * seen) as u8) });
                    if k % 9 < 8 && k / 9 < 8 {
                        mesh.indices.extend([k, k + 1, k + 9, k + 1, k + 10, k + 9]);
                    }
                }
                painter.add(Shape::mesh(mesh));
            } else {
                let live = self.o.color_spot.filter(|_| self.o.color_live).and_then(|spot| clip.average(self.cur, view.map(spot)));
                for layer in export::layers(&export::inflate(&mask_ref, self.o.grow), live.unwrap_or(self.o.color), &self.o.gradients, self.o.bands, None).into_iter().filter(|_| hides != Cover::Hidden) {
                    let [r, g, b] = layer.color;
                    let placed: Vec<P> = layer.loops[0].iter().map(|v| view.place(mask_c, self.o.mask_follow, *v)).collect();
                    let pts = front.map_or(placed.clone(), |line| export::clip(&placed, line)).into_iter().map(scr).collect();
                    painter.add(Shape::convex_polygon(pts, Color32::from_rgba_unmultiplied(r, g, b, (170. * seen * layer.solid) as u8), Stroke::NONE));
                }
            }
        }
        if !self.o.mask.is_empty() || self.mode == Mode::Mask {
            painter.add(Shape::closed_line(mask.clone(), Stroke::new(1.5, Color32::from_rgb(80, 200, 255))));
            if !self.o.mask.is_empty() {
                for v in &mask {
                    painter.circle_filled(*v, 3.5, Color32::from_rgb(80, 200, 255));
                }
            }
        }
        for (i, o) in self.all().enumerate().filter(|(i, _)| *i != self.sel) {
            if let Some(f) = o.frames.get(self.cur).and_then(Option::as_ref) {
                let outline: Vec<Pos2> = o.shape.iter().map(|v| scr(f.map(&o.ref_pts, o.warp, *v))).collect();
                if let Some(first) = outline.first() {
                    painter.text(*first, Align2::LEFT_BOTTOM, format!("{}", i + 1), FontId::proportional(12.), Color32::from_gray(200));
                }
                painter.add(Shape::closed_line(outline, Stroke::new(1., Color32::from_gray(200))));
            }
        }
        if self.o.hole.len() >= 2 || self.holing {
            let hole: Vec<Pos2> = self.o.hole.iter().map(|v| scr(view.map(*v))).collect();
            for v in &hole {
                painter.circle_filled(*v, 3., Color32::from_rgb(230, 70, 70));
            }
            painter.add(Shape::closed_line(hole, Stroke::new(1.5, Color32::from_rgb(230, 70, 70))));
        }
        let shape: Vec<Pos2> = self.o.shape.iter().map(|v| scr(view.map(*v))).collect();
        if self.o.zone && shape.len() >= 2 {
            match front {
                Some(edge) => {
                    let placed: Vec<P> = self.o.shape.iter().map(|v| view.map(*v)).collect();
                    painter.add(Shape::closed_line(shape.clone(), Stroke::new(1., Color32::GRAY)));
                    painter.add(Shape::closed_line(export::clip(&placed, edge).into_iter().map(scr).collect(), Stroke::new(2., line)));
                }
                None => drop(painter.add(Shape::closed_line(shape.clone(), Stroke::new(1.5, line)))),
            }
            if hides == Cover::Hidden {
                painter.text(shape[0] - Vec2::Y * 6., Align2::LEFT_BOTTOM, t!("recouvert", "covered"), FontId::proportional(13.), Color32::from_gray(200));
            }
        }
        for v in &shape {
            painter.circle_filled(*v, 4., line);
            if !self.o.zone {
                for (half, alpha) in [(self.o.pattern as f32, 255), (self.o.pattern as f32 + self.o.search, 90)] {
                    painter.rect_stroke(Rect::from_center_size(*v, Vec2::splat(2. * half * zoom)), 0., Stroke::new(1., line.gamma_multiply_u8(alpha)), StrokeKind::Middle);
                }
            }
        }
        if self.o.text_on && (tracked || self.o.anchor.is_some() || !self.o.shape.is_empty()) {
            let (a, fol) = (self.anchor_pt(), self.o.text_follow);
            let (pos, grow, rot) = match &view.fd {
                Some(f) => {
                    let (p, sx, sy, r) = export::local_motion(f, &view.ref_pts, view.warp, a, 1.);
                    (if fol.translation { p } else { a }, if fol.scale { (sx * sy).sqrt() as f32 } else { 1. }, if fol.rotation { r as f32 } else { 0. })
                }
                None => (a, 1., 0.),
            };
            let c = scr(pos);
            let key = Self::preview_key(&self.ass_head(), &self.preview_events(&clip, self.cur, [0, 10_000]), &clip);
            self.take_previews(ui.ctx());
            let busy = self.rendering.as_ref().is_some_and(|b| b.keys.contains(&key) || !b.started);
            if !self.previews.contains_key(&key) && !busy && !self.no_preview && self.worker.is_none() {
                self.start_previews(ui.ctx(), &clip, key);
            }
            if self.previews.contains_key(&key) {
                self.last_preview = Some(key);
            }
            match self.last_preview.and_then(|k| self.previews.get(&k)).filter(|_| !self.no_preview) {
                Some(Some((tex, at))) => {
                    painter.image(tex.id(), Rect::from_min_size(scr([at[0] as f32, at[1] as f32]), tex.size_vec2() * zoom), Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)), Color32::WHITE);
                }
                Some(None) => {}
                None => {
                    let text = match &self.job {
                        Some(j) if self.o.import => export::strip_tags(&j.lines.iter().find(|l| (l.first..l.end).contains(&self.cur)).unwrap_or(&j.lines[0]).text),
                        _ => export::strip_tags(&self.o.text),
                    };
                    let st = &self.o.style;
                    let rgb = |c: [u8; 3]| Color32::from_rgb(c[0], c[1], c[2]);
                    let (fill, edge) = if st.custom { (rgb(st.color), rgb(st.outline_color)) } else { (Color32::WHITE, Color32::BLACK) };
                    let base = if st.custom { st.size * clip.h as f32 / self.res[1] as f32 } else { clip.h as f32 * 0.05 };
                    let galley = painter.layout_no_wrap(text, FontId::proportional((base * grow * zoom).clamp(4., 600.)), fill);
                    for (dx, dy, color) in [(-2., 0., edge), (2., 0., edge), (0., -2., edge), (0., 2., edge), (0., 0., fill)] {
                        let at = c - galley.size() / 2. + Vec2::new(dx, dy);
                        painter.add(egui::epaint::TextShape::new(at, galley.clone(), color).with_override_text_color(color).with_angle_and_anchor(rot, Align2::CENTER_CENTER));
                    }
                }
            }
            painter.line_segment([c - Vec2::X * 8., c + Vec2::X * 8.], Stroke::new(1.5, Color32::from_rgb(255, 90, 90)));
            painter.line_segment([c - Vec2::Y * 8., c + Vec2::Y * 8.], Stroke::new(1.5, Color32::from_rgb(255, 90, 90)));
        }
        if let (true, Some(pos), Some((_, tex))) = (self.loupe, resp.hover_pos(), &self.tex) {
            let corner = if pos.x < rect.left() + 200. && pos.y < rect.top() + 200. { rect.right_top() - Vec2::new(184., -8.) } else { rect.min + Vec2::splat(8.) };
            let (at, frame) = (img(pos), Rect::from_min_size(corner, Vec2::splat(176.)));
            let (u, v, du, dv) = (at[0] / size.x, at[1] / size.y, 11. / size.x, 11. / size.y);
            painter.rect_filled(frame, 0., Color32::BLACK);
            painter.image(tex.id(), frame, Rect::from_min_max(Pos2::new(u - du, v - dv), Pos2::new(u + du, v + dv)), Color32::WHITE);
            painter.rect_stroke(frame, 0., Stroke::new(1., ACCENT), StrokeKind::Outside);
            painter.line_segment([frame.center() - Vec2::X * 9., frame.center() + Vec2::X * 9.], Stroke::new(1., ACCENT));
            painter.line_segment([frame.center() - Vec2::Y * 9., frame.center() + Vec2::Y * 9.], Stroke::new(1., ACCENT));
        }
    }

    fn apply_drag(&mut self, d: Drag, delta: P, view: &View, tracked: bool) {
        let (c, fol) = (export::centroid(&self.o.mask), self.o.mask_follow);
        let moved = |v: &mut P| *v = view.unplace(c, fol, add(view.place(c, fol, *v), delta));
        match (self.mode, d) {
            (Mode::Track, Drag::Vertex(i)) if !tracked => self.o.shape[i] = add(self.o.shape[i], delta),
            (Mode::Track, Drag::Whole) if !tracked => self.o.shape.iter_mut().for_each(|v| *v = add(*v, delta)),
            (Mode::Track, _) => {
                if self.o.frames[self.cur].is_none() {
                    self.o.frames[self.cur] = self.nearest(self.cur).and_then(|i| self.o.frames[i].clone());
                }
                let Some(f) = &mut self.o.frames[self.cur] else { return };
                f.guessed = false;
                match d {
                    Drag::Vertex(i) if !self.o.zone => {
                        f.pts[i] = add(f.pts[i], delta);
                        f.h = track::fit(&self.o.motion, &self.o.ref_pts, &f.pts, &vec![true; f.pts.len()]).unwrap_or(f.h);
                    }
                    _ => {
                        f.pts.iter_mut().for_each(|p| *p = add(*p, delta));
                        f.h = track::mul(&track::translation(delta[0] as f64, delta[1] as f64), &f.h);
                    }
                }
                self.dirty = true;
            }
            (Mode::Mask, Drag::Vertex(i)) => moved(&mut self.o.mask[i]),
            (Mode::Mask, Drag::Whole) => self.o.mask.iter_mut().for_each(moved),
            (Mode::Text, _) => {}
        }
    }

    fn range_dialog(&mut self, ctx: &egui::Context) {
        let Some(p) = &mut self.pending else { return };
        let mut choice = None;
        egui::Window::new(t!("Plage à charger", "Range to load")).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, [0., 0.]).show(ctx, |ui| {
            ui.label(p.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
            ui.weak(format!("{}×{} · {:.3} i/s · {:.1} s", p.info.w, p.info.h, p.info.fps, p.info.duration));
            ui.horizontal(|ui| {
                ui.label(t!("Début", "Start"));
                ui.add(egui::DragValue::new(&mut p.start).range(0.0..=p.info.duration.max(0.)).speed(0.1).suffix(" s"));
                ui.label(t!("Durée", "Duration"));
                ui.add(egui::DragValue::new(&mut p.dur).range(0.1..=3600.0).speed(0.1).suffix(" s"));
            });
            let frames = (p.dur * p.info.fps).ceil() as usize;
            ui.weak(tf!("{frames} images extraites dans le dossier temporaire", "{frames} frames extracted to the temporary folder"));
            ui.horizontal(|ui| {
                if ui.button(t!("Charger", "Load")).clicked() {
                    choice = Some(Some(frames));
                }
                if ui.button(t!("Annuler", "Cancel")).clicked() {
                    choice = Some(None);
                }
            });
        });
        if let Some(c) = choice {
            let p = self.pending.take().unwrap();
            if let Some(frames) = c {
                self.load(&p.path, p.info, p.start, frames);
            }
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll(&ctx);
        egui::Panel::top("top").show(ui, |ui| self.top_bar(ui));
        egui::Panel::bottom("timeline").show(ui, |ui| self.timeline(ui));
        egui::Panel::right("settings").resizable(false).default_size(270.).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| self.settings(ui));
        });
        egui::CentralPanel::default().show(ui, |ui| self.canvas(ui));
        self.range_dialog(&ctx);
        self.curves_window(&ctx);
        self.confirm_dialog(&ctx);
        self.image_dialog(&ctx);
        self.import_dialog(&ctx);
        self.resume_dialog(&ctx);
        self.update_dialog(&ctx);
    }

    fn on_exit(&mut self) {
        self.remember();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_comes_back_with_its_objects_and_tracking() {
        let mut app = App::new(None, None, None);
        app.o.frames = vec![None; 4];
        (app.o.shape, app.o.grow, app.o.ease, app.o.text) = (vec![[1.5, 2.], [30., 2.], [30., 40.]], 3., [4, 0, 2], "{\\b1}Sign".into());
        app.o.frames[2] = Some(FrameData { h: track::translation(5.25, -3.), pts: vec![[7., 8.5]], opacity: 0.5, guessed: true, score: 0.75 });
        app.add_object();
        (app.o.hole, app.o.anchor, app.o.opa, app.o.mask_on) = (vec![[1., 1.], [2., 2.], [3., 1.]], Some([9., 10.]), vec![([4., 5.], 0.25)], true);
        let saved = app.keep();
        let mut fresh = App::new(None, None, None);
        fresh.o.frames = vec![None; 4];
        fresh.restore(&saved);
        assert_eq!((fresh.objects.len(), fresh.sel), (2, 1));
        assert_eq!((fresh.o.hole.len(), fresh.o.anchor, fresh.o.opa.clone(), fresh.o.mask_on), (3, Some([9., 10.]), vec![([4., 5.], 0.25)], true));
        fresh.select(0);
        let f = fresh.o.frames[2].clone().unwrap();
        assert_eq!((f.h, f.pts, f.opacity, f.guessed, f.score), (track::translation(5.25, -3.), vec![[7., 8.5]], 0.5, true, 0.75));
        assert_eq!((fresh.o.shape.clone(), fresh.o.grow, fresh.o.ease, fresh.o.text.as_str()), (vec![[1.5, 2.], [30., 2.], [30., 40.]], 3., [4, 0, 2], "{\\b1}Sign"));
        assert!(fresh.o.frames[0].is_none() && fresh.o.frames[3].is_none());
    }

    #[test]
    fn only_a_more_recent_release_is_announced() {
        assert!(newer("2026.12.01-abc1234", "2026.11.25-fff0000"));
        assert!(newer("2026.11.25-abc1234", "2026.11.25-fff0000+local"));
        assert!(!newer("2026.11.25-abc1234", "2026.11.25-abc1234"));
        assert!(!newer("2026.11.25-abc1234", "2026.11.25-abc1234+local"));
        assert!(!newer("2026.11.20-abc1234", "2026.11.25-fff0000"));
        assert!(!newer("v1.1.2", "2026.11.25-fff0000"));
    }

    #[test]
    fn settings_survive_a_round_trip() {
        let mut app = App::new(None, None, None);
        app.o.motion.perspective = true;
        (app.o.search, app.o.channel, app.o.color, app.o.cut, app.group) = (77., Channel::G, [1, 2, 3], Cut::Outside, true);
        (app.o.style.font, app.o.style.custom, app.o.text_follow.rotation) = ("Noto Sans".into(), true, false);
        app.o.gradients = vec![Gradient { color: [9, 8, 7], angle: 45., force: 0.5 }];
        let saved: String = app.knobs().iter().map(|(k, v)| format!("{k}={}\n", v.put())).collect();
        let mut fresh = App::new(None, None, None);
        for (key, knob) in fresh.knobs() {
            knob.get(saved.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix('=')).unwrap());
        }
        assert!(fresh.o.motion.perspective && fresh.group && fresh.o.style.custom && !fresh.o.text_follow.rotation);
        assert_eq!((fresh.o.search, fresh.o.color, fresh.o.style.font.as_str()), (77., [1, 2, 3], "Noto Sans"));
        assert!(fresh.o.channel == Channel::G && fresh.o.cut == Cut::Outside);
        assert_eq!((fresh.o.gradients.len(), fresh.o.gradients[0].color, fresh.o.gradients[0].angle), (1, [9, 8, 7], 45.));
    }

    #[test]
    fn diagnostic_describes_the_session() {
        let src = std::env::temp_dir().join(format!("aegisigntrack-test-{}-diag.mkv", std::process::id()));
        let made = std::process::Command::new("ffmpeg").args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc=s=64x48:r=25:d=1", "-pix_fmt", "yuv420p"]).arg(&src).status().unwrap();
        assert!(made.success());
        let info = video::probe(&src).unwrap();
        let mut loading = video::extract(&src, &info, 0., 5, Matrix::Auto).unwrap();
        let clip = loop {
            match loading.poll() {
                Some(r) => break r.unwrap(),
                None => std::thread::sleep(std::time::Duration::from_millis(20)),
            }
        };
        let mut app = App::new(None, None, None);
        app.source = Some((src.clone(), info, 0., 5));
        app.loaded(clip, info, 0., false);
        std::fs::remove_file(&src).unwrap();
        app.o.shape = vec![[1., 2.], [30., 2.], [30., 20.]];
        app.o.frames[3] = Some(FrameData { h: track::translation(2., 0.), pts: vec![], opacity: 0.5, guessed: true, score: 0. });
        let text = app.diagnostic();
        for part in ["AegiSignTrack 20", "ffmpeg : ffmpeg version", "plage chargée : 64x48, 5 frames", "sélection : [[1.0, 2.0], [30.0, 2.0], [30.0, 20.0]]", "\n3 1.000000 0.000000 2.000000 0.000000 1.000000 0.000000 0.000000 0.000000 1.000000 0.500 true\n"] {
            assert!(text.contains(part), "« {part} » absent de :\n{text}");
        }
        assert!(!text.contains("0000000000000000"), "empreintes d'images vides :\n{text}");
    }
}
