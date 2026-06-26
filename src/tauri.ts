import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { check } from '@tauri-apps/plugin-updater';

// === Types ===
export interface BrokerInfo {
  id: string;
  type: string;
  name: string;
  email: string;
  status: string;
}

export interface LogEventPayload {
  message: string;
  brokerType?: string;
  brokerId?: string;
  broker_type?: string;
  broker_id?: string;
}

// === COMMANDS (frontend -> backend) ===

// Auth
export const start2fa = (email: string, brokerType?: string) =>
  invoke('start_2fa', { email, brokerType });

export const loginCmd = (code: string, email: string) =>
  invoke('login', { code, email });

// Broker Management
export const frontendReady = () =>
  invoke('frontend_ready');

export const linkBroker = (brokerType: string) =>
  invoke('link_broker', { brokerType });

export const selectBroker = (brokerId: string) =>
  invoke('select_broker', { brokerId });

export const unlinkBrokerCmd = (brokerId: string) =>
  invoke('unlink_broker', { brokerId });

export const getLinkedBrokers = () =>
  invoke<BrokerInfo[]>('get_linked_brokers');

// Legacy Credential Migration
export const importLegacyCredentials = () =>
  invoke('import_legacy_credentials');

export const skipLegacyImport = () =>
  invoke('skip_legacy_import');

// Accounts
export const getAllBrokerAccounts = () =>
  invoke<any[]>('get_all_broker_accounts');

export const getBrokerAccountHoldings = (brokerId: string, accountId: string) =>
  invoke<any[]>('get_broker_account_holdings', { brokerId, accountId });

export const getBrokerAccountCash = (brokerId: string, accountId: string) =>
  invoke<any>('get_broker_account_cash', { brokerId, accountId });

export const getAccountHoldings = (accountId: string) =>
  invoke<any[]>('get_account_holdings', { accountId });

export const getAccountCash = (accountId: string) =>
  invoke<any>('get_account_cash', { accountId });

// Trading
export const placeOrderMultiBroker = (
  ticker: string, side: string, shares: number,
  accountsByBroker: Record<string, string>,
  sellMax?: boolean
) => invoke<Record<string, string>>('place_order_multi_broker', {
  ticker, side, shares, accountsByBroker, sellMax: sellMax || false
});

// Session
export const isLoggedInCmd = () => invoke<boolean>('is_logged_in');
export const getCurrentEmail = () => invoke<string>('get_current_email');
export const getLoginTime = () => invoke<string>('get_login_time');
export const logoutCmd = () => invoke('logout');
export const checkTokenValidity = () => invoke<boolean>('check_token_validity');

// App Updates
export const checkForAppUpdate = () => check();

// === EVENTS (backend -> frontend) ===
export const onLog = (cb: (payload: string | LogEventPayload) => void): Promise<UnlistenFn> =>
  listen<string | LogEventPayload>('log', (e) => cb(e.payload));

export const onStartupComplete = (cb: (data: any) => void): Promise<UnlistenFn> =>
  listen('startup_complete', (e) => cb(e.payload));

export const onStartupError = (cb: (msg: string) => void): Promise<UnlistenFn> =>
  listen<string>('startup_error', (e) => cb(e.payload));

export const on2faStarted = (cb: () => void): Promise<UnlistenFn> =>
  listen('twofa_started', () => cb());

export const onLoginSuccess = (cb: () => void): Promise<UnlistenFn> =>
  listen('login_success', () => cb());

export const onLoginFailure = (cb: (msg: string) => void): Promise<UnlistenFn> =>
  listen<string>('login_failure', (e) => cb(e.payload));

export const onLogoutSuccess = (cb: () => void): Promise<UnlistenFn> =>
  listen('logout_success', () => cb());

export const onBrokerLinked = (cb: (info: any) => void): Promise<UnlistenFn> =>
  listen('broker_linked', (e) => cb(e.payload));

export const onBrokerUnlinked = (cb: (id: string) => void): Promise<UnlistenFn> =>
  listen<string>('broker_unlinked', (e) => cb(e.payload));

export const onTokenExpired = (cb: (data: any) => void): Promise<UnlistenFn> =>
  listen('token_expired', (e) => cb(e.payload));

export const onLinkBrokerReady = (cb: (data: any) => void): Promise<UnlistenFn> =>
  listen('link_broker_ready', (e) => cb(e.payload));

export const onBrokerSelected = (cb: (data: any) => void): Promise<UnlistenFn> =>
  listen('broker_selected', (e) => cb(e.payload));

export const onLegacyImportComplete = (cb: (data: any) => void): Promise<UnlistenFn> =>
  listen('legacy_import_complete', (e) => cb(e.payload));
