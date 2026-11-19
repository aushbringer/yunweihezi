import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";

/**
 * Best-effort system notification. Requests permission on first use; silently
 * no-ops when notifications are unavailable (e.g. plain-browser dev mode).
 */
export async function sysNotify(title: string, body: string) {
  try {
    let granted = await isPermissionGranted();
    if (!granted) {
      granted = (await requestPermission()) === "granted";
    }
    if (granted) sendNotification({ title, body });
  } catch {
    /* notification backend unavailable */
  }
}
