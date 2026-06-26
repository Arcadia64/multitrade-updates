<template>
  <div class="account-view">
    <div v-if="!isLoggedIn" class="login-required">
      <p>Please log in to view your accounts.</p>
    </div>

    <div v-else-if="initialLoading" class="loading-page">
      <p>Loading account information...</p>
    </div>

    <div v-else-if="error && !anyAccountLoaded" class="error">
      <p>{{ error }}</p>
      <button class="btn secondary" @click="loadAllAccountDetails">Retry</button>
    </div>

    <div v-else>
      <div v-if="accounts.length === 0" class="no-accounts">
        <p>No accounts found.</p>
        <button class="btn secondary" @click="loadAllAccountDetails">Retry</button>
      </div>

      <div v-else class="accounts-overview">
        <!-- Breadcrumb Navigation -->
        <div v-if="selectedBroker || selectedAccount" class="breadcrumb">
          <span class="breadcrumb-link" @click="goToOverview">All Brokers</span>
          <span v-if="selectedBroker" class="breadcrumb-separator">&gt;</span>
          <span v-if="selectedBroker && !selectedAccount" class="breadcrumb-current">{{ selectedBroker.type }}</span>
          <span v-if="selectedBroker && selectedAccount" class="breadcrumb-link" @click="goToBroker(selectedBroker)">{{ selectedBroker.type }}</span>
          <span v-if="selectedAccount" class="breadcrumb-separator">&gt;</span>
          <span v-if="selectedAccount" class="breadcrumb-current">{{ selectedAccount.name || 'Account' }}</span>
        </div>

        <!-- Level 1: All Brokers Summary -->
        <div v-if="!selectedBroker && !selectedAccount" class="summary-view">
          <div class="accounts-header">
            <h3>Portfolio Overview</h3>
            <div class="portfolio-summary">
              <div class="summary-item">
                <span class="label">Total Value:</span>
                <span class="value highlight">${{ formatNumber(totalPortfolioValue) }}</span>
              </div>
              <div class="summary-item">
                <span class="label">Total Cash:</span>
                <span class="value">${{ formatNumber(totalCashBalance) }}</span>
              </div>
              <div class="summary-item">
                <span class="label">Total Investments:</span>
                <span class="value">${{ formatNumber(totalInvestmentsValue) }}</span>
              </div>
            </div>
          </div>

          <!-- Broker Cards -->
          <div class="broker-cards">
            <div
              v-for="broker in linkedBrokers"
              :key="broker.id"
              class="broker-card"
              :class="{ 'clickable': broker.status === 'connected', 'loading': brokerLoading[broker.id] }"
              @click="broker.status === 'connected' && selectBroker(broker)"
            >
              <div class="broker-card-header">
                <div class="broker-info">
                  <span
                    class="broker-type-badge"
                    :style="{
                      backgroundColor: getBrokerColor(broker.type).primary,
                      color: '#121212'
                    }"
                  >{{ broker.type }}</span>
                  <span class="broker-email">{{ broker.email }}</span>
                </div>
                <span class="broker-status" :class="broker.status">
                  <span v-if="brokerLoading[broker.id]" class="loading-spinner"></span>
                  {{ brokerLoading[broker.id] ? 'loading...' : broker.status }}
                </span>
              </div>
              <div class="broker-card-body">
                <div class="broker-stat">
                  <span class="label">Accounts:</span>
                  <span class="value">
                    <span v-if="brokerLoading[broker.id]" class="loading-text">...</span>
                    <span v-else>{{ getBrokerAccountCount(broker) }}</span>
                  </span>
                </div>
                <div class="broker-stat">
                  <span class="label">Total Value:</span>
                  <span class="value">
                    <span v-if="brokerLoading[broker.id]" class="loading-text">...</span>
                    <span v-else>${{ formatNumber(getBrokerTotalValue(broker)) }}</span>
                  </span>
                </div>
              </div>
              <div v-if="broker.status === 'connected'" class="broker-card-footer">
                <span v-if="brokerLoading[broker.id]" class="loading-message">Loading account data...</span>
                <span v-else class="view-details">View Details &rarr;</span>
              </div>
              <div v-else class="broker-card-footer needs-auth">
                <span class="reauth-message">Requires re-authentication</span>
              </div>
            </div>
          </div>

          <!-- Aggregated Holdings Summary -->
          <div v-if="aggregatedHoldings.length > 0" class="holdings-summary-section">
            <h4>All Holdings</h4>
            <div class="holdings-filter">
              <input
                type="text"
                v-model="holdingsFilter"
                placeholder="Filter by ticker or company name..."
                class="filter-input"
              />
              <button
                v-if="holdingsFilter"
                class="filter-clear"
                @click="holdingsFilter = ''"
                title="Clear filter"
              >&times;</button>
            </div>
            <div v-if="filteredAggregatedHoldings.length === 0" class="no-holdings">
              <p>No holdings match "{{ holdingsFilter }}".</p>
            </div>
            <div v-else class="holdings-table">
              <table>
                <thead>
                  <tr>
                    <th class="sortable" @click="toggleSort(aggregatedSort, 'ticker')">Ticker <span class="sort-icon">{{ aggregatedSort.column === 'ticker' ? (aggregatedSort.direction === 'asc' ? '\u25B2' : '\u25BC') : '\u25B4' }}</span></th>
                    <th class="sortable" @click="toggleSort(aggregatedSort, 'name')">Name <span class="sort-icon">{{ aggregatedSort.column === 'name' ? (aggregatedSort.direction === 'asc' ? '\u25B2' : '\u25BC') : '\u25B4' }}</span></th>
                    <th class="sortable" @click="toggleSort(aggregatedSort, 'brokers')">Brokers <span class="sort-icon">{{ aggregatedSort.column === 'brokers' ? (aggregatedSort.direction === 'asc' ? '\u25B2' : '\u25BC') : '\u25B4' }}</span></th>
                    <th class="sortable" @click="toggleSort(aggregatedSort, 'totalShares')">Total Shares <span class="sort-icon">{{ aggregatedSort.column === 'totalShares' ? (aggregatedSort.direction === 'asc' ? '\u25B2' : '\u25BC') : '\u25B4' }}</span></th>
                    <th class="sortable" @click="toggleSort(aggregatedSort, 'avgPrice')">Avg Price <span class="sort-icon">{{ aggregatedSort.column === 'avgPrice' ? (aggregatedSort.direction === 'asc' ? '\u25B2' : '\u25BC') : '\u25B4' }}</span></th>
                    <th class="sortable" @click="toggleSort(aggregatedSort, 'totalValue')">Total Value <span class="sort-icon">{{ aggregatedSort.column === 'totalValue' ? (aggregatedSort.direction === 'asc' ? '\u25B2' : '\u25BC') : '\u25B4' }}</span></th>
                  </tr>
                </thead>
                <tbody>
                  <tr
                    v-for="holding in filteredAggregatedHoldings"
                    :key="holding.ticker"
                    @click="showContextMenu($event, holding.ticker)"
                    @contextmenu.prevent="showContextMenu($event, holding.ticker)"
                    class="holding-row"
                  >
                    <td class="ticker">{{ holding.ticker }}</td>
                    <td class="name">{{ holding.name }}</td>
                    <td class="brokers">
                      <span
                        v-for="broker in holding.brokers"
                        :key="broker"
                        class="broker-tag"
                        :title="broker.charAt(0).toUpperCase() + broker.slice(1)"
                        :style="{
                          backgroundColor: getBrokerColor(broker).background,
                          color: getBrokerColor(broker).primary,
                          borderColor: getBrokerColor(broker).primary
                        }"
                      >
                        {{ getBrokerAbbrev(broker) }}
                      </span>
                    </td>
                    <td class="shares">{{ formatNumber(holding.totalShares) }}</td>
                    <td class="price">${{ formatNumber(holding.avgPrice) }}</td>
                    <td class="value">${{ formatNumber(holding.totalValue) }}</td>
                  </tr>
                </tbody>
              </table>
            </div>
          </div>
        </div>

        <!-- Level 2: Single Broker View (shows all accounts for that broker) -->
        <div v-else-if="selectedBroker && !selectedAccount" class="broker-view">
          <div class="accounts-header">
            <h3>{{ selectedBroker.type }} - {{ selectedBroker.email }}</h3>
            <div class="portfolio-summary">
              <div class="summary-item">
                <span class="label">Broker Value:</span>
                <span class="value highlight">${{ formatNumber(getBrokerTotalValue(selectedBroker)) }}</span>
              </div>
              <div class="summary-item">
                <span class="label">Accounts:</span>
                <span class="value">{{ getBrokerAccountCount(selectedBroker) }}</span>
              </div>
            </div>
          </div>

          <div class="accounts-list">
            <div
              v-for="(account, index) in brokerAccounts"
              :key="account.id"
              class="account-card clickable"
              :class="{ 'loading': accountsLoading[account.id] }"
              @click="selectAccount(account)"
            >
              <div class="account-header">
                <h4>{{ account.name || 'Account ' + (index + 1) }}</h4>
                <div class="account-status">
                  <span class="status-badge" :class="account.status.toLowerCase()">{{ account.status }}</span>
                  <span v-if="account.isPrimary" class="primary-badge">Primary</span>
                </div>
              </div>

              <div class="account-summary-row">
                <div class="account-stat">
                  <span class="label">Value:</span>
                  <span class="value">${{ formatNumber(getAccountTotalValue(account.id)) }}</span>
                </div>
                <div class="account-stat">
                  <span class="label">Holdings:</span>
                  <span class="value">{{ getAccountHoldings(account.id).filter(h => Number(h.shares) > 0).length }}</span>
                </div>
                <div class="account-stat">
                  <span class="label">Cash:</span>
                  <span class="value">${{ formatNumber(getAccountCash(account.id)?.balance?.canTrade || 0) }}</span>
                </div>
              </div>

              <div class="view-account-link">View Account &rarr;</div>
            </div>
          </div>
        </div>

        <!-- Level 3: Single Account Detail View -->
        <div v-else-if="selectedAccount" class="account-detail-view">
          <div class="accounts-header">
            <h3>{{ selectedAccount.name || 'Account' }}</h3>
            <div class="account-meta">
              <span class="status-badge" :class="selectedAccount.status.toLowerCase()">{{ selectedAccount.status }}</span>
              <span v-if="selectedAccount.isPrimary" class="primary-badge">Primary</span>
            </div>
          </div>

          <div class="account-id">
            <span class="label">ID:</span>
            <span class="value">{{ selectedAccount.id }}</span>
          </div>

          <div v-if="accountsLoading[selectedAccount.id]" class="account-loading">
            <p>Loading account details...</p>
          </div>

          <div v-else-if="accountsError[selectedAccount.id]" class="account-error">
            <p>{{ accountsError[selectedAccount.id] }}</p>
            <button class="btn secondary small-btn" @click="loadAccountDetails(selectedAccount.id)">Retry</button>
          </div>

          <div v-else>
            <div class="account-value">
              <span class="label">Total Value:</span>
              <span class="value highlight">${{ formatNumber(getAccountTotalValue(selectedAccount.id)) }}</span>
            </div>

            <div v-if="getAccountHoldings(selectedAccount.id).length > 0" class="holdings-section">
              <h5>Holdings</h5>
              <div class="holdings-table">
                <table>
                  <thead>
                    <tr>
                      <th class="sortable" @click="toggleSort(accountSort, 'ticker')">Ticker <span class="sort-icon">{{ accountSort.column === 'ticker' ? (accountSort.direction === 'asc' ? '\u25B2' : '\u25BC') : '\u25B4' }}</span></th>
                      <th class="sortable" @click="toggleSort(accountSort, 'name')">Name <span class="sort-icon">{{ accountSort.column === 'name' ? (accountSort.direction === 'asc' ? '\u25B2' : '\u25BC') : '\u25B4' }}</span></th>
                      <th class="sortable" @click="toggleSort(accountSort, 'shares')">Shares <span class="sort-icon">{{ accountSort.column === 'shares' ? (accountSort.direction === 'asc' ? '\u25B2' : '\u25BC') : '\u25B4' }}</span></th>
                      <th class="sortable" @click="toggleSort(accountSort, 'price')">Price <span class="sort-icon">{{ accountSort.column === 'price' ? (accountSort.direction === 'asc' ? '\u25B2' : '\u25BC') : '\u25B4' }}</span></th>
                      <th class="sortable" @click="toggleSort(accountSort, 'marketValue')">Value <span class="sort-icon">{{ accountSort.column === 'marketValue' ? (accountSort.direction === 'asc' ? '\u25B2' : '\u25BC') : '\u25B4' }}</span></th>
                      <th v-if="hasCanSellData(selectedAccount.id)" class="sortable" @click="toggleSort(accountSort, 'canSell')">Status <span class="sort-icon">{{ accountSort.column === 'canSell' ? (accountSort.direction === 'asc' ? '\u25B2' : '\u25BC') : '\u25B4' }}</span></th>
                    </tr>
                  </thead>
                  <tbody>
                    <template v-for="holding in sortedHoldings(selectedAccount.id)" :key="holding.isin">
                      <tr
                        v-if="holding && typeof holding.shares !== 'undefined' && Number(holding.shares) > 0"
                        @click="showContextMenu($event, holding.ticker)"
                        @contextmenu.prevent="showContextMenu($event, holding.ticker)"
                        class="holding-row"
                      >
                        <td class="ticker">{{ holding.ticker }}</td>
                        <td class="name">{{ holding.name }}</td>
                        <td class="shares">{{ formatNumber(holding.shares) }}</td>
                        <td class="price">${{ formatNumber(holding.price) }}</td>
                        <td class="value">${{ formatNumber(holding.marketValue) }}</td>
                        <td v-if="hasCanSellData(selectedAccount.id)" class="status" :class="{ 'sellable': holding.canSell, 'not-sellable': holding.canSell === false }">
                          <span v-if="holding.canSell" class="sellable-tag">Sellable</span>
                          <span v-else-if="holding.canSell === false" class="not-sellable-tag">Not Sellable</span>
                        </td>
                      </tr>
                    </template>
                  </tbody>
                </table>
              </div>
            </div>

            <div v-else class="no-holdings">
              <p>No holdings found for this account.</p>
            </div>

            <div v-if="getAccountCash(selectedAccount.id)" class="cash-section">
              <h5>Cash Balance</h5>
              <div class="cash-info">
                <div class="info-item">
                  <span class="label">Available to Trade:</span>
                  <span class="value">${{ formatNumber(getAccountCash(selectedAccount.id).balance?.canTrade) }}</span>
                </div>
                <div class="info-item">
                  <span class="label">Available to Withdraw:</span>
                  <span class="value">${{ formatNumber(getAccountCash(selectedAccount.id).balance?.canWithdraw) }}</span>
                </div>
                <div class="info-item">
                  <span class="label">Currency:</span>
                  <span class="value">{{ getAccountCash(selectedAccount.id).currency }}</span>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
    <!-- Custom Context Menu -->
    <div
      v-if="contextMenu.visible"
      class="context-menu"
      :style="{ top: contextMenu.y + 'px', left: contextMenu.x + 'px' }"
    >
      <div class="context-menu-item" @click="handleSellTicker">
        Sell {{ contextMenu.ticker }}
      </div>
      <div class="context-menu-item" @click="handleBuyTicker">
        Buy {{ contextMenu.ticker }}
      </div>
    </div>
  </div>
