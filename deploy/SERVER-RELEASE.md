# Linux Server prereleases

`Linux Server Images` builds the release `deploy/Dockerfile` on native Linux amd64 and arm64 runners. Each build runs the server Cargo tests, then checks the actual image: non-root UID/GID, missing-credential refusal, read-only root filesystem, dropped capabilities, health endpoint, unauthorized admin access, SQLite creation and restart. A successful Actions run includes a compressed Docker image archive, SHA-256 checksum, image ID and exact source commit for each platform. These are server images, not Desktop Agent images.

## Publication

`deploy/server-release.json` records the prerelease version, exact DockerHub `namespace/repository`, expected existing visibility, and the publication switch. For v0.1.0-alpha.1 the owner approved public `yxf/aterminal`, with public `yixifeng/aterminal` as the only fallback if the primary is unavailable to the existing credentials. Public GitHub image archives are also approved. The first publication attempt verified that the `yxf` namespace was unavailable; the current release configuration therefore pins the approved `yixifeng/aterminal` fallback. Do not derive the DockerHub namespace from the GitHub owner.

After confirmation, enable publication in a reviewed PR. Only a successful main-branch run can publish, and only when this release configuration changes or the workflow is explicitly dispatched. Ordinary source changes build and test without republishing the previous version. Authentication uses only the existing `DOCKERHUB_USERNAME` and `DOCKERHUB_TOKEN` Actions secrets. The workflow verifies write permission and approved visibility. `allow_create_repository` permits creation of only the explicitly named image repository within an existing namespace the current credentials can manage. It never creates accounts/namespaces, changes existing visibility, purchases a plan, or expands access. Authentication, service, and version conflicts fail closed; only an unavailable destination or missing write permission permits the configured fallback. The DockerHub credential must allow read and write on that repository.

The workflow pushes exactly the smoke-tested image archives, verifies their registry config digests, and assembles an amd64/arm64 manifest. Tags are `v<prerelease>`, `v<prerelease>-amd64`, `v<prerelease>-arm64` and `sha-<full source commit>`. It never updates `latest` or a stable-version alias and refuses to replace existing tags. These tags are treated as immutable by this workflow; registry-wide immutability settings are not changed. Use the published digest for immutable pulls.

Only after registry verification succeeds does it create a GitHub prerelease at that same source commit, with the tested archives and distribution metadata. It never attaches Desktop/Android artifacts from a different commit. `server-image.json` records all image and manifest digests, and `SHA256SUMS` covers the delivered assets. No deployment is performed.

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
