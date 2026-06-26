#![allow(dead_code)]

// Backend -> Frontend event names
pub const EVENT_LOG: &str = "log";
pub const EVENT_STARTUP_COMPLETE: &str = "startup_complete";
pub const EVENT_STARTUP_ERROR: &str = "startup_error";
pub const EVENT_2FA_STARTED: &str = "twofa_started";
pub const EVENT_LOGIN_SUCCESS: &str = "login_success";
pub const EVENT_LOGIN_FAILURE: &str = "login_failure";
pub const EVENT_LOGOUT_SUCCESS: &str = "logout_success";
pub const EVENT_BROKER_LINKED: &str = "broker_linked";
pub const EVENT_BROKER_UNLINKED: &str = "broker_unlinked";
pub const EVENT_LINK_BROKER_READY: &str = "link_broker_ready";
pub const EVENT_LINK_BROKER_ERROR: &str = "link_broker_error";
pub const EVENT_SELECT_BROKER_ERROR: &str = "select_broker_error";
pub const EVENT_BROKER_SELECTED: &str = "broker_selected";
pub const EVENT_UNLINK_BROKER_ERROR: &str = "unlink_broker_error";
pub const EVENT_TOKEN_EXPIRED: &str = "token_expired";
pub const EVENT_ROBINHOOD_PROMPT: &str = "robinhood_prompt_approval";
