# aTerminal cross-platform prereleases

`Linux Server Images` builds the release `deploy/Dockerfile` on native Linux amd64 and arm64 runners. Each build runs the server Cargo tests, then checks the actual image: non-root UID/GID, missing-credential refusal, read-only root filesystem, dropped capabilities, health endpoint, unauthorized admin access, SQLite creation and restart. A successful image build includes a compressed Docker image archive, SHA-256 checksum, image ID and exact source commit for each platform. These are server images, not Desktop Agent images.

## Desktop and Android downloads

Starting with `v0.1.0-alpha.2`, the same release includes Windows x64, macOS arm64 and Intel x64, Linux x64 desktop archives, and a directly downloadable Android APK. Every file must come from the exact tagged commit. A client failure blocks the whole new publication; artifacts from alpha1 or a different source commit are not substituted.

The desktop archives contain unsigned CLI test binaries and usage/license metadata. macOS builds are not notarized. The Android package contains arm64-v8a, x86_64 and x86 native libraries and is signed with an ephemeral CI debug key, not a store/release key. Updates require compatible signing identity: replacing an older differently signed test APK may require uninstalling it, which deletes local app data. Back up important data first; see the [Android signing documentation](https://developer.android.com/studio/publish/app-signing).

`client-assets.json` records exact source SHA, successful workflow/job/artifact provenance and delivered-file hashes. `CLIENT-SHA256SUMS` covers the client outputs; the release's global `SHA256SUMS` covers both clients and server images. The original package archives and build-info summaries are retained, and the bare APK is extracted only after its package is validated. Details and verification boundaries are in [client artifact collection](CLIENT-RELEASE-ASSETS.md).

## Publication

`deploy/server-release.json` records the prerelease version, exact DockerHub `namespace/repository`, expected existing visibility, and the publication switch. The owner approved public `yxf/aterminal`, with public `yixifeng/aterminal` as the only fallback if the primary is unavailable to the existing credentials. Public GitHub image archives are also approved. The first publication attempt verified that the `yxf` namespace was unavailable; the current release configuration therefore pins the approved `yixifeng/aterminal` fallback. Do not derive the DockerHub namespace from the GitHub owner.

After confirmation, enable publication in a reviewed PR. Only a successful main-branch run can publish, and only when this release configuration changes or the workflow is explicitly dispatched. Ordinary source changes build and test without republishing the previous version. Authentication uses only the existing `DOCKERHUB_USERNAME` and `DOCKERHUB_TOKEN` Actions secrets. The workflow verifies write permission and approved visibility. `allow_create_repository` permits creation of only the explicitly named image repository within an existing namespace the current credentials can manage. It never creates accounts/namespaces, changes existing visibility, purchases a plan, or expands access. Authentication, service, and version conflicts fail closed; only an unavailable destination or missing write permission permits the configured fallback. The DockerHub credential must allow read and write on that repository.

The workflow pushes exactly the smoke-tested image archives, verifies their registry config digests, and assembles an amd64/arm64 manifest. Tags are `v<prerelease>`, `v<prerelease>-amd64`, `v<prerelease>-arm64` and `sha-<full source commit>`. It never updates `latest` or a stable-version alias and refuses to replace existing tags. These tags are treated as immutable by this workflow; registry-wide immutability settings are not changed. Use the published digest for immutable pulls.

Before any registry push, the publisher waits for successful `Verify Terminal` and all five `Test Packages` jobs from a main push at the exact release SHA. It rejects incomplete, failed, expired, foreign, unsafe or mismatched packages. Bounded retries may retain earlier successful matrix-job artifacts from the same workflow and commit when provenance proves the execution. Only after package and registry verification succeeds does it create the GitHub prerelease at that same source commit, with actual desktop archives, a bare APK, tested server archives and distribution metadata. It never attaches artifacts from a different commit. `server-image.json` records all image and manifest digests, and `SHA256SUMS` covers the delivered assets. Public release verification compares every GitHub asset digest/size, checks anonymous download access and re-downloads both manifests. No deployment is performed.

If a run stops after a partial registry push, it deliberately refuses to overwrite those tags on a retry. Inspect the existing source commit and digests before recovering, or use a new prerelease version in a new reviewed commit. Do not delete or force-update tags blindly. A completed version remains unchanged; future releases require a new version.

## Run a published image

Read the prerelease's `server-image.json` for the verified pull reference. Replace `NAMESPACE/REPOSITORY@sha256:DIGEST` below with that reference. Verify downloaded offline archive checksums with `sha256sum -c SHA256SUMS`, then use `docker load -i <archive.tar.gz>` if needed. For offline use, replace the registry digest reference in the run command with the platform JSON’s `local_image` tag (`aterminal-server:test-amd64` or `aterminal-server:test-arm64`).

```sh
mkdir -m 700 -p secrets
openssl rand -hex 32 > secrets/admin-token
chmod 444 secrets/admin-token
docker volume create aterminal-server-data
docker run -d --name aterminal-server \
  --read-only --cap-drop=ALL --security-opt=no-new-privileges:true \
  --pids-limit=64 --memory=128m \
  -p 127.0.0.1:8787:8787 \
  -v aterminal-server-data:/data \
  --mount type=bind,src="$(pwd)/secrets/admin-token",dst=/run/secrets/admin_token,readonly \
  -e AI_TERMINAL_ADMIN_TOKEN_FILE=/run/secrets/admin_token \
  NAMESPACE/REPOSITORY@sha256:DIGEST
docker exec aterminal-server /usr/local/bin/ai-terminal-server --healthcheck
```

The token file must be readable by container UID 10001; the containing host directory remains private. Supply credentials only at runtime. The image has no default account, admin token, private LAN endpoint or bundled database. Back up `/data` including consistent SQLite/WAL state. A host bind mount instead of a named volume must be writable by UID/GID 10001.

HTTP is bound only to loopback in this example. Public access requires a TLS reverse proxy; see [deployment instructions](README.md). The built-in healthcheck assumes the default container port 8787. Keep that internal port and change host port mappings instead. Do not expose plaintext HTTP publicly.

This prerelease does not imply completion of public-network/NAT, long-term performance, mobile signing, or store-release acceptance.
