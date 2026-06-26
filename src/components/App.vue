<template>
    <div class="app-container" @contextmenu="handleContextMenu">
        <div class="app-content" :class="{ 'landscape-layout': isLandscape }">
            <!-- Loading State -->
            <div v-if="startupState === 'loading' || startupState === 'restoring'" class="login-screen">
                <div class="login-container">
                    <div class="login-header">
                        <h1>{{ startupState === 'restoring' ? 'Restoring Session...' : 'Loading...' }}</h1>
                        <p class="loading-text">Please wait</p>
                    </div>
                </div>
            </div>

            <!-- Legacy Credential Import -->
            <div v-else-if="startupState === 'legacy_found'" class="login-screen">
                <div class="login-container">
                    <div class="login-header">
                        <h1>{{ legacyImporting ? 'Importing...' : 'Import Credentials' }}</h1>
                        <p class="loading-text">{{ legacyImporting ? 'Restoring your broker accounts' : 'Found credentials from Fennel Trader' }}</p>
                    </div>
                    <div class="login-form">
                        <div v-if="legacyImporting" class="legacy-spinner-container">
                            <div class="legacy-spinner"></div>
                        </div>
                        <template v-else>
                            <p class="legacy-description">
                                Would you like to import your linked broker accounts from the previous version?
                            </p>
                            <div v-if="legacyImportError" class="legacy-error">
                                {{ legacyImportError }}
                            </div>
                            <div class="legacy-actions">
                                <button class="btn primary" @click="handleLegacyImport">Import</button>
                                <button class="btn secondary" @click="handleLegacySkip">Skip</button>
                            </div>
                        </template>
                    </div>
                </div>
            </div>

            <!-- Login Screen -->
            <div v-else-if="!isLoggedIn" class="login-screen">
                <div class="login-container">
                    <div class="login-header">
                        <h1>{{ loginStep === 'select-broker' ? 'Select Broker' : (linkedBrokers.length === 0 ? 'Link Broker' : 'Login') }}</h1>
                        <p v-if="loginStep !== 'select-broker' && selectedBrokerType" class="broker-type-indicator">{{ selectedBrokerType }}</p>
                    </div>

                    <!-- Broker Selection Step (for clean launch) -->
                    <div v-if="loginStep === 'select-broker'" class="login-form">
                        <p class="broker-select-subtitle">Choose a broker to link:</p>
                        <div class="broker-options">
                            <div
                                class="broker-option clickable"
                                @click="selectAndProceedBroker('fennel')"
                            >
                                <div class="broker-option-name">Fennel</div>
                                <div class="broker-option-desc">Zero-commission stock trading (2FA)</div>
                            </div>
                            <div
                                class="broker-option clickable"
                                @click="selectAndProceedBroker('public')"
                            >
                                <div class="broker-option-name">Public</div>
                                <div class="broker-option-desc">Stocks, ETFs, options, crypto (API Key)</div>
                            </div>
                            <div
                                class="broker-option clickable"
                                @click="selectAndProceedBroker('robinhood')"
                            >
                                <div class="broker-option-name">Robinhood</div>
                                <div class="broker-option-desc">Stocks, ETFs, options, crypto (Password)</div>
                            </div>
                            <div
                                class="broker-option clickable"
                                @click="selectAndProceedBroker('tastytrade')"
                            >
                                <div class="broker-option-name">Tastytrade</div>
                                <div class="broker-option-desc">Stocks, options, futures (OAuth / API Key)</div>
                            </div>
                            <div
                                class="broker-option clickable"
                                @click="selectAndProceedBroker('webull')"
                            >
                                <div class="broker-option-name">Webull</div>
                                <div class="broker-option-desc">Stocks, ETFs, options (API Key)</div>
                            </div>
                            <div
                                class="broker-option clickable"
                                @click="selectAndProceedBroker('lighthorse')"
                            >
                                <div class="broker-option-name">Light Horse</div>
                                <div class="broker-option-desc">Stocks, ETFs, options (API Key)</div>
                            </div>
                        </div>
                    </div>

                    <div v-else-if="loginStep === 'email'" class="login-form">
                        <div class="input-group">
                            <label for="loginEmail">{{ selectedBrokerType === 'public' || selectedBrokerType === 'webull' || selectedBrokerType === 'tastytrade' || selectedBrokerType === 'lighthorse' ? 'Email / Identifier' : (selectedBrokerType === 'robinhood' ? 'Username or Email' : 'Email Address') }}</label>
                            <input
                                type="text"
                                id="loginEmail"
                                :name="'login_email_' + randomId"
                                v-model="emailInput123"
                                @focus="disableAutocomplete"
                                @click="disableAutocomplete"
                                @keydown="preventAutofill"
                                autocomplete="new-password"
                                autocorrect="off"
                                autocapitalize="off"
                                spellcheck="false"
                                data-form-type="other"
                                data-lpignore="true"
                                readonly
                                onfocus="this.removeAttribute('readonly');"
                                @keyup.enter="initiateLogin"
                            />
                        </div>
                        <!-- For Public: Show API key input directly -->
                        <div v-if="selectedBrokerType === 'public'" class="input-group">
                            <label for="apiKeyInput">Secret Key</label>
                            <input
                                type="password"
                                id="apiKeyInput"
                                v-model="apiKeyInput"
                                placeholder="Enter your Public API secret key"
                                autocomplete="new-password"
                                @keyup.enter="loginWithApiKey"
                            />
                            <p class="input-hint">Get your secret key from Public.com: Account Settings &rarr; Security &rarr; API</p>
                        </div>
                        <!-- For Robinhood: Show password input -->
                        <div v-if="selectedBrokerType === 'robinhood'" class="input-group">
                            <label for="passwordInput">Password</label>
                            <input
                                type="password"
                                id="passwordInput"
                                v-model="passwordInput"
                                placeholder="Enter your Robinhood password"
                                autocomplete="new-password"
                                @keyup.enter="loginWithPassword"
                            />
                        </div>
                        <!-- For Tastytrade: Show OAuth credential inputs -->
                        <div v-if="selectedBrokerType === 'tastytrade'" class="input-group">
                            <label for="tastyClientSecret">Client Secret</label>
                            <input
                                type="password"
                                id="tastyClientSecret"
                                v-model="tastyClientSecret"
                                placeholder="Enter your Client Secret"
                                autocomplete="new-password"
                            />
                        </div>
                        <div v-if="selectedBrokerType === 'tastytrade'" class="input-group">
                            <label for="tastyRefreshToken">Refresh Token</label>
                            <input
                                type="password"
                                id="tastyRefreshToken"
                                v-model="tastyRefreshToken"
                                placeholder="Enter your Refresh Token"
                                autocomplete="new-password"
                                @keyup.enter="loginWithTastytrade"
                            />
                            <p class="input-hint">Create an OAuth app at <a href="https://my.tastytrade.com/app.html#/manage/api-access/oauth-applications" target="_blank">Tastytrade OAuth Applications</a>, then use Manage &rarr; Create Grant for the refresh token</p>
                        </div>
                        <!-- For Webull: Show App Key and App Secret inputs -->
                        <div v-if="selectedBrokerType === 'webull'" class="input-group">
                            <label for="webullAppKey">App Key</label>
                            <input
                                type="text"
                                id="webullAppKey"
                                v-model="webullAppKey"
                                placeholder="Enter your Webull App Key"
                                autocomplete="new-password"
                            />
                        </div>
                        <div v-if="selectedBrokerType === 'webull'" class="input-group">
                            <label for="webullAppSecret">App Secret</label>
                            <input
                                type="password"
                                id="webullAppSecret"
                                v-model="webullAppSecret"
                                placeholder="Enter your Webull App Secret"
                                autocomplete="new-password"
                                @keyup.enter="loginWithWebull"
                            />
                            <p class="input-hint">Get your API credentials from Webull: OpenAPI Management in your account settings</p>
                        </div>
                        <!-- For Light Horse: Show API Key, API Secret, and Account ID inputs -->
                        <div v-if="selectedBrokerType === 'lighthorse'" class="input-group">
                            <label for="lhApiKey">API Key</label>
                            <input
                                type="text"
                                id="lhApiKey"
                                v-model="lhApiKey"
                                placeholder="Enter your Light Horse API Key"
                                autocomplete="new-password"
                            />
                        </div>
                        <div v-if="selectedBrokerType === 'lighthorse'" class="input-group">
                            <label for="lhApiSecret">API Secret</label>
                            <input
                                type="password"
                                id="lhApiSecret"
                                v-model="lhApiSecret"
                                placeholder="Enter your Light Horse API Secret"
                                autocomplete="new-password"
                            />
                        </div>
                        <div v-if="selectedBrokerType === 'lighthorse'" class="input-group">
                            <label for="lhAccountId">Account ID</label>
                            <input
                                type="text"
                                id="lhAccountId"
                                v-model="lhAccountId"
                                placeholder="Enter your Light Horse Account ID"
                                autocomplete="new-password"
                                @keyup.enter="loginWithLighthorse"
                            />
                            <p class="input-hint">Get your API credentials from Light Horse: log in at <a href="https://portal.lighthorse.io" target="_blank">portal.lighthorse.io</a> &rarr; Settings &rarr; API Keys</p>
                        </div>
                        <div class="login-actions">
                            <button v-if="selectedBrokerType === 'public'" class="btn primary wide-btn" @click="loginWithApiKey" :disabled="!apiKeyInput">Login</button>
                            <button v-else-if="selectedBrokerType === 'robinhood'" class="btn primary wide-btn" @click="loginWithPassword" :disabled="!passwordInput">Login</button>
                            <button v-else-if="selectedBrokerType === 'tastytrade'" class="btn primary wide-btn" @click="loginWithTastytrade" :disabled="!tastyClientSecret || !tastyRefreshToken">Login</button>
                            <button v-else-if="selectedBrokerType === 'webull'" class="btn primary wide-btn" @click="loginWithWebull" :disabled="!webullAppKey || !webullAppSecret">Login</button>
                            <button v-else-if="selectedBrokerType === 'lighthorse'" class="btn primary wide-btn" @click="loginWithLighthorse" :disabled="!lhApiKey || !lhApiSecret || !lhAccountId">Login</button>
                            <button v-else class="btn primary wide-btn" @click="initiateLogin">Continue</button>
                            <button class="btn secondary wide-btn" @click="goBackFromLogin">Back</button>
                        </div>
                    </div>

                    <div v-else-if="loginStep === 'prompt-approval'" class="login-form">
                        <h2>Approve Login</h2>
                        <div class="prompt-approval-container">
                            <div class="prompt-approval-icon">
                                <svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="#64ffda" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                                    <rect x="5" y="2" width="14" height="20" rx="2" ry="2"></rect>
                                    <line x1="12" y1="18" x2="12.01" y2="18"></line>
                                </svg>
                            </div>
                            <p class="prompt-approval-text">Open the <strong>Robinhood app</strong> on your phone and approve the login request.</p>
                            <div class="prompt-approval-spinner"></div>
                            <p class="prompt-approval-hint">Waiting for approval...</p>
                        </div>
                        <div class="tfa-buttons">
                            <button class="btn secondary wide-btn" @click="loginStep = 'email'">Cancel</button>
                        </div>
                    </div>

                    <div v-else-if="loginStep === '2fa'" class="login-form">
                        <h2>{{ selectedBrokerType === 'robinhood' ? 'Multi-Factor Authentication' : 'Two-Factor Authentication' }}</h2>
                        <p>{{ selectedBrokerType === 'robinhood' ? 'Enter the verification code sent to your phone.' : 'Please enter the verification code sent to your email.' }}</p>
                        <input
                            type="text"
                            v-model="verificationCode"
                            :placeholder="selectedBrokerType === 'robinhood' ? 'MFA Code' : 'Verification Code'"
                            class="verification-input"
                            ref="verificationInput"
                            @keyup.enter="verify2FA"
                        />
                        <div class="tfa-buttons">
                            <button class="btn primary wide-btn" @click="verify2FA" :disabled="verificationCode.length === 0">Verify</button>
                            <button class="btn secondary wide-btn" @click="loginStep = 'email'">Back</button>
                        </div>
                        <div v-show="!isTransitioningTo2FA && loginMessage" class="error-message">
                            {{ loginMessage }}
                        </div>
                    </div>
                    
                    <!-- Status message for login feedback (only shown on email screen) -->
                    <div v-if="loginMessage && loginStep === 'email'" class="login-status-message">
                        {{ loginMessage }}
                    </div>
                </div>
            </div>

            <!-- Main App Content (only shown when logged in) -->
            <div v-else>
                <div class="app-header">
                    <div class="app-tabs">
                        <button 
                            class="tab-button" 
                            :class="{ active: activeTab === 'orders' }"
                            @click="activeTab = 'orders'"
                        >
                            Place Orders
                        </button>
                        <button 
                            class="tab-button" 
                            :class="{ active: activeTab === 'accounts' }"
                            @click="activeTab = 'accounts'"
                        >
                            View Accounts
                        </button>
                    </div>
                    <div class="auth-status">
                        <div class="logged-in-status">
                            <div class="linked-brokers-list">
                                <span
                                    v-for="broker in sortedLinkedBrokers"
                                    :key="broker.id"
                                    class="broker-badge"
                                    :class="{
                                        'connected': broker.status === 'connected',
                                        'disconnected': broker.status !== 'connected',
                                        'clickable': broker.status !== 'connected'
                                    }"
                                    :title="broker.status === 'connected' ? broker.email : 'Click to reconnect - ' + broker.email"
                                    :style="getBrokerBadgeStyle(broker)"
                                    @click="broker.status !== 'connected' && startReauthBroker(broker)"
                                >
                                    {{ broker.type.toUpperCase() }}
                                    <span v-if="broker.status !== 'connected'" class="reauth-icon">!</span>
                                </span>
                            </div>
                            <button class="btn secondary small-btn" @click="showLinkBrokerModal = true">+ Link</button>
                            <div class="unlink-dropdown" v-if="linkedBrokers.length > 0">
                                <button class="btn secondary small-btn" @click="showUnlinkDropdown = !showUnlinkDropdown">Unlink</button>
                                <div v-if="showUnlinkDropdown" class="dropdown-menu">
                                    <div
                                        v-for="broker in sortedLinkedBrokers"
                                        :key="broker.id"
                                        class="dropdown-item"
                                        @click="unlinkBroker(broker.id)"
                                    >
                                        {{ broker.type.toUpperCase() }} ({{ broker.email }})
                                    </div>
                                </div>
                            </div>
                            <button
                                class="btn secondary small-btn"
                                @click="checkForUpdates()"
                                :disabled="isCheckingForUpdates || isInstallingUpdate"
                            >
                                {{ isInstallingUpdate ? 'Updating...' : (isCheckingForUpdates ? 'Checking...' : 'Check Updates') }}
                            </button>
                            <span v-if="updateStatus" class="update-status">{{ updateStatus }}</span>
                        </div>
                    </div>

                    <!-- Link Broker Modal -->
                    <div v-if="showLinkBrokerModal" class="modal-overlay" @click.self="showLinkBrokerModal = false">
                        <div class="modal-content">
                            <div class="modal-header">
                                <h2>Link Another Broker</h2>
                                <button class="modal-close" @click="showLinkBrokerModal = false">&times;</button>
                            </div>
                            <div class="broker-options">
                                <div
                                    class="broker-option clickable"
                                    @click="selectAndLinkBroker('fennel')"
                                >
                                    <div class="broker-option-name">Fennel</div>
                                    <div class="broker-option-desc">Zero-commission stock trading (2FA)</div>
                                </div>
                                <div
                                    class="broker-option clickable"
                                    @click="selectAndLinkBroker('public')"
                                >
                                    <div class="broker-option-name">Public</div>
                                    <div class="broker-option-desc">Stocks, ETFs, options, crypto (API Key)</div>
                                </div>
                                <div
                                    class="broker-option clickable"
                                    @click="selectAndLinkBroker('robinhood')"
                                >
                                    <div class="broker-option-name">Robinhood</div>
                                    <div class="broker-option-desc">Stocks, ETFs, options, crypto (Password)</div>
                                </div>
                                <div
                                    class="broker-option clickable"
                                    @click="selectAndLinkBroker('tastytrade')"
                                >
                                    <div class="broker-option-name">Tastytrade</div>
                                    <div class="broker-option-desc">Stocks, options, futures (OAuth / API Key)</div>
                                </div>
                                <div
                                    class="broker-option clickable"
                                    @click="selectAndLinkBroker('webull')"
                                >
                                    <div class="broker-option-name">Webull</div>
                                    <div class="broker-option-desc">Stocks, ETFs, options (API Key)</div>
                                </div>
                                <div
                                    class="broker-option clickable"
                                    @click="selectAndLinkBroker('lighthorse')"
                                >
                                    <div class="broker-option-name">Light Horse</div>
                                    <div class="broker-option-desc">Stocks, ETFs, options (API Key)</div>
                                </div>
                            </div>
                        </div>
                    </div>
                </div>

                <div class="main-section">
                    <!-- Orders Tab -->
                    <div v-if="activeTab === 'orders'" class="tab-content orders-tab-content">
                        <div class="orders-container">
                            <div class="app-section config-section">
                                <div class="section-title-container">
                                    <div class="section-title">Place Orders</div>
                                </div>
                                <div class="input-group">
                                    <label for="tickerInput456">Ticker Symbol</label>
                                    <input type="text" :id="'ticker_' + randomId" :name="'ticker_' + randomId"
                                        v-model="tickerInput456" @focus="disableAutocomplete" @click="disableAutocomplete"
                                        @blur="handleBlur" @keydown="preventAutofill" autocomplete="new-password" autocorrect="off"
                                        autocapitalize="off" spellcheck="false" data-form-type="other" data-lpignore="true" readonly
                                        onfocus="this.removeAttribute('readonly');" />
                                </div>
                                <div class="input-group">
                                    <label>Order Type</label>
                                    <div class="toggle-container">
                                        <label class="toggle-option" :class="{ active: side === 'buy' }">
                                            <input type="radio" id="buy" value="buy" v-model="side" />
                                            Buy
                                        </label>
                                        <label class="toggle-option" :class="{ active: side === 'sell' }">
                                            <input type="radio" id="sell" value="sell" v-model="side" />
                                            Sell
                                        </label>
                                    </div>
                                </div>
                                <div class="input-group">
                                    <label for="sharesInput">Number of Shares</label>
                                    <div class="shares-input-row">
                                        <input
                                            v-if="!maxSharesByBroker"
                                            type="number"
                                            id="sharesInput"
                                            v-model.number="sharesInput"
                                            min="0.01"
                                            step="any"
                                        />
                                        <div v-else class="max-shares-display" @click="clearMaxShares">
                                            All shares
                                        </div>
                                        <button
                                            v-if="side === 'sell' && !maxSharesByBroker"
                                            type="button"
                                            class="btn secondary max-shares-btn"
                                            @click="fillMaxShares"
                                            title="Sell all shares held for this ticker"
                                        >Max</button>
                                        <button
                                            v-if="maxSharesByBroker"
                                            type="button"
                                            class="btn secondary max-shares-btn"
                                            @click="clearMaxShares"
                                            title="Clear and enter a custom amount"
                                        >Clear</button>
                                    </div>
                                </div>
                                <!-- Broker/Account Selection with Checkboxes -->
                                <div class="input-group broker-account-selection">
                                    <div class="broker-selection-header">
                                        <label>Brokers & Accounts</label>
                                        <div class="selection-buttons" v-if="connectedBrokers.length > 0">
                                            <button type="button" class="selection-btn" @click="selectAllBrokers">Select All</button>
                                            <button type="button" class="selection-btn" @click="selectNoneBrokers">Select None</button>
                                        </div>
                                    </div>
                                    <div class="broker-selection-list">
                                        <div
                                            v-for="broker in connectedBrokers"
                                            :key="broker.id"
                                            class="broker-selection-item"
                                        >
                                            <div class="broker-checkbox-row">
                                                <label class="checkbox-label" :style="{ color: getBrokerColor(broker.type).primary }">
                                                    <input
                                                        type="checkbox"
                                                        :checked="selectedBrokers[broker.id]"
                                                        @change="toggleBroker(broker.id)"
                                                    />
                                                    <span class="broker-name">{{ broker.type.toUpperCase() }}</span>
                                                </label>
                                                <!-- Show account dropdown if broker is selected and has multiple accounts -->
                                                <div
                                                    v-if="selectedBrokers[broker.id] && getBrokerAccounts(broker.id).length > 1"
                                                    class="account-selection"
                                                >
                                                    <div class="account-checkboxes">
                                                        <label
                                                            v-for="account in getBrokerAccounts(broker.id)"
                                                            :key="account.id"
                                                            class="account-checkbox-label"
                                                        >
                                                            <input
                                                                type="checkbox"
                                                                :checked="selectedAccounts[account.id]"
                                                                @change="toggleAccount(account.id)"
                                                            />
                                                            <span>{{ account.name }}</span>
                                                        </label>
                                                    </div>
                                                </div>
                                                <!-- Show single account indicator if only one account -->
                                                <span
                                                    v-else-if="selectedBrokers[broker.id] && getBrokerAccounts(broker.id).length === 1"
                                                    class="single-account-indicator"
                                                >
                                                    {{ getBrokerAccounts(broker.id)[0]?.name || '1 account' }}
                                                </span>
                                            </div>
                                        </div>
                                    </div>
                                    <div v-if="connectedBrokers.length === 0" class="no-brokers-message">
                                        No brokers connected
                                    </div>
                                </div>
                                <div class="action-buttons">
                                    <button class="btn primary wide-btn" @click="placeOrder" :disabled="orderCountdown > 0">
                                        <span v-if="orderCountdown > 0">
                                            Processing Order...
                                        </span>
                                        <span v-else>
                                            Place Order
                                        </span>
                                    </button>
                                </div>
                            </div>
                            
                            <div class="app-section log-section">
                                <div class="section-title-container">
                                    <div class="section-title">Activity Log</div>
                                </div>
                                <div class="log-inner">
                                    <div class="log-actions">
                                        <button class="btn secondary" @click="clearLog">Clear Log</button>
                                    </div>
                                    <div class="log-container" id="log">
                                        <div v-for="(entry, index) in logs" :key="index" class="log-entry">
                                            <span class="timestamp">{{ entry.timestamp }}</span>
                                            <span
                                                v-if="entry.broker"
                                                class="log-broker-badge"
                                                :style="{
                                                    backgroundColor: getBrokerColor(entry.broker).background,
                                                    color: getBrokerColor(entry.broker).primary
                                                }"
                                            >{{ entry.broker }}</span>
                                            <span class="message">{{ entry.message }}</span>
                                        </div>
                                    </div>
                                </div>
                            </div>
                        </div>
                    </div>

                    <!-- Accounts Tab -->
                    <div v-else-if="activeTab === 'accounts'" class="tab-content accounts-tab-content">
                        <div class="app-section account-section">
                            <div class="section-title-container">
                                <div class="section-title">Accounts</div>
                                <button class="btn secondary small-btn" @click="refreshAccountsManual">Refresh</button>
                            </div>
                            <AccountView
                                ref="accountViewRef"
                                :isLoggedIn="isLoggedIn"
                                :forceReload="shouldReloadAccounts"
                                :linkedBrokers="linkedBrokers"
                                @accounts-loaded="shouldReloadAccounts = false"
                                @trade-ticker="handleTradeTicker"
                            />
                        </div>
                    </div>
                </div>
            </div>
        </div>
        <!-- Minimal edit context menu (Cut/Copy/Paste) -->
        <div
            v-if="editMenu.visible"
            class="edit-context-menu"
            :style="{ top: editMenu.y + 'px', left: editMenu.x + 'px' }"
            @click.stop
        >
            <div v-if="editMenu.showCut" class="edit-menu-item" @click="editMenuAction('cut')">Cut</div>
            <div v-if="editMenu.showCopy" class="edit-menu-item" @click="editMenuAction('copy')">Copy</div>
            <div v-if="editMenu.showPaste" class="edit-menu-item" @click="editMenuAction('paste')">Paste</div>
        </div>
    </div>
