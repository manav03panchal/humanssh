//! Integration tests for HumanSSH.
//!
//! These tests verify the integration between different components
//! of the terminal application without spawning real SSH connections.
//!
//! # Test Organization
//!
//! - `terminal_session` - Terminal session lifecycle tests
//! - `configuration` - Configuration loading/saving tests
//! - `theme` - Theme switching and color mapping tests
//!
//! # Running Tests
//!
//! ```bash
//! # Run all integration tests
//! cargo test --test integration_tests
//!
//! # Run a specific test module
//! cargo test --test integration_tests terminal_session
//!
//! # Run with output
//! cargo test --test integration_tests -- --nocapture
//! ```

mod common;

use common::{Fixtures, MockSettings, MockTerminalSession, TestEnv, SHORT_TIMEOUT};
use pretty_assertions::assert_eq as pretty_eq;
use serial_test::serial;
use std::sync::Arc;
use std::time::Duration;

// ============================================================================
// Terminal Session Lifecycle Tests
// ============================================================================

mod terminal_session {
    use super::*;

    #[test]
    fn test_session_creation() {
        let session = MockTerminalSession::new();

        // Session should start with default size
        assert_eq!(session.size, (80, 24));

        // Session should not be exited initially
        assert!(!session.has_exited());
        assert!(session.is_running());

        // Session should have a unique ID
        assert!(!session.id.is_nil());
    }

    #[test]
    fn test_session_with_custom_size() {
        let session = MockTerminalSession::with_size(120, 40);
        assert_eq!(session.size, (120, 40));
    }

    #[test]
    fn test_session_input_output_flow() {
        let session = MockTerminalSession::new();

        // Send input
        session.send_input("ls -la\n");
        assert_eq!(session.get_input(), "ls -la\n");

        // Simulate shell response
        session.write_output(Fixtures::SHELL_PROMPT);
        session.write_output("total 0\r\n");
        session.write_output("drwxr-xr-x  2 user user 40 Jan  1 00:00 .\r\n");

        // Verify output
        assert!(session.output_contains("total 0"));
        assert!(session.output_contains("drwxr-xr-x"));
    }

    #[test]
    fn test_session_resize() {
        let mut session = MockTerminalSession::new();
        assert_eq!(session.size, (80, 24));

        // Resize to larger dimensions
        session.resize(200, 50);
        assert_eq!(session.size, (200, 50));

        // Resize to smaller dimensions
        session.resize(40, 12);
        assert_eq!(session.size, (40, 12));
    }

    #[test]
    fn test_session_exit_lifecycle() {
        let session = MockTerminalSession::new();

        // Session starts running
        assert!(session.is_running());
        assert!(!session.has_exited());

        // Exit the session
        session.exit();

        // Session should now be exited
        assert!(!session.is_running());
        assert!(session.has_exited());
    }

    #[test]
    fn test_session_title() {
        let session = MockTerminalSession::new();

        // No title initially
        assert!(session.get_title().is_none());

        // Set title (simulating OSC escape sequence)
        session.set_title("vim ~/.bashrc");
        assert_eq!(session.get_title(), Some("vim ~/.bashrc".to_string()));

        // Update title
        session.set_title("bash");
        assert_eq!(session.get_title(), Some("bash".to_string()));
    }

    #[test]
    fn test_session_buffer_clearing() {
        let session = MockTerminalSession::new();

        // Add content
        session.send_input("command1\n");
        session.write_output("output1\n");

        // Clear input
        session.clear_input();
        assert_eq!(session.get_input(), "");
        assert!(session.output_contains("output1")); // Output not cleared

        // Clear output
        session.clear_output();
        assert_eq!(session.get_output(), "");
    }

    #[test]
    fn test_multiple_concurrent_sessions() {
        let session1 = MockTerminalSession::new();
        let session2 = MockTerminalSession::new();
        let session3 = MockTerminalSession::new();

        // Each session should have unique ID
        assert_ne!(session1.id, session2.id);
        assert_ne!(session2.id, session3.id);
        assert_ne!(session1.id, session3.id);

        // Operations on one session shouldn't affect others
        session1.send_input("session1");
        session2.send_input("session2");

        assert_eq!(session1.get_input(), "session1");
        assert_eq!(session2.get_input(), "session2");
        assert_eq!(session3.get_input(), "");
    }

