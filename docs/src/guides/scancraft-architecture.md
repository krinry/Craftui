# ScanCraft

`examples/scancraft` is a larger example: a document scanner with a capture
screen, an image editor and a document library. It shows how an app uses
permissions, platform services and a multi-screen `.cui` file together.

It is built in CI as part of the workspace, but it has no end-to-end test. On
desktop the camera is simulated. For an example with tests on every layer, see
the [notes app](notes-app.md).

---

## 1. Feature Highlights

1. **Document Capture & Photo Picker:**
   - Real and simulated camera viewfinder with dynamic framing overlay.
   - Integration with `Permission::Camera`, `Permission::Photos`, and `Permission::Vibrate`.
2. **Real-time 2D Canvas Image Processing:**
   - Crop boundary adjustment with touch/mouse handles.
   - Interactive 90° rotation and flip.
   - Color grading pipeline: Brightness, Contrast, Grayscale conversion, and High-Contrast Document Thresholding (Otsu-style B&W binarization).
3. **Tabular Document Library:**
   - `DataTable` with sortable columns (Name, Date, File Size, Format).
   - Selection state, batch deletion, and previewing.
   - Status indicators with `Badge`.
4. **Native Mobile Shell & Telemetry:**
   - Dynamic `SafeArea` insets for notches and home indicators.
   - Real-time battery level and storage capacity telemetry.
   - Haptic vibration patterns on capture (`HapticFeedbackType::MediumImpact`).
   - Native Share sheet export dialog.

---

## 2. Component Structure

```
examples/scancraft/
├── Cargo.toml
├── craftui.toml               # Manifest with camera/photo permissions
├── build.rs                   # AOT .cui template compilation
├── ui/
│   └── scancraft.cui          # Declarative multi-tab UI
└── src/
    ├── lib.rs                 # ScanCraft logic, AOT UI, and Android entrypoint
    └── main.rs                # Desktop launcher
```

---

## 3. Running ScanCraft

### On Linux Desktop
```bash
cargo run -p craftui-example-scancraft
```

### On Android
```bash
cd examples/scancraft
craftui run --platform android
```
