# Maintaining this fork

`main` follows `zeronsh/zeron:main` plus a linear series of fork-specific
commits. The Ubuntu 22.04 build and upstream sync are separate commits and
independent Actions. Future fork changes can use additional commits normally;
sync preserves their boundaries, authors, messages, and order without squash.

## One-time setup

1. Generate a dedicated SSH key pair for this workflow.
2. Add its public key under this fork's **Settings → Deploy keys**, with
   **Allow write access** enabled. Use it for this repository only.
3. Store its private key in **Settings → Secrets and variables → Actions** as
   the repository secret **UPSTREAM_SYNC_SSH_KEY**.
4. Ensure branch rules permit this deploy key to force-push `main`.
   Rebase rewrites the fork commits, so their SHAs change.

The default `GITHUB_TOKEN` cannot update workflow files when upstream changes
them. The dedicated deploy key avoids requiring an account-wide credential.
For rotation, replace both the deploy key and the secret; remove the old key.

## Sync on demand

Choose **Actions → sync-upstream → Run workflow → main**.

- Fetches the latest upstream and fork `main`.
- Finds all fork-only commits after the common ancestor; there is no count limit.
- Rejects merge commits in that range instead of flattening their history.
- Rebases the commits onto upstream without squashing or silently dropping any.
- Stops on conflicts or commits that become empty (for example, when upstream
  already incorporated that change); it never chooses a conflict side.
- Keeps commits that were intentionally empty before rebasing.
- Fast-forwards to upstream when there are no fork-only commits.
- Pushes only after a successful rebase, with an explicit `--force-with-lease`
  for the previously fetched fork tip. Concurrent changes make the push fail.
- Does nothing if the fork already contains the latest upstream commit.

Keep `[skip ci]` in the latest fork commit message: sync uses an SSH key, whose
pushes can otherwise start upstream's push-triggered CI/deploy workflows.
Manual sync and manual build dispatches still work with `[skip ci]`.

## Build for Ubuntu 22.04

After syncing, choose **Actions → build-ubuntu-22.04 → Run workflow → main**.
Download **linux-x86_64-ubuntu2204** from the completed run's artifacts.
This is a Linux x86_64 package containing the desktop app, background engine,
and a bundled ONNX Runtime library.
Sync and build are independent; successful sync does not prove the new source
still builds. The standard upstream release workflow builds on Ubuntu 24.04.

## If sync fails

Give AI the failed run URL and ask it to rebase the fork commits onto the latest
upstream while preserving separate commits. Conflicts, newly empty commits,
fork-only merge commits, permission errors, branch protection, and concurrent
pushes cause failure without a partial update to remote `main`. If upstream
already contains a change, review whether to drop that duplicate commit manually.

Use separate commits for separate changes; no amend or squash is required for
normal additions. Keep feature work on separate branches. Updating a local copy
after a sync requires fetching and rebasing local work onto the rewritten `main`.
