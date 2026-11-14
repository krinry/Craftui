use crate::manifest::{AppManifest, xml_escape};
use craftui_core::{CraftError, Result};
use std::path::Path;

#[allow(dead_code)]
pub fn emit_ios_template(out_dir: &Path, app_name: &str, rust_lib_name: &str) -> Result<()> {
    emit_ios_template_with_manifest(out_dir, app_name, rust_lib_name, None)
}

pub fn emit_ios_template_with_manifest(
    out_dir: &Path,
    app_name: &str,
    rust_lib_name: &str,
    manifest_opt: Option<&AppManifest>,
) -> Result<()> {
    std::fs::create_dir_all(out_dir)
        .map_err(|e| CraftError::Platform(format!("failed creating ios dirs: {e}")))?;

    let effective_manifest = manifest_opt
        .cloned()
        .unwrap_or_else(|| AppManifest::with_app(app_name, AppManifest::sanitize_id(app_name)));

    let header = r#"#ifndef CRAFTUI_IOS_H
#define CRAFTUI_IOS_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct IosAppHost IosAppHost;

typedef struct {
    void *ca_metal_layer;
    float width_pt;
    float height_pt;
    float contents_scale;
} IosMetalLayerDescriptor;

typedef void (*IosBuildUiCallback)(IosAppHost *host, void *user_data);

IosAppHost *craftui_ios_app_create(float width_pt, float height_pt, float scale_factor);
IosAppHost *craftui_ios_app_launch(IosMetalLayerDescriptor layer, IosBuildUiCallback build, void *user_data);
int32_t craftui_ios_app_attach_metal(IosAppHost *app, IosMetalLayerDescriptor layer);
const char *craftui_ios_app_last_error(const IosAppHost *app);
void craftui_ios_app_clear_error(IosAppHost *app);
void craftui_ios_app_destroy(IosAppHost *app);
void craftui_ios_app_resize(IosAppHost *app, float width_pt, float height_pt, float scale_factor);
void craftui_ios_app_set_safe_area(IosAppHost *app, float top, float right, float bottom, float left);
void craftui_ios_app_set_keyboard(IosAppHost *app, float overlap_height_pt, bool visible);
bool craftui_ios_app_touch(IosAppHost *app, uint64_t touch_id, uint8_t phase, float x_pt, float y_pt, uint64_t timestamp_ms);
bool craftui_ios_app_insert_text(IosAppHost *app, const uint8_t *utf8_ptr, size_t utf8_len);
bool craftui_ios_app_set_marked_text(IosAppHost *app, const uint8_t *utf8_ptr, size_t utf8_len, size_t cursor_end);
bool craftui_ios_app_delete_backward(IosAppHost *app);
uint8_t craftui_ios_app_tick(IosAppHost *app, float dt_sec);

uint8_t craftui_ios_app_permission_status_from_key(const IosAppHost *app, const uint8_t *key_ptr, size_t key_len);
void craftui_ios_app_set_permission_status(IosAppHost *app, const uint8_t *key_ptr, size_t key_len, uint8_t status_code);
void craftui_ios_app_deliver_permission_result(IosAppHost *app, uint64_t request_id, const uint8_t *key_ptr, size_t key_len, uint8_t status_code, bool rationale);
int32_t craftui_ios_app_poll_command_json(IosAppHost *app, uint8_t *out_buf, size_t out_cap);
void craftui_ios_app_push_lifecycle(IosAppHost *app, uint8_t event_code);
void craftui_ios_app_set_network_status(IosAppHost *app, uint8_t network_code);
void craftui_ios_app_set_battery(IosAppHost *app, int16_t level_pct, bool charging);

#ifdef __cplusplus
}
#endif

#endif
"#;

    let permissions_swift = r#"import AVFoundation
import Contacts
import CoreBluetooth
import CoreLocation
import CoreMotion
import EventKit
import Foundation
import LocalAuthentication
import Photos
import Security
import UserNotifications

