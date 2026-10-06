# Same-commit prerelease client assets

`scripts/collect-release-assets.py` is a read-only CI asset collector. It does not
build, execute, install, sign, upload, tag, publish a Release, push an image, or
change credentials. The existing server publisher owns publication after these
checks pass. These packages remain test builds even when attached to a prerelease.

## Collection

Run from the exact checked-out release commit with Python 3.10+ and the official
GitHub CLI. `GH_TOKEN` needs `actions: read`; the collector itself does not need
write permissions. Repository and source SHA default to `GITHUB_REPOSITORY` and
`GITHUB_SHA`; explicit values may not conflict with those CI environment values.

```sh
python3 scripts/collect-release-assets.py \
  --repo "$GITHUB_REPOSITORY" --sha "$GITHUB_SHA" \
  --version 0.1.0-alpha.2 --output dist
```

The version must be a prerelease version without a `v` prefix. `--timeout-seconds`
defaults to 5400 and cannot exceed 90 minutes; `--poll-seconds` defaults to 30.
The timeout covers polling and GitHub commands, including downloads. Local
validation is size-bounded. A missing/queued/running eligible run is polled; a
completed failure/cancellation or a definitive current-attempt required-job
failure stops immediately. Retry collection only after the same-source checks
have been repaired/re-run. HTTP/authentication errors are fatal, never treated as
an empty result or as permission to fall back to another source.

`dist` may already contain unrelated server deliverables. Every client filename
must be absent. The complete client set is staged and verified before insertion;
files are installed with no-replace operations and `client-assets.json` is
installed last. No pre-existing file is overwritten. If a filesystem error
interrupts the final insertion, there can be partial client files without the
completion manifest; the publisher must stop and use a clean output directory.

## Source and CI gates

Both workflows must be successful for the exact full source SHA, in the current
repository and head repository, with event `push` and branch `main`:

- `.github/workflows/verify.yml`, display name `Verify Terminal`: Linux/macOS/
  Windows terminal matrix jobs, `mobile-native`, and `webrtc-loopback`
- `.github/workflows/test-packages.yml`, display name `Test Packages`: Windows
  x64, macOS arm64, macOS x64, Linux x64 Desktop jobs, and Android three-ABI job

The latest matching run is used, so an older success cannot hide a newer failed
run. Required job names form an exact allowlist; adding/renaming workflow jobs
requires updating the collector and tests. All effective jobs must succeed.
The collector enumerates **all job attempts**, selecting the latest execution
of each job. If only failed Windows jobs are re-run, the other four successful
jobs and their attempt-1 artifacts remain valid. GitHub can clone those retained
successful jobs into the new attempt with new job IDs: their `created_at` is after
their preserved `completed_at`. Such a clone may use an earlier artifact only if
one original successful execution record is present and its full start/end times,
complete step records (including step numbers and timestamps), source identity
and runner identity match exactly. The original execution must have been created
before it started. Artifact upload time must still fall inside that original
execution. Missing or ambiguous original evidence fails closed.

A genuinely re-run job must use its new execution's artifact; the collector never
substitutes an older artifact for that job. Provenance distinguishes the effective
latest job ID/attempt from the original producing job ID/attempt. Both records
are retained in `workflow_evidence` so offline verification reconstructs and
rechecks the same clone relationship instead of trusting a stored boolean.

Artifact names must include the complete SHA and exact producing job attempt.
Artifact ID, source workflow-run ID, repository IDs, source branch/SHA, expiry,
upload time within the successful job, and API digest are checked. Ambiguous,
missing, expired, foreign, mixed-commit or mismatched artifacts are rejected.
Android's exact `Verify APK metadata and debug signature` step must have completed
successfully, as must build/lint, packaging and artifact upload. Desktop packaging,
workspace tests, optimized build and help-start checks must have succeeded;
Linux/macOS also require the host-terminal step. Runs are rechecked after downloads
to detect concurrent re-runs.

Downloads use `gh run download RUN_ID --repo OWNER/REPO --name EXACT_NAME`; no
handwritten authorization header is forwarded to redirected storage hosts. The
GitHub CLI owns transport-ZIP download and safe extraction. The collector accepts
only the three expected normal, non-symlink, non-hardlinked downloaded files.

The API's GitHub artifact-envelope digest is **recorded, not independently
verified**: `gh run download` unwraps and discards that transport ZIP. This is
distinct from the package `.zip`/`.tar.gz` SHA-256, which is independently verified
against both its external checksum and external build summary. Do not describe
recorded artifact-envelope digests as independently checked or as attestations.

## Package and output checks

