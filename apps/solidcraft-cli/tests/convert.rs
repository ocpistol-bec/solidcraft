//! `convert`: a B-rep CAD file restated as the STEP text the Import feature holds, the PDM's
//! `cad.convert` job runs this.

use std::process::{Command, Output};

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_solidcraft-cli"))
}

/// A 2 × 2 × 1 cm Inventor box (4000 mm³) from the import fixtures.
const BOX_IPT: &[u8] = include_bytes!("../../../crates/io/src/native_cad/fixtures/box.ipt");

fn run(args: &[&str]) -> Output {
    cli().args(args).output().expect("run the CLI")
}

fn temp(tag: &str, name: &str, bytes: &[u8]) -> String {
    let dir = std::env::temp_dir().join(format!("solidcraft-cli-convert-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path.to_string_lossy().to_string()
}

#[test]
fn convert_restates_inventor_as_step() {
    let in_ipt = temp("in", "box.ipt", BOX_IPT);
    let out_step = temp("out", "box.step", b"");
    let r = run(&["convert", in_ipt.as_str(), "--out", out_step.as_str()]);
    assert!(r.status.success(), "stderr: {}", String::from_utf8_lossy(&r.stderr));
    let step = std::fs::read_to_string(&out_step).unwrap();
    assert!(step.contains("MANIFOLD_SOLID_BREP"), "no solid in the restated STEP");

    // The STEP reopens as one 4000 mm³ body: the conversion is not lossy for this file.
    let r = run(&["eval", out_step.as_str()]);
    assert!(r.status.success(), "stderr: {}", String::from_utf8_lossy(&r.stderr));
    let v: serde_json::Value = serde_json::from_slice(&r.stdout).unwrap();
    assert_eq!(v["body_count"], 1);
    assert!((v["total"]["volume_mm3"].as_f64().unwrap() - 4000.0).abs() < 1e-6);
}

#[test]
fn convert_refuses_unreadable_and_unsupported() {
    // Not a B-rep CAD extension.
    let in_stl = temp("stl", "box.stl", b"solid x\nendsolid x\n");
    let out = temp("stl-out", "out.step", b"");
    let r = run(&["convert", in_stl.as_str(), "--out", out.as_str()]);
    assert!(!r.status.success());
    assert!(String::from_utf8_lossy(&r.stderr).contains("reads STEP, IGES, FreeCAD, Rhino or Inventor"));

    // A truncated Inventor file fails with its name in the message.
    let cut = temp("cut", "cut.ipt", &BOX_IPT[..1024]);
    let r = run(&["convert", cut.as_str(), "--out", out.as_str()]);
    assert!(!r.status.success());
    assert!(String::from_utf8_lossy(&r.stderr).starts_with("error: cut.ipt"), "{}", String::from_utf8_lossy(&r.stderr));
}
