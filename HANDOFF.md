# Handoff: Windows installer (issue #15)

For an agent working on **Windows**. Delete this file in the last commit before opening the PR.

## Your task

Finish the Windows MSI installer for parterre: build it, test it, fix what's broken, write the
docs, and open a PR that closes #15. A Linux agent drafted everything on this branch, but WiX
only runs on Windows, so **nothing here has been built or installed yet**. Treat it as a draft:
change whatever turns out wrong.

Read first:

- `CLAUDE.md`: project rules. `TODO.md` numbering is strict.
- Issue #15 (`gh issue view 15`): the spec.
- `docs/distribution.md`, section "Windows": why it's WiX v7, a dual-purpose MSI, and unsigned.

## Branch state

`wip/windows-installer` is based on PR #31 (`t3code/add-binary-file-properties`), which adds
version information (the Details tab) to `parterre.exe`. Once #31 is merged, rebase onto `main`
(`git rebase --onto origin/main 196d23d`) so your PR holds only the installer.

The WIP commit holds:

| File | What it is | Checked on Linux |
|---|---|---|
| `packaging/windows/parterre.wxs` | The MSI source (WiX v4 schema, which v7 still uses) | Compiles up to a Linux-only path error; one real error (WIX0230) fixed |
| `packaging/windows/build-msi.ps1` | Builds the MSI; gathers the files unless given `-Stage` | Parses in pwsh; version extraction works |
| `.github/workflows/ci.yml` | Builds `dist/parterre.msi` on `windows-latest`, uploaded with the other files | Never run |
| `.github/workflows/release.yml` | Builds `parterre-X.Y.Z-x86_64-pc-windows-msvc.msi` next to the zip | Never run |
| `crates/parterre-core/src/git/program.rs` | Finds Git for Windows when `git.exe` isn't on PATH | Unit tests pass; clippy passes for `x86_64-pc-windows-gnu`; never run on Windows |

## What the MSI should do (from #15)

- **Dual-purpose** (`Scope="perUserOrMachine"`). By default it installs per-user with no admin
  prompt, into `%LOCALAPPDATA%\Programs\parterre`. With `ALLUSERS=1` (which Chocolatey passes)
  it installs machine-wide into `C:\Program Files\parterre`.
- **Installs:** `parterre.exe`, `LICENSE`, `NOTICE` and `THIRD-PARTY-NOTICES.html`.
- **Start menu:** one shortcut, no folder. No desktop shortcut.
- **PATH:** the install folder goes on the user's PATH for a per-user install, and on the system
  PATH for a machine-wide one.
- **Settings → Apps:** shows the icon, the repo URL and the issues URL, with no Modify button.
  `ARPINSTALLLOCATION` is set, which winget reads.
- **Upgrades:**
  - A fixed `UpgradeCode`, and major upgrades.
  - The same version upgrades too (`AllowSameVersionUpgrades`), because `0.5.0-rc.1` and
    `0.5.0` both become MSI version `0.5.0`.
  - Installing an older version over a newer one is refused.
- **Manufacturer:** Trustfall AB. The copyright stays with Mattias Åslund.
- **Unsigned** for now.

## Setup

```powershell
dotnet tool install --global wix --version 7.0.0
cargo install cargo-about --locked       # build-msi.ps1 uses it when no -Stage is given
cargo build --release
packaging\windows\build-msi.ps1          # → target\msi\parterre-<version>-x86_64-pc-windows-msvc.msi
```

`build-msi.ps1` passes `-acceptEula wix7` on every run. That accepts WiX's Open Source
Maintenance Fee EULA, which asks a fee only of users with revenue from it, so parterre is
exempt (see `docs/distribution.md`). Don't run `wix eula accept`, which writes an acceptance
file.

## Checklist

Keep install logs (`msiexec /i <msi> /l*v install.log`) whenever something fails.

1. **Build.** `build-msi.ps1` produces an MSI. On Linux WiX also reported `WIX0389: The
   Directory/@Name attribute's value, 'parterre', is not a relative path` and complained about
   backslashes in `File/@Source`. Both are believed to be Linux artefacts; confirm they're gone.
