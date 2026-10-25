//! The typed view of a project file (spec 19.3): options, asset libraries, search paths,
//! folder overrides, Vault settings and the XML kept verbatim, in the reference reader's shape.

use super::xml::Element;
use crate::error::{CorlissError, Result};
use crate::guid::Guid;
use crate::naming::guid_from_name;

/// What a project file says (schema revision 14, spec 19.3).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Project {
    /// `schemarevid`; 10 for pre-revision files with only `version`; `None` with neither.
    pub schema_revision: Option<i64>,
    /// `contentcenterguid` (Name26 in the file).
    pub content_center_guid: Option<Guid>,
    /// Add-in configurations (19.3.2): element name and the element's XML, kept verbatim.
    pub addin_configs: Vec<(String, String)>,
    /// Project options (19.3.3).
    pub options: ProjectOptions,
    /// Appearance asset libraries in list order (19.3.4).
    pub appearance_libraries: Vec<AssetLibrary>,
    /// Material (physical) asset libraries in list order (19.3.4).
    pub material_libraries: Vec<AssetLibrary>,
    /// Search paths and frequently used folders in file order (19.3.5).
    pub paths: Vec<ProjectPath>,
    /// Design data folder (19.3.6), from `DesignDataFolder` or the legacy `StylesFolder`.
    pub design_data: FolderOverride,
    /// Templates folder override.
    pub templates: FolderOverride,
    /// Presets folder override.
    pub presets: FolderOverride,
    /// Content Center files folder and configuration.
    pub content_center: FolderOverride,
    /// Vault settings (19.3.7).
    pub vault: VaultOptions,
    /// Custom sections (19.3.8): element name and the element's XML, kept verbatim.
    pub custom_sections: Vec<(String, String)>,
}

/// `ProjectOptions` (19.3.3), as written in the file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProjectOptions {
    /// `ProjectType` as written; see [`ProjectOptions::project_type`].
    pub project_type_text: String,
    /// `IncludedProject`: another .ipj whose paths and options this one inherits; empty = none.
    pub included_project: String,
    /// `OldVersionsToKeepOnSave`.
    pub old_versions_to_keep: i64,
    /// `UsingUniqueFilenames` = `Yes` (any case).
    pub using_unique_filenames: bool,
    /// `Owner`.
    pub owner: String,
    /// `ReleaseID`.
    pub release_id: String,
    /// `ImportedCompFolder`.
    pub imported_components_folder: String,
    /// `ImportedAsmFolder`.
    pub imported_assemblies_folder: String,
}

/// Project mode (Table 19-3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectType {
    /// Single-user project; also every value Inventor does not know.
    SingleUser,
    /// Shared multi-user project.
    Shared,
    /// Semi-isolated multi-user project.
    SemiIsolated,
    /// Vault project.
    Vault,
}

impl ProjectOptions {
    /// The project mode, compared case-insensitively as Inventor does [R-13].
    pub fn project_type(&self) -> ProjectType {
        match self.project_type_text.to_ascii_lowercase().as_str() {
            "shared" => ProjectType::Shared,
            "semi-isolated" => ProjectType::SemiIsolated,
            "vault" => ProjectType::Vault,
            _ => ProjectType::SingleUser,
        }
    }
}

/// One `ProteinLibrary` (19.3.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetLibrary {
    /// The .adsklib file; empty for the built-in libraries.
    pub path: String,
    /// `ProteinLibraryType` (Table 19-4): 1–3 built in or design data, 4 a user library.
    pub library_type: i64,
    /// The active library of its list.
    pub active: bool,
}

/// What a `ProjectPath` is (19.3.5, `pathtype`, compared case-sensitively).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathKind {
    /// The workspace.
    Workspace,
    /// A workgroup search path.
    Workgroup,
    /// A library search path.
    Library,
    /// A frequently used subfolder.
    FrequentlyUsedFolder,
}

impl PathKind {
    /// Every kind, in the order Inventor writes them.
    pub const ALL: [PathKind; 4] = [
        PathKind::Workspace,
        PathKind::Workgroup,
        PathKind::Library,
        PathKind::FrequentlyUsedFolder,
    ];

