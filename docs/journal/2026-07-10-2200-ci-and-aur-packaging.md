# CI pipeline and AUR packaging setup

Date: 2026-07-10  
Status: Historical record

## Why

The project owner asked for a CI pipeline and an AUR listing, and clarified the target is Arch only — no interest in other distros. See [ADR-0009](../decisions/0009-arch-only-scope-lock.md) for the formal scope decision this produced.

## What was set up

- `git init` locally; default branch renamed to `main`. No commit made yet — no git identity (`user.name`/`user.email`) was configured on this machine, and Claude Code's git safety rules forbid setting global git config on the user's behalf. The project owner needs to run this once themselves.
- `LICENSE-MIT` and `LICENSE-APACHE` added at the repo root, matching the dual license `Cargo.toml` already declared (`MIT OR Apache-2.0`) — this was already decided before this session, just never materialized as files.
- `.github/workflows/ci.yml`: runs `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo build --release`, and `cargo test --release` on every push to `main` and every pull request. This won't execute anywhere until the repo has a GitHub remote.
- `packaging/PKGBUILD`: a git-rolling (`whats-running-git`) AUR package draft. Chosen over a versioned package because there are no tagged releases yet — a numbered `whats-running` package sourced from release tarballs is a natural follow-up once a `v1.0.0` tag exists. `depends=('systemd' 'gcc-libs')` reflects the runtime's use of `systemctl`; no third-party Rust crates means `cargo fetch` has nothing to resolve. Bash syntax was checked (`bash -n`); a full `makepkg -si` build was **not** run, because this machine has `rustc`/`cargo` installed via `rustup`, not via the `pacman` `rust` package that `makedepends` requires — `makepkg` won't see it as satisfied. Full local verification needs either `pacman -S rust` or building in a clean chroot (`extra-x86_64-build`/devtools), and needs an actual GitHub remote for the VCS `source=` line to resolve.

## Still blocked on the project owner

1. Set git identity (`git config --global user.name`/`user.email`) — required before any commit can be made.
2. Confirm the GitHub repo name and that it should be created public under `Ethan-da-Tech-Wizard` (required: AUR's `source=` needs a URL it can `git clone`).
3. Confirm the AUR package naming/strategy (`whats-running-git` now, `whats-running` later off tagged releases — or skip straight to tagged releases).
4. Create an AUR account and upload an SSH key at aur.archlinux.org — this is a separate account from GitHub and nothing here can do it on the owner's behalf. Only after that can `git push` the package repo to `ssh://aur@aur.archlinux.org/whats-running-git.git`.

## What happens once unblocked

Local commit → push to a new GitHub repo → CI runs automatically on that push → `makepkg --printsrcinfo > .SRCINFO` generated from the PKGBUILD → both files pushed to the AUR's own git remote for the package name.