</template>


<script>
import { ref, watch, onMounted, onUnmounted, nextTick, computed, provide, reactive } from 'vue';
import AccountView from './AccountView.vue';
import {
    start2fa, loginCmd, frontendReady, linkBroker, unlinkBrokerCmd, logoutCmd,
    getAllBrokerAccounts, getBrokerAccountHoldings, getBrokerAccountCash,
    getAccountHoldings as getAccountHoldingsFallback, getAccountCash as getAccountCashFallback,
    placeOrderMultiBroker,
    isLoggedInCmd, getCurrentEmail,
    importLegacyCredentials, skipLegacyImport,
    checkForAppUpdate,
    onLog, onStartupComplete, on2faStarted, onLoginSuccess, onLoginFailure,
    onLogoutSuccess, onBrokerLinked, onBrokerUnlinked, onTokenExpired,
    onLegacyImportComplete
} from '@/tauri';

export default {
    components: {
        AccountView
    },
    setup() {
        const emailInput123 = ref('');
        const tickerInput456 = ref('');
        const side = ref('buy');
        const sharesInput = ref(1.0);
        // For the new checkbox-based selection
        const selectedBrokers = ref({}); // { brokerId: true/false }
        const selectedAccounts = ref({}); // { accountId: true/false }
        const logs = ref([]);
        const tfaCode = ref('');
        const randomId = ref(Math.random().toString(36).substring(2, 10) + Date.now());
        const tfaInput = ref(null);
        const isLandscape = ref(false);
        const activeTab = ref('orders');
        const isLoggedIn = ref(false);
        const currentEmail = ref('');
        const loginStep = ref('email');
        const loginMessage = ref('');
        const accountViewRef = ref(null);
        const shouldReloadAccounts = ref(false);
        const orderCountdown = ref(0);
        const verificationCode = ref('');
        const verificationInput = ref(null);
        const isTransitioning = ref(false);
        const isTransitioningTo2FA = ref(false);
        const isInitiatingLogin = ref(false);
        const apiKeyInput = ref('');
        const passwordInput = ref('');
        const webullAppKey = ref('');
        const webullAppSecret = ref('');
        const lhApiKey = ref('');
        const lhApiSecret = ref('');
        const lhAccountId = ref('');
        const tastyClientSecret = ref('');
        const tastyRefreshToken = ref('');
        const showUnlinkDropdown = ref(false);
        const updateStatus = ref('');
        const isCheckingForUpdates = ref(false);
        const isInstallingUpdate = ref(false);

        // Ensure login message is clear on initial setup
        loginMessage.value = ''; 

        // Create global state for persisting account data - Make it reactive
        const globalState = reactive({
            accountsData: {
                accounts: [],
                accountHoldings: {},
                accountCash: {},
                lastLoaded: null
            }
        });

        // Provide global state to child components
        provide('globalState', globalState);

        // Function to load the basic account list from ALL brokers
        const loadAccountList = async () => {
            try {
                console.log("Loading account list from all brokers...");
                const result = await getAllBrokerAccounts();
                console.log("Account list loaded:", result);
                if (result) {
                    globalState.accountsData.accounts = result;
                    globalState.accountsData.lastLoaded = new Date(); // Update timestamp for list load
                }
            } catch (err) {
                console.error('Failed to load account list:', err);
                logMessage('Error loading account list: ' + err.message);
                // Optionally clear the list or handle the error appropriately
                globalState.accountsData.accounts = [];
            }
        };

        // Load full account details (holdings + cash) in the background
        // This runs without requiring AccountView to be mounted
        const loadAccountDetailsBackground = async () => {
            const accounts = globalState.accountsData.accounts || [];
            if (accounts.length === 0) return;

            console.log(`Background loading details for ${accounts.length} accounts...`);

            const promises = accounts.map(async (account) => {
                try {
                    const brokerId = account.brokerId;
                    let holdings, cash;

                    if (brokerId) {
                        holdings = await getBrokerAccountHoldings(brokerId, account.id);
                        cash = await getBrokerAccountCash(brokerId, account.id);
                    } else {
                        holdings = await getAccountHoldingsFallback(account.id);
                        cash = await getAccountCashFallback(account.id);
                    }

                    globalState.accountsData.accountHoldings[account.id] = holdings || [];
                    globalState.accountsData.accountCash[account.id] = cash || null;
                } catch (err) {
                    console.error(`Background load failed for account ${account.id}:`, err);
                }
            });

            await Promise.all(promises);
            globalState.accountsData.lastLoaded = new Date();
            console.log('Background account details loading complete');
        };

        // Computed property for connected brokers only (sorted alphabetically by type)
        const connectedBrokers = computed(() => {
            return linkedBrokers.value
                .filter(b => b.status === 'connected')
                .sort((a, b) => a.type.localeCompare(b.type));
        });

        // Computed property for all linked brokers sorted alphabetically
        const sortedLinkedBrokers = computed(() => {
            return [...linkedBrokers.value].sort((a, b) => a.type.localeCompare(b.type));
        });

        const accountsForDropdown = computed(() => {
            // Use the accounts from the global state if available
            console.log('Global accounts data:', globalState.accountsData?.accounts);
            const allAccounts = globalState.accountsData?.accounts || [];

            // Filter to only APPROVED accounts (non-approved are inactive/closed)
            const accountsToSort = allAccounts.filter(acc => acc.status === 'APPROVED');

            // Apply the same sorting logic as in AccountView
            return [...accountsToSort].sort((a, b) => {
                // Primary accounts first
                if (a.isPrimary && !b.isPrimary) return -1;
                if (!a.isPrimary && b.isPrimary) return 1;

                // Then by name - handle numeric account names properly
                // Ensure names exist, fall back to ID if needed for sorting robustness
                const nameA = a.name || `Account ${a.id?.substring(0, 6) || 'Unknown'}`;
                const nameB = b.name || `Account ${b.id?.substring(0, 6) || 'Unknown'}`;

                // Check if both names are in "Account X" format
                const accountRegex = /^Account\s+(\d+)$/;
                const matchA = nameA.match(accountRegex);
                const matchB = nameB.match(accountRegex);

                if (matchA && matchB) {
                    // If both are numeric accounts, sort numerically
                    return parseInt(matchA[1], 10) - parseInt(matchB[1], 10);
                } else if (matchA) {
                    // If only A is a numeric account, it comes before non-numeric
                    return -1;
                } else if (matchB) {
                    // If only B is a numeric account, it comes before non-numeric
                    return 1;
                }

                // Otherwise use alphabetical sorting for non-numeric names
                return nameA.localeCompare(nameB);
            });
        });

        // Get accounts for a specific broker
        const getBrokerAccounts = (brokerId) => {
            const allAccounts = globalState.accountsData?.accounts || [];
            return allAccounts
                .filter(acc => acc.brokerId === brokerId && acc.status === 'APPROVED')
                .sort((a, b) => {
                    const nameA = a.name || '';
                    const nameB = b.name || '';
                    return nameA.localeCompare(nameB);
                });
        };

        // Toggle broker selection
        const toggleBroker = (brokerId) => {
            const newValue = !selectedBrokers.value[brokerId];
            selectedBrokers.value[brokerId] = newValue;

            // If enabling broker, auto-select all its accounts
            if (newValue) {
                const brokerAccounts = getBrokerAccounts(brokerId);
                brokerAccounts.forEach(acc => {
                    selectedAccounts.value[acc.id] = true;
                });
            } else {
                // If disabling broker, deselect all its accounts
                const brokerAccounts = getBrokerAccounts(brokerId);
                brokerAccounts.forEach(acc => {
                    selectedAccounts.value[acc.id] = false;
                });
            }
        };

        // Toggle account selection
        const toggleAccount = (accountId) => {
            selectedAccounts.value[accountId] = !selectedAccounts.value[accountId];
        };

        // Initialize broker selections when brokers are loaded
        const initializeBrokerSelections = () => {
            // Select all connected brokers by default (except Webull)
            connectedBrokers.value.forEach(broker => {
                if (selectedBrokers.value[broker.id] === undefined) {
                    // Special behavior: Webull is NOT selected by default
                    selectedBrokers.value[broker.id] = broker.type !== 'webull';
                }
            });
            // Select all accounts by default (except Webull accounts)
            const allAccounts = globalState.accountsData?.accounts || [];
            allAccounts.forEach(acc => {
                if (acc.status === 'APPROVED' && selectedAccounts.value[acc.id] === undefined) {
                    // Find the broker for this account to check if it's Webull
                    const accountBroker = connectedBrokers.value.find(b => b.id === acc.brokerId);
                    const isWebull = accountBroker?.type === 'webull';
                    selectedAccounts.value[acc.id] = !isWebull;
                }
            });
        };

        // Select all brokers and their accounts
        const selectAllBrokers = () => {
            connectedBrokers.value.forEach(broker => {
                selectedBrokers.value[broker.id] = true;
            });
            const allAccounts = globalState.accountsData?.accounts || [];
            allAccounts.forEach(acc => {
                if (acc.status === 'APPROVED') {
                    selectedAccounts.value[acc.id] = true;
                }
            });
        };

        // Deselect all brokers and their accounts
        const selectNoneBrokers = () => {
            connectedBrokers.value.forEach(broker => {
                selectedBrokers.value[broker.id] = false;
            });
            const allAccounts = globalState.accountsData?.accounts || [];
            allAccounts.forEach(acc => {
                selectedAccounts.value[acc.id] = false;
            });
        };

        // Watch for changes to track re-renders
        watch([emailInput123, tickerInput456], () => {
            console.log('Re-render triggered');
        });

        // Clear max shares when switching away from sell
        watch(side, (newSide) => {
            if (newSide !== 'sell' && maxSharesByBroker.value) {
                maxSharesByBroker.value = null;
                sharesInput.value = 1;
            }
        });

        // Watch for account data changes to initialize selections
        watch(() => globalState.accountsData.accounts, () => {
            initializeBrokerSelections();
        }, { deep: true });

        // Function to check if window is in landscape mode (wider than tall)
        const checkOrientation = () => {
            isLandscape.value = window.innerWidth > window.innerHeight * 1.2;
        };

        const formatTimestamp = () => {
            const now = new Date();
            return `${now.getHours().toString().padStart(2, '0')}:${now.getMinutes().toString().padStart(2, '0')}:${now.getSeconds().toString().padStart(2, '0')}`;
        };

        const knownBrokerNames = ['fennel', 'public', 'robinhood', 'tastytrade', 'webull', 'lighthorse', 'fidelity', 'schwab'];

        const normalizeBrokerType = (brokerType) => {
            if (!brokerType) return null;
            const normalized = String(brokerType).toLowerCase().trim();
            return knownBrokerNames.includes(normalized) ? normalized : null;
        };

        const getBrokerTypeFromId = (brokerId) => {
            if (!brokerId) return null;
            const broker = linkedBrokers.value.find((b) => b.id === brokerId);
            return normalizeBrokerType(broker?.type);
        };

        const detectBroker = (message, context = {}) => {
            const explicitType = normalizeBrokerType(context.brokerType);
            if (explicitType) {
                return explicitType;
            }

            const typeFromId = getBrokerTypeFromId(context.brokerId);
            if (typeFromId) {
                return typeFromId;
            }

            if (!message) return null;
            const lowerMsg = String(message).toLowerCase();

            for (const name of knownBrokerNames) {
                if (new RegExp(`\\b${name}\\b`).test(lowerMsg)) {
                    return name;
                }
            }

            for (const broker of linkedBrokers.value) {
                const brokerId = broker?.id;
                if (brokerId && lowerMsg.includes(String(brokerId).toLowerCase())) {
                    const resolvedType = normalizeBrokerType(broker.type);
                    if (resolvedType) {
                        return resolvedType;
                    }
                }
            }

            return null;
        };

        const logMessage = (message, context = {}) => {
            logs.value.push({
                message: message,
                timestamp: formatTimestamp(),
                broker: detectBroker(message, context)
            });
            // Scroll to bottom of log container
            setTimeout(() => {
                const logContainer = document.getElementById('log');
                if (logContainer) {
                    logContainer.scrollTop = logContainer.scrollHeight;
                }
            }, 0);
        };

        const clearLog = () => {
            logs.value = [];
        };

        const checkForUpdates = async (silent = false) => {
            if (isCheckingForUpdates.value || isInstallingUpdate.value) {
                return;
            }

            isCheckingForUpdates.value = true;
            if (!silent) {
                updateStatus.value = 'Checking for updates...';
                logMessage('Checking for app updates...');
            }

            try {
                const update = await checkForAppUpdate();
                if (!update) {
                    if (!silent) {
                        updateStatus.value = 'You are on the latest version.';
                        logMessage('No updates available.');
                    }
                    return;
                }

                isInstallingUpdate.value = true;
                updateStatus.value = `Downloading v${update.version}...`;
                logMessage(`Update v${update.version} is available. Downloading and installing...`);

                let downloadedBytes = 0;
                let contentLength = 0;

                await update.downloadAndInstall((event) => {
                    if (event.event === 'Started') {
                        downloadedBytes = 0;
                        contentLength = event.data.contentLength || 0;
                        updateStatus.value = `Downloading v${update.version}...`;
                    } else if (event.event === 'Progress') {
                        downloadedBytes += event.data.chunkLength || 0;
                        if (contentLength > 0) {
                            const percentage = Math.min(
                                100,
                                Math.round((downloadedBytes / contentLength) * 100)
                            );
                            updateStatus.value = `Downloading v${update.version}... ${percentage}%`;
                        }
                    } else if (event.event === 'Finished') {
                        updateStatus.value = 'Installing update...';
                    }
                });

                updateStatus.value = 'Update installed. Restarting may be required.';
                logMessage(`Update v${update.version} installed.`);
            } catch (error) {
                if (!silent) {
                    const message = error?.message || String(error);
                    updateStatus.value = 'Unable to reach update server.';
                    logMessage(`Update check failed (app will continue normally): ${message}`);
                }
            } finally {
                isCheckingForUpdates.value = false;
                isInstallingUpdate.value = false;
            }
        };

        const initiateLogin = () => {
            // Prevent double execution
            if (isInitiatingLogin.value) {
                console.log("Initiate login already in progress, ignoring.");
                return; 
            }
            isInitiatingLogin.value = true;

            // Set transition flag *first*
            isTransitioningTo2FA.value = true; 

            if (!emailInput123.value) {
                loginMessage.value = 'Error: Email is required';
                isTransitioningTo2FA.value = false; // Reset transition flag
                isInitiatingLogin.value = false; // Reset guard flag
                return;
            }
            
            // Clear any existing error messages & reset verification flag *before* switching step
            loginMessage.value = '';
            
            // Set login step to 2FA
            loginStep.value = '2fa';
            
            // Reset verification code input
            verificationCode.value = '';
            
            // Block any error messages from appearing for a short time
            // This prevents race conditions where error events might fire during transition
            const blockErrorMessages = () => {
                if (loginStep.value === '2fa' && !verificationCode.value) {
                    loginMessage.value = '';
                }
            };
            
            // Set up multiple checks to ensure no error messages appear
            const errorBlocker = setInterval(blockErrorMessages, 50);
            
            // Clear the interval after a reasonable time
            setTimeout(() => {
                clearInterval(errorBlocker);
            }, 1000);
            
            logMessage(`Initiating login for ${emailInput123.value}...`, { brokerType: selectedBrokerType.value });

            // Invoke the start 2FA command - include broker type to ensure correct broker is used
            start2fa(emailInput123.value, selectedBrokerType.value);
            
            // Focus on verification input after transition
            nextTick(() => {
                if (verificationInput.value) {
                    verificationInput.value.focus();
                }
                // One final check to ensure no error messages
                blockErrorMessages();
                // Explicitly clear the message here *again* just before resetting flags
                loginMessage.value = ''; 
                // Reset transition flag after DOM update and focus
                isTransitioningTo2FA.value = false; 
                // Reset guard flag after transition is complete
                isInitiatingLogin.value = false; 
            });
        };

        const verify2FA = () => {
            // Clear previous message
            loginMessage.value = '';

            if (!verificationCode.value) {
                loginMessage.value = 'Error: Verification code required';
                return;
            }

            logMessage('Submitting verification code...', { brokerType: selectedBrokerType.value });
            loginCmd(verificationCode.value, emailInput123.value);
            verificationCode.value = '';
        };

        const loginWithApiKey = () => {
            // For Public broker - login with API secret key
            if (!apiKeyInput.value) {
                loginMessage.value = 'Error: API secret key is required';
                return;
            }

            loginMessage.value = '';
            logMessage('Authenticating with Public API...', { brokerType: 'public' });

            // For Public broker, the secret key is passed as the "code" parameter
            // and an empty string or placeholder as email since Public uses API keys
            loginCmd(apiKeyInput.value, 'api-key-auth');
            apiKeyInput.value = '';
        };

        const loginWithPassword = () => {
            // For Robinhood broker - login with password
            if (!passwordInput.value) {
                loginMessage.value = 'Error: Password is required';
                return;
            }
            if (!emailInput123.value) {
                loginMessage.value = 'Error: Username/Email is required';
                return;
            }

            loginMessage.value = '';
            logMessage('Authenticating with Robinhood...', { brokerType: selectedBrokerType.value });

            // Show the prompt-approval screen right away for Robinhood since
            // the backend blocks while waiting for pathfinder + prompt approval
            if (selectedBrokerType.value === 'robinhood') {
                loginStep.value = 'prompt-approval';
            }

            // Password is passed as the "code" parameter
            loginCmd(passwordInput.value, emailInput123.value);
            passwordInput.value = '';
        };

        const loginWithTastytrade = () => {
            // For Tastytrade broker - login with OAuth client secret + refresh token
            if (!tastyClientSecret.value) {
                loginMessage.value = 'Error: Client Secret is required';
                return;
            }
            if (!tastyRefreshToken.value) {
                loginMessage.value = 'Error: Refresh Token is required';
                return;
            }
            if (!emailInput123.value) {
                loginMessage.value = 'Error: Email / Identifier is required';
                return;
            }

            loginMessage.value = '';
            logMessage('Authenticating with Tastytrade...', { brokerType: selectedBrokerType.value });

            // Composite string: "client_secret,refresh_token" passed as "code"
            const composite = tastyClientSecret.value + ',' + tastyRefreshToken.value;
            loginCmd(composite, emailInput123.value);
            tastyClientSecret.value = '';
            tastyRefreshToken.value = '';
        };

        const loginWithWebull = () => {
            // For Webull broker - login with app key and secret
            if (!webullAppKey.value) {
                loginMessage.value = 'Error: App Key is required';
                return;
            }
            if (!webullAppSecret.value) {
                loginMessage.value = 'Error: App Secret is required';
                return;
            }
            if (!emailInput123.value) {
                loginMessage.value = 'Error: Email/Identifier is required';
                return;
            }

            loginMessage.value = '';
            logMessage('Authenticating with Webull...', { brokerType: 'webull' });

            // For Webull, we pass app_key,app_secret as the "code" parameter
            const credentials = webullAppKey.value + ',' + webullAppSecret.value;
            loginCmd(credentials, emailInput123.value);
            webullAppKey.value = '';
            webullAppSecret.value = '';
        };

        const loginWithLighthorse = () => {
            if (!lhApiKey.value) {
                loginMessage.value = 'Error: API Key is required';
                return;
            }
            if (!lhApiSecret.value) {
                loginMessage.value = 'Error: API Secret is required';
                return;
            }
            if (!lhAccountId.value) {
                loginMessage.value = 'Error: Account ID is required';
                return;
            }
            if (!emailInput123.value) {
                loginMessage.value = 'Error: Email/Identifier is required';
                return;
            }

            loginMessage.value = '';
            logMessage('Authenticating with Light Horse...', { brokerType: 'lighthorse' });

            const credentials = lhApiKey.value + ',' + lhApiSecret.value + ',' + lhAccountId.value;
            loginCmd(credentials, emailInput123.value);
            lhApiKey.value = '';
            lhApiSecret.value = '';
            lhAccountId.value = '';
        };

        const cancelLogin = () => {
            loginStep.value = 'email';
            verificationCode.value = '';
            loginMessage.value = '';
            logMessage('Login cancelled');
        };

        // Per-broker share counts for selling (set by Max button)
        const maxSharesByBroker = ref(null); // null = normal mode, truthy = "sell all" mode (resolved at order time)

        // fillMaxShares toggles "sell all" mode on.
        // The backend resolves per-account holdings at order time.
        const fillMaxShares = () => {
            maxSharesByBroker.value = true;
        };

        const clearMaxShares = () => {
            maxSharesByBroker.value = null;
            sharesInput.value = 1;
        };

        const placeOrder = async () => {
            // Check if an order is already in progress
            if (window.orderInProgress) {
                logMessage('Another order is already in progress. Please wait.');
                return;
            }

            // Set order in progress flag
            window.orderInProgress = true;

            // Trim ticker input to handle copy/paste whitespace
            tickerInput456.value = tickerInput456.value.trim();

            // Basic validation
            if (!tickerInput456.value) {
                logMessage('Please enter a ticker symbol.');
                window.orderInProgress = false; // Reset flag
                return;
            }
            if (!maxSharesByBroker.value && sharesInput.value <= 0) {
                logMessage('Please enter a valid number of shares (must be greater than 0).');
                window.orderInProgress = false; // Reset flag
                return;
            }

            // Get selected accounts from the new checkbox system
            const selectedAccountIds = Object.entries(selectedAccounts.value)
                .filter(([_, isSelected]) => isSelected)
                .map(([accountId, _]) => accountId);

            if (selectedAccountIds.length === 0) {
                logMessage('Please select at least one account.');
                window.orderInProgress = false;
                return;
            }

            const ticker = tickerInput456.value.toUpperCase();
            const orderSide = side.value;
            const shares = sharesInput.value;

            // Build account mapping by broker for multi-broker support
            const allAccounts = globalState.accountsData?.accounts || [];
            const accountsByBroker = {};

            selectedAccountIds.forEach(accountId => {
                const account = allAccounts.find(acc => acc.id === accountId);
                if (account && account.brokerId) {
                    if (!accountsByBroker[account.brokerId]) {
                        accountsByBroker[account.brokerId] = [];
                    }
                    accountsByBroker[account.brokerId].push(account.name);
                }
            });

            // Convert arrays to comma-separated strings for the backend
            const accountsByBrokerStr = {};
            for (const [brokerId, accountNames] of Object.entries(accountsByBroker)) {
                accountsByBrokerStr[brokerId] = accountNames.join(',');
            }

            const isSellMax = !!(maxSharesByBroker.value && orderSide === 'sell');

            const totalAccounts = selectedAccountIds.length;
            const brokerCount = Object.keys(accountsByBroker).length;
            const sharesLabel = isSellMax ? 'all' : shares;
            logMessage(`Placing ${orderSide} order for ${sharesLabel} shares of ${ticker} in ${totalAccounts} account(s) across ${brokerCount} broker(s)...`);

            // Set a cooldown period (e.g., 5 seconds)
            orderCountdown.value = 5;
            const countdownInterval = setInterval(() => {
                orderCountdown.value -= 1;
                if (orderCountdown.value <= 0) {
                    clearInterval(countdownInterval);
                    window.orderInProgress = false; // Reset flag when cooldown ends
                }
            }, 1000);

            try {
                const results = await placeOrderMultiBroker(ticker, orderSide, shares, accountsByBrokerStr, isSellMax);
                if (isSellMax) {
                    maxSharesByBroker.value = null; // Reset after use
                }

                // Process results from each broker
                let hasErrors = false;
                let hasSuccess = false;

                for (const [brokerId, result] of Object.entries(results)) {
                    // Find broker type for display
                    const broker = linkedBrokers.value.find(b => b.id === brokerId);
                    const brokerName = broker ? broker.type.toUpperCase() : brokerId;

                    if (result.includes("Error")) {
                        logMessage(`${brokerName}: ${result}`, { brokerId });
                        hasErrors = true;
                    } else {
                        logMessage(`${brokerName}: ${result}`, { brokerId });
                        hasSuccess = true;
                    }
                }

                if (hasSuccess && !hasErrors) {
                    // All orders successful - reset inputs
                    tickerInput456.value = '';
                    sharesInput.value = 1;
                    side.value = 'buy';
                    // Re-select all accounts
                    initializeBrokerSelections();

                    // Refresh accounts data after placing an order
                    await refreshAccounts();
                } else if (hasSuccess && hasErrors) {
                    logMessage('Some orders succeeded, some failed. Check above for details.');
                    // Still refresh to see partial changes
                    await refreshAccounts();
                }
            } catch (error) {
                logMessage(`Failed to place order: ${error}`);
            } finally {
                // Ensure the flag is reset even if there's an error immediately
                // or if the cooldown finishes before the order completes
                if (orderCountdown.value <= 0) {
                   window.orderInProgress = false;
                }
            }
        };

        const handleFocus = (e) => {
            console.log('Input focused');
        };

        const handleBlur = (e) => {
            console.log('Input blurred');
        };

        // Modify the disableAutocomplete function to handle Enter key for 2FA input
        const disableAutocomplete = (e) => {
            // Prevent default behavior
            e.preventDefault();

            // Force focus to stay on the input
            e.target.focus();

            // Disable autocomplete programmatically
            e.target.setAttribute('autocomplete', 'new-password');
            e.target.setAttribute('autocorrect', 'off');
            e.target.setAttribute('autocapitalize', 'off');
            e.target.setAttribute('spellcheck', 'false');

            // Set readonly attribute temporarily to prevent autocomplete
            e.target.setAttribute('readonly', 'readonly');
            setTimeout(() => {
                e.target.removeAttribute('readonly');
                // Reposition cursor at the end
                const val = e.target.value;
                e.target.value = '';
                e.target.value = val;
            }, 10);

            console.log('Input focused and autocomplete disabled');
        };

        // Prevent autofill on keydown and handle Enter key for 2FA
        const preventAutofill = (e) => {
            // If it's an autofill event (typically has no key property)
            if (!e.key) {
                e.preventDefault();
                e.stopPropagation();
                return;
            }

            // Submit 2FA code when Enter key is pressed in the 2FA input
            if (e.key === 'Enter') {
                e.preventDefault();
                if (loginStep.value === 'email') {
                    initiateLogin();
                } else if (loginStep.value === '2fa') {
                    verify2FA();
                }
            }
        };

        // Broker/startup state tracking
        const startupState = ref('loading'); // 'loading', 'need_link', 'legacy_found', 'restoring', 'ready', 'error'
        const linkedBrokers = ref([]);
        const activeBroker = ref(null);
        const selectedBrokerType = ref('fennel'); // Default to fennel for now
        const legacyImporting = ref(false);
        const legacyImportError = ref('');
        const showLinkBrokerModal = ref(false);
        const modalSelectedBroker = ref(null);

        // Broker color configuration - consistent colors per broker type
        const brokerColors = {
            fennel: { primary: '#64ffda', background: 'rgba(100, 255, 218, 0.15)' },
            public: { primary: '#ff9800', background: 'rgba(255, 152, 0, 0.15)' },
            robinhood: { primary: '#00c805', background: 'rgba(0, 200, 5, 0.15)' },
            tastytrade: { primary: '#ff5722', background: 'rgba(255, 87, 34, 0.15)' },
            webull: { primary: '#e91e63', background: 'rgba(233, 30, 99, 0.15)' },
            lighthorse: { primary: '#3f51b5', background: 'rgba(63, 81, 181, 0.15)' },
            fidelity: { primary: '#4caf50', background: 'rgba(76, 175, 80, 0.15)' },
            schwab: { primary: '#2196f3', background: 'rgba(33, 150, 243, 0.15)' },
            default: { primary: '#9c27b0', background: 'rgba(156, 39, 176, 0.15)' }
        };

        const getBrokerColor = (brokerType) => {
            const type = (brokerType || 'default').toLowerCase();
            return brokerColors[type] || brokerColors.default;
        };

        const getBrokerBadgeStyle = (broker) => {
            const colors = getBrokerColor(broker.type);
            if (broker.status === 'connected') {
                return {
                    backgroundColor: colors.primary,
                    color: '#121212'
                };
            } else {
                return {
                    backgroundColor: '#ff1744',
                    color: '#ffffff',
                    border: '1px solid #ff1744',
                    boxShadow: '0 0 8px rgba(255, 23, 68, 0.6), 0 0 16px rgba(255, 23, 68, 0.3)'
                };
            }
        };

        // Function to proceed with selected broker from initial selection
        const proceedWithBroker = () => {
            if (!selectedBrokerType.value) return;
            loginStep.value = 'email';
            // Invoke command to prepare for this broker type
            linkBroker(selectedBrokerType.value);
        };

        // Function to select and immediately proceed with a broker (click = proceed)
        const selectAndProceedBroker = (brokerType) => {
            selectedBrokerType.value = brokerType;
            loginStep.value = 'email';
            // Invoke command to prepare for this broker type
            linkBroker(brokerType);
        };

        // Legacy credential migration
        const handleLegacyImport = async () => {
            legacyImporting.value = true;
            legacyImportError.value = '';
            logMessage('Importing credentials from Fennel Trader...');
            try {
                await importLegacyCredentials();
                // The onLegacyImportComplete listener will handle the rest
            } catch (error) {
                console.error('Legacy import error:', error);
                legacyImporting.value = false;
                legacyImportError.value = String(error);
                logMessage('Import failed: ' + error);
            }
        };

        const handleLegacySkip = async () => {
            logMessage('Skipped legacy credential import');
            try {
                await skipLegacyImport();
                // The onStartupComplete listener will handle switching to need_link state
            } catch (error) {
                console.error('Skip legacy error:', error);
                startupState.value = 'need_link';
                loginStep.value = 'select-broker';
            }
        };

        // Function to start linking a new broker from modal
        const startLinkingBroker = () => {
            if (!modalSelectedBroker.value) return;
            showLinkBrokerModal.value = false;
            selectedBrokerType.value = modalSelectedBroker.value;
            // Log out current session and start fresh link
            isLoggedIn.value = false;
            loginStep.value = 'email';
            emailInput123.value = '';
            verificationCode.value = '';
            loginMessage.value = '';
            linkBroker(modalSelectedBroker.value);
            modalSelectedBroker.value = null;
        };

        // Function to select and immediately link a broker from modal (click = link)
        const selectAndLinkBroker = (brokerType) => {
            showLinkBrokerModal.value = false;
            selectedBrokerType.value = brokerType;
            // Start the login flow for this broker while keeping existing brokers
            loginStep.value = 'email';
            emailInput123.value = '';
            verificationCode.value = '';
            loginMessage.value = '';
            // Mark as not logged in to show login screen
            isLoggedIn.value = false;
            linkBroker(brokerType);
        };

        // Function to go back from login/email screen to broker selection
        const goBackFromLogin = () => {
            // Reset login state
            emailInput123.value = '';
            verificationCode.value = '';
            loginMessage.value = '';
            apiKeyInput.value = '';
            passwordInput.value = '';

            // If we have linked brokers, just cancel and go back to main app
            if (linkedBrokers.value.length > 0) {
                // Re-check if any broker is connected
                const hasConnectedBroker = linkedBrokers.value.some(b => b.status === 'connected');
                if (hasConnectedBroker) {
                    isLoggedIn.value = true;
                    loginStep.value = 'email';
                } else {
                    // No connected brokers, go back to broker selection
                    loginStep.value = 'select-broker';
                }
            } else {
                // No brokers linked, go back to broker selection
                loginStep.value = 'select-broker';
            }
        };

        // Function to start re-authentication for a disconnected broker
        const startReauthBroker = (broker) => {
            console.log('Starting reauth for broker:', broker);
            selectedBrokerType.value = broker.type;
            emailInput123.value = broker.email || '';
            isLoggedIn.value = false;
            loginStep.value = 'email';
            loginMessage.value = '';
            verificationCode.value = '';
            // Remove the old broker from linkedBrokers list (backend will also remove it)
            // This prevents duplicate entries during reauth
            linkedBrokers.value = linkedBrokers.value.filter(b => b.id !== broker.id);
            linkBroker(broker.type);
            logMessage(`Reconnecting ${broker.type.toUpperCase()}...`, { brokerType: broker.type });
        };

        // Store unlisten handles for cleanup
        const unlistenHandles = [];

        // Set up Tauri event listeners in onMounted (they return Promises)
        onMounted(async () => {
            console.log("Registering Tauri event listeners");

            const unlistenLog = await onLog((payload) => {
                if (typeof payload === 'string') {
                    logMessage(payload);
                    return;
                }

                const message = payload?.message != null
                    ? String(payload.message)
                    : String(payload);
                const brokerType = payload?.brokerType || payload?.broker_type;
                const brokerId = payload?.brokerId || payload?.broker_id;
                logMessage(message, { brokerType, brokerId });
            });
            unlistenHandles.push(unlistenLog);

            // Listen for startup complete event (new broker system)
            const unlistenStartup = await onStartupComplete(async (data) => {
                console.log('Startup complete event received:', data);
                startupState.value = data.state;
                linkedBrokers.value = data.linkedBrokers || [];
                activeBroker.value = data.activeBroker || null;

                // Check if any brokers are connected
                const connectedBrokersList = (data.linkedBrokers || []).filter(b => b.status === 'connected');
                const hasConnectedBroker = connectedBrokersList.length > 0;
                const hasNeedsReauth = data.needsReauth && data.needsReauth.length > 0;

                if (data.state === 'ready' && data.activeBroker) {
                    // We have a cached login that worked - auto-login!
                    console.log('Auto-login from cached credentials for:', data.activeBroker.email);
                    isLoggedIn.value = true;
                    currentEmail.value = data.activeBroker.email;
                    loginStep.value = 'email'; // Reset for future use
                    if (hasNeedsReauth) {
                        logMessage('Session restored. Some brokers need re-authentication.');
                    } else {
                        logMessage('Session restored from cached credentials');
                    }
                    await loadAccountList();
                    // Load full details in background so accounts tab is ready
                    loadAccountDetailsBackground();
                } else if (hasConnectedBroker) {
                    // At least one broker is connected - show dashboard with warning
                    console.log('Some brokers connected, some need reauth. Showing dashboard.');
                    isLoggedIn.value = true;
                    currentEmail.value = connectedBrokersList[0].email || '';
                    loginStep.value = 'email'; // Reset for future use
                    logMessage('Some accounts need re-authentication. Click their badge to reconnect.');
                    await loadAccountList();
                    // Load full details in background so accounts tab is ready
                    loadAccountDetailsBackground();
                } else if (data.state === 'legacy_found') {
                    // Legacy Fennel Trader credentials detected
                    console.log('Legacy Fennel Trader credentials found');
                    isLoggedIn.value = false;
                    legacyImporting.value = false;
                    legacyImportError.value = '';
                } else if (data.state === 'need_link') {
                    // No cached credentials - show broker selection first
                    console.log('No cached credentials, showing broker selection');
                    isLoggedIn.value = false;
                    if (linkedBrokers.value.length === 0) {
                        // Clean launch - show broker selection
                        loginStep.value = 'select-broker';
                        selectedBrokerType.value = 'fennel'; // Pre-select fennel
                    } else {
                        // Has brokers but ALL need reauth - find the first one to reconnect
                        const firstBroker = linkedBrokers.value[0];
                        if (firstBroker) {
                            selectedBrokerType.value = firstBroker.type;
                        }
                        loginStep.value = 'email';
                        logMessage('All sessions expired. Please log in again.');
                    }
                } else if (hasNeedsReauth && !hasConnectedBroker) {
                    // All brokers need re-authentication
                    console.log('All brokers need re-auth:', data.needsReauth);
                    logMessage('Session expired. Please log in again.');
                    isLoggedIn.value = false;
                    // Set the broker type to the first one needing reauth
                    const firstNeedsReauth = linkedBrokers.value.find(b => b.status === 'needs_reauth');
                    if (firstNeedsReauth) {
                        selectedBrokerType.value = firstNeedsReauth.type;
                    }
                    loginStep.value = 'email';
                }
            });
            unlistenHandles.push(unlistenStartup);

            // Listen for legacy import completion — re-trigger startup flow
            const unlistenLegacy = await onLegacyImportComplete(async (data) => {
                console.log('Legacy import complete:', data);
                logMessage(`Imported ${data.imported} broker credential(s) from Fennel Trader`);
                // Re-run frontend_ready to restore sessions from imported credentials
                // Keep legacyImporting=true so the spinner stays visible until startupState changes
                await frontendReady();
            });
            unlistenHandles.push(unlistenLegacy);

            // Listen for token expired events
            // In Tauri, the event payload is a JSON object like { broker_id, message }
            const unlistenToken = await onTokenExpired((data) => {
                console.log('Token expired event:', data);

                const brokerId = data?.broker_id || data?.brokerId;
                const message = data?.message;

                logMessage(message || 'A session has expired. Please re-authenticate.', { brokerId });

                // Mark the specific broker as needs_reauth
                if (brokerId) {
                    const broker = linkedBrokers.value.find(b => b.id === brokerId);
                    if (broker) {
                        broker.status = 'needs_reauth';
                        console.log(`Broker ${broker.type} marked as needs_reauth`);
                    }
                }
            });
            unlistenHandles.push(unlistenToken);

            // Listen for broker linked event
            const unlistenBrokerLinked = await onBrokerLinked(async (brokerInfo) => {
                console.log('Broker linked:', brokerInfo);
                // Check if we already have a broker of this type (reauth case)
                const existingByType = linkedBrokers.value.findIndex(b => b.type === brokerInfo.type);
                if (existingByType !== -1) {
                    // Replace the existing broker entry (handles reauth where ID changes)
                    console.log('Replacing existing broker of type:', brokerInfo.type);
                    linkedBrokers.value.splice(existingByType, 1, brokerInfo);
                } else {
                    // Check by ID as well (shouldn't happen but just in case)
                    const existsById = linkedBrokers.value.find(b => b.id === brokerInfo.id);
                    if (!existsById) {
                        linkedBrokers.value.push(brokerInfo);
                    }
                }
                // Set as active if we don't have one
                if (!activeBroker.value) {
                    activeBroker.value = brokerInfo;
                }

                // Reload account list now that linkedBrokers is updated,
                // then initialize broker/account selections for the order form
                await loadAccountList();
                initializeBrokerSelections();

                // Load full details in background so accounts tab is ready
                loadAccountDetailsBackground();
            });
            unlistenHandles.push(unlistenBrokerLinked);

            // Listen for 2FA event
            const unlisten2fa = await on2faStarted(() => {
                // Reset verification code and attempt status when 2FA starts
                verificationCode.value = '';

                // IMPORTANT: Clear any error messages and ensure they stay cleared
                loginMessage.value = '';

                // Add a more aggressive approach to ensure no error messages appear
                // This will override any loginFailure events that might fire during the transition
                const clearErrorMessage = () => {
                    if (loginStep.value === '2fa' && !verificationCode.value) {
                        loginMessage.value = '';
                    }
                };

                // Clear immediately and then again after a short delay to catch any race conditions
                clearErrorMessage();
                setTimeout(clearErrorMessage, 50);
                setTimeout(clearErrorMessage, 100);
                setTimeout(clearErrorMessage, 200);
            });
            unlistenHandles.push(unlisten2fa);

            // Listen for login success event
            const unlistenLoginSuccess = await onLoginSuccess(async () => {
                // Update login status
                isLoggedIn.value = true;
                currentEmail.value = await getCurrentEmail();
                loginMessage.value = '';
                logMessage("Authentication successful", { brokerType: selectedBrokerType.value });

                // Load the account list immediately after login
                await loadAccountList();

                // Initialize broker/account selections for the new broker
                initializeBrokerSelections();

                // Load full account details in the background so they're ready
                loadAccountDetailsBackground();
            });
            unlistenHandles.push(unlistenLoginSuccess);

            // Listen for login failure event
            const unlistenLoginFailure = await onLoginFailure((errorMessage) => {
                console.log("Login failure event received:", errorMessage);

                // Check if broker requires MFA
                if (errorMessage && errorMessage.startsWith('MFA_REQUIRED:')) {
                    const mfaType = errorMessage.split(':')[1] || 'sms';
                    // For SMS/email MFA, show the code entry screen
                    if (mfaType === 'sms' || mfaType === 'email') {
                        logMessage(`MFA required (${mfaType}). Please enter the code.`, { brokerType: selectedBrokerType.value });
                        loginMessage.value = `Enter the verification code sent to your ${mfaType === 'sms' ? 'phone' : 'email'}`;
                        loginStep.value = '2fa';
                    }
                    // For prompt type, the prompt-approval screen is already showing
                    // (or will be shown by loginWithPassword). Nothing else to do —
                    // the backend is polling and will either succeed or timeout.
                    return;
                }

                // On any error, if we're on the prompt-approval screen, go back
                if (loginStep.value === 'prompt-approval') {
                    loginStep.value = 'email';
                }

                // Directly set the message. The v-if in the template handles the transition flag.
                loginMessage.value = `Error: ${errorMessage || 'Login failed'}`;
                logMessage(`Login failed: ${errorMessage || 'Unknown reason'}`, { brokerType: selectedBrokerType.value });
            });
            unlistenHandles.push(unlistenLoginFailure);

            // Listen for logout success event
            const unlistenLogout = await onLogoutSuccess(() => {
                isLoggedIn.value = false;
                currentEmail.value = '';
                loginStep.value = 'email';
                // Explicitly reset 2FA state
                verificationCode.value = '';
                loginMessage.value = 'Logged out successfully';
                setTimeout(() => { loginMessage.value = ''; }, 3000);
                logMessage('Logged out successfully');
                // Clear account data on logout
                globalState.accountsData.accounts = [];
                globalState.accountsData.accountHoldings = {};
                globalState.accountsData.accountCash = {};
                globalState.accountsData.lastLoaded = null;
            });
            unlistenHandles.push(unlistenLogout);

            // Listen for broker unlinked event
            const unlistenBrokerUnlinked = await onBrokerUnlinked((brokerId) => {
                logMessage(`Broker ${brokerId} unlinked successfully`, { brokerId });
                // Remove from linkedBrokers
                linkedBrokers.value = linkedBrokers.value.filter(b => b.id !== brokerId);
                // Clear account data for this broker
                globalState.accountsData.accounts = globalState.accountsData.accounts.filter(a => a.brokerId !== brokerId);
                // If no more brokers linked, go back to login
                if (linkedBrokers.value.length === 0) {
                    isLoggedIn.value = false;
                    startupState.value = 'need_link';
                    loginStep.value = 'broker';
                }
                // Trigger refresh of account view
                shouldReloadAccounts.value = true;
            });
            unlistenHandles.push(unlistenBrokerUnlinked);

            // Notify backend that frontend is ready to receive events
            frontendReady();
            console.log("Frontend ready command invoked");

            // Best-effort startup update check.
            // Any failure is intentionally ignored to keep core app behavior unaffected.
            setTimeout(() => {
                checkForUpdates(true);
            }, 2000);
        });

        // Function to check login status
        const checkLoginStatus = async () => {
            try {
                const isAuthenticated = await isLoggedInCmd();
                if (isAuthenticated) {
                    isLoggedIn.value = true;
                    currentEmail.value = await getCurrentEmail();
                    logMessage("Already authenticated");
                    // Also load account list if already logged in on startup
                    await loadAccountList();
                }
            } catch (error) {
                console.error('Error checking login status:', error);
                logMessage('Error checking login status: ' + error.message);
            }
        };

        // Check login status
        checkLoginStatus();

        // Logout
        const logout = async () => {
            try {
                await logoutCmd();
                isLoggedIn.value = false;
                currentEmail.value = '';
                loginStep.value = 'email';
                // Explicitly reset 2FA state
                verificationCode.value = '';
                loginMessage.value = 'Logged out successfully';
                setTimeout(() => { loginMessage.value = ''; }, 3000);
                logMessage('Logged out successfully');
                // Clear account data on logout
                globalState.accountsData.accounts = [];
                globalState.accountsData.accountHoldings = {};
                globalState.accountsData.accountCash = {};
                globalState.accountsData.lastLoaded = null;
            } catch (error) {
                console.error('Error logging out:', error);
                logMessage('Error logging out: ' + error.message);
            }
        };

        // Unlink a specific broker
        const unlinkBroker = (brokerId) => {
            showUnlinkDropdown.value = false;
            logMessage(`Unlinking broker ${brokerId}...`, { brokerId });
            unlinkBrokerCmd(brokerId);
        };

        // Clean up event listeners
        onUnmounted(() => {
            window.removeEventListener('resize', checkOrientation);
            // Call all stored unlisten functions
            unlistenHandles.forEach(fn => fn());
        });

        // Custom minimal context menu (Cut/Copy/Paste) instead of bloated Chromium default
        const editMenu = reactive({
            visible: false,
            x: 0,
            y: 0,
            showCut: false,
            showCopy: false,
            showPaste: false,
            targetEl: null,
            savedText: '',
            selectionStart: null,
            selectionEnd: null
        });

        const copyTextToClipboard = async (text) => {
            if (!text) return;
            try {
                await navigator.clipboard.writeText(text);
                return;
            } catch {
                const temp = document.createElement('textarea');
                temp.value = text;
                temp.style.position = 'fixed';
                temp.style.opacity = '0';
                document.body.appendChild(temp);
                temp.focus();
                temp.select();
                document.execCommand('copy');
                document.body.removeChild(temp);
            }
        };

        const handleContextMenu = (e) => {
            e.preventDefault();

            const tag = e.target?.tagName;
            const isInput = tag === 'INPUT' || tag === 'TEXTAREA';
            let selectedText = '';
            let selectionStart = null;
            let selectionEnd = null;

            if (isInput) {
                const input = e.target;
                if (input.hasAttribute('readonly')) {
                    input.removeAttribute('readonly');
                }
                selectionStart = typeof input.selectionStart === 'number' ? input.selectionStart : 0;
                selectionEnd = typeof input.selectionEnd === 'number' ? input.selectionEnd : selectionStart;
                selectedText = (selectionEnd > selectionStart)
                    ? input.value.substring(selectionStart, selectionEnd)
                    : '';
            } else {
                selectedText = window.getSelection()?.toString() || '';
            }

            // Only show edit menu for inputs or text selections
            if (!isInput && !selectedText) return;

            editMenu.targetEl = e.target;
            editMenu.savedText = selectedText || '';
            editMenu.selectionStart = selectionStart;
            editMenu.selectionEnd = selectionEnd;
            editMenu.x = e.clientX;
            editMenu.y = e.clientY;
            editMenu.showCut = isInput && !!selectedText;
            editMenu.showCopy = !!selectedText;
            editMenu.showPaste = isInput;
            editMenu.visible = true;
        };

        const editMenuAction = async (action) => {
            const el = editMenu.targetEl;
            const isInput = !!el && (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA');
            const start = typeof editMenu.selectionStart === 'number'
                ? editMenu.selectionStart
                : (isInput && typeof el.selectionStart === 'number' ? el.selectionStart : 0);
            const end = typeof editMenu.selectionEnd === 'number'
                ? editMenu.selectionEnd
                : (isInput && typeof el.selectionEnd === 'number' ? el.selectionEnd : start);

            if (action === 'copy') {
                await copyTextToClipboard(editMenu.savedText);
            } else if (action === 'cut') {
                await copyTextToClipboard(editMenu.savedText);
                if (isInput && !el.readOnly && !el.disabled && end > start) {
                    const val = el.value;
                    el.value = val.substring(0, start) + val.substring(end);
                    el.setSelectionRange(start, start);
                    el.dispatchEvent(new Event('input', { bubbles: true }));
                }
            } else if (action === 'paste') {
                try {
                    const text = await navigator.clipboard.readText();
                    if (isInput && !el.disabled) {
                        if (el.readOnly) {
                            el.removeAttribute('readonly');
                        }
                        el.focus();
                        const val = el.value;
                        const nextValue = val.substring(0, start) + text + val.substring(end);
                        const nextPos = start + text.length;
                        el.value = nextValue;
                        el.setSelectionRange(nextPos, nextPos);
                        el.dispatchEvent(new Event('input', { bubbles: true }));
                    }
                } catch {
                    if (el) {
                        el.focus();
                    }
                    document.execCommand('paste');
                }
            }
            editMenu.visible = false;
        };

        const hideEditMenu = () => {
            editMenu.visible = false;
        };

        // Close edit menu on click anywhere
        const onDocClick = () => { hideEditMenu(); };
        onMounted(() => { document.addEventListener('click', onDocClick); });
        onUnmounted(() => { document.removeEventListener('click', onDocClick); });

        // Handle trade-ticker event from AccountView (right-click sell/buy)
        const handleTradeTicker = ({ ticker, side: orderSide }) => {
            tickerInput456.value = ticker;
            side.value = orderSide;
            sharesInput.value = 1;

            if (orderSide === 'sell') {
                // Auto-activate "Max" mode for sells initiated from the holdings list
                maxSharesByBroker.value = true;

                // Auto-select only brokers/accounts that actually hold this ticker
                const allAccounts = globalState.accountsData?.accounts || [];
                const holdingsMap = globalState.accountsData?.accountHoldings || {};
                const tickerUpper = (ticker || '').toUpperCase();

                const accountsHoldingTicker = new Set();
                const brokersHoldingTicker = new Set();

                allAccounts.forEach(acc => {
                    if (acc.status !== 'APPROVED') return;
                    const holdings = holdingsMap[acc.id] || [];
                    const hasTicker = holdings.some(h =>
                        h && h.ticker && String(h.ticker).toUpperCase() === tickerUpper && Number(h.shares) > 0
                    );
                    if (hasTicker) {
                        accountsHoldingTicker.add(acc.id);
                        if (acc.brokerId) brokersHoldingTicker.add(acc.brokerId);
                    }
                });

                connectedBrokers.value.forEach(broker => {
                    selectedBrokers.value[broker.id] = brokersHoldingTicker.has(broker.id);
                });
                allAccounts.forEach(acc => {
                    if (acc.status === 'APPROVED') {
                        selectedAccounts.value[acc.id] = accountsHoldingTicker.has(acc.id);
                    }
                });
            } else {
                maxSharesByBroker.value = null;
            }

            activeTab.value = 'orders';
        };

        // Refresh accounts - loads data in background so it's ready immediately
        const refreshAccounts = async () => {
            logMessage("Refreshing account data...");
            // Reload the basic account list first
            await loadAccountList();

            // Load full details (holdings + cash) in the background
            await loadAccountDetailsBackground();
            logMessage("Account data refreshed.");
        };

        // Manual refresh from the Accounts tab - triggers AccountView's own reload with loading indicators
        const refreshAccountsManual = async () => {
            logMessage("Refreshing account data...");
            await loadAccountList();
            shouldReloadAccounts.value = true;
        };

        // Watch for tab changes to reset the reload flag when switching to accounts tab
        watch(activeTab, (newTab, oldTab) => {
            if (newTab === 'accounts') {
                console.log("Switching to accounts tab, shouldReloadAccounts:", shouldReloadAccounts.value);
            } else if (newTab === 'orders' && oldTab === 'accounts') {
                // Remember that we've visited the accounts tab
                // console.log("Switching from accounts to orders tab");
                // window.visitedAccountsTab = true; // This state seems unused now
            }
        });

        return {
            emailInput123,
            tickerInput456,
            side,
            sharesInput,
            logs,
            tfaCode,
            randomId,
            tfaInput,
            isLandscape,
            logMessage,
            clearLog,
            placeOrder,
            fillMaxShares,
            clearMaxShares,
            maxSharesByBroker,
            verify2FA,
            cancelLogin,
            handleFocus,
            handleBlur,
            disableAutocomplete,
            preventAutofill,
            formatTimestamp,
            activeTab,
            isLoggedIn,
            currentEmail,
            loginStep,
            loginMessage,
            initiateLogin,
            logout,
            accountViewRef,
            refreshAccounts,
            refreshAccountsManual,
            handleTradeTicker,
            handleContextMenu,
            editMenu,
            editMenuAction,
            shouldReloadAccounts,
            orderCountdown,
            verificationCode,
            verificationInput,
            isTransitioning,
            isTransitioningTo2FA,
            accountsForDropdown,
            startupState,
            linkedBrokers,
            sortedLinkedBrokers,
            activeBroker,
            selectedBrokerType,
            showLinkBrokerModal,
            modalSelectedBroker,
            proceedWithBroker,
            startLinkingBroker,
            selectAndProceedBroker,
            selectAndLinkBroker,
            goBackFromLogin,
            connectedBrokers,
            apiKeyInput,
            loginWithApiKey,
            passwordInput,
            loginWithPassword,
            webullAppKey,
            webullAppSecret,
            loginWithWebull,
            lhApiKey,
            lhApiSecret,
            lhAccountId,
            loginWithLighthorse,
            tastyClientSecret,
            tastyRefreshToken,
            loginWithTastytrade,
            showUnlinkDropdown,
            updateStatus,
            isCheckingForUpdates,
            isInstallingUpdate,
            checkForUpdates,
            unlinkBroker,
            startReauthBroker,
            getBrokerBadgeStyle,
            getBrokerColor,
            brokerColors,
            // New broker/account selection
            selectedBrokers,
            selectedAccounts,
            getBrokerAccounts,
            toggleBroker,
            toggleAccount,
            selectAllBrokers,
            selectNoneBrokers,
            // Legacy migration
            legacyImporting,
            legacyImportError,
            handleLegacyImport,
            handleLegacySkip
        };
    },
};
</script>

<style>
html,
body {
    margin: 0;
    padding: 0;
    height: 100%;
    width: 100%;
    overflow: hidden;
    font-size: 15px;
}

#app {
    height: 100%;
    width: 100%;
    display: flex;
    flex-direction: column;
}
</style>

