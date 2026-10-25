//! Part data outside the feature tree: iMates, sheet metal settings, materials and
//! appearances, iPart tables.

mod ipart;
mod materials;
mod sheet_metal;
mod xlsx;

pub use ipart::{IPartMember, IPartTable, IPartWorkbook};
pub use materials::{
    AssetKind, DocumentStyle, FaceOverride, FeatureOverride, MaterialAsset, PartMaterials,
    PhysicalProperties,
};
pub use sheet_metal::{SheetMetal, SheetMetalEnum, SheetMetalRule, SheetMetalSettings, UnfoldRule};

use super::Part;
use crate::grammar::FieldValue;
use crate::model::view::{node_ref, vec3};

/// PmBrowserSegment browser entry; iMates are those of class `PartMateInterfaceDefEntry`.
const T_BROWSER_ENTRY: u32 = 0x9E77_CCC3;
/// One per iMate in the part model, carrying the uid its PmAppSegment glyph shares.
const T_IMATE_TOKEN: u32 = 0x38C2_654E;
/// Face reference; an iMate's has a plane copy as `ref_2`.
const T_FACE_REF: u32 = 0x9087_4D56;
/// Plane geometry.
const T_PLANE: u32 = 0xCE52_DF42;

/// An iMate.
#[derive(Debug, Clone, PartialEq)]
pub struct IMate {
    /// Browser name ("iMate:1").
    pub name: Option<String>,
    /// The iMate uid's 16 bytes as stored.
    pub uid: Option<Vec<u8>>,
    /// The mate plane, when one is stored.
    pub plane: Option<IMatePlane>,
}

/// The plane an iMate mates on (model cm).
#[derive(Debug, Clone, PartialEq)]
pub struct IMatePlane {
    /// A point on the plane.
    pub center: [f64; 3],
    /// The plane's stored normal.
    pub normal: [f64; 3],
}

impl Part<'_> {
    /// The part's iMates, in browser order. The i-th browser entry pairs with the i-th iMate
    /// token and the i-th plane-backed face reference.
    ///
    /// ```no_run
    /// # let doc = corliss::Document::open("part.ipt")?;
    /// # let model = corliss::model::Model::load(&doc)?;
    /// let part = corliss::model::Part::new(&model)?;
    /// for imate in part.imates() {
    ///     println!("{:?} {:?}", imate.name, imate.plane);
    /// }
    /// # Ok::<(), corliss::CorlissError>(())
    /// ```
    pub fn imates(&self) -> Vec<IMate> {
        let tokens: Vec<Option<Vec<u8>>> = self
            .view()
            .by_type(T_IMATE_TOKEN)
            .map(|(_, n)| n.field("uid").map(|f| f.raw.clone()))
            .collect();
        let planes = self.imate_planes();
        self.imate_names()
            .into_iter()
            .enumerate()
            .map(|(i, name)| IMate {
                name,
                uid: tokens.get(i).cloned().flatten(),
                plane: planes.get(i).cloned(),
            })
            .collect()
    }

    fn imate_names(&self) -> Vec<Option<String>> {
        let Some(browser) = self.model().view("PmBrowserSegment") else {
            return Vec::new();
        };
        browser
            .by_type(T_BROWSER_ENTRY)
            .filter(|(_, n)| n.text("txt_1") == Some("PartMateInterfaceDefEntry"))
            .map(|(_, n)| n.text("name").map(str::to_string))
            .collect()
    }

    fn imate_planes(&self) -> Vec<IMatePlane> {
        let view = self.view();
        view.by_type(T_FACE_REF)
            .filter_map(|(_, n)| node_ref(n, "ref_2").and_then(|p| view.node(p)))
            .filter(|p| p.type_id.0 == T_PLANE)
            .map(|p| IMatePlane {
                center: vec3(p, "origin"),
                normal: vec3(p, "v"),
            })
            .collect()
    }
}

/// A text field as an owned string.
fn text_of(n: &crate::grammar::DecodedNode, name: &str) -> Option<String> {
    n.text(name).map(str::to_string)
}

/// The value of a field as Python's truthiness sees it: missing, zero, false and empty are false.
fn truthy(value: Option<&FieldValue>) -> bool {
    match value {
        None => false,
        Some(FieldValue::Text(s) | FieldValue::Text8(s)) => !s.is_empty(),
        Some(v) => v.as_f64().is_some_and(|x| x != 0.0),
    }
}