</template>

<script>
import { ref, reactive, onMounted, onUnmounted, computed, inject, watch } from 'vue';
import { getBrokerAccountHoldings, getBrokerAccountCash, getAccountHoldings, getAccountCash } from '@/tauri';

export default {
  name: 'AccountView',

  props: {
    isLoggedIn: {
      type: Boolean,
      default: false
    },
    forceReload: {
      type: Boolean,
      default: false
    },
    linkedBrokers: {
      type: Array,
      default: () => []
    }
  },

  emits: ['accounts-loaded', 'trade-ticker'],

  setup(props, { emit }) {
    // Global state to persist across component unmounts
    const globalState = inject('globalState', {
      accountsData: {
        accounts: [],
        accountHoldings: {},
        accountCash: {},
        lastLoaded: null
      }
    });

    // State
    const accounts = ref(globalState.accountsData.accounts || []);
    const accountHoldings = ref(globalState.accountsData.accountHoldings || {});
    const accountCash = ref(globalState.accountsData.accountCash || {});
    const initialLoading = ref(true); // Only true on very first load
    const accountsLoading = ref({});
    const accountsError = ref({});
    const brokerLoading = ref({}); // Track loading state per broker
    const error = ref(null);
    const loadedAccounts = ref([]);
    const lastLoaded = ref(globalState.accountsData.lastLoaded);

    // Broker color configuration - consistent colors per broker type
    const brokerColors = {
      fennel: { primary: '#64ffda', background: 'rgba(100, 255, 218, 0.15)' },
      public: { primary: '#ff9800', background: 'rgba(255, 152, 0, 0.15)' },
      robinhood: { primary: '#00c805', background: 'rgba(0, 200, 5, 0.15)' },
      tastytrade: { primary: '#ff5722', background: 'rgba(255, 87, 34, 0.15)' },
      fidelity: { primary: '#4caf50', background: 'rgba(76, 175, 80, 0.15)' },
      schwab: { primary: '#2196f3', background: 'rgba(33, 150, 243, 0.15)' },
      default: { primary: '#9c27b0', background: 'rgba(156, 39, 176, 0.15)' }
    };

    const getBrokerColor = (brokerType) => {
      const type = (brokerType || 'default').toLowerCase();
      return brokerColors[type] || brokerColors.default;
    };

    const brokerAbbrevs = {
      fennel: 'FN',
      public: 'PB',
      robinhood: 'RH',
      tastytrade: 'TT',
      webull: 'WB',
      fidelity: 'FD',
      schwab: 'SC'
    };

    const getBrokerAbbrev = (brokerType) => {
      const type = (brokerType || '').toLowerCase();
      return brokerAbbrevs[type] || brokerType.substring(0, 2).toUpperCase();
    };

    // Filter state for All Holdings table
    const holdingsFilter = ref('');

    // Sort state for tables
    const aggregatedSort = reactive({ column: 'ticker', direction: 'asc' });
    const accountSort = reactive({ column: 'ticker', direction: 'asc' });

    const toggleSort = (sortState, column) => {
      if (sortState.column === column) {
        sortState.direction = sortState.direction === 'asc' ? 'desc' : 'asc';
      } else {
        sortState.column = column;
        sortState.direction = 'asc';
      }
    };

    const numericColumns = new Set([
      'totalShares', 'totalValue', 'avgPrice', 'price',
      'shares', 'marketValue'
    ]);

    const compareValues = (a, b, column, direction) => {
      let valA = a[column];
      let valB = b[column];

      // Boolean columns (sellable status) - true sorts before false in asc
      if (typeof valA === 'boolean' || typeof valB === 'boolean') {
        const nA = valA ? 1 : 0;
        const nB = valB ? 1 : 0;
        return direction === 'asc' ? nB - nA : nA - nB;
      }

      // Numeric columns (may be strings from backend)
      if (numericColumns.has(column)) {
        const nA = Number(valA) || 0;
        const nB = Number(valB) || 0;
        return direction === 'asc' ? nA - nB : nB - nA;
      }

      // Array columns (brokers) - sort by count
      if (Array.isArray(valA) && Array.isArray(valB)) {
        return direction === 'asc' ? valA.length - valB.length : valB.length - valA.length;
      }

      // String columns
      valA = String(valA || '').toLowerCase();
      valB = String(valB || '').toLowerCase();
      const cmp = valA.localeCompare(valB);
      return direction === 'asc' ? cmp : -cmp;
    };

    // Navigation state for drill-down
    const selectedBroker = ref(null);
    const selectedAccount = ref(null);
    
    // Computed
    const anyAccountLoaded = computed(() => {
      return loadedAccounts.value.length > 0;
    });
    
    const shouldReload = computed(() => {
      // Reload if forced, if no data, or if data is older than 5 minutes
      if (props.forceReload) return true;
      if (!lastLoaded.value) return true;
      if (accounts.value.length === 0) return true;
      
      const fiveMinutesAgo = new Date();
      fiveMinutesAgo.setMinutes(fiveMinutesAgo.getMinutes() - 5);
      return lastLoaded.value < fiveMinutesAgo;
    });
    
    const sortedAccounts = computed(() => {
      return [...accounts.value].sort((a, b) => {
        // Primary accounts first
        if (a.isPrimary && !b.isPrimary) return -1;
        if (!a.isPrimary && b.isPrimary) return 1;
        
        // Then by status (APPROVED first)
        if (a.status === 'APPROVED' && b.status !== 'APPROVED') return -1;
        if (a.status !== 'APPROVED' && b.status === 'APPROVED') return 1;
        
        // Then by name - handle numeric account names properly
        const nameA = a.name || `Account ${a.id.substring(0, 6)}`;
        const nameB = b.name || `Account ${b.id.substring(0, 6)}`;
        
        // Check if both names are in "Account X" format
        const accountRegex = /^Account\s+(\d+)$/;
        const matchA = nameA.match(accountRegex);
        const matchB = nameB.match(accountRegex);
        
        if (matchA && matchB) {
          // If both are numeric accounts, sort numerically
          return parseInt(matchA[1], 10) - parseInt(matchB[1], 10);
        } else if (matchA) {
          // If only A is a numeric account, it comes first
          return -1;
        } else if (matchB) {
          // If only B is a numeric account, it comes first
          return 1;
        }
        
        // Otherwise use alphabetical sorting
        return nameA.localeCompare(nameB);
      });
    });
    
    const totalPortfolioValue = computed(() => {
      let total = 0;
      
      // Add up all holdings values
      accounts.value.forEach(account => {
        if (accountHoldings.value[account.id]) {
          accountHoldings.value[account.id].forEach(holding => {
            total += Number(holding.marketValue) || 0;
          });
        }
        
        // Add cash balance
        if (accountCash.value[account.id] && accountCash.value[account.id].balance) {
          total += Number(accountCash.value[account.id].balance.canTrade) || 0;
        }
      });
      
      return total;
    });
    
    const totalCashBalance = computed(() => {
      let total = 0;
      
      accounts.value.forEach(account => {
        if (accountCash.value[account.id] && accountCash.value[account.id].balance) {
          total += Number(accountCash.value[account.id].balance.canTrade) || 0;
        }
      });
      
      return total;
    });
    
    const totalInvestmentsValue = computed(() => {
      return totalPortfolioValue.value - totalCashBalance.value;
    });

    // Computed properties
    const currentAccounts = computed(() => globalState.accountsData.accounts || []);

    // Get accounts for a specific broker, filtered by broker ID
    const brokerAccounts = computed(() => {
      if (!selectedBroker.value) return [];
      // Filter to APPROVED accounts for this specific broker and sort
      return sortedAccounts.value.filter(acc =>
        acc.brokerId === selectedBroker.value.id && acc.status === 'APPROVED'
      );
    });

    // Aggregate holdings across all accounts for summary view
    const aggregatedHoldings = computed(() => {
      const holdingsMap = {};

      accounts.value.forEach(account => {
        const holdings = accountHoldings.value[account.id] || [];
        const brokerType = account.brokerType || 'unknown';

        holdings.forEach(holding => {
          if (!holding || !holding.ticker || Number(holding.shares) <= 0) return;

          if (holdingsMap[holding.ticker]) {
            holdingsMap[holding.ticker].totalShares += Number(holding.shares) || 0;
            holdingsMap[holding.ticker].totalValue += Number(holding.marketValue) || 0;
            // Track which brokers hold this stock (unique set)
            if (!holdingsMap[holding.ticker].brokers.includes(brokerType)) {
              holdingsMap[holding.ticker].brokers.push(brokerType);
            }
          } else {
            holdingsMap[holding.ticker] = {
              ticker: holding.ticker,
              name: holding.name,
              totalShares: Number(holding.shares) || 0,
              totalValue: Number(holding.marketValue) || 0,
              price: Number(holding.price) || 0,
              brokers: [brokerType] // Track brokers holding this stock
            };
          }
        });
      });

      // Calculate average price and convert to array
      const results = Object.values(holdingsMap)
        .map(h => ({
          ...h,
          avgPrice: h.totalShares > 0 ? h.totalValue / h.totalShares : 0
        }));

      return results.sort((a, b) =>
        compareValues(a, b, aggregatedSort.column, aggregatedSort.direction)
      );
    });

    const filteredAggregatedHoldings = computed(() => {
      const query = holdingsFilter.value.trim().toLowerCase();
      if (!query) return aggregatedHoldings.value;
      return aggregatedHoldings.value.filter(h => {
        const ticker = String(h.ticker || '').toLowerCase();
        const name = String(h.name || '').toLowerCase();
        return ticker.includes(query) || name.includes(query);
      });
    });

    // Returns true if any holding for this account has an explicit canSell value
    const hasCanSellData = (accountId) => {
      const holdings = accountHoldings.value[accountId] || [];
      return holdings.some(h => typeof h.canSell === 'boolean');
    };

    const sortedHoldings = (accountId) => {
      const holdings = accountHoldings.value[accountId] || [];
      return [...holdings].sort((a, b) =>
        compareValues(a, b, accountSort.column, accountSort.direction)
      );
    };

    // Navigation methods
    const selectBroker = (broker) => {
      selectedBroker.value = broker;
      selectedAccount.value = null;

      // Auto-navigate to account if broker has only one account
      const brokerAccountsList = accounts.value.filter(
        acc => acc.brokerId === broker.id && acc.status === 'APPROVED'
      );
      if (brokerAccountsList.length === 1) {
        // Automatically select the single account
        selectedAccount.value = brokerAccountsList[0];
      }
    };

    const selectAccount = (account) => {
      selectedAccount.value = account;
    };

    const goToOverview = () => {
      selectedBroker.value = null;
      selectedAccount.value = null;
    };

    const goToBroker = (broker) => {
      selectedBroker.value = broker;
      selectedAccount.value = null;
    };

    // Broker helper methods
    const getBrokerAccountCount = (broker) => {
      if (broker.status !== 'connected') return 0;
      // Filter accounts by broker ID
      return accounts.value.filter(acc => acc.brokerId === broker.id && acc.status === 'APPROVED').length;
    };

    const getBrokerTotalValue = (broker) => {
      if (broker.status !== 'connected') return 0;

      let total = 0;

      // Get accounts for this broker
      const brokerAccounts = accounts.value.filter(acc => acc.brokerId === broker.id);

      brokerAccounts.forEach(account => {
        // Add holdings value
        if (accountHoldings.value[account.id]) {
          accountHoldings.value[account.id].forEach(holding => {
            total += Number(holding.marketValue) || 0;
          });
        }

        // Add cash balance
        if (accountCash.value[account.id] && accountCash.value[account.id].balance) {
          total += Number(accountCash.value[account.id].balance.canTrade) || 0;
        }
      });

      return total;
    };

    // Methods
    const formatNumber = (value) => {
      if (value === undefined || value === null) return '0.00';
      return Number(value).toLocaleString('en-US', {
        minimumFractionDigits: 2,
        maximumFractionDigits: 2
      });
    };

    const getAccountHoldings = (accountId) => {
      return accountHoldings.value[accountId] || [];
    };
    
    const getAccountCash = (accountId) => {
      return accountCash.value[accountId] || null;
    };
    
    const getAccountTotalValue = (accountId) => {
      let total = 0;
      
      // Add holdings value
      if (accountHoldings.value[accountId]) {
        accountHoldings.value[accountId].forEach(holding => {
          total += Number(holding.marketValue) || 0;
        });
      }
      
      // Add cash balance
      if (accountCash.value[accountId] && accountCash.value[accountId].balance) {
        total += Number(accountCash.value[accountId].balance.canTrade) || 0;
      }
      
      return total;
    };
    
    const loadAllAccountDetails = async (isRefresh = false) => {
      if (!props.isLoggedIn) return;

      error.value = null; // Clear previous errors

      try {
        // Get the account list from the computed property (already loaded by parent)
        const accountsToLoad = currentAccounts.value;

        if (!accountsToLoad || accountsToLoad.length === 0) {
          console.warn("No accounts found in global state to load details for.");
          accounts.value = [];
          emit('accounts-loaded', []);
          initialLoading.value = false;
          return;
        }

        console.log(`AccountView: Found ${accountsToLoad.length} accounts in global state. Loading details...`);

        // Update local ref
        accounts.value = accountsToLoad;

        // Group accounts by broker for per-broker loading tracking
        const brokerAccountsMap = {};
        accountsToLoad.forEach(account => {
          const brokerId = account.brokerId || 'unknown';
          if (!brokerAccountsMap[brokerId]) {
            brokerAccountsMap[brokerId] = [];
          }
          brokerAccountsMap[brokerId].push(account);
        });

        // Set all brokers to loading state (this is the per-broker loading indicator)
        Object.keys(brokerAccountsMap).forEach(brokerId => {
          brokerLoading.value[brokerId] = true;
        });

        // Hide the full-page loading as soon as we have broker structure
        // This allows users to see the broker cards with individual loading indicators
        initialLoading.value = false;

        // Load accounts grouped by broker
        const brokerPromises = Object.entries(brokerAccountsMap).map(async ([brokerId, brokerAccts]) => {
          try {
            // Load all accounts for this broker
            const accountPromises = brokerAccts.map((account, index) => {
              return new Promise(resolve => {
                setTimeout(async () => {
                  try {
                    await loadAccountDetails(account.id);
                  } catch (err) {
                    console.error(`Error loading details for account ${account.id}:`, err);
                  }
                  resolve();
                }, index * 100); // Small delay between accounts in same broker
              });
            });

            await Promise.all(accountPromises);
          } finally {
            // Mark this broker as done loading
            brokerLoading.value[brokerId] = false;
          }
        });

        // Wait for all brokers to finish
        await Promise.all(brokerPromises);

        // Update last loaded timestamp
        lastLoaded.value = new Date();
        globalState.accountsData.lastLoaded = lastLoaded.value;

        // Emit event to notify parent that accounts are loaded
        emit('accounts-loaded', accounts.value);
        console.log("Emitted accounts-loaded event");
      } catch (err) {
        console.error('Failed to load account details:', err);
        error.value = 'Failed to load accounts: ' + err.message;
      } finally {
        initialLoading.value = false;
        // Clear all broker loading states
        Object.keys(brokerLoading.value).forEach(brokerId => {
          brokerLoading.value[brokerId] = false;
        });
      }
    };
    
    const loadAccountDetails = async (accountId) => {
      if (!props.isLoggedIn || !accountId) return;

      accountsLoading.value[accountId] = true;
      accountsError.value[accountId] = null;

      try {
        console.log("Loading account details for:", accountId);

        // Find the account to get its brokerId
        const account = accounts.value.find(acc => acc.id === accountId);
        const brokerId = account?.brokerId;

        let holdings, cash;

        if (brokerId) {
          // Use broker-specific API calls
          console.log("Using broker-specific API for broker:", brokerId);
          holdings = await getBrokerAccountHoldings(brokerId, accountId);
          cash = await getBrokerAccountCash(brokerId, accountId);
        } else {
          // Fallback to active broker API (backwards compatibility)
          console.log("Using active broker API (no brokerId found)");
          holdings = await getAccountHoldings(accountId);
          cash = await getAccountCash(accountId);
        }

        console.log("Holdings loaded for account", accountId, ":", holdings);
        accountHoldings.value[accountId] = holdings || [];

        // Update global state
        globalState.accountsData.accountHoldings[accountId] = holdings || [];

        console.log("Cash balance loaded for account", accountId, ":", cash);
        accountCash.value[accountId] = cash || null;

        // Update global state
        globalState.accountsData.accountCash[accountId] = cash || null;

        // Mark account as loaded
        if (!loadedAccounts.value.includes(accountId)) {
          loadedAccounts.value.push(accountId);
        }
      } catch (err) {
        console.error('Failed to load account details for', accountId, ':', err);
        accountsError.value[accountId] = 'Failed to load account details: ' + (err?.message || err);
      } finally {
        accountsLoading.value[accountId] = false;
      }
    };
    
    const refreshAccounts = async () => {
      console.log("Manually refreshing accounts...");
      // Pass true to indicate this is a refresh (don't reset UI)
      await loadAllAccountDetails(true);
    };
    
    // Lifecycle hooks
    onMounted(() => {
      console.log("AccountView mounted, isLoggedIn:", props.isLoggedIn);
      console.log("forceReload:", props.forceReload);
      console.log("Cached accounts:", globalState.accountsData.accounts.length);
      console.log("Last loaded:", globalState.accountsData.lastLoaded);

      if (props.isLoggedIn) {
        // Always sync local state from global state first
        if (globalState.accountsData.accounts.length > 0) {
          accounts.value = globalState.accountsData.accounts;
          accountHoldings.value = globalState.accountsData.accountHoldings || {};
          accountCash.value = globalState.accountsData.accountCash || {};
          lastLoaded.value = globalState.accountsData.lastLoaded || lastLoaded.value;
        }

        if (shouldReload.value) {
          console.log("Data needs to be reloaded");
          // Triggered by parent flag now
          loadAllAccountDetails();
        } else if (accounts.value.length > 0) {
          console.log("Using cached account data from", lastLoaded.value);
          // Turn off initial loading since we have cached data
          initialLoading.value = false;

          // If we have data but some accounts weren't fully loaded, load them in parallel
          const accountsToLoad = sortedAccounts.value.filter(account =>
            !accountHoldings.value[account.id] || !accountCash.value[account.id]
          );

          if (accountsToLoad.length > 0) {
            console.log(`Loading details for ${accountsToLoad.length} accounts that weren't fully loaded`);

            // Set broker loading states for accounts being loaded
            accountsToLoad.forEach(account => {
              if (account.brokerId) {
                brokerLoading.value[account.brokerId] = true;
              }
            });

            accountsToLoad.forEach((account, index) => {
              setTimeout(async () => {
                await loadAccountDetails(account.id);
                // Check if all accounts for this broker are done
                const brokerAccts = accountsToLoad.filter(a => a.brokerId === account.brokerId);
                const allLoaded = brokerAccts.every(a =>
                  accountHoldings.value[a.id] && accountCash.value[a.id]
                );
                if (allLoaded && account.brokerId) {
                  brokerLoading.value[account.brokerId] = false;
                }
              }, index * 200); // 200ms delay between starting each request
            });
          } else {
            console.log("All account details are already loaded");
          }
        } else {
          // No cached data - need to load
          console.log("No cached data, triggering load");
          loadAllAccountDetails();
        }
      } else {
        initialLoading.value = false;
      }
    });
    
    // Watch for prop changes
    if (import.meta.hot) {
      import.meta.hot.accept(() => {
        console.log("Hot module replacement for AccountView");
        if (props.isLoggedIn && shouldReload.value) {
          loadAllAccountDetails(); 
        }
      });
    }
    
    // Context menu state
    const contextMenu = reactive({
      visible: false,
      x: 0,
      y: 0,
      ticker: ''
    });

    const showContextMenu = (event, ticker) => {
      event.stopPropagation();
      contextMenu.visible = true;
      contextMenu.x = event.clientX;
      contextMenu.y = event.clientY;
      contextMenu.ticker = ticker;
    };

    const hideContextMenu = () => {
      contextMenu.visible = false;
    };

    const handleSellTicker = () => {
      emit('trade-ticker', { ticker: contextMenu.ticker, side: 'sell' });
      hideContextMenu();
    };

    const handleBuyTicker = () => {
      emit('trade-ticker', { ticker: contextMenu.ticker, side: 'buy' });
      hideContextMenu();
    };

    // Close context menu on click anywhere
    const onDocumentClick = () => {
      hideContextMenu();
    };

    onMounted(() => {
      document.addEventListener('click', onDocumentClick);
    });

    onUnmounted(() => {
      document.removeEventListener('click', onDocumentClick);
    });

    // Watch for the forceReload prop change
    watch(() => props.forceReload, (newValue) => {
      if (newValue === true) {
        console.log("AccountView: forceReload prop changed to true, triggering details refresh.");
        loadAllAccountDetails();
        // No need to emit here as loadAllAccountDetails already emits 'accounts-loaded'
        // which the parent uses to set forceReload back to false.
      }
    });
    
    return {
      accounts,
      sortedAccounts,
      initialLoading,
      accountsLoading,
      accountsError,
      error,
      anyAccountLoaded,
      totalPortfolioValue,
      totalCashBalance,
      totalInvestmentsValue,
      formatNumber,
      getAccountHoldings,
      getAccountCash,
      getAccountTotalValue,
      loadAllAccountDetails,
      loadAccountDetails,
      refreshAccounts,
      sortedHoldings,
      hasCanSellData,
      // Multi-broker navigation
      selectedBroker,
      selectedAccount,
      selectBroker,
      selectAccount,
      goToOverview,
      goToBroker,
      brokerAccounts,
      aggregatedHoldings,
      filteredAggregatedHoldings,
      holdingsFilter,
      getBrokerAccountCount,
      getBrokerTotalValue,
      brokerLoading,
      linkedBrokers: computed(() => props.linkedBrokers),
      // Broker colors
      getBrokerColor,
      getBrokerAbbrev,
      brokerColors,
      // Sorting
      aggregatedSort,
      accountSort,
      toggleSort,
      // Context menu
      contextMenu,
      showContextMenu,
      hideContextMenu,
      handleSellTicker,
      handleBuyTicker
    };
  }
};
</script>

