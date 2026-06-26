<script setup lang="ts">
import { provide, reactive, onMounted } from 'vue';
import AppComponent from './components/App.vue';

// Declare custom window properties
declare global {
  interface Window {
    lastOrderPlaced: number | null;
    visitedAccountsTab: boolean;
    orderInProgress: boolean;
  }
}

// Create global state for persisting data across component unmounts
const globalState = reactive({
  accountsData: {
    accounts: [],
    accountHoldings: {},
    accountCash: {},
    lastLoaded: null
  }
});

// Initialize global window variables for tracking state
onMounted(() => {
  window.lastOrderPlaced = null;
  window.visitedAccountsTab = false;
  window.orderInProgress = false;
});

// Provide global state to all components
provide('globalState', globalState);
</script>

<template>
  <AppComponent />
</template>

<style>
body {
  margin: 0;
  padding: 0;
  background-color: var(--bg-primary, #0f0f0f);
  color: var(--text-primary, #e8e8e8);
  font-family: 'Inter', -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
}
</style>