The collector checks exact internal file inventory, matching inner/outer build
metadata, full source SHA, target, expected profile, producing run/attempt, runner
OS/architecture, every file's size/mode/SHA-256, and internal `SHA256SUMS`.
Extraction streams only allowlisted ordinary files into a private directory.
Absolute/noncanonical paths, traversal, links, special files, duplicate entries,
unexpected modes, oversized expansion, encrypted ZIP content, and mismatches fail.
APK entries also receive path/type/duplicate/size checks. The existing packager's
PE, ELF, Mach-O and APK/native-library inspectors are run again; binaries and APKs
are never executed. The Android APK must contain exactly `arm64-v8a`, `x86_64` and
`x86`, each with both required native libraries.

Output contains 19 client files:

- Four original desktop archives, each with original `.sha256` and
  `.build-info.json` (12 files)
- Original Android package ZIP and its two sidecars (3 files)
- Bare `aTerminal-VERSION-android-debug-SHA12.apk` and `.sha256` (2 files)
- `client-assets.json` and `CLIENT-SHA256SUMS` (2 files)

The bare APK is byte-identical to the verified packaged APK, retaining its
signature. `CLIENT-SHA256SUMS` covers all 17 deliverable/sidecar files and the client
manifest. It excludes itself and unrelated server files. The server publisher
may create a separate global `SHA256SUMS` covering the final full release set.

`client-assets.json` schema version 1 contains:

- `source_commit`, `repository`, `version`, `collected_at_utc`
- `test_packages_run_id`, required `platforms`, successful `workflow_evidence`
- `assets`: 17 records with `name`, `sha256`, `size`, `kind`, `target`,
  `source_commit`, and original `artifact_id`
- `sources`: original artifact metadata/IDs/digests, producing job/attempt,
  effective latest job/attempt and retained-execution status, successful required
  steps, binary/APK inspection and signing limitations
- `warnings`: mandatory prerelease, signing and evidence limitations

The script prints a JSON result to stdout containing the chosen Test Packages
run ID, manifest filename, source SHA, version, platform list and delivered asset
hashes. Progress/errors go to stderr; failure returns a nonzero exit status.

## Offline publisher preflight

Before any registry or Release mutation, the publisher should run:

```sh
python3 scripts/collect-release-assets.py --verify-only \
  --repo "$GITHUB_REPOSITORY" --sha "$GITHUB_SHA" \
  --version 0.1.0-alpha.2 --output dist
```

This requires no token, GitHub CLI, or network. It rechecks the exact manifest
identity/version, five-platform inventory, stored successful run/job/artifact
provenance, required signing-step evidence, every output checksum, package inner
contents and binary/APK structure, and bare-APK identity. It validates artifact
expiry against collection time; delivered files remain verifiable after the
Actions retention period. Other server files in `dist` are left for the server
publisher's own validation.

Offline verification proves consistency with recorded evidence, **not a new
GitHub API observation**. Local metadata and ordinary hashes are not independent
cryptographic attestations. Protect the workspace and workflow permissions;
validate the exact commit and complete source checks before publication.

## Signing and testing limits

Desktop archives contain optimized CLI executables, not GUI installers. They
have no distribution-identity signing or macOS notarization; linker ad-hoc
signatures may exist. Follow normal OS/organization approval procedures and do
not disable platform security checks. Linux is a glibc/system-library-dependent
build, not a portable static musl executable.

Android is a debuggable APK using the temporary CI runner's debug key, unsuitable
for store/production distribution. No release key is created or retained here.
A different debug key can prevent an in-place update; uninstalling an old package
would delete local app data. Assess and back up data first; do not automatically
uninstall. Signature checking is evidenced by the successful source CI step and
is not independently re-run by this collector.

No iOS distribution, production endpoint, real-device installation, user session,
account connectivity or long-running runtime acceptance is implied by collection.

## Offline regression tests

```sh
python3 scripts/package-test-artifact.py --self-test
python3 scripts/test-collect-release-assets.py
```

Fixtures are synthetic and not runnable. Tests cover all five package targets,
run source filters, missing/failed jobs, HTTP/authentication failure, expired and
wrong-source artifacts, failed-job re-runs retaining earlier successful outputs,
required Android signature evidence, hashes, unsafe archive paths/links/duplicates,
modified outputs, no-clobber integration with server files and offline preflight.
They never contact GitHub or publish anything.

The reduced public API fixture `scripts/fixtures/test-packages-retained-jobs.json`
records only job/step execution and artifact-provenance fields for Test Packages
run `37411053445`, commit `a8291d2c87939bf5a280bb232ee66991480d24a8`.
Its source API references are recorded in the fixture. That actual run's final
Intel retry was cancelled, so it is explicitly rejected. A separate regression
changes only that final job's hypothetical conclusion to test retained-clone
selection and evidence round-tripping. Additional tests remove or corrupt the
original execution, change step timestamps, and model a genuinely new execution;
none may reuse an old artifact. No actor/profile/avatar data is included.
