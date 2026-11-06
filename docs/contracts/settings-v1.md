# settings-v1

Cross-repository settings contract for the tz-custom suite.

- Status: frozen for implementation on 2026-11-06.
- Owners: `tz-controller` (config app), `XRizer-tz-Custom` (AA), `dxvk-tz-custom` (VRS).
- Version: v1. The JSON files do not carry a version key; this document and the
  shared fixtures are the version.

Any change to a key, default, range, or semantic in this document must update
the fixtures under `tests/fixtures/settings-v1/` in all three repositories in
the same change set.

## 1. Files and path resolution

### XRizer AA

File name: `upscaler.json`.

The runtime and the config app use the same priority order:

1. `XRIZER_UPSCALER_CONFIG`
2. `$XDG_CONFIG_HOME/xrizer/upscaler.json`
3. `$HOME/.config/xrizer/upscaler.json`
4. `$XDG_RUNTIME_DIR/xrizer_upscaler.json`
5. `/tmp/xrizer_upscaler.json`

Only the highest-priority existing file is read. If no file exists, the
runtime uses the defaults in this document. The app creates a new file at the
highest-priority persistent path it can write.

### DXVK VRS

File name: `vrs.json`.

1. `DXVK_TZ_VRS_FILE`
2. `$HOME/.config/dxvk-tz-custom/vrs.json`

If no file exists, the runtime uses the defaults in this document. The runtime
does not create the file.

## 2. Common semantics

- The file is a UTF-8 JSON object.
- Missing keys use the documented default.
- Numeric values are clamped to the documented range on load. A malformed
  type is a parse error, not a value to coerce.
- Unknown keys are preserved by the config app and ignored by the runtimes.
- Old or deprecated keys are preserved as unknown keys. Neither the app nor
  the runtime deletes them.
- Malformed JSON: the runtime keeps its last good configuration; the config
  app refuses to save over the file and reports the error.
- Writes are atomic: write a temporary file in the same directory, flush and
  fsync it, then rename it over the target. The config app keeps one `.bak`
  generation.
- Both runtimes poll the file every 500 ms. A change is applied at a frame
  boundary; a parse failure does not replace the last good configuration.
- The config app saves only on explicit user action. It does not auto-save.
- If a file changes externally while the app has unsaved edits, the app shows
  a conflict and requires the user to reload or overwrite explicitly.

## 3. XRizer AA keys

All values are optional. The table is the authoritative default/range list.

| Key | Type | Default | Range | Meaning |
|---|---|---|---|---|
| `enabled` | bool | `false` | — | Enable the AA pass. |
| `antialias` | string | `"cmaa2"` | `off` / `cmaa2` / `smaa` / `fxaa` | AA method. |
| `weight_mode` | string | `"inverted"` | `center` / `inverted` / `whole` | Weight-field selector. |
| `center` | [x, y] | `[0.5, 0.5]` | each `0.0..=1.0` | Field centre in eye-texture UV. Shared by both eyes. |
| `center_offset` | number | `0.0` | `-0.20..=0.20` | Per-eye x correction: left eye adds `-offset`, right eye adds `+offset`. |
| `radius` | number | `0.35` | `0.0..=1.0` | Inner boundary of the weight field. |
| `feather` | number | `0.10` | `0.0..=1.0` | Transition width outside `radius`. |
| `strength` | number | `1.0` | `0.0..=1.0` | Blend of AA result with the original image. |
| `preserve_alpha` | bool | `true` | — | Do not modify pixels with alpha `< 0.999`. |
| `fxaa_quality` | string | `"quality"` | `performance` / `balanced` / `quality` | FXAA search quality. |
| `smaa_threshold` | number | `0.1` | `0.05..=0.2` | SMAA edge-detection threshold. |
| `compare` | bool | `false` | — | Toggle AA on/off every `compare_interval_ms`. |
| `compare_interval_ms` | integer | `2000` | `500..=10000` | A/B interval. |
| `show_region` | bool | `false` | — | Diagnostic contour at `radius`. |
| `auto_scale` | bool | `false` | — | Retained game render-scale control; not part of AA. |
| `min_scale` | number | `0.5` | `0.1..=4.0` | Retained `auto_scale` lower bound. |
| `max_scale` | number | `1.5` | `0.1..=4.0` | Retained `auto_scale` upper bound. |

If `min_scale > max_scale`, swap them on load.

## 4. AA weight modes

