//! Global actions for HumanSSH.

use gpui::actions;

// Application lifecycle
actions!(humanssh, [Quit, OpenSettings]);

// Tab management
actions!(humanssh, [NewTab, CloseTab, NextTab, PrevTab]);

// Split management
actions!(humanssh, [SplitVertical, SplitHorizontal, ClosePane]);

// Focus navigation
actions!(humanssh, [FocusNextPane, FocusPrevPane]);

// macOS native features
actions!(humanssh, [ToggleSecureInput, ToggleOptionAsAlt]);
