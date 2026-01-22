# Contributing to Ojo

Thank you for your interest in contributing to Ojo! This document provides guidelines and information for contributors.

## Development Setup

### Prerequisites

- Rust 1.70 or later (install via [rustup](https://rustup.rs/))
- Git
- SQLite 3 (for watcher and explorer)

### Getting Started

```bash
# Clone the repository
git clone https://github.com/camshaft/ojo.git
cd ojo

# Build the workspace
cargo build --workspace

# Run tests
cargo test --workspace

# Check code
cargo clippy --workspace

# Format code
cargo fmt --workspace
```

## Project Structure

Ojo is organized as a Cargo workspace with two main crates:

- **ojo-client**: Client library for event recording
- **ojo**: CLI tool with `watch` and `serve` subcommands

See [ARCHITECTURE.md](docs/ARCHITECTURE.md) for detailed architecture information.

## Development Workflow

### 1. Pick an Issue

Check the [issue tracker](https://github.com/camshaft/ojo/issues) for open issues. Look for:

- Issues labeled `good first issue` for beginners
- Issues labeled `help wanted` for community contributions
- The [PROJECT_PLAN.md](PROJECT_PLAN.md) for implementation roadmap

### 2. Create a Branch

```bash
git checkout -b feature/your-feature-name
# or
git checkout -b fix/your-bug-fix
```

### 3. Make Changes

- Write tests for new functionality
- Update documentation as needed
- Follow Rust naming conventions
- Run `cargo fmt` before committing
- Run `cargo clippy` and fix warnings

### 4. Test Your Changes

```bash
# Run all tests
cargo test --workspace

# Run specific crate tests
cargo test -p ojo-client

# Run with output
cargo test -- --nocapture

# Run benchmarks (if applicable)
cargo bench -p ojo-client
```

### 5. Submit a Pull Request

- Push your branch to GitHub
- Create a pull request with a clear description
- Link to related issues
- Wait for review and address feedback

## Code Style

### Rust Conventions

- Follow the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- Use `cargo fmt` with default settings
- Fix all `cargo clippy` warnings
- Add documentation comments for public APIs

### Documentation

````rust
/// Brief description of the function
///
/// More detailed explanation if needed.
///
/// # Examples
///
/// ```
/// use ojo_client::Tracer;
/// let tracer = Tracer::new(config)?;
/// ```
///
/// # Errors
///
/// Returns an error if...
pub fn some_function() -> Result<()> {
    // implementation
}
````

### Testing

- Write unit tests in the same file as the code
- Use descriptive test names: `test_buffer_wraps_correctly`
- Test error cases, not just happy paths
- Avoid trivial tests (e.g., testing that a getter returns what a setter set)

### Performance

For ojo-client specifically:

- Profile code paths on the hot path (event recording)
- Avoid allocations in performance-critical code
- Use atomic operations appropriately
- Document lock-free algorithms clearly

## Commit Messages

Follow conventional commit format:

```
<type>(<scope>): <subject>

<body>

<footer>
```

Types:

- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation changes
- `test`: Test additions or changes
- `refactor`: Code refactoring
- `perf`: Performance improvements
- `chore`: Maintenance tasks

Examples:

```
feat(client): implement lock-free ring buffer

Implements the core lock-free ring buffer using atomic operations
for multi-writer scenarios. Includes tests for concurrent access.

Closes #123
```

```
fix(watcher): handle file rename race condition

When files are renamed atomically, the watcher could miss them.
Added retry logic with exponential backoff.

Fixes #456
```

## Testing Guidelines

### Unit Tests

Test individual functions and components:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = TracerConfig::default();
        assert_eq!(config.buffer_size, 512 * 1024 * 1024);
    }
}
```

### Benchmarks

For performance-critical code (especially ojo-client):

```rust
// benches/event_recording.rs
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn benchmark_event_recording(c: &mut Criterion) {
    let tracer = create_tracer();

    c.bench_function("record_packet_sent", |b| {
        b.iter(|| {
            tracer.record_packet_sent(black_box(1), black_box(100));
        });
    });
}

criterion_group!(benches, benchmark_event_recording);
criterion_main!(benches);
```

## Documentation

### Code Documentation

- Document all public APIs with `///` doc comments
- Include examples in documentation
- Document panics, errors, and safety requirements
- Run `cargo doc --open` to preview

### Architecture Documents

When making significant changes:

- Update [ARCHITECTURE.md](docs/ARCHITECTURE.md)
- Update [PROJECT_PLAN.md](PROJECT_PLAN.md) if changing roadmap
- Add design documents for major features

## Performance Considerations

### ojo-client

The client library is performance-critical:

- **Hot path**: Event recording must be < 100ns
- **Zero allocations**: No heap allocations on hot path
- **Lock-free**: Use atomic operations, not mutexes
- **Benchmark**: Always benchmark performance changes

### ojo

The CLI tool should be efficient:

- Process files in reasonable time (< 1s per 1M events)
- Batch database inserts in transactions
- Use indexes for query performance
- Web server: query response < 100ms for typical queries
- Paginate large result sets
- Cache frequently accessed data

## Security

### Reporting Security Issues

**Do not** open public issues for security vulnerabilities.

Contact: [security contact information]

### Security Best Practices

- Never log or store sensitive data in traces
- Validate all file inputs in watcher
- Use parameterized queries in SQLite
- Sanitize user input in explorer web UI

## Release Process

(For maintainers)

1. Update version in `Cargo.toml` files
2. Update `CHANGELOG.md`
3. Create git tag: `git tag -a v0.1.0 -m "Release v0.1.0"`
4. Push tag: `git push origin v0.1.0`
5. Publish to crates.io: `cargo publish -p ojo-client`
6. Create GitHub release with notes

## Getting Help

- Read the [PROJECT_PLAN.md](PROJECT_PLAN.md) for overall vision
- Read [ARCHITECTURE.md](docs/ARCHITECTURE.md) for technical details
- Check [existing issues](https://github.com/camshaft/ojo/issues)
- Ask questions in issues or discussions

## Code of Conduct

Be respectful, inclusive, and constructive. We want Ojo to be a welcoming project for all contributors.

## License

By contributing to Ojo, you agree that your contributions will be licensed under the same license as the project (MIT).
