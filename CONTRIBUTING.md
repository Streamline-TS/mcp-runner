# Contributing to mcp-runner

First off, thank you for considering contributing to `mcp-runner`! We welcome contributions from everyone.

This document provides guidelines for contributing to this project.

## How to Contribute

There are many ways to contribute, including:

*   Reporting bugs
*   Suggesting enhancements
*   Improving documentation
*   Writing tests
*   Submitting pull requests with code changes

## Getting Started

1.  **Fork the repository** on GitHub.
2.  **Clone your fork** locally: `git clone https://github.com/Streamline-TS/mcp-runner.git`
3.  **Navigate** into the project directory: `cd mcp-runner`
4.  **Build the project** to ensure everything is set up correctly: `cargo build`

## Finding Issues to Work On

Check the [GitHub Issues](https://github.com/Streamline-TS/mcp-runner/issues) page. Look for issues tagged with `good first issue` or `help wanted` if you are new.

You can also propose new features or improvements by creating a new issue.

## Making Changes

1.  **Create a new branch** for your changes: `git checkout -b my-feature-branch` (replace `my-feature-branch` with a descriptive name).
2.  **Make your code changes**. Ensure you add or update tests as necessary.
3.  **Run tests** to ensure your changes haven't broken anything: `cargo test`
4.  **Format your code** using the standard Rust formatter: `cargo fmt`
5.  **Lint your code** to check for common issues and ensure adherence to Rust best practices: `cargo clippy -- -D warnings`
6.  **Commit your changes** with a clear and descriptive commit message: `git commit -m "feat: Add new feature X"` (Consider using [Conventional Commits](https://www.conventionalcommits.org/)).
7.  **Push your branch** to your fork: `git push origin my-feature-branch`

## Submitting Pull Requests

1.  Go to the original `mcp-runner` repository on GitHub.
2.  Click on "New Pull Request".
3.  Choose your fork and the branch containing your changes.
4.  Provide a clear title and description for your pull request, explaining the changes you've made and why.
5.  Link any relevant issues (e.g., "Closes #123").
6.  Submit the pull request.

A maintainer will review your changes. Be prepared to discuss your changes and make adjustments if requested.

## Code Style

This project follows the standard Rust style guidelines enforced by `cargo fmt`. Please run `cargo fmt` before committing your changes.

## Testing

All contributions should include relevant tests. Ensure all tests pass by running `cargo test` before submitting a pull request.

## Linting

We use `clippy` for linting. Please ensure your code passes `cargo clippy -- -D warnings` (treat warnings as errors) before submitting.

## Code of Conduct

Please note that this project is released with a Contributor Code of Conduct. By participating in this project you agree to abide by its terms.

## License

By contributing, you agree that your contributions will be licensed under the terms specified in the [LICENSE](LICENSE) file.
