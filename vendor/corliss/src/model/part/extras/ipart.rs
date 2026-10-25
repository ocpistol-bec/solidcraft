//! iPart factory tables.
//!
//! 26287E96 member rows (a root "Table" row plus one per member: `rowIndex`, `selected` = the
//! active row) share a 182D1C8A workbook node whose blob is a complete .xlsx; its `ref_2`
//! names the 951388CF node holding the factory name.

use super::{text_of, truthy, xlsx};
use crate::grammar::DecodedNode;
use crate::model::part::Part;
use crate::model::view::node_ref;

/// iPart member row.
const T_IPART_ROW: u32 = 0x2628_7E96;

/// An iPart factory's members and table.
#[derive(Debug, Clone, PartialEq)]
pub struct IPartTable {
    /// Member rows (the root row excluded).
    pub members: Vec<IPartMember>,
    /// The shared workbook, when the rows reference one.
    pub workbook: Option<IPartWorkbook>,
}

/// One iPart member.
#[derive(Debug, Clone, PartialEq)]
pub struct IPartMember {
    /// Member name ("Part-01").
    pub name: Option<String>,
    /// 1-based table row.
    pub row: i64,
    /// True for the active member.
    pub active: bool,
}

/// The factory workbook.
#[derive(Debug, Clone, PartialEq)]
pub struct IPartWorkbook {
    /// The first worksheet's cells, when the blob holds an .xlsx.
    pub sheet: Option<IPartSheet>,
    /// Factory name.
    pub factory: Option<String>,
}

/// The table worksheet as text cells (shared strings resolved).
#[derive(Debug, Clone, PartialEq)]
pub struct IPartSheet {
    /// Header row: member column, then one column per varying value.
    pub columns: Vec<String>,
    /// One row per member.
    pub rows: Vec<Vec<String>>,
}

impl Part<'_> {
    /// The iPart table; `None` for a part that is not an iPart factory or member.
    ///
    /// ```no_run
    /// # let doc = corliss::Document::open("part.ipt")?;
    /// # let model = corliss::model::Model::load(&doc)?;
    /// let part = corliss::model::Part::new(&model)?;
    /// if let Some(table) = part.ipart_table() {
    ///     for m in &table.members {
    ///         println!("{:?} row {} active {}", m.name, m.row, m.active);
    ///     }
    /// }
    /// # Ok::<(), corliss::CorlissError>(())
    /// ```
    pub fn ipart_table(&self) -> Option<IPartTable> {
        let rows: Vec<&DecodedNode> = self.view().by_type(T_IPART_ROW).map(|(_, n)| n).collect();
        let first = rows.first()?;
        let members = rows.iter().filter_map(|n| ipart_member(n)).collect();
        let workbook = node_ref(first, "excelWorkbook")
            .and_then(|w| self.view().node(w))
            .map(|wb| self.ipart_workbook(wb));
        Some(IPartTable { members, workbook })
    }

    fn ipart_workbook(&self, wb: &DecodedNode) -> IPartWorkbook {
        let factory = node_ref(wb, "ref_2")
            .and_then(|f| self.view().node(f))
            .map(|f| text_of(f, "name"));
        IPartWorkbook {
            sheet: workbook_sheet(&wb.payload()),
            factory: factory.flatten(),
        }
    }
}

/// A member row; the root row (`rowIndex` -1) and a row 0 are not members.
fn ipart_member(n: &DecodedNode) -> Option<IPartMember> {
    let row = n.int("rowIndex").filter(|&r| r > 0)?;
    Some(IPartMember {
        name: text_of(n, "name"),
        row,
        active: truthy(n.get("selected")),
    })
}

/// The table in the .xlsx that starts at the first local-file signature of `blob`.
fn workbook_sheet(blob: &[u8]) -> Option<IPartSheet> {
    let start = blob.windows(4).position(|w| w == b"PK\x03\x04")?;
    let zip = &blob[start..];
    let text =
        |name: &str| xlsx::zip_entry(zip, name).map(|b| String::from_utf8_lossy(&b).into_owned());
    let strings = text("xl/sharedStrings.xml")
        .map(|x| xlsx::shared_strings(&x))
        .unwrap_or_default();
    let sheet = text("xl/worksheets/sheet1.xml").unwrap_or_default();
    let mut table = xlsx::sheet_rows(&sheet, &strings).into_iter();
    let columns = table.next().unwrap_or_default();
    Some(IPartSheet {
        columns,
        rows: table.collect(),
    })
}
