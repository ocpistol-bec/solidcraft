//! The element table of spec 19.3 (Figure 19-1 and its field tables), as the reference
//! implementation lists it, with the reader's pattern lookup.

/// How an element's content is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Shape {
    /// Child elements.
    Struct,
    /// A repeated child.
    List,
    /// Character data.
    Text,
    /// No content (attributes only).
    Empty,
    /// XML kept verbatim; its inner structure is not Inventor's.
    Opaque,
    /// An element inside an opaque one that the table does not list.
    OpaqueInner,
}

/// An attribute's rule.
pub(super) struct AttrRule {
    pub name: &'static str,
    pub values: Option<&'static [&'static str]>,
    pub name26: bool,
}

/// An element's rule.
pub(super) struct ElementRule {
    pub pattern: &'static str,
    pub shape: Shape,
    pub attrs: &'static [AttrRule],
    pub values: Option<&'static [&'static str]>,
    pub integer: bool,
    pub since: Option<i64>,
    pub legacy: bool,
}

const fn el(pattern: &'static str, shape: Shape) -> ElementRule {
    ElementRule {
        pattern,
        shape,
        attrs: &[],
        values: None,
        integer: false,
        since: None,
        legacy: false,
    }
}

const fn attr(name: &'static str) -> AttrRule {
    AttrRule {
        name,
        values: None,
        name26: false,
    }
}

const YES_RO_NO: &[&str] = &["Yes", "Read Only", "No"];
const OPAQUE_INNER: ElementRule = el("", Shape::OpaqueInner);

