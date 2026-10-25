# Changelog

All notable changes to Corliss are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.1.0] - 2026-10-23

### Added

- Added the `corliss` library. It reads Inventor documents (.ipt, .iam, .idw, .ipn, .ide)
  through the structural layers of the Inventor File Format Specification: container, property
  sets, database streams, UFRxDoc, segment M/B streams, blocks and node framing. It writes
  property-set edits, in-place node payload edits and referenced-file relinks, and keeps every
  unchanged byte.
- Added typed node decoding: `Document::decode_nodes` and `corliss::grammar` decode every node
  type into named, typed fields by running a snapshot of the specification's node grammars.
- Added `corliss::model`: `Part`, `Assembly` and `Drawing` read parameters, work geometry,
  sketches, features, occurrences, constraints, sheets, views, notes and dimensions.
- Added `corliss::acis`: reads the ACIS/ShapeManager bodies (SAB) of parts and assemblies into
  B-rep topology (bodies, faces, loops, edges, vertices) with analytic and B-spline geometry, plus
  point and distance evaluation.
- Added `corliss::edit` for structural edits (W3). It adds, removes and reorders nodes, sets
  typed fields, and rebuilds the M stream, B stream and RSeSegInfo entry of each edited
  segment. The operations are: adding a part user parameter; changing assembly constraint
  values, grounding and placements; adding and removing occurrences; removing and copying
  constraints; and editing, adding and deleting drawing notes, renaming sheets, and moving and
  scaling views.
- Added `corliss::ipj` for Inventor project files (.ipj, spec clause 19). It reads them into a
  lossless tree that writes back byte for byte, and into a typed `Project` (options, asset
  libraries, search paths, folder overrides, Vault settings, add-in and custom sections). It
  also checks them against the spec, writes new ones in Inventor's layout or the Default.ipj
  layout, and resolves relative paths against the project folder. `naming::guid_from_name`
  decodes the 26-character GUID names.
- Added the `inspect` example, which prints a document's segments and referenced files.
- Added CI and release automation: `cargo xtask pr` (the pull-request gate), `cargo xtask full`
  (the full gate the release matrix runs on Linux, macOS and Windows) and `cargo xtask release`
  (checks a release's CHANGELOG section and version), run by `.github/workflows/corliss.yml`.
  Pull requests run the PR gate and the full matrix on Linux, macOS and Windows.

[Unreleased]: https://github.com/Oblikovati/Corliss/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/Oblikovati/Corliss/releases/tag/v0.1.0
