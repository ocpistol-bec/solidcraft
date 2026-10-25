//! Writing a project file from its typed view in one of the two observed layouts (spec 19.2,
//! Table 19-1), as the reference writer renders it.

use super::parse::parse_xml;
use super::project::{AssetLibrary, FolderOverride, PathKind, Project};
use super::xml::{Element, escape};
use crate::naming::segment_stream_id;

/// The layouts seen in the field (19.2). Both are valid; the difference is free [R-12].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// Inventor's own writer (Xerces): `standalone="no"`, sorted attributes, `<X></X>`, blank
    /// lines between sections, a final LF [W-19].
    Inventor,
    /// The shipped Default.ipj: plain declaration, writer-order attributes, `<X/>`, no blank
    /// lines, no final LF.
    Default,
}

impl Layout {
    fn declaration(self) -> &'static str {
        match self {
            Layout::Inventor => "<?xml version=\"1.0\" encoding=\"utf-16\" standalone=\"no\" ?>",
            Layout::Default => "<?xml version=\"1.0\" encoding=\"utf-16\"?>",
        }
    }

    fn sorted(self) -> bool {
        self == Layout::Inventor
    }
}

/// UTF-16LE byte order mark [F-10].
pub(super) const BOM: [u8; 2] = [0xFF, 0xFE];

/// The project as a complete .ipj file in `layout`: UTF-16LE with a BOM, the elements of
/// Figure 19-1 in the writer's order [W-16].
pub(super) fn render(p: &Project, layout: Layout) -> Vec<u8> {
    let w = LayoutWriter { layout };
    let sections = [
        w.addins(p),
        w.options(p),
        w.libraries("AppearanceProteinLibraries", &p.appearance_libraries),
        w.libraries("PhysicalProteinLibraries", &p.material_libraries),
        w.paths(p),
        w.folders(p),
        w.vault(p),
        w.custom(p),
    ];
    let mut lines = vec![
        layout.declaration().to_string(),
        format!("<InventorProject{}>", w.attributes(root_attributes(p))),
    ];
    for section in sections {
        if layout == Layout::Inventor {
            lines.push(String::new());
        }
        lines.extend(section);
    }
    if layout == Layout::Inventor {
        lines.push(String::new());
    }
    lines.push("</InventorProject>".into());
    let mut text = lines.join("\n");
    if layout == Layout::Inventor {
        text.push('\n');
    }
    BOM.iter()
        .copied()
        .chain(text.encode_utf16().flat_map(u16::to_le_bytes))
        .collect()
}

fn root_attributes(p: &Project) -> Vec<(String, String)> {
    let mut out = vec![(
        "schemarevid".to_string(),
        p.schema_revision.unwrap_or(14).to_string(),
    )];
    if let Some(g) = &p.content_center_guid {
        out.push(("contentcenterguid".into(), segment_stream_id(g)));
    }
    out
}

struct LayoutWriter {
    layout: Layout,
}

impl LayoutWriter {
    /// A text element; empty ones as `<X/>` in the Default layout.
    fn text(&self, indent: &str, name: &str, value: &str) -> String {
        if value.is_empty() && self.layout == Layout::Default {
            return format!("{indent}<{name}/>");
        }
        format!("{indent}<{name}>{}</{name}>", escape(value, false))
    }

    fn attributes(&self, mut pairs: Vec<(String, String)>) -> String {
        if self.layout.sorted() {
            pairs.sort();
        }
        pairs
            .iter()
            .map(|(k, v)| format!(" {k}=\"{}\"", escape(v, true)))
            .collect()
    }

    /// Verbatim XML; Inventor re-parses it into elements whose attributes it keeps sorted
    /// [C-189], so its own layout sorts them.
    fn verbatim(&self, raw: &str) -> String {
        if !self.layout.sorted() {
            return raw.to_string();
        }
        let Ok(mut parsed) = parse_xml(raw) else {
            return raw.to_string();
        };
        sort_attributes(&mut parsed.root);
        format!("{}{}{}", parsed.prolog, parsed.root.to_xml(), parsed.epilog)
    }

    fn addins(&self, p: &Project) -> Vec<String> {
        let inner = p
            .addin_configs
            .iter()
            .map(|(_, raw)| format!("    {}", self.verbatim(raw)));
        wrap("  <AddInConfigs>", inner.collect(), "  </AddInConfigs>")
    }

    fn options(&self, p: &Project) -> Vec<String> {
        let o = &p.options;
        let kept = o.old_versions_to_keep.to_string();
        let unique = if o.using_unique_filenames {
            "Yes"
        } else {
            "No"
        };
        let fields = [
            ("ProjectType", o.project_type_text.as_str()),
            ("IncludedProject", &o.included_project),
            ("OldVersionsToKeepOnSave", &kept),
            ("UsingUniqueFilenames", unique),
            ("Owner", &o.owner),
            ("ReleaseID", &o.release_id),
            ("ImportedCompFolder", &o.imported_components_folder),
            ("ImportedAsmFolder", &o.imported_assemblies_folder),
        ];
        let inner = fields
            .iter()
            .map(|(n, v)| self.text("    ", n, v))
            .collect();
        wrap("  <ProjectOptions>", inner, "  </ProjectOptions>")
    }