2. **Validate.** Run `wix msi validate <msi>`. Dual-purpose packages tend to raise ICE38, ICE64
   and ICE91 warnings. Fix what's real, and write down anything you knowingly accept.
3. **Per-user install** (`msiexec /i <msi>`, no admin prompt expected):
   - The files are in `%LOCALAPPDATA%\Programs\parterre`.
   - The Start menu shows "parterre" with the icon.
   - A **new** terminal finds `parterre --version`, with the install folder on the **user**
     PATH (`[Environment]::GetEnvironmentVariable('Path','User')`) and not on the system PATH.
   - Settings → Apps shows parterre by Trustfall AB, with icon and version.
4. **Uninstall** it again, through Settings → Apps and through `msiexec /x <msi>`. The files, the
   folder, the shortcut, the PATH entry and `HKCU\Software\Trustfall AB\parterre` are all gone.
5. **Machine-wide install** (`msiexec /i <msi> ALLUSERS=1`, from an elevated shell):
   - The files are in `C:\Program Files\parterre`.
   - The **system** PATH has the folder, and the user PATH doesn't.
   - The shortcut is in the all-users Start menu.
   - Uninstall cleans up everything.
   - **Most uncertain part:** the two PATH components are chosen by `ALLUSERS = 1` and
     `NOT (ALLUSERS = 1)`. It isn't certain what value `ALLUSERS` holds after Windows
     Installer decides between per-user and machine-wide. If the wrong PATH is changed, check
     the log for the final `ALLUSERS` and `MSIINSTALLPERUSER` values and fix the conditions.
6. **Upgrades:**
   - Build a second MSI with a higher version, e.g. by temporarily passing `-d Version=0.4.1`
     inside the script. Install it over the first: there's one entry in Apps, the new version,
     with PATH and the shortcut intact.
   - Reinstalling the **same** version also replaces it.
   - Installing the older MSI over the newer one fails with the downgrade message.
7. **Git for Windows lookup** (`program.rs`):
   - With `git.exe` on PATH, parterre runs plain `git`.
   - With Git for Windows' folders removed from PATH, parterre started in a repository still
     loads the graph. It finds `InstallPath` under `SOFTWARE\GitForWindows` (HKCU, then HKLM,
     both registry views), or else the default folders.
   - With no git at all, the error still says git couldn't be run.
   - `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` pass
     on Windows.
8. **Details tab (for PR #31, if it isn't merged yet).** Right-click the MSVC-built
   `target\release\parterre.exe` → Properties → Details. You should see product name parterre,
   file version `0.4.0.0`, product version equal to `parterre --version`, and "Copyright (C)
   2026 Mattias Åslund" with a correct Å. Report the result on PR #31.
9. **CI.** Push and check that the `Windows installer` step in `ci.yml` passes, and that the
   `parterre-windows-latest` artifact contains `parterre.msi`. The `release.yml` step only runs
   on tags, so review it by eye: same script, `-Stage` set to the zip's folder, output in
   `dist/`.

## Docs to write

- `docs/building.md`: a "Windows installer" section covering WiX setup, `build-msi.ps1`, and
  the per-user and machine-wide modes.
- `docs/releasing.md`: releases now include the `.msi`.
- `TODO.md`:
  - Note #15 progress in the Distribution item.
  - Add the decisions below as open questions (HITL). They take the next numbers noted in
    `TODO.md`; bump that note in the same change.
- Anything where you deviate from #15 or `docs/distribution.md`: say so in a comment and in
  `TODO.md`.

### Decisions in the draft the maintainer hasn't confirmed

- **No installer UI.** A plain MSI shows only a progress bar, which suits winget and Chocolatey.
  Someone downloading it from GitHub sees no welcome or finish page. Adding WixUI would need
  the `WixToolset.UI.wixext` extension.
- **Registry key** `Software\Trustfall AB\parterre` (HKCU or HKLM), used only as component key
  paths.
- **The Start menu shortcut is of little use until #12** (*Open repository…*): started outside
  a repository, parterre shows nothing. #15 accepts this.

## Before the PR

- Delete `HANDOFF.md`.
- Run `cargo fmt --all` and clippy.
- Rebase onto `main` if #31 has merged.
- PR against `main`, "Closes #15". End the PR body with the attribution line your harness asks
  for.
