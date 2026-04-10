use super::*;
use gpui::TestAppContext;

fn init_test_context(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
    });
}

#[gpui::test]
fn test_workspace_creation(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    cx.read(|app| {
        let ws = workspace.read(app);
        assert_eq!(ws.tabs.len(), 1, "Workspace should start with one tab");
        assert_eq!(ws.active_tab, 0, "Active tab should be 0");
        assert!(ws.pending_action.is_none(), "Should have no pending action");

        let first_tab = &ws.tabs[0];
        assert_eq!(
            first_tab.panes.all_panes().len(),
            1,
            "First tab should have one pane"
        );
        assert_eq!(
            first_tab.fallback_title.as_ref(),
            "Terminal 1",
            "First tab should be named 'Terminal 1'"
        );
    });
}

#[gpui::test]
fn test_workspace_cached_titles_initialized(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    cx.read(|app| {
        let ws = workspace.read(app);
        assert_eq!(
            ws.cached_titles.len(),
            1,
            "Cached titles should be initialized with one title"
        );
        assert!(
            !ws.cached_titles[0].is_empty(),
            "First cached title should not be empty"
        );
    });
}

#[gpui::test]
fn test_new_tab(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    workspace.update(cx, |ws, cx| {
        ws.new_tab(cx);
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        assert_eq!(ws.tabs.len(), 2, "Should have 2 tabs after adding one");
        assert_eq!(ws.active_tab, 1, "Active tab should be the new tab");
        assert_eq!(
            ws.tabs[1].fallback_title.as_ref(),
            "Terminal 2",
            "Second tab should be named 'Terminal 2'"
        );
    });
}

#[gpui::test]
fn test_multiple_new_tabs(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    workspace.update(cx, |ws, cx| {
        ws.new_tab(cx);
        ws.new_tab(cx);
        ws.new_tab(cx);
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        assert_eq!(ws.tabs.len(), 4, "Should have 4 tabs");
        assert_eq!(ws.active_tab, 3, "Active tab should be index 3");
        assert_eq!(ws.tabs[0].fallback_title.as_ref(), "Terminal 1");
        assert_eq!(ws.tabs[1].fallback_title.as_ref(), "Terminal 2");
        assert_eq!(ws.tabs[2].fallback_title.as_ref(), "Terminal 3");
        assert_eq!(ws.tabs[3].fallback_title.as_ref(), "Terminal 4");
    });
}

#[gpui::test]
fn test_switch_tab(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    workspace.update(cx, |ws, cx| {
        ws.new_tab(cx);
        ws.new_tab(cx);
    });

    workspace.update(cx, |ws, cx| {
        ws.switch_tab(0, cx);
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        assert_eq!(ws.active_tab, 0, "Active tab should be 0 after switch");
    });

    workspace.update(cx, |ws, cx| {
        ws.switch_tab(1, cx);
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        assert_eq!(ws.active_tab, 1, "Active tab should be 1 after switch");
    });
}

#[gpui::test]
fn test_switch_tab_out_of_bounds(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    workspace.update(cx, |ws, cx| {
        ws.switch_tab(100, cx);
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        assert_eq!(
            ws.active_tab, 0,
            "Active tab should remain 0 for invalid index"
        );
    });
}

#[gpui::test]
fn test_next_tab(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    workspace.update(cx, |ws, cx| {
        ws.new_tab(cx);
        ws.new_tab(cx);
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        assert_eq!(ws.active_tab, 2, "Should start at tab 2");
    });

    workspace.update(cx, |ws, cx| {
        ws.next_tab(cx);
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        assert_eq!(ws.active_tab, 0, "Should wrap around to tab 0");
    });

    workspace.update(cx, |ws, cx| {
        ws.next_tab(cx);
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        assert_eq!(ws.active_tab, 1, "Should be at tab 1");
    });
}

#[gpui::test]
fn test_prev_tab(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    workspace.update(cx, |ws, cx| {
        ws.new_tab(cx);
        ws.new_tab(cx);
        ws.switch_tab(0, cx);
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        assert_eq!(ws.active_tab, 0, "Should start at tab 0");
    });

    workspace.update(cx, |ws, cx| {
        ws.prev_tab(cx);
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        assert_eq!(ws.active_tab, 2, "Should wrap around to last tab");
    });

    workspace.update(cx, |ws, cx| {
        ws.prev_tab(cx);
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        assert_eq!(ws.active_tab, 1, "Should be at tab 1");
    });
}

