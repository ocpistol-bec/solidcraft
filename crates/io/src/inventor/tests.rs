//! The Inventor history import on the fixture part: a 2 × 2 cm square sketch on the XY plane
//! extruded 1 cm (`d0 = my_len`), saved by Inventor 2027. It must come in as the file's own
//! parameter table, sketch (entities, constraints) and extrude — in the part's order.

use crate::import_inventor;
use solidcraft_doc::FeatureKind;

const BOX_IPT: &[u8] = include_bytes!("../native_cad/fixtures/box.ipt");

/// The part's model: the square's four lines, its closing and orientation constraints, and the
/// extrusion driven by the file's own parameter.
#[test]
fn the_fixture_part_is_its_sketch_its_constraints_and_its_extrude() {
    let (doc, warnings) = import_inventor(BOX_IPT, "box.ipt").expect("the fixture part imports");
    let _ = warnings;

    assert_eq!(doc.features.len(), 2, "the sketch and the extrude, nothing else");
    let FeatureKind::Sketch { plane, sketch } = &doc.features[0].kind else {
        panic!("feature 0 is not a sketch: {:?}", doc.features[0].kind);
    };
    // The sketch lies on the XY plane at the origin, in millimetres.
    let solidcraft_doc::PlaneRef::Custom { plane } = plane else {
        panic!("the imported sketch plane is custom: {plane:?}");
    };
    assert!(plane.origin.dist(solidcraft_geom::Vec3::ZERO) < 1e-9);
    assert_eq!(sketch.points.len(), 4, "the square's four corners");
    assert_eq!(sketch.curves.len(), 4, "the square's four lines");
    assert!(sketch.curves.iter().all(|c| matches!(c.kind, solidcraft_doc::sketch::CurveKind::Line { .. })));
    // The file's 8 coincidents tie each line's end to the corner point: the import shares the
    // point, so they hold structurally, and the sketch's constraints are the ones that orient
    // the square — perpendicular, two parallels and a horizontal.
    assert_eq!(sketch.constraints.len(), 4, "the square's orientation constraints");
    assert!(sketch.constraints.iter().any(|c| matches!(c.kind, solidcraft_doc::sketch::ConstraintKind::Horizontal { .. })));

    let FeatureKind::Extrude { sketch: target, profiles, operation, extent, .. } = &doc.features[1].kind else {
        panic!("feature 1 is not an extrude: {:?}", doc.features[1].kind);
    };
    assert_eq!(target.0, 1, "the extrude profiles the sketch feature just above it");
    let solidcraft_doc::ProfileSel::Curves { loops } = profiles else {
        panic!("the extrude's profiles are not the sketch's curves");
    };
    assert_eq!(loops.len(), 1, "one closed profile");
    assert_eq!(loops[0].len(), 4, "the square's four lines");
    assert_eq!(*operation, solidcraft_doc::Operation::Join);
    assert!(!extent.through_all);
    assert_eq!(extent.distance.as_str(), "d0", "the extrusion depth is the file's own model parameter");
}

/// The file's parameter table: `my_len` in millimetres, the model parameter `d0` driven by it.
#[test]
fn the_file_parameters_come_in_their_own_names_and_units() {
    let (doc, _warnings) = import_inventor(BOX_IPT, "box.ipt").expect("the fixture part imports");
    let by_name = |name: &str| doc.params.iter().find(|p| p.name == name);
    let my_len = by_name("my_len").expect("`my_len` is in the file");
    assert!(!my_len.model, "`my_len` is the file's user parameter");
    assert_eq!(my_len.unit, "mm", "Inventor's centimetres become millimetres");
    assert_eq!(my_len.expr, "10", "the 1 cm value as mm");
    let d0 = by_name("d0").expect("`d0` is in the file");
    assert!(d0.model, "`d0` is a model parameter");
    assert_eq!(d0.expr, "my_len", "its equation references the user parameter, converted with it");
}

/// A file with no part model is refused with its name, not read partially.
#[test]
fn unreadable_files_are_refused_cleanly() {
    for (bytes, name) in [(&b"not an inventor file"[..], "junk.ipt"), (&BOX_IPT[..1024], "cut.ipt")] {
        let e = import_inventor(bytes, name).expect_err(name);
        let crate::IoError::Invalid(m) = &e else {
            panic!("{name}: not an Invalid error: {e:?}");
        };
        assert!(m.starts_with(name), "{name}: {m}");
    }
}
