export async function isPermissionGranted() { return true; }
export async function requestPermission() { return 'granted'; }
export function sendNotification(_: { title: string; body?: string }) {}
