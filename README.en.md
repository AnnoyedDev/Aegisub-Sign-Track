# AegiSignTrack

**Français : [README.md](README.md)**

A motion tracker built for Aegisub typesetting. Point it at something in the video, let it follow that
thing, and get back the ASS lines: a mask to cover the original sign, your own text riding on top, or just
the raw motion applied to lines you already wrote. Tracking, masking and typesetting happen in one window
instead of three tools.

It comes in two parts: the `aegisigntrack` program (Rust, Linux and Windows) and a small Aegisub plugin,
`aegisub/aegisigntrack.lua`, which sends your selection to the program and brings the result back.

Made by AnnoyedDev.

## What it does

- Tracks one point, several points or a whole area: position, scale, rotation, shear or full perspective.
- Copes with fades, with objects that get hidden or leave the picture, and with flat shapes that have no
  texture at all.
- Builds a mask that follows the track: flat colour, gradients, or a picture turned into vector shapes
  (pictures can make a script enormous, you have been warned).
- Places text with its own font, colours, outline, shadow and blur, in true perspective if needed, or
  moves the text of the lines selected in Aegisub.
- Exports to the clipboard, straight into Aegisub, to an `.ass` file, or as tracking data for other tools.
- Several objects can be tracked in the same shot and exported together.

## Install

