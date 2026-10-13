//! Native port of vendor/accessor-mascot/src/bear.mjs (MIT).
//! The supplied 40 × 36 pixels occupy 40 × 18 terminal cells, without Node.

use super::Mood;
use ratatui::{buffer::Buffer, layout::Rect, style::Color, widgets::Widget};
use std::f64::consts::{PI, TAU};

pub const WIDTH: u16 = 40;
pub const HEIGHT: u16 = 18;
pub const FRAME_MS: u64 = 83;
const PIXELS: usize = WIDTH as usize * HEIGHT as usize * 2;

// Index zero is transparent. Keep this order in sync with the reference exporter.
const PALETTE: [Color; 17] = [
    Color::Reset,
    Color::Rgb(112, 74, 56),
    Color::Rgb(216, 152, 99),
    Color::Rgb(247, 201, 154),
    Color::Rgb(179, 110, 72),
    Color::Rgb(255, 227, 188),
    Color::Rgb(41, 30, 33),
    Color::Rgb(237, 155, 137),
    Color::Rgb(127, 227, 191),
    Color::Rgb(57, 158, 136),
    Color::Rgb(237, 240, 220),
    Color::Rgb(245, 166, 160),
    Color::Rgb(239, 203, 118),
    Color::Rgb(16, 23, 24),
    Color::Rgb(23, 33, 34),
    Color::Rgb(52, 69, 70),
    Color::Rgb(148, 168, 166),
];
const OUTLINE: u8 = 1;
const FUR: u8 = 2;
const HIGHLIGHT: u8 = 3;
const SHADE: u8 = 4;
const MUZZLE: u8 = 5;
const INK: u8 = 6;
const BLUSH: u8 = 7;
const MINT: u8 = 8;
const TEAL: u8 = 9;
const CREAM: u8 = 10;
const ROSE: u8 = 11;
const GOLD: u8 = 12;
const SURFACE: u8 = 14;
const BORDER: u8 = 15;
const MUTED: u8 = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Idle,
    Greeting,
    Curious,
    Thinking,
    Working,
    Focused,
    Happy,
    Success,
    Surprised,
    Confused,
    Worried,
    Error,
    Sleepy,
    Sleeping,
    Celebrating,
}
impl State {
    fn from_mood(mood: Mood) -> Self {
        match mood {
            Mood::Asleep | Mood::Locked => Self::Sleeping,
            Mood::Idle => Self::Idle,
            Mood::Listening | Mood::Curious => Self::Curious,
            Mood::Thinking => Self::Thinking,
            Mood::Working => Self::Working,
            Mood::Focused | Mood::Cautious => Self::Focused,
            Mood::Speaking | Mood::Happy => Self::Happy,
            Mood::Proud => Self::Success,
            Mood::Surprised => Self::Surprised,
            Mood::Confused => Self::Confused,
            Mood::Worried => Self::Worried,
            Mood::Sad | Mood::Concerned | Mood::Apologetic | Mood::Alarmed => Self::Error,
            Mood::Sleepy => Self::Sleepy,
            Mood::Delighted | Mood::Excited | Mood::Celebrating => Self::Celebrating,
            Mood::Waving => Self::Greeting,
        }
    }
    fn cycle(self) -> f64 {
        match self {
            Self::Idle => 4000.0,
            Self::Greeting | Self::Surprised => 1600.0,
            Self::Curious | Self::Thinking => 2400.0,
            Self::Working => 1200.0,
            Self::Focused => 2800.0,
            Self::Happy | Self::Celebrating => 1800.0,
            Self::Success | Self::Error => 2000.0,
            Self::Confused | Self::Worried => 2200.0,
            Self::Sleepy => 3200.0,
            Self::Sleeping => 3600.0,
        }
    }
    fn asleep(self) -> bool {
        matches!(self, Self::Sleeping | Self::Sleepy)
    }
}

#[derive(Clone, Copy)]
enum Change {
    Blink,
    Doze,
    Wake,
    Reassure,
}
#[derive(Clone, Copy)]
struct Transition {
    from: State,
    kind: Change,
    start: f64,
    duration: f64,
    from_time: f64,
}

