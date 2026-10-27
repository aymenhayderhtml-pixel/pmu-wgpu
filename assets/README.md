# PMU Assets Directory

Place optional glTF 2.0 models (`.glb` or `.gltf`) in `assets/models/`:

- `assets/models/player.glb`: Optional player model. Automatically scaled at load time so its height is 2.0 world units, with its origin centered at the feet (`y = 0`). If absent or invalid, the engine falls back to the 1x2x1 blue box.
- `assets/models/prop.glb`: Optional environment prop model. If present, up to 50 instances are deterministically scattered across the ground plane. If absent, prop scatter is skipped.
