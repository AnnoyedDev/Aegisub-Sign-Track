use crate::export::{self, Export, Follow, Gradient, OutLine, SrcLine};
use crate::picture::{self, Picture};
use crate::track::{self, Channel, FrameData, Gray, Motion, Params, P};
use crate::video::{self, Clip, Info, Loading, Matrix};
use crate::Job;
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

pub struct App {
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

    zone: bool,
    shape: Vec<P>,
    mask: Vec<P>,
    anchor: Option<P>,
    mode: Mode,
    drag: Option<Drag>,

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
    shown: Vec<Option<FrameData>>,
    dirty: bool,
    range: [usize; 2],
    worker: Option<Worker>,

    mask_on: bool,
    text_on: bool,
    confirm: Option<bool>,
    gradients: Vec<Gradient>,
    bands: usize,
    image: Option<(Vec<u8>, usize, usize)>,
    image_colors: usize,
    picture: Option<(Picture, egui::TextureHandle)>,
    mask_follow: Follow,
    text_follow: Follow,
    style: TextStyle,
    style_name: String,
    fonts: Vec<String>,
    color: [u8; 3],
    blur: f32,
    opacity: bool,
    mask_opacity: bool,
    text_opacity: bool,
    import: bool,
    ask_import: bool,
    image_ask: Option<((Vec<u8>, usize, usize), usize)>,
    timestamps: bool,
    merge: bool,
    picking: Option<Pick>,
    realign: bool,
    ref_frame: usize,
    mix: [f32; 3],
    text: String,
    status: String,
    undo: Vec<Snap>,
    redo: Vec<Snap>,
}

const ACCENT: Color32 = Color32::from_rgb(255, 196, 0);
const HIT: f32 = 10.;

fn add(p: P, d: P) -> P {
    [p[0] + d[0], p[1] + d[1]]
}