    fn libraries(&self, tag: &str, libs: &[AssetLibrary]) -> Vec<String> {
        let mut inner = Vec::new();
        for lib in libs {
            inner.push("    <ProteinLibrary>".to_string());
            inner.push(self.text("      ", "Path", &lib.path));
            inner.push(self.text(
                "      ",
                "ProteinLibraryType",
                &lib.library_type.to_string(),
            ));
            inner.push(self.text(
                "      ",
                "ActiveProteinLibrary",
                if lib.active { "1" } else { "0" },
            ));
            inner.push("    </ProteinLibrary>".to_string());
        }
        wrap(&format!("  <{tag}>"), inner, &format!("  </{tag}>"))
    }

    /// Workspace entries first, then workgroups, libraries and frequently used folders; an
    /// unknown `pathtype` is not written, as Inventor drops it.
    fn paths(&self, p: &Project) -> Vec<String> {
        if p.paths.is_empty() {
            return vec!["  <ProjectPaths/>".into()];
        }
        let mut inner = Vec::new();
        for kind in PathKind::ALL {
            for pp in p.paths_of(kind) {
                inner.push(format!(
                    "    <ProjectPath{}>",
                    self.attributes(vec![("pathtype".into(), kind.as_str().into())])
                ));
                inner.push(self.text("      ", "PathName", &pp.name));
                inner.push(self.text("      ", "Path", &pp.path));
                inner.extend(
                    pp.vault_path
                        .as_deref()
                        .filter(|v| !v.is_empty())
                        .map(|v| self.text("      ", "VaultPath", v)),
                );
                inner.push("    </ProjectPath>".into());
            }
        }
        wrap("  <ProjectPaths>", inner, "  </ProjectPaths>")
    }

    fn folders(&self, p: &Project) -> Vec<String> {
        let mut inner = vec!["    <DesignDataFolder>".to_string()];
        inner.extend(
            p.design_data
                .path
                .as_deref()
                .map(|v| self.text("      ", "Path", v)),
        );
        inner.push(self.text(
            "      ",
            "UseStylesFolder",
            p.design_data.use_styles.as_deref().unwrap_or("Read Only"),
        ));
        inner.push("    </DesignDataFolder>".into());
        for (tag, f) in [
            ("TemplatesFolder", &p.templates),
            ("PresetsFolder", &p.presets),
        ] {
            let Some(path) = &f.path else { continue };
            inner.extend([
                format!("    <{tag}>"),
                self.text("      ", "Path", path),
                format!("    </{tag}>"),
            ]);
        }
        inner.extend(self.content_center(&p.content_center));
        wrap("  <FolderOptions>", inner, "  </FolderOptions>")
    }

    /// Inventor's writer joins `</ContentCenterConfig></ContentCenterFolder>` on one line.
    fn content_center(&self, cc: &FolderOverride) -> Vec<String> {
        let mut out = vec!["    <ContentCenterFolder>".to_string()];
        out.extend(cc.path.as_deref().map(|v| self.text("      ", "Path", v)));
        out.extend(
            cc.vault_path
                .as_deref()
                .filter(|v| !v.is_empty())
                .map(|v| self.text("      ", "VaultPath", v)),
        );
        match (&cc.config_xml, self.layout) {
            (Some(raw), Layout::Inventor) => out.push(format!(
                "      {}</ContentCenterFolder>",
                self.verbatim(raw)
            )),
            (Some(raw), Layout::Default) => out.extend([
                format!("      {}", self.verbatim(raw)),
                "    </ContentCenterFolder>".into(),
            ]),
            (None, _) => out.push("    </ContentCenterFolder>".into()),
        }
        out
    }

    fn vault(&self, p: &Project) -> Vec<String> {
        let v = &p.vault;
        let fields = [
            ("VaultServer", &v.server),
            ("VaultDatabase", &v.database),
            ("VaultVirtualFolder", &v.virtual_folder),
            ("PublishFolder", &v.publish_folder),
        ];
        let inner = fields
            .iter()
            .map(|(n, val)| self.text("    ", n, val))
            .collect();
        wrap("  <VaultOptions>", inner, "  </VaultOptions>")
    }

    fn custom(&self, p: &Project) -> Vec<String> {
        if p.custom_sections.is_empty() {
            return vec!["  <CustomSelections/>".into()];
        }
        let inner = p
            .custom_sections
            .iter()
            .map(|(_, raw)| format!("    {}", self.verbatim(raw)))
            .collect();
        wrap("  <CustomSelections>", inner, "  </CustomSelections>")
    }
}

fn wrap(open: &str, inner: Vec<String>, close: &str) -> Vec<String> {
    std::iter::once(open.to_string())
        .chain(inner)
        .chain(std::iter::once(close.to_string()))
        .collect()
}

fn sort_attributes(e: &mut Element) {
    e.attributes.sort_by(|a, b| a.name.cmp(&b.name));
    for c in &mut e.children {
        if let super::xml::Content::Element(child) = c {
            sort_attributes(child);
        }
    }
}
