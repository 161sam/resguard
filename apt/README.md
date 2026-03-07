# Resguard APT Repository Layout

This directory is a GitHub Pages-ready APT repository root.

Generated structure:

- `pool/main/r/resguard/`
- `dists/stable/main/binary-amd64/`
- `dists/stable/Release`
- `dists/stable/Release.gpg` (when signed)
- `dists/stable/InRelease` (when signed)
- `pubkey.gpg` (when exported)

Generate/update metadata:

```bash
./scripts/generate-apt-repo.sh \
  --repo-dir ./apt \
  --input-dir ./release-assets \
  --distribution stable \
  --component main \
  --arch amd64
```

Signed metadata:

```bash
./scripts/generate-apt-repo.sh \
  --repo-dir ./apt \
  --input-dir ./release-assets \
  --distribution stable \
  --component main \
  --arch amd64 \
  --sign-key <GPG_KEY_ID> \
  --export-pubkey ./apt/pubkey.gpg
```

Example source entry for GitHub Pages:

```text
deb [signed-by=/usr/share/keyrings/resguard-archive-keyring.gpg] https://<owner>.github.io/<repo> stable main
```
