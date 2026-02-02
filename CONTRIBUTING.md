# Contributing to HumanSSH

Thank you for your interest in contributing to HumanSSH! This document provides guidelines and instructions for contributing.

## Development Setup

### Prerequisites

- Rust 1.75 or later
- macOS (primary development platform) or Linux
- A terminal that supports the features you're testing

### Building from Source

```bash
# Clone the repository
git clone https://github.com/manav03panchal/humanssh.git
cd humanssh

# Build in debug mode
cargo build

# Run the application
cargo run

# Build for release
cargo build --release
```

### Running Tests

```bash
# Run all tests
cargo test

# Run with verbose output
cargo test -- --nocapture
```

### Code Quality

Before submitting a PR, ensure your code passes all checks:

```bash
# Format code
cargo fmt

# Run clippy lints
cargo clippy -- -D warnings

# Check for compilation errors
cargo check
```

## Project Structure

```
src/
├── main.rs              # Entry point, window setup
├── lib.rs               # Module exports
├── config.rs            # Centralized configuration constants
├── theme.rs             # Theme system and terminal colors
├── actions.rs           # GPUI actions (Quit, CloseTab, etc.)
├── app/
│   ├── mod.rs           # App module exports
│   ├── workspace.rs     # Main workspace (tabs, dialogs)
│   ├── pane_group.rs    # Split pane tree structure
│   └── settings.rs      # Settings dialog UI
└── terminal/
    ├── mod.rs           # Terminal module exports
    ├── pane.rs          # Terminal pane (rendering, input)
    └── pty_handler.rs   # PTY process management
```

## Code Style

- Follow standard Rust formatting (`cargo fmt`)
- Use meaningful variable and function names
- Add doc comments for public APIs
- Keep functions focused and reasonably sized
- Prefer explicit error handling over `.unwrap()`

## Making Changes

1. **Fork the repository** and create a branch from `main`
2. **Make your changes** following the code style guidelines
3. **Add tests** if applicable
4. **Update documentation** if you change public APIs
5. **Run all checks** (`cargo fmt`, `cargo clippy`, `cargo test`)
6. **Submit a pull request** with a clear description

## Pull Request Guidelines

- Use a clear, descriptive title
- Reference any related issues
- Include a summary of changes
- Add screenshots for UI changes
- Ensure CI passes before requesting review

## Reporting Issues

When reporting bugs, please include:

- HumanSSH version
- Operating system and version
- Steps to reproduce
- Expected vs actual behavior
- Relevant error messages or logs

Enable debug mode for detailed logging:
```bash
HUMANSSH_DEBUG=1 cargo run
```

## License

By contributing to HumanSSH, you agree that your contributions will be licensed under the AGPL-3.0-or-later license.
