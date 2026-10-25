//! Raw field values and the code tables feature records name them with.

use crate::grammar::{FieldValue, OtherValue};

/// A stored value as the reference reader holds it, for codes no table names.
#[derive(Debug, Clone, PartialEq)]
pub enum FeatureValue {
    /// A boolean field.
    Bool(bool),
    /// An integer field (NodeRefs keep their raw word, flag bit included).
    Int(i64),
    /// A float field.
    Float(f64),
    /// A text field.
    Text(String),
    /// Raw bytes.
    Bytes(Vec<u8>),
    /// An array or tuple.
    List(Vec<FeatureValue>),
}

impl FeatureValue {
    /// Converts a decoded field value.
    ///
    /// ```
    /// use corliss::grammar::FieldValue;
    /// use corliss::model::part::FeatureValue;
    /// assert_eq!(FeatureValue::from_field(&FieldValue::U16(3)), FeatureValue::Int(3));
    /// ```
    pub fn from_field(v: &FieldValue) -> FeatureValue {
        if let Some(list) = array_values(v) {
            return FeatureValue::List(list);
        }
        match v {
            FieldValue::Bool(b) => FeatureValue::Bool(*b),
            FieldValue::F32(_) | FieldValue::F64(_) => {
                FeatureValue::Float(v.as_f64().unwrap_or(0.0))
            }
            FieldValue::Text(s) | FieldValue::Text8(s) => FeatureValue::Text(s.clone()),
            FieldValue::Guid(g) => FeatureValue::Text(g.to_string()),
            FieldValue::Text8Bytes(b) => FeatureValue::Bytes(b.clone()),
            FieldValue::Other(_, o) => from_other(o),
            other => other
                .as_int()
                .map_or(FeatureValue::Bytes(Vec::new()), FeatureValue::Int),
        }
    }

    /// Python truthiness: zero, false, empty text and empty lists are false.
    pub fn truthy(&self) -> bool {
        match self {
            FeatureValue::Bool(b) => *b,
            FeatureValue::Int(i) => *i != 0,
            FeatureValue::Float(f) => *f != 0.0,
            FeatureValue::Text(s) => !s.is_empty(),
            FeatureValue::Bytes(b) => !b.is_empty(),
            FeatureValue::List(l) => !l.is_empty(),
        }
    }

    /// The integer a code table would match (Python dict keys equal 1, 1.0 and True alike).
    pub(crate) fn code(&self) -> Option<i64> {
        match self {
            FeatureValue::Int(i) => Some(*i),
            FeatureValue::Bool(b) => Some(i64::from(*b)),
            FeatureValue::Float(f) if f.fract() == 0.0 && f.abs() < 9.0e15 => Some(*f as i64),
            _ => None,
        }
    }
}

fn array_values(v: &FieldValue) -> Option<Vec<FeatureValue>> {
    if let Some(ints) = v.as_ints() {
        return Some(ints.into_iter().map(FeatureValue::Int).collect());
    }
    if let FieldValue::Color(c) = v {
        return Some(
            c.iter()
                .map(|x| FeatureValue::Float(f64::from(*x)))
                .collect(),
        );
    }
    v.as_f64s()
        .map(|f| f.into_iter().map(FeatureValue::Float).collect())
}

fn from_other(o: &OtherValue) -> FeatureValue {
    match o {
        OtherValue::None => FeatureValue::List(Vec::new()),
        OtherValue::Bool(b) => FeatureValue::Bool(*b),
        OtherValue::Int(i) => FeatureValue::Int(*i),
        OtherValue::Float(f) => FeatureValue::Float(*f),
        OtherValue::Str(s) => FeatureValue::Text(s.clone()),
        OtherValue::Bytes(b) => FeatureValue::Bytes(b.clone()),
        OtherValue::Tuple(t) => FeatureValue::List(t.iter().map(from_other).collect()),
    }
}

/// Declares a code table as an enum with an `Other` variant for unnamed codes.
macro_rules! feature_code {
    ($(#[$meta:meta])* $name:ident { $($code:literal => $variant:ident $label:literal),* $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq)]
        pub enum $name {
            $(#[doc = $label] $variant,)*
            /// A code the table does not name, as stored.
            Other(FeatureValue),
        }

        impl $name {
            /// Maps a stored code; `None` when the value is absent.
            pub(crate) fn from_value(v: Option<FeatureValue>) -> Option<Self> {
                let v = v?;
                Some(match v.code() {
                    $(Some($code) => Self::$variant,)*
                    _ => Self::Other(v),
                })
            }

            /// The reference reader's label, `None` for an unnamed code.
            pub fn label(&self) -> Option<&'static str> {
                match self {
                    $(Self::$variant => Some($label),)*
                    Self::Other(_) => None,
                }
            }
        }
    };
}

feature_code!(
    /// Boolean operation of a solid feature (729ABE28 value).
    FeatureOperation { 1 => NewSolid "new solid", 2 => Cut "cut", 3 => Join "join",
        4 => Intersect "intersect", 5 => Surface "surface" }
);
feature_code!(
    /// Extent of an extrusion, hole or extend (92637D29 value).
    FeatureExtent { 1 => Distance "distance", 2 => DistanceSymmetric "distance symmetric",
        4 => ToNext "to next", 5 => ThroughAll "through all", 7 => ToFace "to face",
        12 => DistanceFromFace "distance from face" }
);
feature_code!(
    /// Fillet type.
    FeatureFilletType { 0 => Edge "edge", 1 => Face "face", 2 => FullRound "full round" }
);
feature_code!(
    /// Face draft mode.
    FeatureDraftMode { 0 => FixedEdge "fixed edge", 2 => PartingLine "parting line", 3 => FixedPlane "fixed plane" }
);
feature_code!(
    /// Ruled surface type.
    FeatureRuledType { 0 => Normal "normal", 1 => Tangent "tangent", 2 => Vector "vector" }
);
feature_code!(
    /// Pattern creation method (BB1DD5DF value).
    FeatureCreationMethod { 0 => Identical "identical", 1 => Adjust "adjust", 2 => Optimized "optimized" }
);
feature_code!(
    /// Hole seat.
    FeatureHoleSeat { 0 => NoSeat "none", 1 => Countersink "countersink", 2 => Counterbore "counterbore" }
);
feature_code!(
    /// Split method.
    FeatureSplitMethod { 1 => TrimSolid "trim solid", 2 => SplitFaces "split faces", 3 => SplitSolid "split solid" }
);
feature_code!(
    /// Chamfer method.
    FeatureChamferType { 0 => Distance "distance", 1 => DistanceAndAngle "distance and angle",
        2 => TwoDistances "two distances" }
);
feature_code!(
    /// Shell direction.
    FeatureShellDirection { 0 => Inside "inside", 1 => Outside "outside", 2 => BothSides "both sides" }
);
feature_code!(
    /// Emboss operation.
    FeatureEmbossOperation { 3 => Emboss "emboss (join)", 2 => Engrave "engrave (cut)" }
);
feature_code!(
    /// Revolution extent.
    FeatureRevolveExtent { 1 => Angle "angle", 3 => Full "full" }
);
feature_code!(
    /// Revolution direction.
    FeatureRevolveDirection { 0 => Default "default", 1 => Flipped "flipped", 2 => Symmetric "symmetric" }
);
feature_code!(
    /// Coil method.
    FeatureCoilMethod { 0 => PitchAndRevolution "pitch and revolution" }
);
feature_code!(
    /// Loft type.
    FeatureLoftType { 0 => Rails "rails" }
);
feature_code!(
    /// Sweep orientation.
    FeatureSweepOrientation { 0 => Path "path" }
);