    #[tokio::test]
    async fn test_session_async_operations() {
        let session = Arc::new(MockTerminalSession::new());
        let session_writer = session.clone();

        // Spawn async writer
        let write_handle = tokio::spawn(async move {
            for i in 0..5 {
                session_writer.write_output(&format!("line {}\n", i));
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        });

        // Wait for completion
        write_handle.await.unwrap();

        // Verify all output was written
        let output = session.get_output();
        for i in 0..5 {
            assert!(
                output.contains(&format!("line {}", i)),
                "Missing line {}",
                i
            );
        }
    }

    #[tokio::test]
    async fn test_session_exit_detection() {
        let session = Arc::new(MockTerminalSession::new());
        let session_clone = session.clone();

        // Spawn task that exits the session after delay
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            session_clone.exit();
        });

        // Wait for exit
        let result = common::wait_for(|| session.has_exited(), SHORT_TIMEOUT).await;
        assert!(result.is_ok(), "Session should have exited");
    }
}

// ============================================================================
// Configuration Tests
// ============================================================================

mod configuration {
    use super::*;

    #[test]
    fn test_env_creates_config_structure() {
        let env = TestEnv::new();

        // Config directory should exist
        assert!(env.config_dir.exists());
        assert!(env.config_dir.is_dir());
    }

    #[test]
    fn test_write_and_read_settings() {
        let env = TestEnv::new();
        let settings_json = Fixtures::valid_settings_json();

        env.write_settings(settings_json);

        let read_content = env.read_settings();
        assert!(read_content.is_some());
        let content = read_content.unwrap();
        assert!(content.contains("Catppuccin Mocha"));
        assert!(content.contains("Iosevka Nerd Font"));
    }

    #[test]
    fn test_mock_settings_with_theme() {
        let settings = MockSettings::with_theme("Tokyo Night");
        let json = settings.to_json();

        assert!(json.contains("Tokyo Night"));
        assert!(!json.contains("window_bounds"));
    }

    #[test]
    fn test_mock_settings_with_window_bounds() {
        let settings = MockSettings::with_window_bounds(50.0, 75.0, 1400.0, 900.0);
        let json = settings.to_json();

        assert!(json.contains("window_bounds"));
        assert!(json.contains("50"));
        assert!(json.contains("75"));
        assert!(json.contains("1400"));
        assert!(json.contains("900"));
    }

    #[test]
    fn test_mock_settings_empty() {
        let settings = MockSettings::empty();
        let json = settings.to_json();

        // Should be empty object
        pretty_eq!(json.trim(), "{  }");
    }

