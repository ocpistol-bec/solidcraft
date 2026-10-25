//! Participants of assembly features (as_feat: a hole).
//!
//! The features themselves are ordinary feature nodes in AmDcSegment, in their own 90874D63
//! object space under a 32C3DC38 container; the part feature reader decodes them. What is
//! assembly-specific is A0564E4F, which ties each feature (`ref_1`) to the occurrences it cuts.

use super::Assembly;
use crate::error::Result;
use crate::grammar::DecodedNode;
use crate::model::part::{Feature, Part};
use crate::model::view::{node_ref, ref_list};

/// The DC segment that holds assembly features.
const DC_SEGMENT: &str = "AmDcSegment";

/// An assembly feature: the part feature record and, when tied to occurrences, their labels.
#[derive(Debug, Clone, PartialEq)]
pub struct AssemblyFeature {
    /// The feature, read with the part feature reader.
    pub feature: Feature,
    /// Labels of the occurrences the feature acts on, when it names any.
    pub participants: Option<Vec<Option<String>>>,
}

/// Feature participants node: `ref_1` = feature, `lst0` → 1F213DE0 participant records.
const T_FEATURE_PARTICIPANTS: u32 = 0xA056_4E4F;
/// The participant record's `ref_2` source node has `key` = 0x30 + occurrence id.
const OCCURRENCE_KEY_BASE: i64 = 0x30;

/// The occurrences an assembly feature acts on.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureParticipants {
    /// AmDc node index of the feature.
    pub feature: Option<usize>,
    /// Labels of the participating occurrences.
    pub participants: Vec<Option<String>>,
}

impl Assembly<'_> {
    /// The assembly features with their participants.
    ///
    /// ```no_run
    /// # let doc = corliss::Document::open("assembly.iam")?;
    /// # let model = corliss::model::Model::load(&doc)?;
    /// for f in corliss::model::Assembly::new(&model)?.features()? {
    ///     println!("{:?} cuts {:?}", f.feature.name, f.participants);
    /// }
    /// # Ok::<(), corliss::CorlissError>(())
    /// ```
    pub fn features(&self) -> Result<Vec<AssemblyFeature>> {
        let participants = self.feature_participants();
        let part = Part::on_segment(self.model(), DC_SEGMENT)?;
        let features = part.features().into_iter().map(|feature| {
            let tied = participants
                .iter()
                .rev()
                .find(|p| p.feature == Some(feature.index));
            AssemblyFeature {
                participants: tied.map(|p| p.participants.clone()),
                feature,
            }
        });
        Ok(features.collect())
    }

    /// The participants of every assembly feature, in AmDc order (the `participants` that
    /// the reference reader adds to the part feature records).
    ///
    /// ```no_run
    /// # let doc = corliss::Document::open("assembly.iam")?;
    /// # let model = corliss::model::Model::load(&doc)?;
    /// for f in corliss::model::Assembly::new(&model)?.feature_participants() {
    ///     println!("feature #{:?} cuts {:?}", f.feature, f.participants);
    /// }
    /// # Ok::<(), corliss::CorlissError>(())
    /// ```
    pub fn feature_participants(&self) -> Vec<FeatureParticipants> {
        let by_id = self.names_by_id();
        let nodes = self.dc.by_type(T_FEATURE_PARTICIPANTS);
        nodes
            .map(|(_, n)| FeatureParticipants {
                feature: node_ref(n, "ref_1"),
                participants: self
                    .participant_ids(n)
                    .into_iter()
                    .map(|id| by_id.get(&id).cloned().flatten())
                    .collect(),
            })
            .collect()
    }

    /// Occurrence ids of the participant records of A0564E4F node `n`; records without a
    /// source key name no occurrence and are skipped.
    fn participant_ids(&self, n: &DecodedNode) -> Vec<i64> {
        let records = ref_list(n, "lst0").into_iter();
        let sources =
            records.filter_map(|p| self.node(Some(p)).and_then(|r| self.node_at(r, "ref_2")));
        let keys = sources.filter_map(|s| s.int("key")).filter(|k| *k != 0);
        keys.map(|k| k - OCCURRENCE_KEY_BASE).collect::<Vec<_>>()
    }
}