#[gpui::test]
fn test_close_tab_with_multiple_tabs(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    workspace.update(cx, |ws, cx| {
        ws.new_tab(cx);
        ws.new_tab(cx);
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        assert_eq!(ws.tabs.len(), 3, "Should have 3 tabs");
    });

    workspace.update(cx, |ws, cx| {
        ws.close_tab(1, cx);
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        assert_eq!(ws.tabs.len(), 2, "Should have 2 tabs after closing one");
    });
}

#[gpui::test]
fn test_close_active_tab_adjusts_index(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    workspace.update(cx, |ws, cx| {
        ws.new_tab(cx);
        ws.new_tab(cx);
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        assert_eq!(ws.active_tab, 2, "Should be on tab 2");
    });

    workspace.update(cx, |ws, cx| {
        ws.close_tab(2, cx);
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        assert_eq!(ws.active_tab, 1, "Active tab should adjust to 1");
        assert_eq!(ws.tabs.len(), 2, "Should have 2 tabs");
    });
}

#[gpui::test]
fn test_set_active_pane(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    let first_pane_id = cx.read(|app| {
        let ws = workspace.read(app);
        let tab = &ws.tabs[0];
        tab.panes.first_leaf_id()
    });

    workspace.update(cx, |ws, cx| {
        ws.set_active_pane(first_pane_id, cx);
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        let tab = &ws.tabs[0];
        assert_eq!(
            tab.active_pane, first_pane_id,
            "Active pane should be set to first pane"
        );
    });
}

#[gpui::test]
fn test_cancel_pending_action(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    workspace.update(cx, |ws, cx| {
        ws.pending_action = Some(PendingAction::Quit);
        ws.pending_process_name = Some("test_process".to_string());
        cx.notify();
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        assert!(ws.pending_action.is_some(), "Should have pending action");
        assert!(
            ws.pending_process_name.is_some(),
            "Should have process name"
        );
    });

    workspace.update(cx, |ws, cx| {
        ws.cancel_pending_action(cx);
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        assert!(
            ws.pending_action.is_none(),
            "Pending action should be cleared"
        );
        assert!(
            ws.pending_process_name.is_none(),
            "Process name should be cleared"
        );
    });
}

#[gpui::test]
fn test_pending_action_states(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    workspace.update(cx, |ws, _| {
        ws.pending_action = Some(PendingAction::ClosePane);
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        assert_eq!(ws.pending_action, Some(PendingAction::ClosePane));
    });

    workspace.update(cx, |ws, _| {
        ws.pending_action = Some(PendingAction::CloseTab(0));
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        assert_eq!(ws.pending_action, Some(PendingAction::CloseTab(0)));
    });

    workspace.update(cx, |ws, _| {
        ws.pending_action = Some(PendingAction::Quit);
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        assert_eq!(ws.pending_action, Some(PendingAction::Quit));
    });
}

#[gpui::test]
fn test_get_tab_titles(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    workspace.update(cx, |ws, cx| {
        ws.new_tab(cx);
        ws.new_tab(cx);
    });

    cx.update(|app| {
        workspace.update(app, |ws, cx| {
            let titles = ws.get_tab_titles(cx);
            assert_eq!(titles.len(), 3, "Should have 3 titles");
            for title in &titles {
                assert!(!title.is_empty(), "Tab title should not be empty");
            }
        });
    });
}

#[gpui::test]
fn test_tab_titles_cached(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    cx.update(|app| {
        workspace.update(app, |ws, cx| {
            let _ = ws.get_tab_titles(cx);
        });
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        assert_eq!(ws.cached_titles.len(), 1);
    });

    cx.update(|app| {
        workspace.update(app, |ws, cx| {
            let titles = ws.get_tab_titles(cx);
            assert_eq!(titles.len(), 1);
        });
    });
}

#[gpui::test]
fn test_single_tab_operations(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    workspace.update(cx, |ws, cx| {
        ws.next_tab(cx);
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        assert_eq!(ws.active_tab, 0, "Should stay on tab 0 with single tab");
    });

    workspace.update(cx, |ws, cx| {
        ws.prev_tab(cx);
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        assert_eq!(ws.active_tab, 0, "Should stay on tab 0 with single tab");
    });
}

