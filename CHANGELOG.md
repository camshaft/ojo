# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Initial project setup with Cargo workspace structure
- `ojo-client` crate skeleton with public API design
- `ojo-watcher` crate skeleton with CLI interface
- `ojo-explorer` crate skeleton with web server structure
- Comprehensive PROJECT_PLAN.md document
- Architecture documentation (ARCHITECTURE.md)
- Contributing guidelines (CONTRIBUTING.md)
- Simple usage example
- Event type constants based on specification
- Basic configuration builders for all components

### Documentation
- README with project overview and quick start
- API documentation with examples
- Architecture diagrams and component descriptions

## [0.1.0] - TBD

Initial release (planned)

### Planned Features
- Lock-free ring buffer implementation
- Background flusher thread
- Binary format serialization/deserialization
- SQLite database transformation
- REST API for querying traces
- Basic web UI for visualization

[Unreleased]: https://github.com/camshaft/ojo/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/camshaft/ojo/releases/tag/v0.1.0