    /// The `pathtype` spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            PathKind::Workspace => "Workspace",
            PathKind::Workgroup => "Workgroup",
            PathKind::Library => "Library",
            PathKind::FrequentlyUsedFolder => "FrequentlyUsedFolder",
        }
    }
}

/// One `ProjectPath` (19.3.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectPath {
    /// `pathtype` as written; see [`ProjectPath::kind`].
    pub path_type: String,
    /// `PathName`, the display name.
    pub name: String,
    /// `Path`, absolute or relative to the project folder (see [`super::resolve_path`]).
    pub path: String,
    /// `VaultPath`, when written.
    pub vault_path: Option<String>,
}

impl ProjectPath {
    /// The kind, `None` for a `pathtype` Inventor does not know.
    pub fn kind(&self) -> Option<PathKind> {
        PathKind::ALL
            .into_iter()
            .find(|k| k.as_str() == self.path_type)
    }
}

/// A folder of `FolderOptions` (19.3.6).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FolderOverride {
    /// The folder; `None` = the application default.
    pub path: Option<String>,
    /// `VaultPath`, when written.
    pub vault_path: Option<String>,
    /// `UseStylesFolder` (design data only): `Yes`, `Read Only` or `No`.
    pub use_styles: Option<String>,
    /// `ContentCenterConfig` (Content Center only), kept verbatim.
    pub config_xml: Option<String>,
}

/// `VaultOptions` (19.3.7).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VaultOptions {
    /// `VaultServer`.
    pub server: String,
    /// `VaultDatabase`.
    pub database: String,
    /// `VaultVirtualFolder`.
    pub virtual_folder: String,
    /// `PublishFolder`.
    pub publish_folder: String,
}

impl Project {
    /// The paths of `kind` in search order.
    pub fn paths_of(&self, kind: PathKind) -> impl Iterator<Item = &ProjectPath> {
        self.paths.iter().filter(move |p| p.kind() == Some(kind))
    }

    /// The workspace path as written (the first `Workspace` entry).
    pub fn workspace(&self) -> Option<&str> {
        self.paths_of(PathKind::Workspace)
            .next()
            .map(|p| p.path.as_str())
    }

    /// Reads the typed view from a project file's root element.
    pub(super) fn from_root(root: &Element) -> Result<Project> {
        let mut p = Project {
            schema_revision: schema_revision(root),
            content_center_guid: content_center_guid(root)?,
            ..Project::default()
        };
        p.options.project_type_text = "None".into();
        p.options.old_versions_to_keep = 1;
        p.options.imported_components_folder = "Imported Components".into();
        p.options.imported_assemblies_folder = "Imported Components".into();
        for section in root.elements() {
            p.read_section(section);
        }
        Ok(p)
    }

    fn read_section(&mut self, s: &Element) {
        match s.name.as_str() {
            "AddInConfigs" => self.addin_configs = verbatim_children(s),
            "ProjectOptions" => self.options = read_options(s),
            "AppearanceProteinLibraries" => self.appearance_libraries = read_libraries(s),
            "PhysicalProteinLibraries" => self.material_libraries = read_libraries(s),
            "ProjectPaths" => {
                self.paths = s
                    .elements()
                    .filter(|e| e.name == "ProjectPath")
                    .map(read_path)
                    .collect()
            }
            "FolderOptions" => s.elements().for_each(|f| self.read_folder(f)),
            "VaultOptions" => self.vault = read_vault(s),
            "CustomSelections" => self.custom_sections = verbatim_children(s),
            _ => {}
        }
    }

    fn read_folder(&mut self, f: &Element) {
        let target = match f.name.as_str() {
            "DesignDataFolder" | "StylesFolder" => &mut self.design_data,
            "TemplatesFolder" => &mut self.templates,
            "PresetsFolder" => &mut self.presets,
            "ContentCenterFolder" => &mut self.content_center,
            _ => return,
        };
        f.elements().for_each(|k| read_folder_item(target, k));
    }
}

fn schema_revision(root: &Element) -> Option<i64> {
    match root.attribute("schemarevid") {
        Some(rev) => Some(wtol(&rev)),
        None => root.attribute("version").map(|_| 10),
    }
}

