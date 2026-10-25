# Corliss

Corliss is a Rust library for reading and writing Autodesk Inventor documents (parts .ipt,
assemblies .iam, drawings .idw, presentations .ipn and iFeatures .ide) and their project files
(.ipj). CAD kernels use it to exchange data with Inventor files. Martensite is its first
consumer.

Corliss implements the *Inventor File Format Specification*. Clause and requirement numbers in
the code and the docs (`spec 11.2.1`, `[W-01]`) refer to it.

## What it does

| Layer | Spec | Read | Write |
| --- | --- | --- | --- |
| Compound-file container, root CLSID, document type | 7 | yes | yes |
| Property sets (iProperties) | 8 | yes | edit and add properties (W1) |
| RSeDb, RSeSegInfo, RSeDbRevisionInfo | 9 | yes | yes |
| UFRxDoc: referenced files, occurrences, states | 10 | yes | relink referenced files |
| Segment M streams: block table, type table, t4–t10 | 11 | yes | yes |
| B streams, blocks, end tables, template segments | 11–12 | yes | edit payloads in place (W2) |
| Node framing: common header, NodeRefs, masked matrices | 13, 4.5.1 | yes | yes |
| Node payloads: typed fields of every node type | 13–18 | yes | typed fields; add, remove and reorder nodes (W3) |
| Part, assembly and drawing models | 14–17 | yes | edit operations (`corliss::edit`) |
| ACIS bodies: B-rep topology and geometry | 13.6 | yes | no |
| Project files (.ipj): options, search paths, folders | 19 | yes | edit in place, write new files |

Corliss writes back every byte the caller does not change. Each stream keeps its original
compressed bytes and is recompressed only when its content changes, so a document that is
opened and saved without edits keeps every stream byte for byte.

Every node is decoded into named, typed fields by the specification's node grammars, which
Corliss embeds as a snapshot and runs in a small interpreter. On top of the fields, `corliss::model`
reads parts (parameters, work geometry, sketches, features …), assemblies (occurrences,
placements, constraints …) and drawings (sheets, views, notes, dimensions …). See
[docs/features/node-decoding](docs/features/node-decoding/README.md).

## Usage

```rust
use corliss::{Document, propset::{fmtid, PropertyValue}};

let mut doc = Document::open("part.ipt")?;
println!("{:?} saved with Inventor {:?}", doc.kind(), doc.rsedb()?.saved_with.release_year());

// Walk every segment's nodes.
for loc in doc.segments()? {
    let seg = doc.decode_segment(&loc)?;
    for block in seg.blocks() {
        println!("{} {:?} {} bytes", loc.name, seg.type_id(block), block.payload.len());
    }
}

// Set the part number (Design Tracking Properties, pid 5) and save.
let mut tracking = doc.property_set(&fmtid::DESIGN_TRACKING)?.expect("every document has it");
tracking.set_mut(&fmtid::DESIGN_TRACKING).unwrap().set(5, PropertyValue::Text("PN-001".into()));
doc.write_property_set(&tracking)?;
doc.save("part-renumbered.ipt")?;
```

Typed nodes and models:

```rust
use corliss::model::{Model, Part};

let doc = corliss::Document::open("part.ipt")?;
let nodes = doc.decode_nodes(&doc.segment_location("PmDCSegment")?)?;
println!("{} nodes, first: {:?}", nodes.nodes.len(), nodes.nodes[0].fields);

let model = Model::load(&doc)?;
let part = Part::new(&model)?;
for sketch in part.sketches() {
    println!("{:?}: {} lines", sketch.name, sketch.lines.len());
}
for feature in part.features() {
    println!("{:?} {}", feature.name, feature.kind.type_name());
}
```

`cargo run --example inspect -- file.iam` prints a document's segments and references.

## Testing

```bash
cargo test
```

The tests run against 298 documents saved by Inventor 2027 under `tests/fixtures/`. These are
the controlled single-change fixtures the format was reverse-engineered from. Every layer must decode and
re-encode to the exact bytes Inventor wrote. Every node must decode to the same fields as the
reference grammar engine, and every model method must give the same output as the reference
readers (`tests/fixtures/grammar-digests.tsv`, `tests/fixtures/models.json`).

Debug builds check the node decoding on a sample of the fixtures. Run
`cargo test --release` to check all of them.

## Limits

