//! Part material, part appearance and appearance overrides.
//!
//! PmAppSegment 11FBECCD (document style settings) names the material asset (6759D86E) and the
//! appearance asset (6759D86F); `u8_0` = 1 when the appearance follows the material. FDA6D020
//! style collections list the cached assets. Face overrides are FBAttributeSegment attributes
//! (`FaceColor`, `Owner_Feature_ID`); feature overrides are DC 578432A6 nodes (`u16_0` = 3).

use super::{text_of, truthy};
use crate::grammar::{DecodedNode, FieldValue};
use crate::model::SegmentView;
use crate::model::part::Part;
use crate::model::view::{named, node_ref};

/// Document style settings.
const T_STYLE_SETTINGS: u32 = 0x11FB_ECCD;
/// Material asset.
const T_MATERIAL: u32 = 0x6759_D86E;
/// Style collection ("Materials", "Colors", ...).
const T_STYLE_COLLECTION: u32 = 0xFDA6_D020;
/// Per-feature appearance override.
const T_FEATURE_APPEARANCE: u32 = 0x5784_32A6;
/// FBAttributeSegment attribute (name, value).
const T_ATTRIBUTE: u32 = 0x28C2_5C45;

/// Material and appearance assignments of a part.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PartMaterials {
    /// The document style settings, when present.
    pub document_style: Option<DocumentStyle>,
    /// Asset names of the "Materials" style collection, when present.
    pub library_materials: Option<Vec<String>>,
    /// Asset names of the "Colors" style collection, when present.
    pub library_colors: Option<Vec<String>>,
    /// Features with an appearance override.
    pub feature_overrides: Vec<FeatureOverride>,
    /// Faces with an appearance override, in attribute order.
    pub face_overrides: Vec<FaceOverride>,
}

/// The part's material and appearance.
#[derive(Debug, Clone, PartialEq)]
pub struct DocumentStyle {
    /// The part material.
    pub material: Option<MaterialAsset>,
    /// The part appearance.
    pub appearance: Option<MaterialAsset>,
    /// True when the appearance follows the material.
    pub appearance_from_material: bool,
}

/// A material or appearance asset.
#[derive(Debug, Clone, PartialEq)]
pub struct MaterialAsset {
    /// Asset name ("Steel").
    pub name: Option<String>,
    /// Library asset id ("Material-077").
    pub library_id: Option<String>,
    /// Comment.
    pub comment: Option<String>,
    /// Material or appearance data.
    pub kind: AssetKind,
}

/// What an asset describes.
#[derive(Debug, Clone, PartialEq)]
pub enum AssetKind {
    /// A material: physical properties and its appearance.
    Material {
        /// Physical properties.
        physical: PhysicalProperties,
        /// The material's appearance asset.
        appearance: Option<Box<MaterialAsset>>,
    },
    /// An appearance: colour and texture.
    Appearance {
        /// Diffuse RGBA.
        diffuse: Vec<f64>,
        /// Texture file name.
        texture: Option<String>,
    },
}

/// A material's physical properties; `None` for values the asset does not store.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PhysicalProperties {
    /// Density (g/cm³).
    pub density_g_cm3: Option<f64>,
    /// Young's modulus (GPa).
    pub youngs_modulus_gpa: Option<f64>,
    /// Poisson's ratio.
    pub poisson_ratio: Option<f64>,
    /// Yield strength (MPa).
    pub yield_strength_mpa: Option<f64>,
    /// Ultimate tensile strength (MPa).
    pub ultimate_tensile_mpa: Option<f64>,
    /// Thermal conductivity (W/(m·K)).
    pub thermal_conductivity_w_mk: Option<f64>,
    /// Linear expansion (µm/(m·°C)).
    pub linear_expansion_um_mc: Option<f64>,
    /// Specific heat (J/(g·°C)).
    pub specific_heat_j_gc: Option<f64>,
}

/// A feature's appearance override.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureOverride {
    /// Browser name of the feature.
    pub feature: Option<String>,
    /// Appearance asset name.
    pub appearance: Option<String>,
}

/// A face's appearance override.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FaceOverride {
    /// Appearance asset name.
    pub appearance: Option<String>,
    /// Id of the feature that owns the face.
    pub owner_feature_id: Option<i64>,
}