pub struct Animator {
    state: State,
    entered: f64,
    transition: Option<Transition>,
}
impl Animator {
    pub fn new(mood: Mood) -> Self {
        Self {
            state: State::from_mood(mood),
            entered: 0.0,
            transition: None,
        }
    }
    pub fn set_mood(&mut self, mood: Mood, now: f64) {
        let next = State::from_mood(mood);
        if next == self.state {
            return;
        }
        // Interrupt from the pose actually being displayed, including its loop time.
        let (from, from_time, _) = self.sample(now);
        let (kind, duration) = if next.asleep() {
            (Change::Doze, 760.0)
        } else if from.asleep() {
            (Change::Wake, 680.0)
        } else if matches!(next, State::Worried | State::Error) {
            (Change::Reassure, 480.0)
        } else {
            (Change::Blink, 480.0)
        };
        self.state = next;
        self.entered = now;
        self.transition = Some(Transition {
            from,
            kind,
            start: now,
            duration,
            from_time,
        });
    }
    fn sample(&mut self, now: f64) -> (State, f64, Option<(Change, f64)>) {
        if let Some(tr) = self.transition {
            let elapsed = (now - tr.start).max(0.0);
            if elapsed < tr.duration {
                let progress = elapsed / tr.duration;
                let (state, time) = if progress < 0.5 {
                    (tr.from, tr.from_time + elapsed)
                } else {
                    (self.state, elapsed - tr.duration / 2.0)
                };
                return (state, time, Some((tr.kind, progress)));
            }
            self.entered = tr.start + tr.duration / 2.0;
            self.transition = None;
        }
        (self.state, (now - self.entered).max(0.0), None)
    }
    pub fn frame(&mut self, now: f64) -> Bear {
        let (state, time, change) = self.sample(now);
        pixels(state, time, change)
    }
}

pub struct Bear {
    pixels: [u8; PIXELS],
}
impl Widget for &Bear {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < WIDTH || area.height < HEIGHT {
            return;
        }
        let x0 = area.x + (area.width - WIDTH) / 2;
        let y0 = area.y + (area.height - HEIGHT) / 2;
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let top = self.pixels[(y * 2 * WIDTH + x) as usize] as usize;
                let bottom = self.pixels[((y * 2 + 1) * WIDTH + x) as usize] as usize;
                let (symbol, fg, bg) = if top != 0 {
                    ("▀", PALETTE[top], PALETTE[bottom])
                } else if bottom != 0 {
                    ("▄", PALETTE[bottom], Color::Reset)
                } else {
                    (" ", Color::Reset, Color::Reset)
                };
                buf[(x0 + x, y0 + y)]
                    .set_symbol(symbol)
                    .set_fg(fg)
                    .set_bg(bg);
            }
        }
    }
}