enum CraftUIPermissionCode: UInt8 {
    case notDetermined = 0
    case granted = 1
    case denied = 2
    case permanentlyDenied = 3
    case restricted = 4
    case limited = 5
    case provisional = 6
    case unsupported = 7
}

final class CraftUIPermissionBroker: NSObject, CLLocationManagerDelegate, CBCentralManagerDelegate {
    private var locationManager: CLLocationManager?
    private var pendingLocationCompletion: ((CraftUIPermissionCode) -> Void)?
    private var bluetoothManager: CBCentralManager?
    private var pendingBluetoothCompletion: ((CraftUIPermissionCode) -> Void)?

    func pushStatus(_ code: CraftUIPermissionCode, forKey key: String, app: OpaquePointer) {
        var bytes = Array(key.utf8)
        bytes.withUnsafeMutableBufferPointer { buf in
            craftui_ios_app_set_permission_status(app, buf.baseAddress, buf.count, code.rawValue)
        }
    }

    func deliverResult(requestId: UInt64, key: String, code: CraftUIPermissionCode, rationale: Bool = false, app: OpaquePointer) {
        var bytes = Array(key.utf8)
        bytes.withUnsafeMutableBufferPointer { buf in
            craftui_ios_app_deliver_permission_result(app, requestId, buf.baseAddress, buf.count, code.rawValue, rationale)
        }
    }