#[gpui::test]
fn test_tab_ids_are_unique(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    workspace.update(cx, |ws, cx| {
        ws.new_tab(cx);
        ws.new_tab(cx);
        ws.new_tab(cx);
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        let ids: Vec<Uuid> = ws.tabs.iter().map(|t| t.id).collect();

        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                assert_ne!(ids[i], ids[j], "Tab IDs should be unique");
            }
        }
    });
}

#[gpui::test]
fn test_pane_tree_first_leaf_id(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    let first_id = cx.read(|app| {
        let ws = workspace.read(app);
        ws.tabs[0].panes.first_leaf_id()
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        let second_read_id = ws.tabs[0].panes.first_leaf_id();
        assert_eq!(
            first_id, second_read_id,
            "First leaf ID should be consistent"
        );
    });
}

#[gpui::test]
fn test_find_pane_by_id(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    let pane_id = cx.read(|app| {
        let ws = workspace.read(app);
        ws.tabs[0].panes.first_leaf_id()
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        let found = ws.tabs[0].panes.find_pane(pane_id);
        assert!(found.is_some(), "Should find pane by ID");
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        let random_id = Uuid::new_v4();
        let found = ws.tabs[0].panes.find_pane(random_id);
        assert!(found.is_none(), "Should not find random UUID");
    });
}

#[gpui::test]
fn test_rapid_tab_creation(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    const NUM_TABS: usize = 50;

    workspace.update(cx, |ws, cx| {
        for _ in 0..NUM_TABS {
            ws.new_tab(cx);
        }
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        assert_eq!(
            ws.tabs.len(),
            NUM_TABS + 1,
            "All tabs should be created"
        );
        assert_eq!(
            ws.active_tab, NUM_TABS,
            "Active tab should be the last created"
        );
    });
}

#[gpui::test]
fn test_rapid_tab_deletion(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    const NUM_TABS: usize = 20;

    workspace.update(cx, |ws, cx| {
        for _ in 0..NUM_TABS {
            ws.new_tab(cx);
        }
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        assert_eq!(ws.tabs.len(), NUM_TABS + 1);
    });

    workspace.update(cx, |ws, cx| {
        while ws.tabs.len() > 1 {
            let last_idx = ws.tabs.len() - 1;
            ws.close_tab(last_idx, cx);
        }
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        assert_eq!(ws.tabs.len(), 1, "Should have one tab remaining");
        assert_eq!(ws.active_tab, 0, "Active tab should be 0");
    });
}

#[gpui::test]
fn test_interleaved_create_delete(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    workspace.update(cx, |ws, cx| {
        for _ in 0..5 {
            ws.new_tab(cx);
        }
        ws.close_tab(3, cx);
        ws.close_tab(2, cx);
        for _ in 0..3 {
            ws.new_tab(cx);
        }
        ws.close_tab(4, cx);
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        assert_eq!(
            ws.tabs.len(),
            6,
            "Tab count should be correct after interleaved operations"
        );
    });
}

#[gpui::test]
fn test_rapid_focus_changes(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    workspace.update(cx, |ws, cx| {
        for _ in 0..10 {
            ws.new_tab(cx);
        }
    });

    workspace.update(cx, |ws, cx| {
        for i in 0..100 {
            ws.switch_tab(i % 11, cx);
        }
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        assert_eq!(
            ws.active_tab,
            99 % 11,
            "Active tab should be correct after rapid switches"
        );
    });
}

#[gpui::test]
fn test_rapid_next_prev_tab(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    workspace.update(cx, |ws, cx| {
        for _ in 0..4 {
            ws.new_tab(cx);
        }
    });

    workspace.update(cx, |ws, cx| {
        for _ in 0..50 {
            ws.next_tab(cx);
        }
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        assert_eq!(ws.active_tab, 4, "Should wrap around correctly");
    });

    workspace.update(cx, |ws, cx| {
        for _ in 0..50 {
            ws.prev_tab(cx);
        }
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        assert_eq!(ws.active_tab, 4, "Should wrap around correctly backwards");
    });
}

#[gpui::test]
fn test_delete_while_iterating(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    workspace.update(cx, |ws, cx| {
        for _ in 0..10 {
            ws.new_tab(cx);
        }
        ws.switch_tab(5, cx);
    });

    workspace.update(cx, |ws, cx| {
        ws.close_tab(2, cx);
        ws.close_tab(1, cx);
        ws.close_tab(0, cx);
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        assert!(ws.active_tab < ws.tabs.len(), "Active tab should be valid");
    });
}

