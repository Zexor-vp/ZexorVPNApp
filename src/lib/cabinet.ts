// Типизированные обёртки над Cabinet API — тем же бэкендом, что обслуживает веб-кабинет.
// Поля описаны не все, а только те, что показывает приложение.
import { cabinetRequest } from './commands';

const API = '/api/cabinet';

export interface SubscriptionData {
  id: number;
  status: string;
  is_trial: boolean;
  end_date: string;
  days_left: number;
  hours_left: number;
  time_left_display: string;
  traffic_limit_gb: number;
  traffic_used_gb: number;
  traffic_used_percent: number;
  device_limit: number;
  autopay_enabled: boolean;
  autopay_days_before: number;
  is_active: boolean;
  is_expired: boolean;
  is_limited: boolean;
  is_daily: boolean;
  tariff_name: string | null;
  is_grace_period: boolean;
}

export interface SubscriptionStatus {
  has_subscription: boolean;
  subscription: SubscriptionData | null;
}

export const getSubscription = (): Promise<SubscriptionStatus> =>
  cabinetRequest('GET', `${API}/subscription/info`);

export interface ProtocolInfo {
  active_protocol: 'vless' | 'wireguard' | string;
  protocol_switch_available: boolean;
}

export const getProtocol = (): Promise<ProtocolInfo> =>
  cabinetRequest('GET', `${API}/subscription/protocol`);

export const setProtocol = (protocol: 'vless' | 'wireguard'): Promise<ProtocolInfo> =>
  cabinetRequest('POST', `${API}/subscription/protocol`, { protocol });

export interface RenewalOption {
  period_days: number;
  price_kopeks: number;
  price_label: string | null;
  discount_percent: number;
}

export const getRenewalOptions = (): Promise<RenewalOption[]> =>
  cabinetRequest('GET', `${API}/subscription/renewal-options`);

export const renew = (periodDays: number): Promise<{ message?: string; new_end_date?: string }> =>
  cabinetRequest('POST', `${API}/subscription/renew`, { period_days: periodDays });

export interface DeviceItem {
  hwid: string;
  platform: string | null;
  device_model: string | null;
  local_name: string | null;
}

export interface DevicesResponse {
  devices: DeviceItem[];
  total: number;
  device_limit: number;
}

export const getDevices = (): Promise<DevicesResponse> =>
  cabinetRequest('GET', `${API}/subscription/devices`);

export const deleteDevice = (hwid: string): Promise<unknown> =>
  cabinetRequest('DELETE', `${API}/subscription/devices/${encodeURIComponent(hwid)}`);

export interface DevicePrice {
  available: boolean;
  reason?: string;
  total_price_kopeks?: number;
  total_price_label?: string;
  price_per_device_label?: string;
  can_add?: number;
  current_device_limit?: number;
  max_device_limit?: number | null;
}

export const getDevicePrice = (devices: number): Promise<DevicePrice> =>
  cabinetRequest('GET', `${API}/subscription/devices/price?devices=${devices}`);

export const purchaseDevices = (devices: number): Promise<unknown> =>
  cabinetRequest('POST', `${API}/subscription/devices/purchase`, { devices });

export interface TrafficPackage {
  gb: number;
  price_kopeks: number;
  is_unlimited: boolean;
  discount_percent: number;
}

export const getTrafficPackages = (): Promise<TrafficPackage[]> =>
  cabinetRequest('GET', `${API}/subscription/traffic-packages`);

export const purchaseTraffic = (gb: number): Promise<unknown> =>
  cabinetRequest('POST', `${API}/subscription/traffic`, { gb });

export const setAutopay = (enabled: boolean): Promise<unknown> =>
  cabinetRequest('PATCH', `${API}/subscription/autopay`, { enabled });

export interface Me {
  id: number;
  username: string | null;
  first_name: string | null;
  email: string | null;
  email_verified: boolean;
  balance_kopeks: number;
  balance_usd_cents: number;
  balance_eur_cents: number;
  currency: 'RUB' | 'EUR' | 'USD' | string;
  referral_code: string | null;
  auth_type: string;
}

export const getMe = (): Promise<Me> => cabinetRequest('GET', `${API}/auth/me`);

export const setCurrency = (currency: string): Promise<unknown> =>
  cabinetRequest('PATCH', `${API}/info/user/currency`, { currency });

export const requestEmailChange = (newEmail: string): Promise<{ message: string }> =>
  cabinetRequest('POST', `${API}/auth/email/change`, { new_email: newEmail });

export const verifyEmailChange = (code: string): Promise<unknown> =>
  cabinetRequest('POST', `${API}/auth/email/change/verify`, { code });

export interface ReferralInfo {
  referral_code: string;
  total_referrals: number;
  active_referrals: number;
  total_earnings_kopeks: number;
  commission_percent: number;
  referral_balance_kopeks: number;
}

export const getReferral = (): Promise<ReferralInfo> => cabinetRequest('GET', `${API}/referral`);

/** Сумма в валюте пользователя: пока бэкенд отдаёт копейки/центы, показываем с символом. */
export function formatMoney(minorUnits: number, currency: string): string {
  const symbol = currency === 'EUR' ? '€' : currency === 'USD' ? '$' : '₽';
  const value = minorUnits / 100;
  const text = Number.isInteger(value) ? String(value) : value.toFixed(2);
  return currency === 'RUB' ? `${text} ${symbol}` : `${symbol}${text}`;
}

export const balanceFor = (me: Me): number =>
  me.currency === 'EUR'
    ? me.balance_eur_cents
    : me.currency === 'USD'
      ? me.balance_usd_cents
      : me.balance_kopeks;