#[derive(Default)]
struct Pose {
    x: i32,
    y: i32,
    head_x: i32,
    head_y: i32,
    blink: bool,
    eyes: &'static str,
    mouth: &'static str,
    left: &'static str,
    right: &'static str,
    prop: &'static str,
    phase: f64,
}
fn rounded(value: f64) -> i32 {
    (value + 0.5).floor() as i32
}
fn pose(state: State, time: f64, change: Option<(Change, f64)>) -> Pose {
    let p = time.max(0.0) % state.cycle() / state.cycle();
    let wave = (p * TAU).sin();
    let mut a = Pose {
        blink: p > 0.92 && p < 0.97,
        eyes: "open",
        mouth: "smile",
        left: "rest",
        right: "rest",
        phase: p,
        ..Pose::default()
    };
    match state {
        State::Idle => a.y = if p < 0.5 { 0 } else { -1 },
        State::Greeting => {
            a.right = if wave > 0.0 { "wave-high" } else { "wave-low" };
            if p > 0.8 {
                a.eyes = "happy";
            }
        }
        State::Curious => {
            a.head_x = if wave > 0.25 { 1 } else { -1 };
            a.head_y = if p < 0.4 { -1 } else { 0 };
            a.left = "chin";
            a.prop = "?";
        }
        State::Thinking => {
            a.left = "chin";
            a.eyes = "look";
            a.head_y = i32::from(p > 0.5);
            a.prop = "dots";
        }
        State::Working => {
            a.prop = "laptop";
            let up = p % 0.25 < 0.125;
            a.left = if up { "type-up" } else { "type-down" };
            a.right = if up { "type-down" } else { "type-up" };
            a.eyes = "look-down";
            a.head_y = i32::from(p % 0.5 >= 0.25);
        }
        State::Focused => {
            a.prop = "glasses";
            a.eyes = "look-down";
            a.head_y = i32::from(wave > 0.7);
            a.mouth = "small";
        }
        State::Happy => {
            a.eyes = "happy";
            a.x = if wave > 0.3 {
                1
            } else if wave < -0.3 {
                -1
            } else {
                0
            };
            a.prop = "heart";
        }
        State::Success => {
            a.right = if p < 0.5 { "wave-high" } else { "wave-low" };
            a.eyes = "happy";
            a.head_y = i32::from(p > 0.55 && p < 0.8);
            a.prop = "check";
        }
        State::Surprised => {
            a.eyes = "wide";
            a.mouth = "o";
            a.y = if p < 0.2 {
                -2
            } else if p < 0.35 {
                -1
            } else {
                0
            };
            a.left = "out";
            a.right = "out";
            a.prop = "!";
        }
        State::Confused => {
            a.head_x = if wave > 0.0 { 1 } else { -1 };
            a.eyes = "uneven";
            a.mouth = "slant";
            a.left = "chin";
            a.prop = "?";
        }
        State::Worried => {
            a.eyes = "worried";
            a.mouth = "frown";
            a.left = "tuck";
            a.right = "tuck";
            a.y = i32::from(p >= 0.5);
        }
        State::Error => {
            a.head_x = if p < 0.3 {
                rounded((p / 0.3 * TAU * 2.0).sin())
            } else {
                0
            };
            a.eyes = "sad";
            a.mouth = "frown";
            a.prop = "error";
        }
        State::Sleepy => {
            a.eyes = "sleepy";
            a.head_y = i32::from(p > 0.4);
            let yawn = p > 0.3 && p < 0.7;
            a.mouth = if yawn { "o" } else { "small" };
            a.right = if yawn { "chin" } else { "rest" };
        }
        State::Sleeping => {
            a.eyes = "closed";
            a.head_y = 1;
            a.y = i32::from(p >= 0.5);
            a.mouth = "small";
            a.prop = "zzz";
        }
        State::Celebrating => {
            a.eyes = "happy";
            a.mouth = "grin";
            a.left = if p < 0.5 { "wave-high" } else { "wave-low" };
            a.right = if p < 0.5 { "wave-low" } else { "wave-high" };
            a.y = if wave.abs() > 0.6 { -2 } else { 0 };
            a.x = if wave > 0.3 {
                1
            } else if wave < -0.3 {
                -1
            } else {
                0
            };
            a.prop = "confetti";
        }
    }
    if let Some((kind, t)) = change {
        let envelope = (t * PI).sin();
        a.blink = t > 0.24 && t < 0.7;
        match kind {
            Change::Doze => {
                a.head_y += rounded(envelope);
                a.y += rounded(envelope);
            }
            Change::Wake => {
                a.head_y -= rounded(envelope * 2.0);
                a.right = if t > 0.5 { "wave-low" } else { "rest" };
            }
            Change::Reassure => a.head_x += rounded((t * TAU).sin() * envelope),
            Change::Blink => {
                a.y += if t < 0.5 {
                    rounded(envelope)
                } else {
                    -rounded(envelope * 2.0)
                }
            }
        }
    }
    a
}

