# The workshop's host

The cluster's hosts run wash v2.12.0 with one addition: the toolbox host
plugin (`toolbox/`), compiled in. wasmCloud's hosts are built from plugins.
Most ship with wash; a platform can add its own, which is what this is.

The toolbox serves the agent SDK's `tools` interface (`cosmonic:agent/tools`).
An agent imports `tools`, and the toolbox lists and calls the tool components
in that agent's own workload, each with its own grant. Cosmonic Desktop's
dispatcher works the same way, built into its host. It finds a workload's
tools in two ways:

- **Typed tools** (`curl`, `sqlite`, `gh`): the manifest lists the tool's
  interface under `hostInterfaces`, and a component exports it. The toolbox
  translates between the model's JSON and the tool's typed function, from a
  table written to match `wit/toolbox.wit`.
- **Toolsets** that speak `cosmonic:agent/tools` themselves, such as an MCP
  server composed with Cosmonic's MCP bridge
  (`components/talks-mcp/toolset.wac`): a component exports it under the
  plain name `cosmonic-toolset`, as on Desktop. The toolset lists its own
  tools.

The plugin has no network and no folders of its own.

## Building it (maintainers)

```sh
scripts/build-host.sh       # builds $HOST_IMAGE for this machine (scripts/env.sh)
```

`scripts/build-host.sh` fetches the wasmCloud v2.12.0 source into
`.upstream` (as `make up` does for the chart), copies it to `.run/host-src`,
applies `wash-v2.12.0.patch` (three lines: the crate joins the workspace,
wash depends on it, and the host registers the plugin), and builds
`Dockerfile`: upstream's build with the `host-component-plugins` feature, as
in upstream's all-features image. The first build takes a while; later ones
reuse cargo's cache. CI builds it for linux/amd64 on every pull request that
changes `host/` (`.github/workflows/host.yml`).

To test your own build, `HOST_IMAGE_DIGEST= make up` loads it instead of the
published image.

## Publishing it (maintainers)

CI publishes it, like the components in `artifacts/`, to this repo's
packages: `ghcr.io/cosmonic-labs/sandbox-workshop-kcna26/workshop-host`.
Push a tag named `workshop-host-<tag>` on the commit to build from:

```sh
git tag workshop-host-2.12.0-toolbox.3 && git push origin workshop-host-2.12.0-toolbox.3
```

The `host` workflow (`.github/workflows/host.yml`) builds both architectures
on native runners, publishes one multi-platform image, `workshop-host:<tag>`,
and prints the image and its digest in the run's summary. Pin them in
`scripts/env.sh` (`HOST_IMAGE`, `HOST_IMAGE_DIGEST`) and `k8s/values.yaml`
(`runtime.image`) together, in a commit after the tag.

Unlike `artifacts/`, the image is not reproducible byte for byte, so CI
can't check a pin against its own build: the pin is the digest CI published.