1. Get the latest [release](https://github.com/AnnoyedDev/Aegisub-Sign-Track/releases). Each archive holds
   the program and the plugin. A release is published for every commit; its name (date and commit) is the
   version shown in the window title.
2. Make sure `ffmpeg` and `ffprobe` can be found: in the PATH, next to the program, or in a folder you
   pick with *ffmpeg folder...* at the bottom of the settings.
3. Copy `aegisigntrack.lua` into Aegisub's `automation/autoload` folder
   (`~/.aegisub/automation/autoload/` on Linux, `%APPDATA%\Aegisub\automation\autoload\` on Windows).
4. If the program is not in the PATH, tell Aegisub where it is: *Automation > AegiSignTrack > Tracker
   path*.

The interface follows the system language (French or English) and can be switched in *Options > Langue /
Language*. The plugin picks up that choice the next time Aegisub starts.

At startup the program checks GitHub for a newer release and, if there is one, shows the installed and
the new version with a link to the download page. Nothing installs itself. This needs `curl` (shipped
with Windows 10 and 11) and can be turned off in *Options*.

### Building it yourself

```sh
cargo build --release
```

The binary lands in `target/release/`. To cross-compile for Windows from Linux, add the
`x86_64-pc-windows-gnu` target with rustup, install mingw-w64, and build with
`--target x86_64-pc-windows-gnu`.

## A first track

From Aegisub, select the line or lines of the sign and run *Automation > AegiSignTrack > Track
selection*. The tracker opens on the part of the video those lines cover. Without Aegisub, start the
program, open or drop a video and choose the range to load.

The panel on the right has three tabs, and a click on the video acts on the tab that is showing.

**Tracking.** Pick *Points* (each point followed on its own) or *Area* (a polygon whose whole surface is
followed), click on the video to draw it, tick the kinds of motion you expect, then press *Track ▶*,
*◀ Track* or *◀ All ▶* from the displayed frame. `[` and `]` set the range. When the track goes wrong on
a frame, go there, drag the selection where it belongs and track again from that point.

**Mask.** Tick *Export the mask*. It starts as the tracked selection; click on the video to draw another
shape anywhere. Choose the colour (there is an eyedropper), the blur, a margin, gradients or an image.

**Text.** Tick *Export the text* and click where it should sit. *Customise the appearance* replaces the
style of the line with your own font and colours.

When you are done, *Send to Aegisub* inserts the lines into the script (one Ctrl+Z undoes it) and
*Copy for Aegisub* puts them on the clipboard. With neither mask nor text ticked, you get the motion
only, after a confirmation.

If the selected lines contain text, the tracker asks at startup whether to import it. Imported text keeps
its own style and timing and each line is replaced by its tracked versions. Otherwise the lines only mark
out the video, and whatever you type in the Text tab is added next to them.

Mask and text each have a list, *What it follows from the tracking*: movement, size, rotation,
perspective, opacity. A text can follow the movement only while the mask follows everything.

Keys: arrows for previous and next frame, Space to play, Ctrl+Z and Ctrl+Shift+Z to undo and redo, H to
hide or show the overlays, wheel to zoom, right button held to move the view.

## When tracking gets difficult

| Situation | What to do |
|---|---|
| The object pops in suddenly | Go to a frame where it is fully visible, draw the area, track backwards |
| Flat colours, no detail | Use *Area* and draw it on the edges of the object; the *Automatic* channel finds the colour mix where it stands out |
| It squashes, stretches or is seen at an angle | Tick *Shear* or *Perspective* before tracking |
| It fades in or out | Tick *Opacity* under *Other tracking* before tracking, starting from a frame where it is clearly visible |
| It goes behind something or leaves the picture | Keep *Realign on the start frame* ticked: the track coasts along its path and picks the object up again. Those frames are orange on the timeline |
| The track slowly drifts | *Realign on the start frame* prevents it; *Refine* fixes an existing track |
| Part of the area blinks or is animated | *Area to ignore*, drawn before tracking |
| The sign leaves the picture at the end | Set the range, then *Advanced > Extrapolate* |
| Something slides over it (a page being turned, a shutter, someone walking by) | Set *Covering* under *Other tracking*: the mask and the text are cut along the covering edge, then dropped while fully covered. *Lasting* keeps it gone even if a look-alike shows up at the same place |
| It only ever moves sideways, or only up and down | Set *Axis* to *Horizontal* or *Vertical*: the other axis is frozen, jitter included. Wrong if the object does move both ways |
| The shot changes | Tracking stops at Aegisub keyframes (white marks on the timeline) |

On the timeline, green frames are tracked, orange ones are estimates (object lost or hidden, position
carried on) and grey ones are frames where the object is covered. Green and orange are exported, grey is
not. Orange frames deserve a look. *◀ estimated* and *estimated ▶* jump between them. Fix
one by hand in the middle of a run, then *Interpolate* draws a steady passage between the reliable
frames around it.

To judge a track, the *View* menu offers *Lock the view on the object* (it stays put on screen, so any
slip is obvious), a confidence curve above the timeline, a magnifier for placing points to the pixel,
looped playback over the range, and a way to hide every overlay to compare with the untouched picture.

## Going further

- **Tracking curves** (*Advanced*): position, size and rotation over time. Smooth one of them alone, or
  type exact values for the displayed frame.
- **Tracking made elsewhere** (*Advanced > Import tracking data...*): reads "After Effects Keyframe Data"
  from Mocha, After Effects or Blender, including Power Pin corners, and lets you use masks and text on it.
- **Tracking for other tools** (*Options*): *Tracking data* for Aegisub-Motion, *Power Pin data* for
  perspective scripts, *Save as .ass...* for a complete file.
- **Text that deforms with the object**: with *Perspective and distortion* ticked in what the text
  follows, the export writes separate `\fscx` and `\fscy`, then `\frx`, `\fry`, `\fax` and `\org`, so the
  text squashes, stretches and tilts like the object. The tracking has to have followed that, which means
  *Shear* or *Perspective* in the Tracking tab. The preview is rendered by libass, during playback too, so
  it shows what Aegisub will show.
- **Text behind a character**: draw the mask over the character and pick *Hidden by the mask* in the Text
  tab. The mask itself does not have to be exported.
- **Blending in**: *Measure* reads how soft the edges of the object are and sets the blur to match.
  *Blur the inside of the letters too* exports outlined text as two stacked lines, because in ASS a blur
  only softens the outside of an outline.
- **A picture on a billboard**: with a 4-point mask, *Place the image on the 4 points of the mask* lays
  the picture down in perspective. SVG files keep their shapes as they are.
- **Changing light**: *Pick the colour again on every frame* reads the mask colour from the video each
  frame.
- **Covering** works by comparing the colours of the area with those of the start frame. It needs Area
  mode, assumes a straight covering edge, and mistakes fades or changing light for a cover. *Tolerance*
  sets how different a colour must be. The outline of the area shows the part still visible.
- **What the tracker sees**: the *View* button next to *Channel* shows the video in greyscale for the
  chosen channel.
- **Layers**: *Layer +* in the Mask and Text tabs is added to the layer of the source line. With 0 for
  the mask and 1 for the text, the text stays in front whatever happens to the order of the lines.
- **Fewer lines**: still frames are merged into one line; *Group steady motion* turns regular movement of
  text into `\move` and `\t`.
- **Sessions**: settings are remembered per script. Objects, areas, masks and tracking are saved too:
  reopen the same video on the same range and you can resume or start over. Mask images are not kept.

## Good to know

A gradient adds one line per band and per frame; a picture adds one line per colour and per frame. Pictures
are reduced to 200 px and a handful of colours first, and the result can still weigh a lot.

If colours look wrong, force BT.709 or BT.601 with *Colour matrix*.

If two machines give different results, compare the version in the window title first, then save
*Options > Diagnostic...* on both and compare the files.

## Tests

```sh
cargo test --release
```

They cover tracking, ASS export and video extraction, and need ffmpeg.

## Licence

CC BY-NC-SA 4.0 - Copyright 2026 AnnoyedDev. No commercial use; modified versions that are distributed
must carry the same licence. Full text in `LICENSE`.
