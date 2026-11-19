# Vendored dependencies

These sources are committed directly so building equart needs no additional
repositories, submodules, or dependency preparation script. Cargo downloads the
remaining registry dependencies normally.

| Directory | Upstream | Revision | License |
| --- | --- | --- | --- |
| `nice-plug` | https://codeberg.org/RustAudio/nice-plug | `7197104c0d5c1a65be0cf53055efa7278f5df887` | ISC |

The nice-plug sources already include the DPI fixes that f0lock recorded in
`nice-plug-ui-dpi.patch`. Do not apply that patch again; keep the fixes when
updating upstream sources.

`baseview` and `egui-baseview` are also patched local sources, wired up through
`[patch.crates-io]` in `Cargo.toml`. Their individual license files and upstream
metadata are retained in their directories.