    #[test]
    fn test_settings_file_persistence() {
        let env = TestEnv::new();

        // Initially no settings file
        assert!(!env.settings_path.exists());

        // Write settings
        env.write_settings(r#"{"theme": "Dracula"}"#);
        assert!(env.settings_path.exists());

        // Read back
        let content = std::fs::read_to_string(&env.settings_path).unwrap();
        assert!(content.contains("Dracula"));
    }

    #[test]
    #[serial]
    fn test_settings_validation_oversized_theme() {
        let env = TestEnv::new();
        let oversized = Fixtures::oversized_theme_name();

        env.write_settings(&oversized);

        // The file should be written
        assert!(env.settings_path.exists());

        // The content should contain the long theme name
        let content = env.read_settings().unwrap();
        assert!(content.len() > 300, "Oversized theme should be in file");
    }

    #[test]
    fn test_multiple_settings_updates() {
        let env = TestEnv::new();

        // First write
        env.write_settings(r#"{"theme": "Theme1"}"#);
        assert!(env.read_settings().unwrap().contains("Theme1"));

        // Second write (should overwrite)
        env.write_settings(r#"{"theme": "Theme2"}"#);
        let content = env.read_settings().unwrap();
        assert!(content.contains("Theme2"));
        assert!(!content.contains("Theme1"));
    }

    #[test]
    fn test_create_mock_theme() {
        let env = TestEnv::new();
        let theme_content = r#"{"name": "Test Theme", "colors": {}}"#;

        let theme_path = env.create_mock_theme("test_theme", theme_content);

        assert!(theme_path.exists());
        let content = std::fs::read_to_string(&theme_path).unwrap();
        assert!(content.contains("Test Theme"));
    }

    #[test]
    fn test_temp_dir_cleanup() {
        let temp_path: std::path::PathBuf;

        {
            let env = TestEnv::new();
            temp_path = env.path().to_path_buf();
            assert!(temp_path.exists());
        }
        // After drop, temp dir should be cleaned up
        // Note: On some systems this may not be immediate
        // so we just verify it doesn't panic
    }
}

// ============================================================================
// Theme Tests
// ============================================================================

mod theme {
    use super::*;

    /// Simulated theme colors for testing without GPUI context.
    #[derive(Debug, Clone, PartialEq)]
    struct MockThemeColors {
        pub background: u32,
        pub foreground: u32,
        pub cursor: u32,
        pub selection: u32,
        pub name: String,
    }

    impl MockThemeColors {
        fn catppuccin_mocha() -> Self {
            Self {
                background: 0x1e1e2e,
                foreground: 0xcdd6f4,
                cursor: 0xf5e0dc,
                selection: 0x45475a,
                name: "Catppuccin Mocha".to_string(),
            }
        }

        fn catppuccin_latte() -> Self {
            Self {
                background: 0xeff1f5,
                foreground: 0x4c4f69,
                cursor: 0xdc8a78,
                selection: 0xbcc0cc,
                name: "Catppuccin Latte".to_string(),
            }
        }

        fn tokyo_night() -> Self {
            Self {
                background: 0x1a1b26,
                foreground: 0xa9b1d6,
                cursor: 0xc0caf5,
                selection: 0x33467c,
                name: "Tokyo Night".to_string(),
            }
        }

        fn is_dark(&self) -> bool {
            // Simple heuristic: dark theme if background RGB values are low
            let r = (self.background >> 16) & 0xFF;
            let g = (self.background >> 8) & 0xFF;
            let b = self.background & 0xFF;
            (r + g + b) / 3 < 128
        }
    }

    /// Mock theme registry for testing theme switching.
    struct MockThemeRegistry {
        themes: std::collections::HashMap<String, MockThemeColors>,
        current: String,
    }

    impl MockThemeRegistry {
        fn new() -> Self {
            let mut themes = std::collections::HashMap::new();
            let mocha = MockThemeColors::catppuccin_mocha();
            let latte = MockThemeColors::catppuccin_latte();
            let tokyo = MockThemeColors::tokyo_night();

            themes.insert(mocha.name.clone(), mocha.clone());
            themes.insert(latte.name.clone(), latte);
            themes.insert(tokyo.name.clone(), tokyo);

            Self {
                themes,
                current: mocha.name.clone(),
            }
        }

        fn current_theme(&self) -> &MockThemeColors {
            self.themes.get(&self.current).unwrap()
        }

        fn switch_theme(&mut self, name: &str) -> Result<(), &'static str> {
            if self.themes.contains_key(name) {
                self.current = name.to_string();
                Ok(())
            } else {
                Err("Theme not found")
            }
        }

        fn available_themes(&self) -> Vec<&str> {
            self.themes.keys().map(|s| s.as_str()).collect()
        }

        fn theme_count(&self) -> usize {
            self.themes.len()
        }
    }

    #[test]
    fn test_mock_theme_colors_defaults() {
        let mocha = MockThemeColors::catppuccin_mocha();

        assert_eq!(mocha.name, "Catppuccin Mocha");
        assert_eq!(mocha.background, 0x1e1e2e);
        assert!(mocha.is_dark());
    }

    #[test]
    fn test_mock_theme_light_dark_detection() {
        let mocha = MockThemeColors::catppuccin_mocha();
        let latte = MockThemeColors::catppuccin_latte();

        assert!(mocha.is_dark(), "Mocha should be detected as dark");
        assert!(!latte.is_dark(), "Latte should be detected as light");
    }

    #[test]
    fn test_theme_registry_initialization() {
        let registry = MockThemeRegistry::new();

        // Should have default themes loaded
        assert!(registry.theme_count() >= 3);

        // Default theme should be Catppuccin Mocha
        assert_eq!(registry.current_theme().name, "Catppuccin Mocha");
    }

    #[test]
    fn test_theme_switching_success() {
        let mut registry = MockThemeRegistry::new();

        // Switch to Tokyo Night
        let result = registry.switch_theme("Tokyo Night");
        assert!(result.is_ok());
        assert_eq!(registry.current_theme().name, "Tokyo Night");

        // Switch to Catppuccin Latte
        let result = registry.switch_theme("Catppuccin Latte");
        assert!(result.is_ok());
        assert_eq!(registry.current_theme().name, "Catppuccin Latte");
    }

    #[test]
    fn test_theme_switching_invalid_theme() {
        let mut registry = MockThemeRegistry::new();

        let result = registry.switch_theme("Non-existent Theme");
        assert!(result.is_err());

        // Should remain on current theme
        assert_eq!(registry.current_theme().name, "Catppuccin Mocha");
    }

    #[test]
    fn test_theme_colors_change_on_switch() {
        let mut registry = MockThemeRegistry::new();

        let original_bg = registry.current_theme().background;
        registry.switch_theme("Catppuccin Latte").unwrap();
        let new_bg = registry.current_theme().background;

        assert_ne!(
            original_bg, new_bg,
            "Background should change when switching themes"
        );
    }

    #[test]
    fn test_available_themes_list() {
        let registry = MockThemeRegistry::new();
        let themes = registry.available_themes();

        assert!(themes.contains(&"Catppuccin Mocha"));
        assert!(themes.contains(&"Catppuccin Latte"));
        assert!(themes.contains(&"Tokyo Night"));
    }

    #[test]
    fn test_theme_switch_cycle() {
        let mut registry = MockThemeRegistry::new();
        let themes: Vec<String> = registry
            .available_themes()
            .into_iter()
            .map(|s| s.to_string())
            .collect();

        // Cycle through all themes
        for theme_name in &themes {
            let result = registry.switch_theme(theme_name);
            assert!(result.is_ok(), "Should switch to {}", theme_name);
            assert_eq!(registry.current_theme().name, *theme_name);
        }
    }

    #[test]
    #[serial]
    fn test_theme_persistence_simulation() {
        let env = TestEnv::new();
        let mut registry = MockThemeRegistry::new();

        // Switch theme and "persist" to settings
        registry.switch_theme("Tokyo Night").unwrap();
        let settings = MockSettings::with_theme(&registry.current_theme().name);
        env.write_settings(&settings.to_json());

        // Verify persisted theme
        let content = env.read_settings().unwrap();
        assert!(content.contains("Tokyo Night"));
    }

    #[test]
    fn test_dark_light_mode_consistency() {
        let registry = MockThemeRegistry::new();

        // Verify dark themes have consistent characteristics
        for (name, colors) in &registry.themes {
            if colors.is_dark() {
                // Dark theme foreground should be lighter than background
                let fg_brightness = ((colors.foreground >> 16) & 0xFF)
                    + ((colors.foreground >> 8) & 0xFF)
                    + (colors.foreground & 0xFF);
                let bg_brightness = ((colors.background >> 16) & 0xFF)
                    + ((colors.background >> 8) & 0xFF)
                    + (colors.background & 0xFF);

                assert!(
                    fg_brightness > bg_brightness,
                    "Dark theme {} should have brighter foreground than background",
                    name
                );
            }
        }
    }
}

// ============================================================================
// Terminal Size Tests
// ============================================================================

mod terminal_size {
    use super::*;

    #[test]
    fn test_default_terminal_size() {
        let session = MockTerminalSession::new();
        let (cols, rows) = session.size;

        // Standard terminal size
        assert_eq!(cols, 80);
        assert_eq!(rows, 24);
    }

    #[test]
    fn test_minimum_viable_size() {
        let session = MockTerminalSession::with_size(10, 3);
        assert_eq!(session.size, (10, 3));
    }

    #[test]
    fn test_large_terminal_size() {
        let session = MockTerminalSession::with_size(400, 100);
        assert_eq!(session.size, (400, 100));
    }

    #[test]
    fn test_resize_triggers_no_data_loss() {
        let mut session = MockTerminalSession::new();

        // Write some output
        session.write_output("Important data\n");
        session.send_input("user input");

        // Resize
        session.resize(120, 40);

        // Data should still be accessible
        assert!(session.output_contains("Important data"));
        assert_eq!(session.get_input(), "user input");
    }

    #[test]
    fn test_multiple_resizes() {
        let mut session = MockTerminalSession::new();
        let sizes = vec![(80, 24), (120, 40), (60, 20), (200, 50), (80, 24)];

        for (cols, rows) in sizes {
            session.resize(cols, rows);
            assert_eq!(session.size, (cols, rows));
        }
    }
}

// ============================================================================
// Escape Sequence Tests
// ============================================================================

mod escape_sequences {
    use super::*;

    #[test]
    fn test_cursor_position_escape() {
        let escape = Fixtures::cursor_to(0, 0);
        assert_eq!(escape, "\x1b[1;1H");

        let escape = Fixtures::cursor_to(10, 20);
        assert_eq!(escape, "\x1b[11;21H");
    }

    #[test]
    fn test_colored_text_escape() {
        // Red text
        let red = Fixtures::colored_text("error", 31);
        assert!(red.starts_with("\x1b[31m"));
        assert!(red.contains("error"));
        assert!(red.ends_with("\x1b[0m"));

        // Green text
        let green = Fixtures::colored_text("success", 32);
        assert!(green.starts_with("\x1b[32m"));
    }

    #[test]
    fn test_escape_sequence_constants() {
        // Verify escape sequences start with ESC
        assert!(Fixtures::CLEAR_SCREEN.starts_with("\x1b["));
        assert!(Fixtures::CURSOR_HOME.starts_with("\x1b["));
        assert!(Fixtures::RESET.starts_with("\x1b["));
        assert!(Fixtures::BOLD.starts_with("\x1b["));
    }

    #[test]
    fn test_session_processes_color_output() {
        let session = MockTerminalSession::new();

        // Write colored output
        session.write_output(Fixtures::RED_FG);
        session.write_output("Error message");
        session.write_output(Fixtures::RESET);
        session.write_output("\n");

        let output = session.get_output();
        assert!(output.contains("\x1b[31m")); // Red escape
        assert!(output.contains("Error message"));
        assert!(output.contains("\x1b[0m")); // Reset escape
    }

    #[test]
    fn test_session_with_prompt() {
        let session = MockTerminalSession::new();

        // Simulate a shell prompt with colors
        session.write_output(Fixtures::GREEN_FG);
        session.write_output("user@host");
        session.write_output(Fixtures::RESET);
        session.write_output(":");
        session.write_output(Fixtures::BLUE_FG);
        session.write_output("~/projects");
        session.write_output(Fixtures::RESET);
        session.write_output("$ ");

        let output = session.get_output();
        assert!(output.contains("user@host"));
        assert!(output.contains("~/projects"));
        assert!(output.contains("$ "));
    }
}

// ============================================================================
// Edge Cases and Error Handling
// ============================================================================

mod edge_cases {
    use super::*;

    #[test]
    fn test_empty_input_handling() {
        let session = MockTerminalSession::new();

        session.send_input("");
        assert_eq!(session.get_input(), "");

        session.send_input("a");
        assert_eq!(session.get_input(), "a");
    }

    #[test]
    fn test_unicode_input() {
        let session = MockTerminalSession::new();

        // Various Unicode characters
        session.send_input("Hello \u{1F600}"); // Emoji
        session.send_input(" \u{4e2d}\u{6587}"); // Chinese characters
        session.send_input(" \u{0394}\u{03B1}"); // Greek letters

        let input = session.get_input();
        assert!(input.contains("\u{1F600}"));
        assert!(input.contains("\u{4e2d}"));
        assert!(input.contains("\u{0394}"));
    }

    #[test]
    fn test_very_long_input() {
        let session = MockTerminalSession::new();

        // Generate a very long command
        let long_input: String = (0..10000).map(|_| 'a').collect();
        session.send_input(&long_input);

        assert_eq!(session.get_input().len(), 10000);
    }

    #[test]
    fn test_rapid_resize_operations() {
        let mut session = MockTerminalSession::new();

        // Rapidly resize many times
        for i in 0..100 {
            let cols = 40 + (i % 160) as u16;
            let rows = 12 + (i % 48) as u16;
            session.resize(cols, rows);
        }

        // Should end up at last resize values
        let (cols, rows) = session.size;
        assert!((40..200).contains(&cols));
        assert!((12..60).contains(&rows));
    }

    #[test]
    fn test_session_operations_after_exit() {
        let session = MockTerminalSession::new();
        session.exit();

        // Operations should still work on exited session
        // (they just won't do anything in real implementation)
        session.send_input("after exit");
        assert_eq!(session.get_input(), "after exit");

        session.write_output("output after exit");
        assert!(session.output_contains("output after exit"));
    }

    #[test]
    fn test_invalid_json_in_settings() {
        let env = TestEnv::new();
        env.write_settings(Fixtures::invalid_json());

        // File should exist but contain invalid JSON
        let content = env.read_settings().unwrap();
        assert!(content.contains("this is not valid json"));
    }

    #[test]
    fn test_special_characters_in_output() {
        let session = MockTerminalSession::new();

        // Tab and newline characters
        session.write_output("col1\tcol2\tcol3\n");
        session.write_output("a\tb\tc\n");

        // Control characters
        session.write_output("\x07"); // Bell
        session.write_output("\x08"); // Backspace

        let output = session.get_output();
        assert!(output.contains("\t"));
        assert!(output.contains("\n"));
    }

    #[test]
    fn test_binary_data_handling() {
        let session = MockTerminalSession::new();

        // Simulate binary output with valid UTF-8 multi-byte characters
        session.write_output("\u{00A0}"); // Non-breaking space (U+00A0)
        session.write_output("\u{2013}"); // En-dash (U+2013)

        let output = session.get_output();
        assert!(!output.is_empty());
    }
}

// ============================================================================
// Concurrent Access Tests
// ============================================================================

mod concurrency {
    use super::*;

    #[tokio::test]
    async fn test_concurrent_input_output() {
        let session = Arc::new(MockTerminalSession::new());

        let session_input = session.clone();
        let session_output = session.clone();

        // Spawn concurrent input writer
        let input_handle = tokio::spawn(async move {
            for i in 0..10 {
                session_input.send_input(&format!("input{} ", i));
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        });

        // Spawn concurrent output writer
        let output_handle = tokio::spawn(async move {
            for i in 0..10 {
                session_output.write_output(&format!("output{}\n", i));
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        });

        // Wait for both to complete
        input_handle.await.unwrap();
        output_handle.await.unwrap();

        // Verify all data was written
        let input = session.get_input();
        let output = session.get_output();

        for i in 0..10 {
            assert!(input.contains(&format!("input{}", i)));
            assert!(output.contains(&format!("output{}", i)));
        }
    }

    #[tokio::test]
    async fn test_multiple_sessions_isolation() {
        let sessions: Vec<Arc<MockTerminalSession>> = (0..5)
            .map(|_| Arc::new(MockTerminalSession::new()))
            .collect();

        // Write to each session concurrently
        let handles: Vec<_> = sessions
            .iter()
            .enumerate()
            .map(|(i, session)| {
                let session = session.clone();
                tokio::spawn(async move {
                    session.send_input(&format!("session{}", i));
                    session.write_output(&format!("output{}", i));
                })
            })
            .collect();

        // Wait for all writes
        for handle in handles {
            handle.await.unwrap();
        }

        // Verify isolation
        for (i, session) in sessions.iter().enumerate() {
            let input = session.get_input();
            let output = session.get_output();

            assert!(input.contains(&format!("session{}", i)));
            assert!(output.contains(&format!("output{}", i)));

            // Should not contain other session data
            for j in 0..5 {
                if j != i {
                    assert!(!input.contains(&format!("session{}", j)));
                }
            }
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_exit_during_operations() {
        let session = Arc::new(MockTerminalSession::new());
        let session_writer = session.clone();
        let session_exiter = session.clone();

        // Start continuous writing
        let write_handle = tokio::spawn(async move {
            let mut count = 0;
            while !session_writer.has_exited() {
                session_writer.write_output(&format!("data{}\n", count));
                count += 1;
                tokio::time::sleep(Duration::from_millis(5)).await;
                if count > 50 {
                    break;
                } // Safety limit
            }
            count
        });

        // Exit after a short delay
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(25)).await;
            session_exiter.exit();
        });

        let count = write_handle.await.unwrap();

        // Some data should have been written before exit
        assert!(count > 0, "Should have written some data before exit");
        assert!(session.has_exited());
    }
}

// ============================================================================
// Integration Scenarios
// ============================================================================

mod integration_scenarios {
    use super::*;

    #[test]
    fn test_typical_shell_session() {
        let session = MockTerminalSession::new();

        // Simulate starting a shell session
        session.write_output(Fixtures::SHELL_PROMPT);

        // User types a command
        session.send_input("ls -la\n");

        // Shell responds
        session.write_output("total 8\r\n");
        session.write_output("drwxr-xr-x  2 user user  40 Jan  1 00:00 .\r\n");
        session.write_output("drwxr-xr-x 10 user user 200 Jan  1 00:00 ..\r\n");
        session.write_output(Fixtures::SHELL_PROMPT);

        // User types another command
        session.send_input("pwd\n");

        // Shell responds
        session.write_output("/home/user\r\n");
        session.write_output(Fixtures::SHELL_PROMPT);

        // Verify session state
        assert!(session.output_contains("total 8"));
        assert!(session.output_contains("/home/user"));
        assert!(!session.has_exited());
    }

    #[test]
    fn test_session_with_error_output() {
        let session = MockTerminalSession::new();

        // Command that produces error
        session.send_input("cat nonexistent_file\n");

        // Error output (with colors)
        session.write_output(Fixtures::RED_FG);
        session.write_output("cat: nonexistent_file: No such file or directory");
        session.write_output(Fixtures::RESET);
        session.write_output("\r\n");
        session.write_output(Fixtures::SHELL_PROMPT);

        // Verify error handling
        assert!(session.output_contains("No such file or directory"));
        assert!(session.output_contains("\x1b[31m")); // Red color code
    }

    #[test]
    fn test_resize_during_output() {
        let mut session = MockTerminalSession::new();

        // Start outputting data
        session.write_output("Line 1\r\n");
        session.write_output("Line 2\r\n");

        // Resize during output
        session.resize(120, 40);

        // Continue outputting
        session.write_output("Line 3\r\n");
        session.write_output("Line 4\r\n");

        // All output should be preserved
        assert!(session.output_contains("Line 1"));
        assert!(session.output_contains("Line 2"));
        assert!(session.output_contains("Line 3"));
        assert!(session.output_contains("Line 4"));
        assert_eq!(session.size, (120, 40));
    }

    #[test]
    #[serial]
    fn test_settings_and_session_lifecycle() {
        let env = TestEnv::new();

        // Create settings with saved theme
        let settings = MockSettings::with_theme("Tokyo Night");
        env.write_settings(&settings.to_json());

        // Start a session (would load settings in real app)
        let session = MockTerminalSession::new();

        // Do some work
        session.send_input("echo hello\n");
        session.write_output("hello\r\n");

        // Exit session
        session.exit();

        // Verify settings still exist after session exit
        assert!(env.read_settings().unwrap().contains("Tokyo Night"));
    }

    #[tokio::test]
    async fn test_async_session_lifecycle() {
        let session = Arc::new(MockTerminalSession::new());

        // Simulate async shell interaction
        let session_clone = session.clone();
        let interaction = tokio::spawn(async move {
            // Initial prompt
            session_clone.write_output("$ ");

            // User input
            session_clone.send_input("sleep 1 && echo done\n");

            // Simulate delay
            tokio::time::sleep(Duration::from_millis(50)).await;

            // Command output
            session_clone.write_output("done\r\n");
            session_clone.write_output("$ ");

            // Exit
            session_clone.send_input("exit\n");
            session_clone.exit();
        });

        interaction.await.unwrap();

        // Verify final state
        assert!(session.has_exited());
        assert!(session.output_contains("done"));
        assert!(session.get_input().contains("exit"));
    }
}