#[gpui::test]
fn test_pending_action_state_transitions(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    workspace.update(cx, |ws, cx| {
        for i in 0..100 {
            match i % 4 {
                0 => {
                    ws.pending_action = Some(PendingAction::ClosePane);
                    ws.pending_process_name = Some(format!("proc-{}", i));
                }
                1 => {
                    ws.pending_action = Some(PendingAction::CloseTab(i % 10));
                    ws.pending_process_name = Some(format!("proc-{}", i));
                }
                2 => {
                    ws.pending_action = Some(PendingAction::Quit);
                    ws.pending_process_name = Some("important-process".to_string());
                }
                3 => {
                    ws.cancel_pending_action(cx);
                }
                _ => unreachable!(),
            }
        }
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        assert!(
            ws.pending_action.is_none(),
            "Pending action should be cleared"
        );
        assert!(
            ws.pending_process_name.is_none(),
            "Process name should be cleared"
        );
    });
}

#[gpui::test]
fn test_cache_invalidation_under_load(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    for _ in 0..10 {
        workspace.update(cx, |ws, cx| {
            ws.new_tab(cx);
            ws.new_tab(cx);
            ws.new_tab(cx);
        });

        cx.update(|app| {
            workspace.update(app, |ws, cx| {
                let titles = ws.get_tab_titles(cx);
                assert_eq!(
                    titles.len(),
                    ws.tabs.len(),
                    "Title cache should match tab count"
                );
            });
        });

        workspace.update(cx, |ws, cx| {
            if ws.tabs.len() > 2 {
                ws.close_tab(ws.tabs.len() - 1, cx);
            }
        });
    }
}

#[gpui::test]
fn test_tab_id_uniqueness_under_stress(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    let mut all_ids: std::collections::HashSet<Uuid> = std::collections::HashSet::new();

    cx.read(|app| {
        let ws = workspace.read(app);
        for tab in &ws.tabs {
            all_ids.insert(tab.id);
        }
    });

    for _cycle in 0..20 {
        workspace.update(cx, |ws, cx| {
            for _ in 0..5 {
                ws.new_tab(cx);
            }
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            for tab in &ws.tabs {
                all_ids.insert(tab.id);
            }
        });

        workspace.update(cx, |ws, cx| {
            while ws.tabs.len() > 2 {
                ws.close_tab(1, cx);
            }
        });
    }

    assert!(
        all_ids.len() >= 100,
        "Should have accumulated many unique tab IDs: got {}",
        all_ids.len()
    );
}

#[gpui::test]
fn test_active_pane_consistency(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    workspace.update(cx, |ws, cx| {
        for _ in 0..10 {
            ws.new_tab(cx);
        }
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        for (i, tab) in ws.tabs.iter().enumerate() {
            let pane = tab.panes.find_pane(tab.active_pane);
            assert!(pane.is_some(), "Tab {} should have a valid active_pane", i);
        }
    });

    for target_tab in 0..10 {
        workspace.update(cx, |ws, cx| {
            ws.switch_tab(target_tab, cx);
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            let tab = &ws.tabs[ws.active_tab];
            let pane = tab.panes.find_pane(tab.active_pane);
            assert!(
                pane.is_some(),
                "Active tab should have valid active_pane after switch to {}",
                target_tab
            );
        });
    }
}

#[gpui::test]
#[ignore]
fn stress_tab_operations(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    const ITERATIONS: usize = 100;
    const TABS_PER_ITERATION: usize = 10;

    for iteration in 0..ITERATIONS {
        workspace.update(cx, |ws, cx| {
            for _ in 0..TABS_PER_ITERATION {
                ws.new_tab(cx);
            }
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            assert!(
                ws.tabs.len() <= (iteration + 1) * TABS_PER_ITERATION + 1,
                "Tab count should be bounded"
            );
        });

        workspace.update(cx, |ws, cx| {
            let to_delete = ws.tabs.len() / 2;
            for _ in 0..to_delete {
                if ws.tabs.len() > 1 {
                    ws.close_tab(1, cx);
                }
            }
        });

        workspace.update(cx, |ws, cx| {
            for _ in 0..50 {
                ws.next_tab(cx);
                ws.prev_tab(cx);
            }
        });
    }

    cx.read(|app| {
        let ws = workspace.read(app);
        assert!(!ws.tabs.is_empty(), "Should have at least one tab");
        assert!(ws.active_tab < ws.tabs.len(), "Active tab should be valid");
    });
}

