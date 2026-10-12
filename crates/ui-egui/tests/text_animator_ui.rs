//! Text animators in Effect Controls, on-screen layer rotation and spatial motion paths,
//! driven headless through the control channel: adding an animator and toggling a stopwatch
//! from its Effect Controls rows (one undo step each), dragging a layer's rotate zone (the
//! rotation changes, undo restores it), and motion-path overlays appearing once a Position is
//! animated (Motion's and a graphic layer's).
//!
//! With `FILMCRAFT_UI_SHOTS=<dir>` the tests also render the UI with wgpu and write PNGs there.

use std::sync::mpsc::{Sender, channel};

use egui_kittest::Harness;
use filmcraft_engine::Session;
use filmcraft_time::TICKS_PER_SECOND;
use filmcraft_ui_egui::FilmcraftApp;
use filmcraft_ui_egui::control::ControlRequest;
use serde_json::{Value, json};

struct Driver {
    harness: Harness<'static, FilmcraftApp>,
    tx: Sender<ControlRequest>,
    shots: Option<std::path::PathBuf>,
}

impl Driver {
    fn demo() -> Self {
        let mut session = Session::default();
        session.execute("file.openDemoProject", json!({})).expect("demo project");
        let (tx, rx) = channel();
        let app = FilmcraftApp::new(session).with_control(rx);
        let shots = std::env::var_os("FILMCRAFT_UI_SHOTS").map(std::path::PathBuf::from);
        let mut b = Harness::builder().with_size(egui::vec2(1600.0, 980.0)).with_max_steps(10_000);
        if shots.is_some() {
            b = b.wgpu().with_pixels_per_point(1.0);
        }
        let harness = b.build_eframe(move |_cc| app);
        let mut d = Driver { harness, tx, shots };
        d.frames(4);
        d.ok("ui.set", json!({"workspace": "Effects"}));
        d
    }
    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            let ctx = self.harness.ctx.clone();
            let mut raw = std::mem::take(self.harness.input_mut());
            eframe::App::raw_input_hook(self.harness.state_mut(), &ctx, &mut raw);
            *self.harness.input_mut() = raw;
            self.harness.step();
        }
    }
    fn ok(&mut self, method: &str, params: Value) -> Value {
        let (req, reply) = ControlRequest::new(method, params.clone());
        self.tx.send(req).unwrap();
        for _ in 0..600 {
            self.frames(1);
            if let Ok(v) = reply.try_recv() {
                assert_eq!(v["ok"], json!(true), "{method} {params}: {v}");
                return v["result"].clone();
            }
        }
        panic!("no reply to {method} {params}");
    }
    fn exec(&mut self, command: &str, params: Value) -> Value {
        self.ok("engine.execute", json!({"command": command, "params": params}))
    }
    fn ids(&mut self, prefix: &str) -> Vec<String> {
        let v = self.ok("ui.elements", json!({"prefix": prefix}));
        v.as_array().unwrap().iter().filter_map(|e| e["id"].as_str().map(str::to_string)).collect()
    }
    fn has(&mut self, id: &str) -> bool {
        self.ids(id).iter().any(|e| e == id)
    }
    fn click(&mut self, id: &str) {
        self.ok("ui.click", json!({"id": id}));
        self.frames(2);
    }
    /// Click an element that may sit below the fold: scroll the Effect Controls body until its
    /// centre lands mid-viewport (scrolls anchored on the always-visible Motion twirl),
    /// then click it.
    fn click_visible(&mut self, id: &str) {
        for _ in 0..12 {
            let v = self.ok("ui.elements", json!({"prefix": id}));
            let e = v.as_array().unwrap().iter().find(|e| e["id"] == id).unwrap_or_else(|| panic!("no element {id}")).clone();
            let r: Vec<f64> = e["rect"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
            let y = r[1] + r[3] / 2.0;
            if (250.0..350.0).contains(&y) {
                break;
            }
            let tw = self.ok("ui.elements", json!({"prefix": "effectControls.effect.motion.twirl"}));
            let tr: Vec<f64> = tw.as_array().unwrap()[0]["rect"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
            let dy = if y >= 350.0 { -150.0 } else { 150.0 };
            self.ok("ui.scroll", json!({"x": tr[0] + 5.0, "y": tr[1] + 5.0, "dy": dy}));
            self.frames(3);
        }
        self.click(id);
    }
    fn drag(&mut self, id: &str, dx: f64, dy: f64) {
        let v = self.ok("ui.elements", json!({"prefix": id}));
        let e = v.as_array().unwrap().iter().find(|e| e["id"] == id).unwrap().clone();
        let r: Vec<f64> = e["rect"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
        let (x, y) = (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0);
        self.ok("ui.drag", json!({"from": {"x": x, "y": y}, "to": {"x": x + dx, "y": y + dy}, "steps": 6}));
        self.frames(2);
    }
    fn undo_len(&mut self) -> usize {
        self.exec("history.list", json!({}))["undo"].as_array().unwrap().len()
    }
    fn snapshot(&mut self, name: &str) {
        let Some(dir) = self.shots.clone() else { return };
        self.frames(2);
        match self.harness.render() {
            Ok(img) => {
                std::fs::create_dir_all(&dir).unwrap();
                img.save(dir.join(format!("animator-{name}.png"))).unwrap();
            }
            Err(e) => eprintln!("snapshot {name} skipped: {e}"),
        }
    }
    /// A fresh text clip, selected with its layer.
    fn text_clip(&mut self) -> u64 {
        let r = self.exec("graphics.newText", json!({"text": "ABCD"}));
        self.frames(3);
        r["clip"].as_u64().unwrap()
    }
    fn layer_rotation(&mut self, clip: u64) -> f64 {
        self.exec("graphics.list", json!({"clip": clip}))["layers"][0]["rotation"].as_f64().unwrap()
    }
}

#[test]
fn animator_rows_toggle_and_add_keyframes() {
    let mut d = Driver::demo();
    let clip = d.text_clip();
    let base = "effectControls.graphic_text.animator";
    assert!(d.has(&format!("{base}.add")), "add row without animators");
    assert!(!d.has(&format!("{base}.0.opacity.stopwatch")), "no rows yet");
    d.click_visible(&format!("{base}.add"));
    let sw = format!("{base}.0.opacity.stopwatch");
    assert!(d.has(&sw), "stopwatch after adding");
    assert!(d.has(&format!("{base}.0.opacity.addKeyframe")), "navigator diamond");
    // stopwatch on: one undo step
    let undo = d.undo_len();
    d.click_visible(&sw);
    assert_eq!(d.undo_len(), undo + 1);
    // diamond adds a keyframe at the playhead: one more undo step
    d.click_visible(&format!("{base}.0.opacity.addKeyframe"));
    assert_eq!(d.undo_len(), undo + 2);
    // clicking it again removes that keyframe
    d.click_visible(&format!("{base}.0.opacity.addKeyframe"));
    assert_eq!(d.undo_len(), undo + 3);
    // the animator survives in the list until removed
    assert_eq!(d.exec("graphics.list", json!({"clip": clip}))["layers"][0]["animators"], json!(["Animator 1"]));
    d.click_visible(&format!("{base}.0.remove"));
    assert!(!d.has(&sw), "rows gone with the animator");
    d.snapshot("rows");
}

#[test]
fn layer_rotate_zone_drags_rotation() {
    let mut d = Driver::demo();
    let clip = d.text_clip();
    let id = format!("program.layer.{clip}.0.rotate.0");
    assert!(d.has(&id), "rotate zone on a selected layer");
    assert_eq!(d.layer_rotation(clip), 0.0);
    let undo = d.undo_len();
    d.drag(&id, 40.0, -30.0);
    let rot = d.layer_rotation(clip);
    assert!(rot != 0.0, "rotation changed: {rot}");
    assert_eq!(d.undo_len(), undo + 1, "one undo step per drag");
    d.exec("edit.undo", json!({}));
    assert_eq!(d.layer_rotation(clip), 0.0);
    d.snapshot("rotate");
}

#[test]
fn motion_paths_appear_for_animated_positions() {
    let mut d = Driver::demo();
    let clip = d.text_clip();
    let layer_path = format!("program.layer.{clip}.0.path");
    assert!(!d.has(&layer_path), "no path while static");
    assert!(!d.has("program.transform.path"), "no Motion path while static");
    // animate the layer's Position (stopwatch, then a second keyframe 2 s in)
    let list = d.exec("graphics.list", json!({"clip": clip}));
    let ei = list["layers"][0]["effectIndex"].as_u64().unwrap();
    let start = list["start"].as_i64().unwrap();
    let two_s = 2 * TICKS_PER_SECOND;
    d.exec("effects.toggleAnimation", json!({"clip": clip, "effect": ei, "param": "position"}));
    d.exec("playhead.set", json!({"time": start}));
    d.exec("effects.setParam", json!({"clip": clip, "effect": ei, "param": "position", "value": [960.0, 540.0]}));
    d.exec("playhead.set", json!({"time": start + two_s}));
    d.exec("effects.setParam", json!({"clip": clip, "effect": ei, "param": "position", "value": [1000.0, 560.0]}));
    d.frames(3);
    assert!(d.has(&layer_path), "layer path once animated");
    // animate Motion's Position too (two keyframes, then select it for its overlay)
    d.exec("playhead.set", json!({"time": start}));
    d.exec("effects.toggleAnimation", json!({"clip": clip, "effect": "motion", "param": "position"}));
    d.exec("effects.setParam", json!({"clip": clip, "effect": "motion", "param": "position", "value": [960.0, 540.0]}));
    d.exec("playhead.set", json!({"time": start + two_s}));
    d.exec("effects.setParam", json!({"clip": clip, "effect": "motion", "param": "position", "value": [1000.0, 560.0]}));
    d.exec("effects.select", json!({"clip": clip, "effect": "motion"}));
    d.frames(3);
    assert!(d.has("program.transform.path"), "Motion path once animated");
    d.snapshot("paths");
}
