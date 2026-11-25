#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[macro_use]
mod lang;
mod app;
mod cover;
mod export;
mod picture;
mod track;
mod video;

use eframe::egui;
use std::path::PathBuf;

pub struct Job {
    pub video: PathBuf,
    pub result: PathBuf,
    pub res: [f64; 2],
    pub times: Vec<i64>,
    pub lines: Vec<export::SrcLine>,
    pub styles: Vec<String>,
    pub script: String,
    pub keyframes: Vec<usize>,
}

fn parse_job(text: &str) -> Option<Job> {
    let (mut video, mut result, mut res, mut times, mut lines, mut styles, mut script, mut keyframes) = (None, None, [0f64; 2], vec![], vec![], vec![], String::new(), vec![]);
    for l in text.lines() {
        let Some((key, v)) = l.split_once('=') else { continue };
        match key {
            "video" => video = Some(PathBuf::from(v)),
            "result" => result = Some(PathBuf::from(v)),
            "res_x" => res[0] = v.parse().ok()?,
            "res_y" => res[1] = v.parse().ok()?,
            "times" => times = v.split(',').map(|t| t.parse::<f64>().map(|t| t as i64)).collect::<Result<_, _>>().ok()?,
            "line" => lines.push(export::SrcLine::parse(v)?),
            "style" => styles.push(v.to_string()),
            "script" => script = v.to_string(),
            "keyframes" => keyframes = v.split(',').filter_map(|k| k.parse().ok()).collect(),
            "box" => {
                let f: Vec<f64> = v.split('\t').filter_map(|t| t.parse().ok()).collect();
                if let (&[index, align, font], true) = (f.as_slice(), f.len() == 3) {
                    lines.iter_mut().filter(|l| l.index == index as usize).for_each(|l| l.set_box(align as u8, font));
                }
            }
            _ => {}
        }
    }
    lines.iter_mut().for_each(|l| l.set_edge(&styles));
    (times.len() >= 2 && !lines.is_empty() && res[0] > 0. && res[1] > 0.).then_some(Job { video: video?, result: result?, res, times, lines, styles, script, keyframes })
}

fn main() -> eframe::Result {
    app::language();
    video::clean_stale();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mut job, mut open, mut error) = (None, None, None);
    match args.as_slice() {
        [flag, path] if flag == "--job" => match std::fs::read_to_string(path).ok().as_deref().and_then(parse_job) {
            Some(j) => job = Some(j),
            None => error = Some(tf!("Fichier de travail Aegisub illisible : {path}", "Unreadable Aegisub job file: {path}")),
        },
        [path] => open = Some(PathBuf::from(path)),
        _ => {}
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1280., 800.]).with_title(concat!("AegiSignTrack ", env!("AEGISIGNTRACK_BUILD"))),
        ..Default::default()
    };
    let start = move |cc: &eframe::CreationContext| -> Result<Box<dyn eframe::App>, Box<dyn std::error::Error + Send + Sync>> {
        cc.egui_ctx.style_mut_of(egui::Theme::Light, |s| s.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1., egui::Color32::from_gray(170)));
        Ok(Box::new(app::App::new(job, open, error)))
    };
    eframe::run_native("AegiSignTrack", options, Box::new(start))
}

#[cfg(test)]
mod tests {
    #[test]
    fn job_file_round_trip() {
        let job = super::parse_job("video=/a/b=c.mkv\nresult=/tmp/r.txt\nres_x=1920\nres_y=1080\ntimes=1001,1043,1084\nstyle=Style: Sign,Arial,40\nline=12\t0\t2\t100\t100\t0\t2\t0\tDialogue: 0,0:00:01.00,0:00:01.08,Default,,0,0,0,,{\\pos(1,2)}a, b\nbox=12\t8\t40\n").unwrap();
        assert_eq!(job.video.to_str(), Some("/a/b=c.mkv"));
        assert_eq!((job.res, job.times.as_slice()), ([1920., 1080.], &[1001, 1043, 1084][..]));
        assert_eq!((job.lines[0].index, job.lines[0].text.as_str()), (12, "{\\pos(1,2)}a, b"));
        assert_eq!(job.styles, ["Style: Sign,Arial,40"]);
        assert!(super::parse_job("video=x\nresult=y\nres_x=1\nres_y=1\ntimes=5\n").is_none());
    }
}