fn content_center_guid(root: &Element) -> Result<Option<Guid>> {
    let Some(name) = root
        .attribute("contentcenterguid")
        .filter(|n| !n.is_empty())
    else {
        return Ok(None);
    };
    let expected = "26 characters of a-z and 1-6 (Name26, spec 19.3)";
    guid_from_name(&name).map(Some).ok_or_else(|| {
        CorlissError::invalid(
            "project file",
            "contentcenterguid",
            0,
            format!("{name:?}"),
            expected,
        )
    })
}

fn text_of(parent: &Element, name: &str) -> String {
    parent.child(name).map(Element::text).unwrap_or_default()
}

fn verbatim_children(s: &Element) -> Vec<(String, String)> {
    s.elements().map(|e| (e.name.clone(), e.to_xml())).collect()
}

fn read_options(s: &Element) -> ProjectOptions {
    let kept = text_of(s, "OldVersionsToKeepOnSave");
    let project_type_text = s
        .child("ProjectType")
        .map_or_else(|| "None".to_string(), Element::text);
    ProjectOptions {
        project_type_text,
        included_project: text_of(s, "IncludedProject"),
        old_versions_to_keep: if s.child("OldVersionsToKeepOnSave").is_none() {
            1
        } else {
            strict_int(kept.trim())
        },
        using_unique_filenames: text_of(s, "UsingUniqueFilenames").eq_ignore_ascii_case("yes"),
        owner: text_of(s, "Owner"),
        release_id: text_of(s, "ReleaseID"),
        imported_components_folder: text_of(s, "ImportedCompFolder"),
        imported_assemblies_folder: text_of(s, "ImportedAsmFolder"),
    }
}

/// Inventor skips a library whose type or active flag is empty (19.3.4).
fn read_libraries(s: &Element) -> Vec<AssetLibrary> {
    let read = |e: &Element| {
        let (t, a) = (
            text_of(e, "ProteinLibraryType"),
            text_of(e, "ActiveProteinLibrary"),
        );
        let active = !matches!(a.trim(), "0" | "");
        (!t.is_empty() && !a.is_empty()).then(|| AssetLibrary {
            path: text_of(e, "Path"),
            library_type: wtol(&t),
            active,
        })
    };
    s.elements()
        .filter(|e| e.name == "ProteinLibrary")
        .filter_map(read)
        .collect()
}

fn read_path(e: &Element) -> ProjectPath {
    ProjectPath {
        path_type: e.attribute("pathtype").unwrap_or_default(),
        name: text_of(e, "PathName"),
        path: text_of(e, "Path"),
        vault_path: e.child("VaultPath").map(Element::text),
    }
}

/// An empty `Path` leaves the folder on its default (19.3.6).
fn read_folder_item(f: &mut FolderOverride, k: &Element) {
    match k.name.as_str() {
        "Path" if !k.text().is_empty() => f.path = Some(k.text()),
        "VaultPath" => f.vault_path = Some(k.text()),
        "UseStylesFolder" | "UseFolder" => f.use_styles = Some(k.text()),
        "ContentCenterConfig" => f.config_xml = Some(k.to_xml()),
        _ => {}
    }
}

fn read_vault(s: &Element) -> VaultOptions {
    VaultOptions {
        server: text_of(s, "VaultServer"),
        database: text_of(s, "VaultDatabase"),
        virtual_folder: text_of(s, "VaultVirtualFolder"),
        publish_folder: text_of(s, "PublishFolder"),
    }
}

/// `-?\d+` as a whole, else 0 (the reference reader's rule for `OldVersionsToKeepOnSave`).
fn strict_int(s: &str) -> i64 {
    let digits = s.strip_prefix('-').unwrap_or(s);
    let ok = !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit());
    if ok { s.parse().unwrap_or(0) } else { 0 }
}

/// `_wtol` (Table 19-2, Int): leading whitespace, an optional sign and the digits that follow;
/// no digits = 0.
pub(super) fn wtol(s: &str) -> i64 {
    let t = s.trim_start();
    let (sign, rest) = match t.as_bytes().first() {
        Some(b'-') => (-1, &t[1..]),
        Some(b'+') => (1, &t[1..]),
        _ => (1, t),
    };
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    sign * digits.parse::<i64>().unwrap_or(0)
}