<style scoped>
.account-view {
  width: 100%;
  height: 100%;
  overflow: auto;
}

.login-required, .loading-page, .error, .no-accounts {
  display: flex;
  flex-direction: column;
  justify-content: center;
  align-items: center;
  height: 200px;
  color: var(--text-secondary, #999);
  gap: 16px;
}

.error {
  color: var(--error-color, #f87171);
}

.accounts-overview {
  display: flex;
  flex-direction: column;
  gap: 24px;
}

.accounts-header {
  display: flex;
  flex-direction: column;
  gap: 12px;
  background-color: var(--bg-secondary, #1a1a1a);
  border: 1px solid var(--border-color, rgba(255, 255, 255, 0.08));
  border-radius: var(--radius-md, 10px);
  padding: 20px;
}

.accounts-header h3 {
  margin: 0;
  font-size: 18px;
  font-weight: 600;
  color: var(--text-primary, #e8e8e8);
}

.portfolio-summary {
  display: flex;
  flex-wrap: wrap;
  gap: 24px;
}

.summary-item {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.accounts-list {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.account-card {
  background-color: var(--bg-secondary, #1a1a1a);
  border: 1px solid var(--border-color, rgba(255, 255, 255, 0.08));
  border-radius: var(--radius-md, 10px);
  padding: 20px;
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.account-card.loading {
  opacity: 0.7;
}

.account-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.account-header h4 {
  margin: 0;
  font-size: 16px;
  font-weight: 600;
  color: var(--text-primary, #e8e8e8);
}

.account-status {
  display: flex;
  gap: 8px;
  align-items: center;
}

.status-badge {
  padding: 4px 10px;
  border-radius: 20px;
  font-size: 11px;
  font-weight: 500;
}

.status-badge.approved {
  background-color: rgba(74, 222, 128, 0.12);
  color: var(--success-color, #4ade80);
}

.status-badge.pending {
  background-color: rgba(251, 191, 36, 0.12);
  color: var(--warning-color, #fbbf24);
}

.primary-badge {
  padding: 4px 10px;
  border-radius: 20px;
  font-size: 11px;
  font-weight: 500;
  background-color: var(--accent-muted, rgba(124, 138, 255, 0.15));
  color: var(--accent-primary, #7c8aff);
}

.account-id {
  display: flex;
  gap: 8px;
  font-size: 12px;
  color: var(--text-muted, #666);
}

.account-value {
  display: flex;
  gap: 8px;
  font-size: 14px;
  color: var(--text-primary, #e8e8e8);
  margin-bottom: 8px;
}

.account-loading, .account-error {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  padding: 16px;
  color: var(--text-secondary, #999);
}

.account-error {
  color: var(--error-color, #f87171);
}

.holdings-section, .cash-section {
  margin-top: 16px;
}

h5 {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-secondary, #999);
  text-transform: uppercase;
  letter-spacing: 0.3px;
  margin: 0 0 10px 0;
}

.holdings-table {
  overflow-x: auto;
}

table {
  width: 100%;
  border-collapse: collapse;
}

th, td {
  padding: 10px 12px;
  text-align: left;
}

th {
  font-weight: 500;
  color: var(--text-muted, #666);
  font-size: 12px;
  border-bottom: 1px solid var(--border-color, rgba(255, 255, 255, 0.08));
}

td {
  color: var(--text-primary, #e8e8e8);
  font-size: 14px;
  border-bottom: 1px solid var(--border-color, rgba(255, 255, 255, 0.04));
}

tr:hover td {
  background-color: var(--bg-tertiary, #242424);
}

tr:last-child td {
  border-bottom: none;
}

td.ticker {
  font-weight: 600;
}

td.price, td.value, td.shares {
  text-align: right;
}

.cash-info {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  gap: 8px;
  background-color: var(--bg-tertiary, #242424);
  border-radius: var(--radius-sm, 6px);
  padding: 14px;
}

.info-item {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.label {
  font-size: 12px;
  color: var(--text-muted, #666);
}

.value {
  font-size: 14px;
  color: var(--text-primary, #e8e8e8);
}

.no-holdings {
  color: var(--text-muted, #666);
  padding: 8px 0;
  font-size: 14px;
}

.btn {
  padding: 9px 18px;
  font-size: 14px;
  border-radius: var(--radius-sm, 6px);
  cursor: pointer;
  transition: all 0.15s ease;
  border: 1px solid var(--border-hover, rgba(255, 255, 255, 0.15));
  font-weight: 500;
  white-space: nowrap;
  font-family: inherit;
  background-color: transparent;
  color: var(--text-secondary, #999);
}

.btn:hover {
  border-color: var(--accent-primary, #7c8aff);
  color: var(--text-primary, #e8e8e8);
}

.small-btn {
  padding: 4px 10px;
  font-size: 12px;
}

.sellable {
  color: var(--success-color, #4ade80);
}

.not-sellable {
  color: var(--error-color, #f87171);
}

.sellable-tag {
  padding: 4px 10px;
  border-radius: 20px;
  background-color: rgba(74, 222, 128, 0.12);
  color: var(--success-color, #4ade80);
  font-weight: 500;
  font-size: 0.75rem;
  display: inline-block;
}

.not-sellable-tag {
  padding: 4px 10px;
  border-radius: 20px;
  background-color: rgba(248, 113, 113, 0.12);
  color: var(--error-color, #f87171);
  font-weight: 500;
  font-size: 0.75rem;
  display: inline-block;
}

/* Breadcrumb Navigation */
.breadcrumb {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 16px;
  font-size: 14px;
}

.breadcrumb-link {
  color: var(--accent-primary, #7c8aff);
  cursor: pointer;
  transition: opacity 0.15s;
}

.breadcrumb-link:hover {
  opacity: 0.8;
}

.breadcrumb-separator {
  color: var(--text-muted, #666);
}

.breadcrumb-current {
  color: var(--text-primary, #e8e8e8);
  font-weight: 600;
}

/* Broker Cards */
.broker-cards {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
  gap: 16px;
  margin-bottom: 24px;
}

.broker-card {
  background-color: var(--bg-secondary, #1a1a1a);
  border: 1px solid var(--border-color, rgba(255, 255, 255, 0.08));
  border-radius: var(--radius-md, 10px);
  padding: 20px;
  transition: all 0.15s ease;
}

.broker-card.loading {
  border-color: var(--accent-primary, #7c8aff);
  opacity: 0.9;
}

.broker-card.clickable {
  cursor: pointer;
}

.broker-card.clickable:hover {
  border-color: var(--border-hover, rgba(255, 255, 255, 0.15));
  background-color: var(--bg-tertiary, #242424);
}

/* Loading states */
.loading-spinner {
  display: inline-block;
  width: 10px;
  height: 10px;
  border: 2px solid var(--bg-tertiary, #242424);
  border-top-color: var(--accent-primary, #7c8aff);
  border-radius: 50%;
  animation: spin 1s linear infinite;
  margin-right: 6px;
}

@keyframes spin {
  to { transform: rotate(360deg); }
}

.loading-text {
  color: var(--text-muted, #666);
  animation: pulse 1.5s ease-in-out infinite;
}

@keyframes pulse {
  0%, 100% { opacity: 0.5; }
  50% { opacity: 1; }
}

.loading-message {
  color: var(--accent-primary, #7c8aff);
  font-size: 13px;
  animation: pulse 1.5s ease-in-out infinite;
}

.broker-card-header {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  margin-bottom: 12px;
}

.broker-info {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.broker-type-badge {
  padding: 4px 10px;
  border-radius: 20px;
  font-size: 11px;
  font-weight: 600;
  width: fit-content;
}

/* Broker tags in holdings table */
.broker-tag {
  display: inline-block;
  padding: 2px 8px;
  border-radius: 20px;
  font-size: 10px;
  font-weight: 600;
  margin-right: 4px;
  border: none;
}

td.brokers {
  white-space: nowrap;
}

.broker-email {
  color: var(--text-muted, #666);
  font-size: 13px;
}

.broker-status {
  font-size: 11px;
  font-weight: 500;
  padding: 4px 10px;
  border-radius: 20px;
}

.broker-status.connected {
  background-color: rgba(74, 222, 128, 0.12);
  color: var(--success-color, #4ade80);
}

.broker-status.needs_reauth {
  background-color: rgba(251, 191, 36, 0.12);
  color: var(--warning-color, #fbbf24);
}

.broker-status.disconnected {
  background-color: rgba(248, 113, 113, 0.12);
  color: var(--error-color, #f87171);
}

.broker-card-body {
  display: flex;
  gap: 24px;
  margin-bottom: 12px;
}

.broker-stat {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.broker-stat .label {
  font-size: 11px;
  color: var(--text-muted, #666);
}

.broker-stat .value {
  font-size: 16px;
  color: var(--text-primary, #e8e8e8);
  font-weight: 500;
}

.broker-card-footer {
  border-top: 1px solid var(--border-color, rgba(255, 255, 255, 0.08));
  padding-top: 12px;
  margin-top: 8px;
}

.view-details {
  color: var(--accent-primary, #7c8aff);
  font-size: 13px;
  font-weight: 500;
}

.broker-card-footer.needs-auth .reauth-message {
  color: var(--warning-color, #fbbf24);
  font-size: 12px;
}

/* Holdings Summary Section */
.holdings-summary-section {
  background-color: var(--bg-secondary, #1a1a1a);
  border: 1px solid var(--border-color, rgba(255, 255, 255, 0.08));
  border-radius: var(--radius-md, 10px);
  padding: 20px;
}

.holdings-summary-section h4 {
  margin: 0 0 12px 0;
  font-size: 16px;
  color: var(--text-primary, #e8e8e8);
}

.holdings-filter {
  position: relative;
  margin-bottom: 12px;
  max-width: 360px;
}

.filter-input {
  width: 100%;
  padding: 8px 32px 8px 12px;
  font-size: 13px;
  font-family: inherit;
  color: var(--text-primary, #e8e8e8);
  background-color: var(--bg-tertiary, #242424);
  border: 1px solid var(--border-color, rgba(255, 255, 255, 0.08));
  border-radius: var(--radius-sm, 6px);
  outline: none;
  transition: border-color 0.15s ease;
}

.filter-input:focus {
  border-color: var(--accent-primary, #7c8aff);
}

.filter-input::placeholder {
  color: var(--text-muted, #666);
}

.filter-clear {
  position: absolute;
  right: 6px;
  top: 50%;
  transform: translateY(-50%);
  background: none;
  border: none;
  color: var(--text-muted, #666);
  font-size: 18px;
  line-height: 1;
  cursor: pointer;
  padding: 2px 8px;
  border-radius: 4px;
  transition: color 0.15s ease, background-color 0.15s ease;
}

.filter-clear:hover {
  color: var(--text-primary, #e8e8e8);
  background-color: var(--bg-secondary, #1a1a1a);
}

/* Account Card Improvements */
.account-card.clickable {
  cursor: pointer;
  transition: all 0.15s ease;
}

.account-card.clickable:hover {
  border-color: var(--border-hover, rgba(255, 255, 255, 0.15));
  background-color: var(--bg-tertiary, #242424);
}

.account-summary-row {
  display: flex;
  gap: 24px;
  margin: 12px 0;
}

.account-stat {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.account-stat .label {
  font-size: 11px;
  color: var(--text-muted, #666);
}

.account-stat .value {
  font-size: 14px;
  color: var(--text-primary, #e8e8e8);
}

.view-account-link {
  color: var(--accent-primary, #7c8aff);
  font-size: 13px;
  font-weight: 500;
  margin-top: 8px;
}

/* Highlight class for important values */
.value.highlight {
  color: var(--accent-primary, #7c8aff);
  font-weight: 600;
  font-size: 18px;
}

/* Account Detail View */
.account-detail-view .account-value {
  margin: 16px 0;
}

.account-meta {
  display: flex;
  gap: 8px;
  align-items: center;
}

/* Summary View styling */
.summary-view {
  display: flex;
  flex-direction: column;
  gap: 24px;
}

/* Broker View styling */
.broker-view {
  display: flex;
  flex-direction: column;
  gap: 24px;
}

.broker-view .accounts-list {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
  gap: 16px;
}

.broker-view .account-card {
  border: 1px solid var(--border-color, rgba(255, 255, 255, 0.08));
}

/* Sortable column headers */
th.sortable {
  cursor: pointer;
  user-select: none;
  transition: color 0.15s ease;
}

th.sortable:hover {
  color: var(--accent-primary, #7c8aff);
}

.sort-icon {
  font-size: 9px;
  opacity: 0.4;
  margin-left: 2px;
}

th.sortable:hover .sort-icon {
  opacity: 0.7;
}

/* Holding row hover cursor */
.holding-row {
  cursor: pointer;
}

/* Custom Context Menu */
.context-menu {
  position: fixed;
  z-index: 10000;
  background-color: var(--bg-secondary, #1a1a1a);
  border: 1px solid var(--border-hover, rgba(255, 255, 255, 0.15));
  border-radius: var(--radius-sm, 6px);
  padding: 4px 0;
  min-width: 160px;
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.5);
}

.context-menu-item {
  padding: 8px 16px;
  font-size: 13px;
  color: var(--text-primary, #e8e8e8);
  cursor: pointer;
  transition: background-color 0.1s ease;
}

.context-menu-item:hover {
  background-color: var(--accent-muted, rgba(124, 138, 255, 0.15));
  color: var(--accent-primary, #7c8aff);
}
</style>