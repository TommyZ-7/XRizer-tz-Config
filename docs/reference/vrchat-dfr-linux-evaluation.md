# vrchat-dfr-linux evaluation for A (dxvk-tz-custom)

- Source: `https://github.com/poprox24/vrchat-dfr-linux`
- Reviewed commit: `62c40ef` (2026-11-03)
- License: custom scripts/config/docs MIT; DXVK patch Zlib/libpng; WiVRn
  patch GPL-3.0-or-later (not used by A).
- Purpose in this project: primary reference for the VRS attachment, pass
  selection, rate query, and fixed-centre operation.

## What it does

1. WiVRn writes per-eye gaze centres to `/dev/shm/wivrn_gaze`.
2. Patched DXVK reads the shared memory through Wine (`Z:\dev\shm\wivrn_gaze`).
3. It builds an `R8_UINT` fragment-shading-rate attachment, uploads tile rates,
   and attaches it to a dynamic-rendering pass with
   `VkRenderingFragmentShadingRateAttachmentInfoKHR`.
4. The pass is selected by:
   - color target present (depth-only passes rejected),
   - 2-layer array stereo, or 1-layer side-by-side with
     `width * 10 >= height * 18`,
   - optional sample count filter,
   - optional format filter; the tested VRChat value is `fmt=97` (RGBA16F).
5. `DXVK_GAZE_FLIP_Y=1` is required for the tested VRChat scene pass.
6. `DXVK_GAZE_FAKE=fixed` supplies a fixed synthetic centre without gaze input.
7. The coarse rate is queried with `vkGetPhysicalDeviceFragmentShadingRatesKHR`
   and filtered by `sampleCounts`; `auto` picks the highest available rate for
   the current sample count.

## Findings that change our plan

### 1. `stereo_only` is not enough for VRChat

VRChat renders several passes of similar size. Applying VRS to every stereo
pass produces multiple foveation spots. The reference limits the attachment to
the RGBA16F scene pass with a Y flip. Therefore A's pass selection needs:

- a stereo test (2-layer array, or 1-layer SBS with aspect >= 1.8),
- a color/depth test,
- a scene-format filter (`scene_format = "rgba16f"` by default),
- a Y-flip policy (`flip_y = true` by default for the tested VRChat path).

The contract now carries `scene_format` and `flip_y`. `stereo_only` remains the
stereo test; it is documented as necessary but not sufficient.

### 2. Fixed centre can start from the reference's fixed mode

`DXVK_GAZE_FAKE=fixed` bypasses the shared-memory read and uses a constant
centre. We can replace that branch with `vrs.json`'s `center_left` /
`center_right`, remove the WiVRn shared-memory dependency, and keep the
attachment/upload/barrier code. This is the fastest path to A1 (single-eye)
and A2 (stereo) without inventing a new VRS backend.

### 3. RADV rate limits and MSAA

RADV (Mesa main `radv_physical_device.c`) returns only `1x1`, `1x2`, `2x1`,
and `2x2` from `vkGetPhysicalDeviceFragmentShadingRatesKHR`; `4x4`, `2x4`,
and `4x2` do not exist. On the RX 7900 XTX (GFX11), the maximum effective
outer rate is `2x2`. The enum may keep the NVIDIA-only values, but the runtime
must clamp every request to the queried list and log the effective rate.

With MSAA, RADV reports `sampleCounts` only for 1, 2, and 4 samples. Higher
MSAA can restrict the coarse rate. The runtime must query per sample count and
clamp; the settings guide must recommend MSAA off or a low sample count for
maximum peripheral coarseness.

### 4. DXVK base revision

- Reference pin: `d30be2ba` (2026-10-01).
- Upstream master at review: `cf0a0d9` (2026-11-02), 133 commits ahead.
- The reference README explicitly says the patch was generated against a
  working tree and the DXVK ref must match exactly; the patch records parent
  blob hashes.
- Recommendation: A0 and A1 pin `d30be2ba` to reuse the working patch and
  reduce unknowns. A forward-port to current master is a separate change
  after A1 is validated. Do not mix the port with the VRS bring-up.

### 5. Licensing and provenance

- Keep the reference repository's MIT notice for adapted custom code.
- Keep DXVK's Zlib notice for DXVK-derived code.
- Record the reference commit and DXVK pin in `NOTICE`/README.
- Do not copy the WiVRn GPL patch into this project; we are not using it.

## Impact by phase

### A0

- Build from `d30be2ba`, not current master, for the first working artifact.
- Reuse the reference Meson options:
  `-Dbuildtype=release -Denable_d3d8=false -Denable_d3d9=false -Denable_d3d10=false`.
- Add the reference build/license notes to the A0 README.
- Verify `d3d11.dll` and `dxgi.dll` are PE32+ x86-64.

### A1

- Adapt `DxvkGazeVrs` as a fixed-centre `DxvkTzVrs`:
  - remove shared-memory gaze read,
  - read `vrs.json` centres,
  - keep the `R8_UINT` image, tile-rate upload, barriers, and
    `VkRenderingFragmentShadingRateAttachmentInfoKHR` attachment,
  - start with a single color pass and the RGBA16F filter.
- Add validation-layer checks and a debug map.

### A2

- Implement array/SBS stereo detection and per-eye centres.
- Add `scene_format` / `flip_y` handling.
- Query and clamp rates for the current sample count; log effective rates.
- Test resolution changes, fallback, compare, and debug visualization.
- Verify on RADV/GFX11 that `rate_outer=4x4` reports effective `2x2`.

### A3

- Replace startup/environment-only configuration with the `vrs.json` watcher.
- Keep the fixed-centre path as the runtime data source.
- The reference has no dynamic config; A3 remains our own work.

## Open decisions for the parent

- Confirm `scene_format` default `"rgba16f"` (recommended for VRChat).
- Confirm `flip_y` default `true` (recommended for VRChat; `false` for generic
  apps that do not need the inversion).
- Confirm pinning `d30be2ba` for A0/A1 and treating the master forward-port as
  a separate task.