impl App {
    pub fn new(job: Option<Job>, open: Option<PathBuf>, error: Option<String>) -> Self {
        let mut app = App {
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
            zone: true,
            shape: vec![],
            mask: vec![],
            anchor: None,
            mode: Mode::Track,
            drag: None,
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
            shown: vec![],
            dirty: false,
            range: [0, 0],
            worker: None,
            mask_on: false,
            text_on: false,
            confirm: None,
            gradients: vec![],
            bands: 16,
            image: None,
            image_colors: 8,
            picture: None,
            mask_follow: Follow::ALL,
            text_follow: Follow::ALL,
            style: TextStyle { custom: false, font: "Arial".into(), size: 60., bold: false, italic: false, color: [255, 255, 255], outline_color: [0, 0, 0], outline: 2., shadow_color: [0, 0, 0], shadow: 0., blur: 0. },
            style_name: "Default".into(),
            fonts: vec![],
            color: [0, 0, 0],
            blur: 0.,
            opacity: false,
            mask_opacity: false,
            text_opacity: false,
            import: false,
            ask_import: false,
            image_ask: None,
            timestamps: true,
            merge: true,
            picking: None,
            realign: true,
            ref_frame: 0,
            mix: [0.299, 0.587, 0.114],
            text: "Texte".into(),
            status: error.unwrap_or_else(|| "Ouvrez une vidéo, ou glissez-la dans la fenêtre.".into()),
            undo: vec![],
            redo: vec![],
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
                self.status = "Extraction de la plage vidéo...".into();
            }
            Err(e) => self.status = e,
        }
    }

    fn loaded(&mut self, clip: Clip, info: Info, start: f64, keep: bool) {
        if keep && self.frames.len() == clip.n {
            self.tex = None;
            self.status = "Vidéo rechargée avec la nouvelle matrice de couleur.".into();
            self.clip = Some(Arc::new(clip));
            return;
        }
        self.fps = info.fps;
        (self.times, self.res) = match &self.job {
            Some(j) => (j.times.iter().copied().take(clip.n + 1).collect(), j.res),
            None => ((0..=clip.n).map(|i| ((start + i as f64 / info.fps) * 1000.).round() as i64).collect(), [info.w as f64, info.h as f64]),
        };
        let pos = self.job.as_ref().and_then(|j| export::line_pos(&j.lines[0].text));
        self.anchor = pos.map(|(x, y)| [(x * clip.w as f64 / self.res[0]) as f32, (y * clip.h as f64 / self.res[1]) as f32]);
        (self.shape, self.mask) = (vec![], vec![]);
        (self.undo, self.redo) = (vec![], vec![]);
        self.frames = vec![None; clip.n];
        self.dirty = true;
        (self.cur, self.range, self.tex, self.fit) = (0, [0, clip.n - 1], None, true);
        self.status = format!("{} images chargées ({}×{}). Dessinez la sélection puis lancez le tracking.", clip.n, clip.w, clip.h);
        self.clip = Some(Arc::new(clip));
    }

    fn snap(&self) -> Snap {
        Snap { zone: self.zone, shape: self.shape.clone(), mask: self.mask.clone(), anchor: self.anchor, ref_pts: self.ref_pts.clone(), opa: self.opa.clone(), warp: self.warp, ref_frame: self.ref_frame, mix: self.mix, frames: self.frames.clone() }
    }

    fn checkpoint(&mut self) {
        self.undo.push(self.snap());
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn time_travel(&mut self, back: bool) {
        let popped = if back { self.undo.pop() } else { self.redo.pop() };
        let Some(s) = popped else {
            self.status = if back { "Rien à annuler.".into() } else { "Rien à rétablir.".into() };
            return;
        };
        let now = self.snap();
        if back { self.redo.push(now) } else { self.undo.push(now) }
        (self.zone, self.shape, self.mask, self.anchor, self.ref_pts, self.opa, self.warp, self.frames) = (s.zone, s.shape, s.mask, s.anchor, s.ref_pts, s.opa, s.warp, s.frames);
        self.ref_frame = s.ref_frame;
        self.mix = s.mix;
        self.dirty = true;
        self.status = if back { "Action annulée (Ctrl+Maj+Z pour la rétablir).".into() } else { "Action rétablie.".into() };
    }

    fn tracked(&self) -> bool {
        self.frames.iter().any(Option::is_some)
    }

    fn nearest(&self, i: usize) -> Option<usize> {
        (0..self.frames.len()).filter(|&j| self.frames[j].is_some()).min_by_key(|&j| j.abs_diff(i))
    }

    fn mask_poly(&self) -> &[P] {
        if self.mask.is_empty() { &self.shape } else { &self.mask }
    }

    fn anchor_pt(&self) -> P {
        self.anchor.unwrap_or_else(|| {
            let m = self.mask_poly();
            let n = m.len().max(1) as f32;
            m.iter().fold([0., 0.], |s, p| [s[0] + p[0] / n, s[1] + p[1] / n])
        })
    }

    fn stop(&mut self) {
        if let Some(w) = &self.worker {
            w.stop.store(true, Ordering::Relaxed);
        }
    }

    fn clear_track(&mut self) {
        self.frames.iter_mut().for_each(|f| *f = None);
        self.dirty = true;
    }

    fn start_track(&mut self, dirs: &[i32], ctx: &egui::Context) {
        let Some(clip) = self.clip.clone() else { return };
        if self.shape.is_empty() || (self.zone && self.shape.len() < 3) {
            self.status = if self.zone { "Dessinez d'abord une zone d'au moins 3 points sur la vidéo.".into() } else { "Posez d'abord au moins un point sur la vidéo.".into() };
            return;
        }
        self.checkpoint();
        if !self.tracked() {
            if let Ok(rgb) = clip.rgb(self.cur) {
                self.mix = track::contrast(&rgb, clip.w, clip.h, &self.shape, self.pattern as f32);
            }
        }
        let (p, load) = self.tracker(&clip);
        if !self.tracked() {
            self.ref_frame = self.cur;
            let Some(g) = load(self.cur) else {
                self.status = "Lecture de la vidéo impossible.".into();
                return;
            };
            self.ref_pts = if !self.zone {
                self.shape.clone()
            } else if self.mesh {
                track::mesh_nodes(&self.shape, self.mesh_size)
            } else {
                track::detect(&g[0], &self.shape)
            };
            if self.ref_pts.is_empty() {
                self.status = "Rien à suivre : la zone est hors de l'image.".into();
                return;
            }
            self.opa = track::opacity_samples(&g[0], &self.shape, self.pattern);
            self.warp = p.warp;
            self.frames[self.cur] = Some(FrameData { h: track::IDENT, pts: self.ref_pts.clone(), opacity: 1., guessed: false });
        }
        let mut passes = vec![];
        for &d in dirs {
            let (start, end) = if d > 0 {
                ((0..=self.cur).rev().find(|&i| self.frames[i].is_some()), self.range[1])
            } else {
                ((self.cur..clip.n).find(|&i| self.frames[i].is_some()), self.range[0])
            };
            if let Some(s) = start.filter(|&s| if d > 0 { end > s } else { end < s }) {
                passes.push((s, self.frames[s].clone().unwrap(), end));
            }
        }
        self.dirty = true;
        if passes.is_empty() {
            self.status = "Rien à tracker dans ce sens : placez-vous sur une frame déjà trackée, à l'intérieur de la plage.".into();
            return;
        }
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let (ref_pts, opa, stop2, ctx) = (self.ref_pts.clone(), self.opa.clone(), stop.clone(), ctx.clone());
        let zone = if p.warp { vec![] } else { self.shape.clone() };
        let ref_frame = self.ref_frame;
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
        self.status = "Tracking en cours...".into();
    }

    fn tracker(&self, clip: &Arc<Clip>) -> (Params, impl Fn(usize) -> Option<Vec<Gray>> + Send + 'static) {
        let p = Params {
            motion: self.motion,
            search: self.search,
            pattern: self.pattern,
            channel: if self.channel == Channel::Auto { Channel::Mix(self.mix) } else { self.channel },
            user_pts: !self.zone,
            warp: !self.zone || self.mesh,
            hold_fades: self.opacity,
            realign: self.realign,
        };
        let (clip, q) = (clip.clone(), p.clone());
        (p, move |i: usize| clip.rgb(i).ok().map(|rgb| track::pyramid(Gray::from_rgb(&rgb, clip.w, clip.h, q.channel), q.search, q.pattern)))
    }

    fn start_refine(&mut self, ctx: &egui::Context) {
        let Some(clip) = self.clip.clone() else { return };
        let (p, load) = self.tracker(&clip);
        let frames: Vec<(usize, FrameData)> = (self.range[0]..=self.range[1]).filter_map(|i| Some((i, self.frames[i].clone()?))).collect();
        self.checkpoint();
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let (ref_pts, ref_frame, stop2, ctx) = (self.ref_pts.clone(), self.ref_frame, stop.clone(), ctx.clone());
        std::thread::spawn(move || {
            let mut emit = |i, f| {
                let _ = tx.send(Msg::Frame(i, f));
                ctx.request_repaint();
            };
            let err = track::refine(&load, &ref_pts, ref_frame, frames, &p, &stop2, &mut emit).err();
            let _ = tx.send(Msg::Done(err));
            ctx.request_repaint();
        });
        self.worker = Some(Worker { rx, stop, what: "Affinage" });
        self.playing = false;
        self.status = "Affinage en cours...".into();
    }

    fn export(&self) -> Result<(Vec<OutLine>, Vec<SrcLine>), String> {
        let clip = self.clip.as_ref().ok_or("Aucune vidéo chargée.")?;
        if !self.tracked() {
            return Err("Rien à exporter : lancez d'abord le tracking.".into());
        }
        let bare = !self.mask_on && !self.text_on;
        let typed = if bare { "" } else { &self.text };
        let mut lines = match &self.job {
            Some(j) if self.import => j.lines.clone(),
            Some(j) => vec![j.lines[0].with_text(typed)],
            None => vec![SrcLine::standalone(typed, &self.style_name)],
        };
        if !bare {
            lines.iter_mut().for_each(|l| l.text = self.style.apply(&l.text));
        }
        let e = Export {
            frames: &self.shown,
            ref_pts: &self.ref_pts,
            warp: self.warp,
            times: &self.times,
            scale: [self.res[0] / clip.w as f64, self.res[1] / clip.h as f64],
            mask: self.mask_poly(),
            anchor: self.anchor_pt(),
            own_pos: bare,
            add_text: self.job.is_some() && !self.import,
            color: self.color,
            gradients: &self.gradients,
            bands: self.bands,
            picture: self.picture.as_ref().map(|p| &p.0),
            blur: self.blur,
            mask_opacity: self.mask_opacity,
            text_opacity: self.text_opacity,
            mask_follow: self.mask_follow,
            text_follow: self.text_follow,
            merge: self.merge,
        };
        let mut out = vec![];
        if self.mask_on {
            if e.mask.len() < 3 {
                return Err("Le cache a besoin d'au moins 3 points : dessinez-le dans l'onglet Cache.".into());
            }
            out.extend(e.mask_lines(lines[0].index));
        }
        if self.text_on || bare {
            out.extend(e.text_lines(&lines));
        }
        if out.is_empty() { Err("Aucune frame trackée ne tombe dans la durée des lignes.".into()) } else { Ok((out, lines)) }
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
            writeln!(t, "plage chargée : {}x{}, {} frames, empreintes des images : première {:016x}, référence {:016x}, dernière {:016x}", clip.w, clip.h, clip.n, sum(0), sum(self.ref_frame.min(clip.n - 1)), sum(clip.n - 1)).unwrap();
        }
        writeln!(t, "réglages : zone {}, mesh {} ({}), {:?}, recherche {}, motif {}, canal {:?}, mélange {:?}, recaler {}, lissage {}, opacité {}, matrice {:?}, script {:?}", self.zone, self.mesh, self.mesh_size, self.motion, self.search, self.pattern, self.channel, self.mix, self.realign, self.smoothing, self.opacity, self.matrix, self.res).unwrap();
        writeln!(t, "sélection : {:?}\nplage : {:?}, frame de référence {}, {} points suivis", self.shape, self.range, self.ref_frame, self.ref_pts.len()).unwrap();
        writeln!(t, "résultat (frame, matrice 3x3, opacité, estimée) :").unwrap();
        for (i, f) in self.frames.iter().enumerate().filter_map(|(i, f)| Some((i, f.as_ref()?))) {
            writeln!(t, "{i} {} {:.3} {}", f.h.map(|v| format!("{v:.6}")).join(" "), f.opacity, f.guessed).unwrap();
        }
        t
    }

    fn deliver(&mut self, ctx: &egui::Context, send: bool) {
        let done = self.export().and_then(|(out, lines)| match &self.job {
            Some(j) if send => std::fs::write(&j.result, export::result_file(&out)).map(|_| None).map_err(|e| e.to_string()),
            _ => {
                ctx.copy_text(export::clipboard(&out, &lines, self.timestamps));
                Ok(Some(out.len()))
            }
        });
        match done {
            Ok(None) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Ok(Some(n)) => self.status = format!("{n} lignes copiées dans le presse-papiers."),
            Err(e) => self.status = e,
        }
    }

    fn vectorise(&mut self, ctx: &egui::Context) {
        let Some((rgba, w, h)) = &self.image else { return };
        let pic = picture::trace(rgba, *w, *h, self.image_colors);
        let tex = ctx.load_texture("picture", egui::ColorImage::from_rgba_unmultiplied([pic.w, pic.h], &pic.rgba), egui::TextureOptions::NEAREST);
        let points: usize = pic.layers.iter().flat_map(|l| &l.1).map(Vec::len).sum();
        self.status = format!("Image vectorisée : {} couleurs, {points} points par frame.", pic.layers.len());
        (self.picture, self.mask_on) = (Some((pic, tex)), true);
    }

    fn image_dialog(&mut self, ctx: &egui::Context) {
        let Some((_, points)) = &self.image_ask else { return };
        let frames = self.range[1] - self.range[0] + 1;
        let size = *points as f64 * 11. * frames as f64 / 1e6;
        let mut choice = None;
        egui::Window::new("Image à la place du cache").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, [0., 0.]).show(ctx, |ui| {
            ui.set_max_width(460.);
            ui.label("L'image va être vectorisée : réduite à 200 px et à quelques couleurs, puis convertie en formes ASS, une ligne par couleur et par frame.");
            ui.label(format!("Le script peut très vite devenir très lourd dans Aegisub. Pour cette image : {points} points par frame, soit environ {size:.1} Mo pour les {frames} frames de la plage."));
            ui.label("Au moment de copier ou d'envoyer, AegiSignTrack peut ne plus répondre pendant plusieurs secondes, et Aegisub peut ensuite rester bloqué bien plus longtemps, voire plusieurs minutes, si l'image est grande ou détaillée, si elle a beaucoup de couleurs ou si le tracking est long.");
            ui.horizontal(|ui| {
                if ui.button("Utiliser l'image").clicked() {
                    choice = Some(true);
                }
                if ui.button("Annuler").clicked() {
                    choice = Some(false);
                }
            });
        });
        if let (Some(go), Some((img, _))) = (choice, self.image_ask.take_if(|_| choice.is_some())) {
            if go {
                self.image = Some(img);
                self.vectorise(ctx);
            }
        }
    }

    fn import_dialog(&mut self, ctx: &egui::Context) {
        let Some(j) = self.job.as_ref().filter(|_| self.ask_import) else { return };
        let mut choice = None;
        egui::Window::new("Texte des lignes sélectionnées").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, [0., 0.]).show(ctx, |ui| {
            ui.set_max_width(460.);
            ui.label(format!("{} ligne(s) sélectionnée(s) dans Aegisub :", j.lines.len()));
            for l in j.lines.iter().take(8) {
                ui.weak(format!("• {}", export::strip_tags(&l.text)));
            }
            if j.lines.len() > 8 {
                ui.weak(format!("... et {} autres", j.lines.len() - 8));
            }
            ui.label("Importer ce texte ?");
            ui.label("• Importer : chaque ligne garde son texte et sera remplacée par ses versions trackées.");
            ui.label("• Ne pas importer : les lignes ne servent qu'à délimiter la vidéo ; le texte éventuel se saisit dans l'onglet Texte et s'ajoute à côté.");
            ui.weak("Ce choix reste modifiable dans l'onglet Texte.");
            ui.horizontal(|ui| {
                if ui.button("Importer le texte").clicked() {
                    choice = Some(true);
                }
                if ui.button("Ne pas importer").clicked() {
                    choice = Some(false);
                }
            });
        });
        if let Some(go) = choice {
            (self.import, self.ask_import) = (go, false);
        }
    }

    fn confirm_dialog(&mut self, ctx: &egui::Context) {
        let Some(send) = self.confirm else { return };
        let mut choice = None;
        egui::Window::new("Tracking seul").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, [0., 0.]).show(ctx, |ui| {
            ui.label("Ni le cache ni le texte ne sont cochés.");
            ui.label(format!("Seules les informations du tracking (position, taille, rotation) seront {}, rien d'autre.", if send { "envoyées à Aegisub" } else { "copiées" }));
            ui.horizontal(|ui| {
                if ui.button("Continuer").clicked() {
                    choice = Some(true);
                }
                if ui.button("Annuler").clicked() {
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
                        self.frames[i] = Some(f);
                        (self.cur, self.dirty) = (i, true);
                    }
                    Msg::Done(err) => done = Some(err),
                }
            }
        }
        if let Some(err) = done {
            self.worker = None;
            let n = self.frames.iter().flatten().count();
            let guessed = self.frames.iter().flatten().filter(|f| f.guessed).count();
            self.status = match err {
                Some(e) => format!("{what} arrêté — {e}"),
                None if guessed > 0 => format!("{what} terminé : {n} frames suivies, dont {guessed} estimées (objet masqué ou perdu, en orange sur la timeline)."),
                None => format!("{what} terminé : {n} frames suivies."),
            };
        }
        let n = self.clip.as_ref().map_or(0, |c| c.n);
        if n > 0 && self.worker.is_none() && !ctx.egui_wants_keyboard_input() {
            let (redo, undo) = ctx.input_mut(|i| (i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::Z), i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z)));
            if redo || undo {
                self.time_travel(undo);
            }
            let (left, right, space) = ctx.input(|i| (i.key_pressed(egui::Key::ArrowLeft), i.key_pressed(egui::Key::ArrowRight), i.key_pressed(egui::Key::Space)));
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
                if self.cur + 1 < n { self.cur += 1 } else { self.playing = false }
            }
            ctx.request_repaint();
        }
        if self.dirty {
            self.shown = track::smooth(&self.frames, self.smoothing as f32 * 0.5);
            self.dirty = false;
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if self.job.is_none() && ui.button("Ouvrir une vidéo...").clicked() {
                if let Some(p) = rfd::FileDialog::new().set_title("Ouvrir une vidéo").pick_file() {
                    self.open(p);
                }
            }
            if let Some(j) = &self.job {
                ui.strong(format!("Aegisub · {} ligne(s)", j.lines.len()));
            }
            if let Some((l, ..)) = &self.loading {
                ui.add(egui::ProgressBar::new(l.progress()).desired_width(160.).show_percentage());
            }
            ui.label(&self.status);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut ask = None;
                if self.job.is_some() && ui.button("Envoyer à Aegisub").on_hover_text("Insère les lignes dans le script et ferme le tracker").clicked() {
                    ask = Some(true);
                }
                if ui.button("Copier pour Aegisub").on_hover_text("Copie les lignes : collez-les (Ctrl+V) dans la grille d'Aegisub").clicked() {
                    ask = Some(false);
                }
                match ask {
                    Some(send) if !self.mask_on && !self.text_on && self.tracked() => self.confirm = Some(send),
                    Some(send) => self.deliver(ui.ctx(), send),
                    None => {}
                }
                if ui.button("Diagnostic...").on_hover_text("Enregistre un fichier texte décrivant la version, ffmpeg, la vidéo, les réglages et le résultat du tracking.\nÀ comparer entre deux machines quand le tracking ne donne pas la même chose.").clicked() {
                    if let Some(p) = rfd::FileDialog::new().set_title("Enregistrer le diagnostic").set_file_name("aegisigntrack-diagnostic.txt").save_file() {
                        self.status = match std::fs::write(&p, self.diagnostic()) {
                            Ok(()) => format!("Diagnostic enregistré : {}", p.display()),
                            Err(e) => e.to_string(),
                        };
                    }
                }
                ui.checkbox(&mut self.timestamps, "Avec les timestamps").on_hover_text("Décoché : les lignes copiées démarrent à 0:00:00.00");
                ui.checkbox(&mut self.merge, "Fusionner les frames immobiles").on_hover_text("Tant que le cache ou le texte ne bouge pas, une seule ligne couvre toute la durée.\nDécoché : une ligne par frame.");
            });
        });
    }

    fn settings(&mut self, ui: &mut egui::Ui) {
        ui.add_enabled_ui(self.worker.is_none(), |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.mode, Mode::Track, "Tracking").on_hover_text("Choisir ce qui est suivi dans la vidéo");
                ui.selectable_value(&mut self.mode, Mode::Mask, "Cache").on_hover_text("Placer un cache qui suit le tracking");
                ui.selectable_value(&mut self.mode, Mode::Text, "Texte").on_hover_text("Placer un texte qui suit le tracking");
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
            egui::ComboBox::from_label("Matrice couleur").selected_text(name(self.matrix)).show_ui(ui, |ui| {
                for m in [Matrix::Auto, Matrix::Bt709, Matrix::Bt601] {
                    ui.selectable_value(&mut self.matrix, m, name(m));
                }
            })
            .response
            .on_hover_text("Conversion des couleurs de la vidéo (affichage et pipette).\nAuto : celle déclarée dans le fichier, sinon BT.709 en HD et BT.601 en dessous.");
            if self.matrix != before && self.loading.is_none() {
                self.reload(true);
            }
            if self.job.is_none() {
                ui.horizontal(|ui| {
                    ui.label("Script");
                    ui.add(egui::DragValue::new(&mut self.res[0]).range(1.0..=16384.0));
                    ui.label("×");
                    ui.add(egui::DragValue::new(&mut self.res[1]).range(1.0..=16384.0));
                })
                .response
                .on_hover_text("Résolution du script Aegisub (PlayResX × PlayResY)");
            }
        });
    }

    fn track_tab(&mut self, ui: &mut egui::Ui) {
        let tracked = self.tracked();
        ui.strong("Sélection");
        ui.add_enabled_ui(!tracked, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.zone, false, "Points").on_hover_text("1 point ou plus : chaque point est suivi individuellement");
                ui.selectable_value(&mut self.zone, true, "Zone").on_hover_text("Polygone (4 points ou plus) : toute la surface est suivie");
            });
        });
        ui.weak(if tracked { "Glisser : corriger à la main la frame affichée (tracking manuel)." } else { "Clic : ajouter un point. Glisser : déplacer. Clic droit : supprimer. Ctrl+Z : annuler." });
        if ui.button("Effacer la sélection").clicked() {
            self.checkpoint();
            self.shape.clear();
            self.clear_track();
        }

        ui.separator();
        ui.strong("Mouvement suivi");
        let m = &mut self.motion;
        ui.checkbox(&mut m.translation, "Translation");
        if m.shear || m.perspective {
            for name in ["Échelle", "Rotation"] {
                ui.add_enabled(false, egui::Checkbox::new(&mut true, name)).on_disabled_hover_text("Toujours suivie avec le cisaillement ou la perspective, qui la contiennent.");
            }
        } else {
            ui.checkbox(&mut m.scale, "Échelle");
            ui.checkbox(&mut m.rotation, "Rotation");
        }
        ui.checkbox(&mut m.shear, "Cisaillement").on_hover_text("Inclut échelle et rotation. 3 points minimum.");
        ui.checkbox(&mut m.perspective, "Perspective").on_hover_text("Inclut tout le reste. 4 points minimum.");
        ui.add_enabled_ui(!tracked && self.zone, |ui| {
            ui.checkbox(&mut self.mesh, "Mesh").on_hover_text("Grille de points suivis un par un : la sélection se déforme avec la surface");
            if self.mesh {
                ui.add(egui::Slider::new(&mut self.mesh_size, 10.0..=200.0).integer().text("Maille (px)"));
            }
        });

        ui.separator();
        ui.strong("Recherche");
        ui.horizontal(|ui| {
            for (name, v) in [("Petit mouvement", 8.), ("Normal", 24.), ("Grand", 96.)] {
                if ui.selectable_label(self.search == v, name).clicked() {
                    self.search = v;
                }
            }
        });
        ui.add(egui::Slider::new(&mut self.search, 2.0..=300.0).integer().logarithmic(true).text("Zone de recherche (px)")).on_hover_text("Déplacement maximal d'un point entre deux frames");
        ui.add(egui::Slider::new(&mut self.pattern, 3..=40).text("Motif (px)")).on_hover_text("Demi-taille du motif suivi autour de chaque point");
        let name = |c| match c {
            Channel::Luma => "Luminance",
            Channel::R => "Rouge",
            Channel::G => "Vert",
            Channel::B => "Bleu",
            Channel::Auto | Channel::Mix(_) => "Automatique",
        };
        egui::ComboBox::from_label("Canal").selected_text(name(self.channel)).show_ui(ui, |ui| {
            for c in [Channel::Auto, Channel::Luma, Channel::R, Channel::G, Channel::B] {
                ui.selectable_value(&mut self.channel, c, name(c));
            }
        }).response.on_hover_text("Automatique : le mélange de couleurs où la zone ressort le mieux, calculé au départ du tracking");
        ui.checkbox(&mut self.realign, "Recaler sur la frame de départ").on_hover_text("Chaque frame est réalignée sur celle où le tracking a commencé : le suivi ne se décale plus peu à peu.\nÀ décocher si l'objet change beaucoup d'aspect en cours de route.");
        if ui.add(egui::Slider::new(&mut self.smoothing, 0..=10).text("Lissage")).changed() {
            self.dirty = true;
        }
        let fades = ui.checkbox(&mut self.opacity, "Suivre l'opacité").on_hover_text("Fondus : garde la sélection en place quand elle devient invisible, et coche « Opacité » pour le cache et le texte (\\alpha frame par frame).\nÀ cocher avant de lancer le tracking, en partant d'une frame où la zone est bien visible.");
        if fades.changed() {
            (self.mask_opacity, self.text_opacity) = (self.opacity, self.opacity);
        }
    }

    fn follow_ui(ui: &mut egui::Ui, fol: &mut Follow, deform: &str, hint: &str) {
        ui.strong("Ce qu'il suit du tracking");
        ui.checkbox(&mut fol.translation, "Déplacement");
        ui.checkbox(&mut fol.scale, "Agrandissement / rétrécissement");
        ui.checkbox(&mut fol.rotation, "Rotation");
        ui.checkbox(&mut fol.deform, deform).on_hover_text(hint);
    }

    fn color_row(ui: &mut egui::Ui, label: &str, color: &mut [u8; 3], picking: &mut Option<Pick>, me: Pick) {
        ui.label(label);
        ui.color_edit_button_srgb(color);
        if ui.add(egui::Button::new("Pipette").selected(*picking == Some(me))).on_hover_text("Puis cliquer sur la vidéo pour prendre la couleur à cet endroit").clicked() {
            *picking = if *picking == Some(me) { None } else { Some(me) };
        }
    }

    fn mask_tab(&mut self, ui: &mut egui::Ui) {
        ui.checkbox(&mut self.mask_on, "Exporter le cache");
        ui.weak(if self.mask.is_empty() { "Le cache reprend la sélection du tracking. Clic sur la vidéo : en dessiner un autre, où vous voulez." } else { "Clic : ajouter un point. Glisser : déplacer. Clic droit : supprimer." });
        if ui.add_enabled(!self.mask.is_empty(), egui::Button::new("Revenir à la sélection du tracking")).clicked() {
            self.checkpoint();
            self.mask.clear();
        }
        ui.separator();
        ui.strong("Apparence");
        ui.horizontal(|ui| Self::color_row(ui, "Couleur", &mut self.color, &mut self.picking, Pick::Mask));
        ui.add(egui::DragValue::new(&mut self.blur).range(0.0..=20.0).speed(0.1).prefix("Flou "));
        ui.separator();
        ui.strong("Dégradés");
        let mut gone = None;
        for (i, g) in self.gradients.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                Self::color_row(ui, &format!("{}", i + 1), &mut g.color, &mut self.picking, Pick::Gradient(i));
                if ui.button("✕").on_hover_text("Retirer ce dégradé").clicked() {
                    gone = Some(i);
                }
            });
            ui.add(egui::Slider::new(&mut g.angle, 0.0..=360.0).suffix("°").text("Orientation")).on_hover_text("Sens dans lequel la couleur apparaît : 0° vers la droite, 90° vers le bas");
            ui.add(egui::Slider::new(&mut g.force, 0.0..=1.0).custom_formatter(|v, _| format!("{:.0} %", v * 100.)).text("Force")).on_hover_text("Opacité de la couleur au bout du dégradé");
        }
        if let Some(i) = gone {
            self.gradients.remove(i);
            self.picking = None;
        }
        if ui.button("Ajouter un dégradé").on_hover_text("La couleur du dégradé se fond progressivement sur celle du cache. Plusieurs dégradés se superposent.").clicked() {
            self.gradients.push(Gradient { color: [255, 255, 255], angle: 90., force: 1. });
        }
        if !self.gradients.is_empty() {
            ui.add(egui::Slider::new(&mut self.bands, 2..=64).text("Bandes")).on_hover_text("Plus de bandes : dégradé plus fin, mais autant de lignes en plus par frame et par dégradé");
        }
        ui.separator();
        ui.strong("Image");
        ui.horizontal(|ui| {
            if ui.button("Choisir une image...").clicked() {
                if let Some(p) = rfd::FileDialog::new().set_title("Image à mettre à la place du cache").pick_file() {
                    match picture::load(&p) {
                        Ok(img) => {
                            let points = picture::trace(&img.0, img.1, img.2, self.image_colors).layers.iter().flat_map(|l| &l.1).map(Vec::len).sum();
                            self.image_ask = Some((img, points));
                        }
                        Err(e) => self.status = e,
                    }
                }
            }
            if self.image.is_some() && ui.button("Retirer").clicked() {
                (self.image, self.picture) = (None, None);
            }
        });
        if self.image.is_some() && ui.add(egui::Slider::new(&mut self.image_colors, 2..=32).text("Couleurs")).on_hover_text("L'image est réduite à ce nombre de couleurs, une ligne par couleur et par frame").changed() {
            self.vectorise(ui.ctx());
        }
        ui.weak("L'image remplace le cache et remplit le rectangle qui l'entoure. Elle est convertie en formes : préférer un logo ou un dessin simple.");
        ui.separator();
        Self::follow_ui(ui, &mut self.mask_follow, "Perspective et déformation", "Décoché : le cache garde sa forme d'origine, seulement déplacée, agrandie ou tournée.");
        ui.checkbox(&mut self.mask_opacity, "Opacité (fondus)").on_hover_text("Le cache devient transparent en même temps que ce qui est suivi : \\alpha frame par frame.\nPour un objet qui disparaît complètement, cocher aussi « Suivre l'opacité » dans l'onglet Tracking avant de tracker.");
    }

    fn text_tab(&mut self, ui: &mut egui::Ui) {
        ui.checkbox(&mut self.text_on, "Exporter le texte");
        if let Some(j) = &self.job {
            ui.radio_value(&mut self.import, true, format!("Texte importé d'Aegisub ({} ligne(s))", j.lines.len())).on_hover_text("Chaque ligne sélectionnée garde son texte et son style, et est remplacée par ses versions trackées.");
            ui.radio_value(&mut self.import, false, "Texte saisi ici").on_hover_text("Une nouvelle ligne est ajoutée à côté de la sélection, qui n'est pas modifiée. Elle reprend le style de la première ligne sélectionnée.");
        }
        match &self.job {
            Some(j) if self.import => {
                for l in j.lines.iter().take(6) {
                    ui.weak(format!("• {}", export::strip_tags(&l.text)));
                }
                if j.lines.len() > 6 {
                    ui.weak(format!("... et {} autres", j.lines.len() - 6));
                }
            }
            _ => {
                ui.text_edit_singleline(&mut self.text).on_hover_text("Texte de la ligne, tags ASS compris");
            }
        }
        if self.job.is_none() {
            ui.horizontal(|ui| {
                ui.label("Style");
                ui.add(egui::TextEdit::singleline(&mut self.style_name).desired_width(120.));
            })
            .response
            .on_hover_text("Nom du style Aegisub donné aux lignes copiées");
        }
        ui.weak("Clic ou glisser sur la vidéo : placer le texte.");
        ui.separator();
        let st = &mut self.style;
        ui.checkbox(&mut st.custom, "Personnaliser l'apparence").on_hover_text("Décoché : le texte garde le style de la ligne dans Aegisub.\nCoché : les réglages ci-dessous remplacent ce style.");
        ui.add_enabled_ui(st.custom, |ui| {
            if st.custom && self.fonts.is_empty() {
                let mut all = fontique::Collection::new(Default::default());
                self.fonts = all.family_names().map(str::to_owned).collect();
                self.fonts.sort_by_key(|f| f.to_lowercase());
                self.fonts.dedup();
            }
            ui.horizontal(|ui| {
                ui.label("Police");
                ui.add(egui::TextEdit::singleline(&mut st.font).desired_width(120.));
                egui::ComboBox::from_id_salt("fonts").selected_text("Choisir").width(60.).show_ui(ui, |ui| {
                    for f in &self.fonts {
                        if ui.selectable_label(st.font == *f, f).clicked() {
                            st.font = f.clone();
                        }
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut st.size).range(1.0..=1000.0).prefix("Taille "));
                ui.checkbox(&mut st.bold, "Gras");
                ui.checkbox(&mut st.italic, "Italique");
            });
            ui.horizontal(|ui| Self::color_row(ui, "Couleur", &mut st.color, &mut self.picking, Pick::Text));
            ui.horizontal(|ui| {
                Self::color_row(ui, "Contour", &mut st.outline_color, &mut self.picking, Pick::Outline);
                ui.add(egui::DragValue::new(&mut st.outline).range(0.0..=50.0).speed(0.1));
            });
            ui.horizontal(|ui| {
                Self::color_row(ui, "Ombre", &mut st.shadow_color, &mut self.picking, Pick::Shadow);
                ui.add(egui::DragValue::new(&mut st.shadow).range(0.0..=50.0).speed(0.1));
            });
            ui.add(egui::DragValue::new(&mut st.blur).range(0.0..=20.0).speed(0.1).prefix("Flou "));
        });
        ui.separator();
        Self::follow_ui(ui, &mut self.text_follow, "Perspective (étirement)", "Un texte ASS ne peut pas être mis en vraie perspective ici : sa largeur et sa hauteur s'adaptent séparément.\nDécoché : il grossit de la même façon dans les deux sens.");
        ui.checkbox(&mut self.text_opacity, "Opacité (fondus)").on_hover_text("Le texte devient transparent en même temps que ce qui est suivi : \\alpha frame par frame.\nPour un objet qui disparaît complètement, cocher aussi « Suivre l'opacité » dans l'onglet Tracking avant de tracker.");
    }

    fn timeline(&mut self, ui: &mut egui::Ui) {
        let Some(n) = self.clip.as_ref().map(|c| c.n) else { return };
        let busy = self.worker.is_some();
        ui.add_space(4.);
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 20.), Sense::click_and_drag());
        let x = |i: usize| rect.left() + rect.width() * i as f32 / n as f32;
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 2., ui.visuals().extreme_bg_color);
        p.rect_filled(Rect::from_x_y_ranges(x(self.range[0])..=x(self.range[1] + 1), rect.y_range()), 2., ui.visuals().faint_bg_color);
        for (i, f) in self.frames.iter().enumerate().filter_map(|(i, f)| Some((i, f.as_ref()?))) {
            let color = if f.guessed { Color32::from_rgb(230, 140, 40) } else { Color32::from_rgb(70, 170, 90) };
            p.rect_filled(Rect::from_x_y_ranges(x(i)..=x(i + 1), rect.bottom() - 6.0..=rect.bottom()), 0., color);
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
                if ui.button(if self.playing { "⏸" } else { "▶" }).on_hover_text("Lecture (Espace)").clicked() {
                    self.playing = !self.playing;
                }
                if ui.button("▶|").clicked() {
                    self.cur = (self.cur + 1).min(n - 1);
                }
                if ui.button("⏭").clicked() {
                    self.cur = self.range[1];
                }
            });
            ui.monospace(format!("{:>4} / {n}   {:.3} s", self.cur + 1, self.times.get(self.cur).copied().unwrap_or(0) as f64 / 1000.));
            if let Some(f) = self.shown.get(self.cur).and_then(Option::as_ref).filter(|_| self.opacity || self.mask_opacity || self.text_opacity) {
                ui.monospace(format!("opacité {:>3.0} %", 100. * export::relative_opacity(&self.shown, f)));
            }
            ui.separator();
            ui.add_enabled_ui(!busy, |ui| {
                ui.label("Plage").on_hover_text("Passage à tracker dans la vidéo chargée : les boutons Tracker s'arrêtent à ses bornes.\n[ et ] placent le début et la fin sur la frame affichée.");
                let (mut a, mut b) = (self.range[0] + 1, self.range[1] + 1);
                ui.add(egui::DragValue::new(&mut a).range(1..=b));
                if ui.button("[").on_hover_text("Début de plage = frame affichée").clicked() {
                    a = (self.cur + 1).min(b);
                }
                if ui.button("]").on_hover_text("Fin de plage = frame affichée").clicked() {
                    b = (self.cur + 1).max(a);
                }
                ui.add(egui::DragValue::new(&mut b).range(a..=n));
                self.range = [a - 1, b - 1];
            });
            ui.separator();
            if busy {
                ui.spinner();
                if ui.button("⏹ Arrêter").clicked() {
                    self.stop();
                }
            } else {
                let ctx = ui.ctx().clone();
                if ui.button("◀ Tracker").on_hover_text("Vers le début de la plage").clicked() {
                    self.start_track(&[-1], &ctx);
                }
                if ui.button("Tracker ▶").on_hover_text("Vers la fin de la plage").clicked() {
                    self.start_track(&[1], &ctx);
                }
                if ui.button("◀ Tout ▶").on_hover_text("Dans les deux sens depuis la frame affichée").clicked() {
                    self.start_track(&[1, -1], &ctx);
                }
                if ui.add_enabled(self.tracked(), egui::Button::new("Affiner")).on_hover_text("Réaligne chaque frame suivie de la plage sur la frame de départ.\nÀ utiliser quand le suivi s'est décalé peu à peu, ou après une correction à la main.").clicked() {
                    self.start_refine(&ctx);
                }
                if ui.add_enabled(self.tracked(), egui::Button::new("Effacer le tracking")).clicked() {
                    self.checkpoint();
                    self.clear_track();
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Ajuster la vue").on_hover_text("Molette : zoom. Clic droit (ou clic molette) maintenu : déplacer la vue.").clicked() {
                    self.fit = true;
                }
            });
        });
        ui.add_space(2.);
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let rect = resp.rect;
        let Some(clip) = self.clip.clone() else {
            painter.text(rect.center(), Align2::CENTER_CENTER, "Aucune vidéo", FontId::proportional(18.), ui.visuals().weak_text_color());
            return;
        };
        if self.tex.as_ref().map(|t| t.0) != Some(self.cur) {
            if let Ok(rgb) = clip.rgb(self.cur) {
                let img = egui::ColorImage::from_rgb([clip.w, clip.h], &rgb);
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
        let (origin, zoom) = (rect.min + self.pan, self.zoom);
        let scr = |p: P| origin + Vec2::new(p[0], p[1]) * zoom;
        let img = |p: Pos2| -> P { [(p.x - origin.x) / zoom, (p.y - origin.y) / zoom] };

        let near = self.nearest(self.cur);
        let ghost = near.is_some() && near != Some(self.cur);
        let view = View { fd: near.and_then(|i| self.shown[i].clone()), ref_pts: self.ref_pts.clone(), warp: self.warp };
        let tracked = near.is_some();

        let mask_ref: Vec<P> = self.mask_poly().to_vec();
        let (mask_c, mask_fol, text_fol) = (export::centroid(&mask_ref), self.mask_follow, self.text_follow);
        if let Some(pick) = self.picking {
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            }
            let at = resp.interact_pointer_pos().filter(|_| resp.clicked()).map(|pos| img(pos).map(|v| v.round() as i64));
            if let Some([x, y]) = at.filter(|[x, y]| (0..clip.w as i64).contains(x) && (0..clip.h as i64).contains(y)) {
                if let Ok(rgb) = clip.rgb(self.cur) {
                    let (mut sum, mut n) = ([0u32; 3], 0);
                    for v in (y - 1).max(0)..=(y + 1).min(clip.h as i64 - 1) {
                        for u in (x - 1).max(0)..=(x + 1).min(clip.w as i64 - 1) {
                            let i = (v as usize * clip.w + u as usize) * 3;
                            (0..3).for_each(|c| sum[c] += rgb[i + c] as u32);
                            n += 1;
                        }
                    }
                    let color = sum.map(|s| ((s + n / 2) / n) as u8);
                    match pick {
                        Pick::Mask => self.color = color,
                        Pick::Gradient(i) => self.gradients[i].color = color,
                        Pick::Text => self.style.color = color,
                        Pick::Outline => self.style.outline_color = color,
                        Pick::Shadow => self.style.shadow_color = color,
                    }
                    self.picking = None;
                    self.status = format!("Couleur prélevée : #{:02X}{:02X}{:02X}.", color[0], color[1], color[2]);
                }
            }
        } else if self.worker.is_none() {
            let active: Vec<P> = match self.mode {
                Mode::Track => self.shape.iter().map(|v| view.map(*v)).collect(),
                Mode::Mask => self.mask.iter().map(|v| view.place(mask_c, mask_fol, *v)).collect(),
                Mode::Text => vec![],
            };
            let pointer = resp.interact_pointer_pos();
            if self.mode == Mode::Text && (resp.clicked() || resp.drag_started_by(PointerButton::Primary)) {
                self.checkpoint();
                self.text_on = true;
            }
            if let Some(pos) = pointer.filter(|_| self.mode == Mode::Text && (resp.clicked() || resp.dragged_by(PointerButton::Primary))) {
                self.anchor = Some(if text_fol.translation { view.unmap(img(pos)) } else { img(pos) });
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
                        self.shape.push(img(pos));
                    }
                    Mode::Track => self.status = "La sélection est verrouillée : glissez-la pour corriger cette frame, ou effacez le tracking pour la redessiner.".into(),
                    Mode::Mask => {
                        self.checkpoint();
                        self.mask_on = true;
                        let v = if self.mask.is_empty() {
                            if mask_fol.translation { view.unmap(img(pos)) } else { img(pos) }
                        } else {
                            view.unplace(mask_c, mask_fol, img(pos))
                        };
                        self.mask.push(v);
                    }
                    Mode::Text => {}
                }
            }
            if let Some(i) = pointer.filter(|_| resp.secondary_clicked()).and_then(hit) {
                if self.mode == Mode::Mask || !tracked {
                    self.checkpoint();
                }
                match self.mode {
                    Mode::Track if !tracked => drop(self.shape.remove(i)),
                    Mode::Mask => drop(self.mask.remove(i)),
                    _ => {}
                }
            }
        }

        if let Some((_, tex)) = &self.tex {
            painter.image(tex.id(), Rect::from_min_size(origin, size * zoom), Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)), Color32::WHITE);
        }
        if self.picking.is_some() {
            let hint = painter.layout_no_wrap("Pipette : cliquez sur la vidéo pour prendre la couleur".into(), FontId::proportional(15.), Color32::WHITE);
            let at = Pos2::new(rect.center().x - hint.size().x / 2., rect.top() + 10.);
            painter.rect_filled(Rect::from_min_size(at, hint.size()).expand(6.), 4., Color32::from_black_alpha(200));
            painter.galley(at, hint, Color32::WHITE);
        }
        let line = if ghost { Color32::GRAY } else { ACCENT };
        if self.zone {
            for p in view.fd.iter().flat_map(|f| &f.pts) {
                painter.circle_filled(scr(*p), 1.5, Color32::from_rgb(90, 220, 120));
            }
        }
        let mask_ref: Vec<P> = self.mask_poly().to_vec();
        let mask_c = export::centroid(&mask_ref);
        let mask: Vec<Pos2> = mask_ref.iter().map(|v| scr(view.place(mask_c, self.mask_follow, *v))).collect();
        if self.mask_on && mask.len() >= 3 {
            let seen = view.fd.as_ref().filter(|_| self.mask_opacity).map_or(1., |f| export::relative_opacity(&self.shown, f));
            if let Some((_, tex)) = &self.picture {
                let b = mask_ref.iter().fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |b, p| [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])]);
                let mut mesh = egui::Mesh::with_texture(tex.id());
                for k in 0..81u32 {
                    let (u, v) = ((k % 9) as f32 / 8., (k / 9) as f32 / 8.);
                    let pos = scr(view.place(mask_c, self.mask_follow, [b[0] + u * (b[2] - b[0]), b[1] + v * (b[3] - b[1])]));
                    mesh.vertices.push(egui::epaint::Vertex { pos, uv: Pos2::new(u, v), color: Color32::from_white_alpha((255. * seen) as u8) });
                    if k % 9 < 8 && k / 9 < 8 {
                        mesh.indices.extend([k, k + 1, k + 9, k + 1, k + 10, k + 9]);
                    }
                }
                painter.add(Shape::mesh(mesh));
            } else {
                for layer in export::layers(&mask_ref, self.color, &self.gradients, self.bands, None) {
                    let [r, g, b] = layer.color;
                    let pts = layer.loops[0].iter().map(|v| scr(view.place(mask_c, self.mask_follow, *v))).collect();
                    painter.add(Shape::convex_polygon(pts, Color32::from_rgba_unmultiplied(r, g, b, (170. * seen * layer.solid) as u8), Stroke::NONE));
                }
            }
        }
        if !self.mask.is_empty() || self.mode == Mode::Mask {
            painter.add(Shape::closed_line(mask.clone(), Stroke::new(1.5, Color32::from_rgb(80, 200, 255))));
            if !self.mask.is_empty() {
                for v in &mask {
                    painter.circle_filled(*v, 3.5, Color32::from_rgb(80, 200, 255));
                }
            }
        }
        let shape: Vec<Pos2> = self.shape.iter().map(|v| scr(view.map(*v))).collect();
        if self.zone && shape.len() >= 2 {
            painter.add(Shape::closed_line(shape.clone(), Stroke::new(1.5, line)));
        }
        for v in &shape {
            painter.circle_filled(*v, 4., line);
            if !self.zone {
                for (half, alpha) in [(self.pattern as f32, 255), (self.pattern as f32 + self.search, 90)] {
                    painter.rect_stroke(Rect::from_center_size(*v, Vec2::splat(2. * half * zoom)), 0., Stroke::new(1., line.gamma_multiply_u8(alpha)), StrokeKind::Middle);
                }
            }
        }
        if self.text_on && (tracked || self.anchor.is_some() || !self.shape.is_empty()) {
            let (a, fol) = (self.anchor_pt(), self.text_follow);
            let (pos, grow, rot) = match &view.fd {
                Some(f) => {
                    let (p, sx, sy, r) = export::local_motion(f, &view.ref_pts, view.warp, a, 1.);
                    (if fol.translation { p } else { a }, if fol.scale { (sx * sy).sqrt() as f32 } else { 1. }, if fol.rotation { r as f32 } else { 0. })
                }
                None => (a, 1., 0.),
            };
            let c = scr(pos);
            let text = match &self.job {
                Some(j) if self.import => export::strip_tags(&j.lines.iter().find(|l| (l.first..l.end).contains(&self.cur)).unwrap_or(&j.lines[0]).text),
                _ => export::strip_tags(&self.text),
            };
            let st = &self.style;
            let rgb = |c: [u8; 3]| Color32::from_rgb(c[0], c[1], c[2]);
            let (fill, edge) = if st.custom { (rgb(st.color), rgb(st.outline_color)) } else { (Color32::WHITE, Color32::BLACK) };
            let base = if st.custom { st.size * clip.h as f32 / self.res[1] as f32 } else { clip.h as f32 * 0.05 };
            let galley = painter.layout_no_wrap(text, FontId::proportional((base * grow * zoom).clamp(4., 600.)), fill);
            for (dx, dy, color) in [(-2., 0., edge), (2., 0., edge), (0., -2., edge), (0., 2., edge), (0., 0., fill)] {
                let at = c - galley.size() / 2. + Vec2::new(dx, dy);
                painter.add(egui::epaint::TextShape::new(at, galley.clone(), color).with_override_text_color(color).with_angle_and_anchor(rot, Align2::CENTER_CENTER));
            }
            painter.line_segment([c - Vec2::X * 8., c + Vec2::X * 8.], Stroke::new(1.5, Color32::from_rgb(255, 90, 90)));
            painter.line_segment([c - Vec2::Y * 8., c + Vec2::Y * 8.], Stroke::new(1.5, Color32::from_rgb(255, 90, 90)));
        }
    }

    fn apply_drag(&mut self, d: Drag, delta: P, view: &View, tracked: bool) {
        let (c, fol) = (export::centroid(&self.mask), self.mask_follow);
        let moved = |v: &mut P| *v = view.unplace(c, fol, add(view.place(c, fol, *v), delta));
        match (self.mode, d) {
            (Mode::Track, Drag::Vertex(i)) if !tracked => self.shape[i] = add(self.shape[i], delta),
            (Mode::Track, Drag::Whole) if !tracked => self.shape.iter_mut().for_each(|v| *v = add(*v, delta)),
            (Mode::Track, _) => {
                if self.frames[self.cur].is_none() {
                    self.frames[self.cur] = self.nearest(self.cur).and_then(|i| self.frames[i].clone());
                }
                let Some(f) = &mut self.frames[self.cur] else { return };
                f.guessed = false;
                match d {
                    Drag::Vertex(i) if !self.zone => {
                        f.pts[i] = add(f.pts[i], delta);
                        f.h = track::fit(&self.motion, &self.ref_pts, &f.pts, &vec![true; f.pts.len()]).unwrap_or(f.h);
                    }
                    _ => {
                        f.pts.iter_mut().for_each(|p| *p = add(*p, delta));
                        f.h = track::mul(&track::translation(delta[0] as f64, delta[1] as f64), &f.h);
                    }
                }
                self.dirty = true;
            }
            (Mode::Mask, Drag::Vertex(i)) => moved(&mut self.mask[i]),
            (Mode::Mask, Drag::Whole) => self.mask.iter_mut().for_each(moved),
            (Mode::Text, _) => {}
        }
    }

    fn range_dialog(&mut self, ctx: &egui::Context) {
        let Some(p) = &mut self.pending else { return };
        let mut choice = None;
        egui::Window::new("Plage à charger").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, [0., 0.]).show(ctx, |ui| {
            ui.label(p.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
            ui.weak(format!("{}×{} · {:.3} i/s · {:.1} s", p.info.w, p.info.h, p.info.fps, p.info.duration));
            ui.horizontal(|ui| {
                ui.label("Début");
                ui.add(egui::DragValue::new(&mut p.start).range(0.0..=p.info.duration.max(0.)).speed(0.1).suffix(" s"));
                ui.label("Durée");
                ui.add(egui::DragValue::new(&mut p.dur).range(0.1..=3600.0).speed(0.1).suffix(" s"));
            });
            let frames = (p.dur * p.info.fps).ceil() as usize;
            ui.weak(format!("{frames} images extraites dans le dossier temporaire"));
            ui.horizontal(|ui| {
                if ui.button("Charger").clicked() {
                    choice = Some(Some(frames));
                }
                if ui.button("Annuler").clicked() {
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
        self.confirm_dialog(&ctx);
        self.image_dialog(&ctx);
        self.import_dialog(&ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        app.shape = vec![[1., 2.], [30., 2.], [30., 20.]];
        app.frames[3] = Some(FrameData { h: track::translation(2., 0.), pts: vec![], opacity: 0.5, guessed: true });
        let text = app.diagnostic();
        for part in ["AegiSignTrack 20", "ffmpeg : ffmpeg version", "plage chargée : 64x48, 5 frames", "sélection : [[1.0, 2.0], [30.0, 2.0], [30.0, 20.0]]", "\n3 1.000000 0.000000 2.000000 0.000000 1.000000 0.000000 0.000000 0.000000 1.000000 0.500 true\n"] {
            assert!(text.contains(part), "« {part} » absent de :\n{text}");
        }
        assert!(!text.contains("0000000000000000"), "empreintes d'images vides :\n{text}");
    }
}
