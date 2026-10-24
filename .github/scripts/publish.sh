#!/usr/bin/env bash
# Pushes image.tar.gz as $IMAGE:<image tag> and the chart as <chart version> to $CHARTS, its
# default image tag being <image tag>. Needs REGISTRY_USER and REGISTRY_TOKEN; runs helm and
# skopeo from nixpkgs.
set -euo pipefail
image_tag="$1"
chart_version="$2"
nix shell nixpkgs#skopeo -c skopeo copy \
  --dest-creds "$REGISTRY_USER:$REGISTRY_TOKEN" \
  docker-archive:image.tar.gz "docker://$IMAGE:$image_tag"
# shellcheck disable=SC2016 # expanded by the inner bash
nix shell nixpkgs#kubernetes-helm -c bash -euo pipefail -c '
  helm registry login ghcr.io -u "$REGISTRY_USER" --password-stdin <<< "$REGISTRY_TOKEN"
  helm package charts/netbox-provisioner --version "$1" --app-version "$2"
  helm push "netbox-provisioner-$1.tgz" "$CHARTS"' - "$chart_version" "$image_tag"