    func requestPermission(requestId: UInt64, key: String, app: OpaquePointer) {
        switch key {
        case "camera":
            AVCaptureDevice.requestAccess(for: .video) { granted in
                DispatchQueue.main.async {
                    self.deliverResult(requestId: requestId, key: key, code: granted ? .granted : .denied, app: app)
                }
            }
        case "microphone":
            AVCaptureDevice.requestAccess(for: .audio) { granted in
                DispatchQueue.main.async {
                    self.deliverResult(requestId: requestId, key: key, code: granted ? .granted : .denied, app: app)
                }
            }
        case "notifications":
            UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .badge, .sound]) { granted, _ in
                DispatchQueue.main.async {
                    self.deliverResult(requestId: requestId, key: key, code: granted ? .granted : .denied, app: app)
                }
            }
        case "photos", "videos":
            PHPhotoLibrary.requestAuthorization(for: .readWrite) { status in
                let code: CraftUIPermissionCode = switch status {
                case .authorized: .granted
                case .limited: .limited
                case .restricted: .restricted
                case .denied: .permanentlyDenied
                default: .notDetermined
                }
                DispatchQueue.main.async {
                    self.deliverResult(requestId: requestId, key: key, code: code, app: app)
                }
            }
        case "photos_add_only":
            PHPhotoLibrary.requestAuthorization(for: .addOnly) { status in
                let code: CraftUIPermissionCode = status == .authorized ? .granted : .denied
                DispatchQueue.main.async {
                    self.deliverResult(requestId: requestId, key: key, code: code, app: app)
                }
            }
        case "location_when_in_use", "location_coarse":
            let mgr = CLLocationManager()
            mgr.delegate = self
            locationManager = mgr
            pendingLocationCompletion = { code in
                self.deliverResult(requestId: requestId, key: key, code: code, app: app)
            }
            mgr.requestWhenInUseAuthorization()
        case "location_always":
            let mgr = CLLocationManager()
            mgr.delegate = self
            locationManager = mgr
            pendingLocationCompletion = { code in
                self.deliverResult(requestId: requestId, key: key, code: code, app: app)
            }
            mgr.requestAlwaysAuthorization()
        case "contacts":
            CNContactStore().requestAccess(for: .contacts) { granted, _ in
                DispatchQueue.main.async {
                    self.deliverResult(requestId: requestId, key: key, code: granted ? .granted : .denied, app: app)
                }
            }
        case "calendar_read", "calendar_write":
            let store = EKEventStore()
            if #available(iOS 17.0, *) {
                store.requestFullAccessToEvents { granted, _ in
                    DispatchQueue.main.async {
                        self.deliverResult(requestId: requestId, key: key, code: granted ? .granted : .denied, app: app)
                    }
                }
            } else {
                store.requestAccess(to: .event) { granted, _ in
                    DispatchQueue.main.async {
                        self.deliverResult(requestId: requestId, key: key, code: granted ? .granted : .denied, app: app)
                    }
                }
            }
        case "biometrics":
            let ctx = LAContext()
            var err: NSError?
            if ctx.canEvaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, error: &err) {
                ctx.evaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, localizedReason: "Authenticate with biometrics") { granted, _ in
                    DispatchQueue.main.async {
                        self.deliverResult(requestId: requestId, key: key, code: granted ? .granted : .denied, app: app)
                    }
                }
            } else {
                deliverResult(requestId: requestId, key: key, code: .unsupported, app: app)
            }
        case "bluetooth", "bluetooth_scan", "bluetooth_connect", "bluetooth_advertise":
            if #available(iOS 13.1, *) {
                let auth = CBCentralManager.authorization
                if auth == .allowedAlways {
                    deliverResult(requestId: requestId, key: key, code: .granted, app: app)
                    return
                } else if auth == .denied {
                    deliverResult(requestId: requestId, key: key, code: .permanentlyDenied, app: app)
                    return
                } else if auth == .restricted {
                    deliverResult(requestId: requestId, key: key, code: .restricted, app: app)
                    return
                }
            }
            pendingBluetoothCompletion = { code in
                self.deliverResult(requestId: requestId, key: key, code: code, app: app)
            }
            bluetoothManager = CBCentralManager(delegate: self, queue: nil)
        case "motion_activity", "sensors":
            let pedometer = CMPedometer()
            pedometer.queryPedometerData(from: Date().addingTimeInterval(-60), to: Date()) { _, error in
                DispatchQueue.main.async {
                    let code: CraftUIPermissionCode = (error == nil) ? .granted : .denied
                    self.deliverResult(requestId: requestId, key: key, code: code, app: app)
                }
            }
        default:
            deliverResult(requestId: requestId, key: key, code: .unsupported, app: app)
        }
    }

    func locationManagerDidChangeAuthorization(_ manager: CLLocationManager) {
        let code: CraftUIPermissionCode = switch manager.authorizationStatus {
        case .authorizedAlways, .authorizedWhenInUse: .granted
        case .denied: .permanentlyDenied
        case .restricted: .restricted
        default: .notDetermined
        }
        pendingLocationCompletion?(code)
        pendingLocationCompletion = nil
    }

    func centralManagerDidUpdateState(_ central: CBCentralManager) {
        let code: CraftUIPermissionCode
        if #available(iOS 13.1, *) {
            code = switch CBCentralManager.authorization {
            case .allowedAlways: .granted
            case .denied: .permanentlyDenied
            case .restricted: .restricted
            case .notDetermined:
                switch central.state {
                case .poweredOn: .granted
                case .unauthorized: .permanentlyDenied
                case .unsupported: .unsupported
                default: .notDetermined
                }
            @unknown default: .notDetermined
            }
        } else {
            code = switch central.state {
            case .poweredOn: .granted
            case .unauthorized: .permanentlyDenied
            case .unsupported: .unsupported
            default: .notDetermined
            }
        }
        if code != .notDetermined {
            pendingBluetoothCompletion?(code)
            pendingBluetoothCompletion = nil
        }
    }

    func saveKeychainSecret(key: String, value: String) {
        guard let data = value.data(using: .utf8) else { return }
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrAccount as String: key,
            kSecValueData as String: data
        ]
        SecItemDelete(query as CFDictionary)
        SecItemAdd(query as CFDictionary, nil)
    }

    func deleteKeychainSecret(key: String) {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrAccount as String: key
        ]
        SecItemDelete(query as CFDictionary)
    }
}
"#;

    let swift = r#"import Network
import QuartzCore
import UIKit

final class CraftUIMetalView: UIView {
    override class var layerClass: AnyClass { CAMetalLayer.self }
    var metalLayer: CAMetalLayer { layer as! CAMetalLayer }
}