`d` is the aspect-corrected radial distance from `center` in pre-distortion
eye-texture UV space. `w` is the AA weight before `strength` is applied.

| Mode | Formula | Result |
|---|---|---|
| `center` | `w = 1 - smoothstep(radius, radius + feather, d)` | Maximum at the centre, zero outside the feather. |
| `inverted` | `w = smoothstep(radius, radius + feather, d)` | Zero inside `radius`, maximum at the periphery. |
| `whole` | `w = 1.0` | Uniform AA over the whole image. |

Special cases:

- `feather == 0`: `center` is `1.0` for `d <= radius`, otherwise `0.0`;
  `inverted` is `0.0` for `d <= radius`, otherwise `1.0`.
- `whole` ignores `center`, `center_offset`, `radius`, and `feather`.
- `strength` still scales the AA result in `whole` mode.
- `show_region` draws the `radius` contour for `center` and `inverted`.
  It draws nothing for `whole`.

## 5. XRizer AA behavior

- `antialias = off` disables the AA pass; `enabled` is the master switch.
- If the selected method is unavailable, fall back in this order:
  `cmaa2 -> fxaa`, `smaa -> fxaa`, `fxaa -> off`.
- `compare` toggles only the AA effect. The submitted image is always the
  same size as the input; there is no upscaling path.
- `compare` does not change `weight_mode` or any other setting.
- `show_region` follows the AA pass. It is not drawn while `compare` holds AA
  off, and it is not drawn for `whole`.
- `preserve_alpha` bypasses AA for `alpha < 0.999` and keeps the original
  pixel, including its alpha.
- The AA pass runs on the final submitted eye image. It does not run on
  overlays or on non-eye textures.

### Deprecated keys

These keys are not read by v1. They are preserved as unknown keys:

- `preset`
- `output_scale`
- `model_path`
- ML model and ML preset keys
- `antialias_stage`

The config app does not display them and does not write new values for them.
The runtime ignores them.

## 6. DXVK VRS keys

All values are optional. The table is the authoritative default/range list.

| Key | Type | Default | Range | Meaning |
|---|---|---|---|---|
| `enabled` | bool | `false` | — | Apply VRS. The extension itself is enabled at startup when supported. |
| `center_left` | [x, y] | `[0.5, 0.5]` | each `0.0..=1.0` | Left-eye centre in its half-image UV. |
| `center_right` | [x, y] | `[0.5, 0.5]` | each `0.0..=1.0` | Right-eye centre in its half-image UV. |
| `center_offset` | number | `0.0` | `-0.20..=0.20` | Per-eye x correction: left adds `-offset`, right adds `+offset`. |
| `inner_radius` | number | `0.35` | `0.0..=1.0` | Inside this radius use `rate_inner`. |
| `outer_radius` | number | `0.70` | `0.0..=1.0` | Outside this radius use `rate_outer`. |
| `feather` | number | `0.10` | `0.0..=1.0` | Transition width between inner and outer rates. |
| `rate_inner` | string | `"1x1"` | see below | Inner shading rate. |
| `rate_mid` | string | `"2x2"` | see below | Middle shading rate. |
| `rate_outer` | string | `"4x4"` | see below | Outer shading rate. |
| `stereo_only` | bool | `true` | — | Apply only to draws detected as a stereo eye pass (2-layer array, or a 1-layer side-by-side pass with aspect >= 1.8). |
| `scene_format` | string | `"rgba16f"` | `rgba16f` / `any` | Restrict VRS to the scene pass format. `rgba16f` is VRChat's Vulkan format 97 and avoids applying VRS to multiple same-sized passes. |
| `flip_y` | bool | `true` | — | Invert the scene-pass Y coordinate when mapping the centre. VRChat's tested scene pass requires this. |
| `compare` | bool | `false` | — | Toggle VRS application every `compare_interval_ms`. |
| `compare_interval_ms` | integer | `2000` | `500..=10000` | A/B interval. |
| `debug_show_map` | bool | `false` | — | Colour-code the rate regions. |

Allowed rates: `1x1`, `1x2`, `2x1`, `2x2`, `2x4`, `4x2`, `4x4`.

If `outer_radius < inner_radius`, swap them on load. If the GPU does not
support a requested rate, round to the supported rate with the closest area;
on a tie choose the lower rate. `1x1` is always supported by a conformant
implementation.

### 6.1 Platform constraints