impl Part<'_> {
    /// Material, appearance and appearance overrides.
    ///
    /// ```no_run
    /// # let doc = corliss::Document::open("part.ipt")?;
    /// # let model = corliss::model::Model::load(&doc)?;
    /// let part = corliss::model::Part::new(&model)?;
    /// let materials = part.materials();
    /// if let Some(style) = &materials.document_style {
    ///     println!("{:?}", style.material.as_ref().and_then(|m| m.name.as_deref()));
    /// }
    /// # Ok::<(), corliss::CorlissError>(())
    /// ```
    pub fn materials(&self) -> PartMaterials {
        let mut out = PartMaterials::default();
        if let Some(app) = self.model().view("PmAppSegment") {
            out.document_style = app
                .by_type(T_STYLE_SETTINGS)
                .next()
                .map(|(_, n)| document_style(&app, n));
            library_lists(&app, &mut out);
        }
        out.feature_overrides = self.feature_overrides();
        out.face_overrides = self.face_overrides();
        out
    }

    fn feature_overrides(&self) -> Vec<FeatureOverride> {
        self.view()
            .by_type(T_FEATURE_APPEARANCE)
            .filter(|(_, n)| n.int("u16_0") == Some(3))
            .map(|(_, n)| FeatureOverride {
                feature: node_ref(n, "fx").and_then(|f| self.name(f)),
                appearance: text_of(n, "name"),
            })
            .collect()
    }

    /// A `FaceColor` attribute starts an override; the `Owner_Feature_ID` after it completes it.
    fn face_overrides(&self) -> Vec<FaceOverride> {
        let Some(fb) = self.model().view("FBAttributeSegment") else {
            return Vec::new();
        };
        let mut faces: Vec<FaceOverride> = Vec::new();
        for (_, n) in fb.by_type(T_ATTRIBUTE) {
            match n.text("name") {
                Some("FaceColor") => faces.push(FaceOverride {
                    appearance: text_of(n, "value"),
                    ..Default::default()
                }),
                Some("Owner_Feature_ID") => {
                    if let Some(face) = faces.last_mut() {
                        face.owner_feature_id = n.int("value");
                    }
                }
                _ => {}
            }
        }
        faces
    }
}

fn document_style(app: &SegmentView<'_>, n: &DecodedNode) -> DocumentStyle {
    DocumentStyle {
        material: material_asset(app, node_ref(n, "material")),
        appearance: material_asset(app, node_ref(n, "renderingStyle")),
        appearance_from_material: truthy(n.get("u8_0")),
    }
}

/// The asset node at `index`; a material carries its appearance asset.
fn material_asset(app: &SegmentView<'_>, index: Option<usize>) -> Option<MaterialAsset> {
    let a = app.node(index?)?;
    let kind = if a.type_id.0 == T_MATERIAL {
        material_kind(app, a)
    } else {
        appearance_kind(a)
    };
    Some(MaterialAsset {
        name: text_of(a, "name"),
        library_id: library_id(a),
        comment: text_of(a, "comment"),
        kind,
    })
}

fn material_kind(app: &SegmentView<'_>, a: &DecodedNode) -> AssetKind {
    // `renderStyle` is a plain u32 holding a NodeRef.
    let render = a
        .int("renderStyle")
        .map(|r| (r & 0x7FFF_FFFF) as usize)
        .filter(|&r| r != 0);
    AssetKind::Material {
        physical: physical(&first_values(a, "props")),
        appearance: material_asset(app, render).map(Box::new),
    }
}

fn appearance_kind(a: &DecodedNode) -> AssetKind {
    AssetKind::Appearance {
        diffuse: first_values(a, "mat.diffuse"),
        texture: text_of(a, "FileMapTexture").filter(|t| !t.is_empty()),
    }
}

/// The first non-empty of `library` (materials), `library_asset` (appearances) and `txt_2`
/// (older appearances); else `txt_2` as stored.
fn library_id(a: &DecodedNode) -> Option<String> {
    let first = ["library", "library_asset"]
        .iter()
        .filter_map(|k| text_of(a, k))
        .find(|s| !s.is_empty());
    first.or_else(|| text_of(a, "txt_2"))
}

/// The values of field `name`: the first field's when it is an array or colour, else one value
/// per scalar field.
fn first_values(n: &DecodedNode, name: &str) -> Vec<f64> {
    let fields = named(n, name);
    if let Some(first) = fields.first().and_then(|f| array_values(&f.value)) {
        return first;
    }
    fields.iter().filter_map(|f| f.value.as_f64()).collect()
}

fn array_values(value: &FieldValue) -> Option<Vec<f64>> {
    match value {
        FieldValue::Color(c) => Some(c.iter().map(|x| f64::from(*x)).collect()),
        other => other.as_f64s(),
    }
}

fn physical(props: &[f64]) -> PhysicalProperties {
    let at = |k: usize| props.get(k).copied();
    PhysicalProperties {
        density_g_cm3: at(0),
        youngs_modulus_gpa: at(1),
        poisson_ratio: at(2),
        yield_strength_mpa: at(3),
        ultimate_tensile_mpa: at(4),
        thermal_conductivity_w_mk: at(5),
        linear_expansion_um_mc: at(6),
        specific_heat_j_gc: at(7),
    }
}

/// The "Materials" and "Colors" collections' Text16 entries of their `lst0` list.
fn library_lists(app: &SegmentView<'_>, out: &mut PartMaterials) {
    for (_, n) in app.by_type(T_STYLE_COLLECTION) {
        let entries = || collection_entries(n);
        match n.text("name") {
            Some("Materials") => out.library_materials = Some(entries()),
            Some("Colors") => out.library_colors = Some(entries()),
            _ => {}
        }
    }
}

fn collection_entries(n: &DecodedNode) -> Vec<String> {
    let lists = n
        .fields
        .iter()
        .filter(|f| f.name.as_deref() == Some("lst0"));
    lists
        .filter_map(|f| match &f.value {
            FieldValue::List(l) => Some(l),
            _ => None,
        })
        .flat_map(|l| l.elements.iter().flatten())
        .filter_map(|x| match &x.value {
            FieldValue::Text(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}