#[gpui::test]
#[ignore]
fn stress_focus_consistency(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    const NUM_TABS: usize = 50;
    const FOCUS_CYCLES: usize = 1000;

    workspace.update(cx, |ws, cx| {
        for _ in 0..NUM_TABS {
            ws.new_tab(cx);
        }
    });

    workspace.update(cx, |ws, cx| {
        for i in 0..FOCUS_CYCLES {
            match i % 3 {
                0 => ws.next_tab(cx),
                1 => ws.prev_tab(cx),
                2 => ws.switch_tab(i % (NUM_TABS + 1), cx),
                _ => unreachable!(),
            }
        }
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        for (i, tab) in ws.tabs.iter().enumerate() {
            let pane = tab.panes.find_pane(tab.active_pane);
            assert!(
                pane.is_some(),
                "Tab {} should have valid pane after stress test",
                i
            );
        }
    });
}

#[gpui::test]
#[ignore]
fn stress_pending_actions(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    const ITERATIONS: usize = 1000;

    workspace.update(cx, |ws, cx| {
        for i in 0..ITERATIONS {
            ws.pending_action = Some(match i % 3 {
                0 => PendingAction::ClosePane,
                1 => PendingAction::CloseTab(i % 10),
                2 => PendingAction::Quit,
                _ => unreachable!(),
            });
            ws.pending_process_name = Some(format!("process-{}", i));

            if i % 7 == 0 {
                ws.cancel_pending_action(cx);
            }
        }
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        if ws.pending_action.is_none() {
            assert!(
                ws.pending_process_name.is_none(),
                "Process name should be cleared with pending action"
            );
        }
    });
}

#[gpui::test]
#[ignore]
fn stress_mixed_operations(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    const ITERATIONS: usize = 500;

    for i in 0..ITERATIONS {
        workspace.update(cx, |ws, cx| {
            match i % 10 {
                0..=2 => ws.new_tab(cx),
                3 | 4 => {
                    if ws.tabs.len() > 1 {
                        ws.close_tab(ws.tabs.len() - 1, cx);
                    }
                }
                5 | 6 => ws.next_tab(cx),
                7 | 8 => ws.prev_tab(cx),
                9 => {
                    ws.pending_action = Some(PendingAction::ClosePane);
                    ws.cancel_pending_action(cx);
                }
                _ => unreachable!(),
            }
        });

        if i % 50 == 0 {
            cx.update(|app| {
                workspace.update(app, |ws, cx| {
                    let titles = ws.get_tab_titles(cx);
                    assert_eq!(
                        titles.len(),
                        ws.tabs.len(),
                        "Cache should match tabs at iteration {}",
                        i
                    );
                });
            });
        }
    }

    cx.read(|app| {
        let ws = workspace.read(app);
        assert!(!ws.tabs.is_empty(), "Should have at least one tab");
        for tab in &ws.tabs {
            assert!(
                tab.panes.find_pane(tab.active_pane).is_some(),
                "All tabs should have valid panes"
            );
        }
    });
}

#[gpui::test]
fn test_workspace_cannot_have_zero_tabs(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    cx.read(|app| {
        let ws = workspace.read(app);
        assert!(
            !ws.tabs.is_empty(),
            "Workspace must always have at least 1 tab"
        );
        assert_eq!(ws.tabs.len(), 1, "New workspace starts with exactly 1 tab");
    });
}

#[gpui::test]
fn test_single_tab_next_tab_wraps(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    workspace.update(cx, |ws, cx| {
        assert_eq!(ws.tabs.len(), 1);
        ws.next_tab(cx);
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        assert_eq!(ws.active_tab, 0, "Single tab: next_tab(0) should wrap to 0");
    });
}

#[gpui::test]
fn test_single_tab_prev_tab_wraps(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    workspace.update(cx, |ws, cx| {
        assert_eq!(ws.tabs.len(), 1);
        ws.prev_tab(cx);
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        assert_eq!(ws.active_tab, 0, "Single tab: prev_tab(0) should wrap to 0");
    });
}

#[gpui::test]
fn test_single_tab_switch_to_0(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    workspace.update(cx, |ws, cx| {
        ws.switch_tab(0, cx);
    });

    cx.read(|app| {
        assert_eq!(workspace.read(app).active_tab, 0);
    });
}

