#!/usr/bin/env bash
# Deletes snapshots from GHCR, rather too many than too few (they are for a quick try only):
# every package version whose tags are all snapshot tags (image `sha-*`, chart `*-snapshot.*`)
# except the newest, and every untagged one. A version GHCR deletes goes with all its tags,
# and a release image shares its digest with the snapshot of its commit: one with any other
# tag stays.
# DRY_RUN=true only lists. Needs GH_TOKEN; runs gh and jq.
set -euo pipefail
dry_run="${DRY_RUN:-false}"

# the ids to delete of the versions on stdin (the API's JSON array), by tag pattern $1
select_old() {
  jq -r --arg pattern "$1" '
    map(select(.metadata.container.tags | all(test($pattern))))
    | (map(select(.metadata.container.tags != [])) | max_by(.created_at) | .id) as $newest
    | map(select(.id != $newest))
    | .[] | "\(.id) \(.metadata.container.tags | join(","))"'
}

clean() {
  local package="$1" pattern="$2"
  gh api --paginate "$base/${package//\//%2F}/versions" | jq -s add | select_old "$pattern" |
    while read -r id tags; do
      echo "$package: ${tags:-untagged} ($id)"
      [ "$dry_run" = true ] || gh api -X DELETE "$base/${package//\//%2F}/versions/$id"
    done
}

if [ "${1:-}" = select ]; then
  select_old "$2"
else
  owner="${OWNER:?}"
  if [ "$(gh api "users/$owner" --jq .type)" = Organization ]; then
    base="orgs/$owner/packages/container"
  else
    base="users/$owner/packages/container"
  fi
  clean netbox-provisioner '^sha-[0-9a-f]+$'
  clean charts/netbox-provisioner '-snapshot\.'
fi
