/** Приложение запущено на телефоне (Android). На телефоне нет трея, автообновления самим приложением,
 * режимов Proxy/TUN и маршрутизации по программам — эти части интерфейса скрываются. */
export const isMobile = typeof navigator !== 'undefined' && /Android/i.test(navigator.userAgent);