#[gpui::test]
fn test_single_tab_switch_beyond_bounds_noop(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    workspace.update(cx, |ws, cx| {
        ws.switch_tab(1, cx);
    });

    cx.read(|app| {
        assert_eq!(
            workspace.read(app).active_tab,
            0,
            "switch_tab(1) on single tab should be no-op"
        );
    });
}

#[gpui::test]
fn test_many_tabs_100_creation(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    workspace.update(cx, |ws, cx| {
        for _ in 0..99 {
            ws.new_tab(cx);
        }
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        assert_eq!(ws.tabs.len(), 100, "Should have exactly 100 tabs");
        assert_eq!(ws.active_tab, 99, "Active tab should be last (99)");
    });
}

#[gpui::test]
fn test_many_tabs_navigation_wrapping(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    workspace.update(cx, |ws, cx| {
        for _ in 0..9 {
            ws.new_tab(cx);
        }
    });

    workspace.update(cx, |ws, cx| ws.next_tab(cx));
    cx.read(|app| {
        assert_eq!(
            workspace.read(app).active_tab,
            0,
            "next from 9 should wrap to 0"
        );
    });

    workspace.update(cx, |ws, cx| ws.prev_tab(cx));
    cx.read(|app| {
        assert_eq!(
            workspace.read(app).active_tab,
            9,
            "prev from 0 should wrap to 9"
        );
    });
}

#[gpui::test]
fn test_tab_index_0_operations(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    workspace.update(cx, |ws, cx| {
        ws.new_tab(cx);
        ws.new_tab(cx);
        ws.new_tab(cx);
    });

    workspace.update(cx, |ws, cx| ws.switch_tab(0, cx));
    cx.read(|app| {
        assert_eq!(workspace.read(app).active_tab, 0);
    });

    workspace.update(cx, |ws, cx| ws.close_tab(0, cx));
    cx.read(|app| {
        let ws = workspace.read(app);
        assert_eq!(ws.tabs.len(), 3);
        assert!(ws.active_tab < ws.tabs.len());
    });
}

#[gpui::test]
fn test_tab_index_last_operations(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    workspace.update(cx, |ws, cx| {
        ws.new_tab(cx);
        ws.new_tab(cx);
        ws.new_tab(cx);
        ws.switch_tab(0, cx);
    });

    workspace.update(cx, |ws, cx| ws.switch_tab(3, cx));
    cx.read(|app| {
        assert_eq!(workspace.read(app).active_tab, 3);
    });

    workspace.update(cx, |ws, cx| ws.close_tab(3, cx));
    cx.read(|app| {
        let ws = workspace.read(app);
        assert_eq!(ws.tabs.len(), 3);
        assert_eq!(ws.active_tab, 2);
    });
}

#[gpui::test]
fn test_tab_index_beyond_last_switch_noop(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    workspace.update(cx, |ws, cx| {
        ws.new_tab(cx);
        ws.new_tab(cx);
    });

    let original = cx.read(|app| workspace.read(app).active_tab);

    workspace.update(cx, |ws, cx| ws.switch_tab(3, cx));
    cx.read(|app| {
        assert_eq!(
            workspace.read(app).active_tab,
            original,
            "switch_tab beyond last should be no-op"
        );
    });

    workspace.update(cx, |ws, cx| ws.switch_tab(100, cx));
    cx.read(|app| {
        assert_eq!(workspace.read(app).active_tab, original);
    });

    workspace.update(cx, |ws, cx| ws.switch_tab(usize::MAX, cx));
    cx.read(|app| {
        assert_eq!(workspace.read(app).active_tab, original);
    });
}

#[gpui::test]
fn test_tab_fallback_titles_never_empty(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    workspace.update(cx, |ws, cx| {
        for _ in 0..20 {
            ws.new_tab(cx);
        }
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        for (i, tab) in ws.tabs.iter().enumerate() {
            assert!(
                !tab.fallback_title.is_empty(),
                "Tab {} fallback_title should not be empty",
                i
            );
        }
    });
}

#[gpui::test]
fn test_get_tab_titles_returns_non_empty_strings(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    workspace.update(cx, |ws, cx| {
        for _ in 0..10 {
            ws.new_tab(cx);
        }
    });

    cx.update(|app| {
        workspace.update(app, |ws, cx| {
            let titles = ws.get_tab_titles(cx);
            for (i, title) in titles.iter().enumerate() {
                assert!(!title.is_empty(), "Title {} should not be empty", i);
            }
        });
    });
}