final class CraftUIViewController: UIViewController, UIKeyInput {
    private let metalView = CraftUIMetalView()
    private let permissionBroker = CraftUIPermissionBroker()
    private let pathMonitor = NWPathMonitor()
    private var app: OpaquePointer?
    private var displayLink: CADisplayLink?
    private var lastTimestamp: CFTimeInterval = 0
    private var keyboardActive = false

    var hasText: Bool { true }
    override var canBecomeFirstResponder: Bool { true }

    override func loadView() {
        metalView.isMultipleTouchEnabled = true
        metalView.metalLayer.pixelFormat = .bgra8Unorm
        view = metalView
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        let scale = Float(UIScreen.main.nativeScale)
        let layerDesc = IosMetalLayerDescriptor(
            ca_metal_layer: Unmanaged.passUnretained(metalView.metalLayer).toOpaque(),
            width_pt: Float(view.bounds.width),
            height_pt: Float(view.bounds.height),
            contents_scale: scale
        )
        app = craftui_ios_app_launch(layerDesc, nil, nil)
        if let app, let errPtr = craftui_ios_app_last_error(app) {
            NSLog("CraftUI launch error: %@", String(cString: errPtr))
            craftui_ios_app_clear_error(app)
        }

        UIDevice.current.isBatteryMonitoringEnabled = true
        pushBatteryTelemetry()

        pathMonitor.pathUpdateHandler = { [weak self] path in
            DispatchQueue.main.async {
                guard let self, let app = self.app else { return }
                let code: UInt8 = if path.status != .satisfied {
                    1
                } else if path.usesInterfaceType(.wifi) {
                    2
                } else if path.usesInterfaceType(.cellular) {
                    3
                } else if path.usesInterfaceType(.wiredEthernet) {
                    4
                } else {
                    0
                }
                craftui_ios_app_set_network_status(app, code)
            }
        }
        pathMonitor.start(queue: DispatchQueue.global(qos: .utility))

        NotificationCenter.default.addObserver(
            self,
            selector: #selector(handleKeyboard(_:)),
            name: UIResponder.keyboardWillChangeFrameNotification,
            object: nil
        )
        NotificationCenter.default.addObserver(
            self,
            selector: #selector(handleBattery(_:)),
            name: UIDevice.batteryLevelDidChangeNotification,
            object: nil
        )
        NotificationCenter.default.addObserver(
            self,
            selector: #selector(handleDidBecomeActive),
            name: UIApplication.didBecomeActiveNotification,
            object: nil
        )
        NotificationCenter.default.addObserver(
            self,
            selector: #selector(handleDidEnterBackground),
            name: UIApplication.didEnterBackgroundNotification,
            object: nil
        )
        NotificationCenter.default.addObserver(
            self,
            selector: #selector(handleMemoryWarning),
            name: UIApplication.didReceiveMemoryWarningNotification,
            object: nil
        )

        let link = CADisplayLink(target: self, selector: #selector(stepFrame(_:)))
        link.add(to: .main, forMode: .common)
        displayLink = link
    }

    override func viewDidLayoutSubviews() {
        super.viewDidLayoutSubviews()
        guard let app else { return }
        let scale = Float(view.window?.screen.nativeScale ?? UIScreen.main.nativeScale)
        metalView.metalLayer.contentsScale = CGFloat(scale)
        craftui_ios_app_resize(app, Float(view.bounds.width), Float(view.bounds.height), scale)
        let sa = view.safeAreaInsets
        craftui_ios_app_set_safe_area(app, Float(sa.top), Float(sa.right), Float(sa.bottom), Float(sa.left))
    }

    @objc private func handleKeyboard(_ note: Notification) {
        guard let app,
              let endFrame = note.userInfo?[UIResponder.keyboardFrameEndUserInfoKey] as? CGRect else { return }
        let overlap = max(0, view.bounds.maxY - view.convert(endFrame, from: nil).minY)
        craftui_ios_app_set_keyboard(app, Float(overlap), overlap > 0)
    }

    @objc private func handleBattery(_ note: Notification) { pushBatteryTelemetry() }
    @objc private func handleDidBecomeActive() { if let app { craftui_ios_app_push_lifecycle(app, 0) } }
    @objc private func handleDidEnterBackground() { if let app { craftui_ios_app_push_lifecycle(app, 1) } }
    @objc private func handleMemoryWarning() { if let app { craftui_ios_app_push_lifecycle(app, 4) } }

    private func pushBatteryTelemetry() {
        guard let app else { return }
        let level = UIDevice.current.batteryLevel
        let pct: Int16 = level < 0 ? -1 : Int16((level * 100.0).rounded())
        let charging = UIDevice.current.batteryState == .charging || UIDevice.current.batteryState == .full
        craftui_ios_app_set_battery(app, pct, charging)
    }

    @objc private func stepFrame(_ link: CADisplayLink) {
        guard let app else { return }
        let dt = lastTimestamp == 0 ? 0.016 : Float(min(0.05, max(0.001, link.timestamp - lastTimestamp)))
        lastTimestamp = link.timestamp
        let flags = craftui_ios_app_tick(app, dt)
        let wantsKeyboard = (flags & 0b010) != 0
        if wantsKeyboard && !keyboardActive {
            keyboardActive = true
            becomeFirstResponder()
        } else if !wantsKeyboard && keyboardActive {
            keyboardActive = false
            resignFirstResponder()
        }
        if (flags & 0b100) != 0, let errPtr = craftui_ios_app_last_error(app) {
            NSLog("CraftUI runtime error: %@", String(cString: errPtr))
            craftui_ios_app_clear_error(app)
        }
        drainPlatformCommands(app)
    }

    private func drainPlatformCommands(_ app: OpaquePointer) {
        var buffer = [UInt8](repeating: 0, count: 4096)
        while true {
            let n = buffer.withUnsafeMutableBufferPointer { ptr in
                craftui_ios_app_poll_command_json(app, ptr.baseAddress, ptr.count)
            }
            if n == 0 {
                break
            }
            if n < 0 {
                buffer = [UInt8](repeating: 0, count: Int(-n) + 1)
                continue
            }
            let data = Data(buffer.prefix(Int(n)))
            guard let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                  let type = json["type"] as? String else {
                continue
            }
            handleCommand(type: type, json: json, app: app)
        }
    }

    private func handleCommand(type: String, json: [String: Any], app: OpaquePointer) {
        switch type {
        case "request_permission":
            if let key = json["key"] as? String {
                let reqId = (json["request_id"] as? NSNumber)?.uint64Value ?? 0
                permissionBroker.requestPermission(requestId: reqId, key: key, app: app)
            }
        case "open_app_settings":
            if let url = URL(string: UIApplication.openSettingsURLString) {
                UIApplication.shared.open(url)
            }
        case "haptic":
            switch json["style"] as? String {
            case "selection":
                UISelectionFeedbackGenerator().selectionChanged()
            case "success":
                UINotificationFeedbackGenerator().notificationOccurred(.success)
            case "warning":
                UINotificationFeedbackGenerator().notificationOccurred(.warning)
            case "error":
                UINotificationFeedbackGenerator().notificationOccurred(.error)
            case "heavy":
                UIImpactFeedbackGenerator(style: .heavy).impactOccurred()
            case "medium":
                UIImpactFeedbackGenerator(style: .medium).impactOccurred()
            default:
                UIImpactFeedbackGenerator(style: .light).impactOccurred()
            }
        case "share":
            var items: [Any] = []
            if let txt = json["text"] as? String { items.append(txt) }
            if let urlStr = json["url"] as? String, let url = URL(string: urlStr) { items.append(url) }
            if !items.isEmpty {
                let sheet = UIActivityViewController(activityItems: items, applicationActivities: nil)
                present(sheet, animated: true)
            }
        case "sync_secret":
            if let k = json["key"] as? String, let v = json["value"] as? String {
                permissionBroker.saveKeychainSecret(key: k, value: v)
            }
        case "delete_secret":
            if let k = json["key"] as? String {
                permissionBroker.deleteKeychainSecret(key: k)
            }
        default:
            break
        }
    }

    private func sendTouches(_ touches: Set<UITouch>, phase: UInt8) {
        guard let app else { return }
        for t in touches {
            let pt = t.location(in: view)
            let tid = UInt64(bitPattern: Int64(ObjectIdentifier(t).hashValue))
            _ = craftui_ios_app_touch(app, tid, phase, Float(pt.x), Float(pt.y), UInt64(t.timestamp * 1000.0))
        }
    }

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) { sendTouches(touches, phase: 0) }
    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) { sendTouches(touches, phase: 1) }
    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) { sendTouches(touches, phase: 2) }
    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) { sendTouches(touches, phase: 3) }

    func insertText(_ text: String) {
        guard let app else { return }
        var bytes = Array(text.utf8)
        bytes.withUnsafeMutableBufferPointer { buf in
            _ = craftui_ios_app_insert_text(app, buf.baseAddress, buf.count)
        }
    }

    func deleteBackward() {
        guard let app else { return }
        _ = craftui_ios_app_delete_backward(app)
    }

    deinit {
        pathMonitor.cancel()
        displayLink?.invalidate()
        NotificationCenter.default.removeObserver(self)
        if let app { craftui_ios_app_destroy(app) }
    }
}
"#;

    let mut extra_plist = String::new();
    for (key, desc) in effective_manifest.ios_info_plist_usage_entries() {
        use std::fmt::Write as _;
        let _ = writeln!(
            extra_plist,
            "  <key>{}</key>\n  <string>{}</string>",
            xml_escape(&key),
            xml_escape(&desc)
        );
    }
    let bg_modes = effective_manifest.ios_background_modes();
    if !bg_modes.is_empty() {
        use std::fmt::Write as _;
        let _ = writeln!(extra_plist, "  <key>UIBackgroundModes</key>\n  <array>");
        for m in bg_modes {
            let _ = writeln!(extra_plist, "    <string>{}</string>", xml_escape(&m));
        }
        let _ = writeln!(extra_plist, "  </array>");
    }

    let info_plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>
  <string>{}</string>
  <key>CFBundleIdentifier</key>
  <string>{}</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>{}</string>
  <key>MinimumOSVersion</key>
  <string>{}</string>
  <key>UIRequiredDeviceCapabilities</key>
  <array><string>arm64</string><string>metal</string></array>
{}</dict>
</plist>
"#,
        xml_escape(&effective_manifest.app.name),
        xml_escape(&effective_manifest.app.id),
        xml_escape(&effective_manifest.app.version),
        xml_escape(&effective_manifest.ios.deployment_target),
        extra_plist
    );

    let readme = format!(
        "# {} (iOS UIKit + Metal + Native Permissions Bridge)\n\n1. On macOS with Xcode installed, build the static library:\n   ```sh\n   cargo build --target aarch64-apple-ios --release -p {rust_lib_name}\n   ```\n2. Add `CraftUI.h` (Bridging Header), `CraftUIPermissions.swift`, `CraftUIViewController.swift`, `Info.plist`, and link `lib{rust_lib_name}.a` + `Metal.framework`, `QuartzCore.framework`, `UIKit.framework`, `Network.framework`.\n",
        effective_manifest.app.name
    );

    std::fs::write(out_dir.join("CraftUI.h"), header)
        .and_then(|_| std::fs::write(out_dir.join("CraftUIPermissions.swift"), permissions_swift))
        .and_then(|_| std::fs::write(out_dir.join("CraftUIViewController.swift"), swift))
        .and_then(|_| std::fs::write(out_dir.join("Info.plist"), info_plist))
        .and_then(|_| std::fs::write(out_dir.join("README.md"), readme))
        .map_err(|e| CraftError::Platform(format!("failed writing ios template: {e}")))?;
    Ok(())
}