/// The rules in lookup order: the first matching pattern wins.
static RULES: &[ElementRule] = &[
    ElementRule {
        attrs: &[
            attr("schemarevid"),
            AttrRule {
                name26: true,
                ..attr("contentcenterguid")
            },
            attr("version"),
        ],
        ..el("InventorProject", Shape::Struct)
    },
    ElementRule {
        since: Some(13),
        ..el("InventorProject/AddInConfigs", Shape::Struct)
    },
    ElementRule {
        attrs: &[AttrRule {
            name26: true,
            ..attr("configguid")
        }],
        ..el("InventorProject/AddInConfigs/*", Shape::Opaque)
    },
    el("InventorProject/ProjectOptions", Shape::Struct),
    ElementRule {
        values: Some(&["None", "Shared", "Semi-Isolated", "Vault"]),
        ..el("InventorProject/ProjectOptions/ProjectType", Shape::Text)
    },
    el(
        "InventorProject/ProjectOptions/IncludedProject",
        Shape::Text,
    ),
    ElementRule {
        integer: true,
        ..el(
            "InventorProject/ProjectOptions/OldVersionsToKeepOnSave",
            Shape::Text,
        )
    },
    ElementRule {
        values: Some(&["Yes", "No"]),
        ..el(
            "InventorProject/ProjectOptions/UsingUniqueFilenames",
            Shape::Text,
        )
    },
    el("InventorProject/ProjectOptions/Owner", Shape::Text),
    el("InventorProject/ProjectOptions/ReleaseID", Shape::Text),
    el(
        "InventorProject/ProjectOptions/ImportedCompFolder",
        Shape::Text,
    ),
    el(
        "InventorProject/ProjectOptions/ImportedAsmFolder",
        Shape::Text,
    ),
    el("InventorProject/AppearanceProteinLibraries", Shape::List),
    el("InventorProject/PhysicalProteinLibraries", Shape::List),
    el(
        "InventorProject/*ProteinLibraries/ProteinLibrary",
        Shape::Struct,
    ),
    el(
        "InventorProject/*ProteinLibraries/ProteinLibrary/Path",
        Shape::Text,
    ),
    ElementRule {
        values: Some(&["1", "2", "3", "4"]),
        integer: true,
        ..el(
            "InventorProject/*ProteinLibraries/ProteinLibrary/ProteinLibraryType",
            Shape::Text,
        )
    },
    ElementRule {
        values: Some(&["0", "1"]),
        ..el(
            "InventorProject/*ProteinLibraries/ProteinLibrary/ActiveProteinLibrary",
            Shape::Text,
        )
    },
    el("InventorProject/ProjectPaths", Shape::List),
    ElementRule {
        attrs: &[AttrRule {
            values: Some(&["Workspace", "Workgroup", "Library", "FrequentlyUsedFolder"]),
            ..attr("pathtype")
        }],
        ..el("InventorProject/ProjectPaths/ProjectPath", Shape::Struct)
    },
    el(
        "InventorProject/ProjectPaths/ProjectPath/PathName",
        Shape::Text,
    ),
    el("InventorProject/ProjectPaths/ProjectPath/Path", Shape::Text),
    el(
        "InventorProject/ProjectPaths/ProjectPath/VaultPath",
        Shape::Text,
    ),
    el("InventorProject/FolderOptions", Shape::Struct),
    el(
        "InventorProject/FolderOptions/DesignDataFolder",
        Shape::Struct,
    ),
    el(
        "InventorProject/FolderOptions/DesignDataFolder/Path",
        Shape::Text,
    ),
    ElementRule {
        values: Some(YES_RO_NO),
        ..el(
            "InventorProject/FolderOptions/DesignDataFolder/UseStylesFolder",
            Shape::Text,
        )
    },
    ElementRule {
        legacy: true,
        ..el("InventorProject/FolderOptions/StylesFolder", Shape::Struct)
    },
    ElementRule {
        legacy: true,
        ..el(
            "InventorProject/FolderOptions/StylesFolder/Path",
            Shape::Text,
        )
    },
    ElementRule {
        values: Some(YES_RO_NO),
        legacy: true,
        ..el(
            "InventorProject/FolderOptions/StylesFolder/UseFolder",
            Shape::Text,
        )
    },
    ElementRule {
        values: Some(YES_RO_NO),
        legacy: true,
        ..el(
            "InventorProject/FolderOptions/DesignDataFolder/UseFolder",
            Shape::Text,
        )
    },
    el(
        "InventorProject/FolderOptions/TemplatesFolder",
        Shape::Struct,
    ),
    el(
        "InventorProject/FolderOptions/TemplatesFolder/Path",
        Shape::Text,
    ),
    el("InventorProject/FolderOptions/PresetsFolder", Shape::Struct),
    el(
        "InventorProject/FolderOptions/PresetsFolder/Path",
        Shape::Text,
    ),
    el(
        "InventorProject/FolderOptions/ContentCenterFolder",
        Shape::Struct,
    ),
    el(
        "InventorProject/FolderOptions/ContentCenterFolder/Path",
        Shape::Text,
    ),
    el(
        "InventorProject/FolderOptions/ContentCenterFolder/VaultPath",
        Shape::Text,
    ),
    el(
        "InventorProject/FolderOptions/ContentCenterFolder/ContentCenterConfig",
        Shape::Opaque,
    ),
    el(
        "InventorProject/FolderOptions/ContentCenterFolder/ContentCenterConfig/ConfiguredLibraries",
        Shape::List,
    ),
    ElementRule {
        attrs: &[
            attr("DisplayName"),
            attr("ServerName"),
            attr("LibraryAttachName"),
            attr("InternalName"),
            AttrRule {
                values: Some(&["True", "False"]),
                ..attr("IsReadOnly")
            },
        ],
        ..el(
            "InventorProject/FolderOptions/ContentCenterFolder/ContentCenterConfig/ConfiguredLibraries/Library",
            Shape::Empty,
        )
    },
    el("InventorProject/VaultOptions", Shape::Struct),
    el("InventorProject/VaultOptions/VaultServer", Shape::Text),
    el("InventorProject/VaultOptions/VaultDatabase", Shape::Text),
    el(
        "InventorProject/VaultOptions/VaultVirtualFolder",
        Shape::Text,
    ),
    el("InventorProject/VaultOptions/PublishFolder", Shape::Text),
    ElementRule {
        since: Some(14),
        ..el("InventorProject/CustomSelections", Shape::Struct)
    },
    el("InventorProject/CustomSelections/*", Shape::Opaque),
];

/// The rule for element path `path` (`a/b/c`); inside an opaque element a path the table does
/// not list is [`Shape::OpaqueInner`]; `None` for an unknown element.
pub(super) fn rule_for(path: &str) -> Option<&'static ElementRule> {
    let parts: Vec<&str> = path.split('/').collect();
    // An exact entry wins over a pattern listed before it.
    let exact = RULES.iter().find(|r| r.pattern == path);
    if let Some(r) = exact.or_else(|| RULES.iter().find(|r| matches(r.pattern, &parts))) {
        return Some(r);
    }
    let inside_opaque = (1..parts.len())
        .rev()
        .any(|i| rule_for(&parts[..i].join("/")).is_some_and(|r| r.shape == Shape::Opaque));
    inside_opaque.then_some(&OPAQUE_INNER)
}

/// Each pattern part is a name, `*` (any name) or `*Suffix` (a name ending in Suffix).
fn matches(pattern: &str, parts: &[&str]) -> bool {
    let pat: Vec<&str> = pattern.split('/').collect();
    pat.len() == parts.len()
        && pat
            .iter()
            .zip(parts)
            .all(|(k, p)| k == p || k.strip_prefix('*').is_some_and(|s| p.ends_with(s)))
}