<style scoped>
/* Minimal edit context menu */
.edit-context-menu {
    position: fixed;
    z-index: 10000;
    background-color: var(--bg-secondary, #1a1a1a);
    border: 1px solid var(--border-hover, rgba(255, 255, 255, 0.15));
    border-radius: var(--radius-sm, 6px);
    padding: 4px 0;
    min-width: 120px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.5);
}

.edit-menu-item {
    padding: 8px 16px;
    font-size: 13px;
    color: var(--text-primary, #e8e8e8);
    cursor: pointer;
    transition: background-color 0.1s ease;
}

.edit-menu-item:hover {
    background-color: var(--accent-muted, rgba(124, 138, 255, 0.15));
    color: var(--accent-primary, #7c8aff);
}

.app-container {
    display: flex;
    flex-direction: column;
    height: 100vh;
    min-height: 100%;
    width: 100%;
    background-color: var(--bg-primary, #0f0f0f);
    color: var(--text-primary, #e8e8e8);
    font-family: 'Inter', -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
    overflow: hidden;
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
}

.app-content {
    flex: 1;
    padding: 24px;
    overflow-y: auto;
    overflow-x: hidden;
    display: flex;
    flex-direction: column;
    gap: 24px;
    margin: 0 auto;
    width: 100%;
    max-width: 1200px;
    min-width: 0;
    box-sizing: border-box;
    min-height: 0;
}

/* Make the logged-in wrapper a flex column so children lay out properly */
.app-content > div {
    display: flex;
    flex-direction: column;
    flex: 1 0 auto;
}

.main-section {
    display: flex;
    flex-direction: column;
    gap: 24px;
    width: 100%;
    flex: 1 0 auto;
}

.landscape-layout .main-section {
    flex-direction: column;
    align-items: stretch;
    width: 100%;
    min-height: 0;
}

.landscape-layout .config-section,
.landscape-layout .log-section {
    min-width: 0;
    max-width: none;
    display: flex;
    flex-direction: column;
    min-height: 0;
}

.landscape-layout .tab-content {
    display: flex;
    flex-direction: column;
    width: 100%;
}

.app-section {
    display: flex;
    flex-direction: column;
    gap: 16px;
    background-color: var(--bg-secondary, #1c1c21);
    border: 1px solid var(--border-color, rgba(255, 255, 255, 0.1));
    border-radius: var(--radius-md, 10px);
    padding: 24px;
    width: 100%;
    position: relative;
    margin-top: 15px;
    box-sizing: border-box;
    min-width: 0;
}

.app-section.config-section {
    flex: 1 0 auto;
}

.app-section.log-section {
    flex: 1 0 0;
    min-height: 250px;
}

.log-inner {
    flex: 1 1 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
}

.app-section.account-section {
    flex: 1;
    min-height: 0;
}

.section-title-container {
    position: absolute;
    top: -12px;
    left: 20px;
    background-color: var(--bg-secondary, #1a1a1a);
    padding: 0 10px;
    z-index: 1;
    display: flex;
    align-items: center;
    gap: 10px;
}

.section-title {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-secondary, #999);
    text-transform: uppercase;
    letter-spacing: 0.5px;
}

.section-header {
    display: none;
}

.input-group {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-bottom: 16px;
    width: 100%;
    max-width: 100%;
    padding: 16px;
    border-radius: var(--radius-sm, 6px);
    background-color: var(--bg-tertiary, #242424);
    border: none;
    box-sizing: border-box;
}

.input-group label {
    font-size: 13px;
    color: var(--text-secondary, #999);
    margin-bottom: 4px;
    font-weight: 500;
}

.shares-input-row {
    display: flex;
    gap: 8px;
    align-items: stretch;
}

.shares-input-row input {
    flex: 1;
}

.max-shares-display {
    flex: 1;
    background-color: var(--bg-tertiary, #242424);
    border: 1px solid var(--accent-primary, #7c8aff);
    border-radius: var(--radius-sm, 6px);
    padding: 8px 12px;
    color: var(--accent-primary, #7c8aff);
    font-size: 14px;
    display: flex;
    align-items: center;
    cursor: pointer;
    font-weight: 500;
}

.max-shares-display:hover {
    background-color: var(--accent-muted, rgba(124, 138, 255, 0.15));
}

.max-shares-btn {
    padding: 6px 12px !important;
    font-size: 12px !important;
    white-space: nowrap;
    min-width: auto !important;
}

.input-hint {
    font-size: 12px;
    color: var(--text-muted, #666);
    margin-top: 6px;
    margin-bottom: 0;
}

.input-hint a {
    color: var(--text-secondary, #aaa);
    text-decoration: underline;
}

.input-hint a:hover {
    color: var(--text-primary, #e8e8e8);
}

input[type="text"] {
    background-color: var(--bg-tertiary, #242424);
    color: var(--text-primary, #e8e8e8);
    border: 1px solid var(--border-color, rgba(255, 255, 255, 0.08));
    border-radius: var(--radius-sm, 6px);
    padding: 10px 12px;
    font-size: 14px;
    font-family: inherit;
    transition: border-color 0.2s ease;
    width: 100%;
    min-width: 0;
}

input[type="text"]:focus {
    border-color: var(--accent-primary, #7c8aff);
    outline: none;
    box-shadow: 0 0 0 3px var(--accent-muted, rgba(124, 138, 255, 0.15));
}

input[type="password"] {
    background-color: var(--bg-tertiary, #242424);
    color: var(--text-primary, #e8e8e8);
    border: 1px solid var(--border-color, rgba(255, 255, 255, 0.08));
    border-radius: var(--radius-sm, 6px);
    padding: 10px 12px;
    font-size: 14px;
    font-family: inherit;
    transition: border-color 0.2s ease;
    width: 100%;
    min-width: 0;
    box-sizing: border-box;
}

input[type="password"]:focus {
    border-color: var(--accent-primary, #7c8aff);
    outline: none;
    box-shadow: 0 0 0 3px var(--accent-muted, rgba(124, 138, 255, 0.15));
}

/* Robinhood prompt approval screen */
.prompt-approval-container {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 16px;
    padding: 24px 0;
}

.prompt-approval-icon {
    opacity: 0.9;
}

.prompt-approval-text {
    font-size: 15px;
    color: var(--text-primary, #e8e8e8);
    text-align: center;
    line-height: 1.5;
    max-width: 280px;
}

.prompt-approval-text strong {
    color: var(--accent-primary, #7c8aff);
}

.prompt-approval-hint {
    font-size: 13px;
    color: var(--text-muted, #666);
}

.prompt-approval-spinner {
    width: 28px;
    height: 28px;
    border: 3px solid var(--bg-tertiary, #242424);
    border-top: 3px solid var(--accent-primary, #7c8aff);
    border-radius: 50%;
    animation: spin 1s linear infinite;
}

@keyframes spin {
    to { transform: rotate(360deg); }
}

.toggle-container {
    display: flex;
    gap: 4px;
    width: fit-content;
    background-color: var(--bg-tertiary, #242424);
    border-radius: var(--radius-md, 10px);
    padding: 3px;
}

.toggle-option {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 8px 20px;
    background-color: transparent;
    border: none;
    border-radius: 8px;
    cursor: pointer;
    font-size: 14px;
    transition: all 0.2s ease;
    min-width: 80px;
    color: var(--text-secondary, #999);
}

.toggle-option.active {
    background-color: var(--accent-primary, #7c8aff);
    color: #fff;
}

.toggle-option input {
    display: none;
}

.action-buttons {
    display: flex;
    justify-content: center;
    margin-top: 16px;
    width: 100%;
}

.btn {
    padding: 9px 18px;
    font-size: 14px;
    border-radius: var(--radius-sm, 6px);
    cursor: pointer;
    transition: all 0.15s ease;
    border: none;
    font-weight: 500;
    white-space: nowrap;
    font-family: inherit;
}

.wide-btn {
    min-width: 200px;
    padding: 11px 24px;
}

.btn.primary {
    background-color: var(--accent-primary, #7c8aff);
    color: #fff;
}

.btn.primary:hover {
    background-color: var(--accent-hover, #9ba6ff);
}

.btn.secondary {
    background-color: transparent;
    border: 1px solid var(--border-hover, rgba(255, 255, 255, 0.15));
    color: var(--text-secondary, #999);
}

.btn.secondary:hover {
    border-color: var(--accent-primary, #7c8aff);
    color: var(--text-primary, #e8e8e8);
}

.btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
}

.btn:disabled:hover {
    background-color: var(--accent-primary, #7c8aff);
    color: #fff;
}

.btn.secondary:disabled:hover {
    background-color: transparent;
    border-color: var(--border-hover, rgba(255, 255, 255, 0.15));
    color: var(--text-secondary, #999);
}

.log-container {
    background-color: var(--bg-secondary, #1a1a1a);
    border: 1px solid var(--border-color, rgba(255, 255, 255, 0.08));
    border-radius: var(--radius-md, 10px);
    padding: 16px;
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overflow-x: hidden;
    font-size: 13px;
    line-height: 1.6;
    font-family: inherit;
    width: 100%;
    scrollbar-width: thin;
    scrollbar-color: rgba(255, 255, 255, 0.12) transparent;
}

.log-container::-webkit-scrollbar {
    width: 6px;
}

.log-container::-webkit-scrollbar-track {
    background: transparent;
}

.log-container::-webkit-scrollbar-thumb {
    background-color: rgba(255, 255, 255, 0.12);
    border-radius: 3px;
}

.log-container::-webkit-scrollbar-thumb:hover {
    background-color: rgba(255, 255, 255, 0.2);
}

.log-entry {
    margin-bottom: 4px;
    display: flex;
    gap: 10px;
    padding: 6px 8px;
    border-radius: var(--radius-sm, 6px);
}

.log-entry:hover {
    background-color: var(--bg-tertiary, #242424);
}

.log-entry:last-child {
    margin-bottom: 0;
}

.log-broker-badge {
    padding: 2px 8px;
    border-radius: 20px;
    font-size: 10px;
    font-weight: 600;
    flex-shrink: 0;
    text-transform: uppercase;
    letter-spacing: 0.3px;
}

.timestamp {
    color: var(--text-muted, #666);
    font-size: 12px;
    flex-shrink: 0;
    font-weight: 400;
}

.message {
    color: var(--text-primary, #e8e8e8);
    word-break: break-word;
    overflow-wrap: break-word;
    line-height: 1.4;
    min-width: 0;
}

.log-actions {
    display: flex;
    justify-content: flex-start;
    margin-bottom: 12px;
}

/* Wide screen: place orders + log side by side when window is wide enough */
@media screen and (min-width: 900px) {
    .orders-container {
        flex-direction: row !important;
        align-items: stretch;
    }

    .orders-container .config-section,
    .orders-container .log-section {
        flex: 1;
        min-width: 0;
        max-width: none;
        width: auto;
    }

    .orders-container .log-container {
        flex: 1;
        min-height: 0;
    }

    .orders-container .input-group {
        max-width: 100%;
    }
}

.login-screen {
    display: flex;
    flex-direction: column;
    width: 100%;
    height: 100%;
    gap: 24px;
    align-items: center;
    justify-content: center;
}

.login-container {
    background-color: var(--bg-secondary, #1a1a1a);
    border: 1px solid var(--border-color, rgba(255, 255, 255, 0.08));
    border-radius: var(--radius-lg, 14px);
    padding: 40px;
    width: 100%;
    max-width: 440px;
    margin: 0 auto;
    position: relative;
}

.login-header {
    text-align: center;
    margin-bottom: 32px;
}

.login-header h1 {
    color: var(--text-primary, #e8e8e8);
    font-size: 22px;
    font-weight: 600;
    margin-bottom: 8px;
    letter-spacing: -0.01em;
}

.login-header p {
    display: none;
}

.login-form {
    display: flex;
    flex-direction: column;
    gap: 20px;
}

.login-actions {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
    margin-top: 8px;
}

.login-status-message {
    margin-top: 20px;
    padding: 12px 16px;
    text-align: center;
    color: var(--text-primary, #e8e8e8);
    font-size: 14px;
    background-color: var(--accent-muted, rgba(124, 138, 255, 0.15));
    border-radius: var(--radius-sm, 6px);
    border-left: 3px solid var(--accent-primary, #7c8aff);
}

.landscape-layout .login-screen {
    flex-direction: row;
    align-items: flex-start;
    justify-content: center;
    padding-top: 32px;
}

.landscape-layout .login-container {
    margin: 0;
    margin-right: 24px;
}

@media screen and (max-width: 768px) {
    .landscape-layout .login-screen {
        flex-direction: column;
        align-items: center;
    }

    .landscape-layout .login-container {
        margin: 0 auto;
        margin-bottom: 24px;
    }
}

.account-section {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
}

.landscape-layout .account-section {
    width: 100%;
    max-width: 100%;
}

.tab-content {
    display: flex;
    flex-direction: column;
    min-width: 0;
    flex: 1 0 auto;
}

.landscape-layout .app-content {
    max-width: 95%;
    width: 95%;
}

.orders-container {
    display: flex;
    flex-direction: column;
    gap: 24px;
    width: 100%;
    min-width: 0;
    flex: 1 0 auto;
}

.landscape-layout .orders-container {
    flex-direction: row;
    width: 100%;
    align-items: stretch;
}

.landscape-layout .orders-container .config-section,
.landscape-layout .orders-container .log-section {
    flex: 1;
    min-width: 0;
    max-width: none;
    width: auto;
}

.landscape-layout .orders-container .log-container {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
}

.log-section {
    display: flex;
    flex-direction: column;
    min-height: 0;
    flex: 1;
}

.verification-input {
    font-size: 1.2rem;
    letter-spacing: 4px;
    text-align: center;
    padding: 12px;
    border: 1px solid var(--border-color, rgba(255, 255, 255, 0.08));
    border-radius: var(--radius-sm, 6px);
    width: 100%;
    max-width: 250px;
    margin: 20px auto;
    display: block;
    background-color: var(--bg-tertiary, #242424);
    color: var(--text-primary, #e8e8e8);
    font-family: inherit;
    transition: border-color 0.2s ease;
}

.verification-input:focus {
    border-color: var(--accent-primary, #7c8aff);
    outline: none;
    box-shadow: 0 0 0 3px var(--accent-muted, rgba(124, 138, 255, 0.15));
}

.tfa-buttons {
    display: flex;
    flex-direction: column;
    gap: 12px;
    width: 100%;
    max-width: 250px;
    margin: 0 auto;
}

.login-form h2 {
    color: var(--text-primary, #e8e8e8);
    text-align: center;
    margin-bottom: 10px;
    font-size: 1.3rem;
    font-weight: 600;
}

.login-form p {
    text-align: center;
    margin-bottom: 20px;
    color: var(--text-secondary, #999);
    font-size: 0.9rem;
}

.error-message {
    color: var(--error-color, #f87171);
    text-align: center;
    margin-top: 15px;
    font-size: 0.9rem;
    background-color: rgba(248, 113, 113, 0.1);
    border-radius: var(--radius-sm, 6px);
    padding: 10px 12px;
    border-left: 3px solid var(--error-color, #f87171);
}

.app-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 12px 16px;
    background-color: var(--bg-secondary, #1a1a1a);
    border-radius: var(--radius-md, 10px);
    border: 1px solid var(--border-color, rgba(255, 255, 255, 0.08));
    margin-bottom: 16px;
}

.app-tabs {
    display: flex;
    gap: 4px;
}

.tab-button {
    background-color: transparent;
    border: none;
    border-radius: var(--radius-sm, 6px);
    padding: 8px 16px;
    color: var(--text-secondary, #999);
    cursor: pointer;
    font-size: 14px;
    font-weight: 500;
    transition: all 0.15s ease;
}

.tab-button:hover {
    color: var(--text-primary, #e8e8e8);
    background-color: var(--bg-tertiary, #242424);
}

.tab-button.active {
    background-color: var(--accent-muted, rgba(124, 138, 255, 0.15));
    color: var(--accent-primary, #7c8aff);
    font-weight: 600;
}

.auth-status {
    display: flex;
    align-items: center;
}

.logged-in-status, .logged-out-status {
    display: flex;
    align-items: center;
    gap: 8px;
}

.update-status {
    font-size: 12px;
    color: var(--text-secondary, #999);
}

.status-indicator {
    width: 8px;
    height: 8px;
    border-radius: 50%;
}

.status-indicator.online {
    background-color: var(--success-color, #4ade80);
}

.status-indicator.offline {
    background-color: var(--error-color, #f87171);
}

.status-text {
    font-size: 13px;
    color: var(--text-secondary, #999);
}

.small-btn {
    padding: 4px 8px;
    font-size: 12px;
}

.landscape-layout .log-section {
    width: auto;
    min-width: 0;
}

.landscape-layout .tab-content:has(.config-section) {
    flex-direction: row;
    justify-content: space-between;
    gap: 24px;
}

.landscape-layout .tab-content {
    display: flex;
    width: 100%;
}

.landscape-layout div[class="tab-content"] {
    flex-direction: column;
    width: 100%;
}

input[type="number"] {
    background-color: var(--bg-tertiary, #242424);
    color: var(--text-primary, #e8e8e8);
    border: 1px solid var(--border-color, rgba(255, 255, 255, 0.08));
    border-radius: var(--radius-sm, 6px);
    padding: 10px 12px;
    font-size: 14px;
    font-family: inherit;
    transition: border-color 0.2s ease;
    width: 100%;
    min-width: 0;
    box-sizing: border-box;
}

input[type="number"]:focus {
    border-color: var(--accent-primary, #7c8aff);
    outline: none;
    box-shadow: 0 0 0 3px var(--accent-muted, rgba(124, 138, 255, 0.15));
}

select {
    background-color: var(--bg-tertiary, #242424);
    color: var(--text-primary, #e8e8e8);
    border: 1px solid var(--border-color, rgba(255, 255, 255, 0.08));
    border-radius: var(--radius-sm, 6px);
    padding: 10px 12px;
    font-size: 14px;
    font-family: inherit;
    transition: border-color 0.2s ease;
    width: 100%;
    min-width: 0;
    box-sizing: border-box;
    appearance: none;
    -webkit-appearance: none;
    -moz-appearance: none;
    background-image: url('data:image/svg+xml;charset=US-ASCII,%3Csvg%20xmlns%3D%22http%3A%2F%2Fwww.w3.org%2F2000%2Fsvg%22%20width%3D%22292.4%22%20height%3D%22292.4%22%3E%3Cpath%20fill%3D%22%23666666%22%20d%3D%22M287%2069.4a17.6%2017.6%200%200%200-13-5.4H18.4c-5%200-9.3%201.8-12.9%205.4A17.6%2017.6%200%200%200%200%2082.2c0%205%201.8%209.3%205.4%2012.9l128%20127.9c3.6%203.6%207.8%205.4%2012.8%205.4s9.2-1.8%2012.8-5.4L287%2095c3.5-3.5%205.4-7.8%205.4-12.8%200-5-1.9-9.2-5.5-12.8z%22%2F%3E%3C%2Fsvg%3E');
    background-repeat: no-repeat;
    background-position: right 12px center;
    background-size: 10px 10px;
}

select:focus {
    border-color: var(--accent-primary, #7c8aff);
    outline: none;
    box-shadow: 0 0 0 3px var(--accent-muted, rgba(124, 138, 255, 0.15));
}

select option {
    background-color: var(--bg-tertiary, #242424);
    color: var(--text-primary, #e8e8e8);
}

/* Broker/Account Selection */
.broker-account-selection {
    padding: 12px 15px;
}

.broker-selection-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 10px;
    flex-wrap: wrap;
    gap: 6px;
}

.broker-selection-header label {
    margin-bottom: 0;
}

.selection-buttons {
    display: flex;
    gap: 8px;
}

.selection-btn {
    background-color: transparent;
    border: 1px solid var(--border-color, rgba(255, 255, 255, 0.08));
    color: var(--text-secondary, #999);
    padding: 4px 10px;
    border-radius: var(--radius-sm, 6px);
    cursor: pointer;
    font-size: 11px;
    transition: all 0.15s ease;
}

.selection-btn:hover {
    background-color: var(--bg-tertiary, #242424);
    border-color: var(--border-hover, rgba(255, 255, 255, 0.15));
    color: var(--text-primary, #e8e8e8);
}

.broker-selection-list {
    display: flex;
    flex-direction: column;
    gap: 10px;
}

.broker-selection-item {
    background-color: var(--bg-tertiary, #242424);
    border-radius: var(--radius-sm, 6px);
    padding: 10px 12px;
}

.broker-checkbox-row {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
    flex-wrap: wrap;
}

.checkbox-label {
    display: flex;
    align-items: center;
    gap: 8px;
    cursor: pointer;
    font-weight: 600;
    font-size: 13px;
}

.checkbox-label input[type="checkbox"] {
    width: 16px;
    height: 16px;
    accent-color: var(--accent-primary, #7c8aff);
    cursor: pointer;
}

.broker-name {
    user-select: none;
}

.account-selection {
    flex: 1;
    min-width: 0;
}

.account-checkboxes {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
}

.account-checkbox-label {
    display: flex;
    align-items: center;
    gap: 6px;
    cursor: pointer;
    font-size: 12px;
    color: var(--text-secondary, #999);
    background-color: var(--bg-secondary, #1a1a1a);
    padding: 4px 10px;
    border-radius: var(--radius-sm, 6px);
    border: 1px solid var(--border-color, rgba(255, 255, 255, 0.08));
    transition: all 0.15s ease;
}

.account-checkbox-label:hover {
    border-color: var(--accent-primary, #7c8aff);
}

.account-checkbox-label input[type="checkbox"] {
    width: 14px;
    height: 14px;
    accent-color: var(--accent-primary, #7c8aff);
    cursor: pointer;
}

.single-account-indicator {
    font-size: 12px;
    color: var(--text-muted, #666);
    font-style: italic;
}

.no-brokers-message {
    color: var(--text-muted, #666);
    font-size: 13px;
    text-align: center;
    padding: 12px;
}

.loading-text {
    display: block !important;
    color: var(--text-secondary, #999);
    font-size: 14px;
    animation: pulse 1.5s ease-in-out infinite;
}

@keyframes pulse {
    0%, 100% { opacity: 0.5; }
    50% { opacity: 1; }
}

.broker-type-indicator {
    display: block !important;
    color: var(--accent-primary, #7c8aff);
    font-size: 13px;
    margin-top: 8px;
    opacity: 0.8;
}

/* Broker badges in header */
.linked-brokers-list {
    display: flex;
    gap: 6px;
    margin-right: 12px;
}

.broker-badge {
    padding: 4px 10px;
    border-radius: 20px;
    font-size: 11px;
    font-weight: 600;
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: 4px;
}

.broker-badge.clickable {
    cursor: pointer;
    transition: transform 0.15s ease, box-shadow 0.15s ease;
}

.broker-badge.clickable:hover {
    transform: translateY(-1px);
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.3);
}

.broker-badge.disconnected {
    animation: reauth-glow 2s ease-in-out infinite;
}

@keyframes reauth-glow {
    0%, 100% { box-shadow: 0 0 8px rgba(248, 113, 113, 0.4); }
    50% { box-shadow: 0 0 16px rgba(248, 113, 113, 0.6); }
}

.broker-badge .reauth-icon {
    font-weight: bold;
    font-size: 10px;
}

/* Unlink dropdown */
.unlink-dropdown {
    position: relative;
    display: inline-block;
}

.dropdown-menu {
    position: absolute;
    top: 100%;
    right: 0;
    background-color: var(--bg-elevated, #2a2a2a);
    border: 1px solid var(--border-hover, rgba(255, 255, 255, 0.15));
    border-radius: var(--radius-md, 10px);
    min-width: 200px;
    z-index: 1000;
    margin-top: 4px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
    overflow: hidden;
}

.dropdown-item {
    padding: 10px 14px;
    cursor: pointer;
    color: var(--text-primary, #e8e8e8);
    font-size: 13px;
    transition: background-color 0.15s;
}

.dropdown-item:hover {
    background-color: rgba(248, 113, 113, 0.1);
    color: var(--error-color, #f87171);
}

/* Legacy credential import */
.legacy-description {
    text-align: center;
    color: var(--text-secondary, #999);
    margin-bottom: 20px;
    line-height: 1.5;
}

.legacy-error {
    background: rgba(239, 68, 68, 0.1);
    border: 1px solid rgba(239, 68, 68, 0.3);
    border-radius: var(--radius-md, 10px);
    padding: 12px 16px;
    margin-bottom: 16px;
    color: #ef4444;
    font-size: 13px;
}

.legacy-actions {
    display: flex;
    gap: 12px;
}

.legacy-actions .btn {
    flex: 1;
}

.legacy-spinner-container {
    display: flex;
    justify-content: center;
    align-items: center;
    padding: 24px 0;
}

.legacy-spinner {
    width: 32px;
    height: 32px;
    border: 3px solid var(--border-color, rgba(255, 255, 255, 0.08));
    border-top-color: var(--accent-color, #4a9eff);
    border-radius: 50%;
    animation: legacy-spin 0.8s linear infinite;
}

@keyframes legacy-spin {
    to { transform: rotate(360deg); }
}

/* Broker selection styles */
.broker-select-subtitle {
    text-align: center;
    color: var(--text-secondary, #999);
    margin-bottom: 16px;
}

.broker-options {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin-bottom: 20px;
}

.broker-option {
    background-color: var(--bg-tertiary, #242424);
    border: 1px solid var(--border-color, rgba(255, 255, 255, 0.08));
    border-radius: var(--radius-md, 10px);
    padding: 16px;
    cursor: pointer;
    transition: all 0.15s ease;
}

.broker-option:hover:not(.disabled) {
    border-color: var(--border-hover, rgba(255, 255, 255, 0.15));
    background-color: var(--bg-elevated, #2a2a2a);
}

.broker-option.clickable {
    cursor: pointer;
}

.broker-option.clickable:hover {
    border-color: var(--accent-primary, #7c8aff);
    background-color: var(--accent-muted, rgba(124, 138, 255, 0.15));
    transform: translateY(-1px);
}

.broker-option.selected {
    border-color: var(--accent-primary, #7c8aff);
    background-color: var(--accent-muted, rgba(124, 138, 255, 0.15));
}

.broker-option.disabled {
    opacity: 0.4;
    cursor: not-allowed;
}

.broker-option-name {
    font-size: 15px;
    font-weight: 600;
    color: var(--text-primary, #e8e8e8);
    margin-bottom: 4px;
}

.broker-option-desc {
    font-size: 12px;
    color: var(--text-muted, #666);
}

/* Modal styles */
.modal-overlay {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    background-color: rgba(0, 0, 0, 0.6);
    backdrop-filter: blur(4px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
}

.modal-content {
    background-color: var(--bg-secondary, #1a1a1a);
    border: 1px solid var(--border-color, rgba(255, 255, 255, 0.08));
    border-radius: var(--radius-lg, 14px);
    padding: 28px;
    width: 90%;
    max-width: 400px;
    box-shadow: 0 16px 48px rgba(0, 0, 0, 0.5);
}

.modal-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 20px;
}

.modal-header h2 {
    color: var(--text-primary, #e8e8e8);
    font-size: 18px;
    font-weight: 600;
    margin: 0;
}

.modal-close {
    background: none;
    border: none;
    color: var(--text-muted, #666);
    font-size: 24px;
    cursor: pointer;
    padding: 0;
    line-height: 1;
    transition: color 0.15s;
}

.modal-close:hover {
    color: var(--text-primary, #e8e8e8);
}

.modal-actions {
    display: flex;
    justify-content: center;
    margin-top: 16px;
}
</style>
