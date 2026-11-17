#!/bin/bash
# Run on an ephemeral runner; only public archive files enter the history branch.
set -euo pipefail
operation=$1
state=$2
: "${GH_REPO:?}" "${GH_TOKEN:?}"
case "$operation" in
  fetch)
    mkdir -p "$state"
    git -C "$state" init -b apt-history
    git -C "$state" remote add origin "https://github.com/$GH_REPO.git"
    gh auth setup-git
    # A failed lookup must not masquerade as an empty archive.
    refs=$(git -C "$state" ls-remote --heads origin refs/heads/distribution/apt-history)
    if test -n "$refs"; then
      git -C "$state" fetch --depth=1 origin distribution/apt-history
      git -C "$state" checkout -B apt-history FETCH_HEAD
    else
      # If Pages has an archive but its history branch vanished, fail closed.
      status=$(curl --proto '=https' --tlsv1.2 -sS -o /dev/null -w '%{http_code}' \
        "https://${GH_REPO%%/*}.github.io/${GH_REPO#*/}/apt/dists/preview/InRelease")
      test "$status" = 404 || { echo "Refusing first publication: public archive status $status, expected 404"; exit 1; }
    fi
    ;;
  push)
    test -f "$state/apt/dists/preview/InRelease"
    git -C "$state" config user.name 'github-actions[bot]'
    git -C "$state" config user.email '41898282+github-actions[bot]@users.noreply.github.com'
    git -C "$state" add apt
    git -C "$state" commit -m "Publish signed preview archive from $GITHUB_SHA ($GITHUB_RUN_ID)"
    # Normal fast-forward only; concurrent mutations fail rather than overwrite.
    git -C "$state" push origin HEAD:refs/heads/distribution/apt-history
    test "$(git -C "$state" rev-parse HEAD)" = "$(git -C "$state" ls-remote origin refs/heads/distribution/apt-history | cut -f1)"
    ;;
  *) exit 2 ;;
esac