- Only Inventor 2027 samples are in the corpus. The zlib codec of Inventor 2025 and earlier is
  covered by synthetic re-encoding tests, not by real files.
- RSeDb schemas below 0x1D (before 2009) and M streams below version 7 are rejected.
- UFRxDoc branches that no sample shows (auxiliary persist data, state references, mirror links,
  BOM setting kinds other than 0 and 1) are rejected rather than guessed (spec 10.1).
- W3 edits are limited to the operations in `corliss::edit` (see
  [docs/features/structural-editing](docs/features/structural-editing/README.md)); each one is
  checked against the reference implementation's output on the fixtures.
- The `corliss::model` types read; edits go through `corliss::edit`. ACIS bodies are read into B-rep topology and geometry, but not written.

## License

GPL-2.0-only. See [LICENSE](LICENSE).

## Specification fragments

These are the parts of the specification that back the citations in this file. Each feature
document under [docs/features/](docs/features/) ends with the fragments for its own citations.

- **`spec 11.2.1`** in the second paragraph only shows how clause numbers are written; it backs
  no statement here.
- **[W-01]** (5.3): Writers MUST preserve every byte that the edit does not need to change:
  unknown streams, nodes and fields.
- **Clauses 7–19** (scope of each clause, from the specification's layer overview):
  - 7 Compound file container: every document is one Microsoft Compound File ([MS-CFB]) version
    3 with 512-byte sectors; the root storage's CLSID identifies the document type.
  - 8 Property sets: iProperties are ordinary OLE property sets ([MS-OLEPS]), one stream per set
    at the root of the container.
  - 9 Database streams: RSeDb (database header), RSeSegInfo (segment directory) and
    RSeDbRevisionInfo (revision history).
  - 10 Document record (UFRxDoc): the document's identity and save history, its
    representations, the files it references and one record per occurrence of them, its states.
  - 11 Segments: each segment is a pair of streams; `M<id>` holds the segment's name and block
    layout (header, block table, type table, t4–t10), `B<id>` holds the compressed block data.
  - 12 Blocks: a decompressed B stream is a run of blocks (header, payload, length echo,
    trailer) in block-table order, then a segment end table.
  - 13 Node structure: each block payload is one node, with a common header, a content header
    and fields; nodes point to each other by NodeRef.
  - 13.6 Embedded ACIS body: the `sab` token of node F645595C holds one ACIS / ShapeManager
    binary save file (SAB).
  - 14–18 Node families and document content: 14 shared node families, 15 part documents,
    16 assembly documents, 17 drawing documents, 18 presentation and iFeature documents.
  - 19 Project files (.ipj): an XML document that names the folders in which Inventor looks for
    the documents of a design, and the project's options.
- **5.3 Writers (editors)**: A conforming writer edits an existing document. Writer classes:
  W1 edits property sets only; W2 changes payload values of existing nodes in place, without
  changing any length; W3 adds or removes nodes.
- **4.5.1 MaskedMatrix4**: a row-major 4 × 4 matrix of f64 stored with two u16 masks; used by
  the sketch transform, the occurrence placement, the SM view placement, the scene transform
  and the graphics placements.
- **10.1 Stream layout** (conformance limits): Four branches that Inventor's writer has are not
  specified because no sample has them (OI-093): UFRxPersist auxiliary data
  (`persist_flag` = 1), BOM settings of a kind other than 0 or 1, state references
  (`n_refs` > 0) and mirror link records (`n_mirror_links` > 0). A reader that meets one of them
  MUST reject the stream rather than guess its length.

This library is named after **George Henry Corliss** (1817–1888) an American mechanical engineer, inventor, and entrepreneur whose dramatic improvements to the stationary steam engine provided the reliable, highly efficient mechanical power that fueled the rapid expansion of late 19th-century manufacturing. Patented in 1849, his famous "Corliss valve gear" utilized independent rotary valves and an automatic variable cut-off mechanism regulated by a governor, which minimized waste steam, optimized fuel consumption, and ensured smooth, precise rotational speeds under varying loads. This leap in thermodynamic efficiency freed industrial factories from their traditional reliance on water power, enabling widespread manufacturing in inland regions. Widely recognized as one of the most prominent engineers of the Industrial Revolution, Corliss gained international renown for his giant 1,400-horsepower "Centennial Engine"—a mechanical centerpiece that powered all the exhibits in Machinery Hall at the 1876 Philadelphia Exposition.