struct Canvas([u8; PIXELS]);
impl Canvas {
    fn dot(&mut self, x: i32, y: i32, color: u8) {
        if (0..WIDTH as i32).contains(&x) && (0..HEIGHT as i32 * 2).contains(&y) {
            self.0[y as usize * WIDTH as usize + x as usize] = color;
        }
    }
    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: u8) {
        for j in 0..h {
            for i in 0..w {
                self.dot(x + i, y + j, color);
            }
        }
    }
    fn rows(&mut self, x: i32, y: i32, rows: &[(i32, i32)], color: u8) {
        for (j, &(start, end)) in rows.iter().enumerate() {
            for i in start..=end {
                self.dot(x + i, y + j as i32, color);
            }
        }
    }
    fn disk(&mut self, x: i32, y: f64, rx: i32, ry: i32, color: u8) {
        for j in -ry..=ry {
            for i in -rx..=rx {
                if (i as f64 / rx as f64).powi(2) + (j as f64 / ry as f64).powi(2) <= 1.15 {
                    self.dot(x + i, rounded(y + j as f64), color);
                }
            }
        }
    }
    fn stamp(&mut self, x: i32, y: i32, pattern: &[&str], color: u8) {
        for (j, row) in pattern.iter().enumerate() {
            for (i, ch) in row.chars().enumerate() {
                if ch != ' ' {
                    self.dot(x + i as i32, y + j as i32, color);
                }
            }
        }
    }
    fn paw(&mut self, x: i32, y: i32, s: i32, mode: &str) {
        let (mut px, mut py, mut ry) = (x + s * 9, y + 5, 4);
        match mode {
            "wave-high" => {
                px = x + s * 13;
                py = y - 7;
                ry = 3;
                self.rect(x + s * 10 - 1, y - 4, 3, 8, OUTLINE);
                self.rect(x + s * 10, y - 3, 1, 7, FUR);
            }
            "wave-low" => {
                px = x + s * 12;
                py = y - 3;
                ry = 3;
                self.rect(x + s * 10 - 1, y - 2, 3, 6, OUTLINE);
            }
            "out" => {
                px = x + s * 11;
                py = y + 1;
                ry = 2;
            }
            "chin" => {
                px = x + s * 5;
                py = y - 1;
                ry = 2;
            }
            "tuck" => {
                px = x + s * 3;
                py = y + 5;
                ry = 2;
            }
            "type-up" | "type-down" => {
                px = x + s * 5;
                py = y + if mode == "type-up" { 5 } else { 6 };
                ry = 1;
            }
            _ => {}
        }
        self.disk(px, py as f64, 2, ry, OUTLINE);
        self.disk(px, py as f64 - 0.5, 1, (ry - 1).max(1), FUR);
        if mode.starts_with("wave") {
            self.dot(px, py - 1, MUZZLE);
            self.dot(px - 1, py, BLUSH);
            self.dot(px + 1, py, BLUSH);
        }
    }
    fn eye(&mut self, ex: i32, hy: i32, side: i32, p: &Pose) {
        let ey = hy + 1;
        match p.eyes {
            _ if p.blink || p.eyes == "closed" => self.rect(ex - 1, ey + 1, 3, 1, INK),
            "happy" => self.stamp(ex - 1, ey, &[" # ", "# #"], INK),
            "sleepy" => {
                self.rect(ex - 1, ey + 1, 3, 1, INK);
                self.dot(ex + 1, ey + 2, INK);
            }
            "wide" => {
                self.rect(ex - 1, ey - 1, 3, 4, MUZZLE);
                self.rect(ex, ey, 2, 3, INK);
                self.dot(ex, ey, CREAM);
            }
            "uneven" if side == 1 => {
                self.rect(ex - 1, ey, 3, 1, INK);
                self.dot(ex + 1, ey + 1, INK);
            }
            _ => {
                let down = i32::from(p.eyes == "look-down");
                self.rect(ex, ey + down, 2, 2, INK);
                self.dot(ex + i32::from(p.eyes == "look"), ey + down, CREAM);
                if matches!(p.eyes, "worried" | "sad") {
                    self.dot(ex + i32::from(side < 0), ey - 2, INK);
                    self.dot(ex + i32::from(side >= 0), ey - 1, INK);
                }
            }
        }
    }
}