- RADV (Mesa main `radv_physical_device.c`) reports only `1x1`, `1x2`,
  `2x1`, and `2x2` from `vkGetPhysicalDeviceFragmentShadingRatesKHR`.
  `4x4`, `2x4`, and `4x2` do not exist on RADV; this includes the RX 7900 XTX
  (GFX11). Therefore `rate_outer = "4x4"` is a valid configuration value but
  its effective rate on Radeon is `2x2`.
- The runtime must query `vkGetPhysicalDeviceFragmentShadingRatesKHR` at
  startup or when the sample count changes, and clamp every requested rate to
  the returned list. It must not assume the enum value is usable.
- With MSAA, coarse-rate support is narrower: RADV reports `sampleCounts`
  only for 1, 2, and 4 samples. A higher MSAA sample count can reduce or
  remove the maximum coarse rate. The runtime clamps to a supported
  `(rate, sampleCount)` pair and logs the effective rate.
- The settings guide must recommend MSAA off (or a low sample count) when
  maximum peripheral coarseness is the goal. This is a performance/quality
  trade-off, not a requirement for correctness.

### 6.2 Pass selection

`stereo_only` is necessary but not sufficient for VRChat. The runtime must
also require a color pass (not depth-only) and apply the `scene_format`
filter. The reference implementation uses `VK_FORMAT_RGBA16F` (Vulkan format
97) and a Y flip for the tested VRChat scene pass. `scene_format = "any"`
disables the format filter for other applications but may reproduce the
multiple-foveation-spot problem when several same-sized passes match.

## 7. DXVK VRS behavior

- `enabled` toggles application of VRS. It does not create or destroy the
  device or the extension.
- The map is regenerated when a map-affecting parameter changes:
  centres, offset, radii, feather, or rates.
- `compare` toggles application of the existing map; it does not regenerate
  the map.
- `stereo_only` applies only to draws identified as stereo: a 2-layer array
  pass, or a 1-layer side-by-side pass with `width * 10 >= height * 18`.
  Non-stereo draws use normal shading.
- `scene_format = "rgba16f"` restricts VRS to the RGBA16F scene pass. VRChat
  has several same-sized passes; without this filter the foveation region can
  appear more than once.
- `flip_y` maps the centre into the scene pass's vertical orientation. The
  tested VRChat scene pass requires the inversion.
- `debug_show_map` is diagnostic only and does not change the submitted image.
- If the extension, GPU, driver, or render pass cannot support VRS, use normal
  shading and keep the frame valid.
- Resolution changes regenerate the map at the next frame boundary.

## 8. Cross-repo fixtures

The following files live under `tests/fixtures/settings-v1/` in each
repository:

- `aa-default.json`
- `aa-center.json`
- `aa-inverted.json`
- `aa-whole.json`
- `vrs-default.json`
- `vrs-rates.json`

`aa-default.json` and `vrs-default.json` pin every key and default.
`aa-center.json`, `aa-inverted.json`, and `aa-whole.json` pin the three
weight-mode values. `vrs-rates.json` pins the seven allowed rate strings, the
RADV GFX11 effective set, and the MSAA sample-count note.

## 9. Reference implementation

The A workstream uses `https://github.com/poprox24/vrchat-dfr-linux` as its
primary reference. It is a Linux DXVK patch set for WiVRn gaze-driven VRS:

- Custom scripts, configuration, and documentation: MIT.
- DXVK patch: Zlib/libpng, same as DXVK. Binaries built from it are modified
  DXVK, not the original.
- Pinned DXVK base: `d30be2ba` (2026-10-01). Upstream master was `cf0a0d9`
  (2026-11-02), 133 commits ahead at the time of this review.
- The reference uses `DXVK_GAZE_FMT=97` (RGBA16F), `DXVK_GAZE_FLIP_Y=1`,
  `DXVK_GAZE_FAKE=fixed` for a fixed centre, and a dynamic-rendering
  `VkRenderingFragmentShadingRateAttachmentInfoKHR` attachment.
- Our A0/A1 starts from the pinned revision to reuse the working patch, then
  evaluates a forward-port to current master as a separate change. The
  reference's shared-memory gaze input is replaced by the `vrs.json` centres;
  the map generation, attachment, rate query, and diagnostic paths are
  reusable.
- Any copied or adapted code must retain the MIT notice for the custom code
  and the DXVK Zlib notice for the DXVK-derived code.