#[gpui::test]
fn test_active_tab_always_valid_after_close_first(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    workspace.update(cx, |ws, cx| {
        ws.new_tab(cx);
        ws.new_tab(cx);
        ws.new_tab(cx);
    });

    for _ in 0..3 {
        workspace.update(cx, |ws, cx| {
            ws.close_tab(0, cx);
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            assert!(
                ws.active_tab < ws.tabs.len(),
                "active_tab {} must be < tabs.len() {}",
                ws.active_tab,
                ws.tabs.len()
            );
        });
    }
}

#[gpui::test]
fn test_active_tab_always_valid_after_close_last(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    workspace.update(cx, |ws, cx| {
        ws.new_tab(cx);
        ws.new_tab(cx);
        ws.new_tab(cx);
    });

    for _ in 0..3 {
        workspace.update(cx, |ws, cx| {
            let last = ws.tabs.len() - 1;
            ws.close_tab(last, cx);
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            assert!(
                ws.active_tab < ws.tabs.len(),
                "active_tab {} must be < tabs.len() {}",
                ws.active_tab,
                ws.tabs.len()
            );
        });
    }
}

#[gpui::test]
fn test_active_tab_adjusts_when_closing_active(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    workspace.update(cx, |ws, cx| {
        ws.new_tab(cx);
        ws.new_tab(cx);
        ws.switch_tab(1, cx);
    });

    workspace.update(cx, |ws, cx| {
        ws.close_tab(1, cx);
    });

    cx.read(|app| {
        let ws = workspace.read(app);
        assert!(ws.active_tab < ws.tabs.len());
    });
}

#[gpui::test]
fn test_pending_close_tab_index_0(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    workspace.update(cx, |ws, _| {
        ws.pending_action = Some(PendingAction::CloseTab(0));
    });

    cx.read(|app| {
        assert_eq!(
            workspace.read(app).pending_action,
            Some(PendingAction::CloseTab(0))
        );
    });
}

#[gpui::test]
fn test_pending_close_tab_index_max(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    workspace.update(cx, |ws, _| {
        ws.pending_action = Some(PendingAction::CloseTab(usize::MAX));
    });

    cx.read(|app| {
        assert_eq!(
            workspace.read(app).pending_action,
            Some(PendingAction::CloseTab(usize::MAX))
        );
    });
}

#[gpui::test]
fn test_cached_titles_with_single_tab(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    cx.update(|app| {
        workspace.update(app, |ws, cx| {
            let titles = ws.get_tab_titles(cx);
            assert_eq!(titles.len(), 1);
            assert_eq!(ws.cached_titles.len(), 1);
        });
    });
}

#[gpui::test]
fn test_cached_titles_with_many_tabs(cx: &mut TestAppContext) {
    init_test_context(cx);
    let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

    workspace.update(cx, |ws, cx| {
        for _ in 0..49 {
            ws.new_tab(cx);
        }
    });

    cx.update(|app| {
        workspace.update(app, |ws, cx| {
            let titles = ws.get_tab_titles(cx);
            assert_eq!(titles.len(), 50);
            assert_eq!(ws.cached_titles.len(), 50);
        });
    });
}

#[gpui::test]
fn test_tab_count_boundary_matrix(cx: &mut TestAppContext) {
    let test_counts = [1, 2, 3, 5, 10, 50];

    for &target_count in &test_counts {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        workspace.update(cx, |ws, cx| {
            for _ in 1..target_count {
                ws.new_tab(cx);
            }
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            assert_eq!(
                ws.tabs.len(),
                target_count,
                "Should have {} tabs",
                target_count
            );
        });

        workspace.update(cx, |ws, cx| {
            ws.next_tab(cx);
            assert!(ws.active_tab < ws.tabs.len());

            ws.prev_tab(cx);
            assert!(ws.active_tab < ws.tabs.len());

            ws.switch_tab(0, cx);
            assert_eq!(ws.active_tab, 0);

            ws.switch_tab(target_count - 1, cx);
            assert_eq!(ws.active_tab, target_count - 1);

            let before = ws.active_tab;
            ws.switch_tab(target_count, cx);
            assert_eq!(ws.active_tab, before);
        });
    }
}