export const CABINET_URL = 'https://cabinet.zexor.site';
export const SUPPORT_URL = 'https://t.me/ZexorVPNsupport';

export interface FaqPage {
  id: number;
  title: string;
  content: string;
  order: number;
}

export const getFaq = (language = 'ru'): Promise<FaqPage[]> =>
  cabinetRequest('GET', `${API}/info/faq?language=${encodeURIComponent(language)}`);

/** HTML из редактора кабинета → простой текст (абзацы и списки сохраняют переносы). */
export function htmlToText(html: string): string {
  const withBreaks = html.replace(/<\s*(br|\/p|\/li|\/div|\/h[1-6])\s*\/?>/gi, '\n');
  const doc = new DOMParser().parseFromString(withBreaks, 'text/html');
  return (doc.body.textContent ?? '').replace(/\n{3,}/g, '\n\n').trim();
}

export type TicketStatus = 'open' | 'pending' | 'answered' | 'closed' | string;

export interface TicketMessage {
  id: number;
  message_text: string;
  is_from_admin: boolean;
  has_media: boolean;
  created_at: string;
}

export interface TicketSummary {
  id: number;
  title: string;
  status: TicketStatus;
  updated_at: string;
  messages_count: number;
  last_message: TicketMessage | null;
}

export interface TicketDetail {
  id: number;
  title: string;
  status: TicketStatus;
  created_at: string;
  closed_at: string | null;
  is_reply_blocked: boolean;
  messages: TicketMessage[];
}

export const getTickets = (): Promise<{ items: TicketSummary[]; total: number }> =>
  cabinetRequest('GET', `${API}/tickets?page=1&per_page=30`);

export const getTicket = (id: number): Promise<TicketDetail> => cabinetRequest('GET', `${API}/tickets/${id}`);

export const createTicket = (title: string, message: string): Promise<TicketDetail> =>
  cabinetRequest('POST', `${API}/tickets`, { title, message });

export const replyTicket = (id: number, message: string): Promise<TicketMessage> =>
  cabinetRequest('POST', `${API}/tickets/${id}/messages`, { message });

export const TICKET_STATUS_LABEL: Record<string, string> = {
  open: 'Открыто',
  pending: 'В работе',
  answered: 'Есть ответ',
  closed: 'Закрыто',
};

export interface TariffPeriod {
  days: number;
  months: number;
  label: string;
  price_kopeks: number;
  price_label: string;
  price_per_month_label?: string;
  discount_percent?: number;
  original_price_label?: string;
}

export interface TariffOption {
  id: number;
  name: string;
  description: string | null;
  traffic_limit_label: string;
  device_limit: number;
  servers_count: number;
  periods: TariffPeriod[];
  is_current: boolean;
  is_available: boolean;
  is_daily: boolean;
}

export interface PurchaseOptions {
  sales_mode: string;
  tariffs?: TariffOption[];
  current_tariff_id?: number | null;
  currency: string;
  balance_kopeks: number;
  balance_usd_cents: number;
  balance_eur_cents: number;
  balance_label: string;
  subscription_is_expired: boolean;
  has_subscription: boolean;
}

export const getPurchaseOptions = (): Promise<PurchaseOptions> =>
  cabinetRequest('GET', `${API}/subscription/purchase-options`);

export const purchaseTariff = (tariffId: number, periodDays: number): Promise<unknown> =>
  cabinetRequest('POST', `${API}/subscription/purchase-tariff`, { tariff_id: tariffId, period_days: periodDays });

export interface SwitchPreview {
  can_switch: boolean;
  new_tariff_name: string;
  remaining_days: number;
  upgrade_cost_kopeks: number;
  upgrade_cost_label: string;
  has_enough_balance: boolean;
  missing_amount_kopeks: number;
  missing_amount_label: string;
  is_upgrade: boolean;
}

export const previewTariffSwitch = (tariffId: number, periodDays: number): Promise<SwitchPreview> =>
  cabinetRequest('POST', `${API}/subscription/tariff/switch/preview`, { tariff_id: tariffId, period_days: periodDays });

export const switchTariff = (tariffId: number, periodDays: number): Promise<unknown> =>
  cabinetRequest('POST', `${API}/subscription/tariff/switch`, { tariff_id: tariffId, period_days: periodDays });

export interface PaymentMethodOption {
  id?: string;
  name?: string;
  [key: string]: unknown;
}

export interface PaymentMethod {
  id: string;
  name: string;
  description: string | null;
  min_amount_kopeks: number;
  max_amount_kopeks: number;
  is_available: boolean;
  options: PaymentMethodOption[] | null;
  quick_amounts: number[];
}

export const getPaymentMethods = (): Promise<PaymentMethod[]> => cabinetRequest('GET', `${API}/balance/payment-methods`);

export interface TopUpResult {
  payment_id: string;
  payment_url: string;
  amount_kopeks: number;
  status: string;
}

export const createTopUp = (amountMinor: number, method: string, option?: string): Promise<TopUpResult> =>
  cabinetRequest('POST', `${API}/balance/topup`, {
    amount_kopeks: amountMinor,
    payment_method: method,
    ...(option ? { payment_option: option } : {}),
  });

/** Баланс в валюте пользователя (копейки / центы). */
export function balanceFromOptions(options: Pick<PurchaseOptions, 'currency' | 'balance_kopeks' | 'balance_usd_cents' | 'balance_eur_cents'>): number {
  return options.currency === 'EUR' ? options.balance_eur_cents : options.currency === 'USD' ? options.balance_usd_cents : options.balance_kopeks;
}