fn pixels(state: State, time: f64, change: Option<(Change, f64)>) -> Bear {
    let p = pose(state, time, change);
    let mut c = Canvas([0; PIXELS]);
    let (x, y) = (19 + p.x, 20 + p.y);
    c.rect(x - 7, y + 10, 6, 3, OUTLINE);
    c.rect(x + 2, y + 10, 6, 3, OUTLINE);
    c.rect(x - 6, y + 10, 4, 2, FUR);
    c.rect(x + 3, y + 10, 4, 2, FUR);
    c.disk(x, (y + 6) as f64, 8, 6, OUTLINE);
    c.disk(x, (y + 6) as f64, 7, 5, FUR);
    c.disk(x - 1, (y + 6) as f64, 4, 4, HIGHLIGHT);
    if !matches!(p.left, "chin" | "tuck") {
        c.paw(x, y, -1, p.left);
    }
    if !matches!(p.right, "chin" | "tuck") {
        c.paw(x, y, 1, p.right);
    }
    c.rect(x - 7, y, 15, 3, TEAL);
    c.rect(x - 7, y, 15, 2, MINT);
    c.rect(x + 4, y + 2, 3, 6, TEAL);
    c.rect(x + 4, y + 2, 2, 5, MINT);
    c.dot(x + 5, y + 7, MINT);
    let (hx, hy) = (x + p.head_x, 11 + p.y + p.head_y);
    for offset in [-7, 7] {
        c.disk(hx + offset, (hy - 5) as f64, 4, 4, OUTLINE);
        c.disk(hx + offset, (hy - 5) as f64, 3, 3, FUR);
        c.disk(hx + offset, (hy - 5) as f64, 1, 2, BLUSH);
    }
    let head = [
        (-6, 6),
        (-8, 8),
        (-9, 9),
        (-10, 10),
        (-10, 10),
        (-10, 10),
        (-10, 10),
        (-10, 10),
        (-9, 9),
        (-9, 9),
        (-8, 8),
        (-6, 6),
    ];
    c.rows(hx, hy - 4, &head, OUTLINE);
    let inner: Vec<_> = head[1..11].iter().map(|&(a, b)| (a + 1, b - 1)).collect();
    c.rows(hx, hy - 3, &inner, FUR);
    c.rect(hx - 6, hy - 3, 10, 1, HIGHLIGHT);
    c.rect(hx - 8, hy - 2, 2, 3, HIGHLIGHT);
    c.rows(hx, hy + 5, &[(-7, 7), (-5, 5)], SHADE);
    c.disk(hx, (hy + 4) as f64, 5, 3, MUZZLE);
    c.rect(hx - 8, hy + 3, 2, 1, BLUSH);
    c.rect(hx + 7, hy + 3, 2, 1, BLUSH);
    c.eye(hx - 5, hy, -1, &p);
    c.eye(hx + 4, hy, 1, &p);
    c.rect(hx - 1, hy + 3, 3, 1, INK);
    c.dot(hx, hy + 4, INK);
    match p.mouth {
        "o" => c.stamp(hx - 1, hy + 5, &["###", "# #", "###"], INK),
        "frown" => c.stamp(hx - 2, hy + 6, &[" ### ", "#   #"], INK),
        "slant" => c.stamp(hx - 2, hy + 6, &["##   ", "  ###"], INK),
        "grin" => c.stamp(hx - 3, hy + 5, &["#     #", " ##### ", "  ###  "], INK),
        "small" => c.rect(hx - 1, hy + 6, 3, 1, INK),
        _ => c.stamp(hx - 2, hy + 5, &["# # #", " ### "], INK),
    }
    if p.prop == "glasses" {
        c.stamp(
            hx - 7,
            hy - 1,
            &["##### #####", "#   ###   #", "#   # #   #", "##### #####"],
            TEAL,
        );
        c.rect(hx - 9, hy, 2, 1, TEAL);
        c.rect(hx + 6, hy, 3, 1, TEAL);
    }
    if matches!(p.left, "chin" | "tuck") {
        c.paw(x, y, -1, p.left);
    }
    if matches!(p.right, "chin" | "tuck") {
        c.paw(x, y, 1, p.right);
    }
    match p.prop {
        "laptop" => {
            c.rect(x - 7, y + 5, 15, 6, OUTLINE);
            c.rect(x - 6, y + 5, 13, 5, TEAL);
            c.rect(x - 5, y + 6, 11, 3, SURFACE);
            c.stamp(x - 3, y + 6, &["#   ", " #  ", "# ##"], MINT);
            c.rect(x - 9, y + 11, 19, 1, MINT);
            c.dot(x - 5, y + if p.left == "type-up" { 4 } else { 5 }, MUZZLE);
            c.dot(x + 5, y + if p.right == "type-up" { 4 } else { 5 }, MUZZLE);
        }
        "?" => c.stamp(
            33,
            4 + if p.phase < 0.5 { 0 } else { -1 },
            &["###", "  #", " ##", "   ", " # "],
            MINT,
        ),
        "!" => c.stamp(
            33,
            4 + if p.phase < 0.5 { 0 } else { -1 },
            &[" # ", " # ", " # ", "   ", " # "],
            GOLD,
        ),
        "check" => c.stamp(
            31,
            4 + i32::from(p.phase >= 0.5),
            &["    #", "   # ", "# #  ", " #   "],
            MINT,
        ),
        "heart" => c.stamp(
            31,
            5 + rounded((p.phase * TAU).sin()),
            &[" # # ", "#####", "#####", " ### ", "  #  "],
            ROSE,
        ),
        "dots" => {
            c.rect(30, 3, 8, 5, BORDER);
            c.rect(31, 3, 6, 4, SURFACE);
            c.dot(30, 8, BORDER);
            for i in 0..1 + (p.phase * 3.0).floor() as i32 {
                c.dot(32 + i * 2, 5, MINT);
            }
        }
        "error" => c.stamp(33, 5, &["# #", " # ", "# #"], ROSE),
        "zzz" => {
            let dy = (p.phase * 3.0).floor() as i32;
            let z = &["###", "  #", " # ", "#  ", "###"];
            c.stamp(30, 8 - dy, z, MINT);
            if p.phase > 0.28 {
                c.stamp(34, 4 - dy, z, MUTED);
            }
        }
        "confetti" => {
            for i in 0..12 {
                let cx = if i % 2 == 0 { 2 } else { 31 + (i * 3 % 7) };
                let cy = ((p.phase * 23.0 + (i * 5) as f64) % 25.0).floor() as i32;
                let (w, h) = if i % 3 == 0 { (2, 1) } else { (1, 2) };
                c.rect(cx, cy, w, h, [MINT, ROSE, GOLD][i as usize % 3]);
            }
        }
        _ => {}
    }
    Bear { pixels: c.0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use sha2::{Digest, Sha256};

    fn state(name: &str) -> State {
        State::from_mood(Mood::from_name(name).expect("reference state"))
    }
    fn hash(bear: Bear) -> String {
        format!("{:x}", Sha256::digest(bear.pixels))
    }

    #[test]
    fn pixels_and_all_transitions_match_the_supplied_renderer() {
        let reference: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/mascot-reference.json"))
                .unwrap();
        for frame in reference["frames"].as_array().unwrap() {
            let change = frame.get("transition").map(|tr| {
                let kind = match tr["kind"].as_str().unwrap() {
                    "blink" => Change::Blink,
                    "doze" => Change::Doze,
                    "wake" => Change::Wake,
                    "reassure" => Change::Reassure,
                    _ => unreachable!(),
                };
                (kind, tr["progress"].as_f64().unwrap())
            });
            assert_eq!(
                hash(pixels(
                    state(frame["state"].as_str().unwrap()),
                    frame["time"].as_f64().unwrap(),
                    change
                )),
                frame["hash"].as_str().unwrap(),
                "{frame}"
            );
        }
        for tr in reference["transitions"].as_array().unwrap() {
            let from = Mood::from_name(tr["from"].as_str().unwrap()).unwrap();
            let to = Mood::from_name(tr["to"].as_str().unwrap()).unwrap();
            let mut animator = Animator::new(from);
            animator.set_mood(to, 613.0);
            for sample in tr["samples"].as_array().unwrap() {
                if sample["interrupted"].as_bool() == Some(true) {
                    animator = Animator::new(from);
                    animator.set_mood(to, 613.0);
                    animator.set_mood(Mood::Happy, 713.0);
                }
                assert_eq!(
                    hash(animator.frame(sample["now"].as_f64().unwrap())),
                    sample["hash"].as_str().unwrap(),
                    "{tr}"
                );
            }
        }
    }
}
