use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

#[derive(Clone, Copy, Debug)]
pub struct Info {
    pub w: usize,
    pub h: usize,
    pub fps: f64,
    pub duration: f64,
    hd_untagged: bool,
    lead: f64,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Matrix {
    Auto,
    Bt709,
    Bt601,
}

static DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn set_dir(dir: Option<PathBuf>) {
    *DIR.lock().unwrap() = dir;
}

pub fn missing() -> Option<String> {
    let lost: Vec<&str> = ["ffmpeg", "ffprobe"].into_iter().filter(|b| cmd(b).arg("-version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_err()).collect();
    (!lost.is_empty()).then(|| tf!("{} introuvable : installez ffmpeg, placez ffmpeg et ffprobe à côté d'AegiSignTrack, ou indiquez leur dossier avec « Dossier de ffmpeg... » en bas des réglages.", "{} not found: install ffmpeg, put ffmpeg and ffprobe next to AegiSignTrack, or give their folder with “ffmpeg folder...” at the bottom of the settings.", lost.join(t!(" et ", " and "))))
}

pub fn cmd(bin: &str) -> Command {
    let name = format!("{bin}{}", std::env::consts::EXE_SUFFIX);
    let chosen = DIR.lock().unwrap().as_ref().map(|d| d.join(&name));
    let beside = std::env::current_exe().ok().and_then(|p| Some(p.parent()?.join(&name)));
    let mut c = Command::new(chosen.into_iter().chain(beside).find(|p| p.is_file()).unwrap_or_else(|| PathBuf::from(bin)));
    c.stdin(Stdio::null());
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut c, 0x0800_0000);
    c
}

pub fn probe(path: &Path) -> Result<Info, String> {
    let out = cmd("ffprobe")
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height,r_frame_rate,color_space,start_time:format=duration,start_time", "-of", "default=noprint_wrappers=1"])
        .arg(path)
        .output()
        .map_err(|e| tf!("ffprobe introuvable (installez ffmpeg et ajoutez-le au PATH) : {e}", "ffprobe not found (install ffmpeg and add it to the PATH): {e}"))?;
    if !out.status.success() {
        return Err(tf!("Vidéo illisible : {}", "Video unreadable: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let get = |key: &str| text.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix('='));
    let int = |key: &str| get(key).and_then(|v| v.parse::<usize>().ok()).filter(|v| *v > 0);
    let (num, den) = get("r_frame_rate").and_then(|v| v.split_once('/')).unwrap_or(("25", "1"));
    let fps = num.parse::<f64>().unwrap_or(25.) / den.parse::<f64>().unwrap_or(1.).max(1e-9);
    let starts: Vec<f64> = text.lines().filter_map(|l| l.strip_prefix("start_time=")).map(|v| v.parse().unwrap_or(0.)).collect();
    let (w, h) = (int("width").ok_or(t!("Aucune piste vidéo dans ce fichier", "No video stream in this file"))?, int("height").ok_or(t!("Aucune piste vidéo dans ce fichier", "No video stream in this file"))?);
    Ok(Info {
        w,
        h,
        hd_untagged: h >= 720 && get("color_space").is_none_or(|c| c == "unknown"),
        fps: if fps.is_finite() && fps > 0. { fps } else { 25. },
        duration: get("duration").and_then(|v| v.parse().ok()).unwrap_or(0.),
        lead: if starts.len() == 2 { (starts[0] - starts[1]).max(0.) } else { 0. },
    })
}

struct Temp(PathBuf);

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn scratch() -> PathBuf {
    let cache = std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).or_else(|| Some(PathBuf::from(std::env::var_os("HOME")?).join(".cache")));
    match cache.filter(|_| cfg!(target_os = "linux")).map(|c| c.join("aegisigntrack")) {
        Some(dir) if std::fs::create_dir_all(&dir).is_ok() => dir,
        _ => std::env::temp_dir(),
    }
}

pub fn clean_stale() {
    for e in [scratch(), std::env::temp_dir()].iter().flat_map(std::fs::read_dir).flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with("aegisigntrack-") && (name.ends_with(".rgb") || name.ends_with(".ass")) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

pub struct Clip {
    file: Mutex<File>,
    _tmp: Temp,
    pub w: usize,
    pub h: usize,
    pub n: usize,
}

impl Clip {
    pub fn average(&self, i: usize, at: [f32; 2]) -> Option<[u8; 3]> {
        let (x, y) = (at[0].round() as i64, at[1].round() as i64);
        if i >= self.n || x < 0 || y < 0 || x >= self.w as i64 || y >= self.h as i64 {
            return None;
        }
        let (x0, x1) = ((x - 1).max(0) as usize, (x + 1).min(self.w as i64 - 1) as usize);
        let (mut sum, mut n) = ([0u32; 3], 0);
        let mut f = self.file.lock().unwrap();
        for row in (y - 1).max(0) as usize..=(y + 1).min(self.h as i64 - 1) as usize {
            let mut buf = vec![0; (x1 - x0 + 1) * 3];
            f.seek(SeekFrom::Start(((i * self.h + row) * self.w + x0) as u64 * 3)).ok()?;
            f.read_exact(&mut buf).ok()?;
            for px in buf.chunks_exact(3) {
                (0..3).for_each(|c| sum[c] += px[c] as u32);
                n += 1;
            }
        }
        Some(sum.map(|s| ((s + n / 2) / n) as u8))
    }

    pub fn rgb(&self, i: usize) -> std::io::Result<Vec<u8>> {
        let size = self.w * self.h * 3;
        let mut buf = vec![0; size];
        let mut f = self.file.lock().unwrap();
        f.seek(SeekFrom::Start(i as u64 * size as u64))?;
        f.read_exact(&mut buf)?;
        Ok(buf)
    }
}

pub struct Loading {
    child: Child,
    tmp: Option<Temp>,
    pub w: usize,
    pub h: usize,
    expect: usize,
}

impl Drop for Loading {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

const BUDGET: u64 = 16 << 30;

pub fn extract(src: &Path, info: &Info, start: f64, frames: usize, matrix: Matrix) -> Result<Loading, String> {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let mut div = 1;
    while (info.w / div * (info.h / div) * 3 * frames) as u64 > BUDGET && div < 8 {
        div *= 2;
    }
    let (w, h) = (info.w / div, info.h / div);
    let path = scratch().join(format!("aegisigntrack-{}-{}.rgb", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed)));
    let matrix = match matrix {
        Matrix::Bt709 => ":in_color_matrix=bt709",
        Matrix::Bt601 => ":in_color_matrix=bt601",
        Matrix::Auto if info.hd_untagged => ":in_color_matrix=bt709",
        Matrix::Auto => "",
    };
    let child = cmd("ffmpeg")
        .args(["-v", "error", "-y", "-ss", &format!("{:.4}", start.max(0.) + info.lead)])
        .arg("-i")
        .arg(src)
        .args(["-map", "0:v:0", "-frames:v", &frames.to_string(), "-vf", &format!("scale={w}:{h}{matrix}"), "-fps_mode", "passthrough", "-pix_fmt", "rgb24", "-f", "rawvideo"])
        .arg(&path)
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| tf!("ffmpeg introuvable (installez ffmpeg et ajoutez-le au PATH) : {e}", "ffmpeg not found (install ffmpeg and add it to the PATH): {e}"))?;
    Ok(Loading { child, tmp: Some(Temp(path)), w, h, expect: frames })
}

impl Loading {
    fn path(&self) -> &Path {
        &self.tmp.as_ref().unwrap().0
    }

    pub fn progress(&self) -> f32 {
        let done = std::fs::metadata(self.path()).map_or(0, |m| m.len()) / (self.w * self.h * 3) as u64;
        done as f32 / self.expect.max(1) as f32
    }

    pub fn poll(&mut self) -> Option<Result<Clip, String>> {
        let status = match self.child.try_wait() {
            Ok(None) => return None,
            Ok(Some(s)) => s,
            Err(e) => return Some(Err(e.to_string())),
        };
        let mut err = String::new();
        if let Some(mut e) = self.child.stderr.take() {
            let _ = e.read_to_string(&mut err);
        }
        let open = || -> std::io::Result<(File, usize)> {
            let f = File::open(self.path())?;
            let n = f.metadata()?.len() / (self.w * self.h * 3) as u64;
            Ok((f, n as usize))
        };
        Some(match open() {
            Ok((file, n)) if n > 0 => Ok(Clip { file: Mutex::new(file), _tmp: self.tmp.take().unwrap(), w: self.w, h: self.h, n }),
            _ if !status.success() => Err(tf!("ffmpeg a échoué : {}", "ffmpeg failed: {}", err.trim())),
            _ => Err(t!("Aucune image dans cette plage de la vidéo", "No frame in this range of the video").into()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_exactly_the_requested_frames() {
        for (delay, name) in [("0", "a"), ("0.08", "b")] {
            let src = std::env::temp_dir().join(format!("aegisigntrack-test-{}-{name}.mkv", std::process::id()));
            let ok = Command::new("ffmpeg")
                .args(["-v", "error", "-y", "-itsoffset", delay, "-f", "lavfi", "-i", "color=black:s=64x48:r=25:d=2,geq=lum='4*N':cb=128:cr=128", "-f", "lavfi", "-i", "sine=d=2"])
                .args(["-c:v", "libx264", "-qp", "0", "-pix_fmt", "yuv420p", "-c:a", "aac"])
                .arg(&src)
                .status()
                .unwrap()
                .success();
            assert!(ok);
            let info = probe(&src).unwrap();
            assert_eq!((info.w, info.h, info.fps.round() as i32), (64, 48, 25));
            let mut loading = extract(&src, &info, 0.7996, 10, Matrix::Auto).unwrap();
            let clip = loop {
                match loading.poll() {
                    Some(r) => break r.unwrap(),
                    None => std::thread::sleep(std::time::Duration::from_millis(20)),
                }
            };
            std::fs::remove_file(&src).unwrap();
            assert_eq!((clip.w, clip.h, clip.n), (64, 48, 10));
            let lum = |i: usize| clip.rgb(i).unwrap()[0] as f32;
            assert_eq!(clip.average(9, [20., 20.]).map(|c| c[0] as f32), Some(lum(9)));
            assert_eq!(clip.average(9, [64., 20.]), None);
            assert!((lum(0) - (80. - 16.) * 255. / 219.).abs() < 2., "piste vidéo décalée de {delay} s, première image : {}", lum(0));
            assert!((lum(9) - (116. - 16.) * 255. / 219.).abs() < 2., "piste vidéo décalée de {delay} s, dernière image : {}", lum(9));
        }
    }
}
